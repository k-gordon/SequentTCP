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
//!
//! ## Install detection
//!
//! On first run, if the binary is not installed to `/usr/local/bin/`,
//! the wizard offers to copy itself there (along with board definitions
//! into `/etc/sequent-gateway/boards/`).  After install it re-launches
//! from the system path so all subsequent commands use the installed
//! binary.

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

/// Standard system install path for the gateway binary.
#[cfg(target_os = "linux")]
const INSTALL_BIN: &str = "/usr/local/bin/sequent-gateway";
#[cfg(not(target_os = "linux"))]
const INSTALL_BIN: &str = "/usr/local/bin/sequent-gateway";

/// Standard system path for board definitions.
const INSTALL_BOARDS_DIR: &str = "/etc/sequent-gateway/boards";

/// Standard system path for the configuration file.
#[cfg(target_os = "linux")]
const INSTALL_CONFIG_DIR: &str = "/etc/sequent-gateway";
#[cfg(not(target_os = "linux"))]
const INSTALL_CONFIG_DIR: &str = "/etc/sequent-gateway";

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
    // ── Install detection (Linux only) ───────────────────────────────
    #[cfg(target_os = "linux")]
    if let Some(relaunch) = check_install(boards_dir, output_path, install_boards)? {
        return relaunch;
    }

    run_tui(boards_dir, output_path, install_boards)
}

