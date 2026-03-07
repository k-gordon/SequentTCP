//! Automated hardware validation runner.
//!
//! Usage (from the repo root on the Pi):
//!
//! ```bash
//! # Validate with explicit board selection:
//! sudo ./target/release/sequent-gateway validate --board megaind --board relay16
//!
//! # Interactive board picker:
//! sudo ./target/release/sequent-gateway validate
//!
//! # Skip relay/OD/analog writes:
//! sudo ./target/release/sequent-gateway validate --skip-writes
//! ```
//!
//! For each scenario the runner:
//!   1. Spawns a child `sequent-gateway` process with the scenario's CLI flags
//!   2. Waits for the health endpoint to respond
//!   3. Connects as a Modbus TCP client
//!   4. Runs all enabled test categories
//!   5. Terminates the child process
//!   6. Prints a per-scenario subtotal
//!
//! A combined PASS / FAIL report is printed at the end and saved to
//! `hw-runner-report.txt`.

pub mod modbus_client;
pub mod results;
pub mod scenario;
mod tests;

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::cli::ValidateArgs;
use modbus_client::ModbusClient;
use results::Results;
use scenario::ScenarioConfig;

// ════════════════════════════════════════════════════════════════════════
// Public entry point
// ════════════════════════════════════════════════════════════════════════

