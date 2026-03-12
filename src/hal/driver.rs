//! Generic I²C HAL driver for all Sequent Microsystems boards.
//!
//! A single driver handles all board types by reading I/O group
//! descriptors from TOML board definitions.  Adding support for a new
//! board requires only a TOML file — no Rust code changes.
//!
//! ## Supported I²C operations
//!
//! | `op` value          | Dir    | Description                              |
//! |---------------------|--------|------------------------------------------|
//! | `read_u8_bitmask`   | input  | 1-byte bitmask → discrete inputs         |
//! | `read_u16_bitmask`  | input  | 2-byte LE bitmask → discrete inputs      |
//! | `read_u16_le`       | input  | N × 2-byte LE → holding registers        |
//! | `write_set_clr`     | output | Channel number to SET/CLR register       |
//! | `write_u16_le`      | output | N × 2-byte LE from holding registers     |
//! | `pca9535_rmw_bit`   | output | Read-modify-write on 16-bit port (relays)|

use anyhow::{Context, Result};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use tracing::{debug, info, warn};

use crate::board_def::{BoardDef, IoGroup};
use crate::cache::OutputCache;
use crate::databank::DataBank;
use crate::hal::traits::BoardCapability;

/// Generic HAL for any Sequent Microsystems board.
///
/// Behaviour is driven entirely by the `[[io_group]]` descriptors in
/// the board TOML.  Two board instances backed by different TOMLs will
/// perform completely different I²C transactions.
pub struct GenericBoard {
    dev: LinuxI2CDevice,
    name: String,
    stack_id: u8,
    input_groups: Vec<IoGroup>,
    output_groups: Vec<IoGroup>,
    capabilities: Vec<BoardCapability>,
    relay_count: usize,
    /// Index into `output_groups` for the relay group (for read-back).
    relay_output_idx: Option<usize>,
    cache: OutputCache,
}

impl GenericBoard {
    /// Open the I²C bus and initialise the board from its definition.
    ///
    /// If `io_groups` is empty in the definition, they are
    /// auto-synthesized from the legacy `[channels]` + `[registers]`
    /// config for backward compatibility.
    pub fn new(bus: &str, stack_id: u8, def: &BoardDef) -> Result<Self> {
        // Clone + synthesize so callers can keep their original def.
        let mut def = def.clone();
        def.synthesize_io_groups();

        let addr = def.address.resolve(stack_id);
        let mut dev = LinuxI2CDevice::new(bus, addr)
            .with_context(|| format!("Failed to open {bus} at 0x{addr:02X}"))?;
        debug!(
            "Opened {} at {bus} 0x{addr:02X} (stack {stack_id})",
            def.board.name
        );

        // ── PCA9535 initialisation ───────────────────────────────────
        if let Some(ref pca) = def.pca9535 {
            let mut cfg_buf = [0u8; 2];
            dev.write(&[pca.config_reg])
                .with_context(|| "PCA9535: select config register")?;
            dev.read(&mut cfg_buf)
                .with_context(|| "PCA9535: read config register")?;
            let cfg_val = u16::from_le_bytes(cfg_buf);
            if cfg_val != 0 {
                info!(
                    "{}: configuring I/O expander pins as outputs",
                    def.board.name
                );
                dev.write(&[pca.outport_reg, 0, 0])
                    .with_context(|| "PCA9535: clear outport")?;
                dev.write(&[pca.config_reg, 0, 0])
                    .with_context(|| "PCA9535: set all as outputs")?;
            }
        }

        // ── Firmware version (best-effort) ───────────────────────────
        if let Some(major_reg) = def.registers.revision_major {
            let mut buf = [0u8; 2];
            if dev.write(&[major_reg]).is_ok() && dev.read(&mut buf).is_ok() {
                info!(
                    "{} firmware: v{:02}.{:02}",
                    def.board.name, buf[0], buf[1]
                );
            }
        }

        // ── Partition I/O groups ─────────────────────────────────────
        let input_groups: Vec<IoGroup> = def
            .io_groups
            .iter()
            .filter(|g| g.direction == "input")
            .cloned()
            .collect();
        let output_groups: Vec<IoGroup> = def
            .io_groups
            .iter()
            .filter(|g| g.direction == "output")
            .cloned()
            .collect();

        let capabilities = derive_capabilities(&def);
        let relay_count = def.channels.relays.unwrap_or(0);

        // Find the relay output group (for PCA9535 read-back).
        let relay_output_idx = output_groups
            .iter()
            .position(|g| g.op == "pca9535_rmw_bit" && g.modbus_region == "coil");

        let output_sizes: Vec<usize> = output_groups.iter().map(|g| g.channels).collect();
        let cache = OutputCache::from_groups(&output_sizes);
        let name = def.board.name.clone();

        Ok(Self {
            dev,
            name,
            stack_id,
            input_groups,
            output_groups,
            capabilities,
            relay_count,
            relay_output_idx,
            cache,
        })
    }

