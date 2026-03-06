# Roadmap

## Background

The `modbusTCP.py` gateway is the result of a significant architectural pivot. After deep-dive diagnostics proved that native Modbus RTU over the Pi 4's UART was fundamentally incompatible with the Sequent firmware bridge (v04.09) in a stacked configuration, the project moved to a **Modbus TCP translation layer** instead.

The gateway turns a Raspberry Pi into a high-performance Industrial Modbus Gateway, letting standard SCADA/HMI/vPLC software control I²C hardware over plain TCP — bypassing all serial-link issues.

---

## ✅ Implemented

### User-Defined Goals

- **vPLC Compatibility** — Standard Modbus TCP interface so that a vPLC (e.g. OpenPLC) can control the hardware without understanding I²C or Sequent-specific commands.
- **Full I/O Mapping** — Every physical input and output is accessible: 4-20 mA sensors, 0-10 V sensors, opto-isolated digital inputs, open-drain outputs, and the 16-relay bank.
- **Configuration Flexibility** — CLI options for network settings (`--host`, `--port`) and logic toggles (`--map-opto-to-reg`).
- **High Visibility** — Full heartbeat console output showing the state of every pin and sensor in real-time (every 5 s).

### Technical Enhancements

- **Modular Hardware Abstraction** — Class-based architecture (`IndustrialBoard`, `RelayBoard`) isolating the firmware quirks of each HAT.
- **I²C Bus Optimisation (State Caching)** — Relay and open-drain outputs track current state; an I²C write is only issued when the vPLC actually requests a change, preventing bus saturation.
- **Robust CLI Parsing** — Strips units (`mA`, `V`) and handles `out of range` / `error` responses so the vPLC only receives clean numeric data.
- **Integer Scaling** — Telemetry is scaled (mA × 100, V × 100) for high-precision monitoring within 16-bit Modbus registers.
- **Deterministic Timing** — 10 Hz loop with compensated sleep, critical for stable PLC timers and PID loops.

---

## 🔜 Planned

### P0 — Production Readiness

| Item | Description |
|---|---|
| **Systemd Service** | Create a `.service` unit file so the gateway starts on boot and auto-restarts on failure. |
| **I²C Bus Hardware Reset** | "Nuclear Reset" — toggle GPIO pins to clear a hung I²C bus without a full reboot. |

### P1 — Protocol & Addressing

| Item | Description |
|---|---|
| **Multi-Slave Addressing** | Split boards into separate Modbus Slave IDs (e.g. Relay board = Slave 1, Industrial board = Slave 2) for cleaner PLC mapping. |
| **Configurable Stack IDs** | CLI flags to set the stack ID for each board instead of hardcoded `0` / `1`. |

### P2 — Observability & Reliability

| Item | Description |
|---|---|
| **File Logging** | Rotating log file output alongside console logging. |
| **Health Endpoint** | Lightweight HTTP/JSON status endpoint for monitoring dashboards. |
| **Watchdog Timer** | Detect and recover from stalled I²C reads that exceed a timeout budget. |

### P3 — Extended I/O

| Item | Description |
|---|---|
| **Write-Back Registers** | Holding register writes for analog outputs (0-10 V out, if supported by HAT). |
| **Additional HAT Support** | Extend the abstraction layer to support other Sequent boards (e.g. 8-relay, building automation). |

---

## ❌ Abandoned

| Item | Reason |
|---|---|
| **Native Modbus RTU (Serial)** | Firmware bridge v04.09 and Pi 4 UART are fundamentally incompatible in the stacked configuration. Confirmed via deep-dive diagnostics. |
