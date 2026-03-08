# Roadmap

## Background

The gateway is the result of a significant architectural pivot. After deep-dive diagnostics proved that native Modbus RTU over the Pi 4's UART was fundamentally incompatible with the Sequent firmware bridge (v04.09) in a stacked configuration, the project moved to a **Modbus TCP translation layer** instead.

The gateway turns a Raspberry Pi into a high-performance Industrial Modbus Gateway, letting standard SCADA/HMI/vPLC software control I²C hardware over plain TCP — bypassing all serial-link issues.

---

## Implemented

### Goals

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

## Completed — Rust Rewrite

> The original Python proof-of-concept has been **removed**. All phases below are implemented in the Rust binary (`sequent-gateway/`). See [STORIES.md](STORIES.md) for the detailed acceptance criteria.

### Phase 0 — Native I²C Rewrite

> The `subprocess` → CLI-tool bottleneck has been eliminated. The Rust gateway talks directly to the I²C bus using the register map from Sequent's `megaind.h` — achieving < 1 ms I/O cycles.

#### Reference Material

Sequent's [`megaind-rpi`](https://github.com/SequentMicrosystems/megaind-rpi) repository contains all the information needed:

| Source | What it tells us |
|---|---|
| [`src/comm.c`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/comm.c) | Raw I²C transport — `open("/dev/i2c-1")`, `ioctl(I2C_SLAVE, addr)`, then `read()`/`write()` with a 1-byte register prefix. |
| [`src/megaind.h`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/megaind.h) | Full register map enum (0x00–0xFF): relay set/clr, opto inputs, analog I/O, OD PWM, RTC, watchdog, 1-Wire, calibration. |
| [`src/analog.c`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/analog.c) | `val16Get()` / `val16Set()` — how 16-bit analog values are read/written (little-endian, millivolt scaling). |
| [`python/megaind/__init__.py`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/python/megaind/__init__.py) | Sequent's Python library using `smbus2` for direct I²C — already validated as working in our stack. |

#### Decision: Rust

The register map and I²C access patterns were validated against the hardware early on. The project went directly to a full Rust implementation.

**Why Rust:**
- Single static binary — no runtime dependencies on the target Pi
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
| [`tracing`](https://crates.io/crates/tracing) + [`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) | Structured logging |

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
│                   │       │           │       │  │
│                   │    /dev/i2c-1     │       │  │
│                   └───────────────────────────┘  │
└──────────────────────────────────────────────────┘
         ▲                  │
   Modbus TCP          I²C Bus
   (SCADA/vPLC)        (Sequent HATs)
```

#### Milestone Checklist

- [x] Scaffold Rust project (`cargo init sequent-gateway`)
- [x] Port I²C register map from `megaind.h` → `src/registers.rs` (`#[repr(u8)]` enum)
- [x] Implement I²C HAL: `MegaIndBoard` struct wrapping `i2cdev` for the Industrial HAT
- [x] Implement I²C HAL: `RelayBoard` struct wrapping `i2cdev` for the 16-Relay HAT
- [x] Implement state-caching layer (only write on change)
- [x] Custom Modbus TCP server with MBAP framing (FC 01/02/03/05/06/0F/10)
- [x] Wire up the 10 Hz poll loop (read inputs → update data bank → apply coil writes)
- [x] Add heartbeat logging via `tracing` (match current console output format)
- [x] Add `clap` CLI (`--host`, `--port`, `--ind-stack`, `--relay-stack`, `--board`, etc.)
- [x] Cross-compile and validate on Raspberry Pi against known-good output
- [x] Create `systemd` unit file for single-binary deployment
- [x] Sub-millisecond full I/O cycle achieved

---

### P1 — Production Readiness

| Item | Status | Description |
|---|---|---|
| **Systemd Service** | Done | `deploy/sequent-gateway.service` — auto-start on boot with restart-on-failure |
| **I²C Bus Recovery** | Done | `i2c_recovery.rs` — GPIO-level SDA/SCL toggle to clear hung bus |

### P2 — Protocol & Addressing

| Item | Status | Description |
|---|---|---|
| **Multi-Slave Addressing** | Done | `slave_map.rs` — route boards to separate Modbus unit IDs |
| **Configurable Stack IDs** | Done | `--ind-stack` / `--relay-stack` CLI flags |

### P3 — Observability & Reliability

| Item | Status | Description |
|---|---|---|
| **Rotating File Logs** | Done | `tracing-appender` with `--log-dir` flag |
| **Health Endpoint** | Done | `health.rs` — HTTP/JSON on `--health-port` (lock-free atomics) |
| **Channel Watchdog** | Done | `channel_watchdog.rs` — per-channel timeout with last-known-good fallback |

### P4 — Extended I/O

| Item | Status | Description |
|---|---|---|
| **Analog Output Write-Back** | Done | FC 0x06/0x10 for 0-10V (HR 16-19) and 4-20mA (HR 20-23) outputs |
| **Additional HAT Support** | Done | `SequentBoard` trait + `--board` flag + 8-Relay HAT support |

---

##  Abandoned

| Item | Reason |
|---|---|
| **Native Modbus RTU (Serial)** | Firmware bridge v04.09 and Pi 4 UART are fundamentally incompatible in the stacked configuration. Confirmed via deep-dive diagnostics. |
