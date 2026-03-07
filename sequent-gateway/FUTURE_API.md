# Future API Surface

> These public methods and traits are implemented, tested, and
> suppressed with `#[allow(dead_code)]`. They exist as extension
> points for future work but are **not wired into the runtime today**
> (unless marked ✅ below).

---

## 1. `SequentBoard` Trait & `BoardCapability` Enum ✅ Wired (SEQGW-25, SEQGW-26)

**Files:** `src/hal/traits.rs`

### What it does

```rust
pub trait SequentBoard: Send {
    fn name(&self) -> &str;
    fn stack_id(&self) -> u8;
    fn capabilities(&self) -> &'static [BoardCapability];
    fn relay_count(&self) -> usize { 0 }
    fn has_capability(&self, cap: BoardCapability) -> bool;
    fn poll_inputs(&mut self, db: &mut DataBank) -> Result<()> { Ok(()) }
    fn apply_outputs(&mut self, db: &DataBank, cache: &mut OutputCache) -> Result<()> { Ok(()) }
}
```

Both `MegaIndBoard` and `RelayBoard` implement this trait (including
Windows stubs). `MegaIndBoard` overrides `poll_inputs` (reads all
analog + opto + voltage inputs) and `apply_outputs` (OD + analog
outputs). `RelayBoard` overrides `apply_outputs` (relay coils).

### How to use it

**Dynamic dispatch poll loop** — replace the current concrete-typed
poll loop with a `Vec<Box<dyn SequentBoard>>`:

```rust
let boards: Vec<Box<dyn SequentBoard>> = vec![
    Box::new(megaind),
    Box::new(relay_board),
];

for board in &mut boards {
    board.poll_inputs(&mut db)?;
    board.apply_outputs(&db, &mut cache)?;
}
```

This would let the gateway load an arbitrary number of boards from
TOML config and iterate them generically, instead of hard-coding
`use_megaind` / `use_relay16` / `use_relay8` branches in `main.rs`.

**Next step:** SEQGW-27 (stack_id Getters) will use `board.stack_id()`
from trait objects for all startup and heartbeat logging.

---

## 2. `Channel::ALL` ✅ Wired (SEQGW-22)

**File:** `src/channel_watchdog.rs`

### What it does

```rust
pub const ALL: [Channel; 4] = [Channel::Ma, Channel::Volt, Channel::Psu, Channel::Opto];
```

### How to use it

Iterate all channels for bulk diagnostics or health snapshots:

```rust
for ch in Channel::ALL {
    if ch_wd.is_faulted(ch) {
        warn!("{} channel is FAULTED", ch.label());
    }
}
```

Useful when wiring `ChannelWatchdog` status into the health endpoint
or a Modbus diagnostic register block.

---

## 3. `read_relay_state()` ✅ Wired (SEQGW-23)

**File:** `src/hal/relay16.rs`

### What it does

Reads the PCA9535 output port register back and un-remaps it to
logical relay order, returning a `u16` bitmask.

### How to use it

**Read-back verification** — after writing a relay, read back the
actual hardware state and compare to the cached value:

```rust
let actual = relay_board.read_relay_state()?;
let expected = cache.relay_bitmask();
if actual != expected {
    warn!("Relay state mismatch: expected 0x{expected:04X}, got 0x{actual:04X}");
    cache.invalidate_all_relays();
}
```

Could also be exposed as a Modbus read-only register for diagnostics.

---

## 4. `stack_id()` on `MegaIndBoard` / `RelayBoard`

**Files:** `src/hal/megaind.rs`, `src/hal/relay16.rs`

### What it does

Returns the I²C stack offset (0–7) that was passed at construction.

### How to use it

Currently `main.rs` logs stack IDs from the CLI args. These getters
would matter if boards were stored as trait objects — you'd need
`board.stack_id()` instead of `args.ind_stack`:

```rust
info!("Board {} at stack {}", board.name(), board.stack_id());
```

---

## 5. `HealthStats::inc_i2c_errors()` ✅ Wired (SEQGW-20)

**File:** `src/health.rs`

### What it does

Atomically increments a cumulative I²C error counter in the
lock-free `HealthStats` struct shared between the poll thread and
the HTTP health handler.

### How to use it

Call it from the poll loop whenever an I²C operation fails:

```rust
match ind_board.read_4_20ma_inputs() {
    Ok(vals) => { /* update data bank */ }
    Err(e) => {
        warn!("4-20mA read failed: {e:#}");
        health_stats.inc_i2c_errors();   // ← wire this in
    }
}
```

The `/health` JSON already serialises this field — once wired, the
error count shows up automatically:

```json
{"status":"degraded","i2c_errors":42, ...}
```

---

## 6. `I2cWatchdog::recovery_count()` ✅ Wired (SEQGW-21)

**File:** `src/i2c_recovery.rs`

### What it does

Returns the total number of GPIO-level bus recoveries performed
since process start.

### How to use it

Expose in the health endpoint or heartbeat log:

```rust
info!("I²C recoveries: {}", i2c_wd.recovery_count());
```

Or include in the `/health` JSON payload:

```rust
format!(r#""i2c_recoveries":{}"#, i2c_wd.recovery_count())
```

---

## Wiring Checklist

If you decide to activate any of these, here's the rough order:

1. ~~**`inc_i2c_errors()`**~~ ✅ Done — wired into all 8 `Err` branches
   in the poll loop. Visible in `/health` JSON. (SEQGW-20)
2. ~~**`Channel::ALL`**~~ ✅ Done — used in `HealthStats::update_channel_status()`
   instead of hard-coding the four channels. (SEQGW-22)
3. ~~**`recovery_count()`**~~ ✅ Done — added to health JSON and heartbeat log. (SEQGW-21)
4. ~~**`read_relay_state()`**~~ ✅ Done — periodic read-back verification
   every N-th tick, mismatches invalidate cache. HR 24 diagnostic register. (SEQGW-23/24)
5. **`S