#!/usr/bin/env python3
"""
╔══════════════════════════════════════════════════════════════════╗
║  ⚠️  DEPRECATED — DO NOT USE FOR NEW DEPLOYMENTS                ║
║                                                                  ║
║  This Python gateway has been superseded by the Rust binary:     ║
║    sequent-gateway/  (see README.md for build & install)         ║
║                                                                  ║
║  The Rust version provides:                                      ║
║    • Direct I²C HAL (no subprocess shelling to CLI tools)        ║
║    • < 1 ms I/O cycle (vs ~100 ms with subprocess)               ║
║    • Analog output write-back (0-10V, 4-20mA)                   ║
║    • Multi-slave addressing, health endpoint, log rotation       ║
║    • I²C bus recovery, channel watchdog                          ║
║    • Dynamic board selection (MegaInd, 16-Relay, 8-Relay)        ║
║    • Single static binary — no Python runtime on the Pi          ║
║                                                                  ║
║  This file is kept for historical reference only.                ║
╚══════════════════════════════════════════════════════════════════╝
"""

import sys
import warnings

warnings.warn(
    "modbusTCP.py is DEPRECATED. Use the Rust gateway instead: "
    "see sequent-gateway/ and README.md for instructions.",
    DeprecationWarning,
    stacklevel=2,
)
print(
    "\n⚠️  This Python gateway is DEPRECATED.\n"
    "   The Rust replacement lives in sequent-gateway/.\n"
    "   Run: cd sequent-gateway && cargo build --release\n"
    "   See README.md for full instructions.\n",
    file=sys.stderr,
)

import time
import subprocess
import logging
import os
import argparse
from pyModbusTCP.server import ModbusServer, DataBank

"""
LEGACY MODBUS MEMORY MAP (Slave ID 1 or Any):
----------------------------------------------
COILS (Read/Write):
[0-15]    : Relay Board (Stack 0) Relays 1-16
[16-19]   : Industrial Board (Stack 1) Open Drain Outputs 1-4

DISCRETE INPUTS (Read Only):
[0-7]     : Industrial Board (Stack 1) Opto-Inputs 1-8

HOLDING REGISTERS (Read Only):
[0-7]     : Industrial Board (Stack 1) 4-20mA Inputs 1-8 (mA * 100)
[8]       : Industrial Board (Stack 1) PSU Voltage (V * 100)
[10-13]   : Industrial Board (Stack 1) 0-10V Inputs 1-4 (V * 100)
[15]      : Opto-Input Bitmask (0-255) - ONLY if --map-opto-to-reg is used
----------------------------------------------
"""

# --- DEFAULT CONFIGURATION ---
DEFAULT_IP = "0.0.0.0"
DEFAULT_PORT = 502
POLL_INTERVAL = 0.1  # 10Hz loop
LOG_INTERVAL = 5.0   # Log telemetry every 5 seconds

logging.basicConfig(
    level=logging.INFO, 
    format='%(asctime)s - %(levelname)s - %(message)s',
    handlers=[logging.StreamHandler(sys.stdout)]
)

class IndustrialBoard:
    """Handles the Sequent Industrial HAT (Stack 1)"""
    def __init__(self, stack_id=1):
        self.stack_id = str(stack_id)
        self.current_od_states = [None] * 4

    def read_opto_inputs(self):
        """Reads all 8 Opto-Isolated Inputs."""
        try:
            res = subprocess.run(["megaind", self.stack_id, "optord"], capture_output=True, text=True)
            val = int(res.stdout.strip())
            # Return both the bitmask and the boolean list
            bits = [(val >> i) & 1 == 1 for i in range(8)]
            return val, bits
        except:
            return 0, [False] * 8

    def read_analogs_4_20(self):
        """Reads 4-20mA inputs. Returns list of floats."""
        readings = [0.0] * 8
        for ch in range(1, 9):
            try:
                res = subprocess.run(["megaind", self.stack_id, "iinrd", str(ch)], capture_output=True, text=True, timeout=0.5)
                output = res.stdout.strip()
                if output and "out of range" not in output.lower():
                    readings[ch-1] = float(output.split()[0])
            except: pass
        return readings

    def read_analogs_0_10(self):
        """Reads 0-10V inputs. Returns list of floats."""
        readings = [0.0] * 4
        for ch in range(1, 5):
            try:
                res = subprocess.run(["megaind", self.stack_id, "uinrd", str(ch)], capture_output=True, text=True, timeout=0.5)
                output = res.stdout.strip()
                if output and "error" not in output.lower():
                    parts = output.split()
                    if parts:
                        readings[ch-1] = float(parts[0])
            except: pass
        return readings

    def read_system_voltage(self):
        """Reads supply voltage."""
        try:
            res = subprocess.run(["megaind", self.stack_id, "board"], capture_output=True, text=True)
            for line in res.stdout.split(','):
                if "Power source" in line:
                    return float(line.strip().split()[2])
        except: pass
        return 0.0

    def set_od_outputs(self, target_states):
        """Sets Open Drain outputs 1-4."""
        for i, state in enumerate(target_states):
            if state != self.current_od_states[i]:
                relay_num = i + 1
                cmd = "1" if state else "0"
                try:
                    subprocess.run(["megaind", self.stack_id, "odwr", str(relay_num), cmd], capture_output=True)
                    self.current_od_states[i] = state
                    logging.info(f"Ind. Board OD Output {relay_num} -> {'ON' if state else 'OFF'}")
                except Exception as e:
                    logging.error(f"Failed Ind. OD {relay_num}: {e}")

