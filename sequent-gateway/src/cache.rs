//! Output state caching.
//!
//! Tracks the last-written state of relays and open-drain outputs.
//! An I²C write is only issued when the requested state differs from
//! the cached value — preventing I²C bus saturation when the vPLC
//! holds outputs steady.
//!
//! On startup, all slots are `None` (unknown), forcing every output
//! to be written on the first cycle so hardware is synchronised.

use crate::registers::{
    I4_20_OUT_CHANNELS, OD_CHANNELS, RELAY16_CHANNELS, U0_10_OUT_CHANNELS,
};

/// Cached output state for relays, OD outputs, and analog outputs.
#[derive(Debug)]
pub struct OutputCache {
    relays: [Option<bool>; RELAY16_CHANNELS],
    od_outputs: [Option<bool>; OD_CHANNELS],
    v_outputs: [Option<u16>; U0_10_OUT_CHANNELS],
    ma_outputs: [Option<u16>; I4_20_OUT_CHANNELS],
}

impl OutputCache {
    /// Create a new cache with all states unknown (`None`).
    pub fn new() -> Self {
        Self {
            relays: [None; RELAY16_CHANNELS],
            od_outputs: [None; OD_CHANNELS],
            v_outputs: [None; U0_10_OUT_CHANNELS],
            ma_outputs: [None; I4_20_OUT_CHANNELS],
        }
    }

    // ── Relays ───────────────────────────────────────────────────────

    /// Returns `true` if the relay at `index` needs a write (state changed or unknown).
    pub fn should_update_relay(&self, index: usize, new_state: bool) -> bool {
        self.relays
            .get(index)
            .map_or(false, |cached| *cached != Some(new_state))
    }

    /// Mark relay at `index` as successfully written.
    pub fn confirm_relay(&mut self, index: usize, state: bool) {
        if let Some(slot) = self.relays.get_mut(index) {
            *slot = Some(state);
        }
    }

    /// Clear cached state so the next cycle retries the write.
    pub fn invalidate_relay(&mut self, index: usize) {
        if let Some(slot) = self.relays.get_mut(index) {
            *slot = None;
        }
    }

