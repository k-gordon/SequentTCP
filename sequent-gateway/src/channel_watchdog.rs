//! Per-channel I²C read watchdog with last-known-good fallback.
//!
//! Each logical I/O channel (4-20 mA bank, 0-10 V bank, system voltage,
//! opto inputs) is tracked independently.  When a read fails the watchdog:
//!
//! 1. Returns the last-known-good value so the Modbus data bank stays fresh.
//! 2. Increments a per-channel consecutive-failure counter.
//! 3. After `fault_threshold` consecutive failures the channel is marked
//!    **FAULT** and appears as such in the heartbeat log.
//!
//! If *all* channels enter FAULT simultaneously the caller can trigger the
//! bus-level GPIO recovery from [`crate::i2c_recovery`].

use tracing::warn;

use crate::registers::{I4_20_IN_CHANNELS, OPTO_CHANNELS, U0_10_IN_CHANNELS};

// ════════════════════════════════════════════════════════════════════════
// Channel identifiers
// ════════════════════════════════════════════════════════════════════════

/// Logical I/O channel groups tracked by the watchdog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Ma,      // 4-20 mA inputs (8 channels as one I²C read)
    Volt,    // 0-10 V inputs  (4 channels as one I²C read)
    Psu,     // System / PSU voltage
    Opto,    // Opto-isolated inputs
}

impl Channel {
    /// All channel variants, for iteration.
    #[allow(dead_code)]
    pub const ALL: [Channel; 4] = [Channel::Ma, Channel::Volt, Channel::Psu, Channel::Opto];

    fn label(self) -> &'static str {
        match self {
            Channel::Ma => "4-20mA",
            Channel::Volt => "0-10V",
            Channel::Psu => "PSU voltage",
            Channel::Opto => "Opto inputs",
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Per-channel state
// ════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct ChannelState {
    consecutive_failures: u32,
    faulted: bool,
}

impl ChannelState {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            faulted: false,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Channel Watchdog
// ════════════════════════════════════════════════════════════════════════

/// Tracks per-channel I²C read health and caches last-known-good values.
#[derive(Debug, Clone)]
pub struct ChannelWatchdog {
    states: [ChannelState; 4],
    fault_threshold: u32,

    // Last-known-good values
    last_ma: [f32; I4_20_IN_CHANNELS],
    last_volt: [f32; U0_10_IN_CHANNELS],
    last_psu: f32,
    last_opto_val: u8,
    last_opto_bits: [bool; OPTO_CHANNELS],
}

impl ChannelWatchdog {
    /// Create a new watchdog.  `fault_threshold` is the number of
    /// consecutive failures before a channel is marked FAULT.
    /// A threshold of `0` disables fault detection (failures are still
    /// counted and last-known-good values still returned).
    pub fn new(fault_threshold: u32) -> Self {
        Self {
            states: [
                ChannelState::new(),
                ChannelState::new(),
                ChannelState::new(),
                ChannelState::new(),
            ],
            fault_threshold,
            last_ma: [0.0; I4_20_IN_CHANNELS],
            last_volt: [0.0; U0_10_IN_CHANNELS],
            last_psu: 0.0,
            last_opto_val: 0,
            last_opto_bits: [false; OPTO_CHANNELS],
        }
    }

    // ── Index helpers ────────────────────────────────────────────────

    fn idx(ch: Channel) -> usize {
        match ch {
            Channel::Ma => 0,
            Channel::Volt => 1,
            Channel::Psu => 2,
            Channel::Opto => 3,
        }
    }

    // ── Success / failure recording ──────────────────────────────────

    /// Record a successful read for the given channel.
    pub fn record_success(&mut self, ch: Channel) {
        let s = &mut self.states[Self::idx(ch)];
        if s.faulted {
            warn!("{} channel recovered from FAULT", ch.label());
        }
        s.consecutive_failures = 0;
        s.faulted = false;
    }

    /// Record a failed read for the given channel.
    /// Returns `true` if the channel just transitioned to FAULT.
    pub fn record_failure(&mut self, ch: Channel) -> bool {
        let s = &mut self.states[Self::idx(ch)];
        s.consecutive_failures += 1;

        let just_faulted = self.fault_threshold > 0
            && s.consecutive_failures >= self.fault_threshold
            && !s.faulted;

        if just_faulted {
            s.faulted = true;
            warn!(
                "{} channel FAULT after {} consecutive failures",
                ch.label(),
                s.consecutive_failures
            );
        } else if s.consecutive_failures == 1 {
            warn!(
                "{} read failed — using last-known-good value",
                ch.label()
            );
        }

        just_faulted
    }

    // ── Fault queries ────────────────────────────────────────────────

    /// Whether a specific channel is in FAULT state.
    pub fn is_faulted(&self, ch: Channel) -> bool {
        self.states[Self::idx(ch)].faulted
    }

    /// Whether **all** channels are simultaneously in FAULT state.
    /// This indicates a probable bus-level failure.
    pub fn all_faulted(&self) -> bool {
        self.fault_threshold > 0 && self.states.iter().all(|s| s.faulted)
    }

    /// Number of consecutive failures for a channel.
    pub fn failure_count(&self, ch: Channel) -> u32 {
        self.states[Self::idx(ch)].consecutive_failures
    }

    // ── Last-known-good value management ─────────────────────────────

    /// Store a good 4-20 mA reading and return it.
    pub fn update_ma(&mut self, values: [f32; I4_20_IN_CHANNELS]) -> [f32; I4_20_IN_CHANNELS] {
        self.record_success(Channel::Ma);
        self.last_ma = values;
        values
    }

