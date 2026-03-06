//! I²C HAL for the Sequent Microsystems 16-Relay HAT.
//!
//! The board uses a **PCA9535 I/O expander**, not a Sequent custom MCU.
//! Relay state is controlled via read-modify-write on the Output Port
//! register (`0x02`).  Channel numbering is remapped: relay 1 → bit 15,
//! relay 16 → bit 0.
//!
//! I²C address = `(0x20 + stack_id) ^ 0x07`  (active-low address jumpers).

use anyhow::{Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use tracing::{debug, info};

use crate::registers::*;

/// HAL wrapper for a single 16-Relay HAT.
pub struct RelayBoard {
    dev: LinuxI2CDevice,
    stack_id: u8,
}

impl RelayBoard {
    /// Open the I²C bus for a 16-Relay board at the given stack level (0–7).
    ///
    /// Address = `(0x20 + stack_id) ^ 0x07`  (Sequent active-low convention).
    pub fn new(bus: &str, stack_id: u8) -> Result<Self> {
        let addr = (RELAY16_BASE_ADDR + stack_id as u16) ^ 0x07;
        let mut dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!("Opened 16-Relay HAT at {bus} 0x{addr:02X} (stack {stack_id})");

        // ── Initialise PCA9535: configure all pins as outputs ────────
        let mut cfg_buf = [0u8; 2];
        dev.write(&[RELAY16_CFG_REG])
            .with_context(|| "Failed to set CFG register address")?;
        dev.read(&mut cfg_buf)
            .with_context(|| "Failed to read CFG register")?;
        let cfg_val = u16::from_le_bytes(cfg_buf);

        if cfg_val != 0 {
            info!("16-Relay HAT: configuring I/O expander pins as outputs");
            // All outputs LOW first, then set direction to output
            let zero = [0u8; 2];
            dev.write(&[RELAY16_OUTPORT_REG, zero[0], zero[1]])
                .with_context(|| "Failed to clear OUTPORT register")?;
            dev.write(&[RELAY16_CFG_REG, zero[0], zero[1]])
                .with_context(|| "Failed to write CFG register")?;
        }

        Ok(Self { dev, stack_id })
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Read the 16-bit Output Port register (raw I/O-expander bit order).
    fn read_output_reg(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.dev
            .write(&[RELAY16_OUTPORT_REG])
            .with_context(|| "Failed to set OUTPORT address")?;
        self.dev
            .read(&mut buf)
            .with_context(|| "Failed to read OUTPORT register")?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Write a 16-bit value to the Output Port register.
    fn write_output_reg(&mut self, val: u16) -> Result<()> {
        let bytes = val.to_le_bytes();
        self.dev
            .write(&[RELAY16_OUTPORT_REG, bytes[0], bytes[1]])
            .with_context(|| "Failed to write OUTPORT register")?;
        Ok(())
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Set a relay on or off (read-modify-write with channel remapping).
    ///
    /// `channel` is 1-based (1–16).
    pub fn set_relay(&mut self, channel: u8, state: bool) -> Result<()> {
        anyhow::ensure!(
            (1..=RELAY16_CHANNELS as u8).contains(&channel),
            "Relay channel must be 1–{RELAY16_CHANNELS}, got {channel}"
        );

        // Read current output state
        let mut io_val = self.read_output_reg()?;

        // Apply change using the channel-to-bit remap
        let bit = RELAY_CH_REMAP[(channel - 1) as usize];
        if state {
            io_val |= 1 << bit;
        } else {
            io_val &= !(1 << bit);
        }

        // Write back
        self.write_output_reg(io_val)?;

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
