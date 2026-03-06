# SequentTCP — Modbus TCP ↔ I²C Gateway

A Modbus TCP gateway for **Sequent Microsystems** Raspberry Pi HATs.  
It bridges Modbus TCP clients (SCADA, HMI, PLC) to the I²C-based Sequent hardware, exposing relays, analog inputs, opto-isolated inputs, and open-drain outputs over standard Modbus registers.

## Supported Hardware

| Board | Stack ID | CLI Tool |
|---|---|---|
| [Sequent 16-Relay HAT](https://sequentmicrosystems.com/) | 0 | `16relind` |
| [Sequent Mega-Industrial HAT](https://sequentmicrosystems.com/) | 1 | `megaind` |

## Modbus Memory Map

**Slave ID:** 1 (or Any)

| Register Type | Address | Description |
|---|---|---|
| **Coils** (R/W) | 0–15 | Relay Board — Relays 1–16 |
| **Coils** (R/W) | 16–19 | Industrial Board — Open Drain Outputs 1–4 |
| **Discrete Inputs** (RO) | 0–7 | Industrial Board — Opto-Inputs 1–8 |
| **Holding Registers** (RO) | 0–7 | Industrial Board — 4-20 mA Inputs 1–8 (mA × 100) |
| **Holding Registers** (RO) | 8 | Industrial Board — PSU Voltage (V × 100) |
| **Holding Registers** (RO) | 10–13 | Industrial Board — 0-10 V Inputs 1–4 (V × 100) |
| **Holding Registers** (RO) | 15 | Opto-Input Bitmask (0–255) — only with `--map-opto-to-reg` |

## Quick Start

### Option A: Rust Gateway (recommended)

#### Prerequisites

- Raspberry Pi with Sequent HATs installed and I²C enabled
- Rust toolchain (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

#### Build & Run

```bash
cd sequent-gateway
cargo build --release
sudo ./target/release/sequent-gateway --host 0.0.0.0 --port 502 --ind-stack 1 --relay-stack 0
```

#### Install as a systemd Service

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

### Option B: Python Gateway (legacy PoC)

#### Prerequisites

- Raspberry Pi with Sequent HATs installed and I²C enabled
- Sequent CLI tools installed (`megaind`, `16relind`)
- Python 3.7+

### Installation

```bash
# Clone the repo
git clone https://github.com/<your-user>/SequentTCP.git
cd SequentTCP

# Create virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt
```

### Usage

```bash
# Run with defaults (0.0.0.0:502, requires root for I²C)
sudo ./venv/bin/python3 modbusTCP.py

# Custom host & port
sudo ./venv/bin/python3 modbusTCP.py --host 192.168.1.100 --port 5020

# Also expose opto-inputs as a holding register bitmask
sudo ./venv/bin/python3 modbusTCP.py --map-opto-to-reg
```

### CLI Options

| Flag | Default | Description |
|---|---|---|
| `--host` | `0.0.0.0` | IP address to bind |
| `--port` | `502` | Modbus TCP port |
| `--map-opto-to-reg` | off | Mirror opto-input bitmask to Holding Register 15 |

## Architecture

```
Modbus TCP Client  ←→  pyModbusTCP Server  ←→  subprocess (megaind / 16relind)  ←→  I²C Bus  ←→  HATs
```

The gateway runs a 10 Hz polling loop:

1. **Read** analog & digital inputs from hardware via CLI tools
2. **Update** the Modbus data bank (holding registers, discrete inputs)
3. **Apply** coil writes to relay and open-drain outputs
4. **Log** a heartbeat summary every 5 seconds

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full project roadmap — what's implemented, what's planned, and what was abandoned.

## License

[MIT](LICENSE)
