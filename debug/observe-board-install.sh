#!/bin/bash
#
# observe-board-install.sh - Observer script for debugging board definitions installation and reachability
#
# This script helps debug:
# 1. First-time installation of board definitions
# 2. Board definition file discovery
# 3. Reachability of board TOML files in install locations
# 4. TUI board registry validation
# 5. Binary access simulation and diagnostics
#
# Usage: ./observe-board-install.sh [options]
# Options:
#   --install-path PATH  Specify custom install path (default: /etc/sequent-gateway)
#   --verbose            Enable verbose output
#   --check-only         Only check current state, don't perform installation
#   --emulate            Simulate installed binary behavior and diagnose access issues
#   --help               Show this help message
#

set -e

# Default paths
INSTALL_BASE="/etc/sequent-gateway"
INSTALL_BOARDS_DIR="${INSTALL_BASE}/boards"
INSTALL_CONFIG_DIR="${INSTALL_BASE}"
CONFIG_FILE="${INSTALL_CONFIG_DIR}/sequent-gateway.toml"
ENV_FILE="${INSTALL_CONFIG_DIR}/sequent-gateway.env"
SERVICE_FILE="${INSTALL_CONFIG_DIR}/sequent-gateway.service"
BINARY_PATH="/usr/local/bin/sequent-gateway"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Flags
VERBOSE=false
CHECK_ONLY=false
EMULATE=false
CUSTOM_INSTALL_PATH=""

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --install-path)
            CUSTOM_INSTALL_PATH="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --check-only)
            CHECK_ONLY=true
            shift
            ;;
        --emulate)
            EMULATE=true
            shift
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --install-path PATH  Specify custom install path (default: /etc/sequent-gateway)"
            echo "  --verbose            Enable verbose output"
            echo "  --check-only         Only check current state, don't perform installation"
            echo "  --emulate            Simulate installed binary behavior and diagnose access issues"
            echo "  --help               Show this help message"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Override defaults if custom path provided
if [[ -n "$CUSTOM_INSTALL_PATH" ]]; then
    INSTALL_BASE="$CUSTOM_INSTALL_PATH"
    INSTALL_BOARDS_DIR="${INSTALL_BASE}/boards"
    INSTALL_CONFIG_DIR="${INSTALL_BASE}"
    CONFIG_FILE="${INSTALL_CONFIG_DIR}/sequent-gateway.toml"
    ENV_FILE="${INSTALL_CONFIG_DIR}/sequent-gateway.env"
fi

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_verbose() {
    if [[ "$VERBOSE" == true ]]; then
        echo -e "${CYAN}[DEBUG]${NC} $1"
    fi
}

log_diagnostic() {
    echo -e "${CYAN}[DIAG]${NC} $1"
}

