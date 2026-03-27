#!/bin/bash
#
# observe-board-install.sh - Observer script for debugging board definitions installation and reachability
#
# This script helps debug:
# 1. First-time installation of board definitions
# 2. Board definition file discovery
# 3. Reachability of board TOML files in install locations
# 4. TUI board registry validation
#
# Usage: ./observe-board-install.sh [options]
# Options:
#   --install-path PATH  Specify custom install path (default: /etc/sequent-gateway)
#   --verbose            Enable verbose output
#   --check-only         Only check current state, don't perform installation
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
        --help)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --install-path PATH  Specify custom install path (default: /etc/sequent-gateway)"
            echo "  --verbose            Enable verbose output"
            echo "  --check-only         Only check current state, don't perform installation"
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
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_verbose() {
    if [[ "$VERBOSE" == true ]]; then
        echo -e "${CYAN}[DEBUG]${NC} $1"
    fi
}

# Check if running as root (required for system install)
check_root() {
    if [[ $EUID -ne 0 ]] && [[ "$INSTALL_BASE" == /etc/* ]]; then
        log_warning "Not running as root. Some operations may fail."
        log_info "Consider running with: sudo $0"
    fi
}

# Check current directory structure
check_workspace_boards() {
    log_info "Checking workspace board definitions..."
    
    local workspace_boards="./boards"
    local experimental_boards="./boards/experimental"
    
    if [[ -d "$workspace_boards" ]]; then
        log_success "Workspace boards directory exists: $workspace_boards"
        
        local board_count=$(find "$workspace_boards" -name "*.toml" -type f | wc -l)
        log_info "Found $board_count board TOML files in $workspace_boards"
        
        # List all board files
        log_verbose "Board files in workspace:"
        find "$workspace_boards" -name "*.toml" -type f -exec basename {} \; | sort | while read board; do
            echo "  - $board"
        done
        
        # Check for experimental boards
        if [[ -d "$experimental_boards" ]]; then
            local exp_count=$(find "$experimental_boards" -name "*.toml" -type f | wc -l)
            log_info "Found $exp_count experimental board TOML files"
        fi
        
        return 0
    else
        log_error "Workspace boards directory not found: $workspace_boards"
        return 1
    fi
}

# Check install location for board definitions
check_install_boards() {
    log_info "Checking install location: $INSTALL_BOARDS_DIR"
    
    if [[ -d "$INSTALL_BOARDS_DIR" ]]; then
        log_success "Install boards directory exists: $INSTALL_BOARDS_DIR"
        
        local board_count=$(find "$INSTALL_BOARDS_DIR" -name "*.toml" -type f | wc -l)
        log_info "Found $board_count board TOML files in install location"
        
        # List all board files
        log_verbose "Board files in install location:"
        find "$INSTALL_BOARDS_DIR" -name "*.toml" -type f -exec basename {} \; | sort | while read board; do
            echo "  - $board"
        done
        
        # Check file permissions
        log_verbose "Checking file permissions..."
        find "$INSTALL_BOARDS_DIR" -name "*.toml" -type f -exec ls -la {} \; | while read line; do
            log_verbose "  $line"
        done
        
        return 0
    else
        log_warning "Install boards directory does not exist: $INSTALL_BOARDS_DIR"
        return 1
    fi
}

# Check configuration file
check_config_file() {
    log_info "Checking configuration file: $CONFIG_FILE"
    
    if [[ -f "$CONFIG_FILE" ]]; then
        log_success "Configuration file exists: $CONFIG_FILE"
        
        # Check if boards_dir is configured correctly
        if grep -q "boards_dir" "$CONFIG_FILE"; then
            local boards_dir=$(grep "boards_dir" "$CONFIG_FILE" | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
            log_info "Configured boards_dir: $boards_dir"
            
            if [[ "$boards_dir" == "$INSTALL_BOARDS_DIR" ]]; then
                log_success "boards_dir matches install location"
            else
                log_warning "boards_dir does not match install location"
                log_warning "Expected: $INSTALL_BOARDS_DIR"
                log_warning "Found: $boards_dir"
            fi
        else
            log_warning "boards_dir not explicitly configured in $CONFIG_FILE"
        fi
        
        # Show config summary
        log_verbose "Configuration file contents:"
        cat "$CONFIG_FILE" | while read line; do
            log_verbose "  $line"
        done
        
        return 0
    else
        log_warning "Configuration file does not exist: $CONFIG_FILE"
        return 1
    fi
}

# Check environment file
check_env_file() {
    log_info "Checking environment file: $ENV_FILE"
    
    if [[ -f "$ENV_FILE" ]]; then
        log_success "Environment file exists: $ENV_FILE"
        
        # Check BOARDS_DIR in env file
        if grep -q "BOARDS_DIR" "$ENV_FILE"; then
            local boards_dir=$(grep "BOARDS_DIR" "$ENV_FILE" | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
            log_info "BOARDS_DIR in env: $boards_dir"
            
            if [[ "$boards_dir" == "$INSTALL_BOARDS_DIR" ]]; then
                log_success "BOARDS_DIR matches install location"
            else
                log_warning "BOARDS_DIR does not match install location"
                log_warning "Expected: $INSTALL_BOARDS_DIR"
                log_warning "Found: $boards_dir"
            fi
        else
            log_warning "BOARDS_DIR not set in $ENV_FILE"
        fi
        
        return 0
    else
        log_warning "Environment file does not exist: $ENV_FILE"
        return 1
    fi
}

# Check binary installation
check_binary() {
    log_info "Checking binary installation: $BINARY_PATH"
    
    if [[ -f "$BINARY_PATH" ]]; then
        log_success "Binary exists: $BINARY_PATH"
        
        # Check executable permissions
        if [[ -x "$BINARY_PATH" ]]; then
            log_success "Binary is executable"
        else
            log_warning "Binary is not executable"
        fi
        
        # Show binary version/info if available
        if "$BINARY_PATH" --version 2>/dev/null; then
            log_verbose "Binary version info available"
        else
            log_verbose "Binary version info not available"
        fi
        
        return 0
    else
        log_warning "Binary does not exist: $BINARY_PATH"
        return 1
    fi
}

# Check systemd service
check_service() {
    log_info "Checking systemd service..."
    
    if systemctl list-units --type=service --all | grep -q "sequent-gateway"; then
        log_success "Service unit is known to systemd"
        
        if systemctl is-active --quiet sequent-gateway; then
            log_success "Service is currently active"
        else
            log_warning "Service is not active"
        fi
    else
        log_warning "Service unit not found in systemd"
    fi
    
    return 0
}

# Validate board TOML files
validate_board_tomls() {
    log_info "Validating board TOML files..."
    
    local board_dir="$1"
    local valid_count=0
    local invalid_count=0
    
    if [[ ! -d "$board_dir" ]]; then
        log_error "Board directory does not exist: $board_dir"
        return 1
    fi
    
    # Find all TOML files
    find "$board_dir" -name "*.toml" -type f | while read board_file; do
        local board_name=$(basename "$board_file")
        
        # Basic TOML validation - check for required fields
        if grep -q "^\[board\]" "$board_file" || grep -q "^\[info\]" "$board_file"; then
            log_verbose "  OK - $board_name - appears valid"
            ((valid_count++))
        else
            log_warning "  X - $board_name - missing board/info section"
            ((invalid_count++))
        fi
    done
    
    log_info "Validation complete"
}

# Install board definitions to system location
install_boards() {
    if [[ "$CHECK_ONLY" == true ]]; then
        log_info "Check-only mode: skipping installation"
        return 0
    fi
    
    log_info "Installing board definitions to $INSTALL_BOARDS_DIR"
    
    # Create directories
    log_verbose "Creating directories..."
    mkdir -p "$INSTALL_BOARDS_DIR"
    mkdir -p "$INSTALL_CONFIG_DIR"
    
    # Copy board files from workspace
    if [[ -d "./boards" ]]; then
        log_verbose "Copying board files from ./boards..."
        cp -r ./boards/* "$INSTALL_BOARDS_DIR/" 2>/dev/null || true
        
        local copied_count=$(find "$INSTALL_BOARDS_DIR" -name "*.toml" -type f | wc -l)
        log_success "Copied $copied_count board files to $INSTALL_BOARDS_DIR"
    else
        log_warning "No boards directory found in workspace"
    fi
    
    # Set permissions
    log_verbose "Setting permissions..."
    chmod -R 644 "$INSTALL_BOARDS_DIR"/*.toml 2>/dev/null || true
    chmod -R 755 "$INSTALL_BOARDS_DIR" 2>/dev/null || true
    
    log_success "Board definitions installed successfully"
}

# Generate diagnostic report
generate_report() {
    log_info "Generating diagnostic report..."
    echo ""
    echo "═══════════════════════════════════════════════════════════"
    echo "  SequentTCP Board Installation Diagnostic Report"
    echo "═══════════════════════════════════════════════════════════"
    echo ""
    echo "Timestamp: $(date)"
    echo "Install Base: $INSTALL_BASE"
    echo "Boards Directory: $INSTALL_BOARDS_DIR"
    echo ""
    
    echo "  Workspace Status"
    if check_workspace_boards >/dev/null 2>&1; then
        echo "        OK - Workspace boards directory exists"
    else
        echo "        X - Workspace boards directory missing"
    fi
    echo ""
    
    echo "  Install Location Status"
    if check_install_boards >/dev/null 2>&1; then
        echo "        OK - Install boards directory exists"
    else
        echo "        X - Install boards directory missing"
    fi
    echo ""
    
    echo "  Configuration Status"
    if check_config_file >/dev/null 2>&1; then
        echo "        OK - Configuration file exists"
    else
        echo "        X - Configuration file missing"
    fi
    echo ""
    
    echo "  Environment Status "
    if check_env_file >/dev/null 2>&1; then
        echo "        OK - Environment file exists   "
    else
        echo "        X - Environment file missing  "
    fi
    echo ""
    
    echo "  Binary Status"
    if check_binary >/dev/null 2>&1; then
        echo "        OK - Binary installed and executable"
    else
        echo "        X - Binary not installed"
    fi
    echo ""
    
    echo "  Service Status"
    if check_service >/dev/null 2>&1; then
        echo "        OK - Service configured"
    else
        echo "        X - Service not configured"
    fi
    echo ""
}

# Main execution
main() {
    echo ""
    echo "═══════════════════════════════════════════════════════════"
    echo "  SequentTCP Board Installation Observer"
    echo "═══════════════════════════════════════════════════════════"
    echo ""
    
    if [[ "$CHECK_ONLY" == true ]]; then
        log_info "Running in check-only mode"
    else
        log_info "Running with installation permissions"
    fi
    
    if [[ "$VERBOSE" == true ]]; then
        log_info "Verbose output enabled"
    fi
    
    echo ""
    
    # Check root permissions
    check_root
    
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
    fi
    
    log_success "Diagnostic complete"
}

# Run main
main
