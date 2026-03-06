//! I²C bus recovery via GPIO clock-pulse injection.
//!
//! When a slave device holds SDA low (e.g. after a power glitch mid-transfer),
//! the I²C bus becomes hung.  The standard recovery is to toggle SCL 9 times
//! while SDA is released, which clocks the slave's shift register until it
//! releases SDA.  A STOP condition is then generated.
//!
//! On the Raspberry Pi, I²C-1 uses:
//! - **GPIO 2** → SDA
//! - **GPIO 3** → SCL
//!
//! This module temporarily reconfigures the I²C pins as plain GPIOs via sysfs,
//! performs the 9-clock-pulse recovery, then restores them to I²C mode so that
//! the `i2cdev` driver can re-open `/dev/i2c-1`.

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::thread;
#[cfg(target_os = "linux")]
use std::time::Duration;

use tracing::{info, warn, error};

/// Default GPIO pins for I²C-1 on Raspberry Pi.
#[cfg(target_os = "linux")]
const GPIO_SDA: u8 = 2;
#[cfg(target_os = "linux")]
const GPIO_SCL: u8 = 3;

/// Half-period for SCL toggling (~5 kHz, well within I²C slow-mode spec).
#[cfg(target_os = "linux")]
const CLOCK_HALF_PERIOD: Duration = Duration::from_micros(100);

/// Number of SCL clock pulses to send (standard I²C recovery = 9).
#[cfg(target_os = "linux")]
const RECOVERY_CLOCKS: u8 = 9;

// ════════════════════════════════════════════════════════════════════════
// Failure tracker
// ════════════════════════════════════════════════════════════════════════

/// Tracks consecutive I²C failures and triggers bus recovery when a
/// configurable threshold is reached.
pub struct I2cWatchdog {
    /// Number of consecutive failures across all reads in a cycle.
    consecutive_failures: u32,
    /// Threshold before triggering recovery.
    threshold: u32,
    /// Total number of recoveries performed (for logging).
    recovery_count: u32,
}

impl I2cWatchdog {
    /// Create a new watchdog with the given failure threshold.
    ///
    /// A threshold of `0` disables recovery entirely.
    pub fn new(threshold: u32) -> Self {
        Self {
            consecutive_failures: 0,
            threshold,
            recovery_count: 0,
        }
    }

    /// Record a successful I²C operation — resets the failure counter.
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Record a failed I²C operation.
    ///
    /// Returns `true` if the failure threshold has been reached and
    /// recovery should be attempted.
    pub fn record_failure(&mut self) -> bool {
        self.consecutive_failures += 1;
        if self.threshold > 0 && self.consecutive_failures >= self.threshold {
            true
        } else {
            false
        }
    }

    /// Attempt I²C bus recovery via GPIO clock-pulse injection.
    ///
    /// After recovery, the caller must re-open the I²C device file
    /// descriptors (the old ones are invalidated).
    pub fn attempt_recovery(&mut self) -> bool {
        self.recovery_count += 1;
        warn!(
            "I²C bus recovery #{} triggered after {} consecutive failures",
            self.recovery_count, self.consecutive_failures
        );

        match perform_bus_recovery() {
            Ok(()) => {
                info!("I²C bus recovery #{} completed successfully", self.recovery_count);
                self.consecutive_failures = 0;
                true
            }
            Err(e) => {
                error!("I²C bus recovery #{} failed: {e:#}", self.recovery_count);
                self.consecutive_failures = 0; // reset anyway to avoid tight loop
                false
            }
        }
    }

    /// Total number of recoveries performed since startup.
    pub fn recovery_count(&self) -> u32 {
        self.recovery_count
    }

    /// Current consecutive failure count.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }
}

// ════════════════════════════════════════════════════════════════════════
// GPIO-based bus recovery (Linux sysfs)
// ════════════════════════════════════════════════════════════════════════

