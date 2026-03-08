//! Scenario configuration — dynamic board-driven test setup.
//!
//! A scenario is built **dynamically** from the board TOML files in
//! `--boards-dir`.  The user selects boards via `--board` flags or an
//! interactive picker; channel counts, test toggles, and addressing
//! are derived automatically.
//!
//! ```bash
//! # Explicit board selection:
//! sequent-gateway validate --board megaind --board relay16
//!
//! # Interactive picker (no --board flags):
//! sequent-gateway validate
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::board_def::BoardDef;
use crate::cli::ValidateArgs;

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
    pub boards_dir: String,

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
    // ── Dynamic builder (primary path) ───────────────────────────────

    /// Build a scenario dynamically from loaded `BoardDef`s and CLI args.
    ///
    /// Channel counts are summed across all selected boards, so adding
    /// a relay board automatically increases `relay_count`, etc.
    pub fn from_boards(board_names: &[String], defs: &[BoardDef], args: &ValidateArgs) -> Self {
        let mut relay_count: u16 = 0;
        let mut opto_channels: u16 = 0;
        let mut ma_in: u16 = 0;
        let mut v_in: u16 = 0;
        let mut od: u16 = 0;
        let mut v_out: u16 = 0;
        let mut ma_out: u16 = 0;
        let mut has_megaind = false;

        for def in defs {
            let ch = &def.channels;
            relay_count += ch.relays.unwrap_or(0) as u16;
            opto_channels += ch.opto_inputs.unwrap_or(0) as u16;
            ma_in += ch.analog_4_20ma_inputs.unwrap_or(0) as u16;
            v_in += ch.analog_0_10v_inputs.unwrap_or(0) as u16;
            od += ch.od_outputs.unwrap_or(0) as u16;
            v_out += ch.analog_0_10v_outputs.unwrap_or(0) as u16;
            ma_out += ch.analog_4_20ma_outputs.unwrap_or(0) as u16;
            if def.board.name.to_lowercase().contains("megaind")
                || def.board.name.to_lowercase().contains("industrial")
            {
                has_megaind = true;
            }
        }

        let label = board_names.join(" + ");
        let mode = if args.single_slave {
            "single-slave"
        } else {
            "multi-slave"
        };

        Self {
            name: format!("{label} ({mode})"),
            description: format!(
                "Dynamic scenario: {} board(s), {mode} addressing",
                board_names.len()
            ),

            boards: board_names.to_vec(),
            single_slave: args.single_slave,
            relay_slave_id: args.relay_slave_id,
            ind_slave_id: args.ind_slave_id,
            ind_stack: args.ind_stack,
            relay_stack: args.relay_stack,
            health_port: args.health_port,
            modbus_port: args.modbus_port,
            boards_dir: args.boards_dir.to_string_lossy().into(),

            relay_count,
            opto_channels,
            ma_in_channels: ma_in,
            v_in_channels: v_in,
            od_channels: od,
            v_out_channels: v_out,
            ma_out_channels: ma_out,
            relay_readback: has_megaind && relay_count > 0,

            test_health: true,
            test_analog_inputs: true,
            test_relay_writes: true,
            test_od_outputs: true,
            test_analog_outputs: true,
            test_stability: true,
        }
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
        args.push("--boards-dir".into());
        args.push(self.boards_dir.clone());
        args
    }

    /// Does this scenario include a MegaInd board?
    pub fn has_megaind(&self) -> bool {
        self.boards.iter().any(|b| b == "megaind")
    }
}

// ════════════════════════════════════════════════════════════════════════
// Board discovery
// ════════════════════════════════════════════════════════════════════════

/// A board available for selection in the interactive picker.
#[derive(Debug, Clone)]
pub struct AvailableBoard {
    /// Short name derived from filename (e.g. "megaind").
    pub slug: String,
    /// Human-readable name from the TOML `[board] name` field.
    pub display_name: String,
    /// Full path to the TOML file.
    #[allow(dead_code)]
    pub path: PathBuf,
    /// Parsed board definition.
    pub def: BoardDef,
}

/// Discover all board TOML files in `boards_dir`, sorted by name.
///
/// Skips the `experimental/` subdirectory — only production-ready
/// boards in the top-level directory are offered for validation.
pub fn discover_boards(boards_dir: &Path) -> Result<Vec<AvailableBoard>> {
    if !boards_dir.is_dir() {
        anyhow::bail!("Boards directory not found: {}", boards_dir.display());
    }
    let mut boards: Vec<AvailableBoard> = Vec::new();
    for entry in std::fs::read_dir(boards_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map_or(true, |ext| ext != "toml") {
            continue;
        }
        // Skip directories (e.g. experimental/)
        if path.is_dir() {
            continue;
        }
        match BoardDef::load(&path) {
            Ok(def) => {
                let slug = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
                boards.push(AvailableBoard {
                    display_name: def.board.name.clone(),
                    slug,
                    path,
                    def,
                });
            }
            Err(e) => {
                eprintln!(
                    "  WARNING: skipping {}: {e:#}",
                    path.display()
                );
            }
        }
    }
    boards.sort_by(|a, b| a.slug.cmp(&b.slug));
    if boards.is_empty() {
        anyhow::bail!("No board TOML files in {}", boards_dir.display());
    }
    Ok(boards)
}