    // ── Low-level I²C helpers ────────────────────────────────────────

    /// Write 1-byte register address, then read `buf.len()` bytes back.
    fn i2c_read(&mut self, register: u8, buf: &mut [u8]) -> Result<()> {
        self.dev
            .write(&[register])
            .with_context(|| format!("I²C select 0x{register:02X}"))?;
        self.dev
            .read(buf)
            .with_context(|| format!("I²C read {} bytes from 0x{register:02X}", buf.len()))?;
        Ok(())
    }

    /// Write 1-byte register address followed by `data` in one transaction.
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
    fn read_u16_le_reg(&mut self, register: u8) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.i2c_read(register, &mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Write a 16-bit little-endian value to a register pair.
    fn write_u16_le_reg(&mut self, register: u8, val: u16) -> Result<()> {
        self.i2c_write(register, &val.to_le_bytes())
    }
}

// ════════════════════════════════════════════════════════════════════════
// Capability derivation
// ════════════════════════════════════════════════════════════════════════

/// Derive board capabilities from the `[channels]` config.
fn derive_capabilities(def: &BoardDef) -> Vec<BoardCapability> {
    let ch = &def.channels;
    let mut caps = Vec::new();
    if ch.relays.unwrap_or(0) > 0 {
        caps.push(BoardCapability::Relays);
    }
    if ch.opto_inputs.unwrap_or(0) > 0 {
        caps.push(BoardCapability::DiscreteInputs);
    }
    if ch.od_outputs.unwrap_or(0) > 0 {
        caps.push(BoardCapability::DiscreteOutputs);
    }
    if ch.analog_4_20ma_inputs.unwrap_or(0) > 0
        || ch.analog_0_10v_inputs.unwrap_or(0) > 0
    {
        caps.push(BoardCapability::AnalogInputs);
    }
    if ch.analog_4_20ma_outputs.unwrap_or(0) > 0
        || ch.analog_0_10v_outputs.unwrap_or(0) > 0
    {
        caps.push(BoardCapability::AnalogOutputs);
    }
    caps
}

// ════════════════════════════════════════════════════════════════════════
// SequentBoard trait implementation
// ════════════════════════════════════════════════════════════════════════

impl super::traits::SequentBoard for GenericBoard {
    fn name(&self) -> &str {
        &self.name
    }

    fn stack_id(&self) -> u8 {
        self.stack_id
    }

    fn capabilities(&self) -> &[BoardCapability] {
        &self.capabilities
    }

    fn relay_count(&self) -> usize {
        self.relay_count
    }

    // ── Input polling ────────────────────────────────────────────────

    fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()> {
        for idx in 0..self.input_groups.len() {
            // Extract everything we need before calling &mut self methods.
            let op = self.input_groups[idx].op.clone();
            let register = self.input_groups[idx].register.unwrap_or(0);
            let channels = self.input_groups[idx].channels;
            let offset = self.input_groups[idx].modbus_offset;
            let region = self.input_groups[idx].modbus_region.clone();
            let i2c_scale = self.input_groups[idx].i2c_scale;
            let modbus_scale = self.input_groups[idx].modbus_scale;

            match op.as_str() {
                "read_u8_bitmask" => {
                    let mut buf = [0u8; 1];
                    self.i2c_read(register, &mut buf)?;
                    let val = buf[0];
                    Self::write_bitmask_u8(val, channels, offset, &region, db);
                }

                "read_u16_bitmask" => {
                    let val = self.read_u16_le_reg(register)?;
                    Self::write_bitmask_u16(val, channels, offset, &region, db);
                }

                "read_u16_le" => {
                    let name = self.input_groups[idx].name.clone();
                    for ch in 0..channels {
                        let reg = register.wrapping_add((ch as u8) * 2);
                        match self.read_u16_le_reg(reg) {
                            Ok(raw) => {
                                let modbus_val =
                                    (raw as f32 * modbus_scale / i2c_scale) as u16;
                                if let Some(slot) =
                                    db.holding_registers.get_mut(offset + ch)
                                {
                                    *slot = modbus_val;
                                }
                            }
                            Err(e) => {
                                warn!("{} ch{}: {e:#}", name, ch + 1);
                            }
                        }
                    }
                }

                other => warn!("Unknown input op: {other}"),
            }
        }
        Ok(())
    }

    // ── Output application ───────────────────────────────────────────

    fn apply_outputs(&mut self, db: &DataBank) -> Result<()> {
        for idx in 0..self.output_groups.len() {
            let op = self.output_groups[idx].op.clone();
            let channels = self.output_groups[idx].channels;
            let offset = self.output_groups[idx].modbus_offset;

            match op.as_str() {
                "write_set_clr" => {
                    let reg_set = self.output_groups[idx].register_set.unwrap();
                    let reg_clr = self.output_groups[idx].register_clr.unwrap();
                    for ch in 0..channels {
                        let coil = db.coils.get(offset + ch).copied().unwrap_or(false);
                        let val = u16::from(coil);
                        if self.cache.should_update(idx, ch, val) {
                            let reg = if coil { reg_set } else { reg_clr };
                            match self.i2c_write(reg, &[(ch as u8 + 1)]) {
                                Ok(()) => {
                                    self.cache.confirm(idx, ch, val);
                                    debug!(
                                        "{} ch{} → {}",
                                        self.output_groups[idx].name,
                                        ch + 1,
                                        if coil { "ON" } else { "OFF" }
                                    );
                                }
                                Err(e) => {
                                    self.cache.invalidate(idx, ch);
                                    return Err(e);
                                }
                            }
                        }
                    }
                }

                "write_u16_le" => {
                    let register = self.output_groups[idx].register.unwrap();
                    let i2c_scale = self.output_groups[idx].i2c_scale;
                    let modbus_scale = self.output_groups[idx].modbus_scale;
                    for ch in 0..channels {
                        let reg_val = db
                            .holding_registers
                            .get(offset + ch)
                            .copied()
                            .unwrap_or(0);
                        if self.cache.should_update(idx, ch, reg_val) {
                            let raw_f = reg_val as f32 * i2c_scale / modbus_scale;
                            let raw = if raw_f > u16::MAX as f32 {
                                u16::MAX
                            } else {
                                raw_f as u16
                            };
                            let reg = register.wrapping_add((ch as u8) * 2);
                            match self.i2c_write(reg, &raw.to_le_bytes()) {
                                Ok(()) => {
                                    self.cache.confirm(idx, ch, reg_val);
                                    debug!(
                                        "{} ch{} → {}",
                                        self.output_groups[idx].name,
                                        ch + 1,
                                        reg_val
                                    );
                                }
                                Err(e) => {
                                    self.cache.invalidate(idx, ch);
                                    return Err(e);
                                }
                            }
                        }
                    }
                }

                "pca9535_rmw_bit" => {
                    let register = self.output_groups[idx].register.unwrap();
                    let remap = self.output_groups[idx]
                        .bit_remap
                        .clone()
                        .unwrap_or_default();

                    // Check if any channel needs updating.
                    let mut any_change = false;
                    for ch in 0..channels {
                        let coil = db.coils.get(offset + ch).copied().unwrap_or(false);
                        let val = u16::from(coil);
                        if self.cache.should_update(idx, ch, val) {
                            any_change = true;
                            break;
                        }
                    }

                    if any_change {
                        // Single read, batch-modify, single write.
                        let mut io_val = self.read_u16_le_reg(register)?;

                        for ch in 0..channels {
                            let coil =
                                db.coils.get(offset + ch).copied().unwrap_or(false);
                            let val = u16::from(coil);
                            if self.cache.should_update(idx, ch, val) {
                                let bit =
                                    remap.get(ch).copied().unwrap_or(ch as u8);
                                if coil {
                                    io_val |= 1 << bit;
                                } else {
                                    io_val &= !(1 << bit);
                                }
                            }
                        }

                        match self.write_u16_le_reg(register, io_val) {
                            Ok(()) => {
                                for ch in 0..channels {
                                    let coil = db
                                        .coils
                                        .get(offset + ch)
                                        .copied()
                                        .unwrap_or(false);
                                    self.cache.confirm(idx, ch, u16::from(coil));
                                }
                            }
                            Err(e) => {
                                for ch in 0..channels {
                                    self.cache.invalidate(idx, ch);
                                }
                                return Err(e);
                            }
                        }
                    }
                }

                other => warn!("Unknown output op: {other}"),
            }
        }
        Ok(())
    }

