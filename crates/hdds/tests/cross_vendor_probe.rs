// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Cross-vendor empirical anchor for F01 cdr2_alignment systemic fix.
//!
//! The 18 wire captures under
//! `tests/golden/{xcdr1,xcdr2}_crossvendor/{fastdds,rti6,rti7}/probe_*.bin`
//! are byte-level outputs of `@final struct Probe { octet a; double b; }`
//! emitted by Fast DDS 3.x, RTI Connext 6.x, and RTI Connext 7.x for
//! the same logical IDL type. Each `.bin` file contains the 12-byte
//! body (XCDR2: encap header + 4-byte DHEADER stripped) or 16-byte
//! body (XCDR1: encap header stripped, 8-byte alignment cap).
//!
//! Per OMG DDS-XTypes v1.3 §7.4.3.4.1 Tab.15 the alignment cap for
//! 8-byte primitives in XCDR2 is 4 bytes (vs 8 in XCDR1). HDDS' encoder
//! historically emitted 8-byte alignment for both, breaking cross-vendor
//! interop on the XCDR2 wire. The fix lives in the F01 / 1.6.1
//! `encode_cdr2_le_at` migration; this file gates progress empirically.
//!
//! ## Status (1.6.1d-unignore-cross-vendor, 2026-05-10)
//!
//! The 18 tests are LIVE — `#[ignore]` markers removed after the
//! migration chain (impls-macro-primitives, impls-lib, codegen-rust,
//! codegen-encode, sdk-samples-regen) landed and the spec-compliant
//! XCDR2 alignment is active end-to-end. The tests validate F01
//! (cdr2_alignment systemic) + F04 (macro alignment) byte-for-byte
//! against cross-vendor golden hex sequences for user-struct types
//! (`struct Probe { octet a; double b }`).
//!
//! The independently-tracked F29 (DHEADER missing for
//! @extensibility(APPENDABLE) `MinimalTypeObject` / `CompleteTypeObject`
//! containers) does NOT affect these tests: Probe is a regular user
//! data type, not a TypeObject, so the spec framing required for
//! cross-vendor `EquivalenceHash` matching is out of scope here.
//!
//! ## Canonical-trait design (post-1.7c)
//!
//! `Probe` implements `hdds::Cdr2Encode` directly. The XCDR2 path
//! (`encode_cdr2_le_at` / `encode_xcdr2_le_at`) delegates to the
//! primitive impls in `core::ser::traits`, so the spec-compliant
//! 8-byte-cap-4 alignment is gated end-to-end here: a regression in
//! `impl_cdr2_primitive!` will surface as failures across the 9 XCDR2
//! goldens. The XCDR1 path overrides `encode_xcdr1_le_at` to emit the
//! strict 8-byte alignment that vendors use on the XCDR1 wire.

use hdds::{Cdr2Encode, CdrError};

#[derive(Debug, Clone, PartialEq)]
struct Probe {
    a: u8,
    b: f64,
}

impl Cdr2Encode for Probe {
    fn max_cdr2_size(&self) -> usize {
        1 + 3 + 8
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.a.encode_cdr2_le_at(dst, offset)?;
        self.b.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }

    fn encode_xcdr1_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        if *offset + 1 > dst.len() {
            return Err(CdrError::BufferTooSmall);
        }
        dst[*offset] = self.a;
        *offset += 1;

        let pad = (8 - (*offset % 8)) % 8;
        if *offset + pad + 8 > dst.len() {
            return Err(CdrError::BufferTooSmall);
        }
        dst[*offset..*offset + pad].fill(0);
        *offset += pad;

        dst[*offset..*offset + 8].copy_from_slice(&self.b.to_le_bytes());
        *offset += 8;
        Ok(())
    }
}

/// Common assert helper: encode `probe` via the requested CDR variant
/// and compare against the golden bytes.
fn assert_probe_matches_golden(probe: Probe, golden: &[u8], cdr_v2: bool) {
    let mut buf = vec![0u8; 64];
    let mut offset = 0;
    if cdr_v2 {
        probe
            .encode_xcdr2_le_at(&mut buf, &mut offset)
            .expect("encode XCDR2");
    } else {
        probe
            .encode_xcdr1_le_at(&mut buf, &mut offset)
            .expect("encode XCDR1");
    }
    assert_eq!(
        &buf[..offset],
        golden,
        "wire bytes diverge from vendor golden\nexpected: {:02x?}\nactual:   {:02x?}",
        golden,
        &buf[..offset]
    );
}