/// Run the full validation suite.
pub fn run(args: &ValidateArgs) -> Result<()> {
    let gateway_bin = match &args.gateway_bin {
        Some(p) => p.clone(),
        None => std::env::current_exe().context("cannot determine own exe path")?,
    };

    // ── Build scenario config(s) ─────────────────────────────────────
    let available = scenario::discover_boards(&args.boards_dir)?;
    let (names, defs) = if !args.boards.is_empty() {
        // CLI-specified boards
        scenario::resolve_boards(&args.boards, &available)?
    } else {
        // Interactive picker
        scenario::pick_boards_interactive(&available)?
    };
    let configs = vec![ScenarioConfig::from_boards(&names, &defs, args)];

    if configs.is_empty() {
        anyhow::bail!("No valid scenarios loaded");
    }

    println!();
    println!("{}", "=".repeat(70));
    println!("  Sequent Gateway -- Automated Hardware Validation");
    println!("  Gateway:    {}", gateway_bin.display());
    println!("  Scenarios:  {}", configs.len());
    for cfg in &configs {
        println!("    * {}", cfg.name);
    }
    println!("{}", "=".repeat(70));

    // ── Run each scenario ────────────────────────────────────────────
    let mut results = Results::new();

    for cfg in &configs {
        run_scenario(cfg, &mut results, &gateway_bin, args)?;
        // Brief pause to let TCP ports release
        thread::sleep(Duration::from_secs(1));
    }

    // ── Final report ─────────────────────────────────────────────────
    let report = results.report();
    println!("{report}");

    let report_path = "hw-runner-report.txt";
    if let Err(e) = std::fs::write(report_path, &report) {
        eprintln!("  WARNING: could not save report: {e}");
    } else {
        println!("  Report saved to: {report_path}");
    }

    let (_, _, failed) = results.totals();
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Per-scenario runner
// ════════════════════════════════════════════════════════════════════════

fn run_scenario(
    cfg: &ScenarioConfig,
    results: &mut Results,
    gateway_bin: &Path,
    args: &ValidateArgs,
) -> Result<()> {
    results.set_scenario(&cfg.name);

    println!();
    println!("{}", "\u{2501}".repeat(70));
    println!("  SCENARIO: {}", cfg.name);
    println!("  {}", cfg.description);
    println!(
        "  Boards: {:?}  |  {}  |  relay_count={}",
        cfg.boards,
        if cfg.single_slave {
            "single-slave"
        } else {
            "multi-slave"
        },
        cfg.relay_count,
    );
    println!("{}", "\u{2501}".repeat(70));

    // ── Launch gateway ───────────────────────────────────────────────
    let cli_args = cfg.gateway_args(gateway_bin);
    println!("    > Launching: {}", cli_args.join(" "));

    let mut child = Command::new(&cli_args[0])
        .args(&cli_args[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {}", cli_args[0]))?;

    // Drain stdout in a background thread so the child doesn't block
    let stdout = child.stdout.take();
    let _drain = stdout.map(|s| {
        thread::spawn(move || {
            let reader = BufReader::new(s);
            for _line in reader.lines().map_while(|l| l.ok()) {
                // Silently consume; could log with --verbose later
            }
        })
    });

    // Wait for health endpoint
    match wait_for_health(cfg.health_port, Duration::from_secs(args.startup_timeout)) {
        Ok(()) => println!("    OK  Gateway healthy on port {}", cfg.health_port),
        Err(e) => {
            results.record("LAUNCH", "Gateway startup", false, &format!("{e:#}"));
            kill_child(&mut child);
            return Ok(());
        }
    }

    // Brief settling period
    thread::sleep(Duration::from_millis(500));

    // ── Run tests ────────────────────────────────────────────────────
    let test_result = run_tests(cfg, results, args);

    // ── Tear down ────────────────────────────────────────────────────
    kill_child(&mut child);

    // Print scenario subtotal
    let (st, sp, sf) = results.scenario_totals(&cfg.name);
    let icon = if sf == 0 { '\u{2705}' } else { '\u{274C}' };
    println!("  {icon} Scenario '{}': {sp}/{st} passed", cfg.name);

    test_result
}

/// Run all enabled test categories for one scenario.
fn run_tests(
    cfg: &ScenarioConfig,
    results: &mut Results,
    args: &ValidateArgs,
) -> Result<()> {
    // ── Health ────────────────────────────────────────────────────────
    if cfg.test_health {
        println!("\n    -- Health Endpoint --");
        tests::test_health(results, cfg);
    }

    // ── Analog inputs ────────────────────────────────────────────────
    let has_analog = cfg.ma_in_channels > 0 || cfg.v_in_channels > 0 || cfg.opto_channels > 0;
    if cfg.test_analog_inputs && has_analog {
        println!("\n    -- Analog Inputs --");
        let mut client = make_client(cfg, "ind")?;
        tests::test_analog_inputs(results, &mut client, cfg);
    }

    // ── Relay writes ─────────────────────────────────────────────────
    if cfg.test_relay_writes && cfg.relay_count > 0 && !args.skip_writes {
        println!("\n    -- Relay Writes --");
        let mut client = make_client(cfg, "relay")?;
        tests::test_relay_writes(results, &mut client, cfg);
    }

    // ── OD outputs ───────────────────────────────────────────────────
    if cfg.test_od_outputs && cfg.od_channels > 0 && !args.skip_writes {
        println!("\n    -- Open-Drain Outputs --");
        let mut client = make_client(cfg, "ind")?;
        tests::test_od_outputs(results, &mut client, cfg);
    }

    // ── Analog outputs ───────────────────────────────────────────────
    let has_aout = cfg.v_out_channels > 0 || cfg.ma_out_channels > 0;
    if cfg.test_analog_outputs && has_aout && !args.skip_writes {
        println!("\n    -- Analog Outputs --");
        let mut client = make_client(cfg, "ind")?;
        tests::test_analog_outputs(results, &mut client, cfg);
    }

    // ── Stability ────────────────────────────────────────────────────
    if cfg.test_stability {
        println!("\n    -- Stability & Performance --");
        tests::test_stability(results, cfg, args.stability_duration);
    }

    Ok(())
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Create a Modbus TCP client pointed at the right Unit ID.
fn make_client(cfg: &ScenarioConfig, board: &str) -> Result<ModbusClient> {
    let uid = if cfg.single_slave {
        cfg.relay_slave_id
    } else if board == "relay" {
        cfg.relay_slave_id
    } else {
        cfg.ind_slave_id
    };
    ModbusClient::connect("127.0.0.1", cfg.modbus_port, uid)
}

/// Poll the health endpoint until it responds or the timeout elapses.
fn wait_for_health(port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(300));
    }
    anyhow::bail!(
        "Gateway health endpoint did not respond on port {port} within {}s",
        timeout.as_secs()
    );
}

/// Gracefully terminate a child process.
fn kill_child(child: &mut Child) {
    println!("    [] Shutting down gateway ...");
    // On Unix, terminate sends SIGTERM.  On Windows, it calls
    // TerminateProcess.
    let _ = child.kill();
    match child.wait() {
        Ok(status) => println!("    [] Gateway exited ({})", status),
        Err(e) => eprintln!("    [] Wait failed: {e}"),
    }
}
