//! Multi-slave addressing for Modbus TCP.
//!
//! Routes Modbus requests by Unit ID to the correct slice of the shared
//! [`DataBank`].  Two modes are supported:
//!
//! | Mode           | Behaviour                                              |
//! |----------------|--------------------------------------------------------|
//! | **Multi-slave**| Each board has its own Unit ID and its own address 0.   |
//! | **Single-slave**| Flat memory map on one Unit ID (backward-compatible). |
//!
//! # Multi-slave register mapping
//!
//! | Slave | Register Type      | Slave Addr | DataBank Addr |
//! |-------|--------------------|------------|---------------|
//! | Relay | Coils 0–15         | 0–15       | 0–15          |
//! | Ind   | Coils 0–3          | 0–3        | 16–19         |
//! | Ind   | Discrete Inputs 0–7| 0–7        | 0–7           |
//! | Ind   | Holding Regs 0–15  | 0–15       | 0–15          |

use crate::databank::{COIL_COUNT, DISCRETE_INPUT_COUNT, HOLDING_REGISTER_COUNT};

/// Number of relay coils on the 16-Relay HAT.
const RELAY_COIL_COUNT: usize = 16;

/// Number of OD output coils on the Industrial HAT.
const IND_COIL_COUNT: usize = 4;

/// Offset of OD coils in the flat DataBank.
const IND_COIL_OFFSET: usize = 16;

// ════════════════════════════════════════════════════════════════════════
// Types
// ════════════════════════════════════════════════════════════════════════

/// Result of resolving a register request for a given Unit ID.
///
/// `offset` is the starting index in the flat DataBank array.
/// `max_count` is the maximum number of registers available from that offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice {
    pub offset: usize,
    pub max_count: usize,
}

/// Register type being accessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegType {
    Coils,
    DiscreteInputs,
    HoldingRegisters,
}

/// Addressing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlaveMode {
    /// Each board has its own slave ID and register space starting at 0.
    Multi,
    /// Single flat memory map — accepts any unit ID.
    Single,
}

/// Routes Modbus unit IDs to DataBank slices.
#[derive(Debug, Clone)]
pub struct SlaveMap {
    pub mode: SlaveMode,
    pub relay_slave_id: u8,
    pub ind_slave_id: u8,
}

// ════════════════════════════════════════════════════════════════════════
// Implementation
// ════════════════════════════════════════════════════════════════════════

impl SlaveMap {
    /// Create a new slave map.
    pub fn new(relay_slave_id: u8, ind_slave_id: u8, single_slave: bool) -> Self {
        Self {
            mode: if single_slave {
                SlaveMode::Single
            } else {
                SlaveMode::Multi
            },
            relay_slave_id,
            ind_slave_id,
        }
    }

    /// Resolve a register request into a DataBank slice.
    ///
    /// Returns `None` if the unit ID is not recognised (→ Modbus exception
    /// *Gateway Target Device Failed To Respond*, 0x0B).
    pub fn resolve(&self, unit_id: u8, reg_type: RegType) -> Option<Slice> {
        match self.mode {
            SlaveMode::Single => Some(self.single_slice(reg_type)),
            SlaveMode::Multi => self.multi_slice(unit_id, reg_type),
        }
    }

    // ── Single-slave (flat) ──────────────────────────────────────────

    fn single_slice(&self, reg_type: RegType) -> Slice {
        match reg_type {
            RegType::Coils => Slice {
                offset: 0,
                max_count: COIL_COUNT,
            },
            RegType::DiscreteInputs => Slice {
                offset: 0,
                max_count: DISCRETE_INPUT_COUNT,
            },
            RegType::HoldingRegisters => Slice {
                offset: 0,
                max_count: HOLDING_REGISTER_COUNT,
            },
        }
    }

    // ── Multi-slave (per-board) ──────────────────────────────────────

    fn multi_slice(&self, unit_id: u8, reg_type: RegType) -> Option<Slice> {
        if unit_id == self.relay_slave_id {
            self.relay_slice(reg_type)
        } else if unit_id == self.ind_slave_id {
            self.ind_slice(reg_type)
        } else {
            None // unknown slave
        }
    }

