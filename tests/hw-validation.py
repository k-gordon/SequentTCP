#!/usr/bin/env python3
"""
Hardware Validation Suite for Sequent Gateway
==============================================

Runs against the LIVE gateway on a Raspberry Pi with real Sequent HATs.
Produces a structured PASS / FAIL report you can paste back to the developer.

Prerequisites:
    pip3 install pyModbusTCP

Usage:
    # Start the gateway first:
    sudo ./target/release/sequent-gateway --health-port 8080 --builtin-defaults

    # Then in another terminal (pyModbusTCP lives in ~/venv):
    ~/venv/bin/python3 tests/hw-validation.py

    # Or with custom ports:
    ~/venv/bin/python3 tests/hw-validation.py --modbus-port 502 --health-port 8080

    # Skip relay toggle tests (if relays control live equipment):
    ~/venv/bin/python3 tests/hw-validation.py --skip-writes

    # Only run a specific category:
    ~/venv/bin/python3 tests/hw-validation.py --only health
    ~/venv/bin/python3 tests/hw-validation.py --only analog
    ~/venv/bin/python3 tests/hw-validation.py --only relay

Copy the full output and paste it back — it contains everything needed
to check off Story 10 and Epic 2 acceptance criteria.
"""

import argparse
import json
import sys
import time
import urllib.request
import urllib.error
from datetime import datetime

try:
    from pyModbusTCP.client import ModbusClient
except ImportError:
    print("ERROR: pyModbusTCP not installed.  Run:  pip3 install pyModbusTCP")
    sys.exit(1)


# ── Configuration ─────────────────────────────────────────────────────

DEFAULT_MODBUS_HOST = "127.0.0.1"
DEFAULT_MODBUS_PORT = 502
DEFAULT_HEALTH_PORT = 8080

# Holding register layout (must match databank.rs)
HR_MA_IN_BASE  = 0    # 0-7:  4-20 mA inputs  (mA × 100)
HR_MA_IN_COUNT = 8
HR_PSU_VOLTAGE = 8    # 8:    PSU voltage      (V × 100)
HR_VOLT_IN_BASE  = 10 # 10-13: 0-10 V inputs   (V × 100)
HR_VOLT_IN_COUNT = 4
HR_OPTO_BITMASK  = 15 # 15:   opto bitmask (if --map-opto-to-reg)
HR_VOLT_OUT_BASE = 16 # 16-19: 0-10 V outputs  (V × 100)
HR_VOLT_OUT_COUNT = 4
HR_MA_OUT_BASE   = 20 # 20-23: 4-20 mA outputs (mA × 100)
HR_MA_OUT_COUNT  = 4
HR_RELAY_READBACK = 24 # 24:   relay read-back bitmask

# Coil layout
COIL_RELAY_BASE = 0    # 0-15:  relays
COIL_RELAY_COUNT = 16
COIL_OD_BASE    = 16   # 16-19: open-drain outputs
COIL_OD_COUNT   = 4

# Discrete inputs
DI_OPTO_BASE  = 0
DI_OPTO_COUNT = 8


# ── Result tracking ──────────────────────────────────────────────────

class Results:
    def __init__(self):
        self.tests = []
        self.category = ""

    def set_category(self, name):
        self.category = name

    def record(self, test_id, desc, passed, detail=""):
        status = "PASS" if passed else "FAIL"
        entry = {
            "id": test_id,
            "category": self.category,
            "desc": desc,
            "status": status,
            "detail": detail,
        }
        self.tests.append(entry)
        marker = "✅" if passed else "❌"
        line = f"  [{status}] {test_id}: {desc}"
        if detail:
            line += f"  ({detail})"
        print(f"  {marker} {line.strip()}")

    def summary(self):
        total = len(self.tests)
        passed = sum(1 for t in self.tests if t["status"] == "PASS")
        failed = total - passed
        return total, passed, failed

    def report(self):
        """Produce the final paste-friendly report."""
        total, passed, failed = self.summary()
        lines = []
        lines.append("")
        lines.append("=" * 60)
        lines.append("  HARDWARE VALIDATION REPORT")
        lines.append(f"  Date: {datetime.now().isoformat(timespec='seconds')}")
        lines.append(f"  Result: {passed}/{total} passed, {failed} failed")
        lines.append("=" * 60)

        current_cat = None
        for t in self.tests:
            if t["category"] != current_cat:
                current_cat = t["category"]
                lines.append(f"\n  --- {current_cat} ---")
            status = "PASS" if t["status"] == "PASS" else "FAIL"
            line = f"  [{status}] {t['id']}: {t['desc']}"
            if t["detail"]:
                line += f"  ({t['detail']})"
            lines.append(line)

        lines.append("")
        lines.append(f"  TOTAL: {passed}/{total} passed")
        if failed > 0:
            lines.append(f"  FAILED: {', '.join(t['id'] for t in self.tests if t['status'] == 'FAIL')}")
        lines.append("=" * 60)
        return "\n".join(lines)


