//! Common HAL trait for Sequent Microsystems I²C boards.
//!
//! [`SequentBoard`] provides identity and capability introspection so that
//! the gateway can discover and report on boards at runtime without needing
//! to know the concrete HAL type.
//!
//! # Adding a new board
//!
//! 1. Create a `.toml` board definition in `boards/`.
//! 2. Implement the I²C HAL struct with board-specific read/write methods.
//! 3. Implement `SequentBoard` for the struct.
//! 4. Add a compiled default in [`crate::board_def::BoardDef`].
//! 5. Register the board type in the CLI (`--board <type>`) and poll loop.

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
/// dynamic board discovery, capability introspection, and future
/// trait-object-based poll loop dispatch.
///
/// Concrete I/O methods (e.g. `read_4_20ma_inputs`, `set_relay`) remain
/// on the implementing struct — the trait provides identity and capability
/// metadata.
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
}

// ════════════════════════════════════════════════════════════════════════
// Tests
// ════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

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
}