// ============================================================================
// XCDR2 — Fast DDS 3.x captures
// ============================================================================

const FASTDDS_X2_NOMINAL: &[u8] =
    include_bytes!("golden/xcdr2_crossvendor/fastdds/probe_nominal.bin");
const FASTDDS_X2_NAN: &[u8] = include_bytes!("golden/xcdr2_crossvendor/fastdds/probe_nan.bin");
const FASTDDS_X2_NEG_ZERO: &[u8] =
    include_bytes!("golden/xcdr2_crossvendor/fastdds/probe_neg_zero.bin");

#[test]

fn fastdds_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, FASTDDS_X2_NOMINAL, true);
}

#[test]

fn fastdds_xcdr2_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        FASTDDS_X2_NAN,
        true,
    );
}

#[test]

fn fastdds_xcdr2_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, FASTDDS_X2_NEG_ZERO, true);
}

// ============================================================================
// XCDR2 — RTI Connext 6.x captures
// ============================================================================

const RTI6_X2_NOMINAL: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti6/probe_nominal.bin");
const RTI6_X2_NAN: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti6/probe_nan.bin");
const RTI6_X2_NEG_ZERO: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti6/probe_neg_zero.bin");

#[test]

fn rti6_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI6_X2_NOMINAL, true);
}

#[test]

fn rti6_xcdr2_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        RTI6_X2_NAN,
        true,
    );
}

#[test]

fn rti6_xcdr2_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, RTI6_X2_NEG_ZERO, true);
}

// ============================================================================
// XCDR2 — RTI Connext 7.x captures
// ============================================================================

const RTI7_X2_NOMINAL: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti7/probe_nominal.bin");
const RTI7_X2_NAN: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti7/probe_nan.bin");
const RTI7_X2_NEG_ZERO: &[u8] = include_bytes!("golden/xcdr2_crossvendor/rti7/probe_neg_zero.bin");

#[test]

fn rti7_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI7_X2_NOMINAL, true);
}

#[test]

fn rti7_xcdr2_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        RTI7_X2_NAN,
        true,
    );
}

#[test]

fn rti7_xcdr2_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, RTI7_X2_NEG_ZERO, true);
}

// ============================================================================
// XCDR1 — Fast DDS 3.x captures (8-byte alignment, no cap)
// ============================================================================

const FASTDDS_X1_NOMINAL: &[u8] =
    include_bytes!("golden/xcdr1_crossvendor/fastdds/probe_nominal.bin");
const FASTDDS_X1_NAN: &[u8] = include_bytes!("golden/xcdr1_crossvendor/fastdds/probe_nan.bin");
const FASTDDS_X1_NEG_ZERO: &[u8] =
    include_bytes!("golden/xcdr1_crossvendor/fastdds/probe_neg_zero.bin");

#[test]

fn fastdds_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, FASTDDS_X1_NOMINAL, false);
}

#[test]

fn fastdds_xcdr1_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        FASTDDS_X1_NAN,
        false,
    );
}

#[test]

fn fastdds_xcdr1_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, FASTDDS_X1_NEG_ZERO, false);
}

// ============================================================================
// XCDR1 — RTI Connext 6.x captures
// ============================================================================

const RTI6_X1_NOMINAL: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti6/probe_nominal.bin");
const RTI6_X1_NAN: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti6/probe_nan.bin");
const RTI6_X1_NEG_ZERO: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti6/probe_neg_zero.bin");

#[test]

fn rti6_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI6_X1_NOMINAL, false);
}

#[test]

fn rti6_xcdr1_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        RTI6_X1_NAN,
        false,
    );
}

#[test]

fn rti6_xcdr1_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, RTI6_X1_NEG_ZERO, false);
}

// ============================================================================
// XCDR1 — RTI Connext 7.x captures
// ============================================================================

const RTI7_X1_NOMINAL: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti7/probe_nominal.bin");
const RTI7_X1_NAN: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti7/probe_nan.bin");
const RTI7_X1_NEG_ZERO: &[u8] = include_bytes!("golden/xcdr1_crossvendor/rti7/probe_neg_zero.bin");

#[test]

fn rti7_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI7_X1_NOMINAL, false);
}

#[test]

fn rti7_xcdr1_nan() {
    assert_probe_matches_golden(
        Probe {
            a: 0x42,
            b: f64::NAN,
        },
        RTI7_X1_NAN,
        false,
    );
}

#[test]

fn rti7_xcdr1_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, RTI7_X1_NEG_ZERO, false);
}
