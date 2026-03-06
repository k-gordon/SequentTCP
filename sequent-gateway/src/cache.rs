//! Output state caching.
//!
//! Tracks the last-written state of relays and open-drain outputs.
//! An I²C write is only issued when the requested state differs from
//! the cached value — preventing I²C bus saturation when the vPLC
//! holds outputs steady.
//!
//! On startup, all slots are `None` (unknown), forcing every output
//! to be written on the first cycle so hardware is synchronised.

use crate::registers::{OD_CHANNELS, RELAY16_CHANNELS};

/// Cached output state for relays and OD outputs.
#[derive(Debug)]
pub struct OutputCache {
    relays: [Option<bool>; RELAY16_CHANNELS],
    od_outputs: [Option<bool>; OD_CHANNELS],
}

impl OutputCache {
    /// Create a new cache with all states unknown (`None`).
    pub fn new() -> Self {
        Self {
            relays: [None; RELAY16_CHANNELS],
            od_outputs: [None; OD_CHANNELS],
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
}
