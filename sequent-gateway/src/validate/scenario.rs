//! Scenario configuration — TOML-driven test declarations.
//!
//! Each scenario file describes one gateway configuration to test:
//!
//! ```toml
//! [scenario]
//! name = "Default Multi-Slave"
//!
//! [gateway]
//! boards = ["megaind", "relay16"]
//! single_slave = false
//! health_port = 8080
//! # ... all CLI flags
//!
//! [expect]
//! relay_count = 16
//! opto_channels = 8
//! # ... expected channel counts
//!
//! [tests]
//! health = true
//! relay_writes = true
//! # ... toggle each test category
//! ```
//!
//! Adding a new board or configuration = adding a new TOML file.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

// ════════════════════════════════════════════════════════════════════════
// Raw TOML shape (private — maps 1:1 to file structure)
// ════════════════════════════════════════════════════════════════════════

#[derive(Deserialize)]
struct ScenarioFile {
    scenario: ScenarioMeta,
    #[serde(default)]
    gateway: GatewaySection,
    #[serde(default)]
    expect: ExpectSection,
    #[serde(default)]
    tests: TestSection,
}

#[derive(Deserialize)]
struct ScenarioMeta {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize, Default)]
struct GatewaySection {
    #[serde(default = "default_boards")]
    boards: Vec<String>,
    #[serde(default)]
    single_slave: bool,
    #[serde(default = "one_u16")]
    relay_slave_id: u16,
    #[serde(default = "two_u16")]
    ind_slave_id: u16,
    #[serde(default = "one_u16")]
    ind_stack: u16,
    #[serde(default)]
    relay_stack: u16,
    #[serde(default = "default_health_port")]
    health_port: u16,
    #[serde(default = "default_modbus_port")]
    modbus_port: u16,
    #[serde(default = "bool_true")]
    builtin_defaults: bool,
    #[serde(default)]
    extra_args: Vec<String>,
}

#[derive(Deserialize, Default)]
struct ExpectSection {
    #[serde(default = "sixteen")]
    relay_count: u16,
    #[serde(default = "eight")]
    opto_channels: u16,
    #[serde(default = "eight")]
    ma_in_channels: u16,
    #[serde(default = "four")]
    v_in_channels: u16,
    #[serde(default = "four")]
    od_channels: u16,
    #[serde(default = "four")]
    v_out_channels: u16,
    #[serde(default = "four")]
    ma_out_channels: u16,
    #[serde(default = "bool_true")]
    relay_readback: bool,
}

#[derive(Deserialize, Default)]
struct TestSection {
    #[serde(default = "bool_true")]
    health: bool,
    #[serde(default = "bool_true")]
    analog_inputs: bool,
    #[serde(default = "bool_true")]
    relay_writes: bool,
    #[serde(default = "bool_true")]
    od_outputs: bool,
    #[serde(default = "bool_true")]
    analog_outputs: bool,
    #[serde(default = "bool_true")]
    stability: bool,
}

// Serde default helpers
fn default_boards() -> Vec<String> {
    vec!["megaind".into(), "relay16".into()]
}
fn default_health_port() -> u16 {
    8080
}
fn default_modbus_port() -> u16 {
    502
}
fn one_u16() -> u16 {
    1
}
fn two_u16() -> u16 {
    2
}
fn four() -> u16 {
    4
}
fn eight() -> u16 {
    8
}
fn sixteen() -> u16 {
    16
}
fn bool_true() -> bool {
    true
}

// ════════════════════════════════════════════════════════════════════════
// Public API
// ════════════════════════════════════════════════════════════════════════

/// Fully-resolved scenario configuration.
#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    // Identity
    pub name: String,
    pub description: String,

    // Gateway CLI
    pub boards: Vec<String>,
    pub single_slave: bool,
    pub relay_slave_id: u8,
    pub ind_slave_id: u8,
    pub ind_stack: u8,
    pub relay_stack: u8,
    pub health_port: u16,
    pub modbus_port: u16,
    pub builtin_defaults: bool,
    pub extra_args: Vec<String>,

    // Expected capabilities
    pub relay_count: u16,
    pub opto_channels: u16,
    pub ma_in_channels: u16,
    pub v_in_channels: u16,
    pub od_channels: u16,
    pub v_out_channels: u16,
    pub ma_out_channels: u16,
    pub relay_readback: bool,

    // Test toggles
    pub test_health: bool,
    pub test_analog_inputs: bool,
    pub test_relay_writes: bool,
    pub test_od_outputs: bool,
    pub test_analog_outputs: bool,
    pub test_stability: bool,
}

impl ScenarioConfig {
    /// Load a scenario from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let raw: ScenarioFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

        Ok(Self {
            name: raw.scenario.name,
            description: raw.scenario.description,

            boards: raw.gateway.boards,
            single_slave: raw.gateway.single_slave,
            relay_slave_id: raw.gateway.relay_slave_id as u8,
            ind_slave_id: raw.gateway.ind_slave_id as u8,
            ind_stack: raw.gateway.ind_stack as u8,
            relay_stack: raw.gateway.relay_stack as u8,
            health_port: raw.gateway.health_port,
            modbus_port: raw.gateway.modbus_port,
            builtin_defaults: raw.gateway.builtin_defaults,
            extra_args: raw.gateway.extra_args,

            relay_count: raw.expect.relay_count,
            opto_channels: raw.expect.opto_channels,
            ma_in_channels: raw.expect.ma_in_channels,
            v_in_channels: raw.expect.v_in_channels,
            od_channels: raw.expect.od_channels,
            v_out_channels: raw.expect.v_out_channels,
            ma_out_channels: raw.expect.ma_out_channels,
            relay_readback: raw.expect.relay_readback,

            test_health: raw.tests.health,
            test_analog_inputs: raw.tests.analog_inputs,
            test_relay_writes: raw.tests.relay_writes,
            test_od_outputs: raw.tests.od_outputs,
            test_analog_outputs: raw.tests.analog_outputs,
            test_stability: raw.tests.stability,
        })
    }

    /// Build the CLI argument list for spawning the gateway.
    pub fn gateway_args(&self, gateway_bin: &Path) -> Vec<String> {
        let mut args: Vec<String> = vec![gateway_bin.to_string_lossy().into()];
        for b in &self.boards {
            args.push("--board".into());
            args.push(b.clone());
        }
        args.push("--port".into());
        args.push(self.modbus_port.to_string());
        args.push("--health-port".into());
        args.push(self.health_port.to_string());
        args.push("--ind-stack".into());
        args.push(self.ind_stack.to_string());
        args.push("--relay-stack".into());
        args.push(self.relay_stack.to_string());
        args.push("--relay-slave-id".into());
        args.push(self.relay_slave_id.to_string());
        args.push("--ind-slave-id".into());
        args.push(self.ind_slave_id.to_string());
        if self.single_slave {
            args.push("--single-slave".into());
        }
        if self.builtin_defaults {
            args.push("--builtin-defaults".into());
        }
        for ea in &self.extra_args {
            args.push(ea.clone());
        }
        args
    }

    /// Does this scenario include a MegaInd board?
    pub fn has_megaind(&self) -> bool {
        self.boards.iter().any(|b| b == "megaind")
    }
}

/// Discover all `.toml` scenario files in a directory, sorted by name.
pub fn discover(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        anyhow::bail!("Scenario directory not found: {}", dir.display());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "toml"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        anyhow::bail!("No .toml files in {}", dir.display());
    }
    Ok(paths)
}
