//! I²C register map for Sequent Microsystems HATs.
//!
//! Ported from:
//! - MegaInd: <https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/megaind.h>
//! - MegaInd Python: <https://github.com/SequentMicrosystems/megaind-rpi/blob/main/python/megaind/__init__.py>
//! - 16-Relay: register layout follows the same set/clr convention.

// ============================================================================
// MegaInd (Industrial) HAT — I²C base address: 0x50 + stack_id
// ============================================================================

/// Base I²C address for the MegaInd Industrial HAT.
pub const MEGAIND_BASE_ADDR: u16 = 0x50;

/// Current relay/OD/LED output state bitmask (read).
pub const I2C_MEM_RELAY_VAL: u8 = 0x00;

/// Write a channel number (1-based) to SET (turn ON) an output.
pub const I2C_MEM_RELAY_SET: u8 = 0x01;

/// Write a channel number (1-based) to CLEAR (turn OFF) an output.
pub const I2C_MEM_RELAY_CLR: u8 = 0x02;

/// Opto-isolated input bitmask — 8 bits, one per channel (read-only).
pub const I2C_MEM_OPTO_IN_VAL: u8 = 0x03;

/// 0-10V analog output base (4 channels × 2 bytes, LE, millivolts).
pub const I2C_MEM_U0_10_OUT_VAL1: u8 = 0x04; // 4

/// 4-20mA analog output base (4 channels × 2 bytes, LE, milliamps × 1000).
pub const I2C_MEM_I4_20_OUT_VAL1: u8 = 0x0C; // 12

/// Open-drain PWM output base (4 channels × 2 bytes, LE, 0–10000 = 0–100.00%).
pub const I2C_MEM_OD_PWM1: u8 = 0x14; // 20

/// 0-10V analog input base (4 channels × 2 bytes, LE, millivolts).
pub const I2C_MEM_U0_10_IN_VAL1: u8 = 0x1C; // 28

/// ±10V analog input base (4 channels × 2 bytes, LE, millivolts + 10000 offset).
pub const I2C_MEM_U_PM_10_IN_VAL1: u8 = 0x24; // 36

/// 4-20mA analog input base (8 channels × 2 bytes, LE, milliamps × 1000).
pub const I2C_MEM_I4_20_IN_VAL1: u8 = 0x2C; // 44

/// Calibration value register.
pub const I2C_MEM_CALIB_VALUE: u8 = 0x3C; // 60

/// Modbus RS-485 settings base.
pub const I2C_MODBUS_SETTINGS_ADD: u8 = 0x41; // 65

/// RTC year register (base for 6-byte date/time block).
pub const I2C_RTC_YEAR_ADD: u8 = 0x46; // 70

/// Watchdog reset register.
pub const I2C_MEM_WDT_RESET_ADD: u8 = 0x53; // 83

/// Opto rising-edge counter enable bitmask.
pub const I2C_MEM_OPTO_RISING_ENABLE: u8 = 0x67; // 103

/// Opto falling-edge counter enable bitmask.
pub const I2C_MEM_OPTO_FALLING_ENABLE: u8 = 0x68; // 104

/// Write channel number to reset its opto counter.
pub const I2C_MEM_OPTO_CH_CONT_RESET: u8 = 0x69; // 105

/// Opto counter base (8 channels × 2 bytes, LE).
pub const I2C_MEM_OPTO_COUNT1: u8 = 0x6A; // 106

/// CPU temperature (1 byte, °C, read-only).
pub const I2C_MEM_DIAG_TEMPERATURE: u8 = 0x72; // 114

/// 24V supply rail voltage (2 bytes LE, millivolts, read-only).
pub const I2C_MEM_DIAG_24V: u8 = 0x73; // 115

/// Raspberry Pi supply rail (2 bytes LE, millivolts, read-only).
pub const I2C_MEM_DIAG_5V: u8 = 0x75; // 117

/// Firmware revision — major byte.
pub const I2C_MEM_REVISION_MAJOR: u8 = 0x78; // 120

/// Firmware revision — minor byte.
pub const I2C_MEM_REVISION_MINOR: u8 = 0x79; // 121

/// 1-Wire bus device count.
pub const I2C_MEM_1WB_DEV: u8 = 0x93; // 147

