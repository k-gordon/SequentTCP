# SequentTCP — Modbus TCP ↔ I²C Gateway

A high-performance Modbus TCP gateway for **Sequent Microsystems** Raspberry Pi HATs, written in Rust.  
It bridges Modbus TCP clients (SCADA, HMI, PLC) to the I²C-based Sequent hardware — relays, analog I/O, opto-isolated inputs, and open-drain outputs — over standard Modbus registers.

> **Note:** The original Python proof-of-concept (`modbusTCP.py`) is **deprecated**.
> All new development targets the Rust binary in `sequent-gateway/`.

## Supported Hardware

| Board | Stack ID | `--board` flag |
|---|---|---|
| [Sequent Mega-Industrial HAT](https://sequentmicrosystems.com/) | 1 (default) | `megaind` |
| [Sequent 16-Relay HAT](https://sequentmicrosystems.com/) | 0 (default) | `relay16` |
| [Sequent 8-Relay HAT](https://sequentmicrosystems.com/) | 0 | `relay8` |

## Modbus Memory Map

**Slave ID:** 1 (default, configurable via `--slave-id`)

| Register Type | Address | Description |
|---|---|---|
| **Coils** (R/W) | 0–15 | Relay Board — Relays 1–16 (or 1–8 for 8-Relay) |
| **Coils** (R/W) | 16–19 | Industrial Board — Open Drain Outputs 1–4 |
| **Discrete Inputs** (RO) | 0–7 | Industrial Board — Opto-Inputs 1–8 |
| **Holding Registers** (RO) | 0–7 | Industrial Board — 4-20 mA Inputs (mA × 100) |
| **Holding Registers** (RO) | 8 | Industrial Board — PSU Voltage (V × 100) |
| **Holding Registers** (RO) | 10–13 | Industrial Board — 0-10 V Inputs (V × 100) |
| **Holding Registers** (R/W) | 16–19 | Industrial Board — 0-10 V Outputs (V × 100) |
| **Holding Registers** (R/W) | 20–23 | Industrial Board — 4-20 mA Outputs (mA × 100) |

## Quick Start

### Prerequisites

- Raspberry Pi with Sequent HATs installed and I²C enabled
- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

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

| Flag | Default | Description |
|---|---|---|
| `--host` | `0.0.0.0` | IP address to bind |
| `--port` | `502` | Modbus TCP port |
| `--ind-stack` | `1` | Industrial HAT I²C stack ID |
| `--relay-stack` | `0` | Relay HAT I²C stack ID |
| `--slave-id` | `1` | Modbus slave/unit ID |
| `--board` | `megaind,relay16` | Board types to load (repeatable) |
| `--health-port` | `8080` | HTTP health endpoint port |
| `--log-dir` | (none) | Directory for rotating log files |

### Install as a systemd Service

```bash
# Install binary
sudo cp target/release/sequent-gateway /usr/local/bin/

# Install config
sudo mkdir -p /etc/sequent-gateway
sudo cp deploy/sequent-gateway.env /etc/sequent-gateway/
sudo cp -r boards/ /etc/sequent-gateway/boards/

# Edit configuration to match your hardware
sudo nano /etc/sequent-gateway/sequent-gateway.env

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
# {"status":"ok","uptime_s":1234,"last_cycle_ms":0.42,"i2c_errors":0,"channels":{...}}
```

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
│                   │       │           │        │  │
│  ┌────────────┐   │    /dev/i2c-1     │        │  │
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

- **Direct I²C** — no subprocess shelling, < 1 ms I/O cycle
- **Write-on-change caching** — only touches the bus when outputs actually change
- **Analog output write-back** — 0-10 V and 4-20 mA outputs via holding registers
- **Multi-slave addressing** — route boards to different Modbus unit IDs
- **I²C bus recovery** — automatic GPIO-level reset on hung bus
- **Channel watchdog** — per-channel health tracking with last-known-good fallback
- **Rotating file logs** — structured tracing with optional log directory
- **Health endpoint** — lightweight HTTP/JSON status for monitoring dashboards
- **Dynamic board selection** — `SequentBoard` trait for runtime HAL introspection
- **Single static binary** — no Python runtime, no pip, no venv on the Pi

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the project history and completed milestones.

<details>
<summary>📜 Legacy Python Gateway (deprecated)</summary>

The original proof-of-concept (`legacy/modbusTCP.py`) used `pyModbusTCP` and shelled out
to Sequent's CLI tools (`megaind`, `16relind`) via `subprocess`. It validated the
concept but added ~100 ms latency per I/O call and had no analog output support.

The file is retained in the `legacy/` folder for historical reference only.
**Do not use it for new deployments.**

If you still need it for some reason:

```bash
python3 -m venv venv && source venv/bin/activate
pip install pyModbusTCP
sudo ./venv/bin/python3 legacy/modbusTCP.py
```

</details>

## License

[MIT](LICENSE)