/// The actual TUI flow, called after install detection.
fn run_tui(
    boards_dir: &Path,
    output_path: &Path,
    install_boards: Option<&Path>,
) -> Result<()> {
    // ── Discover boards ──────────────────────────────────────────────
    // Pecking order: ./boards, /etc/sequent-gateway/boards, built-in defaults
    let cwd_boards = Path::new("./boards");
    let sys_boards = Path::new(INSTALL_BOARDS_DIR);
    let mut available = Vec::new();
    if cwd_boards.is_dir() && has_toml_files(cwd_boards) {
        available = discover_all_boards(cwd_boards)?;
    } else if sys_boards.is_dir() && has_toml_files(sys_boards) {
        available = discover_all_boards(sys_boards)?;
    }

    // Optionally: add built-in defaults if no TOMLs found
    // Optionally: add built-in defaults if no TOMLs found
    if available.is_empty() {
        println!("\n  No board TOML files found in ./boards or /etc/sequent-gateway/boards.");
        println!("  Would you like to download the boards directory from GitHub? [Y/n]");
        use std::io::{self, Write, BufRead};
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" || answer == "yes" {
            // Download boards directory from GitHub main branch
            let url = "https://raw.githubusercontent.com/k-gordon/SequentTCP/main/boards.zip";
            let zip_path = "boards_download.zip";
            println!("  Downloading boards from {} ...", url);
            match download_boards_zip(url, zip_path) {
                Ok(_) => {
                    println!("  Extracting boards ...");
                    if let Err(e) = extract_boards_zip(zip_path, "./boards") {
                        anyhow::bail!("Failed to extract boards: {e}");
                    }
                    // Re-discover boards
                    available = discover_all_boards(cwd_boards)?;
                    if available.is_empty() {
                        anyhow::bail!("Downloaded boards directory did not contain any valid board TOML files.");
                    }
                }
                Err(e) => {
                    anyhow::bail!("Failed to download boards directory: {e}");
                }
            }
        } else {
            anyhow::bail!("No board TOML files found. Please place board definitions in ./boards or /etc/sequent-gateway/boards, or use --builtin-defaults.");
        }
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
    // ...existing code...

// Helper: Download boards zip from GitHub
fn download_boards_zip(url: &str, zip_path: &str) -> anyhow::Result<()> {
    use std::fs::File;
    use std::io::copy;
    let resp = reqwest::blocking::get(url)
        .map_err(|e| anyhow::anyhow!("HTTP error: {e}"))?;
    let mut out = File::create(zip_path)?;
    let content = resp.bytes().map_err(|e| anyhow::anyhow!("Read error: {e}"))?;
    copy(&mut content.as_ref(), &mut out)?;
    Ok(())
}

// Helper: Extract boards zip
fn extract_boards_zip(zip_path: &str, dest_dir: &str) -> anyhow::Result<()> {
    use std::fs;
    use zip::ZipArchive;
    let file = fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = std::path::Path::new(dest_dir).join(file.name());
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}
    // ── Restore terminal ─────────────────────────────────────────────
    disable_raw_mode()?;
    std::io::stdout().execute(LeaveAlternateScreen)?;

    result?;

    // ── Post-TUI actions ────────────────────────────────────────────
    if app.saved {
        println!("\n  Configuration saved to: {}", output_path.display());

        // Install boards if requested
        if let Some(dest) = install_boards {
            install_board_files(boards_dir, dest)?;
            println!("  Board definitions installed to: {}", dest.display());
        }

        // Install systemd service if requested
        if app.install_systemd {
            install_systemd_service(output_path)?;
        }

        println!();
        println!("  Start the gateway with:");
        if app.install_systemd {
            println!("    systemctl start sequent-gateway");
            println!("    systemctl enable sequent-gateway  # Enable on boot");
        } else {
            println!("    sequent-gateway --config {}", output_path.display());
        }
        println!();
    } else {
        println!("\n  Configuration cancelled — no changes written.\n");
    }

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Install detection
// ════════════════════════════════════════════════════════════════════════

/// Check whether the binary is installed to the system path.
///
/// If not, prompts the user to install it.  Returns:
/// - `Ok(None)` → not installed but user declined, or already installed
///   — continue to the TUI.
/// - `Ok(Some(Ok(())))` → installed and re-launched from system path;
///   the caller should return this result.
/// - `Err(_)` → install failed.
#[cfg(target_os = "linux")]
fn check_install(
    boards_dir: &Path,
    output_path: &Path,
    install_boards: Option<&Path>,
) -> Result<Option<Result<()>>> {
    use std::io::{self, BufRead, Write};
    use anyhow::Context;

    let current_exe = std::env::current_exe()
        .context("cannot determine own executable path")?;
    let install_path = std::path::Path::new(INSTALL_BIN);

    // Already running from the installed location — nothing to do
    if current_exe == install_path {
        return Ok(None);
    }

    // Check if an installed binary already exists and is up to date
    if install_path.exists() {
        // Compare file sizes as a quick staleness check
        let src_meta = std::fs::metadata(&current_exe).ok();
        let dst_meta = std::fs::metadata(install_path).ok();
        let same_size = match (src_meta, dst_meta) {
            (Some(s), Some(d)) => s.len() == d.len(),
            _ => false,
        };
        if same_size {
            // Installed binary looks current — skip the prompt
            return Ok(None);
        }
    }

    println!();
    println!("  ╔══════════════════════════════════════════════════════════╗");
    println!("  ║  sequent-gateway is not installed to /usr/local/bin/    ║");
    println!("  ╚══════════════════════════════════════════════════════════╝");
    println!();
    println!("  Current:   {}", current_exe.display());
    println!("  Install to: {INSTALL_BIN}");
    println!("  Boards to:  {INSTALL_BOARDS_DIR}");
    println!("  Config dir: {INSTALL_CONFIG_DIR}");
    println!();
    print!("  Install now? [Y/n] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();

    if !answer.is_empty() && answer != "y" && answer != "yes" {
        println!("  Skipped install — continuing from current location.\n");
        return Ok(None);
    }

    // ── Perform installation ─────────────────────────────────────────
    println!();

    // 1. Copy binary
    std::fs::copy(&current_exe, INSTALL_BIN)
        .with_context(|| format!("Failed to copy binary to {INSTALL_BIN}"))?;

    // Ensure executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(INSTALL_BIN, perms)?;
    }
    println!("  Binary installed to {INSTALL_BIN}");

    // 2. Install board definitions
    let boards_dest = std::path::Path::new(INSTALL_BOARDS_DIR);
    install_board_files(boards_dir, boards_dest)?;
    println!("  Board definitions installed to {INSTALL_BOARDS_DIR}");

    // 3. Create config directory
    std::fs::create_dir_all(INSTALL_CONFIG_DIR)?;
    println!("  Config directory: {INSTALL_CONFIG_DIR}");

    println!();
    println!("  Restarting from {INSTALL_BIN} ...");
    println!();

    // ── Re-launch from the installed binary ──────────────────────────
    // After install, let the binary use its default board search logic
    // (./boards first, then /etc/sequent-gateway/boards)
    let mut args: Vec<String> = vec![
        INSTALL_BIN.to_string(),
        "configure".to_string(),
        "--output".to_string(),
        output_path.to_string_lossy().into(),
    ];

    if let Some(ib) = install_boards {
        args.push("--install-boards".to_string());
        args.push(ib.to_string_lossy().into());
    }

    use std::process::Command;
    let mut cmd = Command::new(INSTALL_BIN);
    cmd.args(&args[1..]);
    // Set working directory to the config dir (or current dir if not possible)
    if let Some(parent) = std::path::Path::new(output_path).parent() {
        cmd.current_dir(parent);
    }
    // Pass through relevant environment variables
    for (key, value) in std::env::vars() {
        // Optionally filter or pass all
        cmd.env(key, value);
    }
    let status = cmd.status().with_context(|| format!("Failed to relaunch {INSTALL_BIN}"))?;

    if status.success() {
        Ok(Some(Ok(())))
    } else {
        eprintln!("\n  ERROR: Failed to relaunch the installed sequent-gateway binary.\n");
        eprintln!("  Tried to launch: {INSTALL_BIN}");
        eprintln!("  With args: {:?}", &args[1..]);
        eprintln!("  In working directory: {:?}", std::env::current_dir().unwrap_or_default());
        eprintln!("  Exit status: {status}\n");
        eprintln!("  Please try running 'sequent-gateway configure' manually from the install location.\n");
        anyhow::bail!("Re-launched gateway exited with: {status}")
    }
}

/// Check if a directory contains any `.toml` files.
fn has_toml_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().map_or(false, |ext| ext == "toml"))
        })
        .unwrap_or(false)
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

