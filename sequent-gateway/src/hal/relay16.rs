//! I²C HAL for the Sequent Microsystems 16-Relay HAT.
//!
//! Uses the same set/clr register convention as the MegaInd board.
//!
//! **NOTE:** The base address (`0x20`) should be verified against your
//! specific hardware revision. Adjust `RELAY16_BASE_ADDR` in
//! `registers.rs` if needed.

use anyhow::{Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use tracing::debug;

use crate::registers::*;

/// HAL wrapper for a single 16-Relay HAT.
pub struct RelayBoard {
    dev: LinuxI2CDevice,
    stack_id: u8,
}

impl RelayBoard {
    /// Open the I²C bus for a 16-Relay board at the given stack level (0–7).
    ///
    /// The I²C slave address is `0x20 + stack_id`.
    pub fn new(bus: &str, stack_id: u8) -> Result<Self> {
        let addr = RELAY16_BASE_ADDR + stack_id as u16;
        let dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!("Opened 16-Relay HAT at {bus} 0x{addr:02X} (stack {stack_id})");
        Ok(Self { dev, stack_id })
    }

    /// Write a register address followed by data in a single transaction.
    fn i2c_write(&mut self, register: u8, data: &[u8]) -> Result<()> {
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(register);
        buf.extend_from_slice(data);
        self.dev
            .write(&buf)
            .with_context(|| format!("I²C write {} bytes to 0x{register:02X}", data.len()))?;
        Ok(())
    }

    /// Set a relay on or off.
    ///
    /// `channel` is 1-based (1–16).
    pub fn set_relay(&mut self, channel: u8, state: bool) -> Result<()> {
        anyhow::ensure!(
            (1..=RELAY16_CHANNELS as u8).contains(&channel),
            "Relay channel must be 1–{RELAY16_CHANNELS}, got {channel}"
        );
        let register = if state {
            RELAY16_MEM_RELAY_SET
        } else {
            RELAY16_MEM_RELAY_CLR
        };
        self.i2c_write(register, &[channel])?;
        debug!(
            "Relay board stack {} relay {} → {}",
            self.stack_id,
            channel,
            if state { "ON" } else { "OFF" }
        );
        Ok(())
    }

    /// Return the stack ID (for logging).
    pub fn stack_id(&self) -> u8 {
        self.stack_id
    }
}
