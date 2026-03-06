# justfile — task runner for sequent-gateway
# Install: cargo install just
# Usage:   just <recipe>    e.g.  just build, just cross, just test

# Default target for Raspberry Pi 4 (64-bit)
pi_target := "aarch64-unknown-linux-gnu"

# Raspberry Pi 3 / Zero 2 W (32-bit)
pi32_target := "armv7-unknown-linux-gnueabihf"

# ── Development ───────────────────────────────────────────────────────

# Build debug binary (native platform)
build:
    cargo build

# Build release binary (native platform)
build-release:
    cargo build --release

# Run all tests
test:
    cargo test

# Check without building (fast feedback)
check:
    cargo check

# Format code
fmt:
    cargo fmt

# Lint with clippy
lint:
    cargo clippy -- -D warnings

# ── Cross-compilation ────────────────────────────────────────────────

# Cross-compile release binary for Raspberry Pi 4 (aarch64)
cross:
    cross build --release --target {{pi_target}}

# Cross-compile release binary for Raspberry Pi 3 / Zero 2 W (armv7)
cross-32:
    cross build --release --target {{pi32_target}}

# ── Deployment ────────────────────────────────────────────────────────

# Deploy binary to Pi via scp (set PI_HOST, e.g. just deploy PI_HOST=pi@192.168.1.100)
deploy PI_HOST:
    cross build --release --target {{pi_target}}
    scp target/{{pi_target}}/release/sequent-gateway {{PI_HOST}}:~/sequent-gateway

# ── Housekeeping ──────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# Show binary size (release, native)
size:
    cargo build --release
    ls -lh target/release/sequent-gateway

# Show binary size (release, cross-compiled for Pi)
size-pi:
    cross build --release --target {{pi_target}}
    ls -lh target/{{pi_target}}/release/sequent-gateway
