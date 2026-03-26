# Board Installation Debugging Scripts

This directory contains helper scripts for debugging board definition installation and reachability in the SequentTCP gateway.

## Scripts Overview

### 1. `observe-board-install.sh`

A comprehensive observer script for monitoring and debugging the installation of board definitions to system locations.

**Features:**

- Checks workspace board definitions
- Validates install location (`/etc/sequent-gateway/boards`)
- Verifies configuration files
- Checks binary installation
- Validates systemd service status
- Generates diagnostic reports

**Usage:**

```bash
# Basic diagnostic check
./observe-board-install.sh

# Check-only mode (no installation)
./observe-board-install.sh --check-only

# Verbose output
./observe-board-install.sh --verbose

# Custom install path
./observe-board-install.sh --install-path /opt/sequent-gateway

# Full installation
sudo ./observe-board-install.sh
```

**Options:**

- `--install-path PATH`: Specify custom install path (default: `/etc/sequent-gateway`)
- `--verbose`: Enable verbose/debug output
- `--check-only`: Only check current state, don't perform installation
- `--help`: Show help message

### 2. `debug-board-reachability.sh`

A detailed debugging script for deep inspection of board definitions and their reachability.

**Features:**

- Parses and validates TOML board definitions
- Checks I²C bus accessibility
- Tests TUI board discovery mechanism
- Validates protocol handler compatibility
- Generates board summary reports

**Usage:**

```bash
# Run all debug checks
./debug-board-reachability.sh

# As root for I²C bus scanning
sudo ./debug-board-reachability.sh
```

## When to Use These Scripts

### First-Time Installation

1. Run `./observe-board-install.sh --check-only` to see current state
2. Run `sudo ./observe-board-install.sh` to install boards to system location
3. Verify with `./observe-board-install.sh --verbose`

### Debugging TUI Board Discovery

1. Run `./debug-board-reachability.sh` to see what boards the TUI will discover
2. Check that all expected board TOML files are present
3. Validate board definitions for syntax errors

### I²C Bus Issues

1. Run `sudo ./debug-board-reachability.sh` to scan I²C bus
2. Verify that board addresses match expected ranges
3. Check for address conflicts

### Production Deployment

1. Use `observe-board-install.sh` to verify installation
2. Check configuration files are in correct locations
3. Verify systemd service status

## Diagnostic Report Sections

### Workspace Status

- Checks if `./boards` directory exists
- Counts board TOML files
- Lists experimental boards

### Install Location Status

- Checks `/etc/sequent-gateway/boards` existence
- Verifies file permissions
- Counts installed boards

### Configuration Status

- Validates `sequent-gateway.toml`
- Checks `boards_dir` configuration
- Verifies paths match install location

### Environment Status

- Checks `sequent-gateway.env`
- Validates `BOARDS_DIR` setting
- Ensures systemd compatibility

### Binary Status

- Verifies binary installation at `/usr/local/bin/sequent-gateway`
- Checks executable permissions
- Tests binary version info

### Service Status

- Checks systemd service registration
- Verifies service active state
- Validates service configuration

## Board TOML Validation

The scripts validate board TOML files for:

- `[board]` section presence
- `name` field
- `protocol` field (`sequent_mcu` or `pca9535`)
- `[address]` section
- `base` address
- `mode` (address calculation method)
- Optional sections: `[channels]`, `[registers]`, `[pca9535]`

## I²C Bus Scanning

When run as root, the debug script scans I²C bus 1 for:

- PCA9535 devices (0x20-0x27)
- MegaInd devices (0x50-0x57)
- Other common Sequent board addresses

## Troubleshooting

### "No board TOML files found" Error

1. Run `./observe-board-install.sh --check-only`
2. Verify boards exist in `./boards` or `/etc/sequent-gateway/boards`
3. Run `sudo ./observe-board-install.sh` to install

### TUI Can't Find Boards

1. Run `./debug-board-reachability.sh`
2. Check both workspace and install directories
3. Verify file permissions (should be readable)

### I²C Address Conflicts

1. Run `sudo ./debug-board-reachability.sh`
2. Check I²C bus scan output
3. Verify stack IDs don't overlap
4. Adjust board configurations if needed

### Protocol Handler Mismatch

1. Run `./debug-board-reachability.sh`
2. Check protocol distribution
3. Verify protocol matches board hardware
4. Update TOML if incorrect

## Integration with TUI

These scripts complement the TUI configuration wizard:

- `observe-board-install.sh` can be run before/after TUI configuration
- `debug-board-reachability.sh` helps debug TUI board selection issues
- Both scripts work with the TUI's `--install-boards` feature

## Example Workflow

```bash
# 1. Initial check
./observe-board-install.sh --check-only

# 2. Install boards to system location
sudo ./observe-board-install.sh

# 3. Debug and validate
sudo ./debug-board-reachability.sh

# 4. Run TUI configuration
sudo sequent-gateway configure

# 5. Verify installation
./observe-board-install.sh --verbose

# 6. Start gateway
sudo systemctl start sequent-gateway

# 7. Monitor logs
sudo journalctl -u sequent-gateway -f
```

## File Locations

| Component | Development | Production |
|-----------|-------------|------------|
| Board Definitions | `./boards/` | `/etc/sequent-gateway/boards/` |
| Configuration | `./sequent-gateway.toml` | `/etc/sequent-gateway/sequent-gateway.toml` |
| Environment | - | `/etc/sequent-gateway/sequent-gateway.env` |
| Binary | `./target/release/sequent-gateway` | `/usr/local/bin/sequent-gateway` |
| Service | - | `/etc/systemd/system/sequent-gateway.service` |

## Notes

- Some features require root permissions
