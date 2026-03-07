# Experimental Board Definitions

> **⚠️  These TOML files are EXPERIMENTAL and UNTESTED on real hardware.**

They were generated from the register maps published in
[SequentMicrosystems](https://github.com/SequentMicrosystems) GitHub
repositories and should be close to correct, but they have **not** been
validated against physical boards.

## How to use

1. **Copy** the `.toml` file for your board into the main `boards/` directory
   (one level up from here).
2. Restart the gateway — it will pick up every `*.toml` file in `boards/`.
3. The gateway currently supports two protocols:
   - `sequent_mcu` — Sequent's custom STM32 MCU firmware (1-byte register
     address prefix, direct read/write).
   - `pca9535` — NXP PCA9535 I/O expander (read-modify-write on output port).

## Limitations

Many Sequent boards expose features (GPIO, thermistors, current sensors,
encoders, RTC, watchdog, RS-485/Modbus passthrough, etc.) that the gateway's
`RegisterMap` schema does **not** yet cover.  Where possible, these extra
registers are noted in TOML comments so they can be added in a future gateway
release.

Boards that use **both** a PCA9535 (for relays) **and** a Sequent MCU (for
analog I/O) simultaneously are noted as "dual-protocol".  The gateway can
only use one protocol per board definition today.  Two separate TOML files
(one per protocol) may be needed.

## Register sources

Each file includes a link to the upstream C header or Python library it was
derived from.  When in doubt, cross-reference that source.
