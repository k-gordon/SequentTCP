//! Board definitions loaded from TOML files at runtime.
//!
//! Each board type (MegaInd, 16-Relay, 8-Relay, etc.) is described by a
//! `.toml` file in the `boards/` directory.  **TOML files are the primary
//! and only supported way to define board register maps.**  The compiled-in
//! `default_*()` methods are **deprecated** — they exist only as a
//! migration aid behind `--builtin-defaults` and will be removed in a
//! future release.
//!
//! # Adding a new board
//!
//! 1. Copy an existing `.toml` file as a starting point.
//! 2. Set the `protocol` field to match a compiled protocol handler
//!    (`"sequent_mcu"` or `"pca9535"`).
//! 3. Fill in the address, channel, and register details.
//! 4. Drop the file into the `boards/` directory.
//!
//! The gateway will pick it up on next restart.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

// ════════════════════════════════════════════════════════════════════════
// Board definition types
// ════════════════════════════════════════════════════════════════════════

/// Complete board definition — deserialized from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardDef {
    pub board: BoardInfo,
    pub address: AddressConfig,
    #[serde(default)]
    pub channels: ChannelConfig,
    #[serde(default)]
    pub registers: RegisterMap,
    pub pca9535: Option<Pca9535Config>,
}

/// Board identity and protocol selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardInfo {
    /// Human-readable board name (for logging).
    pub name: String,
    /// Protocol handler to use: `"sequent_mcu"` or `"pca9535"`.
    pub protocol: String,
}

/// I²C address calculation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressConfig {
    /// Base I²C address (e.g. `0x20` for 16-Relay, `0x50` for MegaInd).
    pub base: u16,
    /// Address mode:
    /// - `"direct"` → `base + stack_id`
    /// - `"xor7"`   → `(base + stack_id) ^ 0x07`
    pub mode: String,
}

impl AddressConfig {
    /// Compute the actual I²C slave address for a given stack ID.
    #[allow(dead_code)] // called from Linux HAL constructors and tests
    pub fn resolve(&self, stack_id: u8) -> u16 {
        let raw = self.base + stack_id as u16;
        match self.mode.as_str() {
            "xor7" => raw ^ 0x07,
            _ => raw, // "direct" or unrecognised → simple addition
        }
    }
}

/// Channel counts and remapping tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Number of relay channels (16-Relay HAT).
    pub relays: Option<usize>,
    /// Channel-to-bit remapping: index `N-1` → bit position for relay `N`.
    pub relay_remap: Option<Vec<u8>>,
    /// Number of opto-isolated input channels.
    pub opto_inputs: Option<usize>,
    /// Number of 4-20 mA input channels.
    pub analog_4_20ma_inputs: Option<usize>,
    /// Number of 0-10 V input channels.
    pub analog_0_10v_inputs: Option<usize>,
    /// Number of open-drain output channels.
    pub od_outputs: Option<usize>,
    /// Number of 0-10 V analog output channels.
    pub analog_0_10v_outputs: Option<usize>,
    /// Number of 4-20 mA analog output channels.
    pub analog_4_20ma_outputs: Option<usize>,
}

/// Register addresses for Sequent custom-MCU boards.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegisterMap {
    pub relay_val: Option<u8>,
    pub relay_set: Option<u8>,
    pub relay_clr: Option<u8>,
    pub opto_in: Option<u8>,
    pub u0_10_out: Option<u8>,
    pub i4_20_out: Option<u8>,
    pub od_pwm: Option<u8>,
    pub u0_10_in: Option<u8>,
    pub u_pm_10_in: Option<u8>,
    pub i4_20_in: Option<u8>,
    pub calib_value: Option<u8>,
    pub diag_temperature: Option<u8>,
    pub diag_24v: Option<u8>,
    pub diag_5v: Option<u8>,
    pub revision_major: Option<u8>,
    pub revision_minor: Option<u8>,
    pub voltage_scale: Option<f32>,
}

