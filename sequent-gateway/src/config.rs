//! Gateway configuration file support.
//!
//! `sequent-gateway.toml` is the primary way to configure the gateway.
//! CLI flags override config-file values (CLI wins).
//!
//! # Example
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 502
//! health_port = 8080
//! single_slave = false
//!
//! [logging]
//! interval = 5
//! file = "/var/log/sequent-gateway.log"
//! retention = 7
//!
//! [i2c]
//! reset_threshold = 10
//! channel_fault_threshold = 5
//! relay_verify_interval = 10
//!
//! [[board]]
//! type = "megaind"
//! stack = 1
//! slave_id = 2
//!
//! [[board]]
//! type = "relay16"
//! stack = 0
//! slave_id = 1
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ════════════════════════════════════════════════════════════════════════
// Top-level config
// ════════════════════════════════════════════════════════════════════════

/// Complete gateway configuration, deserialized from `sequent-gateway.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayConfig {
    /// Server / networking settings.
    #[serde(default)]
    pub server: ServerConfig,

    /// Logging settings.
    #[serde(default)]
    pub logging: LoggingConfig,

    /// I²C bus tuning.
    #[serde(default)]
    pub i2c: I2cConfig,

    /// Board instances to expose over Modbus.
    #[serde(default)]
    pub board: Vec<BoardInstance>,

    /// Directory containing board definition TOML files.
    #[serde(default = "default_boards_dir")]
    pub boards_dir: PathBuf,

    /// Map opto-input bitmask to Holding Register 15.
    #[serde(default)]
    pub map_opto_to_reg: bool,

    /// Fall back to compiled-in register maps (deprecated).
    #[serde(default)]
    pub builtin_defaults: bool,
}

fn default_boards_dir() -> PathBuf {
    PathBuf::from("boards")
}

// ════════════════════════════════════════════════════════════════════════
// Sub-sections
// ════════════════════════════════════════════════════════════════════════

/// Network binding and Modbus addressing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// IP address to bind.
    #[serde(default = "default_host")]
    pub host: String,

    /// Modbus TCP port.
    #[serde(default = "default_port")]
    pub port: u16,

    /// HTTP health endpoint port (None = disabled).
    #[serde(default)]
    pub health_port: Option<u16>,

    /// Use a single flat Modbus Slave ID.
    #[serde(default)]
    pub single_slave: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            health_port: None,
            single_slave: false,
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    502
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Heartbeat log interval in seconds.
    #[serde(default = "default_log_interval")]
    pub interval: u64,

    /// Path to a daily-rotated log file (None = stdout only).
    #[serde(default)]
    pub file: Option<PathBuf>,

    /// Number of rotated log files to retain.
    #[serde(default = "default_log_retention")]
    pub retention: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            interval: default_log_interval(),
            file: None,
            retention: default_log_retention(),
        }
    }
}

fn default_log_interval() -> u64 {
    5
}
fn default_log_retention() -> usize {
    7
}

/// I²C bus recovery and watchdog tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I2cConfig {
    /// Consecutive I²C failures before GPIO bus recovery.
    #[serde(default = "default_reset_threshold")]
    pub reset_threshold: u32,

    /// Consecutive per-channel failures before marking FAULT.
    #[serde(default = "default_channel_fault")]
    pub channel_fault_threshold: u32,

    /// Poll ticks between relay read-back verifications.
    #[serde(default = "default_relay_verify")]
    pub relay_verify_interval: u32,
}

impl Default for I2cConfig {
    fn default() -> Self {
        Self {
            reset_threshold: default_reset_threshold(),
            channel_fault_threshold: default_channel_fault(),
            relay_verify_interval: default_relay_verify(),
        }
    }
}

fn default_reset_threshold() -> u32 {
    10
}
fn default_channel_fault() -> u32 {
    5
}
fn default_relay_verify() -> u32 {
    10
}