    /// Get last-known-good 4-20 mA values (after a failed read).
    pub fn fallback_ma(&mut self) -> [f32; I4_20_IN_CHANNELS] {
        self.record_failure(Channel::Ma);
        self.last_ma
    }

    /// Store a good 0-10 V reading and return it.
    pub fn update_volt(&mut self, values: [f32; U0_10_IN_CHANNELS]) -> [f32; U0_10_IN_CHANNELS] {
        self.record_success(Channel::Volt);
        self.last_volt = values;
        values
    }

    /// Get last-known-good 0-10 V values (after a failed read).
    pub fn fallback_volt(&mut self) -> [f32; U0_10_IN_CHANNELS] {
        self.record_failure(Channel::Volt);
        self.last_volt
    }

    /// Store a good PSU voltage reading and return it.
    pub fn update_psu(&mut self, value: f32) -> f32 {
        self.record_success(Channel::Psu);
        self.last_psu = value;
        value
    }

    /// Get last-known-good PSU voltage (after a failed read).
    pub fn fallback_psu(&mut self) -> f32 {
        self.record_failure(Channel::Psu);
        self.last_psu
    }

    /// Store a good opto reading and return it.
    pub fn update_opto(&mut self, val: u8, bits: [bool; OPTO_CHANNELS]) -> (u8, [bool; OPTO_CHANNELS]) {
        self.record_success(Channel::Opto);
        self.last_opto_val = val;
        self.last_opto_bits = bits;
        (val, bits)
    }

    /// Get last-known-good opto values (after a failed read).
    pub fn fallback_opto(&mut self) -> (u8, [bool; OPTO_CHANNELS]) {
        self.record_failure(Channel::Opto);
        (self.last_opto_val, self.last_opto_bits)
    }

    // ── Heartbeat status strings ─────────────────────────────────────

    /// Returns a short status tag for the heartbeat log.
    pub fn status_tag(&self, ch: Channel) -> &'static str {
        if self.is_faulted(ch) {
            "FAULT"
        } else if self.states[Self::idx(ch)].consecutive_failures > 0 {
            "STALE"
        } else {
            "OK"
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_watchdog_all_ok() {
        let wd = ChannelWatchdog::new(5);
        for ch in Channel::ALL {
            assert!(!wd.is_faulted(ch));
            assert_eq!(wd.failure_count(ch), 0);
            assert_eq!(wd.status_tag(ch), "OK");
        }
        assert!(!wd.all_faulted());
    }

    #[test]
    fn single_failure_returns_stale() {
        let mut wd = ChannelWatchdog::new(5);
        wd.record_failure(Channel::Ma);
        assert_eq!(wd.status_tag(Channel::Ma), "STALE");
        assert!(!wd.is_faulted(Channel::Ma));
    }

    #[test]
    fn fault_at_threshold() {
        let mut wd = ChannelWatchdog::new(3);
        wd.record_failure(Channel::Volt);
        wd.record_failure(Channel::Volt);
        assert!(!wd.is_faulted(Channel::Volt));
        let just_faulted = wd.record_failure(Channel::Volt);
        assert!(just_faulted);
        assert!(wd.is_faulted(Channel::Volt));
        assert_eq!(wd.status_tag(Channel::Volt), "FAULT");
    }

    #[test]
    fn success_clears_fault() {
        let mut wd = ChannelWatchdog::new(2);
        wd.record_failure(Channel::Psu);
        wd.record_failure(Channel::Psu);
        assert!(wd.is_faulted(Channel::Psu));
        wd.record_success(Channel::Psu);
        assert!(!wd.is_faulted(Channel::Psu));
        assert_eq!(wd.failure_count(Channel::Psu), 0);
        assert_eq!(wd.status_tag(Channel::Psu), "OK");
    }

    #[test]
    fn all_faulted_only_when_every_channel_faults() {
        let mut wd = ChannelWatchdog::new(1);
        wd.record_failure(Channel::Ma);
        wd.record_failure(Channel::Volt);
        wd.record_failure(Channel::Psu);
        assert!(!wd.all_faulted()); // Opto still OK
        wd.record_failure(Channel::Opto);
        assert!(wd.all_faulted());
    }

    #[test]
    fn disabled_threshold_never_faults() {
        let mut wd = ChannelWatchdog::new(0);
        for _ in 0..100 {
            wd.record_failure(Channel::Ma);
        }
        assert!(!wd.is_faulted(Channel::Ma));
        assert!(!wd.all_faulted());
        // Still tracks count though
        assert_eq!(wd.failure_count(Channel::Ma), 100);
    }

    #[test]
    fn last_known_good_ma_fallback() {
        let mut wd = ChannelWatchdog::new(5);
        let good = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        wd.update_ma(good);
        let fallback = wd.fallback_ma();
        assert_eq!(fallback, good);
    }

    #[test]
    fn last_known_good_opto_fallback() {
        let mut wd = ChannelWatchdog::new(5);
        let bits = [true, false, true, false, true, false, true, false];
        wd.update_opto(0xAA, bits);
        let (val, fb_bits) = wd.fallback_opto();
        assert_eq!(val, 0xAA);
        assert_eq!(fb_bits, bits);
    }

    #[test]
    fn channels_are_independent() {
        let mut wd = ChannelWatchdog::new(2);
        wd.record_failure(Channel::Ma);
        wd.record_failure(Channel::Ma);
        assert!(wd.is_faulted(Channel::Ma));
        assert!(!wd.is_faulted(Channel::Volt));
        assert!(!wd.is_faulted(Channel::Psu));
        assert!(!wd.is_faulted(Channel::Opto));
    }
}
