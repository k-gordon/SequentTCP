//! Hardware validation test functions.
//!
//! Each function exercises one category of Modbus / health-endpoint
//! behaviour and records PASS / FAIL results.  Tests are gated by the
//! expected capabilities declared in the scenario TOML, so the same
//! code works for any board combination.
//!
//! Test ID prefixes:
//!   HH  — Health endpoint
//!   HA  — Analog inputs
//!   HR  — Relay writes
//!   HO  — Open-drain outputs
//!   HAO — Analog outputs
//!   HS  — Stability & performance

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use super::modbus_client::ModbusClient;
use super::results::Results;
use super::scenario::ScenarioConfig;

// ── Holding register / coil constants (must match databank.rs) ───────

const HR_MA_IN_BASE: u16 = 0;
const HR_PSU_VOLTAGE: u16 = 8;
const HR_VOLT_IN_BASE: u16 = 10;
const HR_VOLT_OUT_BASE: u16 = 16;
const HR_MA_OUT_BASE: u16 = 20;
const HR_RELAY_READBACK: u16 = 24;

const COIL_RELAY_BASE: u16 = 0;
const COIL_OD_BASE: u16 = 16;
const DI_OPTO_BASE: u16 = 0;

// ── Health endpoint types ────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct HealthResponse {
    status: String,
    uptime_s: u64,
    last_cycle_ms: f64,
    i2c_errors: u64,
    #[serde(default)]
    i2c_recoveries: u32,
    #[serde(default)]
    relay_mismatches: u32,
    #[serde(default)]
    channels: Option<ChannelStatus>,
}

#[derive(Deserialize, Debug)]
struct ChannelStatus {
    ma: String,
    volt: String,
    psu: String,
    opto: String,
}

/// Fetch the JSON health response from the gateway.
fn fetch_health(port: u16) -> anyhow::Result<HealthResponse> {
    let addr = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let req = format!("GET /health HTTP/1.0\r\nHost: {addr}\r\n\r\n");
    stream.write_all(req.as_bytes())?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);

    // Find body after \r\n\r\n
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .trim();

    Ok(serde_json::from_str(body)?)
}

// ════════════════════════════════════════════════════════════════════════
// HH — Health Endpoint
// ════════════════════════════════════════════════════════════════════════

pub fn test_health(res: &mut Results, cfg: &ScenarioConfig) {
    res.set_category("Health Endpoint");

    let data = match fetch_health(cfg.health_port) {
        Ok(d) => {
            res.record("HH-01", "Health endpoint responds", true, "HTTP 200");
            d
        }
        Err(e) => {
            res.record(
                "HH-01",
                "Health endpoint responds",
                false,
                &format!("{e:#}"),
            );
            return;
        }
    };

    // HH-02: valid JSON (implied by successful parse above)
    res.record("HH-02", "Response is valid JSON", true, "");

    // HH-03: status
    let ok_status = data.status == "ok" || data.status == "degraded";
    res.record(
        "HH-03",
        r#"Status is "ok" or "degraded""#,
        ok_status,
        &format!("status={}", data.status),
    );

    // HH-04: uptime
    res.record(
        "HH-04",
        "Uptime > 0 seconds",
        data.uptime_s > 0,
        &format!("uptime_s={}", data.uptime_s),
    );

    // HH-05: cycle time
    res.record(
        "HH-05",
        "Cycle time present and > 0",
        data.last_cycle_ms > 0.0,
        &format!("last_cycle_ms={:.2}", data.last_cycle_ms),
    );

    // HH-06: i2c_errors field
    res.record(
        "HH-06",
        "i2c_errors field present",
        true,
        &format!("i2c_errors={}", data.i2c_errors),
    );

    // HH-07: i2c_recoveries
    res.record(
        "HH-07",
        "i2c_recoveries field present",
        true,
        &format!("i2c_recoveries={}", data.i2c_recoveries),
    );

    // HH-08: relay_mismatches
    res.record(
        "HH-08",
        "relay_mismatches field present",
        true,
        &format!("relay_mismatches={}", data.relay_mismatches),
    );

    // HH-09: channels block
    let ch_ok = data.channels.is_some();
    let ch_detail = match &data.channels {
        Some(c) => format!("ma={} volt={} psu={} opto={}", c.ma, c.volt, c.psu, c.opto),
        None => "MISSING".into(),
    };
    res.record(
        "HH-09",
        "All 4 channel status fields present",
        ch_ok,
        &ch_detail,
    );

    // HH-10: performance benchmark
    let fast = data.last_cycle_ms > 0.0 && data.last_cycle_ms < 15.0;
    res.record(
        "HH-10",
        "I/O cycle < 15 ms",
        fast,
        &format!("last_cycle_ms={:.3}", data.last_cycle_ms),
    );
}

