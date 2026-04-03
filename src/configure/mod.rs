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

use std::path::{Path, PathBuf};

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

pub fn download_boards_to_installed_path() -> Result<()> {
    #[cfg(target_os = "linux")]
    if unsafe { libc::geteuid() } != 0 {
        anyhow::bail!("--download-boards must be run as root to write to {INSTALL_BOARDS_DIR}");
    }

    let dest = Path::new(INSTALL_BOARDS_DIR);
    reset_managed_boards_dir(dest)?;
    let extracted = download_boards_from_github(dest)?;
    println!("Downloaded {extracted} board definition files into {}", dest.display());
    Ok(())
}

/// The actual TUI flow, called after install detection.
fn run_tui(
    boards_dir: &Path,
    output_path: &Path,
    install_boards: Option<&Path>,
) -> Result<()> {
    // ── Discover boards ──────────────────────────────────────────────
    let search_paths = configure_board_search_paths(boards_dir);
    let mut available = discover_configure_boards(boards_dir)?;

    if available.is_empty() {
        println!("\n No board TOML files found in any configure search path.");
        println!(" Boards are required to configure the gateway.");
        println!(" Searched:");
        for path in &search_paths {
            println!("   - {}", path.display());
        }
        println!();
        println!(" Options:");
        println!("  1. Place board definitions in one of the searched directories");
        println!("  2. Download board definitions from GitHub now");
        println!("     or run: sequent-gateway --download-boards");
        println!("  3. Use built-in defaults: sequent-gateway --builtin-defaults");
        println!();

        let download_dest = preferred_download_destination(boards_dir);
        if prompt_for_board_download(&download_dest)? {
            let extracted = download_boards_from_github(&download_dest)?;
            println!("  Downloaded {extracted} board definition files into {}", download_dest.display());
            available = discover_configure_boards(boards_dir)?;
        }

        if available.is_empty() {
            anyhow::bail!("No board TOML files found. Please add board definitions, download them from GitHub, or use --builtin-defaults.");
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
    // ── Restore terminal ─────────────────────────────────────────────
    disable_raw_mode()?;
    std::io::stdout().execute(LeaveAlternateScreen)?;

    result?;

    // ── Post-TUI actions ────────────────────────────────────────────
    if app.saved {
        println!("\n  Configuration saved to: {}", output_path.display());

        // Install boards if requested
        if let Some(dest) = install_boards {
            let source = first_board_dir_with_tomls(&configure_board_search_paths(boards_dir))
                .ok_or_else(|| anyhow::anyhow!("No board definitions were found to copy into {}", dest.display()))?;
            install_board_files(&source, dest)?;
            println!("  Board definitions installed from {} to: {}", source.display(), dest.display());
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
    _install_boards: Option<&Path>,
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
            println!("  Installed binary already matches the current executable.");
            println!("  Restarting from {INSTALL_BIN} ...\n");
            relaunch_from_installed_binary(&current_exe, output_path)?;
            return Ok(Some(Ok(())));
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

    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create install directory {}", parent.display()))?;
    }

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

    // 2. Create config directory
    std::fs::create_dir_all(INSTALL_CONFIG_DIR)?;
    println!("  Config directory: {INSTALL_CONFIG_DIR}");

    // 3. Install board definitions when a source is available
    let boards_dest = std::path::Path::new(INSTALL_BOARDS_DIR);
    std::fs::create_dir_all(boards_dest)?;
    let install_source_candidates = install_board_source_candidates(boards_dir, &current_exe);
    if let Some(source) = first_board_dir_with_tomls(&install_source_candidates) {
        install_board_files(&source, boards_dest)?;
        println!("  Board definitions installed from {} to {INSTALL_BOARDS_DIR}", source.display());
    } else {
        println!("  Warning: no local board definitions were found to install.");
        println!("  Looked in:");
        for path in &install_source_candidates {
            println!("    - {}", path.display());
        }
        println!("  Continuing with the installed binary; board discovery fallback will apply on next launch.");
    }

    println!();
    println!("  Restarting from {INSTALL_BIN} ...");
    println!();

    relaunch_from_installed_binary(&current_exe, output_path)?;
    Ok(Some(Ok(())))
}

#[cfg(target_os = "linux")]
fn relaunch_from_installed_binary(current_exe: &Path, output_path: &Path) -> Result<()> {
    use anyhow::Context;
    use std::process::Command;

    let relaunch_output = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("cannot determine current working directory for relaunch")?
            .join(output_path)
    };
    let args: Vec<String> = vec![
        INSTALL_BIN.to_string(),
        "configure".to_string(),
        "--output".to_string(),
        relaunch_output.to_string_lossy().into(),
    ];

    let mut cmd = Command::new(INSTALL_BIN);
    cmd.args(&args[1..]);
    if let Some(parent) = current_exe.parent() {
        cmd.current_dir(parent);
    }
    let status = cmd.status().with_context(|| format!("Failed to relaunch {INSTALL_BIN}"))?;

    if status.success() {
        Ok(())
    } else {
        eprintln!("\n  ERROR: Failed to relaunch the installed sequent-gateway binary.\n");
        eprintln!("  Tried to launch: {INSTALL_BIN}");
        eprintln!("  With args: {:?}", &args[1..]);
        eprintln!("  Output path: {}", relaunch_output.display());
        eprintln!("  Relaunch cwd: {}", current_exe.parent().unwrap_or_else(|| Path::new(".")).display());
        eprintln!("  Exit status: {status}\n");
        eprintln!("  Please try running 'sequent-gateway configure' manually from the install location.\n");
        anyhow::bail!("Re-launched gateway exited with: {status}")
    }
}

fn configure_board_search_paths(boards_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, boards_dir.to_path_buf());
    push_unique_path(&mut paths, PathBuf::from("boards"));
    push_unique_path(&mut paths, PathBuf::from(INSTALL_BOARDS_DIR));
    paths
}

#[cfg(target_os = "linux")]
fn install_board_source_candidates(boards_dir: &Path, current_exe: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_unique_path(&mut paths, boards_dir.to_path_buf());
    if let Some(parent) = current_exe.parent() {
        push_unique_path(&mut paths, parent.join("boards"));
    }
    push_unique_path(&mut paths, PathBuf::from("boards"));
    paths
}

fn first_board_dir_with_tomls(paths: &[PathBuf]) -> Option<PathBuf> {
    paths
        .iter()
        .find(|path| path.is_dir() && has_toml_files(path))
        .cloned()
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

fn discover_configure_boards(boards_dir: &Path) -> Result<Vec<AvailableBoard>> {
    let search_paths = configure_board_search_paths(boards_dir);
    if let Some(found_dir) = first_board_dir_with_tomls(&search_paths) {
        discover_all_boards(&found_dir)
    } else {
        Ok(Vec::new())
    }
}

fn preferred_download_destination(boards_dir: &Path) -> PathBuf {
    let default_dir = Path::new("boards");
    if boards_dir != default_dir {
        return boards_dir.to_path_buf();
    }

    #[cfg(target_os = "linux")]
    {
        if unsafe { libc::geteuid() } == 0 {
            return PathBuf::from(INSTALL_BOARDS_DIR);
        }
    }

    default_dir.to_path_buf()
}

fn prompt_for_board_download(dest: &Path) -> Result<bool> {
    use std::io::{self, BufRead, Write};

    println!("  Download board definitions from GitHub into {}? [Y/n] ", dest.display());
    print!("  > ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    let answer = input.trim().to_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}

fn download_boards_from_github(dest: &Path) -> Result<usize> {
    use anyhow::Context;
    use std::io::{Cursor, Read, Write};

    const REPO_ARCHIVE_URL: &str = "https://github.com/k-gordon/SequentTCP/archive/refs/heads/main.zip";

    println!("  Downloading boards archive from {REPO_ARCHIVE_URL} ...");
    let response = reqwest::blocking::get(REPO_ARCHIVE_URL)
        .context("Failed to request GitHub repository archive")?
        .error_for_status()
        .context("GitHub archive request returned an error status")?;
    let bytes = response
        .bytes()
        .context("Failed to read GitHub archive response body")?;

    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .context("Failed to open GitHub archive as zip")?;

    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create destination directory {}", dest.display()))?;

    let mut extracted = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)
            .with_context(|| format!("Failed to read zip entry #{index}"))?;
        let Some(relative_path) = archive_boards_relative_path(file.name()) else {
            continue;
        };

        let output_path = dest.join(&relative_path);
        if file.is_dir() {
            std::fs::create_dir_all(&output_path)?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut outfile = std::fs::File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .with_context(|| format!("Failed to read archive entry {}", file.name()))?;
        outfile
            .write_all(&buffer)
            .with_context(|| format!("Failed to write {}", output_path.display()))?;
        if output_path.extension().map_or(false, |ext| ext == "toml") {
            extracted += 1;
        }
    }

    if extracted == 0 {
        anyhow::bail!("GitHub archive did not contain any board TOML files");
    }

    Ok(extracted)
}

fn reset_managed_boards_dir(dest: &Path) -> Result<()> {
    use anyhow::Context;

    if dest.exists() {
        std::fs::remove_dir_all(dest)
            .with_context(|| format!("Failed to clear existing boards directory {}", dest.display()))?;
    }
    std::fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create boards directory {}", dest.display()))?;
    Ok(())
}

fn archive_boards_relative_path(entry_name: &str) -> Option<PathBuf> {
    let path = Path::new(entry_name);
    let parts: Vec<_> = path.iter().collect();
    let boards_index = parts.iter().position(|part| *part == std::ffi::OsStr::new("boards"))?;
    let relative_parts = &parts[boards_index + 1..];
    if relative_parts.is_empty() {
        return None;
    }

    let mut relative = PathBuf::new();
    for part in relative_parts {
        relative.push(part);
    }
    Some(relative)
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
