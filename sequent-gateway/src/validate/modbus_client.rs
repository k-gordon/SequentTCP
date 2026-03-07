//! Minimal synchronous Modbus TCP client for hardware validation.
//!
//! Implements only the function codes needed by the test suite:
//!
//! | FC   | Name                    | Direction |
//! |------|-------------------------|-----------|
//! | 0x01 | Read Coils              | Read      |
//! | 0x02 | Read Discrete Inputs    | Read      |
//! | 0x03 | Read Holding Registers  | Read      |
//! | 0x05 | Write Single Coil       | Write     |
//! | 0x06 | Write Single Register   | Write     |
//!
//! Uses blocking `std::net::TcpStream` — the validation runner does
//! not need async I/O.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{bail, Context, Result};

const PROTOCOL_ID: u16 = 0x0000;
const TIMEOUT: Duration = Duration::from_secs(5);

/// Blocking Modbus TCP client.
pub struct ModbusClient {
    stream: TcpStream,
    transaction_id: u16,
    unit_id: u8,
}

impl ModbusClient {
    /// Connect to a Modbus TCP server.
    pub fn connect(host: &str, port: u16, unit_id: u8) -> Result<Self> {
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr)
            .with_context(|| format!("Modbus connect to {addr}"))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        Ok(Self {
            stream,
            transaction_id: 0,
            unit_id,
        })
    }

    /// Change the Unit ID for subsequent requests.
    pub fn set_unit_id(&mut self, uid: u8) {
        self.unit_id = uid;
    }

    // ── Public function code wrappers ────────────────────────────────

    /// FC 01 — Read Coils.
    pub fn read_coils(&mut self, addr: u16, count: u16) -> Result<Vec<bool>> {
        self.read_bits(0x01, addr, count)
    }

    /// FC 02 — Read Discrete Inputs.
    pub fn read_discrete_inputs(
        &mut self,
        addr: u16,
        count: u16,
    ) -> Result<Vec<bool>> {
        self.read_bits(0x02, addr, count)
    }

    /// FC 03 — Read Holding Registers.
    pub fn read_holding_registers(
        &mut self,
        addr: u16,
        count: u16,
    ) -> Result<Vec<u16>> {
        let pdu = [addr.to_be_bytes(), count.to_be_bytes()].concat();
        let resp = self.transact(0x03, &pdu)?;

        // Response: FC (1) + byte_count (1) + N×2 register bytes
        if resp.len() < 2 {
            bail!("FC03 response too short ({} bytes)", resp.len());
        }
        let byte_count = resp[1] as usize;
        if resp.len() < 2 + byte_count {
            bail!("FC03 response truncated");
        }

        let mut regs = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let hi = resp[2 + i * 2] as u16;
            let lo = resp[2 + i * 2 + 1] as u16;
            regs.push((hi << 8) | lo);
        }
        Ok(regs)
    }

    /// FC 05 — Write Single Coil.
    pub fn write_single_coil(&mut self, addr: u16, value: bool) -> Result<()> {
        let val: u16 = if value { 0xFF00 } else { 0x0000 };
        let pdu = [addr.to_be_bytes(), val.to_be_bytes()].concat();
        let _resp = self.transact(0x05, &pdu)?;
        Ok(())
    }

    /// FC 06 — Write Single Register.
    pub fn write_single_register(
        &mut self,
        addr: u16,
        value: u16,
    ) -> Result<()> {
        let pdu = [addr.to_be_bytes(), value.to_be_bytes()].concat();
        let _resp = self.transact(0x06, &pdu)?;
        Ok(())
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Read bit-packed values (FC 01 / FC 02).
    fn read_bits(
        &mut self,
        fc: u8,
        addr: u16,
        count: u16,
    ) -> Result<Vec<bool>> {
        let pdu = [addr.to_be_bytes(), count.to_be_bytes()].concat();
        let resp = self.transact(fc, &pdu)?;

        // Response: FC (1) + byte_count (1) + N packed bytes
        if resp.len() < 2 {
            bail!("FC{fc:02X} response too short");
        }
        let byte_count = resp[1] as usize;
        if resp.len() < 2 + byte_count {
            bail!("FC{fc:02X} response truncated");
        }

        let mut bits = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let byte_idx = i / 8;
            let bit_idx = i % 8;
            let set = (resp[2 + byte_idx] >> bit_idx) & 1 == 1;
            bits.push(set);
        }
        Ok(bits)
    }

    /// Send a Modbus request and receive the response PDU.
    ///
    /// Builds the MBAP frame, sends it, reads the response MBAP + PDU,
    /// and checks for Modbus exception responses.
    fn transact(&mut self, fc: u8, pdu_data: &[u8]) -> Result<Vec<u8>> {
        self.transaction_id = self.transaction_id.wrapping_add(1);
        let tid = self.transaction_id;

        // PDU = FC (1) + data
        let pdu_len = 1 + pdu_data.len();
        // MBAP length field = Unit ID (1) + PDU
        let mbap_length = (1 + pdu_len) as u16;

        // Build frame: TID (2) + Protocol (2) + Length (2) + UID (1) + FC (1) + Data
        let mut frame = Vec::with_capacity(7 + pdu_len);
        frame.extend_from_slice(&tid.to_be_bytes());
        frame.extend_from_slice(&PROTOCOL_ID.to_be_bytes());
        frame.extend_from_slice(&mbap_length.to_be_bytes());
        frame.push(self.unit_id);
        frame.push(fc);
        frame.extend_from_slice(pdu_data);

        self.stream
            .write_all(&frame)
            .context("Modbus TCP write")?;

        // Read MBAP header (7 bytes)
        let mut header = [0u8; 7];
        self.stream
            .read_exact(&mut header)
            .context("Modbus TCP read header")?;

        let resp_length = u16::from_be_bytes([header[4], header[5]]) as usize;
        if resp_length < 1 {
            bail!("MBAP length is zero");
        }

        // resp_length includes the Unit ID byte we already consumed
        // in the header read (byte 6).  The PDU follows.
        let pdu_bytes = resp_length - 1;
        let mut resp_pdu = vec![0u8; pdu_bytes];
        self.stream
            .read_exact(&mut resp_pdu)
            .context("Modbus TCP read PDU")?;

        // Check for exception response (FC | 0x80)
        if !resp_pdu.is_empty() && resp_pdu[0] == (fc | 0x80) {
            let exc = resp_pdu.get(1).copied().unwrap_or(0);
            bail!("Modbus exception FC=0x{:02X} code=0x{exc:02X}", fc | 0x80);
        }

        Ok(resp_pdu)
    }
}