    // ── Relay read-back ──────────────────────────────────────────────

    fn read_relay_state(&mut self) -> Result<u16> {
        if let Some(idx) = self.relay_output_idx {
            let register = self.output_groups[idx].register.unwrap();
            let remap = self.output_groups[idx]
                .bit_remap
                .clone()
                .unwrap_or_default();
            let raw = self.read_u16_le_reg(register)?;
            let mut logical: u16 = 0;
            for (relay_idx, &bit_pos) in remap.iter().enumerate() {
                if raw & (1 << bit_pos) != 0 {
                    logical |= 1 << relay_idx;
                }
            }
            Ok(logical)
        } else {
            Ok(0)
        }
    }

    fn expected_relay_bitmask(&self) -> u16 {
        if let Some(idx) = self.relay_output_idx {
            self.cache.bitmask(idx, self.relay_count)
        } else {
            0
        }
    }

    fn has_confirmed_relay(&self, index: usize) -> bool {
        if let Some(idx) = self.relay_output_idx {
            self.cache.has_confirmed(idx, index)
        } else {
            false
        }
    }

    fn invalidate_relay(&mut self, index: usize) {
        if let Some(idx) = self.relay_output_idx {
            self.cache.invalidate(idx, index);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Bitmask helpers (no &mut self — avoids borrow conflicts)
// ════════════════════════════════════════════════════════════════════════

impl GenericBoard {
    fn write_bitmask_u8(
        val: u8,
        channels: usize,
        offset: usize,
        region: &str,
        db: &mut DataBank,
    ) {
        match region {
            "discrete_input" => {
                for i in 0..channels.min(8) {
                    if let Some(slot) = db.discrete_inputs.get_mut(offset + i) {
                        *slot = (val >> i) & 1 == 1;
                    }
                }
            }
            "coil" => {
                for i in 0..channels.min(8) {
                    if let Some(slot) = db.coils.get_mut(offset + i) {
                        *slot = (val >> i) & 1 == 1;
                    }
                }
            }
            _ => warn!("Unsupported region '{region}' for read_u8_bitmask"),
        }
    }

    fn write_bitmask_u16(
        val: u16,
        channels: usize,
        offset: usize,
        region: &str,
        db: &mut DataBank,
    ) {
        match region {
            "discrete_input" => {
                for i in 0..channels.min(16) {
                    if let Some(slot) = db.discrete_inputs.get_mut(offset + i) {
                        *slot = (val >> i) & 1 == 1;
                    }
                }
            }
            "coil" => {
                for i in 0..channels.min(16) {
                    if let Some(slot) = db.coils.get_mut(offset + i) {
                        *slot = (val >> i) & 1 == 1;
                    }
                }
            }
            _ => warn!("Unsupported region '{region}' for read_u16_bitmask"),
        }
    }
}
