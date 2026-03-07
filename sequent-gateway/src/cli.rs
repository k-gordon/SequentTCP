use std::path::PathBuf;

use clap::{Parser, Subcommand, Args};

/// Modbus TCP ↔ I²C gateway for Sequent Microsystems HATs.
///
/// Bridges Modbus TCP clients (SCADA, HMI, vPLC) to Sequent Industrial
/// and Relay HATs over the I²C bus — no CLI tools required.
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Cli {
    /// Board types to load.  Can be specified multiple times.
    ///
    /// Supported values: `megaind`, `relay16`, `relay8`.
    /// When omitted, defaults to `megaind` + `relay16` for backward
    /// compatibility.
    #[arg(long = "board", value_name = "TYPE")]
    pub boards: Vec<String>,

    /// **[DEPRECATED]** Fall back to compiled-in register maps when no
    /// TOML file is found for a board.
    ///
    /// Without this flag, the gateway requires a TOML definition in
    /// `--boards-dir` for every board listed in `--board`.
    /// Use this only for migration from older versions — the compiled-in
    /// defaults will be removed in a future release.  Prefer placing
    /// proper TOML files in `--boards-dir` instead.
    #[arg(long)]
    pub builtin_defaults: bool,

    /// IP address to bind the Modbus TCP server
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,

    /// TCP port for the Modbus TCP server
    #[arg(long, default_value_t = 502)]
    pub port: u16,

    /// Map opto-input bitmask to Holding Register 15
    #[arg(long)]
    pub map_opto_to_reg: bool,

    /// I²C stack level for the Industrial (MegaInd) HAT [0–7]
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=7))]
    pub ind_stack: u8,

    /// I²C stack level for the 16-Relay HAT [0–7]
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=7))]
    pub relay_stack: u8,

    /// Directory containing board definition TOML files
    #[arg(long, default_value = "boards")]
    pub boards_dir: PathBuf,

    /// Heartbeat log interval in seconds
    #[arg(long, default_value_t = 5)]
    pub log_interval: u64,

    /// Consecutive I²C failures before attempting GPIO bus recovery (0 = disabled)
    #[arg(long, default_value_t = 10)]
    pub i2c_reset_threshold: u32,

    /// Modbus slave ID for the 16-Relay HAT [1–247]
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=247))]
    pub relay_slave_id: u8,

    /// Modbus slave ID for the Industrial (MegaInd) HAT [1–247]
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=247))]
    pub ind_slave_id: u8,

    /// Use a single flat Modbus Slave ID (backward-compatible mode).
    ///
    /// All coils, discrete inputs, and holding registers appear under one
    /// slave ID (the relay-slave-id value) using the flat memory map.
    #[arg(long)]
    pub single_slave: bool,

    /// Path to a log file for rotating file output.
    ///
    /// When set, logs are written to both stdout and a daily-rotated file
    /// at the given path. The filename will have a date suffix appended
    /// (e.g. `gateway.log.2026-03-06`).
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Number of rotated log files to retain (default: 7)
    #[arg(long, default_value_t = 7)]
    pub log_retention: usize,

    /// Consecutive per-channel read failures before marking FAULT (0 = disabled)
    #[arg(long, default_value_t = 5)]
    pub channel_fault_threshold: u32,

    /// Poll ticks between relay read-back verifications (0 = disabled).
    ///
    /// When enabled, the gateway reads the actual relay output register every
    /// N-th poll tick and compares against the cached expected state.
    /// Mismatches are logged at WARN and the affected relay cache entries are
    /// invalidated, forcing a re-write on the next cycle.
    #[arg(long, default_value_t = 10)]
    pub relay_verify_interval: u32,

    /// TCP port for the HTTP health endpoint (disabled if not set).
    ///
    /// When set, a lightweight HTTP server serves `GET /health` with JSON
    /// status including uptime, last cycle time, I²C error count, and
    /// per-channel health.
    #[arg(long)]
    pub health_port: Option<u16>,

    /// Optional subcommand (validate, etc.)
    #[command(subcommand)]
    pub command: Option<Command>,
}

// ════════════════════════════════════════════════════════════════════════
// Subcommands
// ════════════════════════════════════════════════════════════════════════

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Run automated hardware validation tests.
    ///
    /// Discovers scenario TOML files, launches the gateway in each
    /// configuration, runs Modbus + health endpoint tests against it,
    /// tears it down, and produces a PASS/FAIL report.
    Validate(ValidateArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ValidateArgs {
    /// Path to the gateway binary to spawn for each scenario.
    ///
    /// Defaults to the currently running executable.
    #[arg(long)]
    pub gateway_bin: Option<PathBuf>,

    /// Directory containing scenario TOML files
    #[arg(long, default_value = "tests/scenarios")]
    pub scenario_dir: PathBuf,

    /// Run only specific scenario file(s); may be repeated.
    #[arg(long = "scenario")]
    pub scenarios: Vec<PathBuf>,

    /// Skip relay, open-drain, and analog output write tests.
    ///
    /// Use this when relays control live equipment.
    #[arg(long)]
    pub skip_writes: bool,

    /// Duration of the stability test in seconds
    #[arg(long, default_value_t = 5)]
    pub stability_duration: u64,

    /// Seconds to wait for the gateway health endpoint at startup
    #[arg(long, default_value_t = 10)]
    pub startup_timeout: u64,
}
