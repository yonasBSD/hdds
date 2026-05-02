# RTI Connext 6.x XCDR2 Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
RTI Connext DDS 6.x for XCDR2 interop reference.

## Captured versions

| Software                    | Version                                    |
|-----------------------------|--------------------------------------------|
| RTI Connext DDS             | 6.1.0                                      |
| rtiddsgen                   | 3.1.0 (templates 4A3A-C8AC-D250-9F76-...)  |
| Architecture                | x64Linux4gcc7.3.0                          |

## Captures

Extensibility is `@appendable` (XTypes default) for all captures.
All captures use encapsulation `D_CDR2_LE (0x0009)` with a 4-byte
DHEADER (body length = 12) preceding the CDR body. The `.bin` files
contain only the 12-byte body (encap header and DHEADER stripped);
the `.hex` files include the full 20-byte wire sequence.

| File                                    | Case     | `a`  | `b`             | Body (12 bytes, LE)                                |
|-----------------------------------------|----------|------|-----------------|----------------------------------------------------|
| probe_nominal.bin / probe_nominal.hex   | nominal  | 0x42 | 1.0             | `42 00 00 00 00 00 00 00 00 00 f0 3f`              |
| probe_nan.bin / probe_nan.hex           | nan      | 0x42 | qNaN            | `42 00 00 00 00 00 00 00 00 00 f8 7f`              |
| probe_neg_zero.bin / probe_neg_zero.hex | neg_zero | 0x42 | -0.0            | `42 00 00 00 00 00 00 00 00 00 00 80`              |

## Body layout (XCDR2, alignment cap at 4)

Per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15, XCDR2 caps alignment at 4
bytes, so `b` aligns at offset 4 (not 8 as in XCDR1):

- offset 0: `a` (octet)
- offset 1..3: 3 bytes of padding
- offset 4..11: `b` (double, little-endian)

## Cross-vendor diff vs Fast DDS 3.1.2

All 3 cases (nominal, nan, neg_zero) are **byte-identical** to the
Fast DDS captures under `xcdr2_crossvendor/fastdds/`. The
encapsulation kind (`D_CDR2_LE` 0x0009), DHEADER (`0x0000000c`),
body bytes, padding values, and IEEE 754 bit patterns for NaN and
neg_zero match exactly between the two vendors.

RTI Connext 6.1.0 and Fast DDS 3.1.2 agree on:
- The choice of `D_CDR2_LE` for `@appendable` XCDR2 over either
  `PLAIN_CDR2_LE` (0x0007) or `PL_CDR2_LE` (0x000B).
- The 4-byte DHEADER layout per DDS-XTypes v1.3 Sec.7.4.3.4.3.
- The 4-byte alignment cap on the `double` field per Table 15.
- Padding bytes are zeroed.

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15 — alignment rules (XCDR2 cap at 4).
- DDS-XTypes v1.3 Sec.7.4.3.4.3 — DHEADER for `@appendable` in XCDR2.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
