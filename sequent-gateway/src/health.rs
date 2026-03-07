//! Lightweight HTTP health endpoint.
//!
//! When `--health-port` is set, a minimal HTTP server responds to
//! `GET /health` with a JSON payload:
//!
//! ```json
//! {
//!   "status": "ok",
//!   "uptime_s": 1234,
//!   "last_cycle_ms": 0.42,
//!   "i2c_errors": 0,
//!   "channels": {
//!     "ma": "OK",
//!     "volt": "OK",
//!     "psu": "OK",
//!     "opto": "OK"
//!   }
//! }
//! ```
//!
//! Implementation uses raw `tokio::net::TcpListener` — no external HTTP
//! framework dependency.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info};

use crate::channel_watchdog::{Channel, ChannelWatchdog};

// ════════════════════════════════════════════════════════════════════════
// Shared health statistics
// ════════════════════════════════════════════════════════════════════════

/// Lock-free health statistics updated by the poll loop and read by the
/// HTTP handler.
///
/// All fields use atomics so the poll thread (std::thread) and the tokio
/// HTTP handler can access them without a mutex.
#[derive(Debug)]
pub struct HealthStats {
    /// Microseconds of the most recent I/O cycle.
    last_cycle_us: AtomicU64,
    /// Cumulative I²C read/write errors since startup.
    i2c_errors: AtomicU64,
    /// Total GPIO-level I²C bus recoveries since startup.
    i2c_recoveries: AtomicU32,
    /// Cumulative relay read-back mismatches since startup.
    relay_mismatches: AtomicU32,
    /// Epoch instant — used to compute uptime.
    start: Instant,
    /// Per-channel fault flags packed as 4 × u8 in a single u32.
    /// Byte 0 = Ma, 1 = Volt, 2 = Psu, 3 = Opto.
    /// 0 = OK, 1 = STALE, 2 = FAULT.
    channel_status: AtomicU32,
}

impl HealthStats {
    pub fn new() -> Self {
        Self {
            last_cycle_us: AtomicU64::new(0),
            i2c_errors: AtomicU64::new(0),
            i2c_recoveries: AtomicU32::new(0),
            relay_mismatches: AtomicU32::new(0),
            start: Instant::now(),
            channel_status: AtomicU32::new(0),
        }
    }

    /// Record the duration of the most recent I/O cycle.
    pub fn set_cycle_time(&self, microseconds: u64) {
        self.last_cycle_us.store(microseconds, Ordering::Relaxed);
    }

