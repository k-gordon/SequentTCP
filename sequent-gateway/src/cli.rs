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

    /// Heartbeat log interval in seconds
    #[arg(long, default_value_t = 5)]
    pub log_interval: u64,
}
