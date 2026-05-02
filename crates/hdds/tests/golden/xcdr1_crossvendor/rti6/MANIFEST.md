# RTI Connext 6.x XCDR1 Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
RTI Connext DDS 6.x for XCDR1 (legacy CDR) interop reference.

## Captured versions

| Software                    | Version                                    |
|-----------------------------|--------------------------------------------|
| RTI Connext DDS             | 6.1.0                                      |
| rtiddsgen                   | 3.1.0 (templates 4A3A-C8AC-D250-9F76-...)  |
| Architecture                | x64Linux4gcc7.3.0                          |

## Captures

Extensibility is `@appendable` (XTypes default) for all captures.

**Observed:** RTI Connext 6.1.0 emits `@appendable` XCDR1 as plain
CDR v1 (encapsulation kind `PLAIN_CDR_LE`, 0x0001) with **no**
DHEADER, identical to Fast DDS 3.1.2. The body layout follows natural
alignment per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16.

The `.bin` files contain the 16-byte body (encap header stripped).
The `.hex` files include the full 20-byte wire sequence.

| File                                    | Case     | `a`  | `b`             | Body (16 bytes, LE)                                                   |
|-----------------------------------------|----------|------|-----------------|-----------------------------------------------------------------------|
| probe_nominal.bin / probe_nominal.hex   | nominal  | 0x42 | 1.0             | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 f0 3f`                     |
| probe_nan.bin / probe_nan.hex           | nan      | 0x42 | qNaN            | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 f8 7f`                     |
| probe_neg_zero.bin / probe_neg_zero.hex | neg_zero | 0x42 | -0.0            | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 00 80`                     |

## Body layout (XCDR1, natural alignment)

Per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16, XCDR1 uses natural
alignment, so `b` aligns at offset 8:

- offset 0: `a` (octet)
- offset 1..7: 7 bytes of padding
- offset 8..15: `b` (double, little-endian)

## Cross-vendor diff vs Fast DDS 3.1.2

All 3 cases (nominal, nan, neg_zero) are **byte-identical** to the
Fast DDS captures under `xcdr1_crossvendor/fastdds/`. The
encapsulation kind, body bytes, padding values, and NaN / neg_zero
bit patterns match exactly. RTI Connext 6.1.0 and Fast DDS 3.1.2 are
in full agreement on the XCDR1 wire format for `@appendable`
primitive-mix structs.

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16 — natural alignment rules.
- DDS-XTypes v1.3 Sec.7.4.3.4.2 — `@appendable` XCDR1 has no DHEADER.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
