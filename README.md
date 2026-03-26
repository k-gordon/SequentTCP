# SequentTCP - Modbus TCP ↔ I²C Gateway
#### This implementation isn't stable in all ways, nor do all aspects function as described. Use this software at your own risk.

A high-performance Modbus TCP gateway for **Sequent Microsystems** Raspberry Pi HATs, written in Rust.  
It bridges Modbus TCP clients (SCADA, HMI, PLC) to the I²C-based Sequent hardware (relays, analog I/O, opto-isolated inputs, and open-drain outputs) over standard Modbus registers.

## Getting Started

The fastest way to set up the gateway is the **interactive configuration TUI**.
It walks you through board selection, addressing, and server settings, then writes a ready-to-use `sequent-gateway.toml` config file.


### Quick start install & configure (for latest release)


#### Arm64
```bash
wget https://github.com/k-gordon/SequentTCP/releases/latest/download/sequent-gateway-aarch64.zip && unzip sequent-gateway-aarch64.zip && sudo ./sequent-gateway configure
```
#### Armv7
```bash
wget https://github.com/k-gordon/SequentTCP/releases/latest/download/sequent-gateway-aarchv7.zip && unzip sequent-gateway-aarchv7.zip && sudo ./sequent-gateway configure
```
If the binary isn't installed to `/usr/local/bin/` yet, the TUI will detect this and offer to install it for you (copies the binary and board definitions to `/etc/sequent-gateway/`).  After install it re-launches from the system path automatically.

### What the TUI does

| Step | Screen | What you configure |
|------|--------|--------------------|
| 1 | Board Selection | Pick from all 34 board types (3 production + 31 experimental) |
| 2 | Board Config | Per-board I²C stack ID [0–7] and Modbus slave ID [1–247] |
| 3 | Server Settings | Host, port, health endpoint, addressing mode |
| 4 | I²C & Logging | Recovery thresholds, relay verification, log rotation |
| 5 | Review & Save | Preview the generated TOML, then write to disk |

### After configuration


#### Start the gateway using the config file
```bash
sudo sequent-gateway --config /etc/sequent-gateway/sequent-gateway.toml
```

Or, let the TUI handle everything:

The interactive configuration wizard (`sudo sequent-gateway config`) will prompt you to install and enable the systemd service after saving your config. It will copy the config file to the correct location and set up the service for you.


## Supported Hardware


| Board Name                              | Filename              |
|------------------------------------------|----------------------|
| **Sequent Mega-Industrial HAT**          | megaind.toml          |
| **Sequent 16-Relay HAT**                 | relay16.toml          |
| **Sequent 8-Relay HAT**                  | relay8.toml           |


## Experimental Board Definitions

> These TOML files are **experimental and untested** on real hardware. See boards/experimental/README.md for details.

The following boards are available in `boards/experimental/`:



| Board Name                              | Filename              | Board Name                              | Filename              |
|------------------------------------------|-----------------------|------------------------------------------|-----------------------|
| **Sequent 16-Input Industrial HAT**      | 16inpind_pca.toml     | **Sequent 16-Digital-Input HAT**        | 16inputs.toml         |
| **Sequent 16 Universal Input HAT**       | 16univin.toml         | **Sequent 16 Analog 0-10V Output HAT**  | 16uout.toml           |
| **Sequent 24-Bit 8-Voltage-Input HAT**   | 24b8vin.toml          | **Sequent 3-Relay Industrial HAT**      | 3relind.toml          |
| **Sequent 4-Relay 4-Input HAT**          | 4rel4in.toml          | **Sequent 4-Relay HAT**                 | 4relay.toml           |
| **Sequent 4-Relay Industrial MCU**       | 4relind_mcu.toml      | **Sequent 4-Relay Industrial PCA**      | 4relind_pca.toml      |
| **Sequent 8-Channel Relay Test**         | 8crt.toml             | **Sequent 8-Input MCU**                 | 8inputs_mcu.toml      |
| **Sequent 8-Input PCA**                  | 8inputs_pca.toml      | **Sequent 8-MOSFET**                    | 8mosfet.toml          |
| **Sequent 8-MOSIND MCU**                 | 8mosind_mcu.toml      | **Sequent 8-Relay HV**                  | 8relayhv.toml         |
| **Sequent Dash**                         | dash.toml             | **Sequent FSRC**                        | fsrc.toml             |
| **Sequent IOPlus**                       | ioplus.toml           | **Sequent LKit**                        | lkit.toml             |
| **Sequent MegaBas**                      | megabas.toml          | **Sequent MegaIO**                      | megaio.toml           |
| **Sequent MegaIO Industrial**            | megaioind.toml        | **Sequent MultiIO**                     | multiio.toml          |
| **Sequent PLCPI**                        | plcpi.toml            | **Sequent RTD**                         | rtd.toml              |
| **Sequent SmartFan**                     | smartfan.toml         | **Sequent SMTC**                        | smtc.toml             |
| **Sequent TI**                           | ti.toml               | **Sequent WDT**                         | wdt.toml              |

## Quick Start (manual / headless)

> **Prefer the TUI?** Run `sudo sequent-gateway configure` instead it handles everything below automatically.

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
> it installs the binary, board definitions, and writes the config file in one step.

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