// ════════════════════════════════════════════════════════════════════════
// Systemd service installation
// ════════════════════════════════════════════════════════════════════════

/// Install the systemd service for the gateway.
fn install_systemd_service(config_path: &Path) -> Result<()> {
  use anyhow::Context;

  // Determine the system config directory
  let config_dir = std::path::PathBuf::from(INSTALL_CONFIG_DIR);
  
  // Create the config directory if it doesn't exist
  std::fs::create_dir_all(&config_dir)
    .context("Failed to create system config directory")?;

  // Copy the config file to the system directory
  let system_config_path = config_dir.join("sequent-gateway.toml");
  std::fs::copy(config_path, &system_config_path)
    .context("Failed to copy config to system directory")?;
  
  println!(" Config file copied to: {}", system_config_path.display());

  // Create the systemd service file
  let service_content = format!(
    r#"[Unit]
Description=Sequent Gateway - Modbus TCP to I²C Bridge
After=network.target

[Service]
Type=simple
User=root
ExecStart={} --config {}
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
    INSTALL_BIN,
    system_config_path.display()
  );

  let service_path = "/etc/systemd/system/sequent-gateway.service";
  std::fs::write(service_path, service_content)
    .context("Failed to write systemd service file")?;
  
  println!(" Systemd service file created: {}", service_path);

  // Reload systemd daemon
  println!(" Reloading systemd daemon...");
  let status = std::process::Command::new("systemctl")
    .arg("daemon-reload")
    .status()
    .context("Failed to run systemctl daemon-reload")?;
  
  if !status.success() {
    anyhow::bail!("systemctl daemon-reload failed");
  }

  // Enable the service
  println!(" Enabling systemd service...");
  let status = std::process::Command::new("systemctl")
    .arg("enable")
    .arg("sequent-gateway.service")
    .status()
    .context("Failed to enable systemd service")?;
  
  if !status.success() {
    anyhow::bail!("systemctl enable failed");
  }
  
  println!(" Service enabled for automatic startup");

  // Start the service
  println!(" Starting systemd service...");
  let status = std::process::Command::new("systemctl")
    .arg("start")
    .arg("sequent-gateway.service")
    .status()
    .context("Failed to start systemd service")?;
  
  if !status.success() {
    anyhow::bail!("systemctl start failed");
  }
  
  println!(" Service started successfully");

  Ok(())
}
