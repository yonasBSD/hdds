# XCDR2 Specification Divergence -- Phase 0 Investigation Report

- **Verdict:** BUG CONFIRME (spec-level).
- **Date:** 2026-04-20.
- **Pilot WIP:** `../../../../../WIP-XCDR1-INTEROP.md` (rev. 3).
- **Next gate:** Olivier to validate this report before Phase 1.

## Executive summary

HDDS's `encode_cdr2_le()` does not implement XCDR v2 as defined by the OMG
DDS-XTypes v1.3 specification (formal/2020-02-04). The codegen in `hdds_gen`
emits alignment code that aligns 8-byte primitives (`int64`, `uint64`,
`double`, `long double`) on 8-byte boundaries. That is the XCDR v1 rule.

Per XTypes v1.3 Section 7.4.2, PLAIN_CDR2 requires those same types to be
aligned on 4. HDDS therefore ships XCDR v1-shaped payloads under an
XCDR v2 encapsulation header.

Scope of the bug:

- Invisible in OMG ShapeType interop tests (all ShapeType members are 4-byte).
- Exercised immediately on any realistic IDL containing a `double` or `int64`.

Recommended follow-up: proceed with Phase 1 Scenario A (breaking fix) per the
WIP DECISIONS OLIVIER D1 (no production adopters to preserve).

## Four independent lines of evidence

### 1. Normative spec citation

Reference: OMG DDS-XTypes v1.3, formal/2020-02-04 (standard document URL
`https://www.omg.org/spec/DDS-XTypes/1.3/`).

#### Section 7.4.1.1.1 "Primitive types", Table 31 (doc page 122)

| Primitive Type | Encoded Size | Alignment (version 1) |
| -------------- | ------------ | --------------------- |
| Int64, UInt64  | 8            | **8**                 |
| Float64        | 8            | **8**                 |
| Float128       | 16           | **8**                 |

This is the version-1 table. No version-2 equivalent table is provided;
instead version-2 is defined by textual delta in Section 7.4.2.

#### Section 7.4.2 "Extended CDR Representation (encoding version 2)", doc page 129

> PLAIN_CDR2 shall be used for all primitive, strings, and enumerated types.
> It is also used for any type with extensibility kind FINAL. The encoding is
> similar to PLAIN_CDR **except that INT64, UINT64, FLOAT64, and FLOAT128 are
> serialized into the CDR buffer at offsets that are aligned to 4 rather than
> 8 as was the case in PLAIN_CDR.**

#### Section 7.4.3.2 "XCDR Stream State", `maxalign` variable (doc page 130)

> Integer state variable representing the maximum value for the alignment that
> will be used for future objects serialized into the stream. This value
> overrides the required alignment for the object being serialized, so the
> alignment condition for any object O of type O.type becomes:
>
>     ((XCDR.offset - XCDR.origin) % MALIGN(O)) == 0
>
> Where MALIGN(O) = MIN(O.type.alignment, XCDR.maxalign)

#### Section 7.4.3.2.2, `MAXALIGN` operation, Table 37 (doc page 132)

    MAXALIGN(VERSION2) = 4
    MAXALIGN(VERSION1) = 8
    MAXALIGN(VERSION_NONE) = 8

Combined, these three quotes define the XCDR v2 alignment rule as
`MIN(type_natural_size, 4)`. For `double` that is `MIN(8, 4) = 4`.

### 2. HDDS source code

`projects/public/hdds_gen/src/codegen/rust_backend/helpers.rs:211-242`:

```rust
/// Calculate CDR2 alignment for a given IDL type
pub(super) fn cdr2_alignment(idl_type: &IdlType) -> usize {
    match idl_type {
        IdlType::Primitive(p) => match p {
            // ...
            PrimitiveType::LongLong
            | PrimitiveType::UnsignedLongLong
            | PrimitiveType::Int64
            | PrimitiveType::UInt64
            | PrimitiveType::Double
            | PrimitiveType::LongDouble => 8,  // spec says 4 for XCDR2
        },
        // ...
    }
}
```

The function returns 8 for all 8-byte primitives, unconditionally, with no
branch on the encoding version. Its name is a misnomer: the behaviour is
XCDR v1 alignment, not XCDR v2.

Hddsgen propagates this value as a literal inside the emitted encoder, for
example (captured from `hddsgen gen rust Probe.idl`, hddsgen v1.0.10):

```rust
// Align to 8-byte boundary for field 'b'
let padding = (8 - (offset % 8)) % 8;
offset += padding;
```

### 3. Empirical HDDS output

Test: `crates/hdds/tests/xcdr_spec_divergence.rs`.

Input IDL:

    @final
    struct Probe {
        octet a;
        double b;
    };

Input values: `a = 0x42`, `b = 1.0`.

| Pattern                         | Bytes (hex)                                         | Size |
| ------------------------------- | --------------------------------------------------- | ---- |
| HDDS `encode_cdr2_le()` output  | `42 00 00 00 00 00 00 00  00 00 00 00 00 00 F0 3F`  | 16   |
| XCDR v1 reference (align 8-on-8)| `42 00 00 00 00 00 00 00  00 00 00 00 00 00 F0 3F`  | 16   |
| XCDR v2 reference (align 8-on-4)| `42 00 00 00  00 00 00 00 00 00 F0 3F`              | 12   |