/// PCA9535 I/O-expander register addresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pca9535Config {
    /// Output Port register address (read/write, 2 bytes LE).
    pub outport_reg: u8,
    /// Input Port register address (read, 2 bytes LE).
    pub inport_reg: u8,
    /// Configuration register address (0 = output, 1 = input, 2 bytes LE).
    pub config_reg: u8,
}

// ════════════════════════════════════════════════════════════════════════
// Loading
// ════════════════════════════════════════════════════════════════════════

impl BoardDef {
    /// Load a board definition from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Cannot read {}", path.display()))?;
        let def: BoardDef = toml::from_str(&content)
            .with_context(|| format!("Cannot parse {}", path.display()))?;
        Ok(def)
    }

    /// Try to load from `path`; fall back to compiled defaults if missing
    /// **and** `allow_builtin` is true.  Otherwise bail with a helpful
    /// message telling the user where to put the TOML file.
    pub fn load_or_default(path: &Path, default: Self, allow_builtin: bool) -> Result<Self> {
        match Self::load(path) {
            Ok(def) => {
                info!(
                    "Loaded board definition: {} ({})",
                    def.board.name,
                    path.display()
                );
                Ok(def)
            }
            Err(_) if allow_builtin => {
                info!(
                    "No TOML at {}, using built-in defaults for {} (--builtin-defaults)",
                    path.display(),
                    default.board.name
                );
                Ok(default)
            }
            Err(e) => {
                anyhow::bail!(
                    "Board definition not found: {}\n\
                     Either create the TOML file or pass --builtin-defaults \
                     to use compiled-in register maps.\n\
                     Underlying error: {e:#}",
                    path.display()
                );
            }
        }
    }

    // ── Built-in defaults (DEPRECATED — use boards/*.toml) ──────────

    /// Compiled defaults for the Sequent MegaInd Industrial HAT.
    ///
    /// # Deprecated
    /// Use `boards/megaind.toml` instead. These compiled-in defaults are
    /// only reachable when `--builtin-defaults` is passed and the TOML
    /// file is missing. They will be removed in a future release.
    #[deprecated(
        since = "0.9.0",
        note = "use boards/megaind.toml instead of compiled-in defaults"
    )]
    pub fn default_megaind() -> Self {
        Self {
            board: BoardInfo {
                name: "Sequent MegaInd Industrial HAT".into(),
                protocol: "sequent_mcu".into(),
            },
            address: AddressConfig {
                base: 0x50,
                mode: "direct".into(),
            },
            channels: ChannelConfig {
                opto_inputs: Some(8),
                analog_4_20ma_inputs: Some(8),
                analog_0_10v_inputs: Some(4),
                od_outputs: Some(4),
                analog_0_10v_outputs: Some(4),
                analog_4_20ma_outputs: Some(4),
                ..Default::default()
            },
            registers: RegisterMap {
                relay_val: Some(0x00),
                relay_set: Some(0x01),
                relay_clr: Some(0x02),
                opto_in: Some(0x03),
                u0_10_out: Some(0x04),
                i4_20_out: Some(0x0C),
                od_pwm: Some(0x14),
                u0_10_in: Some(0x1C),
                u_pm_10_in: Some(0x24),
                i4_20_in: Some(0x2C),
                calib_value: Some(0x3C),
                diag_temperature: Some(0x72),
                diag_24v: Some(0x73),
                diag_5v: Some(0x75),
                revision_major: Some(0x78),
                revision_minor: Some(0x79),
                voltage_scale: Some(1000.0),
            },
            pca9535: None,
        }
    }

    /// Compiled defaults for the Sequent 16-Relay HAT.
    ///
    /// # Deprecated
    /// Use `boards/relay16.toml` instead. These compiled-in defaults are
    /// only reachable when `--builtin-defaults` is passed and the TOML
    /// file is missing. They will be removed in a future release.
    #[deprecated(
        since = "0.9.0",
        note = "use boards/relay16.toml instead of compiled-in defaults"
    )]
    pub fn default_relay16() -> Self {
        Self {
            board: BoardInfo {
                name: "Sequent 16-Relay HAT".into(),
                protocol: "pca9535".into(),
            },
            address: AddressConfig {
                base: 0x20,
                mode: "xor7".into(),
            },
            channels: ChannelConfig {
                relays: Some(16),
                relay_remap: Some(vec![
                    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
                ]),
                ..Default::default()
            },
            registers: RegisterMap::default(),
            pca9535: Some(Pca9535Config {
                outport_reg: 0x02,
                inport_reg: 0x00,
                config_reg: 0x06,
            }),
        }
    }

    /// Compiled defaults for the Sequent 8-Relay HAT.
    ///
    /// # Deprecated
    /// Use `boards/relay8.toml` instead. These compiled-in defaults are
    /// only reachable when `--builtin-defaults` is passed and the TOML
    /// file is missing. They will be removed in a future release.
    #[deprecated(
        since = "0.9.0",
        note = "use boards/relay8.toml instead of compiled-in defaults"
    )]
    pub fn default_relay8() -> Self {
        Self {
            board: BoardInfo {
                name: "Sequent 8-Relay HAT".into(),
                protocol: "pca9535".into(),
            },
            address: AddressConfig {
                base: 0x38,
                mode: "xor7".into(),
            },
            channels: ChannelConfig {
                relays: Some(8),
                relay_remap: Some(vec![7, 6, 5, 4, 3, 2, 1, 0]),
                ..Default::default()
            },
            registers: RegisterMap::default(),
            pca9535: Some(Pca9535Config {
                outport_reg: 0x02,
                inport_reg: 0x00,
                config_reg: 0x06,
            }),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use crate::registers;

    #[test]
    fn xor7_address_resolution() {
        let cfg = AddressConfig {
            base: 0x20,
            mode: "xor7".into(),
        };
        assert_eq!(cfg.resolve(0), 0x27);
        assert_eq!(cfg.resolve(1), 0x26);
        assert_eq!(cfg.resolve(7), 0x20);
    }

    #[test]
    fn direct_address_resolution() {
        let cfg = AddressConfig {
            base: 0x50,
            mode: "direct".into(),
        };
        assert_eq!(cfg.resolve(0), 0x50);
        assert_eq!(cfg.resolve(1), 0x51);
        assert_eq!(cfg.resolve(7), 0x57);
    }

    #[test]
    fn relay16_defaults_match_registers() {
        let def = BoardDef::default_relay16();
        assert_eq!(def.address.base, registers::RELAY16_BASE_ADDR);
        assert_eq!(
            def.address.resolve(0),
            (registers::RELAY16_BASE_ADDR) ^ 0x07
        );
        let pca = def.pca9535.as_ref().unwrap();
        assert_eq!(pca.outport_reg, registers::RELAY16_OUTPORT_REG);
        assert_eq!(pca.config_reg, registers::RELAY16_CFG_REG);
        assert_eq!(pca.inport_reg, registers::RELAY16_INPORT_REG);
        let remap = def.channels.relay_remap.as_ref().unwrap();
        assert_eq!(remap.as_slice(), &registers::RELAY_CH_REMAP);
    }

    #[test]
    fn megaind_defaults_match_registers() {
        let def = BoardDef::default_megaind();
        assert_eq!(def.address.base, registers::MEGAIND_BASE_ADDR);
        assert_eq!(def.address.resolve(1), 0x51);
        assert_eq!(
            def.registers.opto_in.unwrap(),
            registers::I2C_MEM_OPTO_IN_VAL
        );
        assert_eq!(
            def.registers.i4_20_in.unwrap(),
            registers::I2C_MEM_I4_20_IN_VAL1
        );
        assert_eq!(
            def.registers.diag_24v.unwrap(),
            registers::I2C_MEM_DIAG_24V
        );
        assert_eq!(
            def.registers.voltage_scale.unwrap(),
            registers::VOLT_TO_MILLIVOLT
        );
    }

    #[test]
    fn toml_roundtrip_relay16() {
        let def = BoardDef::default_relay16();
        let toml_str = toml::to_string_pretty(&def).unwrap();
        let parsed: BoardDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.board.name, def.board.name);
        assert_eq!(parsed.board.protocol, "pca9535");
        assert_eq!(parsed.address.base, def.address.base);
        assert_eq!(parsed.address.mode, "xor7");
        assert!(parsed.pca9535.is_some());
    }

    #[test]
    fn toml_roundtrip_megaind() {
        let def = BoardDef::default_megaind();
        let toml_str = toml::to_string_pretty(&def).unwrap();
        let parsed: BoardDef = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.board.name, def.board.name);
        assert_eq!(parsed.board.protocol, "sequent_mcu");
        assert_eq!(parsed.address.base, 0x50);
        assert_eq!(parsed.address.mode, "direct");
        assert!(parsed.pca9535.is_none());
    }

    #[test]
    fn toml_file_parse_relay16() {
        let toml_str = r#"
[board]
name = "Sequent 16-Relay HAT"
protocol = "pca9535"

[address]
base = 0x20
mode = "xor7"

[channels]
relays = 16
relay_remap = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]

