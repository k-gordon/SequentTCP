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

### 🏁 Phase 0 — Native I²C Rewrite (Immediate Next Step)

> **Goal:** Eliminate the `subprocess` → CLI-tool bottleneck. The current Python gateway shells out to `megaind` and `16relind` on every read/write cycle — parsing their stdout for values. This works as a proof-of-concept but adds ~50–100 ms of latency per I/O call, limits error handling, and creates a fragile dependency on CLI output formatting.
>
> The rewrite will talk directly to the I²C bus using the same register map that Sequent's own tools use internally.

#### Reference Material

Sequent's [`megaind-rpi`](https://github.com/SequentMicrosystems/megaind-rpi) repository contains all the information needed:

| Source | What it tells us |
|---|---|
| [`src/comm.c`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/comm.c) | Raw I²C transport — `open("/dev/i2c-1")`, `ioctl(I2C_SLAVE, addr)`, then `read()`/`write()` with a 1-byte register prefix. |
| [`src/megaind.h`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/megaind.h) | Full register map enum (0x00–0xFF): relay set/clr, opto inputs, analog I/O, OD PWM, RTC, watchdog, 1-Wire, calibration. |
| [`src/analog.c`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/analog.c) | `val16Get()` / `val16Set()` — how 16-bit analog values are read/written (little-endian, millivolt scaling). |
| [`python/megaind/__init__.py`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/python/megaind/__init__.py) | Sequent's Python library using `smbus2` for direct I²C — already validated as working in our stack. |

#### Decision: Straight to Rust

The Python + `smbus2` approach has already been validated against the hardware — the register map and I²C access patterns are confirmed working. There is no need for a Python stepping stone. The project will move directly to a full Rust implementation.

**Why Rust:**
- Single static binary — no Python runtime, no pip, no venv on the target Pi
- Memory-safe with zero-cost abstractions; no GC pauses in a 10 Hz control loop
- Excellent `i2cdev` crate maps directly to `/dev/i2c-*` with safe Rust types
- Cross-compile to `armv7`/`aarch64` trivially via `cross` or `cargo-zigbuild`
- Sequent's C register map (`megaind.h` enum) ports 1:1 into a Rust `#[repr(u8)]` enum
- `tokio-modbus` or `rodbus` for async Modbus TCP server
- Native `systemd` notify support — no wrapper scripts
- `clap` for CLI parsing (mirrors the current `argparse` interface)

#### Rust Crate Stack

| Crate | Role |
|---|---|
| [`i2cdev`](https://crates.io/crates/i2cdev) | I²C bus access via Linux `/dev/i2c-*` |
| [`tokio-modbus`](https://crates.io/crates/tokio-modbus) | Async Modbus TCP server |
| [`tokio`](https://crates.io/crates/tokio) | Async runtime (timer, TCP, signal handling) |
| [`clap`](https://crates.io/crates/clap) | CLI argument parsing |
| [`tracing`](https://crates.io/crates/tracing) + [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) | Structured logging (replaces Python `logging`) |

#### Architecture

```
┌──────────────────────────────────────────────────┐
│                  Rust Binary                     │
│                                                  │
│  ┌────────────┐   ┌───────────────────────────┐  │
│  │  Modbus    │   │  I²C HAL Layer            │  │
│  │  TCP       │◄─►│                           │  │
│  │  Server    │   │  ┌─────────┐ ┌──────────┐ │  │
│  │ (tokio-    │   │  │ MegaInd │ │ 16RelInd │ │  │
│  │  modbus)   │   │  │ Regs    │ │ Regs     │ │  │
│  └────────────┘   │  └────┬────┘ └────┬─────┘ │  │
│                   │       │           │        │  │
│                   │    /dev/i2c-1     │        │  │
│                   └───────────────────────────┘  │
└──────────────────────────────────────────────────┘
         ▲                  │
   Modbus TCP          I²C Bus
   (SCADA/vPLC)        (Sequent HATs)
```

#### Milestone Checklist

- [ ] Scaffold Rust project (`cargo init sequent-gateway`)
- [ ] Port I²C register map from `megaind.h` → `src/registers.rs` (`#[repr(u8)]` enum)
- [ ] Implement I²C HAL: `MegaIndBoard` struct wrapping `i2cdev` for the Industrial HAT
- [ ] Implement I²C HAL: `RelayBoard` struct wrapping `i2cdev` for the 16-Relay HAT
- [ ] Implement state-caching layer (only write on change, matching current Python behaviour)
- [ ] Integrate `tokio-modbus` TCP server with the Modbus memory map
- [ ] Wire up the 10 Hz poll loop (read inputs → update data bank → apply coil writes)
- [ ] Add heartbeat logging via `tracing` (match current console output format)
- [ ] Add `clap` CLI (`--host`, `--port`, `--map-opto-to-reg`, `--ind-stack`, `--relay-stack`)
- [ ] Cross-compile and validate on Raspberry Pi against known-good Python output
- [ ] Create `systemd` unit file for single-binary deployment
- [ ] Benchmark: target < 1 ms full I/O cycle (vs ~100+ ms with subprocess)

---

### P1 — Production Readiness

| Item | Description |
|---|---|
| **Systemd Service** | Create a `.service` unit file so the gateway starts on boot and auto-restarts on failure. |
| **I²C Bus Hardware Reset** | "Nuclear Reset" — toggle GPIO pins to clear a hung I²C bus without a full reboot. |

### P2 — Protocol & Addressing

| Item | Description |
|---|---|
| **Multi-Slave Addressing** | Split boards into separate Modbus Slave IDs (e.g. Relay board = Slave 1, Industrial board = Slave 2) for cleaner PLC mapping. |
| **Configurable Stack IDs** | CLI flags to set the stack ID for each board instead of hardcoded `0` / `1`. |

### P3 — Observability & Reliability

| Item | Description |
|---|---|
| **File Logging** | Rotating log file output alongside console logging. |
| **Health Endpoint** | Lightweight HTTP/JSON status endpoint for monitoring dashboards. |
| **Watchdog Timer** | Detect and recover from stalled I²C reads that exceed a timeout budget. |

### P4 — Extended I/O

| Item | Description |
|---|---|
| **Write-Back Registers** | Holding register writes for analog outputs (0-10 V out, if supported by HAT). |
| **Additional HAT Support** | Extend the abstraction layer to support other Sequent boards (e.g. 8-relay, building automation). |

---

## ❌ Abandoned

| Item | Reason |
|---|---|
| **Native Modbus RTU (Serial)** | Firmware bridge v04.09 and Pi 4 UART are fundamentally incompatible in the stacked configuration. Confirmed via deep-dive diagnostics. |