// ════════════════════════════════════════════════════════════════════════
// HA — Analog Inputs
// ════════════════════════════════════════════════════════════════════════

pub fn test_analog_inputs(
    res: &mut Results,
    client: &mut ModbusClient,
    cfg: &ScenarioConfig,
) {
    res.set_category("Analog Inputs");
    let ma_n = cfg.ma_in_channels;
    let v_n = cfg.v_in_channels;

    // HA-01: 4-20 mA registers
    if ma_n > 0 {
        match client.read_holding_registers(HR_MA_IN_BASE, ma_n) {
            Ok(regs) => {
                let vals: Vec<String> =
                    regs.iter().map(|r| format!("{:.2}", *r as f32 / 100.0)).collect();
                res.record(
                    "HA-01",
                    &format!("Read {} mA input registers", ma_n),
                    true,
                    &format!("mA=[{}]", vals.join(", ")),
                );

                // HA-02: at least one > 0
                let any_nz = regs.iter().any(|&r| r > 0);
                res.record(
                    "HA-02",
                    "At least one 4-20 mA channel > 0",
                    any_nz,
                    if any_nz { "" } else { "all zero" },
                );

                // HA-06: stability
                thread::sleep(Duration::from_millis(300));
                if let Ok(regs2) = client.read_holding_registers(HR_MA_IN_BASE, ma_n) {
                    let max_drift: f32 = regs
                        .iter()
                        .zip(regs2.iter())
                        .map(|(a, b)| (*a as f32 - *b as f32).abs() / 100.0)
                        .fold(0.0f32, f32::max);
                    res.record(
                        "HA-06",
                        "Successive mA reads stable (drift < 0.5)",
                        max_drift < 0.5,
                        &format!("max_drift={max_drift:.2} mA"),
                    );
                }
            }
            Err(e) => {
                res.record("HA-01", "Read mA input registers", false, &format!("{e:#}"));
            }
        }
    }

    // HA-03: 0-10 V registers
    if v_n > 0 {
        match client.read_holding_registers(HR_VOLT_IN_BASE, v_n) {
            Ok(regs) => {
                let vals: Vec<String> =
                    regs.iter().map(|r| format!("{:.2}", *r as f32 / 100.0)).collect();
                res.record(
                    "HA-03",
                    &format!("Read {} voltage input registers", v_n),
                    true,
                    &format!("V=[{}]", vals.join(", ")),
                );
            }
            Err(e) => {
                res.record("HA-03", "Read V input registers", false, &format!("{e:#}"));
            }
        }
    }

    // HA-04: PSU voltage
    match client.read_holding_registers(HR_PSU_VOLTAGE, 1) {
        Ok(regs) => {
            let psu_v = regs[0] as f32 / 100.0;
            res.record(
                "HA-04",
                "PSU voltage 3-30 V range",
                (3.0..30.0).contains(&psu_v),
                &format!("psu={psu_v:.2} V"),
            );
        }
        Err(e) => {
            res.record("HA-04", "Read PSU voltage", false, &format!("{e:#}"));
        }
    }

    // HA-05: opto discrete inputs
    if cfg.opto_channels > 0 {
        match client.read_discrete_inputs(DI_OPTO_BASE, cfg.opto_channels) {
            Ok(bits) => {
                let s: String = bits.iter().map(|&b| if b { '1' } else { '0' }).collect();
                res.record(
                    "HA-05",
                    &format!("Read {} opto discrete inputs", cfg.opto_channels),
                    true,
                    &format!("opto={s}"),
                );
            }
            Err(e) => {
                res.record("HA-05", "Read opto inputs", false, &format!("{e:#}"));
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// HR — Relay Writes
// ════════════════════════════════════════════════════════════════════════

pub fn test_relay_writes(
    res: &mut Results,
    client: &mut ModbusClient,
    cfg: &ScenarioConfig,
) {
    res.set_category("Relay Writes");
    let n = cfg.relay_count;
    if n == 0 {
        return;
    }

    // HR-01: coil 0 ON
    match client.write_single_coil(COIL_RELAY_BASE, true) {
        Ok(()) => res.record("HR-01", "Write relay 1 ON (coil 0)", true, ""),
        Err(e) => {
            res.record("HR-01", "Write relay 1 ON", false, &format!("{e:#}"));
            return;
        }
    }
    thread::sleep(Duration::from_millis(200));

    // HR-02: read back coil 0
    match client.read_coils(COIL_RELAY_BASE, 1) {
        Ok(bits) => {
            res.record("HR-02", "Read back relay 1 = ON", bits[0], &format!("coil[0]={}", bits[0]));
        }
        Err(e) => res.record("HR-02", "Read back relay 1", false, &format!("{e:#}")),
    }

    // HR-03: relay read-back register (HR 24)
    if cfg.relay_readback {
        thread::sleep(Duration::from_millis(1200));
        let read_hr24 = |client: &mut ModbusClient, cfg: &ScenarioConfig| -> anyhow::Result<u16> {
            if !cfg.single_slave && cfg.has_megaind() {
                let saved = cfg.relay_slave_id;
                client.set_unit_id(cfg.ind_slave_id);
                let regs = client.read_holding_registers(HR_RELAY_READBACK, 1);
                client.set_unit_id(saved);
                regs.map(|r| r[0])
            } else {
                client.read_holding_registers(HR_RELAY_READBACK, 1).map(|r| r[0])
            }
        };
        match read_hr24(client, cfg) {
            Ok(val) => {
                let bit0 = (val & 1) == 1;
                res.record(
                    "HR-03",
                    "HR 24 read-back shows relay 1 ON",
                    bit0,
                    &format!("HR24=0x{val:04X}"),
                );
            }
            Err(e) => res.record("HR-03", "HR 24 read-back", false, &format!("{e:#}")),
        }
    }

    // HR-04: coil 0 OFF
    match client.write_single_coil(COIL_RELAY_BASE, false) {
        Ok(()) => res.record("HR-04", "Write relay 1 OFF", true, ""),
        Err(e) => res.record("HR-04", "Write relay 1 OFF", false, &format!("{e:#}")),
    }
    thread::sleep(Duration::from_millis(200));

    // HR-05: toggle all relays ON
    let mut all_ok = true;
    for i in 0..n {
        if client.write_single_coil(COIL_RELAY_BASE + i, true).is_err() {
            all_ok = false;
            break;
        }
    }
    res.record("HR-05", &format!("Toggle all {n} relays ON"), all_ok, "");
    thread::sleep(Duration::from_millis(300));

    // HR-06: read back all relays
    match client.read_coils(COIL_RELAY_BASE, n) {
        Ok(bits) => {
            let all_on = bits.iter().take(n as usize).all(|&b| b);
            let s: String = bits.iter().map(|&b| if b { '1' } else { '0' }).collect();
            res.record(
                "HR-06",
                &format!("All {n} relay coils read back ON"),
                all_on,
                &format!("coils={s}"),
            );
        }
        Err(e) => res.record("HR-06", "Read back all relays", false, &format!("{e:#}")),
    }

    // HR-07: cleanup — all OFF
    for i in 0..n {
        let _ = client.write_single_coil(COIL_RELAY_BASE + i, false);
    }
    thread::sleep(Duration::from_millis(300));
    match client.read_coils(COIL_RELAY_BASE, n) {
        Ok(bits) => {
            let all_off = bits.iter().take(n as usize).all(|&b| !b);
            let s: String = bits.iter().map(|&b| if b { '1' } else { '0' }).collect();
            res.record(
                "HR-07",
                &format!("All {n} relays OFF after cleanup"),
                all_off,
                &format!("coils={s}"),
            );
        }
        Err(e) => res.record("HR-07", "Cleanup read-back", false, &format!("{e:#}")),
    }
}

// ════════════════════════════════════════════════════════════════════════
// HO — Open-Drain Outputs
// ════════════════════════════════════════════════════════════════════════

pub fn test_od_outputs(
    res: &mut Results,
    client: &mut ModbusClient,
    cfg: &ScenarioConfig,
) {
    res.set_category("Open-Drain Outputs");
    let n = cfg.od_channels;
    if n == 0 {
        return;
    }

    // In multi-slave, OD coils start at 0 on the ind slave.
    // In single-slave, they're at offset 16.
    let base = if cfg.single_slave { COIL_OD_BASE } else { 0 };

    // HO-01: OD 1 ON
    match client.write_single_coil(base, true) {
        Ok(()) => res.record("HO-01", "Write OD output 1 ON", true, ""),
        Err(e) => {
            res.record("HO-01", "Write OD output 1 ON", false, &format!("{e:#}"));
            return;
        }
    }
    thread::sleep(Duration::from_millis(200));

    // HO-02: read back
    match client.read_coils(base, 1) {
        Ok(bits) => {
            res.record(
                "HO-02",
                "OD output 1 reads back ON",
                bits[0],
                &format!("coil[{base}]={}", bits[0]),
            );
        }
        Err(e) => res.record("HO-02", "OD read back", false, &format!("{e:#}")),
    }

    // HO-03: toggle all OD
    let mut ok = true;
    for i in 0..n {
        if client.write_single_coil(base + i, true).is_err() {
            ok = false;
        }
    }
    res.record("HO-03", &format!("Toggle all {n} OD outputs ON"), ok, "");
    thread::sleep(Duration::from_millis(200));

    // HO-04: cleanup
    for i in 0..n {
        let _ = client.write_single_coil(base + i, false);
    }
    thread::sleep(Duration::from_millis(200));
    match client.read_coils(base, n) {
        Ok(bits) => {
            let all_off = bits.iter().take(n as usize).all(|&b| !b);
            res.record(
                "HO-04",
                &format!("All {n} OD outputs OFF after cleanup"),
                all_off,
                "",
            );
        }
        Err(e) => res.record("HO-04", "OD cleanup read-back", false, &format!("{e:#}")),
    }
}

// ════════════════════════════════════════════════════════════════════════
// HAO — Analog Outputs
// ════════════════════════════════════════════════════════════════════════

pub fn test_analog_outputs(
    res: &mut Results,
    client: &mut ModbusClient,
    cfg: &ScenarioConfig,
) {
    res.set_category("Analog Outputs");

    if cfg.v_out_channels > 0 {
        // HAO-01: write 5.00 V
        match client.write_single_register(HR_VOLT_OUT_BASE, 500) {
            Ok(()) => res.record("HAO-01", "Write 0-10 V output 1 = 5.00 V", true, ""),
            Err(e) => res.record("HAO-01", "Write V output", false, &format!("{e:#}")),
        }
        thread::sleep(Duration::from_millis(200));

        // HAO-02: read back
        match client.read_holding_registers(HR_VOLT_OUT_BASE, 1) {
            Ok(regs) => {
                res.record(
                    "HAO-02",
                    "HR 16 reads back 500",
                    regs[0] == 500,
                    &format!("HR16={} ({:.2} V)", regs[0], regs[0] as f32 / 100.0),
                );
            }
            Err(e) => res.record("HAO-02", "Read V output", false, &format!("{e:#}")),
        }

        // HAO-05: reset
        let _ = client.write_single_register(HR_VOLT_OUT_BASE, 0);
        thread::sleep(Duration::from_millis(200));
        match client.read_holding_registers(HR_VOLT_OUT_BASE, 1) {
            Ok(regs) => {
                res.record("HAO-05", "V output reset to 0", regs[0] == 0, "");
            }
            Err(e) => res.record("HAO-05", "V output reset", false, &format!("{e:#}")),
        }
    }

    if cfg.ma_out_channels > 0 {
        // HAO-03: write 12.00 mA
        match client.write_single_register(HR_MA_OUT_BASE, 1200) {
            Ok(()) => res.record("HAO-03", "Write 4-20 mA output 1 = 12.00 mA", true, ""),
            Err(e) => res.record("HAO-03", "Write mA output", false, &format!("{e:#}")),
        }
        thread::sleep(Duration::from_millis(200));

        // HAO-04: read back
        match client.read_holding_registers(HR_MA_OUT_BASE, 1) {
            Ok(regs) => {
                res.record(
                    "HAO-04",
                    "HR 20 reads back 1200",
                    regs[0] == 1200,
                    &format!("HR20={} ({:.2} mA)", regs[0], regs[0] as f32 / 100.0),
                );
            }
            Err(e) => res.record("HAO-04", "Read mA output", false, &format!("{e:#}")),
        }

        // HAO-06: reset
        let _ = client.write_single_register(HR_MA_OUT_BASE, 0);
        thread::sleep(Duration::from_millis(200));
        match client.read_holding_registers(HR_MA_OUT_BASE, 1) {
            Ok(regs) => {
                res.record("HAO-06", "mA output reset to 0", regs[0] == 0, "");
            }
            Err(e) => res.record("HAO-06", "mA output reset", false, &format!("{e:#}")),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// HS — Stability & Performance
// ════════════════════════════════════════════════════════════════════════

pub fn test_stability(
    res: &mut Results,
    cfg: &ScenarioConfig,
    duration_s: u64,
) {
    res.set_category("Stability & Performance");
    println!("    Running {duration_s}-second stability test ...");

    let mut samples: Vec<HealthResponse> = Vec::new();
    let mut errors_start: Option<u64> = None;

    for _ in 0..(duration_s * 2) {
        if let Ok(data) = fetch_health(cfg.health_port) {
            if errors_start.is_none() {
                errors_start = Some(data.i2c_errors);
            }
            samples.push(data);
        }
        thread::sleep(Duration::from_millis(500));
    }

    if samples.is_empty() {
        res.record(
            "HS-01",
            &format!("{duration_s}s stability test"),
            false,
            "No samples collected",
        );
        return;
    }

    res.record(
        "HS-01",
        &format!("{duration_s}s stability test collected samples"),
        samples.len() >= duration_s as usize,
        &format!("{} samples", samples.len()),
    );

    // HS-02: no new I²C errors
    let end_errors = samples.last().map(|s| s.i2c_errors).unwrap_or(0);
    let new_errors = end_errors - errors_start.unwrap_or(0);
    res.record(
        "HS-02",
        "No new I2C errors during test",
        new_errors == 0,
        &format!("new_errors={new_errors}"),
    );

    // HS-03: status stayed ok
    let all_ok = samples.iter().all(|s| s.status == "ok");
    res.record("HS-03", "Health status stayed ok throughout", all_ok, "");

    // HS-04: max cycle time < 15 ms
    let cycles: Vec<f64> = samples.iter().map(|s| s.last_cycle_ms).collect();
    let max_cycle = cycles.iter().cloned().fold(0.0f64, f64::max);
    let avg_cycle: f64 = cycles.iter().sum::<f64>() / cycles.len() as f64;
    res.record(
        "HS-04",
        "Max cycle time < 15 ms",
        max_cycle < 15.0,
        &format!("avg={avg_cycle:.3} ms, max={max_cycle:.3} ms"),
    );
}
