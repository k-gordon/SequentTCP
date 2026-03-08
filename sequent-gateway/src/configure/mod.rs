//! Interactive TUI configuration wizard using `ratatui`.
//!
//! Launched via `sequent-gateway configure`.  Guides the user through:
//!
//! 1. **Board selection** — pick from all discovered board TOMLs
//! 2. **Per-board config** — set stack ID and Modbus slave ID
//! 3. **Server settings** — host, port, health port, addressing mode
//! 4. **I²C tuning** — recovery thresholds, relay verification
//! 5. **Review & save** — preview the generated TOML and write to disk
//!
//! The TUI works on both local terminals and SSH sessions.

pub mod app;
pub mod ui;

use std::path::Path;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;

use crate::board_def::BoardDef;
use crate::config::GatewayConfig;

use app::{App, Screen};

// ════════════════════════════════════════════════════════════════════════
// Public entry point
// ════════════════════════════════════════════════════════════════════════

/// Run the configuration TUI.
///
/// `boards_dir` is the directory containing board TOML files.
/// `output_path` is where the generated config file will be saved.
/// `install_boards` optionally copies board TOMLs to a system directory.
pub fn run(
    boards_dir: &Path,
    output_path: &Path,
    install_boards: Option<&Path>,
) -> Result<()> {
    // ── Discover boards ──────────────────────────────────────────────
    let available = discover_all_boards(boards_dir)?;

    if available.is_empty() {
        anyhow::bail!("No board TOML files found in {}", boards_dir.display());
    }

    // ── Load existing config if present ──────────────────────────────
    let existing = if output_path.exists() {
        GatewayConfig::load(output_path).ok()
    } else {
        None
    };

    // ── Set up terminal ──────────────────────────────────────────────
    enable_raw_mode()?;
    std::io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // ── Run app ──────────────────────────────────────────────────────
    let mut app = App::new(available, existing, output_path.to_path_buf());
    let result = run_app(&mut terminal, &mut app);

    // ── Restore terminal ─────────────────────────────────────────────
    disable_raw_mode()?;
    std::io::stdout().execute(LeaveAlternateScreen)?;

    result?;

    // ── Post-TUI actions ─────────────────────────────────────────────
    if app.saved {
        println!("\n  ✅ Configuration saved to: {}", output_path.display());

        // Install boards if requested
        if let Some(dest) = install_boards {
            install_board_files(boards_dir, dest)?;
            println!("  📦 Board definitions installed to: {}", dest.display());
        }

        println!();
        println!("  Start the gateway with:");
        println!("    sequent-gateway --config {}", output_path.display());
        println!();
    } else {
        println!("\n  Configuration cancelled — no changes written.\n");
    }

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Event loop
// ════════════════════════════════════════════════════════════════════════

fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match key.code {
                KeyCode::Char('q') if app.screen == Screen::BoardSelect => {
                    return Ok(());
                }
                KeyCode::Esc => {
                    if app.screen == Screen::BoardSelect {
                        return Ok(());
                    }
                    app.go_back();
                }
                _ => {
                    if app.handle_key(key.code) {
                        // App signalled exit (saved or quit)
                        return Ok(());
                    }
                }
            }
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Board discovery (includes experimental/)
// ════════════════════════════════════════════════════════════════════════

/// A board available for selection in the TUI.
#[derive(Debug, Clone)]
pub struct AvailableBoard {
    /// Short name from filename (e.g. "megaind").
    pub slug: String,
    /// Human-readable name from TOML.
    pub display_name: String,
    /// Whether this is from the experimental/ subdirectory.
    pub experimental: bool,
    /// Parsed board definition.
    pub def: BoardDef,
    /// Capability summary string.
    pub capabilities: String,
}

/// Discover all board TOML files in `boards_dir` and `boards_dir/experimental/`.
fn discover_all_boards(boards_dir: &Path) -> Result<Vec<AvailableBoard>> {
    let mut boards = Vec::new();

    // Production boards
    if boards_dir.is_dir() {
        collect_boards(boards_dir, false, &mut boards);
    }

    // Experimental boards
    let exp_dir = boards_dir.join("experimental");
    if exp_dir.is_dir() {
        collect_boards(&exp_dir, true, &mut boards);
    }

    // Sort: production first, then experimental, alphabetical within each
    boards.sort_by(|a, b| {
        a.experimental
            .cmp(&b.experimental)
            .then(a.slug.cmp(&b.slug))
    });

    Ok(boards)
}

fn collect_boards(dir: &Path, experimental: bool, out: &mut Vec<AvailableBoard>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |ext| ext != "toml") {
            continue;
        }
        if path.is_dir() {
            continue;
        }
        if let Ok(def) = BoardDef::load(&path) {
            let slug = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
            let capabilities = summarize_capabilities(&def);
            out.push(AvailableBoard {
                display_name: def.board.name.clone(),
                slug,
                experimental,
                def,
                capabilities,
            });
        }
    }
}

fn summarize_capabilities(def: &BoardDef) -> String {
    let ch = &def.channels;
    let mut caps: Vec<String> = Vec::new();
    if let Some(n) = ch.relays {
        caps.push(format!("{n} relays"));
    }
    if let Some(n) = ch.opto_inputs {
        caps.push(format!("{n} opto-in"));
    }
    if let Some(n) = ch.analog_4_20ma_inputs {
        caps.push(format!("{n}× 4-20mA in"));
    }
    if let Some(n) = ch.analog_0_10v_inputs {
        caps.push(format!("{n}× 0-10V in"));
    }
    if let Some(n) = ch.od_outputs {
        caps.push(format!("{n} OD-out"));
    }
    if let Some(n) = ch.analog_0_10v_outputs {
        caps.push(format!("{n}× 0-10V out"));
    }
    if let Some(n) = ch.analog_4_20ma_outputs {
        caps.push(format!("{n}× 4-20mA out"));
    }
    caps.join(", ")
}

// ════════════════════════════════════════════════════════════════════════
// Board library install
// ════════════════════════════════════════════════════════════════════════

/// Copy all board TOML files to a system directory.
fn install_board_files(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;

    // Copy top-level boards
    copy_toml_files(src, dest)?;

    // Copy experimental/ subdirectory
    let exp_src = src.join("experimental");
    let exp_dest = dest.join("experimental");
    if exp_src.is_dir() {
        std::fs::create_dir_all(&exp_dest)?;
        copy_toml_files(&exp_src, &exp_dest)?;
    }

    Ok(())
}

fn copy_toml_files(src: &Path, dest: &Path) -> Result<()> {
    let entries = std::fs::read_dir(src)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |ext| ext != "toml") {
            continue;
        }
        if path.is_dir() {
            continue;
        }
        let dest_file = dest.join(path.file_name().unwrap());
        std::fs::copy(&path, &dest_file)?;
    }
    Ok(())
}
