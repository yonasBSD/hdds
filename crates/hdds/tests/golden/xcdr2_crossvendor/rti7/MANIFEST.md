# RTI Connext 7.x XCDR2 Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
RTI Connext DDS 7.3.0 for XCDR2 interop reference.

## Captured versions

| Software                    | Version                                    |
|-----------------------------|--------------------------------------------|
| RTI Connext DDS             | 7.3.0                                      |
| rtiddsgen                   | 4.3.0 (templates 07EE-8F52-CE53-CCB0-...)  |
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

## Cross-vendor diff (3-way)

All 3 cases (nominal, nan, neg_zero) are **byte-identical** to both
Fast DDS 3.1.2 (`xcdr2_crossvendor/fastdds/`) and RTI Connext 6.1.0
(`xcdr2_crossvendor/rti6/`).

The three vendors agree on:
- `D_CDR2_LE` (0x0009) for `@appendable` XCDR2 (over `PLAIN_CDR2_LE`
  0x0007 or `PL_CDR2_LE` 0x000B).
- 4-byte DHEADER layout per DDS-XTypes v1.3 Sec.7.4.3.4.3, value
  `0x0000000c` for the 12-byte Probe body.
- 4-byte alignment cap on the `double` field per Table 15.
- Padding bytes are zeroed.
- IEEE 754 bit patterns: qNaN = `0x7FF8000000000000`,
  -0.0 = `0x8000000000000000`.

## Notes vs RTI Connext 6.1.0

- rtiddsgen bumped from 3.1.0 to 4.3.0; the new version parses
  `Probe.idl` without `-ppDisable`.
- QoS XML schema syntax for `<representation>` / `<element>` is
  unchanged between 6.1.0 and 7.3.0.
- Wire format output is bit-identical to 6.1.0 for `@appendable`
  XCDR2 (D_CDR2_LE, DHEADER, alignment, IEEE 754 patterns).

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15 — alignment rules (XCDR2 cap at 4).
- DDS-XTypes v1.3 Sec.7.4.3.4.3 — DHEADER for `@appendable` in XCDR2.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
