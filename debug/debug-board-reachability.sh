#!/bin/bash
#
# debug-board-reachability.sh - Detailed debugging script for board definition reachability
#
# This script provides deep debugging for:
# 1. Board definition file parsing and validation
# 2. TUI board discovery mechanism
# 3. I²C bus accessibility
# 4. Modbus register mapping validation
# 5. Protocol handler compatibility
#
# Usage: ./debug-board-reachability.sh [board-name]
#

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m'

# Paths
WORKSPACE_BOARDS="./boards"
INSTALL_BOARDS="/etc/sequent-gateway/boards"
CURRENT_DIR="$(pwd)"

log_section() {
    echo ""
    echo -e "${MAGENTA}═══════════════════════════════════════════════════════════${NC}"
    echo -e "${MAGENTA}  $1${NC}"
    echo -e "${MAGENTA}═══════════════════════════════════════════════════════════${NC}"
    echo ""
}

log_subsection() {
    echo ""
    echo -e "${CYAN}┌─ $1 ──────────────────────────────────────────────┐${NC}"
}

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[✓]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[!]${NC} $1"
}

log_error() {
    echo -e "${RED}[✗]${NC} $1"
}

log_debug() {
    echo -e "${CYAN}[DEBUG]${NC} $1"
}

# Parse TOML file and extract key fields
parse_board_toml() {
    local toml_file="$1"
    local board_name=$(basename "$toml_file" .toml)
    
    log_subsection "Parsing: $board_name"
    
    if [[ ! -f "$toml_file" ]]; then
        log_error "File not found: $toml_file"
        return 1
    fi
    
    # Extract board info section
    echo "  File: $toml_file"
    echo "  Size: $(stat -c%s "$toml_file" 2>/dev/null || stat -f%z "$toml_file") bytes"
    
    # Check for required sections
    if grep -q "^\[board\]" "$toml_file"; then
        log_success "  Has [board] section"
        
        # Extract board name
        local name=$(grep "^name" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
        echo "  Board Name: $name"
        
        # Extract protocol
        local protocol=$(grep "^protocol" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
        echo "  Protocol: $protocol"
        
        if [[ "$protocol" == "sequent_mcu" ]]; then
            log_success "  Protocol handler: Sequent MCU (compatible)"
        elif [[ "$protocol" == "pca9535" ]]; then
            log_success "  Protocol handler: PCA9535 (compatible)"
        else
            log_warning "  Unknown protocol: $protocol"
        fi
    else
        log_error "  Missing [board] section"
        return 1
    fi
    
    # Check address configuration
    if grep -q "^\[address\]" "$toml_file"; then
        log_success "  Has [address] section"
        
        local base=$(grep "^base" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        local mode=$(grep "^mode" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
        
        echo "  I²C Base Address: 0x$(printf '%x' "$base" 2>/dev/null || echo "$base")"
        echo "  Address Mode: $mode"
    else
        log_warning "  Missing [address] section"
    fi
    
    # Check for channel configuration
    if grep -q "^\[channels\]" "$toml_file"; then
        log_success "  Has [channels] section"
        
        local relays=$(grep "^relays" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        local opto_inputs=$(grep "^opto_inputs" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        
        if [[ -n "$relays" ]]; then
            echo "  Relay Channels: $relays"
        fi
        if [[ -n "$opto_inputs" ]]; then
            echo "  Opto-Isolated Inputs: $opto_inputs"
        fi
    fi
    
    # Check for register map (Sequent MCU boards)
    if grep -q "^\[registers\]" "$toml_file"; then
        log_success "  Has [registers] section"
        
        local relay_set=$(grep "^relay_set" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        local relay_val=$(grep "^relay_val" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        
        if [[ -n "$relay_set" ]]; then
            echo "  Relay Set Register: 0x$(printf '%x' "$relay_set" 2>/dev/null || echo "$relay_set")"
        fi
        if [[ -n "$relay_val" ]]; then
            echo "  Relay Value Register: 0x$(printf '%x' "$relay_val" 2>/dev/null || echo "$relay_val")"
        fi
    fi
    
    # Check for PCA9535 configuration
    if grep -q "^\[pca9535\]" "$toml_file"; then
        log_success "  Has [pca9535] section (PCA9535 protocol)"
        
        local port0_dir=$(grep "^port0_dir" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        local port1_dir=$(grep "^port1_dir" "$toml_file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        
        if [[ -n "$port0_dir" ]]; then
            echo "  Port0 Direction: 0x$(printf '%x' "$port0_dir" 2>/dev/null || echo "$port0_dir")"
        fi
        if [[ -n "$port1_dir" ]]; then
            echo "  Port1 Direction: 0x$(printf '%x' "$port1_dir" 2>/dev/null || echo "$port1_dir")"
        fi
    fi
    
    echo ""
}

# Check I²C bus accessibility
check_i2c_bus() {
    log_section "I²C Bus Accessibility Check"
    
    # Check if i2c-tools are installed
    if command -v i2cdetect &> /dev/null; then
        log_success "i2c-tools installed"
    else
        log_warning "i2c-tools not installed (sudo apt install i2c-tools)"
        return 1
    fi
    
    # List available I²C buses
    log_info "Available I²C buses:"
    i2cdetect -l 2>/dev/null | while read line; do
        echo "  $line"
    done
    
    # Scan I²C bus 1 (common on Raspberry Pi)
    log_info "Scanning I²C bus 1..."
    echo "  Address  00  01  02  03  04  05  06  07  08  09  0A  0B  0C  0D  0E  0F"
    i2cdetect -y 1 2>/dev/null | while read line; do
        echo "  $line"
    done
    
    # Check for common Sequent board addresses
    log_info "Looking for Sequent board addresses..."
    
    local addresses=(32 33 34 35 36 37 38 39  # 0x20-0x27 (PCA9535)
                     80 81 82 83 84 85 86 87)  # 0x50-0x57 (MegaInd)
    
    for addr in "${addresses[@]}"; do
        if i2cdetect -y 1 $addr $addr 2>/dev/null | grep -q "$addr"; then
            log_success "  Found device at 0x$(printf '%02x' $addr)"
        fi
    done
    
    echo ""
}

# Test board discovery by TUI
test_tui_discovery() {
    log_section "TUI Board Discovery Test"
    
    # Test workspace boards directory
    log_subsection "Workspace Boards Directory: $WORKSPACE_BOARDS"
    
    if [[ -d "$WORKSPACE_BOARDS" ]]; then
        log_success "Directory exists"
        
        local count=$(find "$WORKSPACE_BOARDS" -name "*.toml" -type f | wc -l)
        log_info "Found $count board TOML files"
        
        # List all boards
        echo ""
        echo "  Discovered boards:"
        find "$WORKSPACE_BOARDS" -name "*.toml" -type f | sort | while read file; do
            local name=$(basename "$file")
            echo "    - $name"
        done
    else
        log_error "Directory not found"
    fi
    
    # Test install boards directory
    log_subsection "Install Boards Directory: $INSTALL_BOARDS"
    
    if [[ -d "$INSTALL_BOARDS" ]]; then
        log_success "Directory exists"
        
        local count=$(find "$INSTALL_BOARDS" -name "*.toml" -type f | wc -l)
        log_info "Found $count board TOML files"
        
        # List all boards
        echo ""
        echo "  Discovered boards:"
        find "$INSTALL_BOARDS" -name "*.toml" -type f | sort | while read file; do
            local name=$(basename "$file")
            echo "    - $name"
        done
    else
        log_warning "Directory not found (not installed yet)"
    fi
    
    echo ""
}

# Validate all board TOML files
validate_all_boards() {
    log_section "Board TOML Validation"
    
    local board_dir="$1"
    
    if [[ ! -d "$board_dir" ]]; then
        log_error "Directory not found: $board_dir"
        return 1
    fi
    
    local valid=0
    local invalid=0
    
    echo "Validating board definitions in: $board_dir"
    echo ""
    
    find "$board_dir" -name "*.toml" -type f | sort | while read file; do
        local name=$(basename "$file")
        
        # Basic validation
        local errors=0
        
        # Check for [board] section
        if ! grep -q "^\[board\]" "$file"; then
            echo "  ✗ $name: Missing [board] section"
            ((invalid++))
            continue
        fi
        
        # Check for name field
        if ! grep -q "^name" "$file"; then
            echo "  ✗ $name: Missing name field"
            ((invalid++))
            continue
        fi
        
        # Check for protocol field
        if ! grep -q "^protocol" "$file"; then
            echo "  ✗ $name: Missing protocol field"
            ((invalid++))
            continue
        fi
        
        # Check for [address] section
        if ! grep -q "^\[address\]" "$file"; then
            echo "  ✗ $name: Missing [address] section"
            ((invalid++))
            continue
        fi
        
        # Check for base address
        if ! grep -q "^base" "$file"; then
            echo "  ✗ $name: Missing base address"
            ((invalid++))
            continue
        fi
        
        # Check for mode
        if ! grep -q "^mode" "$file"; then
            echo "  ✗ $name: Missing address mode"
            ((invalid++))
            continue
        fi
        
        log_success "  ✓ $name: Valid"
        ((valid++))
    done
    
    echo ""
    echo "Validation complete"
}

# Check protocol handler compatibility
check_protocol_handlers() {
    log_section "Protocol Handler Compatibility"
    
    echo "Supported protocol handlers:"
    echo ""
    
    # Sequent MCU protocol
    echo "  sequent_mcu:"
    echo "    - Used for boards with custom Sequent MCU firmware"
    echo "    - Requires register map definition [registers]"
    echo "    - Supports relay set/value, input reading, analog I/O"
    echo ""
    
    # PCA9535 protocol
    echo "  pca9535:"
    echo "    - Used for PCA9535 GPIO expander chips"
    echo "    - Requires [pca9535] configuration section"
    echo "    - Supports digital I/O via port direction registers"
    echo ""
    
    # Count boards by protocol
    local board_dir="$1"
    
    if [[ -d "$board_dir" ]]; then
        echo "Protocol distribution in $board_dir:"
        
        local mcu_count=$(grep -l "^protocol.*sequent_mcu" "$board_dir"/*.toml 2>/dev/null | wc -l)
        local pca_count=$(grep -l "^protocol.*pca9535" "$board_dir"/*.toml 2>/dev/null | wc -l)
        
        echo "    sequent_mcu: $mcu_count boards"
        echo "    pca9535: $pca_count boards"
    fi
    
    echo ""
}

# Generate board summary report
generate_board_summary() {
    log_section "Board Definition Summary"
    
    local board_dir="$1"
    
    if [[ ! -d "$board_dir" ]]; then
        log_error "Directory not found: $board_dir"
        return 1
    fi
    
    echo "Board definitions in: $board_dir"
    echo ""
    
    # Total count
    local total=$(find "$board_dir" -name "*.toml" -type f | wc -l)
    echo "Total board files: $total"
    echo ""
    
    # By protocol
    echo "By protocol:"
    local mcu_count=$(grep -l "^protocol.*sequent_mcu" "$board_dir"/*.toml 2>/dev/null | wc -l)
    local pca_count=$(grep -l "^protocol.*pca9535" "$board_dir"/*.toml 2>/dev/null | wc -l)
    echo "  - sequent_mcu: $mcu_count"
    echo "  - pca9535: $pca_count"
    echo ""
    
    # By I²C address range
    echo "By I²C address range:"
    local addr_20_count=$(grep -l "^base.*0x2" "$board_dir"/*.toml 2>/dev/null | wc -l)
    local addr_50_count=$(grep -l "^base.*0x5" "$board_dir"/*.toml 2>/dev/null | wc -l)
    echo "  - 0x2x range: $addr_20_count"
    echo "  - 0x5x range: $addr_50_count"
    echo ""
    
    # List all boards with their protocols
    echo "All boards:"
    find "$board_dir" -name "*.toml" -type f | sort | while read file; do
        local name=$(basename "$file" .toml)
        local protocol=$(grep "^protocol" "$file" | head -1 | cut -d'=' -f2 | tr -d ' "' | tr -d "'")
        local base=$(grep "^base" "$file" | head -1 | cut -d'=' -f2 | tr -d ' ')
        
        printf "  %-25s %s  base=0x%s\n" "$name" "($protocol)" "$(printf '%x' "$base" 2>/dev/null || echo "$base")"
    done
    
    echo ""
}

# Main execution
main() {
    echo ""
    echo "═══════════════════════════════════════════════════════════"
    echo "  SequentTCP Board Reachability Debugger"
    echo "═══════════════════════════════════════════════════════════"
    echo ""
    
    # Run all checks
    test_tui_discovery
    
    # Validate workspace boards
    if [[ -d "$WORKSPACE_BOARDS" ]]; then
        validate_all_boards "$WORKSPACE_BOARDS"
        generate_board_summary "$WORKSPACE_BOARDS"
        check_protocol_handlers "$WORKSPACE_BOARDS"
    fi
    
    # Validate install boards if exists
    if [[ -d "$INSTALL_BOARDS" ]]; then
        validate_all_boards "$INSTALL_BOARDS"
        generate_board_summary "$INSTALL_BOARDS"
    fi
    
    # I²C bus check (requires root)
    if [[ $EUID -eq 0 ]]; then
        check_i2c_bus
    else
        log_warning "Run as root to check I²C bus accessibility"
    fi
    
    echo ""
    echo "═══════════════════════════════════════════════════════════"
    echo "  Debug complete"
    echo "═══════════════════════════════════════════════════════════"
    echo ""
}

# Run main
main "$@"
