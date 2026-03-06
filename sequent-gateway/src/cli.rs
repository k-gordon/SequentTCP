use std::path::PathBuf;

use clap::Parser;

/// Modbus TCP ↔ I²C gateway for Sequent Microsystems HATs.
///
/// Bridges Modbus TCP clients (SCADA, HMI, vPLC) to Sequent Industrial
/// and 16-Relay HATs over the I²C bus — no CLI tools required.
#[derive(Parser, Debug)]
#[command(version, about)]
pub struct Cli {
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
}