class RelayBoard:
    """Handles the Sequent 16-Relay HAT (Stack 0)"""
    def __init__(self, stack_id=0):
        self.stack_id = str(stack_id)
        self.current_states = [None] * 16 

    def set_relays(self, target_states):
        """Updates relays based on target coil states."""
        for i, state in enumerate(target_states):
            if state != self.current_states[i]:
                relay_num = i + 1
                cmd = "1" if state else "0"
                try:
                    subprocess.run(["16relind", self.stack_id, "write", str(relay_num), cmd], capture_output=True)
                    self.current_states[i] = state
                    logging.info(f"Relay {relay_num} -> {'ON' if state else 'OFF'}")
                except Exception as e:
                    logging.error(f"Failed Relay {relay_num}: {e}")

class ModbusI2CGateway:
    def __init__(self, host, port, map_opto):
        self.ind_board = IndustrialBoard(stack_id=1)
        self.rel_board = RelayBoard(stack_id=0)
        self.server = ModbusServer(host=host, port=port, no_block=True)
        self.map_opto = map_opto
        self.last_log_time = 0
        
    def update_cycle(self):
        # 1. READ HARDWARE
        ma_inputs = self.ind_board.read_analogs_4_20()
        v_inputs = self.ind_board.read_analogs_0_10()
        voltage = self.ind_board.read_system_voltage()
        opto_val, opto_bits = self.ind_board.read_opto_inputs()
        
        # 2. UPDATE MODBUS DATABANK
        self.server.data_bank.set_holding_registers(0, [int(v * 100) for v in ma_inputs])
        self.server.data_bank.set_holding_registers(8, [int(voltage * 100)])
        self.server.data_bank.set_holding_registers(10, [int(v * 100) for v in v_inputs])
        self.server.data_bank.set_discrete_inputs(0, opto_bits)
        
        if self.map_opto:
            self.server.data_bank.set_holding_registers(15, [opto_val])
        
        # 3. APPLY OUTPUTS
        all_coils = self.server.data_bank.get_coils(0, 20)
        if all_coils:
            self.rel_board.set_relays(all_coils[0:16])
            self.ind_board.set_od_outputs(all_coils[16:20])

        # 4. FULL HEARTBEAT LOGGING
        current_time = time.time()
        if current_time - self.last_log_time > LOG_INTERVAL:
            ma_str = " ".join([f"{v:4.1f}" for v in ma_inputs])
            v_str = " ".join([f"{v:4.1f}" for v in v_inputs])
            opto_str = "".join(["1" if b else "0" for b in reversed(opto_bits)])
            relay_str = "".join(["1" if (all_coils and all_coils[i]) else "0" for i in range(16)])
            od_str = "".join(["1" if (all_coils and all_coils[16+i]) else "0" for i in range(4)])
            
            logging.info("--- SYSTEM HEARTBEAT ---")
            logging.info(f"POWER: {voltage:.2f}V")
            logging.info(f"4-20mA (1-8) : [{ma_str}] mA")
            logging.info(f"0-10V  (1-4) : [{v_str}] V")
            logging.info(f"OPTO INPUTS  : {opto_str} (Binary)")
            logging.info(f"RELAYS (1-16): {relay_str}")
            logging.info(f"OD OUT (1-4) : {od_str}")
            logging.info("------------------------")
            self.last_log_time = current_time

    def start(self):
        if os.geteuid() != 0:
            logging.warning("Warning: Script not running as root. I2C commands may fail.")
            
        logging.info(f"Starting Gateway on {self.server.host}:{self.server.port}")
        if self.map_opto:
            logging.info("Opto-Inputs also mapped to Holding Register 15")
        self.server.start()
        try:
            while True:
                start_loop = time.time()
                self.update_cycle()
                duration = time.time() - start_loop
                time.sleep(max(0, POLL_INTERVAL - duration))
        except KeyboardInterrupt:
            logging.info("Shutting down...")
            self.server.stop()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Modbus-to-I2C Gateway for Sequent HATs")
    parser.add_argument("--host", default=DEFAULT_IP, help=f"IP address to bind (default: {DEFAULT_IP})")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help=f"TCP port (default: {DEFAULT_PORT})")
    parser.add_argument("--map-opto-to-reg", action="store_true", help="Map Opto-Inputs to Holding Register 15")
    args = parser.parse_args()

    gateway = ModbusI2CGateway(host=args.host, port=args.port, map_opto=args.map_opto_to_reg)
    gateway.start()