    /// Relay board only has coils (relays 1-16).
    fn relay_slice(&self, reg_type: RegType) -> Option<Slice> {
        match reg_type {
            RegType::Coils => Some(Slice {
                offset: 0,
                max_count: RELAY_COIL_COUNT,
            }),
            // Relay board has no discrete inputs or holding registers
            RegType::DiscreteInputs | RegType::HoldingRegisters => Some(Slice {
                offset: 0,
                max_count: 0,
            }),
        }
    }

    /// Industrial board has OD coils (offset 16–19), discrete inputs, and
    /// holding registers.
    fn ind_slice(&self, reg_type: RegType) -> Option<Slice> {
        match reg_type {
            RegType::Coils => Some(Slice {
                offset: IND_COIL_OFFSET,
                max_count: IND_COIL_COUNT,
            }),
            RegType::DiscreteInputs => Some(Slice {
                offset: 0,
                max_count: DISCRETE_INPUT_COUNT,
            }),
            RegType::HoldingRegisters => Some(Slice {
                offset: 0,
                max_count: HOLDING_REGISTER_COUNT,
            }),
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
    fn single_slave_accepts_any_unit_id() {
        let map = SlaveMap::new(1, 2, true);
        // Any unit ID should resolve in single-slave mode
        for uid in [0, 1, 2, 5, 42, 247] {
            assert!(
                map.resolve(uid, RegType::Coils).is_some(),
                "unit {uid} should resolve"
            );
        }
    }

    #[test]
    fn single_slave_returns_full_flat_map() {
        let map = SlaveMap::new(1, 2, true);
        let coils = map.resolve(1, RegType::Coils).unwrap();
        assert_eq!(coils.offset, 0);
        assert_eq!(coils.max_count, COIL_COUNT);

        let di = map.resolve(1, RegType::DiscreteInputs).unwrap();
        assert_eq!(di.offset, 0);
        assert_eq!(di.max_count, DISCRETE_INPUT_COUNT);

        let hr = map.resolve(1, RegType::HoldingRegisters).unwrap();
        assert_eq!(hr.offset, 0);
        assert_eq!(hr.max_count, HOLDING_REGISTER_COUNT);
    }

    #[test]
    fn multi_slave_relay_coils() {
        let map = SlaveMap::new(1, 2, false);
        let coils = map.resolve(1, RegType::Coils).unwrap();
        assert_eq!(coils.offset, 0);
        assert_eq!(coils.max_count, RELAY_COIL_COUNT);
    }

    #[test]
    fn multi_slave_relay_has_no_holding_regs() {
        let map = SlaveMap::new(1, 2, false);
        let hr = map.resolve(1, RegType::HoldingRegisters).unwrap();
        assert_eq!(hr.max_count, 0);
    }

    #[test]
    fn multi_slave_ind_coils_offset() {
        let map = SlaveMap::new(1, 2, false);
        let coils = map.resolve(2, RegType::Coils).unwrap();
        assert_eq!(coils.offset, IND_COIL_OFFSET);
        assert_eq!(coils.max_count, IND_COIL_COUNT);
    }

    #[test]
    fn multi_slave_ind_discrete_inputs() {
        let map = SlaveMap::new(1, 2, false);
        let di = map.resolve(2, RegType::DiscreteInputs).unwrap();
        assert_eq!(di.offset, 0);
        assert_eq!(di.max_count, DISCRETE_INPUT_COUNT);
    }

    #[test]
    fn multi_slave_ind_holding_registers() {
        let map = SlaveMap::new(1, 2, false);
        let hr = map.resolve(2, RegType::HoldingRegisters).unwrap();
        assert_eq!(hr.offset, 0);
        assert_eq!(hr.max_count, HOLDING_REGISTER_COUNT);
    }

    #[test]
    fn multi_slave_unknown_unit_returns_none() {
        let map = SlaveMap::new(1, 2, false);
        assert!(map.resolve(3, RegType::Coils).is_none());
        assert!(map.resolve(0, RegType::Coils).is_none());
        assert!(map.resolve(247, RegType::HoldingRegisters).is_none());
    }

    #[test]
    fn custom_slave_ids() {
        let map = SlaveMap::new(10, 20, false);
        assert!(map.resolve(10, RegType::Coils).is_some());
        assert!(map.resolve(20, RegType::Coils).is_some());
        assert!(map.resolve(1, RegType::Coils).is_none());
        assert!(map.resolve(2, RegType::Coils).is_none());
    }
}