[pca9535]
outport_reg = 0x02
inport_reg = 0x00
config_reg = 0x06
"#;
        let def: BoardDef = toml::from_str(toml_str).unwrap();
        assert_eq!(def.address.resolve(0), 0x27);
        assert_eq!(def.pca9535.as_ref().unwrap().outport_reg, 0x02);
        assert_eq!(def.channels.relays.unwrap(), 16);
    }

    #[test]
    fn relay8_defaults_have_correct_address() {
        let def = BoardDef::default_relay8();
        assert_eq!(def.address.base, registers::RELAY8_BASE_ADDR);
        assert_eq!(def.address.resolve(0), 0x38 ^ 0x07);
        assert_eq!(def.channels.relays.unwrap(), 8);
        let remap = def.channels.relay_remap.as_ref().unwrap();
        assert_eq!(remap.len(), 8);
        assert_eq!(remap[0], 7); // relay 1 → bit 7
        assert_eq!(remap[7], 0); // relay 8 → bit 0
        assert!(def.pca9535.is_some());
    }

    #[test]
    fn toml_file_parse_relay8() {
        let toml_str = include_str!("../boards/relay8.toml");
        let def: BoardDef = toml::from_str(toml_str).unwrap();
        assert_eq!(def.board.name, "Sequent 8-Relay HAT");
        assert_eq!(def.board.protocol, "pca9535");
        assert_eq!(def.address.base, 0x38);
        assert_eq!(def.address.mode, "xor7");
        assert_eq!(def.channels.relays.unwrap(), 8);
        assert_eq!(def.channels.relay_remap.as_ref().unwrap().len(), 8);
        assert!(def.pca9535.is_some());
    }

    // ── Experimental board TOML parse tests ─────────────────────────────
    // Verify every experimental TOML file deserializes without error.

    macro_rules! experimental_parse_test {
        ($name:ident, $file:expr, $board_name:expr, $proto:expr) => {
            #[test]
            fn $name() {
                let toml_str = include_str!(concat!("../boards/experimental/", $file));
                let def: BoardDef = toml::from_str(toml_str)
                    .unwrap_or_else(|e| panic!("failed to parse {}: {}", $file, e));
                assert_eq!(def.board.name, $board_name);
                assert_eq!(def.board.protocol, $proto);
            }
        };
    }

    // PCA9535 boards
    experimental_parse_test!(exp_4relay,       "4relay.toml",       "Sequent 4-Relay HAT",                        "pca9535");
    experimental_parse_test!(exp_8mosfet,      "8mosfet.toml",      "Sequent 8-MOSFET HAT (relay outputs)",       "pca9535");
    experimental_parse_test!(exp_8relayhv,     "8relayhv.toml",     "Sequent 8-Relay HV HAT",                     "pca9535");
    experimental_parse_test!(exp_16inputs,     "16inputs.toml",     "Sequent 16-Digital-Input HAT",                "pca9535");
    experimental_parse_test!(exp_4relind_pca,  "4relind_pca.toml",  "Sequent 4-Relay Industrial HAT (relays)",     "pca9535");
    experimental_parse_test!(exp_8inputs_pca,  "8inputs_pca.toml",  "Sequent 8-Input HAT (digital inputs)",        "pca9535");
    experimental_parse_test!(exp_16inpind_pca, "16inpind_pca.toml", "Sequent 16-Input Industrial HAT",             "pca9535");

    // Sequent MCU boards
    experimental_parse_test!(exp_megabas,      "megabas.toml",      "Sequent Building Automation HAT (MegaBAS)",   "sequent_mcu");
    experimental_parse_test!(exp_ioplus,       "ioplus.toml",       "Sequent IO-Plus HAT",                         "sequent_mcu");
    experimental_parse_test!(exp_4rel4in,      "4rel4in.toml",      "Sequent 4-Relay 4-Input HAT",                 "sequent_mcu");
    experimental_parse_test!(exp_rtd,          "rtd.toml",          "Sequent RTD Data Acquisition HAT",            "sequent_mcu");
    experimental_parse_test!(exp_smtc,         "smtc.toml",         "Sequent 8-Thermocouple HAT",                  "sequent_mcu");
    experimental_parse_test!(exp_multiio,      "multiio.toml",      "Sequent Multi-IO HAT",                        "sequent_mcu");
    experimental_parse_test!(exp_16uout,       "16uout.toml",       "Sequent 16 Analog 0-10V Output HAT",          "sequent_mcu");
    experimental_parse_test!(exp_16univin,     "16univin.toml",     "Sequent 16 Universal Input HAT",              "sequent_mcu");
    experimental_parse_test!(exp_megaio,       "megaio.toml",       "Sequent Mega-IO HAT",                         "sequent_mcu");
    experimental_parse_test!(exp_megaioind,    "megaioind.toml",    "Sequent MegaIO Industrial HAT",               "sequent_mcu");
    experimental_parse_test!(exp_3relind,      "3relind.toml",      "Sequent 3-Relay Industrial HAT",              "sequent_mcu");
    experimental_parse_test!(exp_wdt,          "wdt.toml",          "Sequent Watchdog Timer HAT",                  "sequent_mcu");
    experimental_parse_test!(exp_8crt,         "8crt.toml",         "Sequent 8-Current Transducer HAT",            "sequent_mcu");
    experimental_parse_test!(exp_4relind_mcu,  "4relind_mcu.toml",  "Sequent 4-Relay Industrial HAT (analog)",     "sequent_mcu");
    experimental_parse_test!(exp_8mosind_mcu,  "8mosind_mcu.toml",  "Sequent 8-MOSFET Industrial HAT (analog)",    "sequent_mcu");
    experimental_parse_test!(exp_8inputs_mcu,  "8inputs_mcu.toml",  "Sequent 8-Input HAT (analog)",                "sequent_mcu");
    experimental_parse_test!(exp_24b8vin,      "24b8vin.toml",      "Sequent 24-Bit 8-Voltage-Input HAT",          "sequent_mcu");
    experimental_parse_test!(exp_ti,           "ti.toml",           "Sequent Thermal Interface HAT",               "sequent_mcu");
    experimental_parse_test!(exp_smartfan,     "smartfan.toml",     "Sequent Smart Fan HAT",                       "sequent_mcu");
    experimental_parse_test!(exp_plcpi,        "plcpi.toml",        "Sequent PLC-PI08 HAT",                        "sequent_mcu");
    experimental_parse_test!(exp_fsrc,         "fsrc.toml",         "Sequent FSRC Controller HAT",                 "sequent_mcu");
    experimental_parse_test!(exp_lkit,         "lkit.toml",         "Sequent Learning Kit HAT",                    "sequent_mcu");
    experimental_parse_test!(exp_dash,         "dash.toml",         "Sequent Dashboard Controller HAT",            "sequent_mcu");
}