    /// Build a u16 bitmask of the expected relay state.
    ///
    /// Only the first `count` relay bits are included.
    /// `Some(true)` → bit set, `Some(false)` or `None` → bit clear.
    pub fn relay_bitmask(&self, count: usize) -> u16 {
        let mut mask: u16 = 0;
        for i in 0..count.min(self.relays.len()) {
            if self.relays[i] == Some(true) {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Returns `true` if relay at `index` has a confirmed (known) state.
    pub fn has_confirmed_relay(&self, index: usize) -> bool {
        self.relays.get(index).map_or(false, |s| s.is_some())
    }

    // ── Open-Drain Outputs ───────────────────────────────────────────

    /// Returns `true` if the OD output at `index` needs a write.
    pub fn should_update_od(&self, index: usize, new_state: bool) -> bool {
        self.od_outputs
            .get(index)
            .map_or(false, |cached| *cached != Some(new_state))
    }

    /// Mark OD output at `index` as successfully written.
    pub fn confirm_od(&mut self, index: usize, state: bool) {
        if let Some(slot) = self.od_outputs.get_mut(index) {
            *slot = Some(state);
        }
    }

    /// Clear cached state so the next cycle retries the write.
    pub fn invalidate_od(&mut self, index: usize) {
        if let Some(slot) = self.od_outputs.get_mut(index) {
            *slot = None;
        }
    }

    // ── 0-10 V Analog Outputs ────────────────────────────────────────

    /// Returns `true` if the 0-10V output at `index` needs a write.
    pub fn should_update_v_out(&self, index: usize, new_val: u16) -> bool {
        self.v_outputs
            .get(index)
            .map_or(false, |cached| *cached != Some(new_val))
    }

    /// Mark 0-10V output at `index` as successfully written.
    pub fn confirm_v_out(&mut self, index: usize, val: u16) {
        if let Some(slot) = self.v_outputs.get_mut(index) {
            *slot = Some(val);
        }
    }

    /// Clear cached state so the next cycle retries the write.
    pub fn invalidate_v_out(&mut self, index: usize) {
        if let Some(slot) = self.v_outputs.get_mut(index) {
            *slot = None;
        }
    }

    // ── 4-20 mA Analog Outputs ───────────────────────────────────────

    /// Returns `true` if the 4-20mA output at `index` needs a write.
    pub fn should_update_ma_out(&self, index: usize, new_val: u16) -> bool {
        self.ma_outputs
            .get(index)
            .map_or(false, |cached| *cached != Some(new_val))
    }

    /// Mark 4-20mA output at `index` as successfully written.
    pub fn confirm_ma_out(&mut self, index: usize, val: u16) {
        if let Some(slot) = self.ma_outputs.get_mut(index) {
            *slot = Some(val);
        }
    }

    /// Clear cached state so the next cycle retries the write.
    pub fn invalidate_ma_out(&mut self, index: usize) {
        if let Some(slot) = self.ma_outputs.get_mut(index) {
            *slot = None;
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
    fn new_cache_always_needs_update() {
        let cache = OutputCache::new();
        assert!(cache.should_update_relay(0, true));
        assert!(cache.should_update_relay(0, false));
        assert!(cache.should_update_od(0, true));
    }

    #[test]
    fn confirmed_state_skips_update() {
        let mut cache = OutputCache::new();
        cache.confirm_relay(0, true);
        assert!(!cache.should_update_relay(0, true)); // same state → skip
        assert!(cache.should_update_relay(0, false)); // different → update
    }

    #[test]
    fn invalidated_state_forces_retry() {
        let mut cache = OutputCache::new();
        cache.confirm_relay(3, true);
        cache.invalidate_relay(3);
        assert!(cache.should_update_relay(3, true)); // unknown → retry
    }

    #[test]
    fn out_of_range_returns_false() {
        let cache = OutputCache::new();
        assert!(!cache.should_update_relay(99, true));
        assert!(!cache.should_update_od(99, true));
    }

    #[test]
    fn od_cache_works_independently() {
        let mut cache = OutputCache::new();
        cache.confirm_od(2, false);
        assert!(!cache.should_update_od(2, false));
        assert!(cache.should_update_od(2, true));
        cache.invalidate_od(2);
        assert!(cache.should_update_od(2, false));
    }

    #[test]
    fn v_out_cache_write_on_change() {
        let mut cache = OutputCache::new();
        // New → needs write
        assert!(cache.should_update_v_out(0, 500));
        cache.confirm_v_out(0, 500);
        // Same value → skip
        assert!(!cache.should_update_v_out(0, 500));
        // Different value → write
        assert!(cache.should_update_v_out(0, 750));
        cache.confirm_v_out(0, 750);
        assert!(!cache.should_update_v_out(0, 750));
    }

    #[test]
    fn ma_out_cache_write_on_change() {
        let mut cache = OutputCache::new();
        assert!(cache.should_update_ma_out(1, 1200));
        cache.confirm_ma_out(1, 1200);
        assert!(!cache.should_update_ma_out(1, 1200));
        assert!(cache.should_update_ma_out(1, 1600));
    }

    #[test]
    fn analog_out_invalidate_forces_retry() {
        let mut cache = OutputCache::new();
        cache.confirm_v_out(2, 300);
        cache.invalidate_v_out(2);
        assert!(cache.should_update_v_out(2, 300));
    }

    #[test]
    fn analog_out_of_range_returns_false() {
        let cache = OutputCache::new();
        assert!(!cache.should_update_v_out(99, 100));
        assert!(!cache.should_update_ma_out(99, 100));
    }

    #[test]
    fn relay_bitmask_builds_from_confirmed() {
        let mut cache = OutputCache::new();
        cache.confirm_relay(0, true);
        cache.confirm_relay(2, true);
        cache.confirm_relay(5, false);
        // Bit 0 ON, bit 2 ON, bit 5 OFF → 0b0000_0101 = 0x05
        assert_eq!(cache.relay_bitmask(16), 0x0005);
    }

    #[test]
    fn relay_bitmask_respects_count() {
        let mut cache = OutputCache::new();
        cache.confirm_relay(0, true);
        cache.confirm_relay(8, true); // beyond count=8
        assert_eq!(cache.relay_bitmask(8), 0x0001);
    }

    #[test]
    fn has_confirmed_relay_tracks_state() {
        let mut cache = OutputCache::new();
        assert!(!cache.has_confirmed_relay(3));
        cache.confirm_relay(3, true);
        assert!(cache.has_confirmed_relay(3));
        cache.invalidate_relay(3);
        assert!(!cache.has_confirmed_relay(3));
    }

    #[test]
    fn mismatch_invalidates_affected_relays() {
        let mut cache = OutputCache::new();
        // Confirm relays 0-3
        for i in 0..4 {
            cache.confirm_relay(i, i % 2 == 0); // 0=ON, 1=OFF, 2=ON, 3=OFF
        }
        assert_eq!(cache.relay_bitmask(4), 0b0101); // bits 0,2 ON

        // Simulate mismatch on relay 0 (expected ON but hardware says OFF)
        let actual: u16 = 0b0100; // only relay 2 is ON
        let expected = cache.relay_bitmask(4);
        let diff = actual ^ expected;
        for i in 0..4 {
            if diff & (1 << i) != 0 && cache.has_confirmed_relay(i) {
                cache.invalidate_relay(i);
            }
        }
        // Relay 0 should be invalidated (was ON, hardware says OFF)
        assert!(!cache.has_confirmed_relay(0));
        // Relay 2 should still be confirmed (matches)
        assert!(cache.has_confirmed_relay(2));
        // Relay 1 should still be confirmed (both say OFF)
        assert!(cache.has_confirmed_relay(1));
    }
}
