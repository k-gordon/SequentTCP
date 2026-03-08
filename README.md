# SequentTCP — Modbus TCP ↔ I²C Gateway

A high-performance Modbus TCP gateway for **Sequent Microsystems** Raspberry Pi HATs, written in Rust.  
It bridges Modbus TCP clients (SCADA, HMI, PLC) to the I²C-based Sequent hardware (relays, analog I/O, opto-isolated inputs, and open-drain outputs) over standard Modbus registers.

## Getting Started

The fastest way to set up the gateway is the **interactive configuration TUI**.
It walks you through board selection, addressing, and server settings — then
writes a ready-to-use `sequent-gateway.toml` config file.

```bash
# Build (first time only)
cd sequent-gateway
cargo build --release

# Launch the configuration wizard
sudo ./target/release/sequent-gateway configure
```

If the binary isn't installed to `/usr/local/bin/` yet, the TUI will detect
this and offer to install it for you (copies the binary and board definitions
to `/etc/sequent-gateway/`).  After install it re-launches from the system
path automatically.

### What the TUI does

| Step | Screen | What you configure |
|------|--------|--------------------|
| 1 | Board Selection | Pick from all 34 board types (3 production + 31 experimental) |
| 2 | Board Config | Per-board I²C stack ID [0–7] and Modbus slave ID [1–247] |
| 3 | Server Settings | Host, port, health endpoint, addressing mode |
| 4 | I²C & Logging | Recovery thresholds, relay verification, log rotation |
| 5 | Review & Save | Preview the generated TOML, then write to disk |

### After configuration

```bash
# Start the gateway using the config file
sudo sequent-gateway --config /etc/sequent-gateway/sequent-gateway.toml

# Or install as a systemd service (see below)
```

## Supported Hardware

