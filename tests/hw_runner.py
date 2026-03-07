#!/usr/bin/env python3
"""
Automated Hardware Validation Runner
======================================

Discovers test scenarios from TOML files, launches the gateway in each
configuration, runs the applicable tests, tears it down, then moves on.

Each scenario file lives in tests/scenarios/ and declares:
  - [gateway]  — CLI flags for launching sequent-gateway
  - [expect]   — expected board capabilities (relay_count, channel counts)
  - [tests]    — which test categories to enable

Prerequisites:
    pip3 install pyModbusTCP

Usage (run on the Pi, from the repo root):
    # Run ALL scenarios:
    sudo ~/venv/bin/python3 tests/hw_runner.py

    # Run a single scenario file:
    sudo ~/venv/bin/python3 tests/hw_runner.py --scenario tests/scenarios/default_multi.toml

    # Use a custom gateway binary:
    sudo ~/venv/bin/python3 tests/hw_runner.py --gateway-bin ./target/debug/sequent-gateway

    # Skip relay/output writes (safe for live equipment):
    sudo ~/venv/bin/python3 tests/hw_runner.py --skip-writes

    # Override stability test duration:
    sudo ~/venv/bin/python3 tests/hw_runner.py --stability-duration 10
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Any

# ── TOML support ──────────────────────────────────────────────────────
try:
    import tomllib                          # Python 3.11+
except ModuleNotFoundError:
    try:
        import tomli as tomllib             # pip install tomli
    except ModuleNotFoundError:
        print(
            "ERROR: No TOML parser available.\n"
            "  Python 3.11+ has tomllib built-in.\n"
            "  On older versions: pip3 install tomli"
        )
        sys.exit(1)

try:
    from pyModbusTCP.client import ModbusClient
except ImportError:
    print("ERROR: pyModbusTCP not installed.  Run:  pip3 install pyModbusTCP")
    sys.exit(1)


# ════════════════════════════════════════════════════════════════════════
# Scenario configuration (parsed from TOML)
# ════════════════════════════════════════════════════════════════════════

@dataclass
class ScenarioConfig:
    """Everything the runner needs to launch + validate one scenario."""

    # Identity
    name: str = "Unnamed Scenario"
    description: str = ""

    # Gateway CLI
    boards: list[str] = field(default_factory=lambda: ["megaind", "relay16"])
    single_slave: bool = False
    relay_slave_id: int = 1
    ind_slave_id: int = 2
    ind_stack: int = 1
    relay_stack: int = 0
    health_port: int = 8080
    modbus_port: int = 502
    builtin_defaults: bool = False
    boards_dir: str = "boards"
    extra_args: list[str] = field(default_factory=list)

    # Expected capabilities
    relay_count: int = 16
    opto_channels: int = 8
    ma_in_channels: int = 8
    v_in_channels: int = 4
    od_channels: int = 4
    v_out_channels: int = 4
    ma_out_channels: int = 4
    relay_readback: bool = True

    # Test toggles
    test_health: bool = True
    test_analog_inputs: bool = True
    test_relay_writes: bool = True
    test_od_outputs: bool = True
    test_analog_outputs: bool = True
    test_stability: bool = True

    @classmethod
    def from_toml(cls, path: Path) -> "ScenarioConfig":
        """Load a scenario from a TOML file."""
        with open(path, "rb") as f:
            data = tomllib.load(f)

        sc = cls()

        # [scenario]
        scn = data.get("scenario", {})
        sc.name = scn.get("name", path.stem)
        sc.description = scn.get("description", "")

        # [gateway]
        gw = data.get("gateway", {})
        sc.boards = gw.get("boards", sc.boards)
        sc.single_slave = gw.get("single_slave", sc.single_slave)
        sc.relay_slave_id = gw.get("relay_slave_id", sc.relay_slave_id)
        sc.ind_slave_id = gw.get("ind_slave_id", sc.ind_slave_id)
        sc.ind_stack = gw.get("ind_stack", sc.ind_stack)
        sc.relay_stack = gw.get("relay_stack", sc.relay_stack)
        sc.health_port = gw.get("health_port", sc.health_port)
        sc.modbus_port = gw.get("modbus_port", sc.modbus_port)
        sc.builtin_defaults = gw.get("builtin_defaults", sc.builtin_defaults)
        sc.boards_dir = gw.get("boards_dir", sc.boards_dir)
        sc.extra_args = gw.get("extra_args", sc.extra_args)

        # [expect]
        ex = data.get("expect", {})
        sc.relay_count = ex.get("relay_count", sc.relay_count)
        sc.opto_channels = ex.get("opto_channels", sc.opto_channels)
        sc.ma_in_channels = ex.get("ma_in_channels", sc.ma_in_channels)
        sc.v_in_channels = ex.get("v_in_channels", sc.v_in_channels)
        sc.od_channels = ex.get("od_channels", sc.od_channels)
        sc.v_out_channels = ex.get("v_out_channels", sc.v_out_channels)
        sc.ma_out_channels = ex.get("ma_out_channels", sc.ma_out_channels)
        sc.relay_readback = ex.get("relay_readback", sc.relay_readback)

        # [tests]
        ts = data.get("tests", {})
        sc.test_health = ts.get("health", sc.test_health)
        sc.test_analog_inputs = ts.get("analog_inputs", sc.test_analog_inputs)
        sc.test_relay_writes = ts.get("relay_writes", sc.test_relay_writes)
        sc.test_od_outputs = ts.get("od_outputs", sc.test_od_outputs)
        sc.test_analog_outputs = ts.get("analog_outputs", sc.test_analog_outputs)
        sc.test_stability = ts.get("stability", sc.test_stability)

        return sc

    def gateway_args(self, gateway_bin: str) -> list[str]:
        """Build the CLI argv list for launching the gateway."""
        cmd = [gateway_bin]
        for b in self.boards:
            cmd += ["--board", b]
        cmd += ["--port", str(self.modbus_port)]
        cmd += ["--health-port", str(self.health_port)]
        cmd += ["--ind-stack", str(self.ind_stack)]
        cmd += ["--relay-stack", str(self.relay_stack)]
        cmd += ["--relay-slave-id", str(self.relay_slave_id)]
        cmd += ["--ind-slave-id", str(self.ind_slave_id)]
        if self.single_slave:
            cmd.append("--single-slave")
        cmd += ["--boards-dir", self.boards_dir]
        if self.builtin_defaults:
            cmd.append("--builtin-defaults")
        cmd += self.extra_args
        return cmd


# ════════════════════════════════════════════════════════════════════════
# Holding register / coil constants (must match databank.rs)
# ════════════════════════════════════════════════════════════════════════

HR_MA_IN_BASE      = 0     # 0-7:  4-20 mA inputs  (mA × 100)
HR_PSU_VOLTAGE     = 8     # 8:    PSU voltage      (V × 100)
HR_VOLT_IN_BASE    = 10    # 10-13: 0-10 V inputs   (V × 100)
HR_OPTO_BITMASK    = 15    # 15:   opto bitmask (optional)
HR_VOLT_OUT_BASE   = 16    # 16-19: 0-10 V outputs  (V × 100)
HR_MA_OUT_BASE     = 20    # 20-23: 4-20 mA outputs (mA × 100)
HR_RELAY_READBACK  = 24    # 24:   relay read-back bitmask

COIL_RELAY_BASE  = 0
COIL_OD_BASE     = 16
DI_OPTO_BASE     = 0


# ════════════════════════════════════════════════════════════════════════
# Result tracking
# ════════════════════════════════════════════════════════════════════════

class Results:
    """Accumulates test results within (and across) scenarios."""

    def __init__(self):
        self.tests: list[dict[str, Any]] = []
        self.category = ""
        self.scenario = ""

    def set_scenario(self, name: str):
        self.scenario = name

    def set_category(self, name: str):
        self.category = name

    def record(self, test_id: str, desc: str, passed: bool, detail: str = ""):
        status = "PASS" if passed else "FAIL"
        self.tests.append({
            "id": test_id,
            "scenario": self.scenario,
            "category": self.category,
            "desc": desc,
            "status": status,
            "detail": detail,
        })
        marker = "✅" if passed else "❌"
        line = f"  [{status}] {test_id}: {desc}"
        if detail:
            line += f"  ({detail})"
        print(f"  {marker} {line.strip()}")

    def summary(self):
        total = len(self.tests)
        passed = sum(1 for t in self.tests if t["status"] == "PASS")
        return total, passed, total - passed

    def scenario_summary(self, scenario_name: str):
        """Stats for one scenario only."""
        scn = [t for t in self.tests if t["scenario"] == scenario_name]
        total = len(scn)
        passed = sum(1 for t in scn if t["status"] == "PASS")
        return total, passed, total - passed

    def report(self) -> str:
        """Produce the final paste-friendly report."""
        total, passed, failed = self.summary()
        lines: list[str] = []
        lines.append("")
        lines.append("=" * 70)
        lines.append("  HARDWARE VALIDATION REPORT (Automated Runner)")
        lines.append(f"  Date: {datetime.now().isoformat(timespec='seconds')}")
        lines.append(f"  Result: {passed}/{total} passed, {failed} failed")
        lines.append("=" * 70)

        current_scn = None
        current_cat = None
        for t in self.tests:
            if t["scenario"] != current_scn:
                current_scn = t["scenario"]
                s_total, s_passed, s_failed = self.scenario_summary(current_scn)
                lines.append(f"\n  ━━ Scenario: {current_scn} ━━  ({s_passed}/{s_total})")
                current_cat = None
            if t["category"] != current_cat:
                current_cat = t["category"]
                lines.append(f"\n    --- {current_cat} ---")
            line = f"    [{t['status']}] {t['id']}: {t['desc']}"
            if t["detail"]:
                line += f"  ({t['detail']})"
            lines.append(line)

        lines.append("")
        lines.append(f"  TOTAL: {passed}/{total} passed")
        if failed > 0:
            fail_ids = [t["id"] for t in self.tests if t["status"] == "FAIL"]
            lines.append(f"  FAILED: {', '.join(fail_ids)}")
        lines.append("=" * 70)
        return "\n".join(lines)


# ════════════════════════════════════════════════════════════════════════
# Gateway process management
# ════════════════════════════════════════════════════════════════════════

class GatewayProcess:
    """Context manager that launches the gateway and tears it down."""

    def __init__(self, config: ScenarioConfig, gateway_bin: str,
                 startup_timeout: float = 10.0):
        self.config = config
        self.gateway_bin = gateway_bin
        self.startup_timeout = startup_timeout
        self.proc: subprocess.Popen | None = None

    def __enter__(self) -> "GatewayProcess":
        cmd = self.config.gateway_args(self.gateway_bin)
        print(f"\n    ▶ Launching: {' '.join(cmd)}")

        self.proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )

        # Wait for the health endpoint to respond
        health_url = f"http://127.0.0.1:{self.config.health_port}/health"
        deadline = time.monotonic() + self.startup_timeout
        started = False

        while time.monotonic() < deadline:
            # Check the process hasn't crashed
            if self.proc.poll() is not None:
                out = self.proc.stdout.read() if self.proc.stdout else ""
                raise RuntimeError(
                    f"Gateway exited immediately (code {self.proc.returncode}):\n{out}"
                )
            try:
                resp = urllib.request.urlopen(health_url, timeout=2)
                if resp.status == 200:
                    started = True
                    break
            except (urllib.error.URLError, ConnectionError, OSError):
                pass
            time.sleep(0.3)

        if not started:
            self._kill()
            raise RuntimeError(
                f"Gateway did not respond on {health_url} within "
                f"{self.startup_timeout}s"
            )

        print(f"    ✅ Gateway healthy on port {self.config.health_port}")
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._kill()
        return False

    def _kill(self):
        if self.proc is None or self.proc.poll() is not None:
            return
        print("    ■ Shutting down gateway …")
        # Graceful SIGTERM (or TerminateProcess on Windows)
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=3)
        print("    ■ Gateway stopped")


# ════════════════════════════════════════════════════════════════════════
# Test modules
# ════════════════════════════════════════════════════════════════════════

def test_health(results: Results, cfg: ScenarioConfig):
    """Health endpoint checks — always applicable."""
    results.set_category("Health Endpoint")
    health_url = f"http://127.0.0.1:{cfg.health_port}/health"

    # HH-01: endpoint responds
    try:
        resp = urllib.request.urlopen(health_url, timeout=5)
        body = resp.read().decode()
        results.record("HH-01", "Health endpoint responds (200 OK)",
                        resp.status == 200, f"HTTP {resp.status}")
    except Exception as e:
        results.record("HH-01", "Health endpoint responds (200 OK)", False, str(e))
        return

    # HH-02: valid JSON
    try:
        data = json.loads(body)
        results.record("HH-02", "Response is valid JSON", True)
    except json.JSONDecodeError as e:
        results.record("HH-02", "Response is valid JSON", False, str(e))
        return

    # HH-03: status field
    status = data.get("status", "MISSING")
    results.record("HH-03", 'Status field is "ok" or "degraded"',
                    status in ("ok", "degraded"), f'status="{status}"')

    # HH-04: uptime
    uptime = data.get("uptime_s", -1)
    results.record("HH-04", "Uptime > 0 seconds", uptime > 0,
                    f"uptime_s={uptime}")

    # HH-05: cycle time
    cycle = data.get("last_cycle_ms", -1)
    results.record("HH-05", "Cycle time present and > 0",
                    cycle > 0, f"last_cycle_ms={cycle:.2f}")

    # HH-06: i2c_errors field
    errors = data.get("i2c_errors", "MISSING")
    results.record("HH-06", "i2c_errors field present",
                    errors != "MISSING", f"i2c_errors={errors}")

    # HH-07: i2c_recoveries field
    recoveries = data.get("i2c_recoveries", "MISSING")
    results.record("HH-07", "i2c_recoveries field present",
                    recoveries != "MISSING", f"i2c_recoveries={recoveries}")

    # HH-08: relay_mismatches field
    mismatches = data.get("relay_mismatches", "MISSING")
    results.record("HH-08", "relay_mismatches field present",
                    mismatches != "MISSING", f"relay_mismatches={mismatches}")

    # HH-09: channels block
    channels = data.get("channels", {})
    expected_keys = {"ma", "volt", "psu", "opto"}
    all_present = expected_keys.issubset(channels.keys())
    results.record("HH-09", "All 4 channel status fields present",
                    all_present, f"channels={json.dumps(channels)}")

    # HH-10: cycle time benchmark (< 15 ms)
    results.record("HH-10", "I/O cycle < 15 ms (performance benchmark)",
                    0 < cycle < 15.0, f"last_cycle_ms={cycle:.3f}")


def test_analog_inputs(results: Results, client: ModbusClient, cfg: ScenarioConfig):
    """Analog input channel reads — gated by expect.ma_in_channels."""
    results.set_category("Analog Inputs")

    ma_count = cfg.ma_in_channels
    v_count = cfg.v_in_channels

    # HA-01: 4-20 mA registers
    if ma_count > 0:
        ma_regs = client.read_holding_registers(HR_MA_IN_BASE, ma_count)
        if ma_regs is None:
            results.record("HA-01", f"Read 4-20 mA registers (HR 0-{ma_count-1})",
                            False, "Modbus read failed")
        else:
            ma_vals = [r / 100.0 for r in ma_regs]
            results.record("HA-01", f"Read {ma_count} mA input registers", True,
                            f"mA={[f'{v:.2f}' for v in ma_vals]}")

            # HA-02: at least one > 0
            any_nz = any(v > 0 for v in ma_vals)
            results.record("HA-02", "At least one 4-20 mA channel > 0 mA", any_nz,
                            "All zero — check wiring or confirm no input signal")

            # HA-06: successive reads stable
            time.sleep(0.3)
            ma_regs2 = client.read_holding_registers(HR_MA_IN_BASE, ma_count)
            if ma_regs2 is not None:
                drifts = [abs(a - b) / 100.0 for a, b in zip(ma_regs, ma_regs2)]
                max_drift = max(drifts)
                results.record("HA-06", "Successive 4-20 mA reads stable (drift < 0.5 mA)",
                                max_drift < 0.5, f"max_drift={max_drift:.2f} mA")

    # HA-03: 0-10 V registers
    if v_count > 0:
        v_regs = client.read_holding_registers(HR_VOLT_IN_BASE, v_count)
        if v_regs is None:
            results.record("HA-03", f"Read 0-10 V registers (HR 10-{10+v_count-1})",
                            False, "Modbus read failed")
        else:
            v_vals = [r / 100.0 for r in v_regs]
            results.record("HA-03", f"Read {v_count} voltage input registers", True,
                            f"V={[f'{v:.2f}' for v in v_vals]}")

    # HA-04: PSU voltage
    psu_regs = client.read_holding_registers(HR_PSU_VOLTAGE, 1)
    if psu_regs is None:
        results.record("HA-04", "Read PSU voltage (HR 8)", False, "Modbus read failed")
    else:
        psu_v = psu_regs[0] / 100.0
        results.record("HA-04", "PSU voltage 3-30 V range",
                        3.0 < psu_v < 30.0, f"psu={psu_v:.2f} V")

    # HA-05: opto discrete inputs
    if cfg.opto_channels > 0:
        opto_bits = client.read_discrete_inputs(DI_OPTO_BASE, cfg.opto_channels)
        if opto_bits is None:
            results.record("HA-05", f"Read opto discrete inputs (DI 0-{cfg.opto_channels-1})",
                            False, "Modbus read failed")
        else:
            opto_str = "".join("1" if b else "0" for b in opto_bits)
            results.record("HA-05", f"Read {cfg.opto_channels} opto discrete inputs",
                            True, f"opto={opto_str}")


def test_relay_writes(results: Results, client: ModbusClient, cfg: ScenarioConfig):
    """Relay toggle tests — gated by expect.relay_count."""
    results.set_category("Relay Writes")
    n = cfg.relay_count
    if n == 0:
        return

    # HR-01: Write coil 0 ON
    ok = client.write_single_coil(COIL_RELAY_BASE, True)
    results.record("HR-01", "Write relay 1 ON (coil 0)", ok is True)
    time.sleep(0.2)

    # HR-02: Read back coil 0
    coils = client.read_coils(COIL_RELAY_BASE, 1)
    if coils is not None:
        results.record("HR-02", "Read back relay 1 = ON",
                        coils[0] is True, f"coil[0]={coils[0]}")
    else:
        results.record("HR-02", "Read back relay 1 = ON", False, "Modbus read failed")

    # HR-03: Relay read-back register (HR 24)
    if cfg.relay_readback:
        time.sleep(1.2)  # wait for at least one verify interval
        # HR 24 is on the ind slave in multi-slave mode
        if not cfg.single_slave and "megaind" in cfg.boards:
            saved_uid = client.unit_id
            client.unit_id = cfg.ind_slave_id
            rb = client.read_holding_registers(HR_RELAY_READBACK, 1)
            client.unit_id = saved_uid
        else:
            rb = client.read_holding_registers(HR_RELAY_READBACK, 1)
        if rb is not None:
            bit0_set = (rb[0] & 1) == 1
            results.record("HR-03", "HR 24 read-back shows relay 1 ON",
                            bit0_set, f"HR24=0x{rb[0]:04X}")
        else:
            results.record("HR-03", "HR 24 read-back", False, "Modbus read failed")

    # HR-04: Write coil 0 OFF
    ok = client.write_single_coil(COIL_RELAY_BASE, False)
    results.record("HR-04", "Write relay 1 OFF (coil 0)", ok is True)
    time.sleep(0.2)

    # HR-05: Toggle all relays ON
    all_ok = True
    for i in range(n):
        if not client.write_single_coil(COIL_RELAY_BASE + i, True):
            all_ok = False
            break
    results.record("HR-05", f"Toggle all {n} relays ON", all_ok)
    time.sleep(0.3)

    # HR-06: Read back all relays
    coils = client.read_coils(COIL_RELAY_BASE, n)
    if coils is not None:
        all_on = all(coils[:n])
        results.record("HR-06", f"All {n} relay coils read back ON", all_on,
                        f"coils={''.join('1' if c else '0' for c in coils[:n])}")
    else:
        results.record("HR-06", f"All {n} relay coils read back ON", False, "Read failed")

    # HR-07: Cleanup — all OFF
    for i in range(n):
        client.write_single_coil(COIL_RELAY_BASE + i, False)
    time.sleep(0.3)

    coils = client.read_coils(COIL_RELAY_BASE, n)
    if coils is not None:
        all_off = not any(coils[:n])
        results.record("HR-07", f"All {n} relays OFF after cleanup", all_off,
                        f"coils={''.join('1' if c else '0' for c in coils[:n])}")
    else:
        results.record("HR-07", f"All {n} relays OFF after cleanup", False, "Read failed")


def test_od_outputs(results: Results, client: ModbusClient, cfg: ScenarioConfig):
    """Open-drain output tests — gated by expect.od_channels."""
    results.set_category("Open-Drain Outputs")
    n = cfg.od_channels
    if n == 0:
        return

    # In multi-slave mode, OD coils start at 0 on the ind slave.
    # In single-slave mode, they're at offset 16.
    od_base = COIL_OD_BASE if cfg.single_slave else 0

    # HO-01: Toggle OD output 1 ON
    ok = client.write_single_coil(od_base, True)
    results.record("HO-01", "Write OD output 1 ON", ok is True)
    time.sleep(0.2)

    # HO-02: Read back
    coils = client.read_coils(od_base, 1)
    if coils is not None:
        results.record("HO-02", "OD output 1 reads back ON",
                        coils[0] is True, f"coil[{od_base}]={coils[0]}")
    else:
        results.record("HO-02", "OD output 1 reads back ON", False, "Read failed")

    # HO-03: Toggle all OD outputs
    all_ok = True
    for i in range(n):
        if not client.write_single_coil(od_base + i, True):
            all_ok = False
    results.record("HO-03", f"Toggle all {n} OD outputs ON", all_ok)
    time.sleep(0.2)

    # HO-04: Cleanup
    for i in range(n):
        client.write_single_coil(od_base + i, False)
    time.sleep(0.2)
    coils = client.read_coils(od_base, n)
    if coils is not None:
        all_off = not any(coils[:n])
        results.record("HO-04", f"All {n} OD outputs OFF after cleanup", all_off)
    else:
        results.record("HO-04", f"All {n} OD outputs OFF after cleanup", False, "Read failed")


def test_analog_outputs(results: Results, client: ModbusClient, cfg: ScenarioConfig):
    """Analog output write/read tests."""
    results.set_category("Analog Outputs")

    v_out = cfg.v_out_channels
    ma_out = cfg.ma_out_channels

    if v_out > 0:
        # HAO-01: Write 0-10 V output 1 = 5.00 V
        ok = client.write_single_register(HR_VOLT_OUT_BASE, 500)
        results.record("HAO-01", "Write 0-10 V output 1 = 5.00 V (HR 16)", ok is True)
        time.sleep(0.2)

        # HAO-02: Read back
        regs = client.read_holding_registers(HR_VOLT_OUT_BASE, 1)
        if regs is not None:
            results.record("HAO-02", "HR 16 reads back 500", regs[0] == 500,
                            f"HR16={regs[0]} ({regs[0]/100.0:.2f} V)")
        else:
            results.record("HAO-02", "HR 16 reads back 500", False, "Read failed")

        # HAO-05: Reset V output
        client.write_single_register(HR_VOLT_OUT_BASE, 0)
        time.sleep(0.2)
        regs = client.read_holding_registers(HR_VOLT_OUT_BASE, 1)
        results.record("HAO-05", "0-10 V output 1 reset to 0",
                        regs is not None and regs[0] == 0)

    if ma_out > 0:
        # HAO-03: Write 4-20 mA output 1 = 12.00 mA
        ok = client.write_single_register(HR_MA_OUT_BASE, 1200)
        results.record("HAO-03", "Write 4-20 mA output 1 = 12.00 mA (HR 20)", ok is True)
        time.sleep(0.2)

        # HAO-04: Read back
        regs = client.read_holding_registers(HR_MA_OUT_BASE, 1)
        if regs is not None:
            results.record("HAO-04", "HR 20 reads back 1200", regs[0] == 1200,
                            f"HR20={regs[0]} ({regs[0]/100.0:.2f} mA)")
        else:
            results.record("HAO-04", "HR 20 reads back 1200", False, "Read failed")

        # HAO-06: Reset mA output
        client.write_single_register(HR_MA_OUT_BASE, 0)
        time.sleep(0.2)
        regs = client.read_holding_registers(HR_MA_OUT_BASE, 1)
        results.record("HAO-06", "4-20 mA output 1 reset to 0",
                        regs is not None and regs[0] == 0)


def test_stability(results: Results, cfg: ScenarioConfig, duration: int = 5):
    """Multi-second stability test against the health endpoint."""
    results.set_category("Stability & Performance")
    health_url = f"http://127.0.0.1:{cfg.health_port}/health"

    samples: list[dict] = []
    errors_start: int | None = None
    print(f"    Running {duration}-second stability test …")

    for _ in range(duration * 2):  # sample every 500 ms
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
        results.record("HS-01", f"{duration}s stability test collected samples",
                        False, "No samples")
        return

    results.record("HS-01", f"{duration}s stability test collected samples",
                    len(samples) >= duration, f"{len(samples)} samples")

    # HS-02: No new I²C errors
    errors_end = samples[-1].get("i2c_errors", 0)
    new_errors = errors_end - (errors_start or 0)
    results.record("HS-02", "No new I²C errors during stability test",
                    new_errors == 0, f"new_errors={new_errors}")

    # HS-03: Status stayed ok
    all_ok = all(s.get("status") == "ok" for s in samples)
    results.record("HS-03", 'Health status stayed "ok" throughout', all_ok)

    # HS-04: Max cycle time < 15 ms
    cycles = [s.get("last_cycle_ms", 99) for s in samples]
    max_cycle = max(cycles) if cycles else 99
    avg_cycle = sum(cycles) / len(cycles) if cycles else 99
    results.record("HS-04", "Max cycle time < 15 ms over test period",
                    max_cycle < 15.0,
                    f"avg={avg_cycle:.3f} ms, max={max_cycle:.3f} ms")


# ════════════════════════════════════════════════════════════════════════
# Scenario runner
# ════════════════════════════════════════════════════════════════════════

def run_scenario(
    cfg: ScenarioConfig,
    results: Results,
    gateway_bin: str,
    skip_writes: bool = False,
    stability_duration: int = 5,
):
    """Launch the gateway, run all enabled tests, tear it down."""
    results.set_scenario(cfg.name)

    print(f"\n{'━' * 70}")
    print(f"  SCENARIO: {cfg.name}")
    print(f"  {cfg.description}")
    print(f"  Boards: {cfg.boards}  |  "
          f"{'single-slave' if cfg.single_slave else 'multi-slave'}  |  "
          f"relay_count={cfg.relay_count}")
    print(f"{'━' * 70}")

    try:
        with GatewayProcess(cfg, gateway_bin) as _gw:
            # Allow a brief settling period after health responds
            time.sleep(0.5)

            # ── Health (always) ───────────────────────────────────────
            if cfg.test_health:
                print("\n    ── Health Endpoint ──")
                test_health(results, cfg)

            # ── Analog inputs ─────────────────────────────────────────
            if cfg.test_analog_inputs and (cfg.ma_in_channels > 0 or
                                            cfg.v_in_channels > 0 or
                                            cfg.opto_channels > 0):
                print("\n    ── Analog Inputs ──")
                client = _make_client(cfg, board="ind")
                test_analog_inputs(results, client, cfg)
                client.close()

            # ── Relay writes ──────────────────────────────────────────
            if cfg.test_relay_writes and cfg.relay_count > 0 and not skip_writes:
                print("\n    ── Relay Writes ──")
                client = _make_client(cfg, board="relay")
                test_relay_writes(results, client, cfg)
                client.close()

            # ── OD outputs ────────────────────────────────────────────
            if cfg.test_od_outputs and cfg.od_channels > 0 and not skip_writes:
                print("\n    ── Open-Drain Outputs ──")
                client = _make_client(cfg, board="ind")
                test_od_outputs(results, client, cfg)
                client.close()

            # ── Analog outputs ────────────────────────────────────────
            if cfg.test_analog_outputs and (cfg.v_out_channels > 0 or
                                             cfg.ma_out_channels > 0) and not skip_writes:
                print("\n    ── Analog Outputs ──")
                client = _make_client(cfg, board="ind")
                test_analog_outputs(results, client, cfg)
                client.close()

            # ── Stability ─────────────────────────────────────────────
            if cfg.test_stability:
                print("\n    ── Stability & Performance ──")
                test_stability(results, cfg, duration=stability_duration)

    except RuntimeError as e:
        results.record("LAUNCH", f"Gateway launch for '{cfg.name}'", False, str(e))

    # Scenario subtotal
    s_total, s_passed, s_failed = results.scenario_summary(cfg.name)
    status_icon = "✅" if s_failed == 0 else "❌"
    print(f"\n  {status_icon} Scenario '{cfg.name}': {s_passed}/{s_total} passed")


def _make_client(cfg: ScenarioConfig, board: str = "ind") -> ModbusClient:
    """Create a Modbus client pointed at the right slave ID."""
    if cfg.single_slave:
        uid = cfg.relay_slave_id
    elif board == "relay":
        uid = cfg.relay_slave_id
    else:
        uid = cfg.ind_slave_id

    client = ModbusClient(
        host="127.0.0.1",
        port=cfg.modbus_port,
        unit_id=uid,
        auto_open=True,
        auto_close=False,
        timeout=5.0,
    )
    return client


# ════════════════════════════════════════════════════════════════════════
# Scenario discovery
# ════════════════════════════════════════════════════════════════════════

def discover_scenarios(scenario_dir: Path) -> list[Path]:
    """Find all .toml files in the scenarios directory, sorted by name."""
    if not scenario_dir.is_dir():
        print(f"  ⚠️  Scenario directory not found: {scenario_dir}")
        return []
    tomls = sorted(scenario_dir.glob("*.toml"))
    if not tomls:
        print(f"  ⚠️  No .toml files found in {scenario_dir}")
    return tomls


# ════════════════════════════════════════════════════════════════════════
# CLI & main
# ════════════════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(
        description="Automated hardware validation runner for Sequent Gateway",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "Examples:\n"
            "  # Run all scenarios:\n"
            "  sudo ~/venv/bin/python3 tests/hw_runner.py\n\n"
            "  # Run one specific scenario:\n"
            "  sudo ~/venv/bin/python3 tests/hw_runner.py "
            "--scenario tests/scenarios/default_multi.toml\n\n"
            "  # Skip writes (safe for live equipment):\n"
            "  sudo ~/venv/bin/python3 tests/hw_runner.py --skip-writes\n"
        ),
    )
    parser.add_argument(
        "--gateway-bin",
        default="./target/release/sequent-gateway",
        help="Path to the gateway binary (default: ./target/release/sequent-gateway)",
    )
    parser.add_argument(
        "--scenario-dir",
        default="tests/scenarios",
        help="Directory containing scenario TOML files (default: tests/scenarios)",
    )
    parser.add_argument(
        "--scenario",
        action="append",
        dest="scenarios",
        help="Run only this scenario file (can be specified multiple times)",
    )
    parser.add_argument(
        "--skip-writes",
        action="store_true",
        help="Skip relay/OD/analog output write tests",
    )
    parser.add_argument(
        "--stability-duration",
        type=int,
        default=5,
        help="Duration of stability test in seconds (default: 5)",
    )
    parser.add_argument(
        "--startup-timeout",
        type=float,
        default=10.0,
        help="Seconds to wait for gateway health endpoint (default: 10)",
    )
    args = parser.parse_args()

    # ── Discover or select scenarios ──────────────────────────────────
    if args.scenarios:
        scenario_paths = [Path(s) for s in args.scenarios]
        missing = [p for p in scenario_paths if not p.exists()]
        if missing:
            print(f"ERROR: Scenario file(s) not found: {missing}")
            sys.exit(1)
    else:
        scenario_paths = discover_scenarios(Path(args.scenario_dir))
        if not scenario_paths:
            sys.exit(1)

    # ── Header ────────────────────────────────────────────────────────
    print()
    print("=" * 70)
    print("  Sequent Gateway — Automated Hardware Validation Runner")
    print(f"  Date:     {datetime.now().isoformat(timespec='seconds')}")
    print(f"  Gateway:  {args.gateway_bin}")
    print(f"  Scenarios: {len(scenario_paths)} discovered")
    for p in scenario_paths:
        print(f"    • {p.name}")
    print("=" * 70)

    # ── Load & validate scenarios ─────────────────────────────────────
    configs: list[ScenarioConfig] = []
    for path in scenario_paths:
        try:
            cfg = ScenarioConfig.from_toml(path)
            configs.append(cfg)
        except Exception as e:
            print(f"  ⚠️  Failed to parse {path.name}: {e}")

    if not configs:
        print("ERROR: No valid scenarios loaded.")
        sys.exit(1)

    # ── Run each scenario ─────────────────────────────────────────────
    results = Results()

    for cfg in configs:
        run_scenario(
            cfg,
            results,
            gateway_bin=args.gateway_bin,
            skip_writes=args.skip_writes,
            stability_duration=args.stability_duration,
        )
        # Brief pause between scenarios to let ports release
        time.sleep(1.0)

    # ── Final report ──────────────────────────────────────────────────
    report = results.report()
    print(report)

    report_path = "hw-runner-report.txt"
    with open(report_path, "w") as f:
        f.write(report)
    print(f"\n  Report saved to: {report_path}")
    print("  Copy and paste the output above for sign-off.\n")

    _, _, failed = results.summary()
    sys.exit(1 if failed > 0 else 0)


if __name__ == "__main__":
    main()