# ── Test categories ───────────────────────────────────────────────────

def test_health(results, health_url):
    """HW-01 through HW-08: Health endpoint checks."""
    results.set_category("Health Endpoint")

    # HW-01: endpoint responds
    try:
        resp = urllib.request.urlopen(health_url, timeout=5)
        body = resp.read().decode()
        results.record("HW-01", "Health endpoint responds (200 OK)", resp.status == 200,
                        f"HTTP {resp.status}")
    except Exception as e:
        results.record("HW-01", "Health endpoint responds (200 OK)", False, str(e))
        return None  # Can't continue health tests

    # HW-02: valid JSON
    try:
        data = json.loads(body)
        results.record("HW-02", "Response is valid JSON", True)
    except json.JSONDecodeError as e:
        results.record("HW-02", "Response is valid JSON", False, str(e))
        return None

    # HW-03: status field exists and is ok or degraded
    status = data.get("status", "MISSING")
    results.record("HW-03", 'Status field is "ok" or "degraded"',
                    status in ("ok", "degraded"), f'status="{status}"')

    # HW-04: uptime > 0
    uptime = data.get("uptime_s", -1)
    results.record("HW-04", "Uptime > 0 seconds", uptime > 0, f"uptime_s={uptime}")

    # HW-05: cycle time present and reasonable
    cycle = data.get("last_cycle_ms", -1)
    results.record("HW-05", "Cycle time < 10 ms", 0 < cycle < 10,
                    f"last_cycle_ms={cycle:.2f}")

    # HW-06: i2c_errors field present
    errors = data.get("i2c_errors", "MISSING")
    results.record("HW-06", "i2c_errors field present", errors != "MISSING",
                    f"i2c_errors={errors}")

    # HW-07: i2c_recoveries field present (SEQGW-21)
    recoveries = data.get("i2c_recoveries", "MISSING")
    results.record("HW-07", "i2c_recoveries field present (SEQGW-21)",
                    recoveries != "MISSING", f"i2c_recoveries={recoveries}")

    # HW-08: relay_mismatches field present (SEQGW-23)
    mismatches = data.get("relay_mismatches", "MISSING")
    results.record("HW-08", "relay_mismatches field present (SEQGW-23)",
                    mismatches != "MISSING", f"relay_mismatches={mismatches}")

    # HW-09: channels block present with all 4 channels
    channels = data.get("channels", {})
    all_present = all(k in channels for k in ("ma", "volt", "psu", "opto"))
    results.record("HW-09", "All 4 channel status fields present",
                    all_present, f"channels={json.dumps(channels)}")

    # HW-10: benchmark — cycle time < 1 ms (Story 10 AC)
    results.record("HW-10", "I/O cycle < 1.0 ms (Story 10 benchmark)",
                    0 < cycle < 1.0, f"last_cycle_ms={cycle:.3f}")

    return data


