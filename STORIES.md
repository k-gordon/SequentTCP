# Epic & Stories — SequentTCP Rust Gateway

## Epic: Rewrite Sequent Modbus TCP Gateway in Rust

**Epic ID:** SEQGW-EPIC-1

**Summary:** Replace the Python proof-of-concept Modbus TCP ↔ I²C gateway with a production-grade Rust binary that talks directly to Sequent HAT hardware over the I²C bus — eliminating subprocess overhead, enabling single-binary deployment, and achieving sub-millisecond I/O cycle times.

**Business Value:** The current Python gateway validates the architecture but shells out to `megaind`/`16relind` CLI tools on every cycle, adding ~100 ms latency and creating fragile stdout-parsing dependencies. A Rust rewrite delivers a self-contained, memory-safe, `systemd`-ready binary suitable for unattended industrial deployment.

**Acceptance Criteria (Epic-level):**
- Gateway binary cross-compiles to `aarch64`/`armv7` and runs on Raspberry Pi 4
- All Modbus register mappings from the Python PoC are preserved (coils, discrete inputs, holding registers)
- Full I/O cycle completes in < 1 ms (vs ~100+ ms today)
- Single binary, no runtime dependencies on the target device
- Passes side-by-side validation against the Python gateway's known-good output

---

## Phase 0 — Rust Port (Core Gateway)

### Story 1: Project Scaffold & Build Pipeline

**ID:** SEQGW-1 · **Points:** 2

**As a** developer,
**I want** a Rust project scaffold with cross-compilation support,
**so that** I can build and iterate on the gateway from a dev machine targeting the Pi.

**Acceptance Criteria:**
- [x] `cargo init sequent-gateway` with `edition = "2021"`
- [x] `Cargo.toml` includes dependencies: `i2cdev`, `tokio`, `clap`, `tracing`, `tracing-subscriber` (custom Modbus TCP server instead of `tokio-modbus`)
- [x] `.cargo/config.toml` configured for `aarch64-unknown-linux-gnu` and `armv7-unknown-linux-gnueabihf` targets
- [x] `cross` or `cargo-zigbuild` builds a working ARM binary
- [x] CI-ready `Makefile` or `justfile` with `build`, `build-release`, `cross` targets

---

### Story 2: I²C Register Map

**ID:** SEQGW-2 · **Points:** 3

**As a** developer,
**I want** the Sequent I²C register map ported to Rust as type-safe constants,
**so that** all hardware access uses verified register addresses with no magic numbers.

**Acceptance Criteria:**
- [x] `src/registers.rs` contains typed `pub const` constants ported from [`megaind.h`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/megaind.h)
- [x] Covers: relay set/clr (`0x01`/`0x02`), opto input (`0x03`), OD PWM (`0x14`), analog I/O base addresses, diagnostics (`0x72`–`0x79`), RTC, watchdog, CPU reset, 1-Wire
- [x] 16-Relay HAT address constants included (base address `0x20`, relay set/clr registers)
- [x] Constants for `MEGAIND_BASE_ADDR` (`0x50`) / `RELAY16_BASE_ADDR` (`0x20`) and stack offset logic
- [x] `VOLT_TO_MILLIVOLT` scaling constant (`1000.0`)
- [x] Unit tests verifying key register addresses match the C header (2 tests)

---

### Story 3: I²C HAL — Industrial Board (MegaInd)

**ID:** SEQGW-3 · **Points:** 5

**As a** developer,
**I want** a `MegaIndBoard` struct that reads/writes the Industrial HAT over I²C,
**so that** the gateway can access all analog/digital I/O without CLI tools.