/// Interactive board picker — prompts the user via stdin/stdout.
///
/// Returns the slugs and `BoardDef`s of the selected boards.
pub fn pick_boards_interactive(
    available: &[AvailableBoard],
) -> Result<(Vec<String>, Vec<BoardDef>)> {
    use std::io::{self, BufRead, Write};

    println!();
    println!("  Available boards:");
    println!();
    for (i, b) in available.iter().enumerate() {
        let ch = &b.def.channels;
        let mut caps: Vec<String> = Vec::new();
        if let Some(n) = ch.relays {
            caps.push(format!("{n} relays"));
        }
        if let Some(n) = ch.opto_inputs {
            caps.push(format!("{n} opto"));
        }
        if let Some(n) = ch.analog_4_20ma_inputs {
            caps.push(format!("{n}× 4-20mA in"));
        }
        if let Some(n) = ch.analog_0_10v_inputs {
            caps.push(format!("{n}× 0-10V in"));
        }
        if let Some(n) = ch.od_outputs {
            caps.push(format!("{n} OD out"));
        }
        if let Some(n) = ch.analog_0_10v_outputs {
            caps.push(format!("{n}× 0-10V out"));
        }
        if let Some(n) = ch.analog_4_20ma_outputs {
            caps.push(format!("{n}× 4-20mA out"));
        }
        let cap_str = if caps.is_empty() {
            String::new()
        } else {
            format!("  ({})", caps.join(", "))
        };
        println!("    {}. {} — {}{}", i + 1, b.slug, b.display_name, cap_str);
    }
    println!();
    print!("  Select boards (comma-separated numbers, e.g. 1,2): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;

    let mut names = Vec::new();
    let mut defs = Vec::new();
    for token in input.trim().split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let idx: usize = token
            .parse::<usize>()
            .with_context(|| format!("invalid selection: {token:?}"))?;
        if idx == 0 || idx > available.len() {
            anyhow::bail!("selection out of range: {idx} (1-{})", available.len());
        }
        let board = &available[idx - 1];
        names.push(board.slug.clone());
        defs.push(board.def.clone());
    }
    if names.is_empty() {
        anyhow::bail!("no boards selected");
    }
    Ok((names, defs))
}

