//! Shared Modbus data bank.
//!
//! Central state shared between the Modbus TCP server (reads/writes from
//! clients) and the I²C poll loop (hardware I/O).
//!
//! # Memory Map
//!
//! | Register Type        | Address | Description                                    |
//! |----------------------|---------|------------------------------------------------|
//! | **Coils** (R/W)      | 0–15    | 16-Relay board — Relays 1–16                   |
//! | **Coils** (R/W)      | 16–19   | Industrial board — Open Drain Outputs 1–4      |
//! | **Discrete Inputs**  | 0–7     | Industrial board — Opto-Inputs 1–8             |
//! | **Holding Registers**| 0–7     | 4-20 mA inputs (mA × 100)                     |
//! | **Holding Registers**| 8       | PSU voltage (V × 100)                          |
//! | **Holding Registers**| 9       | _(reserved)_                                   |
//! | **Holding Registers**| 10–13   | 0-10 V inputs (V × 100)                       |
//! | **Holding Registers**| 14      | _(reserved)_                                   |
//! | **Holding Registers**| 15      | Opto-input bitmask (0–255, optional)           |

/// Total number of coils: 16 relays + 4 OD outputs.
pub const COIL_COUNT: usize = 20;

/// Total number of discrete inputs: 8 opto-isolated inputs.
pub const DISCRETE_INPUT_COUNT: usize = 8;

/// Total number of holding registers.
pub const HOLDING_REGISTER_COUNT: usize = 16;

/// Shared Modbus data bank.
///
/// Protected by `Arc<RwLock<DataBank>>` — the poll loop writes inputs and
/// reads coils; the Modbus server reads everything and writes coils.
#[derive(Debug)]
pub struct DataBank {
    pub coils: [bool; COIL_COUNT],
    pub discrete_inputs: [bool; DISCRETE_INPUT_COUNT],
    pub holding_registers: [u16; HOLDING_REGISTER_COUNT],
}

impl DataBank {
    pub fn new() -> Self {
        Self {
            coils: [false; COIL_COUNT],
            discrete_inputs: [false; DISCRETE_INPUT_COUNT],
            holding_registers: [0u16; HOLDING_REGISTER_COUNT],
        }
    }
}

impl Default for DataBank {
    fn default() -> Self {
        Self::new()
    }
}