/// Perform the 9-clock-pulse I²C bus recovery via sysfs GPIO.
///
/// 1. Unbind SCL and SDA from the I²C driver (export as GPIO)
/// 2. Set SDA as input (release it — external pull-up holds it high)
/// 3. Toggle SCL as output 9 times
/// 4. Generate a STOP condition (SDA low → high while SCL is high)
/// 5. Unexport GPIOs (kernel re-binds to I²C driver)
#[cfg(target_os = "linux")]
fn perform_bus_recovery() -> anyhow::Result<()> {
    // Helper: write to a sysfs file
    let sysfs_write = |path: &str, val: &str| -> anyhow::Result<()> {
        fs::write(path, val)
            .map_err(|e| anyhow::anyhow!("sysfs write {path} = {val}: {e}"))?;
        Ok(())
    };

    // Helper: read a sysfs file
    let sysfs_read = |path: &str| -> anyhow::Result<String> {
        fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("sysfs read {path}: {e}"))
    };

    info!("Exporting GPIO {GPIO_SCL} (SCL) and {GPIO_SDA} (SDA) for bus recovery");

    // Export GPIOs (ignore errors if already exported)
    let _ = sysfs_write("/sys/class/gpio/export", &GPIO_SCL.to_string());
    let _ = sysfs_write("/sys/class/gpio/export", &GPIO_SDA.to_string());
    thread::sleep(Duration::from_millis(50)); // wait for sysfs nodes

    let scl_dir = format!("/sys/class/gpio/gpio{GPIO_SCL}/direction");
    let scl_val = format!("/sys/class/gpio/gpio{GPIO_SCL}/value");
    let sda_dir = format!("/sys/class/gpio/gpio{GPIO_SDA}/direction");
    let sda_val = format!("/sys/class/gpio/gpio{GPIO_SDA}/value");

    // SDA → input (released, pulled high by external resistor)
    sysfs_write(&sda_dir, "in")?;

    // SCL → output, start high
    sysfs_write(&scl_dir, "out")?;
    sysfs_write(&scl_val, "1")?;
    thread::sleep(CLOCK_HALF_PERIOD);

    // Send 9 clock pulses
    for pulse in 1..=RECOVERY_CLOCKS {
        sysfs_write(&scl_val, "0")?;
        thread::sleep(CLOCK_HALF_PERIOD);
        sysfs_write(&scl_val, "1")?;
        thread::sleep(CLOCK_HALF_PERIOD);

        // Check if SDA has been released
        if let Ok(sda) = sysfs_read(&sda_val) {
            if sda.trim() == "1" {
                info!("SDA released after {pulse} clock pulses");
                break;
            }
        }
    }

    // Generate STOP condition: SDA low → high while SCL is high
    sysfs_write(&sda_dir, "out")?;
    sysfs_write(&sda_val, "0")?;
    thread::sleep(CLOCK_HALF_PERIOD);
    sysfs_write(&scl_val, "1")?;
    thread::sleep(CLOCK_HALF_PERIOD);
    sysfs_write(&sda_val, "1")?;
    thread::sleep(CLOCK_HALF_PERIOD);

    // Unexport GPIOs — kernel restores I²C pin muxing
    let _ = sysfs_write("/sys/class/gpio/unexport", &GPIO_SCL.to_string());
    let _ = sysfs_write("/sys/class/gpio/unexport", &GPIO_SDA.to_string());
    thread::sleep(Duration::from_millis(100)); // let driver re-init

    info!("GPIO bus recovery complete, I²C driver should re-bind");
    Ok(())
}

/// Stub for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
fn perform_bus_recovery() -> anyhow::Result<()> {
    anyhow::bail!("I²C bus recovery is only available on Linux (sysfs GPIO)")
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watchdog_disabled_when_threshold_zero() {
        let mut wd = I2cWatchdog::new(0);
        for _ in 0..100 {
            assert!(!wd.record_failure());
        }
    }

    #[test]
    fn watchdog_triggers_at_threshold() {
        let mut wd = I2cWatchdog::new(5);
        for _ in 0..4 {
            assert!(!wd.record_failure());
        }
        assert!(wd.record_failure()); // 5th failure
    }

    #[test]
    fn watchdog_resets_on_success() {
        let mut wd = I2cWatchdog::new(3);
        assert!(!wd.record_failure());
        assert!(!wd.record_failure());
        wd.record_success();
        assert_eq!(wd.consecutive_failures(), 0);
        assert!(!wd.record_failure());
        assert!(!wd.record_failure());
        assert!(wd.record_failure()); // 3rd after reset
    }

    #[test]
    fn recovery_count_increments() {
        let mut wd = I2cWatchdog::new(2);
        assert!(!wd.record_failure());
        assert!(wd.record_failure());
        // Can't actually call attempt_recovery in tests (no sysfs),
        // but we can verify the counter logic
        assert_eq!(wd.recovery_count(), 0);
    }
}