def test_analog_inputs(results, client):
    """HW-11 through HW-16: Analog input channel reads."""
    results.set_category("Analog Inputs (Story 10)")

    # HW-11: Read 4-20 mA registers (HR 0-7)
    ma_regs = client.read_holding_registers(HR_MA_IN_BASE, HR_MA_IN_COUNT)
    if ma_regs is None:
        results.record("HW-11", "Read 4-20 mA registers (HR 0-7)", False, "Modbus read failed")
        return

    ma_values = [r / 100.0 for r in ma_regs]
    results.record("HW-11", "Read 4-20 mA registers (HR 0-7)", True,
                    f"mA={[f'{v:.2f}' for v in ma_values]}")

    # HW-12: At least one 4-20 mA channel reads > 0 (proves I²C working)
    any_nonzero = any(v > 0 for v in ma_values)
    results.record("HW-12", "At least one 4-20 mA channel > 0 mA",
                    any_nonzero,
                    "All zero — check wiring or confirm no input signal is expected")

    # HW-13: Read 0-10 V registers (HR 10-13)
    v_regs = client.read_holding_registers(HR_VOLT_IN_BASE, HR_VOLT_IN_COUNT)
    if v_regs is None:
        results.record("HW-13", "Read 0-10 V registers (HR 10-13)", False, "Modbus read failed")
        return

    v_values = [r / 100.0 for r in v_regs]
    results.record("HW-13", "Read 0-10 V registers (HR 10-13)", True,
                    f"V={[f'{v:.2f}' for v in v_values]}")

    # HW-14: PSU voltage (HR 8) — should be 5V or 24V range
    psu_regs = client.read_holding_registers(HR_PSU_VOLTAGE, 1)
    if psu_regs is None:
        results.record("HW-14", "Read PSU voltage (HR 8)", False, "Modbus read failed")
        return

    psu_v = psu_regs[0] / 100.0
    in_range = 3.0 < psu_v < 30.0
    results.record("HW-14", "PSU voltage 3-30 V range", in_range,
                    f"psu={psu_v:.2f} V")

    # HW-15: Read opto discrete inputs (DI 0-7)
    opto_bits = client.read_discrete_inputs(DI_OPTO_BASE, DI_OPTO_COUNT)
    if opto_bits is None:
        results.record("HW-15", "Read opto discrete inputs (DI 0-7)", False, "Modbus read failed")
        return

    opto_str = "".join("1" if b else "0" for b in opto_bits)
    results.record("HW-15", "Read opto discrete inputs (DI 0-7)", True,
                    f"opto={opto_str}")

    # HW-16: Successive reads are stable (no I²C flapping)
    time.sleep(0.3)  # wait ~3 poll cycles
    ma_regs2 = client.read_holding_registers(HR_MA_IN_BASE, HR_MA_IN_COUNT)
    if ma_regs2 is not None:
        drifts = [abs(a - b) / 100.0 for a, b in zip(ma_regs, ma_regs2)]
        max_drift = max(drifts)
        results.record("HW-16", "Successive 4-20 mA reads stable (drift < 0.5 mA)",
                        max_drift < 0.5, f"max_drift={max_drift:.2f} mA")
    else:
        results.record("HW-16", "Successive 4-20 mA reads stable", False, "Second read failed")


def test_relay_writes(results, client, relay_count=16):
    """HW-20 through HW-25: Relay toggle tests."""
    results.set_category("Relay Writes (Story 10)")

    # HW-20: Write coil 0 ON
    ok = client.write_single_coil(COIL_RELAY_BASE, True)
    results.record("HW-20", "Write relay 1 ON (coil 0)", ok is True,
                    "" if ok else "Modbus write failed")
    time.sleep(0.2)

    # HW-21: Read back coil 0
    coils = client.read_coils(COIL_RELAY_BASE, 1)
    if coils is not None:
        results.record("HW-21", "Read back relay 1 = ON", coils[0] is True,
                        f"coil[0]={coils[0]}")
    else:
        results.record("HW-21", "Read back relay 1 = ON", False, "Modbus read failed")

    # HW-22: Relay read-back register (HR 24, SEQGW-23/24)
    time.sleep(1.2)  # wait for at least one verify interval (default 10 ticks = 1s)
    rb = client.read_holding_registers(HR_RELAY_READBACK, 1)
    if rb is not None:
        bit0_set = (rb[0] & 1) == 1
        results.record("HW-22", "HR 24 read-back shows relay 1 ON (SEQGW-24)",
                        bit0_set, f"HR24=0x{rb[0]:04X}")
    else:
        results.record("HW-22", "HR 24 read-back (SEQGW-24)", False, "Modbus read failed")

    # HW-23: Write coil 0 OFF
    ok = client.write_single_coil(COIL_RELAY_BASE, False)
    results.record("HW-23", "Write relay 1 OFF (coil 0)", ok is True)
    time.sleep(0.2)

    # HW-24: Toggle all relays ON then OFF
    all_ok = True
    for i in range(relay_count):
        if not client.write_single_coil(COIL_RELAY_BASE + i, True):
            all_ok = False
            break
    results.record("HW-24", f"Toggle all {relay_count} relays ON", all_ok)
    time.sleep(0.3)

    # Read back all relay coils
    coils = client.read_coils(COIL_RELAY_BASE, relay_count)
    if coils is not None:
        all_on = all(coils[:relay_count])
        results.record("HW-25", f"All {relay_count} relay coils read back ON", all_on,
                        f"coils={''.join('1' if c else '0' for c in coils[:relay_count])}")
    else:
        results.record("HW-25", f"All {relay_count} relay coils read back ON", False, "Read failed")

    # HW-26: Turn all relays OFF (cleanup)
    for i in range(relay_count):
        client.write_single_coil(COIL_RELAY_BASE + i, False)
    time.sleep(0.3)

    coils = client.read_coils(COIL_RELAY_BASE, relay_count)
    if coils is not None:
        all_off = not any(coils[:relay_count])
        results.record("HW-26", f"All {relay_count} relays OFF after cleanup", all_off,
                        f"coils={''.join('1' if c else '0' for c in coils[:relay_count])}")
    else:
        results.record("HW-26", f"All {relay_count} relays OFF after cleanup", False, "Read failed")


