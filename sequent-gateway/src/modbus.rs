//! Modbus TCP server.
//!
//! Implements a minimal Modbus TCP server over raw MBAP framing, supporting
//! the function codes needed for the gateway:
//!
//! | FC   | Name                   | Direction |
//! |------|------------------------|-----------|
//! | 0x01 | Read Coils             | Read      |
//! | 0x02 | Read Discrete Inputs   | Read      |
//! | 0x03 | Read Holding Registers | Read      |
//! | 0x05 | Write Single Coil      | Write     |
//! | 0x0F | Write Multiple Coils   | Write     |
//!
//! The server accepts any Modbus Unit ID (slave address), matching the
//! behaviour of the Python PoC (pyModbusTCP defaults).

use std::sync::{Arc, RwLock};

use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

use crate::databank::DataBank;

// ── Modbus function codes ────────────────────────────────────────────
const FC_READ_COILS: u8 = 0x01;
const FC_READ_DISCRETE_INPUTS: u8 = 0x02;
const FC_READ_HOLDING_REGISTERS: u8 = 0x03;
const FC_WRITE_SINGLE_COIL: u8 = 0x05;
const FC_WRITE_MULTIPLE_COILS: u8 = 0x0F;

// ── Modbus exception codes ──────────────────────────────────────────
const EX_ILLEGAL_FUNCTION: u8 = 0x01;
const EX_ILLEGAL_DATA_ADDRESS: u8 = 0x02;
const EX_ILLEGAL_DATA_VALUE: u8 = 0x03;

// ════════════════════════════════════════════════════════════════════════
// Server entry point
// ════════════════════════════════════════════════════════════════════════

/// Bind and run the Modbus TCP server forever.
///
/// Each incoming connection is handled in its own `tokio` task.
pub async fn serve(
    host: &str,
    port: u16,
    data_bank: Arc<RwLock<DataBank>>,
) -> Result<()> {
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("Modbus TCP server listening on {addr}");

    loop {
        let (stream, peer) = listener.accept().await?;
        debug!("Modbus client connected: {peer}");
        let db = data_bank.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, db).await {
                debug!("Connection from {peer} closed: {e:#}");
            }
        });
    }
}

// ════════════════════════════════════════════════════════════════════════
// Connection handler
// ════════════════════════════════════════════════════════════════════════

/// Process Modbus TCP frames on a single client connection.
///
/// MBAP Header (7 bytes):
/// ```text
/// [Transaction ID: 2] [Protocol ID: 2] [Length: 2] [Unit ID: 1]
/// ```
/// Followed by the PDU (function code + data).
async fn handle_connection(
    mut stream: TcpStream,
    data_bank: Arc<RwLock<DataBank>>,
) -> Result<()> {
    let mut header = [0u8; 7];

    loop {
        // Read MBAP header
        if stream.read_exact(&mut header).await.is_err() {
            return Ok(()); // client disconnected
        }

        let transaction_id = u16::from_be_bytes([header[0], header[1]]);
        let protocol_id = u16::from_be_bytes([header[2], header[3]]);
        let length = u16::from_be_bytes([header[4], header[5]]) as usize;
        let unit_id = header[6];

        // Validate frame
        if protocol_id != 0 || length < 1 || length > 253 {
            continue;
        }

        // Read PDU (length includes the unit_id byte we already consumed)
        let pdu_len = length - 1;
        let mut pdu = vec![0u8; pdu_len];
        stream.read_exact(&mut pdu).await?;

        let function_code = pdu[0];
        let pdu_data = &pdu[1..];

        // Process and respond
        let response_pdu = process_request(function_code, pdu_data, &data_bank);

        // Build MBAP response
        let resp_length = (response_pdu.len() + 1) as u16; // +1 for unit_id
        let mut resp = Vec::with_capacity(7 + response_pdu.len());
        resp.extend_from_slice(&transaction_id.to_be_bytes());
        resp.extend_from_slice(&0u16.to_be_bytes()); // protocol ID
        resp.extend_from_slice(&resp_length.to_be_bytes());
        resp.push(unit_id);
        resp.extend_from_slice(&response_pdu);

        stream.write_all(&resp).await?;
    }
}

// ════════════════════════════════════════════════════════════════════════
// Request dispatcher
// ════════════════════════════════════════════════════════════════════════

fn process_request(
    fc: u8,
    data: &[u8],
    data_bank: &Arc<RwLock<DataBank>>,
) -> Vec<u8> {
    match fc {
        FC_READ_COILS => read_bits(fc, data, data_bank, true),
        FC_READ_DISCRETE_INPUTS => read_bits(fc, data, data_bank, false),
        FC_READ_HOLDING_REGISTERS => read_registers(fc, data, data_bank),
        FC_WRITE_SINGLE_COIL => write_single_coil(fc, data, data_bank),
        FC_WRITE_MULTIPLE_COILS => write_multiple_coils(fc, data, data_bank),
        _ => {
            error!("Unsupported Modbus FC 0x{fc:02X}");
            exception(fc, EX_ILLEGAL_FUNCTION)
        }
    }
}

// ════════════════════════════════════════════════════════════════════════
// Function code handlers
// ════════════════════════════════════════════════════════════════════════

