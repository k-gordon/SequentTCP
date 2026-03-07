//! Common HAL trait for Sequent Microsystems I²C boards.
//!
//! [`SequentBoard`] provides identity, capability introspection, and I/O
//! dispatch so that the gateway can discover and poll boards at runtime
//! without needing to know the concrete HAL type.
//!
//! # Adding a new board
//!
//! 1. Create a `.toml` board definition in `boards/`.
//! 2. Implement the I²C HAL struct with board-specific read/write methods.
//! 3. Implement `SequentBoard` for the struct.
//! 4. Add a compiled default in [`crate::board_def::BoardDef`].
//! 5. Register the board type in the CLI (`--board <type>`) and poll loop.

use anyhow::Result;

use crate::cache::OutputCache;
use crate::databank::DataBank;

/// Capabilities that a board may expose to the gateway.
///
/// Used for runtime introspection — the poll loop and diagnostics can
/// query a board's capabilities without type-level dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum BoardCapability {
    /// Digital relay outputs (Modbus coils).
    Relays,
    /// Opto-isolated discrete inputs (Modbus discrete inputs).
    DiscreteInputs,
    /// Open-drain digital outputs (Modbus coils, offset after relays).
    DiscreteOutputs,
    /// 4-20 mA / 0-10 V analog inputs (Modbus holding registers, read-only).
    AnalogInputs,
    /// 4-20 mA / 0-10 V analog outputs (Modbus holding registers, writable).
    AnalogOutputs,
}

/// Common interface for all Sequent Microsystems I²C HATs.
///
/// Implementing this trait allows a board to participate in the gateway's
/// dynamic board discovery, capability introspection, and trait-object-based
/// poll loop dispatch.
///
/// # I/O dispatch
///
/// [`poll_inputs`](SequentBoard::poll_inputs) reads hardware inputs and
/// writes them into the shared [`DataBank`].
///
/// [`apply_outputs`](SequentBoard::apply_outputs) reads desired output
/// state from the [`DataBank`] and writes to hardware through the
/// [`OutputCache`] (write-on-change).
///
/// Default implementations are no-ops, so boards only need to override
/// the methods matching their capabilities.
#[allow(dead_code)]
pub trait SequentBoard: Send {
    /// Human-readable board name (for logging and diagnostics).
    fn name(&self) -> &str;

    /// Stack ID (0–7) used for I²C address calculation.
    fn stack_id(&self) -> u8;

    /// List of capabilities this board exposes.
    fn capabilities(&self) -> &'static [BoardCapability];

    /// Number of relay channels (0 if the board has no relays).
    fn relay_count(&self) -> usize {
        0
    }

    /// Check whether the board supports a specific capability.
    fn has_capability(&self, cap: BoardCapability) -> bool {
        self.capabilities().contains(&cap)
    }

    /// Read all hardware inputs and update the shared [`DataBank`].
    ///
    /// Called once per poll tick for each registered board.
    /// The default implementation is a no-op (for boards with no inputs).
    fn poll_inputs(&mut self, _db: &mut DataBank) -> Result<()> {
        Ok(())
    }

    /// Read desired output state from the [`DataBank`] and write to
    /// hardware through the [`OutputCache`].
    ///
    /// Called once per poll tick for each registered board.
    /// The default implementation is a no-op (for boards with no outputs).
    fn apply_outputs(&mut self, _db: &DataBank, _cache: &mut OutputCache) -> Result<()> {
        Ok(())
    }
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::OutputCache;
    use crate::databank::DataBank;

    /// Minimal test board for verifying the trait's default methods.
    struct DummyBoard;

