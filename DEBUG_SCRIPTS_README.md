# Debug Scripts Documentation

## observe-board-install.sh

Comprehensive observer script for monitoring and debugging board definition installation.

### Features

- **Workspace board detection** - Checks `./boards` directory
- **Install location validation** - Verifies `/etc/sequent-gateway/boards`
- **Configuration file checks** - Validates TOML and env files
- **Binary installation verification** - Confirms binary exists and is executable
- **Systemd service status** - Checks service configuration
- **Emulation mode** - Simulates installed binary behavior with detailed diagnostics

### Usage

```bash
# Basic diagnostic check
./observe-board-install.sh

# Check-only mode (no installation)
./observe-board-install.sh --check-only

# Verbose output
./observe-board-install.sh --verbose

# Custom install path
./observe-board-install.sh --install-path /opt/sequent-gateway

# Emulate installed binary behavior (NEW!)
./observe-board-install.sh --emulate

# Full installation
sudo ./observe-board-install.sh
```

### Emulation Mode (--emulate)

The emulation mode simulates how the installed binary would access board definitions and provides detailed diagnostic reasoning for any access issues.

**What it checks:**

1. **Binary existence** - Verifies binary is installed at `/usr/local/bin/sequent-gateway`
2. **Executable permissions** - Confirms binary has execute permissions
3. **Boards directory existence** - Checks if board definitions are present
4. **Directory permissions** - Validates read access to boards directory
5. **TOML file accessibility** - Ensures all board files are readable
6. **Configuration file** - Checks `sequent-gateway.toml` and `boards_dir` setting
7. **Environment file** - Validates `BOARDS_DIR` in systemd env file
8. **Binary startup simulation** - Tests command invocation
9. **Common access issues** - I2C device access, SELinux/AppArmor

**Example output:**

```bash
[INFO] Emulating installed binary behavior...

[DIAG] Step 1: Binary existence check
[OK] Binary exists at: /usr/local/bin/sequent-gateway

[DIAG] Step 2: Executable permissions check
[OK] Binary is executable

[DIAG] Step 3: Boards directory existence check
[ERROR] Boards directory not found: /etc/sequent-gateway/boards
[DIAG] Reason: Board definitions have not been installed
[DIAG] Expected location: /etc/sequent-gateway/boards
[DIAG] Solution: Run: sudo ./observe-board-install.sh --install-path /etc/sequent-gateway
```

Each check provides:

- **Reason** - Why the issue occurred
- **Solution** - How to fix it

### Options

- `--install-path PATH` - Specify custom install path (default: `/etc/sequent-gateway`)
- `--verbose` - Enable verbose/debug output
- `--check-only` - Only check current state, don't perform installation
- `--emulate` - Simulate installed binary behavior and diagnose access issues
- `--help` - Show help message

## debug-board-reachability.sh

Deep debugging script for board definition validation and I²C bus scanning.

### Features

- **TOML parsing** - Extracts and validates board definitions
- **Protocol handler validation** - Checks `sequent_mcu` and `pca9535` compatibility
- **I²C bus scanning** - Detects connected devices (requires root)
- **TUI discovery simulation** - Shows what boards the TUI will find
- **Board summary reports** - Generates statistics and distributions

### Usage

Run all debug checks as root for I²C bus scanning

```bash
sudo ./debug-board-reachability.sh
```

## Common Workflows

### First-Time Installation

```bash
# 1. Check current state
./observe-board-install.sh --check-only

# 2. Install boards to system location
sudo ./observe-board-install.sh

# 3. Verify installation
./observe-board-install.sh --emulate

# 4. Debug board definitions
sudo ./debug-board-reachability.sh

# 5. Run TUI configuration
sudo sequent-gateway configure
```

### Debugging "Board Not Found" Issues

```bash
# 1. Emulate binary to see what's wrong
./observe-board-install.sh --emulate

# 2. Check board definitions
./debug-board-reachability.sh

# 3. Verify configuration
cat /etc/sequent-gateway/sequent-gateway.toml
```

### I²C Bus Troubleshooting

```bash
# 1. Scan I²C bus
sudo ./debug-board-reachability.sh

# 2. Manual scan
i2cdetect -y 1

# 3. Check device permissions
ls -la /dev/i2c-*
```

## File Locations

| Component | Development | Production |
|-----------|-------------|------------|
| Board Definitions | `./boards/` | `/etc/sequent-gateway/boards/` |
| Configuration | `./sequent-gateway.toml` | `/etc/sequent-gateway/sequent-gateway.toml` |
| Environment | - | `/etc/sequent-gateway/sequent-gateway.env` |
| Binary | `./target/release/sequent-gateway` | `/usr/local/bin/sequent-gateway` |
| Service | - | `/etc/systemd/system/sequent-gateway.service` |

## Troubleshooting

### "Workspace boards directory missing"

**Cause:** No `./boards` directory in current working directory AND no boards in `/etc/sequent-gateway/boards`

**Solution:**

```bash
# Option 1: Navigate to project root (where ./boards exists)
cd /path/to/SequentTCP

# Option 2: Install boards to system location
sudo ./observe-board-install.sh

# Option 3: The binary now searches both locations automatically!
# It will find boards in ./boards OR /etc/sequent-gateway/boards
```

**Note:** The binary now uses **multi-path search**:

1. Explicit `--boards-dir` (if specified)
2. Config file `boards_dir` (if specified)
3. Relative `./boards` (if exists)
4. `/etc/sequent-gateway/boards` (if exists)

This means boards will be found whether you're in development or production mode!

### "Binary not installed"

**Cause:** Binary not found at `/usr/local/bin/sequent-gateway`

**Solution:**

```bash
# Build and install
cargo build --release
sudo cp target/release/sequent-gateway /usr/local/bin/

# Or use TUI which offers auto-install
sudo sequent-gateway configure
```

### "boards_dir mismatch"

**Cause:** Configuration file points to different boards directory than expected

**Solution:**

```bash
# Check current config
cat /etc/sequent-gateway/sequent-gateway.toml | grep boards_dir

# Update config or use CLI flag
sequent-gateway --boards-dir /etc/sequent-gateway/boards
```

### "Permission denied on I2C"

**Cause:** User doesn't have access to I2C device

**Solution:**

```bash
# Add user to i2c group
sudo usermod -aG i2c $USER

# Or run as root (temporary)
sudo sequent-gateway

# Log out and back in for group change to take effect
```

## Output Format

The scripts use simple, clean output without fancy boxes:

- `[INFO]` - General information
- `[OK]` - Success/check passed
- `[WARN]` - Warning/issue detected
- `[ERROR]` - Error/failure
- `[DEBUG]` - Detailed debug info (with `--verbose`)
- `[DIAG]` - Diagnostic reasoning (in emulation mode)

## Integration with TUI

The scripts complement the TUI configuration wizard:

- Run `--check-only` before TUI to see current state
- Use `--emulate` after TUI to verify installation
- `debug-board-reachability.sh` helps debug TUI board selection issues
- Both scripts work with the TUI's `--install-boards` feature