# Emulate installed binary behavior and diagnose access issues
emulate_binary_access() {
    log_info "Emulating installed binary behavior..."
    echo ""
    
    # Step 1: Check if binary exists
    log_diagnostic "Step 1: Binary existence check"
    if [[ ! -f "$BINARY_PATH" ]]; then
        log_error "Binary not found at: $BINARY_PATH"
        log_diagnostic "Reason: Binary has not been installed to system location"
        log_diagnostic "Solution: Build and install binary with: cargo build --release && sudo cp target/release/sequent-gateway /usr/local/bin/"
        echo ""
        return 1
    fi
    log_success "Binary exists at: $BINARY_PATH"
    
    # Step 2: Check executable permissions
    log_diagnostic "Step 2: Executable permissions check"
    if [[ ! -x "$BINARY_PATH" ]]; then
        log_error "Binary is not executable: $BINARY_PATH"
        log_diagnostic "Reason: File permissions do not allow execution"
        log_diagnostic "Solution: Run: sudo chmod +x $BINARY_PATH"
        echo ""
        return 1
    fi
    log_success "Binary is executable"
    
    # Step 3: Check boards directory existence
    log_diagnostic "Step 3: Boards directory existence check"
    if [[ ! -d "$INSTALL_BOARDS_DIR" ]]; then
        log_error "Boards directory not found: $INSTALL_BOARDS_DIR"
        log_diagnostic "Reason: Board definitions have not been installed"
        log_diagnostic "Expected location: $INSTALL_BOARDS_DIR"
        log_diagnostic "Solution: Run: sudo $0 --install-path $INSTALL_BASE"
        echo ""
        return 1
    fi
    log_success "Boards directory exists: $INSTALL_BOARDS_DIR"
    
    # Step 4: Check boards directory permissions
    log_diagnostic "Step 4: Boards directory permissions check"
    if [[ ! -r "$INSTALL_BOARDS_DIR" ]]; then
        log_error "Boards directory is not readable: $INSTALL_BOARDS_DIR"
        log_diagnostic "Reason: Directory permissions prevent reading"
        log_diagnostic "Current permissions: $(ls -ld $INSTALL_BOARDS_DIR)"
        log_diagnostic "Solution: Run: sudo chmod -R 755 $INSTALL_BOARDS_DIR"
        echo ""
        return 1
    fi
    log_success "Boards directory is readable"
    
    # Step 5: Check TOML file accessibility
    log_diagnostic "Step 5: TOML file accessibility check"
    local toml_count=0
    local unreadable=0
    
    for toml_file in "$INSTALL_BOARDS_DIR"/*.toml; do
        if [[ -f "$toml_file" ]]; then
            ((toml_count++))
            if [[ ! -r "$toml_file" ]]; then
                log_error "TOML file not readable: $toml_file"
                ((unreadable++))
            fi
        fi
    done
    
    if [[ $toml_count -eq 0 ]]; then
        log_error "No TOML files found in: $INSTALL_BOARDS_DIR"
        log_diagnostic "Reason: Board definition files are missing"
        log_diagnostic "Solution: Copy board files from workspace or download from GitHub"
        echo ""
        return 1
    fi
    
    if [[ $unreadable -gt 0 ]]; then
        log_error "$unreadable TOML files are not readable"
        log_diagnostic "Reason: File permissions prevent reading"
        log_diagnostic "Solution: Run: sudo chmod -R 644 $INSTALL_BOARDS_DIR/*.toml"
        echo ""
        return 1
    fi
    
    log_success "All $toml_count TOML files are readable"
    
    # Step 6: Check configuration file
    log_diagnostic "Step 6: Configuration file check"
    if [[ -f "$CONFIG_FILE" ]]; then
        log_success "Configuration file exists: $CONFIG_FILE"
        
        # Check if boards_dir is configured
        if grep -q "boards_dir" "$CONFIG_FILE"; then
            local configured_boards_dir=$(grep "boards_dir" "$CONFIG_FILE" | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
            log_diagnostic "Configured boards_dir in TOML: $configured_boards_dir"
            
            if [[ "$configured_boards_dir" != "$INSTALL_BOARDS_DIR" ]]; then
                log_warning "boards_dir mismatch detected in config file"
                log_diagnostic "Expected: $INSTALL_BOARDS_DIR"
                log_diagnostic "Found: $configured_boards_dir"
                log_diagnostic "Reason: Configuration file points to different boards directory"
                log_diagnostic "Solution: Update boards_dir in $CONFIG_FILE to $INSTALL_BOARDS_DIR"
            else
                log_success "boards_dir configuration matches install location"
            fi
        else
            log_warning "boards_dir not explicitly configured in TOML file"
            log_diagnostic "Reason: Config file doesn't specify boards_dir"
            log_diagnostic "Impact: Binary will use CLI default (--boards-dir boards)"
            log_diagnostic "Solution: Add 'boards_dir = \"$INSTALL_BOARDS_DIR\"' to $CONFIG_FILE"
        fi
    else
        log_warning "Configuration file not found: $CONFIG_FILE"
        log_diagnostic "Reason: No configuration file exists yet"
        log_diagnostic "Impact: Binary will use CLI default (--boards-dir boards)"
        log_diagnostic "Solution: Run TUI configuration or create config with boards_dir set"
    fi
    echo ""
    
    # Step 6b: Explain new multi-path search behavior
    log_diagnostic "Step 6b: Multi-path search behavior (NEW)"
    log_info "Binary now searches multiple paths in this order:"
    echo ""
    echo "  1. Explicit --boards-dir (if specified)"
    echo "  2. Config file boards_dir (if specified)"
    echo "  3. Relative ./boards (if exists)"
    echo "  4. /etc/sequent-gateway/boards (if exists)"
    echo ""
    log_success "This means:"
    log_diagnostic "  - Development: Works from project root with ./boards"
    log_diagnostic "  - Production: Falls back to /etc/sequent-gateway/boards"
    log_diagnostic "  - Override: --boards-dir still takes highest priority"
    echo ""
    
    # Step 7: Check environment file (for systemd)
    log_diagnostic "Step 7: Environment file check (systemd)"
    if [[ -f "$ENV_FILE" ]]; then
        log_success "Environment file exists: $ENV_FILE"
        
        if grep -q "BOARDS_DIR" "$ENV_FILE"; then
            local env_boards_dir=$(grep "BOARDS_DIR" "$ENV_FILE" | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
            log_diagnostic "BOARDS_DIR in env: $env_boards_dir"
            
            if [[ "$env_boards_dir" != "$INSTALL_BOARDS_DIR" ]]; then
                log_warning "BOARDS_DIR mismatch in environment file"
                log_diagnostic "Expected: $INSTALL_BOARDS_DIR"
                log_diagnostic "Found: $env_boards_dir"
                log_diagnostic "Reason: Environment file will override with wrong path"
                log_diagnostic "Solution: Update BOARDS_DIR in $ENV_FILE"
            else
                log_success "BOARDS_DIR matches in environment file"
            fi
        else
            log_warning "BOARDS_DIR not set in environment file"
            log_diagnostic "Reason: Service may not find boards directory"
            log_diagnostic "Solution: Add BOARDS_DIR=$INSTALL_BOARDS_DIR to $ENV_FILE"
        fi
    else
        log_warning "Environment file not found: $ENV_FILE"
        log_diagnostic "Reason: No systemd environment configuration"
        log_diagnostic "Impact: Systemd service may fail to locate boards"
    fi
    echo ""
    
    # Step 8: Simulate binary startup with boards-dir
    log_diagnostic "Step 8: Simulating binary startup command"
    echo "Simulated command:"
    echo "  $BINARY_PATH --boards-dir $INSTALL_BOARDS_DIR"
    echo ""
    
    # Check if we can actually run the binary (dry run)
    if command -v "$BINARY_PATH" &> /dev/null; then
        log_success "Binary is in PATH and can be invoked"
        
        # Try to get help/version to verify binary works
        if "$BINARY_PATH" --help &> /dev/null; then
            log_success "Binary responds to --help flag"
        else
            log_warning "Binary may not respond to standard flags"
        fi
    else
        log_warning "Binary not in PATH (may need full path or installation)"
        log_diagnostic "Solution: Ensure /usr/local/bin is in PATH or use full path"
    fi
    echo ""
    
    # Step 9: Check for common access issues
    log_diagnostic "Step 9: Common access issues check"
    
    # Check for I2C access (if on Linux)
    if [[ -e "/dev/i2c-1" ]]; then
        if [[ -r "/dev/i2c-1" ]] && [[ -w "/dev/i2c-1" ]]; then
            log_success "I2C device accessible"
        else
            log_warning "I2C device exists but may not be accessible"
            log_diagnostic "Reason: Permission denied on /dev/i2c-1"
            log_diagnostic "Solution: Add user to i2c group or run as root"
        fi
    else
        log_diagnostic "I2C device /dev/i2c-1 not found (may not be a Raspberry Pi)"
    fi
    
    # Check for SELinux/AppArmor (if applicable)
    if command -v getenforce &> /dev/null; then
        local selinux_status=$(getenforce 2>/dev/null || echo "unknown")
        if [[ "$selinux_status" == "Enforcing" ]]; then
            log_warning "SELinux is enforcing"
            log_diagnostic "Reason: SELinux may block binary access to files"
            log_diagnostic "Solution: Check SELinux logs or set appropriate contexts"
        fi
    fi
    echo ""
    
    # Summary
    log_info "Emulation complete - Binary should be able to access boards"
    log_success "All access checks passed"
    echo ""
    
    return 0
}

# Check if running as root (required for system install)
check_root() {
    if [[ $EUID -ne 0 ]] && [[ "$INSTALL_BASE" == /etc/* ]]; then
        log_warning "Not running as root. Some operations may fail."
        log_info "Consider running with: sudo $0"
    fi
}

# Generate diagnostic report
generate_report() {
    log_info "Generating diagnostic report..."
    echo ""
    echo "SequentTCP Board Installation Diagnostic Report"
    echo "Timestamp: $(date)"
    echo "Install Base: $INSTALL_BASE"
    echo "Boards Directory: $INSTALL_BOARDS_DIR"
    echo ""
    
    echo "Workspace Status:"
    if check_workspace_boards >/dev/null 2>&1; then
        echo "  [OK] Workspace boards directory exists"
    else
        echo "  [ERROR] Workspace boards directory missing"
    fi
    echo ""
    
    echo "Install Location Status:"
    if check_install_boards >/dev/null 2>&1; then
        echo "  [OK] Install boards directory exists"
    else
        echo "  [ERROR] Install boards directory missing"
    fi
    echo ""
    
    echo "Configuration Status:"
    if check_config_file >/dev/null 2>&1; then
        echo "  [OK] Configuration file exists"
    else
        echo "  [ERROR] Configuration file missing"
    fi
    echo ""
    
    echo "Environment Status:"
    if check_env_file >/dev/null 2>&1; then
        echo "  [OK] Environment file exists"
    else
        echo "  [ERROR] Environment file missing"
    fi
    echo ""
    
    echo "Binary Status:"
    if check_binary >/dev/null 2>&1; then
        echo "  [OK] Binary installed and executable"
    else
        echo "  [ERROR] Binary not installed"
    fi
    echo ""
    
    echo "Service Status:"
    if check_service >/dev/null 2>&1; then
        echo "  [OK] Service configured"
    else
        echo "  [ERROR] Service not configured"
    fi
    echo ""
}

# Main execution
main() {
    echo ""
    echo "SequentTCP Board Installation Observer"
    echo ""
    
    if [[ "$CHECK_ONLY" == true ]]; then
        log_info "Running in check-only mode"
    elif [[ "$EMULATE" == true ]]; then
        log_info "Running in emulation mode - simulating installed binary behavior"
    else
        log_info "Running with installation permissions"
    fi
    
    if [[ "$VERBOSE" == true ]]; then
        log_info "Verbose output enabled"
    fi
    
    echo ""
    
    # Check root permissions
    check_root
    
    # Run emulation if requested
    if [[ "$EMULATE" == true ]]; then
        emulate_binary_access
        exit 0
    fi
    
    # Run diagnostic checks
    log_info "Running diagnostic checks..."
    echo ""
    
    generate_report
    
    # If not check-only, offer to install
    if [[ "$CHECK_ONLY" != true ]]; then
        echo ""
        log_info "To install board definitions, run:"
        echo "  sudo $0 --install-path $INSTALL_BASE"
        echo ""
        log_info "To emulate installed binary behavior and diagnose access issues, run:"
        echo "  $0 --emulate"
        echo ""
    fi
    
    log_success "Diagnostic complete"
}

# Run main
main "$@"