**Acceptance Criteria:**
- [x] `src/hal/megaind.rs` — struct wrapping `i2cdev::linux::LinuxI2CDevice`
- [x] `new(bus: &str, stack_id: u8)` constructor opens `/dev/i2c-1` at address `0x50 + stack_id`
- [x] `read_opto_inputs() -> (u8, [bool; 8])` — reads `I2C_MEM_OPTO_IN_VAL` (1 byte), returns bitmask + bool array
- [x] `read_4_20ma_inputs() -> [f32; 8]` — reads 8 × 16-bit LE values from `I2C_MEM_I4_20_IN_VAL1`, divides by `VOLT_TO_MILLIVOLT`
- [x] `read_0_10v_inputs() -> [f32; 4]` — reads 4 × 16-bit LE values from `I2C_MEM_U0_10_IN_VAL1`, divides by `VOLT_TO_MILLIVOLT`
- [x] `read_system_voltage() -> f32` — reads 16-bit LE from `I2C_MEM_DIAG_24V`, divides by `VOLT_TO_MILLIVOLT`
- [x] `set_od_output(channel: u8, state: bool)` — writes `I2C_MEM_RELAY_SET` or `I2C_MEM_RELAY_CLR` with channel byte
- [x] All reads use the 1-byte-address-prefix protocol from Sequent's [`comm.c`](https://github.com/SequentMicrosystems/megaind-rpi/blob/main/src/comm.c)
- [x] Errors are returned as `Result<T, anyhow::Error>`, not panics

---

### Story 4: I²C HAL — 16-Relay Board

**ID:** SEQGW-4 · **Points:** 3

**As a** developer,
**I want** a `RelayBoard` struct that controls the 16-Relay HAT over I²C,
**so that** relay outputs are driven directly without the `16relind` CLI tool.

**Acceptance Criteria:**
- [x] `src/hal/relay16.rs` — struct wrapping `i2cdev::linux::LinuxI2CDevice`
- [x] `new(bus: &str, stack_id: u8)` constructor opens `/dev/i2c-1` at address `0x20 + stack_id`
- [x] `set_relay(channel: u8, state: bool)` — writes relay set/clr register with channel bitmask
- [x] `read_relay_state() -> u16` — reads current relay bitmask (if supported by HAT firmware)
- [x] Errors are returned as `Result<T, anyhow::Error>`

---

### Story 5: State-Caching Layer

**ID:** SEQGW-5 · **Points:** 3

**As a** developer,
**I want** a caching layer that tracks the last-written state of all outputs,
**so that** the gateway only issues I²C writes when a Modbus client actually changes a coil — preventing I²C bus saturation.

**Acceptance Criteria:**
- [x] `src/cache.rs` — `OutputCache` struct holding `[Option<bool>; 16]` for relays and `[Option<bool>; 4]` for OD outputs
- [x] `should_update_relay(index, new_state) -> bool` returns `true` only if state differs from cached value
- [x] On successful I²C write, cache is updated via `confirm_relay()`/`confirm_od()`
- [x] On I²C write failure, cached value is cleared to `None` via `invalidate_relay()`/`invalidate_od()` so the next cycle retries
- [x] Behaviour matches current Python implementation (`current_states = [None] * 16`); 5 unit tests passing

---

### Story 6: Modbus TCP Server Integration

**ID:** SEQGW-6 · **Points:** 8

**As a** vPLC operator,
**I want** the Rust gateway to expose the same Modbus TCP memory map as the Python PoC,
**so that** my existing SCADA/PLC configuration works without changes.

**Acceptance Criteria:**
- [x] `src/modbus.rs` — custom Modbus TCP server with raw MBAP framing (lighter than `tokio-modbus`)
- [x] **Coils (R/W):** addresses 0–15 → 16-Relay board; addresses 16–19 → OD outputs 1–4
- [x] **Discrete Inputs (RO):** addresses 0–7 → Opto-Inputs 1–8
- [x] **Holding Registers (RO):** addresses 0–7 → 4-20 mA × 100; address 8 → PSU voltage × 100; addresses 10–13 → 0-10 V × 100
- [x] **Holding Register 15** → Opto bitmask (only when `--map-opto-to-reg` is enabled)
- [x] Server accepts connections from any Slave ID (unit ID)
- [x] Supports Modbus function codes: 0x01, 0x02, 0x03, 0x05, 0x0F
- [x] Concurrent client connections handled via `tokio` tasks

---

### Story 7: Main Poll Loop

**ID:** SEQGW-7 · **Points:** 5

**As a** vPLC operator,
**I want** the gateway to run a deterministic 10 Hz update loop,
**so that** sensor data is fresh and relay commands are applied within 100 ms.

