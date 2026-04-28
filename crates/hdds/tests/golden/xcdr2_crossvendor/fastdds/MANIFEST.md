# Fast DDS XCDR2 Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
eProsima Fast DDS for XCDR2 interop reference.

## Captured versions

| Software                              | Version       |
|---------------------------------------|---------------|
| Fast DDS runtime                      | 3.1.2         |
| Fast CDR                              | 2.2.5         |
| fastddsgen                            | 4.0.4+dfsg    |

## Captures

Extensibility is `@appendable` (XTypes default) for all captures.
All captures use encapsulation `D_CDR2_LE (0x0009)` with a 4-byte
DHEADER (body length = 12) preceding the CDR body. The `.bin` files
contain only the 12-byte body (encap header and DHEADER stripped);
the `.hex` files include the full 20-byte wire sequence.

| File                                 | Case     | `a` | `b`             | Body (12 bytes, LE)                                |
|--------------------------------------|----------|-----|-----------------|----------------------------------------------------|
| probe_nominal.bin / probe_nominal.hex | nominal  | 0x42 | 1.0             | `42 00 00 00 00 00 00 00 00 00 f0 3f`              |
| probe_nan.bin / probe_nan.hex         | nan      | 0x42 | qNaN            | `42 00 00 00 00 00 00 00 00 00 f8 7f`              |
| probe_neg_zero.bin / probe_neg_zero.hex | neg_zero | 0x42 | -0.0            | `42 00 00 00 00 00 00 00 00 00 00 80`              |

## Body layout (XCDR2, alignment cap at 4)

Per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15, XCDR2 caps alignment at 4
bytes, so `b` aligns at offset 4 (not 8 as in XCDR1):

- offset 0: `a` (octet)
- offset 1..3: 3 bytes of padding
- offset 4..11: `b` (double, little-endian)

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15 — alignment rules (XCDR2 cap at 4).
- DDS-XTypes v1.3 Sec.7.4.3.4.3 — DHEADER for @appendable in XCDR2.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
