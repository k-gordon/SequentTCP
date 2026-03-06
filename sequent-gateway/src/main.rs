mod board_def;
mod cache;
mod cli;
mod databank;
mod hal;
mod i2c_recovery;
mod modbus;
mod registers;
mod slave_map;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tracing::{debug, error, info};

use board_def::BoardDef;
use cache::OutputCache;
use cli::Cli;
use databank::DataBank;
use hal::megaind::MegaIndBoard;
use hal::relay16::RelayBoard;
use i2c_recovery::I2cWatchdog;
use registers::{I4_20_IN_CHANNELS, OD_CHANNELS, OPTO_CHANNELS, RELAY16_CHANNELS, U0_10_IN_CHANNELS};
use slave_map::SlaveMap;

/// I²C bus device path (standard on Raspberry Pi).
const I2C_BUS: &str = "/dev/i2c-1";

/// Poll loop interval — 10 Hz.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// ════════════════════════════════════════════════════════════════════════
// Entry point
// ════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    // ── Logging ──────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sequent_gateway=info".into()),
        )
        .init();

    // ── Root check ───────────────────────────────────────────────────
    #[cfg(unix)]
    {
        if unsafe { libc::geteuid() } != 0 {
            tracing::warn!("Not running as root — I²C access may fail");
        }
    }

    // ── Board definitions ────────────────────────────────────────────
    let megaind_def = BoardDef::load_or_default(
        &args.boards_dir.join("megaind.toml"),
        BoardDef::default_megaind(),
    );
    let relay16_def = BoardDef::load_or_default(
        &args.boards_dir.join("relay16.toml"),
        BoardDef::default_relay16(),
    );

    info!(
        "Sequent Gateway v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    info!(
        "Industrial HAT: stack {} → I²C 0x{:02X} ({})",
        args.ind_stack,
        megaind_def.address.resolve(args.ind_stack),
        megaind_def.board.name
    );
    info!(
        "16-Relay HAT:   stack {} → I²C 0x{:02X} ({})",
        args.relay_stack,
        relay16_def.address.resolve(args.relay_stack),
        relay16_def.board.name
    );
    if args.map_opto_to_reg {
        info!("Opto-inputs also mapped to Holding Register 15");
    }

    // ── Slave addressing ─────────────────────────────────────────────
    let slave_map = Arc::new(SlaveMap::new(
        args.relay_slave_id,
        args.ind_slave_id,
        args.single_slave,
    ));

    if args.single_slave {
        info!(
            "Modbus addressing: SINGLE-SLAVE (flat map on any Unit ID)"
        );
    } else {
        info!(
            "Modbus addressing: MULTI-SLAVE — Relay HAT = Unit {}, Industrial HAT = Unit {}",
            args.relay_slave_id, args.ind_slave_id
        );
        if args.relay_slave_id == args.ind_slave_id {
            tracing::warn!(
                "relay-slave-id and ind-slave-id are both {}; only one board will be reachable",
                args.relay_slave_id
            );
        }
    }

    // ── Shared state ─────────────────────────────────────────────────
    let data_bank = Arc::new(RwLock::new(DataBank::new()));
    let running = Arc::new(AtomicBool::new(true));

    // ── I²C poll loop (dedicated OS thread — blocking I/O) ──────────
    let poll_handle = {
        let db = data_bank.clone();
        let run = running.clone();
        let ind_stack = args.ind_stack;
        let relay_stack = args.relay_stack;
        let map_opto = args.map_opto_to_reg;
        let log_interval = args.log_interval;
        let i2c_reset_threshold = args.i2c_reset_threshold;

        std::thread::Builder::new()
            .name("i2c-poll".into())
            .spawn(move || {
                poll_loop(
                    db,
                    run,
                    ind_stack,
                    relay_stack,
                    map_opto,
                    log_interval,
                    megaind_def,
                    relay16_def,
                    i2c_reset_threshold,
                );
            })?
    };

    // ── Modbus TCP server (async) ────────────────────────────────────
    tokio::select! {
        result = modbus::serve(&args.host, args.port, data_bank.clone(), slave_map.clone()) => {
            if let Err(e) = result {
                error!("Modbus server error: {e:#}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    // ── Shutdown ─────────────────────────────────────────────────────
    info!("Shutting down...");
    running.store(false, Ordering::Relaxed);
    poll_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Poll thread panicked"))?;
    info!("Goodbye.");

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// I²C Poll Loop
// ════════════════════════════════════════════════════════════════════════

/// Blocking poll loop running in a dedicated OS thread.
///
/// Each 100 ms tick:
///   1. Read all hardware inputs via I²C
///   2. Update the shared Modbus data bank
///   3. Apply coil writes to hardware (with state caching)
///   4. Log a heartbeat summary every `log_interval` seconds
fn poll_loop(
    data_bank: Arc<RwLock<DataBank>>,
    running: Arc<AtomicBool>,
    ind_stack: u8,
    relay_stack: u8,
    map_opto: bool,
    log_interval: u64,
    megaind_def: BoardDef,
    relay16_def: BoardDef,
    i2c_reset_threshold: u32,
) {
    // ── Initialise hardware ──────────────────────────────────────────
    let mut ind_board = match MegaIndBoard::new(I2C_BUS, ind_stack, &megaind_def) {
        Ok(mut b) => {
            if let Ok((major, minor)) = b.read_firmware_version() {
                info!("Industrial HAT firmware: v{major:02}.{minor:02}");
            }
            Some(b)
        }
        Err(e) => {
            error!("Failed to open Industrial HAT: {e:#}");
            None
        }
    };

    let mut rel_board = match RelayBoard::new(I2C_BUS, relay_stack, &relay16_def) {
        Ok(b) => Some(b),
        Err(e) => {
            error!("Failed to open 16-Relay HAT: {e:#}");
            None
        }
    };

    let mut cache = OutputCache::new();
    let mut watchdog = I2cWatchdog::new(i2c_reset_threshold);
    let mut last_heartbeat = Instant::now();
    let heartbeat_duration = Duration::from_secs(log_interval);

    if i2c_reset_threshold > 0 {
        info!("I²C bus recovery enabled (threshold: {i2c_reset_threshold} consecutive failures)");
    }

    info!("I²C poll loop started ({}Hz)", 1000 / POLL_INTERVAL.as_millis());

    // ── Main loop ────────────────────────────────────────────────────
    while running.load(Ordering::Relaxed) {
        let cycle_start = Instant::now();

        // 1. READ HARDWARE ────────────────────────────────────────────
        let ma_inputs = ind_board
            .as_mut()
            .and_then(|b| match b.read_4_20ma_inputs() {
                Ok(v) => { watchdog.record_success(); Some(v) }
                Err(_) => { if watchdog.record_failure() { /* handled below */ } None }
            })
            .unwrap_or([0.0; I4_20_IN_CHANNELS]);

        let v_inputs = ind_board
            .as_mut()
            .and_then(|b| match b.read_0_10v_inputs() {
                Ok(v) => { watchdog.record_success(); Some(v) }
                Err(_) => { if watchdog.record_failure() { /* handled below */ } None }
            })
            .unwrap_or([0.0; U0_10_IN_CHANNELS]);

        let voltage = ind_board
            .as_mut()
            .and_then(|b| match b.read_system_voltage() {
                Ok(v) => { watchdog.record_success(); Some(v) }
                Err(_) => { if watchdog.record_failure() { /* handled below */ } None }
            })
            .unwrap_or(0.0);

        let (opto_val, opto_bits) = ind_board
            .as_mut()
            .and_then(|b| match b.read_opto_inputs() {
                Ok(v) => { watchdog.record_success(); Some(v) }
                Err(_) => { if watchdog.record_failure() { /* handled below */ } None }
            })
            .unwrap_or((0, [false; OPTO_CHANNELS]));

        // ── I²C bus recovery check ──────────────────────────────────
        if watchdog.consecutive_failures() >= i2c_reset_threshold && i2c_reset_threshold > 0 {
            if watchdog.attempt_recovery() {
                // Re-open I²C device file descriptors
                ind_board = MegaIndBoard::new(I2C_BUS, ind_stack, &megaind_def).ok();
                rel_board = RelayBoard::new(I2C_BUS, relay_stack, &relay16_def).ok();
                cache = OutputCache::new(); // force re-sync all outputs
                continue; // skip rest of this cycle
            }
        }

        // 2. UPDATE MODBUS DATA BANK ──────────────────────────────────
        {
            let mut db = data_bank.write().unwrap();

            // 4-20mA → holding registers 0–7 (mA × 100)
            for (i, &ma) in ma_inputs.iter().enumerate() {
                db.holding_registers[i] = (ma * 100.0) as u16;
            }

            // PSU voltage → holding register 8 (V × 100)
            db.holding_registers[8] = (voltage * 100.0) as u16;

            // 0-10V → holding registers 10–13 (V × 100)
            for (i, &v) in v_inputs.iter().enumerate() {
                db.holding_registers[10 + i] = (v * 100.0) as u16;
            }

            // Opto bitmask → holding register 15 (optional)
            if map_opto {
                db.holding_registers[15] = opto_val as u16;
            }

            // Opto bits → discrete inputs 0–7
            db.discrete_inputs[..OPTO_CHANNELS].copy_from_slice(&opto_bits);
        }

        // 3. APPLY OUTPUTS ────────────────────────────────────────────
        let coils = {
            let db = data_bank.read().unwrap();
            db.coils
        };

        // Relays 1–16 (coils 0–15)
        if let Some(ref mut board) = rel_board {
            for i in 0..RELAY16_CHANNELS {
                if cache.should_update_relay(i, coils[i]) {
                    let ch = (i + 1) as u8;
                    match board.set_relay(ch, coils[i]) {
                        Ok(()) => {
                            cache.confirm_relay(i, coils[i]);
                            info!(
                                "Relay {} → {}",
                                ch,
                                if coils[i] { "ON" } else { "OFF" }
                            );
                        }
                        Err(e) => {
                            cache.invalidate_relay(i);
                            error!("Relay {} write failed: {e:#}", ch);
                        }
                    }
                }
            }
        }

        // OD outputs 1–4 (coils 16–19)
        if let Some(ref mut board) = ind_board {
            for i in 0..OD_CHANNELS {
                if cache.should_update_od(i, coils[16 + i]) {
                    let ch = (i + 1) as u8;
                    match board.set_od_output(ch, coils[16 + i]) {
                        Ok(()) => {
                            cache.confirm_od(i, coils[16 + i]);
                            info!(
                                "OD output {} → {}",
                                ch,
                                if coils[16 + i] { "ON" } else { "OFF" }
                            );
                        }
                        Err(e) => {
                            cache.invalidate_od(i);
                            error!("OD output {} write failed: {e:#}", ch);
                        }
                    }
                }
            }
        }

        // 4. HEARTBEAT ────────────────────────────────────────────────
        if last_heartbeat.elapsed() >= heartbeat_duration {
            log_heartbeat(&ma_inputs, &v_inputs, voltage, &opto_bits, &coils);
            last_heartbeat = Instant::now();
        }

        // 5. SLEEP FOR REMAINDER OF CYCLE ─────────────────────────────
        let elapsed = cycle_start.elapsed();
        debug!("I/O cycle: {:.2}ms", elapsed.as_secs_f64() * 1000.0);
        if elapsed < POLL_INTERVAL {
            std::thread::sleep(POLL_INTERVAL - elapsed);
        }
    }

    info!("I²C poll loop stopped");
}

// ════════════════════════════════════════════════════════════════════════
// Heartbeat
// ════════════════════════════════════════════════════════════════════════

/// Log a full system heartbeat matching the Python gateway format.
fn log_heartbeat(
    ma_inputs: &[f32; I4_20_IN_CHANNELS],
    v_inputs: &[f32; U0_10_IN_CHANNELS],
    voltage: f32,
    opto_bits: &[bool; OPTO_CHANNELS],
    coils: &[bool],
) {
    let ma_str: String = ma_inputs
        .iter()
        .map(|v| format!("{v:4.1}"))
        .collect::<Vec<_>>()
        .join(" ");

    let v_str: String = v_inputs
        .iter()
        .map(|v| format!("{v:4.1}"))
        .collect::<Vec<_>>()
        .join(" ");

    let opto_str: String = opto_bits
        .iter()
        .rev()
        .map(|&b| if b { '1' } else { '0' })
        .collect();

    let relay_str: String = (0..RELAY16_CHANNELS)
        .map(|i| {
            if coils.get(i).copied().unwrap_or(false) {
                '1'
            } else {
                '0'
            }
        })
        .collect();

    let od_str: String = (0..OD_CHANNELS)
        .map(|i| {
            if coils.get(16 + i).copied().unwrap_or(false) {
                '1'
            } else {
                '0'
            }
        })
        .collect();

    info!("--- SYSTEM HEARTBEAT ---");
    info!("POWER: {voltage:.2}V");
    info!("4-20mA (1-8) : [{ma_str}] mA");
    info!("0-10V  (1-4) : [{v_str}] V");
    info!("OPTO INPUTS  : {opto_str} (Binary)");
    info!("RELAYS (1-16): {relay_str}");
    info!("OD OUT (1-4) : {od_str}");
    info!("------------------------");
}