def test_od_outputs(results, client):
    """HW-30 through HW-33: Open-drain output tests."""
    results.set_category("Open-Drain Outputs (Story 10)")

    # HW-30: Toggle OD output 1 ON
    ok = client.write_single_coil(COIL_OD_BASE, True)
    results.record("HW-30", "Write OD output 1 ON (coil 16)", ok is True)
    time.sleep(0.2)

    # HW-31: Read back
    coils = client.read_coils(COIL_OD_BASE, 1)
    if coils is not None:
        results.record("HW-31", "OD output 1 reads back ON", coils[0] is True,
                        f"coil[16]={coils[0]}")
    else:
        results.record("HW-31", "OD output 1 reads back ON", False, "Read failed")

    # HW-32: Toggle all 4 OD outputs
    all_ok = True
    for i in range(COIL_OD_COUNT):
        if not client.write_single_coil(COIL_OD_BASE + i, True):
            all_ok = False
    results.record("HW-32", "Toggle all 4 OD outputs ON", all_ok)
    time.sleep(0.2)

    # HW-33: Cleanup — all OD OFF
    for i in range(COIL_OD_COUNT):
        client.write_single_coil(COIL_OD_BASE + i, False)
    time.sleep(0.2)
    coils = client.read_coils(COIL_OD_BASE, COIL_OD_COUNT)
    if coils is not None:
        all_off = not any(coils[:COIL_OD_COUNT])
        results.record("HW-33", "All 4 OD outputs OFF after cleanup", all_off)
    else:
        results.record("HW-33", "All 4 OD outputs OFF after cleanup", False, "Read failed")


def test_analog_outputs(results, client):
    """HW-40 through HW-45: Analog output write/read tests."""
    results.set_category("Analog Outputs")

    # HW-40: Write 0-10 V output 1 = 5.00 V (HR 16 = 500)
    ok = client.write_single_register(HR_VOLT_OUT_BASE, 500)
    results.record("HW-40", "Write 0-10 V output 1 = 5.00 V (HR 16)", ok is True)
    time.sleep(0.2)

    # HW-41: Read back HR 16
    regs = client.read_holding_registers(HR_VOLT_OUT_BASE, 1)
    if regs is not None:
        results.record("HW-41", "HR 16 reads back 500", regs[0] == 500,
                        f"HR16={regs[0]} ({regs[0]/100.0:.2f} V)")
    else:
        results.record("HW-41", "HR 16 reads back 500", False, "Read failed")

    # HW-42: Write 4-20 mA output 1 = 12.00 mA (HR 20 = 1200)
    ok = client.write_single_register(HR_MA_OUT_BASE, 1200)
    results.record("HW-42", "Write 4-20 mA output 1 = 12.00 mA (HR 20)", ok is True)
    time.sleep(0.2)

    # HW-43: Read back HR 20
    regs = client.read_holding_registers(HR_MA_OUT_BASE, 1)
    if regs is not None:
        results.record("HW-43", "HR 20 reads back 1200", regs[0] == 1200,
                        f"HR20={regs[0]} ({regs[0]/100.0:.2f} mA)")
    else:
        results.record("HW-43", "HR 20 reads back 1200", False, "Read failed")

    # HW-44: Reset outputs to 0
    client.write_single_register(HR_VOLT_OUT_BASE, 0)
    client.write_single_register(HR_MA_OUT_BASE, 0)
    time.sleep(0.2)
    regs = client.read_holding_registers(HR_VOLT_OUT_BASE, 1)
    results.record("HW-44", "0-10 V output 1 reset to 0", regs is not None and regs[0] == 0)

    regs = client.read_holding_registers(HR_MA_OUT_BASE, 1)
    results.record("HW-45", "4-20 mA output 1 reset to 0", regs is not None and regs[0] == 0)