    /// Increment the cumulative I²C error counter.
    pub fn inc_i2c_errors(&self) {
        self.i2c_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Update the I²C bus recovery counter from the watchdog.
    pub fn set_recovery_count(&self, count: u32) {
        self.i2c_recoveries.store(count, Ordering::Relaxed);
    }

    /// Increment the relay read-back mismatch counter.
    pub fn inc_relay_mismatches(&self) {
        self.relay_mismatches.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot per-channel health from the channel watchdog.
    pub fn update_channel_status(&self, ch_wd: &ChannelWatchdog) {
        let pack = |ch: Channel| -> u8 {
            if ch_wd.is_faulted(ch) {
                2
            } else if ch_wd.failure_count(ch) > 0 {
                1
            } else {
                0
            }
        };
        let mut val: u32 = 0;
        for (i, &ch) in Channel::ALL.iter().enumerate() {
            val |= (pack(ch) as u32) << (i * 8);
        }
        self.channel_status.store(val, Ordering::Relaxed);
    }

    /// Build the JSON health response body.
    fn to_json(&self) -> String {
        let uptime = self.start.elapsed().as_secs();
        let cycle_us = self.last_cycle_us.load(Ordering::Relaxed);
        let cycle_ms = cycle_us as f64 / 1000.0;
        let errors = self.i2c_errors.load(Ordering::Relaxed);
        let recoveries = self.i2c_recoveries.load(Ordering::Relaxed);
        let relay_mm = self.relay_mismatches.load(Ordering::Relaxed);
        let cs = self.channel_status.load(Ordering::Relaxed);

        let tag = |v: u8| match v {
            0 => "OK",
            1 => "STALE",
            _ => "FAULT",
        };

        let ma = tag((cs & 0xFF) as u8);
        let volt = tag(((cs >> 8) & 0xFF) as u8);
        let psu = tag(((cs >> 16) & 0xFF) as u8);
        let opto = tag(((cs >> 24) & 0xFF) as u8);

        // Determine overall status — degraded if any channel unhealthy
        // or if there have been I²C errors
        let status = if cs == 0 && errors == 0 { "ok" } else { "degraded" };

        format!(
            r#"{{"status":"{}","uptime_s":{},"last_cycle_ms":{:.2},"i2c_errors":{},"i2c_recoveries":{},"relay_mismatches":{},"channels":{{"ma":"{}","volt":"{}","psu":"{}","opto":"{}"}}}}"#,
            status, uptime, cycle_ms, errors, recoveries, relay_mm, ma, volt, psu, opto
        )
    }
}

// ════════════════════════════════════════════════════════════════════════
// HTTP server
// ════════════════════════════════════════════════════════════════════════

/// Run the health HTTP endpoint.
///
/// Listens on `0.0.0.0:{port}` and responds to any request with the JSON
/// health payload.  Non-`/health` paths return 404.
pub async fn serve(port: u16, stats: Arc<HealthStats>) -> anyhow::Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("Health endpoint listening on http://{addr}/health");

    loop {
        let (mut stream, peer) = listener.accept().await?;
        let stats = stats.clone();

        tokio::spawn(async move {
            // Read the request (we only need the first line)
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => n,
                Err(_) => return,
            };

            let request = String::from_utf8_lossy(&buf[..n]);
            let first_line = request.lines().next().unwrap_or("");

            debug!("Health request from {peer}: {first_line}");

            let (status_line, body) = if first_line.starts_with("GET /health") {
                ("HTTP/1.1 200 OK", stats.to_json())
            } else {
                (
                    "HTTP/1.1 404 Not Found",
                    r#"{"error":"not found"}"#.to_string(),
                )
            };

            let response = format!(
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );

            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_format_defaults() {
        let stats = HealthStats::new();
        let json = stats.to_json();
        assert!(json.contains(r#""status":"ok""#));
        assert!(json.contains(r#""last_cycle_ms":0.00"#));
        assert!(json.contains(r#""i2c_errors":0"#));
        assert!(json.contains(r#""i2c_recoveries":0"#));
        assert!(json.contains(r#""relay_mismatches":0"#));
        assert!(json.contains(r#""ma":"OK""#));
        assert!(json.contains(r#""volt":"OK""#));
        assert!(json.contains(r#""psu":"OK""#));
        assert!(json.contains(r#""opto":"OK""#));
    }

    #[test]
    fn json_reflects_cycle_time() {
        let stats = HealthStats::new();
        stats.set_cycle_time(420); // 0.42 ms
        let json = stats.to_json();
        assert!(json.contains(r#""last_cycle_ms":0.42"#));
    }

    #[test]
    fn json_reflects_error_count() {
        let stats = HealthStats::new();
        stats.inc_i2c_errors();
        stats.inc_i2c_errors();
        stats.inc_i2c_errors();
        let json = stats.to_json();
        assert!(json.contains(r#""i2c_errors":3"#));
        // Errors cause degraded status
        assert!(json.contains(r#""status":"degraded""#));
    }

    #[test]
    fn json_reflects_recovery_count() {
        let stats = HealthStats::new();
        stats.set_recovery_count(2);
        let json = stats.to_json();
        assert!(json.contains(r#""i2c_recoveries":2"#));
    }

    #[test]
    fn json_reflects_relay_mismatches() {
        let stats = HealthStats::new();
        stats.inc_relay_mismatches();
        stats.inc_relay_mismatches();
        let json = stats.to_json();
        assert!(json.contains(r#""relay_mismatches":2"#));
    }

    #[test]
    fn json_degraded_when_channel_stale() {
        let stats = HealthStats::new();
        // Simulate a STALE channel (value 1 in byte 0 = Ma)
        stats.channel_status.store(1, Ordering::Relaxed);
        let json = stats.to_json();
        assert!(json.contains(r#""status":"degraded""#));
        assert!(json.contains(r#""ma":"STALE""#));
    }

    #[test]
    fn json_degraded_when_channel_fault() {
        let stats = HealthStats::new();
        // Simulate FAULT in Opto (byte 3 = 2)
        stats.channel_status.store(2 << 24, Ordering::Relaxed);
        let json = stats.to_json();
        assert!(json.contains(r#""status":"degraded""#));
        assert!(json.contains(r#""opto":"FAULT""#));
        // Other channels still OK
        assert!(json.contains(r#""ma":"OK""#));
    }

    #[test]
    fn uptime_increases() {
        let stats = HealthStats::new();
        // Can't easily test time, but uptime should be >= 0
        let json = stats.to_json();
        assert!(json.contains(r#""uptime_s":"#));
    }

    #[test]
    fn update_channel_status_from_watchdog() {
        let stats = HealthStats::new();
        let mut ch_wd = ChannelWatchdog::new(2);
        // Fault the Ma channel
        ch_wd.record_failure(Channel::Ma);
        ch_wd.record_failure(Channel::Ma);
        // Stale the Psu channel (1 failure, not yet FAULT)
        ch_wd.record_failure(Channel::Psu);

        stats.update_channel_status(&ch_wd);
        let json = stats.to_json();
        assert!(json.contains(r#""ma":"FAULT""#));
        assert!(json.contains(r#""psu":"STALE""#));
        assert!(json.contains(r#""volt":"OK""#));
        assert!(json.contains(r#""opto":"OK""#));
        assert!(json.contains(r#""status":"degraded""#));
    }
}
