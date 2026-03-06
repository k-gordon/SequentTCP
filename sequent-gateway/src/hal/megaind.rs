//! I²C HAL for the Sequent Microsystems Mega-Industrial (MegaInd) HAT.
//!
//! Communicates directly over `/dev/i2c-*` using the register map from
//! [`megaind.h`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/megaind.h).
//!
//! The I²C protocol follows Sequent's `comm.c`:
//! - **Read:** write 1-byte register address, then read N data bytes.
//! - **Write:** write 1-byte register address + N data bytes in one transaction.

use anyhow::{Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use tracing::{debug, warn};

use crate::registers::*;

/// HAL wrapper for a single MegaInd Industrial HAT.
pub struct MegaIndBoard {
    dev: LinuxI2CDevice,
    stack_id: u8,
}

impl MegaIndBoard {
    /// Open the I²C bus for a MegaInd board at the given stack level (0–7).
    ///
    /// The I²C slave address is `0x50 + stack_id`.
    pub fn new(bus: &str, stack_id: u8) -> Result<Self> {
        let addr = MEGAIND_BASE_ADDR + stack_id as u16;
        let dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!("Opened MegaInd HAT at {bus} 0x{addr:02X} (stack {stack_id})");
        Ok(Self { dev, stack_id })
    }

    // ── Low-level I²C helpers ────────────────────────────────────────

    /// Write a 1-byte register address, then read `buf.len()` bytes back.
    fn i2c_read(&mut self, register: u8, buf: &mut [u8]) -> Result<()> {
        self.dev
            .write(&[register])
            .with_context(|| format!("I²C select register 0x{register:02X}"))?;
        self.dev
            .read(buf)
            .with_context(|| format!("I²C read {} bytes from 0x{register:02X}", buf.len()))?;
        Ok(())
    }

    /// Write a 1-byte register address followed by `data` in a single transaction.
    fn i2c_write(&mut self, register: u8, data: &[u8]) -> Result<()> {
        let mut buf = Vec::with_capacity(1 + data.len());
        buf.push(register);
        buf.extend_from_slice(data);
        self.dev
            .write(&buf)
            .with_context(|| format!("I²C write {} bytes to 0x{register:02X}", data.len()))?;
        Ok(())
    }

    /// Read a 16-bit little-endian value from a register pair.
    fn read_u16_le(&mut self, register: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.i2c_read(register, &mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    // ── Public I/O methods ───────────────────────────────────────────

    /// Read all 8 opto-isolated inputs.
    ///
    /// Returns `(bitmask, [bool; 8])` where bit 0 = channel 1.
    pub fn read_opto_inputs(&mut self) -> Result<(u8, [bool; OPTO_CHANNELS])> {
        let mut buf = [0u8; 1];
        self.i2c_read(I2C_MEM_OPTO_IN_VAL, &mut buf)?;
        let val = buf[0];
        let mut bits = [false; OPTO_CHANNELS];
        for i in 0..OPTO_CHANNELS {
            bits[i] = (val >> i) & 1 == 1;
        }
        Ok((val, bits))
    }

    /// Read all 8 × 4-20 mA input channels.
    ///
    /// Returns milliamps (e.g. 4.0 … 20.0). Individual channel errors
    /// are logged and the channel defaults to 0.0.
    pub fn read_4_20ma_inputs(&mut self) -> Result<[f32; I4_20_IN_CHANNELS]> {
        let mut readings = [0.0f32; I4_20_IN_CHANNELS];
        for ch in 0..I4_20_IN_CHANNELS {
            let reg = I2C_MEM_I4_20_IN_VAL1 + (ch as u8) * 2;
            match self.read_u16_le(reg) {
                Ok(raw) => readings[ch] = raw as f32 / VOLT_TO_MILLIVOLT,
                Err(e) => warn!("4-20mA ch{}: {e:#}", ch + 1),
            }
        }
        Ok(readings)
    }

    /// Read all 4 × 0-10 V input channels.
    ///
    /// Returns volts (e.g. 0.0 … 10.0).
    pub fn read_0_10v_inputs(&mut self) -> Result<[f32; U0_10_IN_CHANNELS]> {
        let mut readings = [0.0f32; U0_10_IN_CHANNELS];
        for ch in 0..U0_10_IN_CHANNELS {
            let reg = I2C_MEM_U0_10_IN_VAL1 + (ch as u8) * 2;
            match self.read_u16_le(reg) {
                Ok(raw) => readings[ch] = raw as f32 / VOLT_TO_MILLIVOLT,
                Err(e) => warn!("0-10V ch{}: {e:#}", ch + 1),
            }
        }
        Ok(readings)
    }

    /// Read the 24 V power supply voltage.
    ///
    /// Returns volts (e.g. 24.12).
    pub fn read_system_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16_le(I2C_MEM_DIAG_24V)?;
        Ok(raw as f32 / VOLT_TO_MILLIVOLT)
    }

    /// Set an open-drain output (1-based channel, 1–4) on or off.
    ///
    /// Uses the shared RELAY_SET / RELAY_CLR register with the OD channel
    /// number, matching the MegaInd firmware convention.
    pub fn set_od_output(&mut self, channel: u8, state: bool) -> Result<()> {
        anyhow::ensure!(
            (1..=OD_CHANNELS as u8).contains(&channel),
            "OD channel must be 1–{OD_CHANNELS}, got {channel}"
        );
        let register = if state {
            I2C_MEM_RELAY_SET
        } else {
            I2C_MEM_RELAY_CLR
        };
        self.i2c_write(register, &[channel])?;
        debug!(
            "MegaInd stack {} OD {} → {}",
            self.stack_id,
            channel,
            if state { "ON" } else { "OFF" }
        );
        Ok(())
    }

    /// Read firmware version (major, minor).
    pub fn read_firmware_version(&mut self) -> Result<(u8, u8)> {
        let mut buf = [0u8; 2];
        self.i2c_read(I2C_MEM_REVISION_MAJOR, &mut buf)?;
        Ok((buf[0], buf[1]))
    }

    /// Return the stack ID (for logging).
    pub fn stack_id(&self) -> u8 {
        self.stack_id
    }
}
