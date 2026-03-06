// Real I²C implementations — Linux only (i2cdev crate requires /dev/i2c-*).
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod megaind;
#[cfg(any(target_os = "linux", target_os = "android"))]
pub mod relay16;

// ── Platform stubs (Windows / macOS dev machines) ────────────────────
// The constructors always bail, so the poll loop degrades gracefully
// (boards = None → Modbus server runs with zeroed data bank).
// Cross-compile for the Pi with:
//   cross build --release --target aarch64-unknown-linux-gnu

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub mod megaind {
    use anyhow::Result;
    use crate::board_def::BoardDef;
    use crate::registers::{OPTO_CHANNELS, I4_20_IN_CHANNELS, U0_10_IN_CHANNELS};

    pub struct MegaIndBoard {
        _stack_id: u8,
    }

    impl MegaIndBoard {
        pub fn new(_bus: &str, _stack_id: u8, _def: &BoardDef) -> Result<Self> {
            anyhow::bail!(
                "I²C is only available on Linux. \
                 Cross-compile with: cross build --release --target aarch64-unknown-linux-gnu"
            )
        }
        pub fn read_opto_inputs(&mut self) -> Result<(u8, [bool; OPTO_CHANNELS])> { unreachable!() }
        pub fn read_4_20ma_inputs(&mut self) -> Result<[f32; I4_20_IN_CHANNELS]> { unreachable!() }
        pub fn read_0_10v_inputs(&mut self) -> Result<[f32; U0_10_IN_CHANNELS]> { unreachable!() }
        pub fn read_system_voltage(&mut self) -> Result<f32> { unreachable!() }
        pub fn set_od_output(&mut self, _ch: u8, _state: bool) -> Result<()> { unreachable!() }
        pub fn read_firmware_version(&mut self) -> Result<(u8, u8)> { unreachable!() }
        pub fn stack_id(&self) -> u8 { self._stack_id }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub mod relay16 {
    use anyhow::Result;
    use crate::board_def::BoardDef;

    pub struct RelayBoard {
        _stack_id: u8,
    }

    impl RelayBoard {
        pub fn new(_bus: &str, _stack_id: u8, _def: &BoardDef) -> Result<Self> {
            anyhow::bail!(
                "I²C is only available on Linux. \
                 Cross-compile with: cross build --release --target aarch64-unknown-linux-gnu"
            )
        }
        pub fn set_relay(&mut self, _ch: u8, _state: bool) -> Result<()> { unreachable!() }
        pub fn stack_id(&self) -> u8 { self._stack_id }
    }
}