/// FC 0x01 Read Coils / FC 0x02 Read Discrete Inputs.
///
/// Request:  `[start_addr: u16, quantity: u16]`
/// Response: `[fc, byte_count, packed_bits…]`
fn read_bits(
    fc: u8,
    data: &[u8],
    data_bank: &Arc<RwLock<DataBank>>,
    is_coils: bool,
) -> Vec<u8> {
    if data.len() < 4 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let start = u16::from_be_bytes([data[0], data[1]]) as usize;
    let quantity = u16::from_be_bytes([data[2], data[3]]) as usize;

    if quantity == 0 || quantity > 2000 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let db = data_bank.read().unwrap();
    let bits: &[bool] = if is_coils {
        &db.coils
    } else {
        &db.discrete_inputs
    };

    if start + quantity > bits.len() {
        return exception(fc, EX_ILLEGAL_DATA_ADDRESS);
    }

    // Pack bools into bytes (LSB first per Modbus spec)
    let byte_count = (quantity + 7) / 8;
    let mut result = Vec::with_capacity(2 + byte_count);
    result.push(fc);
    result.push(byte_count as u8);

    for byte_idx in 0..byte_count {
        let mut byte_val = 0u8;
        for bit_idx in 0..8 {
            let idx = byte_idx * 8 + bit_idx;
            if idx < quantity && bits[start + idx] {
                byte_val |= 1 << bit_idx;
            }
        }
        result.push(byte_val);
    }

    result
}

/// FC 0x03 Read Holding Registers.
///
/// Request:  `[start_addr: u16, quantity: u16]`
/// Response: `[fc, byte_count, reg_hi, reg_lo, …]`
fn read_registers(
    fc: u8,
    data: &[u8],
    data_bank: &Arc<RwLock<DataBank>>,
) -> Vec<u8> {
    if data.len() < 4 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let start = u16::from_be_bytes([data[0], data[1]]) as usize;
    let quantity = u16::from_be_bytes([data[2], data[3]]) as usize;

    if quantity == 0 || quantity > 125 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let db = data_bank.read().unwrap();

    if start + quantity > db.holding_registers.len() {
        return exception(fc, EX_ILLEGAL_DATA_ADDRESS);
    }

    let byte_count = quantity * 2;
    let mut result = Vec::with_capacity(2 + byte_count);
    result.push(fc);
    result.push(byte_count as u8);

    for i in 0..quantity {
        let val = db.holding_registers[start + i];
        result.extend_from_slice(&val.to_be_bytes());
    }

    result
}

/// FC 0x05 Write Single Coil.
///
/// Request:  `[addr: u16, value: u16]`  (0xFF00 = ON, 0x0000 = OFF)
/// Response: echo of request.
fn write_single_coil(
    fc: u8,
    data: &[u8],
    data_bank: &Arc<RwLock<DataBank>>,
) -> Vec<u8> {
    if data.len() < 4 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let addr = u16::from_be_bytes([data[0], data[1]]) as usize;
    let value = u16::from_be_bytes([data[2], data[3]]);

    let state = match value {
        0xFF00 => true,
        0x0000 => false,
        _ => return exception(fc, EX_ILLEGAL_DATA_VALUE),
    };

    let mut db = data_bank.write().unwrap();
    if addr >= db.coils.len() {
        return exception(fc, EX_ILLEGAL_DATA_ADDRESS);
    }
    db.coils[addr] = state;

    // Echo the request PDU back as the response
    let mut result = Vec::with_capacity(5);
    result.push(fc);
    result.extend_from_slice(&data[..4]);
    result
}

/// FC 0x0F Write Multiple Coils.
///
/// Request:  `[start: u16, qty: u16, byte_count: u8, packed_bits…]`
/// Response: `[fc, start: u16, qty: u16]`
fn write_multiple_coils(
    fc: u8,
    data: &[u8],
    data_bank: &Arc<RwLock<DataBank>>,
) -> Vec<u8> {
    if data.len() < 5 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let start = u16::from_be_bytes([data[0], data[1]]) as usize;
    let quantity = u16::from_be_bytes([data[2], data[3]]) as usize;
    let byte_count = data[4] as usize;

    if quantity == 0 || quantity > 1968 || byte_count != (quantity + 7) / 8 {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }
    if data.len() < 5 + byte_count {
        return exception(fc, EX_ILLEGAL_DATA_VALUE);
    }

    let mut db = data_bank.write().unwrap();
    if start + quantity > db.coils.len() {
        return exception(fc, EX_ILLEGAL_DATA_ADDRESS);
    }

    // Unpack bits from bytes
    for i in 0..quantity {
        let byte_idx = i / 8;
        let bit_idx = i % 8;
        db.coils[start + i] = (data[5 + byte_idx] >> bit_idx) & 1 == 1;
    }

    // Response: fc + start address + quantity
    let mut result = Vec::with_capacity(5);
    result.push(fc);
    result.extend_from_slice(&(start as u16).to_be_bytes());
    result.extend_from_slice(&(quantity as u16).to_be_bytes());
    result
}

// ════════════════════════════════════════════════════════════════════════
// Helpers
// ════════════════════════════════════════════════════════════════════════

/// Build a Modbus exception response PDU.
fn exception(function_code: u8, exception_code: u8) -> Vec<u8> {
    vec![function_code | 0x80, exception_code]
}
