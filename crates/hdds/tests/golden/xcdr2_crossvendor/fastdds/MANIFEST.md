# FastDDS XCDR Cross-Vendor Captures

Wire-level byte captures of `Probe { octet a; double b; }` produced by
eProsima Fast DDS for XCDR interop reference.

## Captured versions

| Software                              | Version       |
|---------------------------------------|---------------|
| Fast DDS runtime                      | 3.1.2         |
| Fast CDR                              | 2.2.5         |
| fastddsgen                            | 4.0.4+dfsg    |
| hddsgen (for subscriber self-check)   | 1.0.12        |

## Captures

| File                               | XCDR version | Case     | Extensibility            | Encap ID           | Body size |
|------------------------------------|--------------|----------|--------------------------|--------------------|-----------|
| probe_nominal.bin / probe_nominal.hex | XCDR2 LE  | nominal  | @appendable (default)    | 0x0009 (D_CDR2_LE) | 12 bytes  |

## Spec references

- DDS-XTypes v1.3 Sec.7.4.3.4.1 Table 15 — alignment rules (XCDR2 cap at 4).
- DDS-XTypes v1.3 Sec.7.4.3.4.3 — DHEADER for @appendable in XCDR2.
- DDS-XTypes v1.3 Sec.7.6.2.1.2 — representation IDs.
- DDS-RTPS 2.5 Sec.10.7 — encapsulation header layout.