HDDS bytes are byte-identical to the XCDR v1 reference and 4 bytes longer
than the XCDR v2 reference. The extra 4 bytes are the unnecessary padding
between the `octet` and the `double`.

### 4. Cross-reference: Fast-CDR reference implementation

eProsima's Fast-CDR library (the CDR engine used by FastDDS) sets its internal
`align64_` state variable to 4 when the stream is initialised as
`CdrVersion::XCDRv2`, and to 8 for earlier CDR versions. Fast-CDR uses that
value when serializing 64-bit primitives, so 64-bit values in XCDR v2 land on
4-byte boundaries and not on 8-byte boundaries.

Source file: `github.com/eProsima/Fast-CDR/blob/master/src/cpp/Cdr.cpp`.
Verified via WebFetch 2026-04-20.

## Why the bug is invisible in OMG CI today

The OMG `ShapeType` IDL used by the `dds-rtps` interop test suite contains
exclusively 4-byte primitive members: `color` is a `uint32`, `x`, `y`, and
`shapesize` are `int32`. For 4-byte types, the XCDR v1 alignment rule (4) and
the XCDR v2 alignment rule (`MIN(4, 4) = 4`) coincide. ShapeType never
exercises the 8-byte alignment branch that carries the bug.

This explains why HDDS v1.1.1 was able to pass 48/48 against Connext 7.6 with
`-x 2` (as reported by Angel Martinez, RTI, on 2026-04-10): the payload path
that would diverge from the spec is simply never executed by the OMG suite.
Any realistic IDL with a `double` or `int64` field would immediately fail.

## Runtime-level dispatch gap (separate but related)

Fixing `hdds_gen::cdr2_alignment()` alone will not change the wire bytes
emitted by a HDDS DataWriter. Two sites in the runtime call `encode_cdr2`
unconditionally, with no branch on the QoS-negotiated `data_representation`:

- `crates/hdds/src/dds/writer/runtime.rs:295`
- `crates/hdds/src/dds/writer/runtime.rs:612`

Any future XCDR v1 encoding path added by `hdds_gen` would therefore be
compiled but never called at runtime. See WIP Phase 2 Etape 2.5 for the
mitigation plan.

Bonus observation from the investigation: the function
`write_data_representation_both` at
`crates/hdds/src/protocol/discovery/sedp/build/metadata.rs:264` is compiled
but never called (dead-code warning visible when running
`cargo test xcdr_spec_divergence`). Infrastructure to advertise both XCDR
versions in SEDP exists but is not wired.

## Verdict

**BUG CONFIRME.** All four lines of evidence agree:

1. The OMG spec is unambiguous (three distinct citations in Section 7.4.2 and
   Section 7.4.3.2).
2. The reference implementation (Fast-CDR) matches the spec.
3. HDDS's `cdr2_alignment()` function returns the XCDR v1 value.
4. Empirical HDDS output shows 16 bytes where XCDR v2 demands 12.

## Recommended Phase 1 outcome

Scenario A per WIP (breaking fix, no legacy flag, D1 locked by Olivier):

- Rename `cdr2_alignment()` to `xcdr1_alignment()` -- it factually is that.
- Add a correct `xcdr2_alignment()` that caps at 4 for 8-byte primitives.
- Route `hdds_gen` codegen on the target `@data_representation`.
- Fix the two writer-runtime hardcodes so the negotiated encoding is honoured.
- Bump the HDDS version; document BREAKING CHANGE in CHANGELOG; no migration
  flag.

## Items intentionally deferred from this report

The WIP Phase 0.2 and 0.3 steps planned FastDDS and Connext 6.x/7.x
publisher captures for `Probe{octet;double}`, producing vendor wire-byte
hex dumps. Those captures are **not required to reach the Phase 0 verdict**:
the spec citation and the Fast-CDR source cross-reference are already
authoritative. They remain valuable later as reference material for
`tests/golden/xcdr1/` and `tests/golden/xcdr2/` when Phase 3 builds those
vector sets.

Decision requested from Olivier:

- (a) Skip vendor captures now, move straight to Phase 1 decision and
  Phase 2 implementation, and produce the captures as part of Phase 3.
- (b) Run FastDDS + Connext 6.x + 7.x captures now (~half a day of work
  writing minimal publishers against `Probe.idl`) and amend this report.

Either is fine. My pragmatic preference is (a): the bug is proven and the
vendor captures are documentation value, not verdict-deciding.

## Artifacts from Phase 0

- `crates/hdds/tests/xcdr_spec_divergence.rs` -- differential probe test.
- `crates/hdds/tests/golden/xcdr/INVESTIGATION.md` -- this file.
- `hddsgen gen rust` output for the probe IDL -- reproducible by running
  the command above (version string `hddsgen v1.0.10` was used).

## Stop

No further Phase 1 action is taken in this branch. Awaiting Olivier's
validation of this report and selection between options (a) and (b).
