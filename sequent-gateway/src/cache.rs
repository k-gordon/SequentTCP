//! Generic output state caching.
//!
//! Tracks the last-written state of output channels, organised by
//! I/O group index.  An I²C write is only issued when the requested
//! state differs from the cached value — preventing bus saturation
//! when outputs are held steady.
//!
//! On startup, all slots are `None` (unknown), forcing every output
//! to be written on the first cycle so hardware is synchronised.
//!
//! Boolean outputs (coils/relays) are stored as `0` (off) / `1` (on).
//! Analog outputs (holding registers) are stored as the raw Modbus value.

/// Cached output state, organised by I/O group.
#[derive(Debug)]
pub struct OutputCache {
    groups: Vec<Vec<Option<u16>>>,
}

#[allow(dead_code)] // methods called from Linux-only HAL code and tests
impl OutputCache {
    /// Create a cache with one slot-vector per output I/O group.
    pub fn from_groups(group_sizes: &[usize]) -> Self {
        Self {
            groups: group_sizes.iter().map(|&n| vec![None; n]).collect(),
        }
    }

    /// Returns `true` if the value at `(group, channel)` needs writing.
    pub fn should_update(&self, group: usize, channel: usize, new_val: u16) -> bool {
        self.groups
            .get(group)
            .and_then(|g| g.get(channel))
            .map_or(false, |cached| *cached != Some(new_val))
    }

    /// Mark a channel as successfully written.
    pub fn confirm(&mut self, group: usize, channel: usize, val: u16) {
        if let Some(g) = self.groups.get_mut(group) {
            if let Some(slot) = g.get_mut(channel) {
                *slot = Some(val);
            }
        }
    }

    /// Clear cached state so the next cycle retries the write.
    pub fn invalidate(&mut self, group: usize, channel: usize) {
        if let Some(g) = self.groups.get_mut(group) {
            if let Some(slot) = g.get_mut(channel) {
                *slot = None;
            }
        }
    }

    /// Build a bitmask from confirmed `1` values in a group.
    ///
    /// Only the first `count` channels are included.
    /// `Some(1)` → bit set, anything else → bit clear.
    pub fn bitmask(&self, group: usize, count: usize) -> u16 {
        let g = match self.groups.get(group) {
            Some(g) => g,
            None => return 0,
        };
        let mut mask: u16 = 0;
        for i in 0..count.min(g.len()) {
            if g[i] == Some(1) {
                mask |= 1 << i;
            }
        }
        mask
    }

    /// Returns `true` if the channel has a confirmed (known) state.
    pub fn has_confirmed(&self, group: usize, channel: usize) -> bool {
        self.groups
            .get(group)
            .and_then(|g| g.get(channel))
            .map_or(false, |s| s.is_some())
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
        let cache = OutputCache::from_groups(&[4, 8]);
        assert!(cache.should_update(0, 0, 1));
        assert!(cache.should_update(0, 0, 0));
        assert!(cache.should_update(1, 7, 1));
    }

    #[test]
    fn confirmed_state_skips_update() {
        let mut cache = OutputCache::from_groups(&[4]);
        cache.confirm(0, 0, 1);
        assert!(!cache.should_update(0, 0, 1)); // same → skip
        assert!(cache.should_update(0, 0, 0)); // different → update
    }

    #[test]
    fn invalidated_state_forces_retry() {
        let mut cache = OutputCache::from_groups(&[4]);
        cache.confirm(0, 3, 1);
        cache.invalidate(0, 3);
        assert!(cache.should_update(0, 3, 1)); // unknown → retry
    }

    #[test]
    fn out_of_range_returns_false() {
        let cache = OutputCache::from_groups(&[4]);
        assert!(!cache.should_update(5, 0, 1)); // bad group
        assert!(!cache.should_update(0, 99, 1)); // bad channel
    }

    #[test]
    fn analog_write_on_change() {
        let mut cache = OutputCache::from_groups(&[4]);
        assert!(cache.should_update(0, 0, 500));
        cache.confirm(0, 0, 500);
        assert!(!cache.should_update(0, 0, 500));
        assert!(cache.should_update(0, 0, 750));
    }

    #[test]
    fn bitmask_from_confirmed() {
        let mut cache = OutputCache::from_groups(&[16]);
        cache.confirm(0, 0, 1); // relay 1 ON
        cache.confirm(0, 2, 1); // relay 3 ON
        cache.confirm(0, 5, 0); // relay 6 OFF
        assert_eq!(cache.bitmask(0, 16), 0x0005);
    }

    #[test]
    fn bitmask_respects_count() {
        let mut cache = OutputCache::from_groups(&[16]);
        cache.confirm(0, 0, 1);
        cache.confirm(0, 8, 1); // beyond count=8
        assert_eq!(cache.bitmask(0, 8), 0x0001);
    }

    #[test]
    fn has_confirmed_tracks_state() {
        let mut cache = OutputCache::from_groups(&[4]);
        assert!(!cache.has_confirmed(0, 3));
        cache.confirm(0, 3, 1);
        assert!(cache.has_confirmed(0, 3));
        cache.invalidate(0, 3);
        assert!(!cache.has_confirmed(0, 3));
    }

    #[test]
    fn multi_group_independence() {
        let mut cache = OutputCache::from_groups(&[4, 8]);
        cache.confirm(0, 0, 1);
        cache.confirm(1, 0, 500);
        assert!(!cache.should_update(0, 0, 1));
        assert!(!cache.should_update(1, 0, 500));
        assert!(cache.should_update(0, 0, 0));
        assert!(cache.should_update(1, 0, 501));
    }

    #[test]
    fn mismatch_invalidation() {
        let mut cache = OutputCache::from_groups(&[4]);
        for i in 0..4 {
            cache.confirm(0, i, if i % 2 == 0 { 1 } else { 0 });
        }
        assert_eq!(cache.bitmask(0, 4), 0b0101);

        let actual: u16 = 0b0100; // only relay 2 is ON
        let expected = cache.bitmask(0, 4);
        let diff = actual ^ expected;
        for i in 0..4 {
            if diff & (1 << i) != 0 && cache.has_confirmed(0, i) {
                cache.invalidate(0, i);
            }
        }
        assert!(!cache.has_confirmed(0, 0)); // was ON, actual OFF → invalidated
        assert!(cache.has_confirmed(0, 1)); // both OFF → ok
        assert!(cache.has_confirmed(0, 2)); // both ON → ok
    }
}