| Board | Default Stack | `--board` flag |
|---|---|---|
| [Sequent Mega-Industrial HAT](https://sequentmicrosystems.com/) | 1 | `megaind` |
| [Sequent 16-Relay HAT](https://sequentmicrosystems.com/) | 0 | `relay16` |
| [Sequent 8-Relay HAT](https://sequentmicrosystems.com/) | 0 | `relay8` |

Plus **31 experimental board definitions** in `boards/experimental/` — the TUI
shows all of them with their capabilities.

## Modbus Memory Map

**Slave ID:** 1 (default, configurable via `--slave-id`)

| Register Type | Address | Description |
|---|---|---|
| **Coils** (R/W) | 0–15 | Relay Board - Relays 1–16 (or 1–8 for 8-Relay) |
| **Coils** (R/W) | 16–19 | Industrial Board - Open Drain Outputs 1–4 |
| **Discrete Inputs** (RO) | 0–7 | Industrial Board - Opto-Inputs 1–8 |
| **Holding Registers** (RO) | 0–7 | Industrial Board - 4-20 mA Inputs (mA × 100) |
| **Holding Registers** (RO) | 8 | Industrial Board - PSU Voltage (V × 100) |
| **Holding Registers** (RO) | 10–13 | Industrial Board - 0-10 V Inputs (V × 100) |
| **Holding Registers** (R/W) | 16–19 | Industrial Board - 0-10 V Outputs (V × 100) |
| **Holding Registers** (R/W) | 20–23 | Industrial Board - 4-20 mA Outputs (mA × 100) |
| **Holding Registers** (RO) | 24 | Relay read-back bitmask (diagnostic, updated every `--relay-verify-interval` ticks) |

## Quick Start (manual / headless)

> **Prefer the TUI?** Run `sudo sequent-gateway configure` instead — it handles
> everything below automatically.

### Prerequisites

- Raspberry Pi with Sequent HATs installed and I²C enabled
- If compiling from source, Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

### Build & Run

```bash
cd sequent-gateway
cargo build --release
sudo ./target/release/sequent-gateway --host 0.0.0.0 --port 502 --ind-stack 1 --relay-stack 0
```

### Board Selection

```bash
# Default: megaind + relay16
sudo ./target/release/sequent-gateway

# Explicit board selection (repeatable)
sudo ./target/release/sequent-gateway --board megaind --board relay8

# Only the relay board, no industrial HAT
sudo ./target/release/sequent-gateway --board relay16
```

### CLI Options

> Most of these are set automatically by the TUI wizard.  You only need
> CLI flags for headless / scriptable deployments.

| Flag | Default | Description |
|---|---|---|
| `--config` | auto-detect | Path to `sequent-gateway.toml` config file |
| `--host` | `0.0.0.0` | IP address to bind |
| `--port` | `502` | Modbus TCP port |
| `--ind-stack` | `1` | Industrial HAT I²C stack ID |
| `--relay-stack` | `0` | Relay HAT I²C stack ID |
| `--board` | `megaind,relay16` | Board types to load (repeatable) |
| `--health-port` | *(disabled)* | HTTP health endpoint port |
| `--log-file` | *(none)* | Path for daily-rotated log files |
| `--single-slave` | `false` | Flat Modbus addressing mode |

### Install as a systemd Service

> **Quickest path:** Run `sudo sequent-gateway configure --install-boards /etc/sequent-gateway/boards`
> — it installs the binary, board definitions, and writes the config file in one step.

Manual install:

```bash
# Install binary
sudo cp target/release/sequent-gateway /usr/local/bin/

# Install config & boards
sudo mkdir -p /etc/sequent-gateway
sudo cp sequent-gateway.toml /etc/sequent-gateway/
sudo cp -r boards/ /etc/sequent-gateway/boards/

# Install and start service
sudo cp deploy/sequent-gateway.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now sequent-gateway

# Check status / logs
sudo systemctl status sequent-gateway
sudo journalctl -u sequent-gateway -f
```

### Health Endpoint

```bash
curl http://localhost:8080/health
# {"status":"ok","uptime_s":1234,"last_cycle_ms":0.42,"i2c_errors":0,"i2c_recoveries":0,"relay_mismatches":0,"channels":{...}}
```

### Hardware Validation (on-Pi)

The gateway includes a self-contained `validate` subcommand that exercises live
hardware and produces a structured PASS/FAIL report - no Python or external
tools required.

```bash
# Interactive board picker:
sudo ./target/release/sequent-gateway validate

# Explicit board selection:
sudo ./target/release/sequent-gateway validate --board megaind --board relay16

# Skip relay/OD/analog writes (safe for live equipment):
sudo ./target/release/sequent-gateway validate --skip-writes
```

The report maps directly to Story 10 and Epic 2 acceptance criteria.
Copy-paste the output to report results.

## Architecture

```
┌──────────────────────────────────────────────────┐
│                  Rust Binary                     │
│                                                  │
│  ┌────────────┐   ┌───────────────────────────┐  │
│  │  Modbus    │   │  I²C HAL Layer            │  │
│  │  TCP       │◄─►│                           │  │
│  │  Server    │   │  ┌─────────┐ ┌──────────┐ │  │
│  │            │   │  │ MegaInd │ │ Relay    │ │  │
│  │            │   │  │ Board   │ │ Board    │ │  │
│  └────────────┘   │  └────┬────┘ └────┬─────┘ │  │
│                   │       │           │       │  │
│  ┌────────────┐   │    /dev/i2c-1     │       │  │
│  │  Health    │   └───────────────────────────┘  │
│  │  HTTP      │                                  │
│  └────────────┘                                  │
└──────────────────────────────────────────────────┘
         ▲                  │
   Modbus TCP          I²C Bus
   (SCADA/vPLC)        (Sequent HATs)
```

The gateway runs a 10 Hz polling loop with direct I²C register access (< 1 ms per cycle):

1. **Read** analog & digital inputs via I²C HAL
2. **Update** the Modbus data bank (holding registers, discrete inputs)
3. **Apply** coil/register writes to relay, OD, and analog outputs
4. **Log** a heartbeat summary every 5 seconds

### Key Features

- **Direct I²C** - no subprocess shelling, < 1 ms I/O cycle
- **Write-on-change caching** - only touches the bus when outputs actually change
- **Analog output write-back** - 0-10 V and 4-20 mA outputs via holding registers
- **Multi-slave addressing** - route boards to different Modbus unit IDs
- **I²C bus recovery** - automatic GPIO-level reset on hung bus
- **Channel watchdog** - per-channel health tracking with last-known-good fallback
- **Rotating file logs** - structured tracing with optional log directory
- **Health endpoint** - lightweight HTTP/JSON status for monitoring dashboards
- **Dynamic board selection** - `SequentBoard` trait for runtime HAL introspection
- **Single static binary** - no runtime dependencies on the Pi

## Roadmap

See [STORIES.md](STORIES.md) for the project history and completed milestones.

## License

[MIT](LICENSE)