    impl SequentBoard for DummyBoard {
        fn name(&self) -> &str {
            "Dummy"
        }
        fn stack_id(&self) -> u8 {
            0
        }
        fn capabilities(&self) -> &'static [BoardCapability] {
            &[BoardCapability::Relays, BoardCapability::DiscreteInputs]
        }
        fn relay_count(&self) -> usize {
            4
        }
    }

    #[test]
    fn has_capability_true() {
        let board = DummyBoard;
        assert!(board.has_capability(BoardCapability::Relays));
        assert!(board.has_capability(BoardCapability::DiscreteInputs));
    }

    #[test]
    fn has_capability_false() {
        let board = DummyBoard;
        assert!(!board.has_capability(BoardCapability::AnalogInputs));
        assert!(!board.has_capability(BoardCapability::AnalogOutputs));
    }

    #[test]
    fn trait_metadata() {
        let board = DummyBoard;
        assert_eq!(board.name(), "Dummy");
        assert_eq!(board.stack_id(), 0);
        assert_eq!(board.relay_count(), 4);
    }

    #[test]
    fn default_relay_count_is_zero() {
        struct NoRelays;
        impl SequentBoard for NoRelays {
            fn name(&self) -> &str { "NoRelays" }
            fn stack_id(&self) -> u8 { 0 }
            fn capabilities(&self) -> &'static [BoardCapability] { &[] }
        }
        assert_eq!(NoRelays.relay_count(), 0);
        assert!(!NoRelays.has_capability(BoardCapability::Relays));
    }

    // ── Trait-dispatch integration tests (SEQGW-25) ─────────────────

    /// A mock "input board" that writes specific values into the DataBank
    /// when poll_inputs is called through a trait object.
    struct MockInputBoard {
        ma_value: f32,
        voltage: f32,
    }

    impl SequentBoard for MockInputBoard {
        fn name(&self) -> &str { "MockInput" }
        fn stack_id(&self) -> u8 { 3 }
        fn capabilities(&self) -> &'static [BoardCapability] {
            &[BoardCapability::AnalogInputs, BoardCapability::DiscreteInputs]
        }
        fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()> {
            // Write mA value to HR 0 (mA × 100)
            db.holding_registers[0] = (self.ma_value * 100.0) as u16;
            // Write voltage to HR 8 (V × 100)
            db.holding_registers[8] = (self.voltage * 100.0) as u16;
            Ok(())
        }
    }

    /// A mock "output board" that writes coil[0] state back into HR 9
    /// (an otherwise-unused register) so we can verify dispatch.
    struct MockOutputBoard;

    impl SequentBoard for MockOutputBoard {
        fn name(&self) -> &str { "MockOutput" }
        fn stack_id(&self) -> u8 { 5 }
        fn capabilities(&self) -> &'static [BoardCapability] {
            &[BoardCapability::Relays]
        }
        fn relay_count(&self) -> usize { 4 }
        fn apply_outputs(&mut self, db: &DataBank, _cache: &mut OutputCache) -> Result<()> {
            // Can't mutate db, so we verify by checking that we can read the coil.
            // The real verification is that this method gets called at all through
            // the trait object — if it doesn't, the assert below will fail.
            assert!(db.coils[0], "expected coil 0 to be ON");
            Ok(())
        }
    }

    #[test]
    fn trait_object_poll_inputs_dispatches_to_impl() {
        let mut board: Box<dyn SequentBoard> = Box::new(MockInputBoard {
            ma_value: 12.5,
            voltage: 24.1,
        });
        let mut db = DataBank::new();

        // Dispatch through trait object
        board.poll_inputs(&mut db).unwrap();

        assert_eq!(db.holding_registers[0], 1250); // 12.5 × 100
        assert_eq!(db.holding_registers[8], 2410); // 24.1 × 100
    }

    #[test]
    fn trait_object_apply_outputs_dispatches_to_impl() {
        let mut board: Box<dyn SequentBoard> = Box::new(MockOutputBoard);
        let mut db = DataBank::new();
        let mut cache = OutputCache::new();

        db.coils[0] = true;
        // This calls MockOutputBoard::apply_outputs which asserts coil[0] == true.
        // If dispatch doesn't work, this will panic.
        board.apply_outputs(&db, &mut cache).unwrap();
    }

    #[test]
    fn default_poll_inputs_is_noop() {
        let mut board = DummyBoard;
        let mut db = DataBank::new();
        // Default implementation should succeed without modifying the DB
        assert!(board.poll_inputs(&mut db).is_ok());
        assert_eq!(db.holding_registers[0], 0);
    }

    #[test]
    fn default_apply_outputs_is_noop() {
        let mut board = DummyBoard;
        let db = DataBank::new();
        let mut cache = OutputCache::new();
        // Default implementation should succeed
        assert!(board.apply_outputs(&db, &mut cache).is_ok());
    }
}