/// Resolve board selections from CLI `--board` flags against discovered boards.
pub fn resolve_boards(
    requested: &[String],
    available: &[AvailableBoard],
) -> Result<(Vec<String>, Vec<BoardDef>)> {
    let mut names = Vec::new();
    let mut defs = Vec::new();
    for name in requested {
        let found = available
            .iter()
            .find(|b| b.slug == *name)
            .with_context(|| {
                let slugs: Vec<&str> = available.iter().map(|b| b.slug.as_str()).collect();
                format!(
                    "board {name:?} not found in boards directory. Available: {}",
                    slugs.join(", ")
                )
            })?;
        names.push(found.slug.clone());
        defs.push(found.def.clone());
    }
    Ok((names, defs))
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_def::*;
    use std::path::PathBuf;

    fn stub_args() -> ValidateArgs {
        ValidateArgs {
            gateway_bin: None,
            boards: vec![],
            boards_dir: PathBuf::from("boards"),
            single_slave: false,
            relay_slave_id: 1,
            ind_slave_id: 2,
            ind_stack: 1,
            relay_stack: 0,
            modbus_port: 502,
            health_port: 8080,
            skip_writes: false,
            stability_duration: 5,
            startup_timeout: 10,
        }
    }

    fn megaind_def() -> BoardDef {
        BoardDef {
            board: BoardInfo { name: "MegaInd".into(), protocol: "sequent_mcu".into() },
            address: AddressConfig { base: 0x50, mode: "direct".into() },
            channels: ChannelConfig {
                opto_inputs: Some(8),
                analog_4_20ma_inputs: Some(8),
                analog_0_10v_inputs: Some(4),
                od_outputs: Some(4),
                analog_0_10v_outputs: Some(4),
                analog_4_20ma_outputs: Some(4),
                ..Default::default()
            },
            registers: RegisterMap::default(),
            pca9535: None,
            io_groups: vec![],
        }
    }

    fn relay16_def() -> BoardDef {
        BoardDef {
            board: BoardInfo { name: "16-Relay".into(), protocol: "pca9535".into() },
            address: AddressConfig { base: 0x20, mode: "xor7".into() },
            channels: ChannelConfig { relays: Some(16), ..Default::default() },
            registers: RegisterMap::default(),
            pca9535: Some(Pca9535Config { outport_reg: 0x02, inport_reg: 0x00, config_reg: 0x06 }),
            io_groups: vec![],
        }
    }

    #[test]
    fn from_boards_single_relay_board() {
        let args = stub_args();
        let names = vec!["relay16".into()];
        let defs = vec![relay16_def()];
        let cfg = ScenarioConfig::from_boards(&names, &defs, &args);

        assert_eq!(cfg.relay_count, 16);
        assert_eq!(cfg.opto_channels, 0);
        assert_eq!(cfg.ma_in_channels, 0);
        assert_eq!(cfg.od_channels, 0);
        assert!(!cfg.relay_readback); // no megaind → no readback
        assert!(cfg.name.contains("relay16"));
        assert!(cfg.name.contains("multi-slave"));
    }

    #[test]
    fn from_boards_megaind_plus_relay() {
        let args = stub_args();
        let names = vec!["megaind".into(), "relay16".into()];
        let defs = vec![megaind_def(), relay16_def()];
        let cfg = ScenarioConfig::from_boards(&names, &defs, &args);

        assert_eq!(cfg.relay_count, 16);
        assert_eq!(cfg.opto_channels, 8);
        assert_eq!(cfg.ma_in_channels, 8);
        assert_eq!(cfg.v_in_channels, 4);
        assert_eq!(cfg.od_channels, 4);
        assert_eq!(cfg.v_out_channels, 4);
        assert_eq!(cfg.ma_out_channels, 4);
        assert!(cfg.relay_readback); // megaind + relays → readback
    }

    #[test]
    fn from_boards_single_slave_flag() {
        let mut args = stub_args();
        args.single_slave = true;
        let names = vec!["megaind".into()];
        let defs = vec![megaind_def()];
        let cfg = ScenarioConfig::from_boards(&names, &defs, &args);

        assert!(cfg.single_slave);
        assert!(cfg.name.contains("single-slave"));
    }

    #[test]
    fn from_boards_sums_capabilities_across_boards() {
        let args = stub_args();
        // Two relay boards
        let names = vec!["relay16".into(), "relay8".into()];
        let relay8 = BoardDef {
            board: BoardInfo { name: "8-Relay".into(), protocol: "pca9535".into() },
            address: AddressConfig { base: 0x38, mode: "xor7".into() },
            channels: ChannelConfig { relays: Some(8), ..Default::default() },
            registers: RegisterMap::default(),
            pca9535: Some(Pca9535Config { outport_reg: 0x02, inport_reg: 0x00, config_reg: 0x06 }),
            io_groups: vec![],
        };
        let defs = vec![relay16_def(), relay8];
        let cfg = ScenarioConfig::from_boards(&names, &defs, &args);

        assert_eq!(cfg.relay_count, 24); // 16 + 8
    }

    #[test]
    fn from_boards_passes_cli_addressing() {
        let mut args = stub_args();
        args.relay_slave_id = 5;
        args.ind_slave_id = 10;
        args.ind_stack = 3;
        args.relay_stack = 2;
        args.health_port = 9090;
        args.modbus_port = 5020;
        let names = vec!["megaind".into()];
        let defs = vec![megaind_def()];
        let cfg = ScenarioConfig::from_boards(&names, &defs, &args);

        assert_eq!(cfg.relay_slave_id, 5);
        assert_eq!(cfg.ind_slave_id, 10);
        assert_eq!(cfg.ind_stack, 3);
        assert_eq!(cfg.relay_stack, 2);
        assert_eq!(cfg.health_port, 9090);
        assert_eq!(cfg.modbus_port, 5020);
    }

    #[test]
    fn discover_boards_finds_production_boards() {
        // This test runs from the sequent-gateway/ directory where boards/ exists
        let result = discover_boards(Path::new("boards"));
        if let Ok(boards) = result {
            assert!(boards.len() >= 3, "expected at least megaind, relay16, relay8");
            let slugs: Vec<&str> = boards.iter().map(|b| b.slug.as_str()).collect();
            assert!(slugs.contains(&"megaind"));
            assert!(slugs.contains(&"relay16"));
            assert!(slugs.contains(&"relay8"));
        }
        // If boards/ doesn't exist (e.g. CI), the test simply passes
    }

    #[test]
    fn resolve_boards_matches_by_slug() {
        let available = vec![
            AvailableBoard {
                slug: "megaind".into(),
                display_name: "MegaInd".into(),
                path: PathBuf::from("boards/megaind.toml"),
                def: megaind_def(),
            },
            AvailableBoard {
                slug: "relay16".into(),
                display_name: "16-Relay".into(),
                path: PathBuf::from("boards/relay16.toml"),
                def: relay16_def(),
            },
        ];
        let (names, defs) = resolve_boards(&["relay16".into()], &available).unwrap();
        assert_eq!(names, vec!["relay16"]);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].board.name, "16-Relay");
    }

    #[test]
    fn resolve_boards_unknown_board_errors() {
        let available = vec![AvailableBoard {
            slug: "megaind".into(),
            display_name: "MegaInd".into(),
            path: PathBuf::from("boards/megaind.toml"),
            def: megaind_def(),
        }];
        let result = resolve_boards(&["nonexistent".into()], &available);
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("nonexistent"));
        assert!(msg.contains("megaind")); // lists available boards
    }
}
