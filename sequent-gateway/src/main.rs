mod board_def;
mod board_registry;
mod cache;
mod channel_watchdog;
mod cli;
mod config;
mod configure;
mod databank;
mod hal;
mod health;
mod i2c_recovery;
mod modbus;
mod registers;
mod slave_map;
mod validate;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tracing::{debug, error, info, warn};

use board_def::BoardDef;
use board_registry::BoardRegistry;
use channel_watchdog::ChannelWatchdog;
use cli::Cli;
use databank::DataBank;
use hal::driver::GenericBoard;
use hal::traits::{BoardCapability, SequentBoard};
use health::HealthStats;
use i2c_recovery::I2cWatchdog;
use registers::{
    I4_20_IN_CHANNELS, I4_20_OUT_CHANNELS, OD_CHANNELS, OPTO_CHANNELS,
    U0_10_IN_CHANNELS, U0_10_OUT_CHANNELS,
};
use slave_map::SlaveMap;

/// I²C bus device path (standard on Raspberry Pi).
const I2C_BUS: &str = "/dev/i2c-1";

/// Poll loop interval — 10 Hz.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

// ════════════════════════════════════════════════════════════════════════
// Entry point
// ════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    // ── Subcommands — early return before server startup ─────────────
    match &args.command {
        Some(cli::Command::Validate(ref va)) => return validate::run(va),
        Some(cli::Command::Configure(ref ca)) => {
            return configure::run(
                &ca.boards_dir,
                &ca.output,
                ca.install_boards.as_deref(),
            );
        }
        None => {}
    }

    // ── Load config file (if any) ────────────────────────────────────
    let file_config = if let Some(ref path) = args.config {
        Some(config::GatewayConfig::load(path)?)
    } else {
        config::GatewayConfig::default_path()
            .and_then(|p| config::GatewayConfig::load(&p).ok())
    };
    // Config file values are available but CLI args take precedence.
    // For now the file config is loaded for future use; the existing
    // CLI-driven code path is preserved for backward compatibility.
    let _ = &file_config;

    // ── Logging ──────────────────────────────────────────────────────
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "sequent_gateway=info".into());

    // Hold the file appender guard in scope for the lifetime of main().
    // Dropping it flushes remaining log lines.
    let _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>;

    if let Some(ref log_path) = args.log_file {
        // Resolve directory and file-name prefix from the supplied path
        let log_dir = log_path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let log_name = log_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sequent-gateway.log".into());

        let file_appender = tracing_appender::rolling::daily(log_dir, &log_name);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _log_guard = Some(guard);

        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stdout),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(non_blocking),
            )
            .init();

        // Best-effort cleanup of old log files
        cleanup_old_logs(log_dir, &log_name, args.log_retention);

        // This message goes to both stdout and the file
        tracing::info!(
            "Logging to file: {} (daily rotation, retaining {} files)",
            log_path.display(),
            args.log_retention
        );
    } else {
        _log_guard = None;
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .init();
    };

    // ── Root check ───────────────────────────────────────────────────
    #[cfg(unix)]
    {
        if unsafe { libc::geteuid() } != 0 {
            tracing::warn!("Not running as root — I²C access may fail");
        }
    }

    // ── Board definitions ────────────────────────────────────────────
    // Determine which boards to load.  When --board is not specified,
    // default to megaind + relay16 for backward compatibility.
    let board_types: Vec<String> = if args.boards.is_empty() {
        vec!["megaind".into(), "relay16".into()]
    } else {
        args.boards.iter().map(|s| s.to_lowercase()).collect()
    };

    // Load board definitions dynamically from TOML files.
    let mut board_instances: Vec<BoardInstance> = Vec::new();
    for bt in &board_types {
        let toml_path = args.boards_dir.join(format!("{bt}.toml"));
        #[allow(deprecated)]
        let def = if toml_path.exists() {
            BoardDef::load(&toml_path)?
        } else if args.builtin_defaults {
            match bt.as_str() {
                "megaind" => BoardDef::default_megaind(),
                "relay16" => BoardDef::default_relay16(),
                "relay8" => BoardDef::default_relay8(),
                other => anyhow::bail!(
                    "No TOML found for '{other}' and no built-in defaults available.\n\
                     Place a {other}.toml in {} or run with --install-boards.",
                    args.boards_dir.display()
                ),
            }
        } else {
            anyhow::bail!(
                "Board definition not found: {}\n\
                 Place a {bt}.toml in {} or pass --builtin-defaults.",
                toml_path.display(),
                args.boards_dir.display()
            );
        };

        // Assign stack ID based on protocol.
        let stack = if def.board.protocol == "pca9535" {
            args.relay_stack
        } else {
            args.ind_stack
        };

        info!("Loaded board: {} ({})", def.board.name, bt);
        board_instances.push(BoardInstance {
            slug: bt.clone(),
            def,
            stack_id: stack,
        });
    }

    info!(
        "Sequent Gateway v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    info!("Boards: [{}]", board_types.join(", "));
    // Per-board identity logging (name, stack, caps) is handled by
    // registry.log_startup_summary() inside the poll loop, which uses
    // board.name() and board.stack_id() from trait objects.
    if args.map_opto_to_reg {
        info!("Opto-inputs also mapped to Holding Register 15");
    }

    // ── Slave addressing ─────────────────────────────────────────────
    let slave_map = Arc::new(SlaveMap::new(
        args.relay_slave_id,
        args.ind_slave_id,
        args.single_slave,
    ));

    if args.single_slave {
        info!(
            "Modbus addressing: SINGLE-SLAVE (flat map on any Unit ID)"
        );
    } else {
        info!(
            "Modbus addressing: MULTI-SLAVE — Relay HAT = Unit {}, Industrial HAT = Unit {}",
            args.relay_slave_id, args.ind_slave_id
        );
        if args.relay_slave_id == args.ind_slave_id {
            tracing::warn!(
                "relay-slave-id and ind-slave-id are both {}; only one board will be reachable",
                args.relay_slave_id
            );
        }
    }

    // ── Shared state ─────────────────────────────────────────────────
    let data_bank = Arc::new(RwLock::new(DataBank::new()));
    let running = Arc::new(AtomicBool::new(true));
    let health_stats = Arc::new(HealthStats::new());

    // ── I²C poll loop (dedicated OS thread — blocking I/O) ──────────
    let poll_handle = {
        let db = data_bank.clone();
        let run = running.clone();
        let hs = health_stats.clone();

        let config = PollConfig {
            map_opto: args.map_opto_to_reg,
            log_interval: args.log_interval,
            i2c_reset_threshold: args.i2c_reset_threshold,
            channel_fault_threshold: args.channel_fault_threshold,
            relay_verify_interval: args.relay_verify_interval,
            boards: board_instances,
        };

        std::thread::Builder::new()
            .name("i2c-poll".into())
            .spawn(move || {
                poll_loop(db, run, config, hs);
            })?
    };

    // ── Modbus TCP server (async) ────────────────────────────────────
    let health_port = args.health_port;
    tokio::select! {
        result = modbus::serve(&args.host, args.port, data_bank.clone(), slave_map.clone()) => {
            if let Err(e) = result {
                error!("Modbus server error: {e:#}");
            }
        }
        result = async {
            if let Some(port) = health_port {
                health::serve(port, health_stats.clone()).await
            } else {
                // No health port — never resolve (park forever)
                std::future::pending::<anyhow::Result<()>>().await
            }
        } => {
            if let Err(e) = result {
                error!("Health endpoint error: {e:#}");
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    // ── Shutdown ─────────────────────────────────────────────────────
    info!("Shutting down...");
    running.store(false, Ordering::Relaxed);
    poll_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Poll thread panicked"))?;
    info!("Goodbye.");

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Poll loop configuration
// ════════════════════════════════════════════════════════════════════════

/// All parameters needed by the I²C poll loop, replacing the former 14
/// positional arguments.
struct PollConfig {
    map_opto: bool,
    log_interval: u64,
    i2c_reset_threshold: u32,
    channel_fault_threshold: u32,
    relay_verify_interval: u32,
    boards: Vec<BoardInstance>,
}

/// A board instance ready for the poll loop.
struct BoardInstance {
    #[allow(dead_code)]
    slug: String,
    def: BoardDef,
    stack_id: u8,
}

// ════════════════════════════════════════════════════════════════════════
// Board registry builder
// ════════════════════════════════════════════════════════════════════════

/// Construct a [`BoardRegistry`] from the poll configuration.
///
/// Called at startup and after I²C bus recovery to re-open device file
/// descriptors.
fn build_registry(config: &PollConfig) -> BoardRegistry {
    let mut registry = BoardRegistry::new();
    for inst in &config.boards {
        match GenericBoard::new(I2C_BUS, inst.stack_id, &inst.def) {
            Ok(b) => {
                info!("{} opened (stack {})", b.name(), b.stack_id());
                registry.register(Box::new(b));
            }
            Err(e) => error!("Failed to open {}: {e:#}", inst.def.board.name),
        }
    }
    registry
}

// ════════════════════════════════════════════════════════════════════════
// I²C Poll Loop
// ════════════════════════════════════════════════════════════════════════

/// Blocking poll loop running in a dedicated OS thread.
///
/// Each 100 ms tick:
///   1. Iterate registered boards calling `poll_inputs()`
///   2. Apply output state via `apply_outputs()`
///   3. Relay read-back verification (configurable interval)
///   4. Log a heartbeat summary every `log_interval` seconds
fn poll_loop(
    data_bank: Arc<RwLock<DataBank>>,
    running: Arc<AtomicBool>,
    config: PollConfig,
    health_stats: Arc<HealthStats>,
) {
    // ── Build board registry ─────────────────────────────────────────
    let mut registry = build_registry(&config);
    let relay_count = registry.total_relay_count();

    registry.log_startup_summary();

    let mut watchdog = I2cWatchdog::new(config.i2c_reset_threshold);
    let mut ch_wd = ChannelWatchdog::new(config.channel_fault_threshold);
    let mut last_heartbeat = Instant::now();
    let heartbeat_duration = Duration::from_secs(config.log_interval);
    let mut tick_count: u32 = 0;

    if config.relay_verify_interval > 0 && relay_count > 0 {
        info!("Relay read-back verification enabled (every {} ticks)", config.relay_verify_interval);
    }

    if config.i2c_reset_threshold > 0 {
        info!("I²C bus recovery enabled (threshold: {} consecutive failures)", config.i2c_reset_threshold);
    }

    info!("I²C poll loop started ({}Hz)", 1000 / POLL_INTERVAL.as_millis());

    // ── Main loop ────────────────────────────────────────────────────
    while running.load(Ordering::Relaxed) {
        let cycle_start = Instant::now();

        // 1. READ HARDWARE (poll_inputs) ──────────────────────────────
        {
            let mut db = data_bank.write().unwrap();
            let mut any_failure = false;
            for board in registry.boards_mut() {
                match board.poll_inputs(&mut db) {
                    Ok(()) => {
                        watchdog.record_success();
                    }
                    Err(e) => {
                        watchdog.record_failure();
                        health_stats.inc_i2c_errors();
                        error!("{} poll_inputs failed: {e:#}", board.name());
                        any_failure = true;
                    }
                }
            }

            // Channel watchdog: track success/failure across all channels
            if registry.has_capability(BoardCapability::AnalogInputs) {
                if !any_failure {
                    for ch in channel_watchdog::Channel::ALL {
                        ch_wd.record_success(ch);
                    }
                } else {
                    for ch in channel_watchdog::Channel::ALL {
                        ch_wd.record_failure(ch);
                    }
                }
            }

            // map_opto: reconstruct bitmask from discrete_inputs → HR 15
            if config.map_opto {
                let mut bitmask: u16 = 0;
                for (i, &bit) in db.discrete_inputs[..OPTO_CHANNELS].iter().enumerate() {
                    if bit {
                        bitmask |= 1 << i;
                    }
                }
                db.holding_registers[15] = bitmask;
            }
        }

        // ── I²C bus recovery check ──────────────────────────────────
        let bus_recovery_needed =
            (watchdog.consecutive_failures() >= config.i2c_reset_threshold
                && config.i2c_reset_threshold > 0)
            || ch_wd.all_faulted();

        if bus_recovery_needed {
            if ch_wd.all_faulted() {
                warn!("All I/O channels in FAULT — triggering bus recovery");
            }
            if watchdog.attempt_recovery() {
                // Re-open I²C device file descriptors
                registry = build_registry(&config);
                continue; // skip rest of this cycle
            }
        }

        // 2. APPLY OUTPUTS (apply_outputs) ────────────────────────────
        {
            let db = data_bank.read().unwrap();
            for board in registry.boards_mut() {
                if let Err(e) = board.apply_outputs(&db) {
                    health_stats.inc_i2c_errors();
                    error!("{} apply_outputs failed: {e:#}", board.name());
                }
            }
        }

        // 3. RELAY READ-BACK VERIFICATION ─────────────────────────────
        tick_count = tick_count.wrapping_add(1);
        if config.relay_verify_interval > 0
            && relay_count > 0
            && tick_count % config.relay_verify_interval == 0
        {
            for board in registry.boards_mut() {
                if !board.has_capability(BoardCapability::Relays) {
                    continue;
                }
                let board_relays = board.relay_count();
                match board.read_relay_state() {
                    Ok(actual) => {
                        let expected = board.expected_relay_bitmask();
                        let diff = actual ^ expected;
                        let mut effective_diff: u16 = 0;
                        for i in 0..board_relays {
                            if diff & (1 << i) != 0 && board.has_confirmed_relay(i) {
                                effective_diff |= 1 << i;
                            }
                        }
                        if effective_diff != 0 {
                            warn!(
                                "Relay mismatch: expected 0x{expected:04X}, actual 0x{actual:04X} (diff 0x{effective_diff:04X})"
                            );
                            health_stats.inc_relay_mismatches();
                            for i in 0..board_relays {
                                if effective_diff & (1 << i) != 0 {
                                    board.invalidate_relay(i);
                                }
                            }
                        }
                        // Write read-back bitmask to diagnostic holding register
                        {
                            let mut db = data_bank.write().unwrap();
                            db.holding_registers[databank::HR_RELAY_READBACK] = actual;
                        }
                    }
                    Err(e) => {
                        health_stats.inc_i2c_errors();
                        error!("Relay read-back failed: {e:#}");
                    }
                }
            }
        }

        // 4. HEARTBEAT ────────────────────────────────────────────────
        if last_heartbeat.elapsed() >= heartbeat_duration {
            let db = data_bank.read().unwrap();
            log_heartbeat(&db, &registry, &ch_wd, watchdog.recovery_count());
            last_heartbeat = Instant::now();
        }

        // 5. SLEEP FOR REMAINDER OF CYCLE ─────────────────────────────
        let elapsed = cycle_start.elapsed();
        health_stats.set_cycle_time(elapsed.as_micros() as u64);
        health_stats.update_channel_status(&ch_wd);
        health_stats.set_recovery_count(watchdog.recovery_count());
        debug!("I/O cycle: {:.2}ms", elapsed.as_secs_f64() * 1000.0);
        if elapsed < POLL_INTERVAL {
            std::thread::sleep(POLL_INTERVAL - elapsed);
        }
    }

    info!("I²C poll loop stopped");
}

// ════════════════════════════════════════════════════════════════════════
// Heartbeat
// ════════════════════════════════════════════════════════════════════════

/// Log a full system heartbeat, reading all values from the shared
/// [`DataBank`].  Board identity (name, stack ID) comes from the
/// [`BoardRegistry`] via trait objects.  Per-channel health status
/// comes from the [`ChannelWatchdog`].
fn log_heartbeat(
    db: &DataBank,
    registry: &BoardRegistry,
    ch_wd: &ChannelWatchdog,
    i2c_recoveries: u32,
) {
    let relay_count = registry.total_relay_count();
    // Reconstruct display values from DataBank (HR stores × 100)
    let ma_str: String = (0..I4_20_IN_CHANNELS)
        .map(|i| format!("{:4.1}", db.holding_registers[i] as f32 / 100.0))
        .collect::<Vec<_>>()
        .join(" ");

    let v_str: String = (0..U0_10_IN_CHANNELS)
        .map(|i| format!("{:4.1}", db.holding_registers[10 + i] as f32 / 100.0))
        .collect::<Vec<_>>()
        .join(" ");

    let voltage = db.holding_registers[8] as f32 / 100.0;

    let opto_str: String = db.discrete_inputs[..OPTO_CHANNELS]
        .iter()
        .rev()
        .map(|&b| if b { '1' } else { '0' })
        .collect();

    let relay_str: String = (0..relay_count)
        .map(|i| {
            if db.coils.get(i).copied().unwrap_or(false) {
                '1'
            } else {
                '0'
            }
        })
        .collect();

    let od_str: String = (0..OD_CHANNELS)
        .map(|i| {
            if db.coils.get(16 + i).copied().unwrap_or(false) {
                '1'
            } else {
                '0'
            }
        })
        .collect();

    info!("--- SYSTEM HEARTBEAT ---");
    for board in registry.boards() {
        info!("BOARD: {} (stack {})", board.name(), board.stack_id());
    }
    info!("POWER: {voltage:.2}V [{}]", ch_wd.status_tag(channel_watchdog::Channel::Psu));
    info!("4-20mA (1-8) : [{ma_str}] mA [{}]", ch_wd.status_tag(channel_watchdog::Channel::Ma));
    info!("0-10V  (1-4) : [{v_str}] V [{}]", ch_wd.status_tag(channel_watchdog::Channel::Volt));
    info!("OPTO INPUTS  : {opto_str} (Binary) [{}]", ch_wd.status_tag(channel_watchdog::Channel::Opto));
    info!("RELAYS (1-16): {relay_str}");

    let v_out_str: String = (0..U0_10_OUT_CHANNELS)
        .map(|i| format!("{:5.2}", db.holding_registers[databank::HR_U0_10_OUT_BASE + i] as f32 / 100.0))
        .collect::<Vec<_>>()
        .join(" ");
    let ma_out_str: String = (0..I4_20_OUT_CHANNELS)
        .map(|i| format!("{:5.2}", db.holding_registers[databank::HR_I4_20_OUT_BASE + i] as f32 / 100.0))
        .collect::<Vec<_>>()
        .join(" ");

    info!("OD OUT (1-4) : {od_str}");
    info!("V OUT  (1-4) : [{v_out_str}] V");
    info!("mA OUT (1-4) : [{ma_out_str}] mA");
    if i2c_recoveries > 0 {
        info!("I2C RECOVERIES: {i2c_recoveries}");
    }
    info!("------------------------");
}

// ════════════════════════════════════════════════════════════════════════
// Log file cleanup
// ════════════════════════════════════════════════════════════════════════

/// Delete rotated log files older than `keep` days.
///
/// `tracing-appender` creates files like `gateway.log.2026-03-06`.
/// This function lists siblings in `log_dir` that start with `prefix`
/// and removes all but the newest `keep` files.
fn cleanup_old_logs(log_dir: &std::path::Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(log_dir) else {
        return;
    };

    let mut log_files: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(prefix))
                .unwrap_or(false)
        })
        .collect();

    if log_files.len() <= keep {
        return;
    }

    // Sort alphabetically — date-suffixed names sort chronologically
    log_files.sort();

    let to_remove = log_files.len() - keep;
    for path in log_files.iter().take(to_remove) {
        if let Err(e) = std::fs::remove_file(path) {
            tracing::warn!("Failed to remove old log file {}: {e}", path.display());
        } else {
            tracing::debug!("Removed old log file: {}", path.display());
        }
    }
}
