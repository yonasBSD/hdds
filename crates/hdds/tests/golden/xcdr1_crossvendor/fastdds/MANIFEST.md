# Fast DDS XCDR1 Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
eProsima Fast DDS for XCDR1 (legacy CDR) interop reference.

## Captured versions

| Software                              | Version       |
|---------------------------------------|---------------|
| Fast DDS runtime                      | 3.1.2         |
| Fast CDR                              | 2.2.5         |
| fastddsgen                            | 4.0.4+dfsg    |

## Captures

Extensibility is `@appendable` (XTypes default) for all captures.

**Observed:** Fast DDS 3.1.2 emits `@appendable` XCDR1 as plain CDR v1
(encapsulation kind `PLAIN_CDR_LE`, 0x0001) with **no** DHEADER. The
body layout is identical to the `@final` case under XCDR1, following
natural alignment per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16. The
DHEADER mechanism from Sec.7.4.3.4.3 applies to XCDR2 only; XCDR1 has
no standardised framing for `@appendable` types.

The `.bin` files contain the 16-byte body (encap header stripped).
The `.hex` files include the full 20-byte wire sequence.

| File                                 | Case     | `a` | `b`             | Body (16 bytes, LE)                                                   |
|--------------------------------------|----------|-----|-----------------|-----------------------------------------------------------------------|
| probe_nominal.bin / probe_nominal.hex | nominal  | 0x42 | 1.0             | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 f0 3f`                     |
| probe_nan.bin / probe_nan.hex         | nan      | 0x42 | qNaN            | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 f8 7f`                     |
| probe_neg_zero.bin / probe_neg_zero.hex | neg_zero | 0x42 | -0.0            | `42 00 00 00 00 00 00 00 00 00 00 00 00 00 00 80`                     |

## Body layout (XCDR1, natural alignment)

Per DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16, XCDR1 uses natural alignment,
so `b` aligns at offset 8:

- offset 0: `a` (octet)
- offset 1..7: 7 bytes of padding
- offset 8..15: `b` (double, little-endian)

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 16 — natural alignment rules.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