**Acceptance Criteria:**
- [x] `src/main.rs` — `std::thread::sleep` drives 10 Hz loop on dedicated OS thread (Modbus async on tokio)
- [x] Each tick: read all inputs → update Modbus data bank → apply coil writes via cache
- [x] Loop timing is compensated (interval ticks, not sleep-after-work)
- [x] Graceful shutdown on `SIGINT`/`SIGTERM` via `tokio::signal` + `AtomicBool`
- [x] Root check on startup with warning log if `euid != 0` (Linux only, gated with `#[cfg(unix)]`)
- [x] Log I/O cycle duration per tick at `DEBUG` level

---

### Story 8: Heartbeat Logging

**ID:** SEQGW-8 · **Points:** 2

**As an** operator,
**I want** a periodic heartbeat log showing all pin states,
**so that** I can monitor the system at a glance from the console or journal.

**Acceptance Criteria:**
- [x] Every 5 seconds, log a block matching the Python format:
  ```
  --- SYSTEM HEARTBEAT ---
  POWER: 24.12V
  4-20mA (1-8) : [ 4.0  4.0  0.0 ...] mA
  0-10V  (1-4) : [ 0.0  0.0  0.0  0.0] V
  OPTO INPUTS  : 00000000 (Binary)
  RELAYS (1-16): 0000000000000000
  OD OUT (1-4) : 0000
  ------------------------
  ```
- [x] Uses `tracing::info!` macro
- [x] Interval is configurable via `--log-interval` (default 5 s)

---

### Story 9: CLI Interface

**ID:** SEQGW-9 · **Points:** 2

**As an** operator,
**I want** the same CLI flags as the Python version plus new ones for stack IDs,
**so that** I can configure the gateway at launch without editing code.

**Acceptance Criteria:**
- [x] `clap` derive-based CLI struct in `src/cli.rs`
- [x] `--host <IP>` (default `0.0.0.0`)
- [x] `--port <PORT>` (default `502`)
- [x] `--map-opto-to-reg` (flag, default off)
- [x] `--ind-stack <ID>` (default `1`) — MegaInd HAT stack level
- [x] `--relay-stack <ID>` (default `0`) — 16-Relay HAT stack level
- [x] `--log-interval <SECS>` (default `5`) — heartbeat period
- [x] `--help` and `--version` auto-generated

---

### Story 10: Cross-Compile & On-Device Validation

**ID:** SEQGW-10 · **Points:** 5

**As a** developer,
**I want** to cross-compile the binary and validate it on the Raspberry Pi,
**so that** I can confirm feature parity with the Python gateway before cutover.

**Acceptance Criteria:**
- [ ] Binary cross-compiles with `cross build --release --target aarch64-unknown-linux-gnu`
- [x] Binary runs on Raspberry Pi 4 (64-bit Raspberry Pi OS)
- [ ] Side-by-side test: run Python gateway and Rust gateway on same hardware, compare Modbus register reads from an external client
- [ ] All 8 × 4-20 mA channels read correctly (within ±0.1 mA of Python output)
- [ ] All 4 × 0-10 V channels read correctly
- [ ] Opto inputs match
- [x] All 16 relays toggle correctly via coil writes
- [ ] All 4 OD outputs toggle correctly
- [ ] Benchmark: full I/O cycle time logged and confirmed < 1 ms

---

## Phase 1 — Production Readiness

### Story 11: Systemd Service Unit

**ID:** SEQGW-11 · **Points:** 2

**As an** operator,
**I want** a systemd unit file that starts the gateway on boot,
**so that** the Pi operates as a headless industrial appliance.

**Acceptance Criteria:**
- [x] `deploy/sequent-gateway.service` unit file
- [x] `Type=exec`, `Restart=on-failure`, `RestartSec=3`
- [x] `After=network-online.target`
- [x] `ExecStart=` points to installed binary with CLI flags from an `EnvironmentFile`
- [x] `deploy/sequent-gateway.env` example environment file
- [x] Install instructions in README

---

### Story 12: I²C Bus Hardware Reset

**ID:** SEQGW-12 · **Points:** 3

**As an** operator,
**I want** the gateway to detect a hung I²C bus and recover by toggling GPIO pins,
**so that** the system self-heals without a manual reboot.

**Acceptance Criteria:**
- [x] Detect hung bus: N consecutive I²C read failures within a window
- [x] Recovery: toggle SCL line via GPIO to clock out stuck slave (9-clock-pulse recovery)
- [x] Log recovery attempt at `WARN` level
- [x] Configurable failure threshold (`--i2c-reset-threshold`, default `10`)
- [x] After reset, re-open I²C device file descriptors

