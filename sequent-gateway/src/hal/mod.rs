pub mod traits;

// Generic I²C driver — Linux only (i2cdev crate requires /dev/i2c-*).
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod driver;

// ── Platform stub (Windows / macOS dev machines) ─────────────────────
// The constructor always bails, so the poll loop degrades gracefully
// (boards = None → Modbus server runs with zeroed data bank).
// Cross-compile for the Pi with:
//   cross build --release --target aarch64-unknown-linux-gnu

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub mod driver {
    use anyhow::Result;
    use crate::board_def::BoardDef;
    use super::traits::{BoardCapability, SequentBoard};

    pub struct GenericBoard {
        name: String,
        _stack_id: u8,
    }

    #[allow(dead_code)]
    impl GenericBoard {
        pub fn new(_bus: &str, _stack_id: u8, _def: &BoardDef) -> Result<Self> {
            anyhow::bail!(
                "I²C is only available on Linux. \
                 Cross-compile with: cross build --release --target aarch64-unknown-linux-gnu"
            )
        }
    }

    impl SequentBoard for GenericBoard {
        fn name(&self) -> &str { &self.name }
        fn stack_id(&self) -> u8 { self._stack_id }
        fn capabilities(&self) -> &[BoardCapability] { &[] }
    }
}