def test_stability(results, client, health_url, duration=5):
    """HW-50 through HW-53: Multi-second stability test."""
    results.set_category("Stability & Performance")

    samples = []
    errors_start = None
    print(f"    Running {duration}-second stability test...")

    for i in range(duration * 2):  # sample every 500 ms
        try:
            resp = urllib.request.urlopen(health_url, timeout=5)
            data = json.loads(resp.read().decode())
            samples.append(data)
            if errors_start is None:
                errors_start = data.get("i2c_errors", 0)
        except Exception:
            pass
        time.sleep(0.5)

    if not samples:
        results.record("HW-50", f"{duration}s stability test collected samples", False, "No samples")
        return

    results.record("HW-50", f"{duration}s stability test collected samples",
                    len(samples) >= duration, f"{len(samples)} samples")

    # HW-51: No new I²C errors during test
    errors_end = samples[-1].get("i2c_errors", 0)
    new_errors = errors_end - (errors_start or 0)
    results.record("HW-51", "No new I²C errors during stability test",
                    new_errors == 0, f"new_errors={new_errors}")

    # HW-52: Status stayed ok
    all_ok = all(s.get("status") == "ok" for s in samples)
    results.record("HW-52", 'Health status stayed "ok" throughout', all_ok)

    # HW-53: Cycle time consistently < 1 ms
    cycles = [s.get("last_cycle_ms", 99) for s in samples]
    max_cycle = max(cycles) if cycles else 99
    avg_cycle = sum(cycles) / len(cycles) if cycles else 99
    results.record("HW-53", "Max cycle time < 1.0 ms over test period",
                    max_cycle < 1.0,
                    f"avg={avg_cycle:.3f} ms, max={max_cycle:.3f} ms")


# ── Main ──────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Hardware validation suite for Sequent Gateway",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Paste the full output back to the developer for Story 10 / Epic 2 sign-off.",
    )
    parser.add_argument("--modbus-host", default=DEFAULT_MODBUS_HOST)
    parser.add_argument("--modbus-port", type=int, default=DEFAULT_MODBUS_PORT)
    parser.add_argument("--health-port", type=int, default=DEFAULT_HEALTH_PORT)
    parser.add_argument("--relay-count", type=int, default=16,
                        help="Number of relay channels (8 or 16)")
    parser.add_argument("--skip-writes", action="store_true",
                        help="Skip relay/OD/analog output tests (safe for live equipment)")
    parser.add_argument("--only", choices=["health", "analog", "relay", "od", "aout", "stability"],
                        help="Run only one test category")
    parser.add_argument("--stability-duration", type=int, default=5,
                        help="Duration of stability test in seconds")
    args = parser.parse_args()

    health_url = f"http://{args.modbus_host}:{args.health_port}/health"
    results = Results()

    print()
    print("=" * 60)
    print("  Sequent Gateway — Hardware Validation Suite")
    print(f"  Modbus: {args.modbus_host}:{args.modbus_port}")
    print(f"  Health: {health_url}")
    print(f"  Date:   {datetime.now().isoformat(timespec='seconds')}")
    print("=" * 60)

    # ── Connect Modbus client ─────────────────────────────────────────
    client = ModbusClient(
        host=args.modbus_host,
        port=args.modbus_port,
        auto_open=True,
        auto_close=False,
        timeout=5.0,
    )

    run_all = args.only is None

    # ── Health endpoint tests ─────────────────────────────────────────
    if run_all or args.only == "health":
        print("\n  ── Health Endpoint ──")
        test_health(results, health_url)

    # ── Analog input tests ────────────────────────────────────────────
    if run_all or args.only == "analog":
        print("\n  ── Analog Inputs ──")
        test_analog_inputs(results, client)

    # ── Relay write tests ─────────────────────────────────────────────
    if (run_all or args.only == "relay") and not args.skip_writes:
        print("\n  ── Relay Writes ──")
        test_relay_writes(results, client, relay_count=args.relay_count)

    # ── OD output tests ───────────────────────────────────────────────
    if (run_all or args.only == "od") and not args.skip_writes:
        print("\n  ── Open-Drain Outputs ──")
        test_od_outputs(results, client)

    # ── Analog output tests ───────────────────────────────────────────
    if (run_all or args.only == "aout") and not args.skip_writes:
        print("\n  ── Analog Outputs ──")
        test_analog_outputs(results, client)

    # ── Stability test ────────────────────────────────────────────────
    if run_all or args.only == "stability":
        print("\n  ── Stability & Performance ──")
        test_stability(results, client, health_url,
                       duration=args.stability_duration)

    # ── Final report ──────────────────────────────────────────────────
    client.close()
    report = results.report()
    print(report)

    # Also write to a file for easy copy-paste
    report_path = "hw-validation-report.txt"
    with open(report_path, "w") as f:
        f.write(report)
    print(f"\n  Report saved to: {report_path}")
    print("  Copy and paste the output above back to the developer.\n")

    _, _, failed = results.summary()
    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