---

## Phase 2 — Protocol & Addressing

### Story 13: Multi-Slave Addressing

**ID:** SEQGW-13 · **Points:** 5

**As a** PLC programmer,
**I want** each board to have its own Modbus Slave ID,
**so that** I can address them independently in my PLC program.

**Acceptance Criteria:**
- [x] `--relay-slave-id <ID>` (default `1`)
- [x] `--ind-slave-id <ID>` (default `2`)
- [x] Modbus requests are routed by unit ID to the appropriate board's registers
- [x] Backward-compatible mode: `--single-slave` flag uses current flat mapping on Slave ID 1

---

### Story 14: Configurable Stack IDs

**ID:** SEQGW-14 · **Points:** 1

**As an** integrator with multiple HAT stacks,
**I want** CLI flags to set the I²C stack offset for each board,
**so that** I can address non-default stack positions.

**Acceptance Criteria:**
- [x] `--ind-stack` and `--relay-stack` flags (already included in Story 9)
- [x] Stack ID validated 0–7 at startup; exit with clear error if out of range
- [x] Logged at startup: "Industrial HAT at I²C 0x51, Relay HAT at I²C 0x29"

---

## Phase 3 — Observability & Reliability

### Story 15: Rotating File Logs

**ID:** SEQGW-15 · **Points:** 2

**As an** operator,
**I want** logs written to a rotating file alongside stdout,
**so that** I can diagnose issues after the fact.

**Acceptance Criteria:**
- [x] `--log-file <PATH>` flag (default: none / stdout only)
- [x] `tracing-appender` for daily or size-based rotation
- [x] Retain last 7 log files by default

---

### Story 16: Health Endpoint

**ID:** SEQGW-16 · **Points:** 3

**As a** monitoring system,
**I want** an HTTP JSON health endpoint,
**so that** I can poll gateway status from dashboards and alerting tools.

**Acceptance Criteria:**
- [x] `--health-port <PORT>` (default: disabled)
- [x] `GET /health` returns JSON: `{ "status": "ok", "uptime_s": 1234, "last_cycle_ms": 0.4, "i2c_errors": 0 }`
- [x] Lightweight: raw `tokio::net::TcpListener` — no heavy framework

---

### Story 17: I²C Watchdog Timer

**ID:** SEQGW-17 · **Points:** 3

**As an** operator,
**I want** the gateway to detect stalled I²C reads and recover,
**so that** a single sensor timeout doesn't block the entire poll loop.

**Acceptance Criteria:**
- [x] Per-channel read timeout (default 50 ms)
- [x] If a read exceeds the timeout, return last-known-good value and log at `WARN`
- [x] Consecutive timeout counter per channel; after N failures, mark channel as `FAULT` in heartbeat
- [x] Optionally tie into Story 12 (bus reset) if all channels fault simultaneously

---

## Phase 4 — Extended I/O

### Story 18: Analog Output Write-Back

**ID:** SEQGW-18 · **Points:** 5

**As a** PLC programmer,
**I want** to write 0-10 V and 4-20 mA output values via Modbus holding registers,
**so that** I can control analog outputs from my PLC program.

**Acceptance Criteria:**
- [x] Holding registers for 0-10 V outputs (4 channels) — writable (HR 16–19)
- [x] Holding registers for 4-20 mA outputs (4 channels) — writable (HR 20–23)
- [x] Values written as integer × 100 (matching input scaling convention)
- [x] State-cached (only write on change)
- [x] Register addresses documented in README memory map

---

### Story 19: Additional HAT Support

**ID:** SEQGW-19 · **Points:** 8

**As an** integrator,
**I want** the gateway to support other Sequent boards (8-relay, Building Automation, etc.),
**so that** I can use a single gateway binary for any Sequent hardware combination.

**Acceptance Criteria:**
- [x] HAL trait (`SequentBoard`) that `MegaIndBoard` and `RelayBoard` implement
- [x] New boards added by implementing the trait + adding a register map TOML
- [x] Board type selectable via CLI (`--board relay16 --board megaind --board relay8`)
- [x] At least one additional board type implemented as proof of extensibility (8-Relay HAT)

---

## Summary — Epic 1 (Complete)