/// A single board instance in the configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardInstance {
    /// Board type slug — must match a `.toml` filename in `boards_dir`.
    #[serde(rename = "type")]
    pub board_type: String,

    /// I²C stack level [0–7].
    #[serde(default)]
    pub stack: u8,

    /// Modbus slave ID [1–247].
    #[serde(default = "default_slave_id")]
    pub slave_id: u8,
}

fn default_slave_id() -> u8 {
    1
}

// ════════════════════════════════════════════════════════════════════════
// Loading
// ════════════════════════════════════════════════════════════════════════

impl GatewayConfig {
    /// Load a configuration from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read config: {}", path.display()))?;
        let cfg: GatewayConfig = toml::from_str(&content)
            .with_context(|| format!("Cannot parse config: {}", path.display()))?;
        Ok(cfg)
    }

    /// Save the configuration to a TOML file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("Cannot serialize config to TOML")?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create directory: {}", parent.display()))?;
        }

        std::fs::write(path, &content)
            .with_context(|| format!("Cannot write config: {}", path.display()))?;
        Ok(())
    }

    /// Default configuration file search path.
    ///
    /// 1. `./sequent-gateway.toml`  (development)
    /// 2. `/etc/sequent-gateway/sequent-gateway.toml`  (production)
    pub fn default_path() -> Option<PathBuf> {
        let local = PathBuf::from("sequent-gateway.toml");
        if local.exists() {
            return Some(local);
        }
        let system = PathBuf::from("/etc/sequent-gateway/sequent-gateway.toml");
        if system.exists() {
            return Some(system);
        }
        None
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = GatewayConfig::default();
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.server.port, 502);
        assert_eq!(cfg.logging.interval, 5);
        assert_eq!(cfg.i2c.reset_threshold, 10);
        assert!(cfg.board.is_empty());
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = GatewayConfig {
            server: ServerConfig {
                host: "127.0.0.1".into(),
                port: 5020,
                health_port: Some(8080),
                single_slave: true,
            },
            logging: LoggingConfig {
                interval: 10,
                file: Some(PathBuf::from("/var/log/gw.log")),
                retention: 14,
            },
            i2c: I2cConfig {
                reset_threshold: 20,
                channel_fault_threshold: 3,
                relay_verify_interval: 5,
            },
            board: vec![
                BoardInstance {
                    board_type: "megaind".into(),
                    stack: 1,
                    slave_id: 2,
                },
                BoardInstance {
                    board_type: "relay16".into(),
                    stack: 0,
                    slave_id: 1,
                },
            ],
            boards_dir: PathBuf::from("/etc/sequent-gateway/boards"),
            map_opto_to_reg: true,
            builtin_defaults: false,
        };

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: GatewayConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.server.host, "127.0.0.1");
        assert_eq!(parsed.server.port, 5020);
        assert_eq!(parsed.server.health_port, Some(8080));
        assert!(parsed.server.single_slave);
        assert_eq!(parsed.logging.interval, 10);
        assert_eq!(parsed.board.len(), 2);
        assert_eq!(parsed.board[0].board_type, "megaind");
        assert_eq!(parsed.board[0].stack, 1);
        assert_eq!(parsed.board[0].slave_id, 2);
        assert_eq!(parsed.board[1].board_type, "relay16");
    }

    #[test]
    fn parse_minimal_toml() {
        let toml_str = r#"
[[board]]
type = "megaind"
stack = 1
slave_id = 2
"#;
        let cfg: GatewayConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.board.len(), 1);
        assert_eq!(cfg.board[0].board_type, "megaind");
        // Defaults should be applied
        assert_eq!(cfg.server.port, 502);
        assert_eq!(cfg.logging.interval, 5);
    }

    #[test]
    fn parse_empty_toml() {
        let cfg: GatewayConfig = toml::from_str("").unwrap();
        assert!(cfg.board.is_empty());
        assert_eq!(cfg.server.port, 502);
    }
}
