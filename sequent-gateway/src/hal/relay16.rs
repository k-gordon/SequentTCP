//! I²C HAL for PCA9535-based relay boards (e.g. Sequent 16-Relay HAT).
//!
//! All hardware-specific parameters (address, registers, channel remap)
//! come from a [`BoardDef`] loaded from TOML at runtime.
//!
//! Protocol: read-modify-write on the PCA9535 Output Port register.
//! Channel numbering is remapped according to the board definition.

use anyhow::{Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use tracing::{debug, info};

use crate::board_def::BoardDef;

/// HAL wrapper for a PCA9535-based relay board.
pub struct RelayBoard {
    dev: LinuxI2CDevice,
    stack_id: u8,
    outport_reg: u8,
    ch_remap: Vec<u8>,
}

impl RelayBoard {
    /// Open the I²C bus for a relay board described by `def`.
    ///
    /// The address and register layout are read from the board definition.
    pub fn new(bus: &str, stack_id: u8, def: &BoardDef) -> Result<Self> {
        let pca = def
            .pca9535
            .as_ref()
            .context("Board definition missing [pca9535] section")?;
        let ch_remap = def
            .channels
            .relay_remap
            .clone()
            .unwrap_or_else(|| vec![15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]);

        let addr = def.address.resolve(stack_id);
        let mut dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!(
            "Opened {} at {bus} 0x{addr:02X} (stack {stack_id})",
            def.board.name
        );

        // ── Initialise PCA9535: configure all pins as outputs ────────
        let mut cfg_buf = [0u8; 2];
        dev.write(&[pca.config_reg])
            .with_context(|| "Failed to set CFG register address")?;
        dev.read(&mut cfg_buf)
            .with_context(|| "Failed to read CFG register")?;
        let cfg_val = u16::from_le_bytes(cfg_buf);

        if cfg_val != 0 {
            info!("{}: configuring I/O expander pins as outputs", def.board.name);
            dev.write(&[pca.outport_reg, 0, 0])
                .with_context(|| "Failed to clear OUTPORT register")?;
            dev.write(&[pca.config_reg, 0, 0])
                .with_context(|| "Failed to write CFG register")?;
        }

        Ok(Self {
            dev,
            stack_id,
            outport_reg: pca.outport_reg,
            ch_remap,
        })
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Read the 16-bit Output Port register (raw I/O-expander bit order).
    fn read_output_reg(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.dev
            .write(&[self.outport_reg])
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
            .write(&[self.outport_reg, bytes[0], bytes[1]])
            .with_context(|| "Failed to write OUTPORT register")?;
        Ok(())
    }

    // ── Public API ───────────────────────────────────────────────────

    /// Set a relay on or off (read-modify-write with channel remapping).
    ///
    /// `channel` is 1-based (1–N where N = relay count from definition).
    pub fn set_relay(&mut self, channel: u8, state: bool) -> Result<()> {
        let num_relays = self.ch_remap.len();
        anyhow::ensure!(
            (1..=num_relays as u8).contains(&channel),
            "Relay channel must be 1–{num_relays}, got {channel}"
        );

        // Read current output state
        let mut io_val = self.read_output_reg()?;

        // Apply change using the channel-to-bit remap
        let bit = self.ch_remap[(channel - 1) as usize];
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

    /// Read the current relay output state as a bitmask.
    ///
    /// Returns a `u16` where bit 0 = relay 1, bit 1 = relay 2, … bit 15 = relay 16.
    /// The raw I/O-expander bit order is un-remapped back to logical relay order.
    pub fn read_relay_state(&mut self) -> Result<u16> {
        let raw = self.read_output_reg()?;
        let mut logical: u16 = 0;
        for (relay_idx, &bit_pos) in self.ch_remap.iter().enumerate() {
            if raw & (1 << bit_pos) != 0 {
                logical |= 1 << relay_idx;
            }
        }
        Ok(logical)
    }

    /// Return the stack ID (for logging).
    pub fn stack_id(&self) -> u8 {
        self.stack_id
    }

    /// Return the number of relay channels (from board definition).
    pub fn relay_count(&self) -> usize {
        self.ch_remap.len()
    }
}

impl super::traits::SequentBoard for RelayBoard {
    fn name(&self) -> &str {
        "Relay HAT"
    }
    fn stack_id(&self) -> u8 {
        self.stack_id
    }
    fn capabilities(&self) -> &'static [super::traits::BoardCapability] {
        &[super::traits::BoardCapability::Relays]
    }
    fn relay_count(&self) -> usize {
        self.ch_remap.len()
    }
}