/// CPU/board reset trigger.
pub const I2C_MEM_CPU_RESET: u8 = 0xAA; // 170

/// 1-Wire temperature sensor data start.
pub const I2C_MEM_1WB_T1: u8 = 0xAE; // 174

// ============================================================================
// 16-Relay HAT — I²C base address: 0x20 + stack_id
// ============================================================================
//
// NOTE: Verify the base address against your specific hardware revision.
// The register layout follows the standard Sequent set/clr convention.

/// Base I²C address for the 16-Relay HAT.
pub const RELAY16_BASE_ADDR: u16 = 0x20;

/// Current relay state bitmask (2 bytes LE for 16 relays, read-only).
pub const RELAY16_MEM_RELAY_VAL: u8 = 0x00;

/// Write relay number (1–16) to SET (turn ON).
pub const RELAY16_MEM_RELAY_SET: u8 = 0x01;

/// Write relay number (1–16) to CLEAR (turn OFF).
pub const RELAY16_MEM_RELAY_CLR: u8 = 0x02;

// ============================================================================
// Shared constants
// ============================================================================

/// Scaling factor: raw register value = physical value × 1000.
pub const VOLT_TO_MILLIVOLT: f32 = 1000.0;

/// Number of 4-20mA input channels on the Industrial HAT.
pub const I4_20_IN_CHANNELS: usize = 8;

/// Number of 0-10V input channels on the Industrial HAT.
pub const U0_10_IN_CHANNELS: usize = 4;

/// Number of opto-isolated input channels.
pub const OPTO_CHANNELS: usize = 8;

/// Number of open-drain output channels on the Industrial HAT.
pub const OD_CHANNELS: usize = 4;

/// Number of relays on the 16-Relay HAT.
pub const RELAY16_CHANNELS: usize = 16;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn megaind_register_addresses_match_c_header() {
        // Cross-referenced against megaind.h and the Python library
        assert_eq!(I2C_MEM_RELAY_VAL, 0x00);
        assert_eq!(I2C_MEM_RELAY_SET, 0x01);
        assert_eq!(I2C_MEM_RELAY_CLR, 0x02);
        assert_eq!(I2C_MEM_OPTO_IN_VAL, 0x03);
        assert_eq!(I2C_MEM_U0_10_OUT_VAL1, 0x04);
        assert_eq!(I2C_MEM_I4_20_OUT_VAL1, 0x0C);
        assert_eq!(I2C_MEM_OD_PWM1, 0x14);
        assert_eq!(I2C_MEM_U0_10_IN_VAL1, 28);
        assert_eq!(I2C_MEM_U_PM_10_IN_VAL1, 36);
        assert_eq!(I2C_MEM_I4_20_IN_VAL1, 44);
        assert_eq!(I2C_MEM_CALIB_VALUE, 60);
        assert_eq!(I2C_MODBUS_SETTINGS_ADD, 65);
        assert_eq!(I2C_RTC_YEAR_ADD, 70);
        assert_eq!(I2C_MEM_WDT_RESET_ADD, 83);
        assert_eq!(I2C_MEM_OPTO_RISING_ENABLE, 103);
        assert_eq!(I2C_MEM_DIAG_TEMPERATURE, 114);
        assert_eq!(I2C_MEM_DIAG_24V, 115);
        assert_eq!(I2C_MEM_DIAG_5V, 117);
        assert_eq!(I2C_MEM_REVISION_MAJOR, 120);
        assert_eq!(I2C_MEM_REVISION_MINOR, 121);
    }

    #[test]
    fn analog_input_register_spacing() {
        // 4-20mA: 8 channels × 2 bytes, starting at 44, must not overlap calibration at 60
        for ch in 0..I4_20_IN_CHANNELS as u8 {
            let addr = I2C_MEM_I4_20_IN_VAL1 + ch * 2;
            assert!(addr < I2C_MEM_CALIB_VALUE, "4-20mA ch{ch} overlaps calibration");
        }
        // 0-10V: 4 channels × 2 bytes, starting at 28, must not overlap ±10V at 36
        for ch in 0..U0_10_IN_CHANNELS as u8 {
            let addr = I2C_MEM_U0_10_IN_VAL1 + ch * 2;
            assert!(addr < I2C_MEM_U_PM_10_IN_VAL1, "0-10V ch{ch} overlaps ±10V");
        }
    }
}
