//! Board registry for dynamic board dispatch.
//!
//! Holds a `Vec<Box<dyn SequentBoard>>` built from `--board` CLI flags and
//! TOML definitions.  The poll loop iterates registered boards calling
//! `poll_inputs()` and `apply_outputs()` instead of hard-coding board types.

use tracing::info;

use crate::hal::traits::{BoardCapability, SequentBoard};

/// Registry of all active boards in the gateway.
///
/// Constructed at startup from CLI flags and board definitions.
/// Passed to the poll loop for generic I/O dispatch.
pub struct BoardRegistry {
    boards: Vec<Box<dyn SequentBoard>>,
}

impl BoardRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { boards: Vec::new() }
    }

    /// Register a board.  Returns the index (for diagnostics).
    pub fn register(&mut self, board: Box<dyn SequentBoard>) -> usize {
        let idx = self.boards.len();
        self.boards.push(board);
        idx
    }

    /// Number of registered boards.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.boards.len()
    }

    /// Whether the registry is empty.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.boards.is_empty()
    }

    /// Mutable iterator over all boards (for poll loop dispatch).
    pub fn boards_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn SequentBoard>> {
        self.boards.iter_mut()
    }

    /// Immutable iterator over all boards (for logging / diagnostics).
    #[allow(dead_code)]
    pub fn boards(&self) -> impl Iterator<Item = &Box<dyn SequentBoard>> {
        self.boards.iter()
    }

    /// Total relay count across all boards that have relay capability.
    pub fn total_relay_count(&self) -> usize {
        self.boards
            .iter()
            .filter(|b| b.has_capability(BoardCapability::Relays))
            .map(|b| b.relay_count())
            .sum()
    }

    /// Whether any registered board has a given capability.
    pub fn has_capability(&self, cap: BoardCapability) -> bool {
        self.boards.iter().any(|b| b.has_capability(cap))
    }

    /// Log a summary of all registered boards at INFO level.
    pub fn log_startup_summary(&self) {
        info!("Registered boards: {}", self.boards.len());
        for (i, board) in self.boards.iter().enumerate() {
            info!(
                "  [{}] {} — stack {} — {:?} — {} relays",
                i,
                board.name(),
                board.stack_id(),
                board.capabilities(),
                board.relay_count(),
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::databank::DataBank;
    use anyhow::Result;

    /// Mock input board for testing.
    struct MockInputBoard {
        stack: u8,
        value: u16,
    }

    impl SequentBoard for MockInputBoard {
        fn name(&self) -> &str { "MockInput" }
        fn stack_id(&self) -> u8 { self.stack }
        fn capabilities(&self) -> &[BoardCapability] {
            &[BoardCapability::AnalogInputs, BoardCapability::DiscreteInputs]
        }
        fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()> {
            db.holding_registers[0] = self.value;
            Ok(())
        }
    }

    /// Mock output board for testing.
    struct MockRelayBoard {
        stack: u8,
        relays: usize,
        applied: bool,
    }

    impl SequentBoard for MockRelayBoard {
        fn name(&self) -> &str { "MockRelay" }
        fn stack_id(&self) -> u8 { self.stack }
        fn capabilities(&self) -> &[BoardCapability] {
            &[BoardCapability::Relays]
        }
        fn relay_count(&self) -> usize { self.relays }
        fn apply_outputs(&mut self, _db: &DataBank) -> Result<()> {
            self.applied = true;
            Ok(())
        }
    }

    #[test]
    fn empty_registry() {
        let reg = BoardRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert_eq!(reg.total_relay_count(), 0);
    }

    #[test]
    fn register_and_count() {
        let mut reg = BoardRegistry::new();
        let idx0 = reg.register(Box::new(MockInputBoard { stack: 1, value: 0 }));
        let idx1 = reg.register(Box::new(MockRelayBoard { stack: 0, relays: 16, applied: false }));
        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());
    }

    #[test]
    fn total_relay_count_sums_relay_boards() {
        let mut reg = BoardRegistry::new();
        reg.register(Box::new(MockInputBoard { stack: 1, value: 0 }));
        reg.register(Box::new(MockRelayBoard { stack: 0, relays: 16, applied: false }));
        assert_eq!(reg.total_relay_count(), 16);
    }

    #[test]
    fn has_capability_queries() {
        let mut reg = BoardRegistry::new();
        reg.register(Box::new(MockInputBoard { stack: 1, value: 0 }));
        assert!(reg.has_capability(BoardCapability::AnalogInputs));
        assert!(!reg.has_capability(BoardCapability::Relays));

        reg.register(Box::new(MockRelayBoard { stack: 0, relays: 8, applied: false }));
        assert!(reg.has_capability(BoardCapability::Relays));
    }

    #[test]
    fn poll_inputs_dispatches_through_registry() {
        let mut reg = BoardRegistry::new();
        reg.register(Box::new(MockInputBoard { stack: 1, value: 4200 }));
        reg.register(Box::new(MockRelayBoard { stack: 0, relays: 16, applied: false }));

        let mut db = DataBank::new();
        for board in reg.boards_mut() {
            board.poll_inputs(&mut db).unwrap();
        }
        assert_eq!(db.holding_registers[0], 4200);
    }

    #[test]
    fn apply_outputs_dispatches_through_registry() {
        let mut reg = BoardRegistry::new();
        reg.register(Box::new(MockInputBoard { stack: 1, value: 0 }));
        reg.register(Box::new(MockRelayBoard { stack: 0, relays: 16, applied: false }));

        let db = DataBank::new();
        for board in reg.boards_mut() {
            board.apply_outputs(&db).unwrap();
        }
        // Relay board's apply_outputs was called (no panic = success)
    }

    #[test]
    fn boards_iterator_yields_metadata() {
        let mut reg = BoardRegistry::new();
        reg.register(Box::new(MockInputBoard { stack: 3, value: 0 }));
        reg.register(Box::new(MockRelayBoard { stack: 5, relays: 8, applied: false }));

        let names: Vec<&str> = reg.boards().map(|b| b.name()).collect();
        assert_eq!(names, vec!["MockInput", "MockRelay"]);

        let stacks: Vec<u8> = reg.boards().map(|b| b.stack_id()).collect();
        assert_eq!(stacks, vec![3, 5]);
    }
}
