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

use crate::board_def::BoardDef;
use crate::cache::OutputCache;
use crate::databank::{DataBank, HR_I4_20_OUT_BASE, HR_U0_10_OUT_BASE};
use crate::registers::{
    I4_20_IN_CHANNELS, I4_20_OUT_CHANNELS, OD_CHANNELS, OPTO_CHANNELS,
    U0_10_IN_CHANNELS, U0_10_OUT_CHANNELS,
};

/// Resolved register addresses (extracted from [`BoardDef`] at construction).
struct Regs {
    relay_set: u8,
    relay_clr: u8,
    opto_in: u8,
    u0_10_out: u8,
    i4_20_out: u8,
    i4_20_in: u8,
    u0_10_in: u8,
    diag_24v: u8,
    revision_major: u8,
    voltage_scale: f32,
}

impl Regs {
    fn from_def(def: &BoardDef) -> Self {
        let r = &def.registers;
        Self {
            relay_set: r.relay_set.unwrap_or(0x01),
            relay_clr: r.relay_clr.unwrap_or(0x02),
            opto_in: r.opto_in.unwrap_or(0x03),
            u0_10_out: r.u0_10_out.unwrap_or(0x04),
            i4_20_out: r.i4_20_out.unwrap_or(0x0C),
            i4_20_in: r.i4_20_in.unwrap_or(0x2C),
            u0_10_in: r.u0_10_in.unwrap_or(0x1C),
            diag_24v: r.diag_24v.unwrap_or(0x73),
            revision_major: r.revision_major.unwrap_or(0x78),
            voltage_scale: r.voltage_scale.unwrap_or(1000.0),
        }
    }
}

/// HAL wrapper for a Sequent custom-MCU Industrial HAT.
pub struct MegaIndBoard {
    dev: LinuxI2CDevice,
    stack_id: u8,
    regs: Regs,
}

impl MegaIndBoard {
    /// Open the I²C bus for an Industrial board described by `def`.
    ///
    /// The address is computed from the board definition.
    pub fn new(bus: &str, stack_id: u8, def: &BoardDef) -> Result<Self> {
        let addr = def.address.resolve(stack_id);
        let dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!(
            "Opened {} at {bus} 0x{addr:02X} (stack {stack_id})",
            def.board.name
        );
        Ok(Self {
            dev,
            stack_id,
            regs: Regs::from_def(def),
        })
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
        self.i2c_read(self.regs.opto_in, &mut buf)?;
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
            let reg = self.regs.i4_20_in + (ch as u8) * 2;
            match self.read_u16_le(reg) {
                Ok(raw) => readings[ch] = raw as f32 / self.regs.voltage_scale,
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
            let reg = self.regs.u0_10_in + (ch as u8) * 2;
            match self.read_u16_le(reg) {
                Ok(raw) => readings[ch] = raw as f32 / self.regs.voltage_scale,
                Err(e) => warn!("0-10V ch{}: {e:#}", ch + 1),
            }
        }
        Ok(readings)
    }

    /// Read the 24 V power supply voltage.
    ///
    /// Returns volts (e.g. 24.12).
    pub fn read_system_voltage(&mut self) -> Result<f32> {
        let raw = self.read_u16_le(self.regs.diag_24v)?;
        Ok(raw as f32 / self.regs.voltage_scale)
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
            self.regs.relay_set
        } else {
            self.regs.relay_clr
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

    /// Write a 0-10 V analog output channel.
    ///
    /// `channel` is 1-based (1–4).  `millivolts` is the raw I²C value
    /// (0 = 0 V, 10 000 = 10.000 V).
    pub fn write_0_10v_output(&mut self, channel: u8, millivolts: u16) -> Result<()> {
        anyhow::ensure!(
            (1..=U0_10_OUT_CHANNELS as u8).contains(&channel),
            "0-10V output channel must be 1–{U0_10_OUT_CHANNELS}, got {channel}"
        );
        let reg = self.regs.u0_10_out + (channel - 1) * 2;
        self.i2c_write(reg, &millivolts.to_le_bytes())?;
        debug!(
            "MegaInd stack {} 0-10V ch{} → {} mV",
            self.stack_id, channel, millivolts
        );
        Ok(())
    }

    /// Write a 4-20 mA analog output channel.
    ///
    /// `channel` is 1-based (1–4).  `microamps` is the raw I²C value
    /// (4 000 = 4.000 mA, 20 000 = 20.000 mA).
    pub fn write_4_20ma_output(&mut self, channel: u8, microamps: u16) -> Result<()> {
        anyhow::ensure!(
            (1..=I4_20_OUT_CHANNELS as u8).contains(&channel),
            "4-20mA output channel must be 1–{I4_20_OUT_CHANNELS}, got {channel}"
        );
        let reg = self.regs.i4_20_out + (channel - 1) * 2;
        self.i2c_write(reg, &microamps.to_le_bytes())?;
        debug!(
            "MegaInd stack {} 4-20mA ch{} → {} µA",
            self.stack_id, channel, microamps
        );
        Ok(())
    }

    /// Read firmware version (major, minor).
    pub fn read_firmware_version(&mut self) -> Result<(u8, u8)> {
        let mut buf = [0u8; 2];
        self.i2c_read(self.regs.revision_major, &mut buf)?;
        Ok((buf[0], buf[1]))
    }

    /// Return the stack ID (for logging).
    #[allow(dead_code)]
    pub fn stack_id(&self) -> u8 {
        self.stack_id
    }
}

impl super::traits::SequentBoard for MegaIndBoard {
    fn name(&self) -> &str {
        "MegaInd Industrial HAT"
    }
    fn stack_id(&self) -> u8 {
        self.stack_id
    }
    fn capabilities(&self) -> &'static [super::traits::BoardCapability] {
        use super::traits::BoardCapability::*;
        &[DiscreteInputs, DiscreteOutputs, AnalogInputs, AnalogOutputs]
    }