| Phase | Stories | Total Points |
|---|---|---|
| **Phase 0** — Rust Port | SEQGW-1 through SEQGW-10 | 38 |
| **Phase 1** — Production Readiness | SEQGW-11, SEQGW-12 | 5 |
| **Phase 2** — Protocol & Addressing | SEQGW-13, SEQGW-14 | 6 |
| **Phase 3** — Observability & Reliability | SEQGW-15, SEQGW-16, SEQGW-17 | 8 |
| **Phase 4** — Extended I/O | SEQGW-18, SEQGW-19 | 13 |
| **Total** | **19 stories** | **70 points** |

---
---

## Epic 2: Operational Instrumentation & Dynamic HAL

**Epic ID:** SEQGW-EPIC-2

**Summary:** Wire the existing-but-unused public API surface into the runtime — adding I²C error counting to the health endpoint, relay read-back verification, and culminating in a trait-object-based poll loop that supports arbitrary board combinations without code changes.

**Business Value:** Epic 1 delivered a working gateway, but several reliability and extensibility features were built without being connected. This epic closes that gap: operators get real-time I²C health metrics in `/health`, relay state is verified against hardware, and integrators can deploy new board types by dropping in a TOML file — no recompilation required.

**Acceptance Criteria (Epic-level):**
- `/health` JSON reports live I²C error count and bus recovery count
- Channel watchdog faults are iterated generically (no hard-coded channel lists)
- Relay output state is periodically verified against hardware read-back
- Poll loop dispatches I/O through `dyn SequentBoard` trait objects
- A new board type can be added with only a TOML file and a HAL impl — no changes to `main.rs`

**See also:** [`sequent-gateway/FUTURE_API.md`](sequent-gateway/FUTURE_API.md) for code snippets and wiring details.

---

## Phase 5 — Health & Diagnostics Wiring

### Story 20: Wire I²C Error Counter into Health Endpoint

**ID:** SEQGW-20 · **Points:** 1

**As an** operator,
**I want** the `/health` JSON to report a live count of I²C errors,
**so that** I can monitor bus reliability from dashboards and set up alerts.

**Acceptance Criteria:**
- [x] Every `Err` branch in the poll loop's I²C reads calls `health_stats.inc_i2c_errors()`
- [x] Every `Err` branch in the poll loop's I²C writes calls `health_stats.inc_i2c_errors()`
- [x] `GET /health` JSON field `i2c_errors` reflects the cumulative count
- [x] `status` field degrades to `"degraded"` when error count > 0 in the last cycle
- [x] Unit test: increment counter, verify JSON output includes updated value

---

### Story 21: Wire Recovery Count into Health Endpoint

**ID:** SEQGW-21 · **Points:** 1

**As an** operator,
**I want** the `/health` JSON to include the number of I²C bus recoveries,
**so that** I can see how often the bus is being reset without tailing logs.

**Acceptance Criteria:**
- [x] `I2cWatchdog::recovery_count()` value is included in `/health` JSON as `"i2c_recoveries"`
- [x] Heartbeat log includes recovery count when > 0
- [x] Unit test: verify JSON field is present and correct after simulated recoveries

---

### Story 22: Generic Channel Iteration via `Channel::ALL`

**ID:** SEQGW-22 · **Points:** 1

**As a** developer,
**I want** channel health checks to iterate `Channel::ALL` instead of hard-coding four channels,
**so that** adding a new channel type in the future doesn't require editing every loop.

**Acceptance Criteria:**
- [x] `HealthStats::update_channel_status()` uses `Channel::ALL` for its iteration
- [x] Heartbeat `log_heartbeat()` uses `Channel::ALL` where applicable
- [x] No remaining hard-coded `[Channel::Ma, Channel::Volt, Channel::Psu, Channel::Opto]` arrays outside of the `ALL` definition
- [x] Existing channel watchdog tests still pass (60+)

---

## Phase 6 — Hardware Verification

### Story 23: Periodic Relay Read-Back Verification

**ID:** SEQGW-23 · **Points:** 3

**As an** operator,
**I want** the gateway to periodically read back actual relay state from the HAT,
**so that** a stuck relay or I²C glitch is detected and logged rather than silently ignored.

