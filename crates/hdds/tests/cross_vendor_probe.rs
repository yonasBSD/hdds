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
//! ## Status (1.6.1a-empirical-anchor, 2026-05-09)
//!
//! All 18 tests are `#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]`
//! today. They become live in 1.6.1d-unignore-cross-vendor once the
//! migration chain (impls-macro-primitives + impls-lib + codegen-rust
//! + callers-internal) restores green compile and the spec-compliant
//! alignment is active end-to-end.
//!
//! ## Self-contained design
//!
//! The test fixture defines its own `Probe` struct + `encode_xcdr2_le_at`
//! / `encode_xcdr1_le_at` inherent methods so it does NOT depend on any
//! type pulled from the `hdds` crate's broken-during-migration lib build.
//! Once the migration completes, a follow-up may rewire the assertions
//! through the canonical `hdds::Cdr2Encode` trait.

#![allow(dead_code)] // ad-hoc fixture; helpers used inside #[ignore] tests

#[derive(Debug, Clone, PartialEq)]
struct Probe {
    a: u8,
    b: f64,
}

#[derive(Debug)]
enum LocalCdrError {
    BufferTooSmall,
}

impl Probe {
    /// Spec-compliant XCDR2 encoder for the Probe IDL type.
    /// 8-byte primitive (`f64`) aligns to `min(8, 4) = 4` per
    /// DDS-XTypes v1.3 §7.4.3.4.1 Tab.15 (XCDR2 cap).
    fn encode_xcdr2_le_at(
        &self,
        dst: &mut [u8],
        offset: &mut usize,
    ) -> Result<(), LocalCdrError> {
        // octet a — alignment 1
        if *offset + 1 > dst.len() {
            return Err(LocalCdrError::BufferTooSmall);
        }
        dst[*offset] = self.a;
        *offset += 1;

        // pad to align(4) for double in XCDR2
        let pad = (4 - (*offset % 4)) % 4;
        if *offset + pad + 8 > dst.len() {
            return Err(LocalCdrError::BufferTooSmall);
        }
        for _ in 0..pad {
            dst[*offset] = 0;
            *offset += 1;
        }

        // double b
        dst[*offset..*offset + 8].copy_from_slice(&self.b.to_le_bytes());
        *offset += 8;
        Ok(())
    }

    /// Spec-compliant XCDR1 encoder for the Probe IDL type.
    /// 8-byte primitive (`f64`) aligns to 8 in XCDR1 (no cap).
    fn encode_xcdr1_le_at(
        &self,
        dst: &mut [u8],
        offset: &mut usize,
    ) -> Result<(), LocalCdrError> {
        // octet a — alignment 1
        if *offset + 1 > dst.len() {
            return Err(LocalCdrError::BufferTooSmall);
        }
        dst[*offset] = self.a;
        *offset += 1;

        // pad to align(8) for double in XCDR1
        let pad = (8 - (*offset % 8)) % 8;
        if *offset + pad + 8 > dst.len() {
            return Err(LocalCdrError::BufferTooSmall);
        }
        for _ in 0..pad {
            dst[*offset] = 0;
            *offset += 1;
        }

        // double b
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn fastdds_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, FASTDDS_X2_NOMINAL, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn fastdds_xcdr2_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, FASTDDS_X2_NAN, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti6_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI6_X2_NOMINAL, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti6_xcdr2_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, RTI6_X2_NAN, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti7_xcdr2_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI7_X2_NOMINAL, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti7_xcdr2_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, RTI7_X2_NAN, true);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn fastdds_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, FASTDDS_X1_NOMINAL, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn fastdds_xcdr1_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, FASTDDS_X1_NAN, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti6_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI6_X1_NOMINAL, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti6_xcdr1_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, RTI6_X1_NAN, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
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
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti7_xcdr1_nominal() {
    assert_probe_matches_golden(Probe { a: 0x42, b: 1.0 }, RTI7_X1_NOMINAL, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti7_xcdr1_nan() {
    assert_probe_matches_golden(Probe { a: 0x42, b: f64::NAN }, RTI7_X1_NAN, false);
}

#[test]
#[ignore = "F01 not yet fixed (cdr2_alignment systemic)"]
fn rti7_xcdr1_neg_zero() {
    assert_probe_matches_golden(Probe { a: 0x42, b: -0.0 }, RTI7_X1_NEG_ZERO, false);
}