    /// Read all Industrial HAT inputs and update the DataBank.
    ///
    /// Delegates to `read_4_20ma_inputs`, `read_0_10v_inputs`,
    /// `read_system_voltage`, and `read_opto_inputs`.
    fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()> {
        // 4-20 mA → HR 0-7 (mA × 100)
        let ma = self.read_4_20ma_inputs()?;
        for (i, &val) in ma.iter().enumerate() {
            db.holding_registers[i] = (val * 100.0) as u16;
        }

        // PSU voltage → HR 8 (V × 100)
        let voltage = self.read_system_voltage()?;
        db.holding_registers[8] = (voltage * 100.0) as u16;

        // 0-10 V → HR 10-13 (V × 100)
        let v = self.read_0_10v_inputs()?;
        for (i, &val) in v.iter().enumerate() {
            db.holding_registers[10 + i] = (val * 100.0) as u16;
        }

        // Opto → DI 0-7
        let (_bitmask, opto_bits) = self.read_opto_inputs()?;
        db.discrete_inputs[..OPTO_CHANNELS].copy_from_slice(&opto_bits);

        Ok(())
    }

    /// Apply OD and analog outputs from the DataBank to hardware.
    ///
    /// Delegates to `set_od_output`, `write_0_10v_output`, and
    /// `write_4_20ma_output` through the OutputCache.
    fn apply_outputs(&mut self, db: &DataBank, cache: &mut OutputCache) -> Result<()> {
        // OD outputs (coils 16-19)
        for i in 0..OD_CHANNELS {
            if cache.should_update_od(i, db.coils[16 + i]) {
                let ch = (i + 1) as u8;
                match self.set_od_output(ch, db.coils[16 + i]) {
                    Ok(()) => cache.confirm_od(i, db.coils[16 + i]),
                    Err(e) => {
                        cache.invalidate_od(i);
                        return Err(e);
                    }
                }
            }
        }

        // 0-10 V outputs (HR 16-19)
        for i in 0..U0_10_OUT_CHANNELS {
            let reg_val = db.holding_registers[HR_U0_10_OUT_BASE + i];
            if cache.should_update_v_out(i, reg_val) {
                let mv = reg_val.saturating_mul(10); // Modbus ×100 → mV
                let ch = (i + 1) as u8;
                match self.write_0_10v_output(ch, mv) {
                    Ok(()) => cache.confirm_v_out(i, reg_val),
                    Err(e) => {
                        cache.invalidate_v_out(i);
                        return Err(e);
                    }
                }
            }
        }

        // 4-20 mA outputs (HR 20-23)
        for i in 0..I4_20_OUT_CHANNELS {
            let reg_val = db.holding_registers[HR_I4_20_OUT_BASE + i];
            if cache.should_update_ma_out(i, reg_val) {
                let ua = reg_val.saturating_mul(10); // Modbus ×100 → µA
                let ch = (i + 1) as u8;
                match self.write_4_20ma_output(ch, ua) {
                    Ok(()) => cache.confirm_ma_out(i, reg_val),
                    Err(e) => {
                        cache.invalidate_ma_out(i);
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }
}