**Acceptance Criteria:**
- [x] Every N-th poll tick (configurable, default every 10th = 1 Hz), call `relay_board.read_relay_state()`
- [x] Compare returned bitmask against `OutputCache` expected state
- [x] On mismatch: log at `WARN` with expected vs actual bitmask, increment a `relay_mismatch` counter
- [x] On mismatch: invalidate affected relay cache entries so next cycle re-writes them
- [x] `--relay-verify-interval <N>` CLI flag (default 10, 0 = disabled)
- [x] Mismatch counter exposed in `/health` JSON as `"relay_mismatches"`
- [x] Unit test: simulated mismatch triggers cache invalidation

---

### Story 24: Relay State Diagnostic Register

**ID:** SEQGW-24 · **Points:** 2

**As a** PLC programmer,
**I want** to read the actual hardware relay bitmask via a Modbus holding register,
**so that** my PLC can verify relay state independently of the coil writes.

**Acceptance Criteria:**
- [x] New read-only Holding Register (HR 24 or configurable) contains the last `read_relay_state()` bitmask
- [x] Updated at the same frequency as the verify interval (Story 23)
- [x] Documented in README memory map
- [x] Unit test: verify HR value matches simulated read-back

---

## Phase 7 — Dynamic Board Dispatch

### Story 25: I/O Methods on `SequentBoard` Trait

**ID:** SEQGW-25 · **Points:** 5

**As a** developer,
**I want** the `SequentBoard` trait to include I/O dispatch methods,
**so that** the poll loop can operate on boards generically without type-specific branching.

**Acceptance Criteria:**
- [ ] `SequentBoard` trait gains: `fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()>`
- [ ] `SequentBoard` trait gains: `fn apply_outputs(&mut self, db: &DataBank, cache: &mut OutputCache) -> Result<()>`
- [ ] Default implementations return `Ok(())` (no-op for boards that don't support a capability)
- [ ] `MegaIndBoard` implements `poll_inputs` (reads analog + opto + voltage, writes to data bank)
- [ ] `RelayBoard` implements `apply_outputs` (reads coils from data bank, writes relays via cache)
- [ ] Both impls delegate to existing concrete methods (no logic duplication)
- [ ] Existing unit tests still pass; 2 new trait-dispatch integration tests

---

### Story 26: Board Registry & Dynamic Poll Loop

**ID:** SEQGW-26 · **Points:** 8

**As an** integrator,
**I want** the gateway to load boards from a registry and poll them generically,
**so that** I can add new hardware by dropping in a TOML file without recompiling.

**Acceptance Criteria:**
- [ ] `src/board_registry.rs` — `BoardRegistry` struct holding `Vec<Box<dyn SequentBoard>>`
- [ ] Boards constructed from `--board` flags + TOML definitions and pushed into the registry
- [ ] Poll loop iterates `registry.boards()` calling `poll_inputs()` and `apply_outputs()`
- [ ] Remove `use_megaind` / `use_relay16` / `use_relay8` boolean flags from `main.rs`
- [ ] Startup log lists all registered boards with name, stack ID, and capabilities
- [ ] At least 2 boards registered and working end-to-end in tests
- [ ] Backward compatible: same behaviour as today when using default `--board` flags

---

### Story 27: `stack_id()` Getters via Trait Objects

**ID:** SEQGW-27 · **Points:** 1

**As a** developer,
**I want** all startup and heartbeat logging to use `board.stack_id()` from trait objects,
**so that** stack IDs come from the actual board instance rather than duplicated CLI args.

**Acceptance Criteria:**
- [ ] Startup log uses `board.name()` and `board.stack_id()` from the registry
- [ ] Heartbeat log references board identity from trait objects
- [ ] `args.ind_stack` / `args.relay_stack` still used for board construction, but not for logging after init
- [ ] Remove `#[allow(dead_code)]` from `stack_id()` on both HAL structs

---

## Summary — Epic 2

| Phase | Stories | Total Points |
|---|---|---|
| **Phase 5** — Health & Diagnostics Wiring | SEQGW-20, SEQGW-21, SEQGW-22 | 3 |
| **Phase 6** — Hardware Verification | SEQGW-23, SEQGW-24 | 5 |
| **Phase 7** — Dynamic Board Dispatch | SEQGW-25, SEQGW-26, SEQGW-27 | 14 |
| **Total** | **8 stories** | **22 points** |
