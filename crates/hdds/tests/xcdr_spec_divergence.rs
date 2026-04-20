// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com
//
// XCDR Specification Divergence Probe (Phase 0 investigation)
//
// This test empirically determines whether hdds_gen's current `cdr2_alignment()`
// emits XCDR1 or XCDR2 spec-correct wire bytes for an 8-byte-aligned primitive.
//
// Context: WIP-XCDR1-INTEROP.md rev. 3, Phase 0.1.
//
// Reference IDL: `@final struct Probe { octet a; double b; };`
//
// XCDR1 expected (8-byte types aligned on 8):    16 bytes (1 + 7 padding + 8)
// XCDR2 spec-correct (8-byte types aligned on 4): 12 bytes (1 + 3 padding + 8)
//
// The `Probe` impl below is COPIED VERBATIM from `hddsgen gen rust Probe.idl`
// (hddsgen v1.0.10) so the test reflects exactly what the codegen emits today.
// The DDS trait impl from the generator is intentionally omitted -- only the
// raw `Cdr2Encode` / `Cdr2Decode` impls are needed to observe the wire bytes.

#![allow(clippy::float_cmp)]

use hdds::{Cdr2Decode, Cdr2Encode, CdrError};

// --- begin verbatim hddsgen v1.0.10 output for Probe.idl ---

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub a: u8,
    pub b: f64,
}

impl Cdr2Encode for Probe {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset: usize = 0;

        if dst.len() < offset + 1 {
            return Err(CdrError::BufferTooSmall);
        }
        dst[offset] = self.a;
        offset += 1;

        // Align to 8-byte boundary for field 'b'
        let padding = (8 - (offset % 8)) % 8;
        offset += padding;

        if dst.len() < offset + 8 {
            return Err(CdrError::BufferTooSmall);
        }
        dst[offset..offset + 8].copy_from_slice(&self.b.to_le_bytes());
        offset += 8;

        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        1 + 7 + 8
    }
}

impl Cdr2Decode for Probe {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset: usize = 0;

        if src.len() < offset + 1 {
            return Err(CdrError::UnexpectedEof);
        }
        let a = src[offset];
        offset += 1;

        let padding = (8 - (offset % 8)) % 8;
        offset += padding;

        if src.len() < offset + 8 {
            return Err(CdrError::UnexpectedEof);
        }
        let b = {
            let mut tmp = [0u8; 8];
            tmp.copy_from_slice(&src[offset..offset + 8]);
            f64::from_le_bytes(tmp)
        };
        offset += 8;

        Ok((Self { a, b }, offset))
    }
}

// --- end verbatim hddsgen v1.0.10 output ---

fn xcdr1_reference_bytes() -> [u8; 16] {
    [
        0x42, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 1 byte + 7 padding (align 8)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // f64 1.0 little-endian
    ]
}

fn xcdr2_spec_correct_reference_bytes() -> [u8; 12] {
    [
        0x42, 0x00, 0x00, 0x00, // 1 byte + 3 padding (align 4 per XTypes v1.3 Table 15)
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // f64 1.0 little-endian
    ]
}

#[test]
fn probe_octet_double_encoded_bytes_match_xcdr1_pattern() {
    let probe = Probe { a: 0x42, b: 1.0f64 };
    let mut buf = vec![0u8; probe.max_cdr2_size()];
    let n = probe.encode_cdr2_le(&mut buf).expect("Probe encode succeeds");
    buf.truncate(n);

    let xcdr1 = xcdr1_reference_bytes();
    let xcdr2 = xcdr2_spec_correct_reference_bytes();

    eprintln!("=== Phase 0 XCDR divergence probe ===");
    eprintln!("Probe {{ a = 0x42, b = 1.0f64 }}");
    eprintln!("HDDS encoded {} bytes:", buf.len());
    eprintln!("  {:02X?}", buf);
    eprintln!("XCDR1 reference (align 8-on-8, {} bytes):", xcdr1.len());
    eprintln!("  {:02X?}", &xcdr1);
    eprintln!(
        "XCDR2 spec-correct reference (align 8-on-4, {} bytes):",
        xcdr2.len()
    );
    eprintln!("  {:02X?}", &xcdr2);

    let matches_xcdr1 = buf.as_slice() == xcdr1.as_slice();
    let matches_xcdr2 = buf.as_slice() == xcdr2.as_slice();

    if matches_xcdr1 {
        eprintln!("\nVERDICT: HDDS emits the XCDR1 pattern (8-byte types aligned on 8).");
        eprintln!("This is consistent with hdds_gen::cdr2_alignment() returning 8 for");
        eprintln!("LongLong / UnsignedLongLong / Int64 / UInt64 / Double / LongDouble");
        eprintln!("(see projects/public/hdds_gen/src/codegen/rust_backend/helpers.rs:233-238).");
    } else if matches_xcdr2 {
        eprintln!("\nVERDICT: HDDS emits the XCDR2 spec-correct pattern (cap to 4).");
    } else {
        panic!(
            "HDDS output matches neither reference pattern.\n\
             Got:   {:02X?}\n\
             XCDR1: {:02X?}\n\
             XCDR2: {:02X?}",
            buf, xcdr1, xcdr2,
        );
    }

    // Lock the current behaviour until the Phase 2 alignment fix lands.
    // When that fix is applied, this assertion will need to flip to `xcdr2`
    // and the test name / docs should be updated accordingly.
    assert_eq!(
        buf.as_slice(),
        xcdr1.as_slice(),
        "Probe wire bytes must match XCDR1 pattern (16 bytes) until Phase 2 \
         applies the XCDR2 alignment cap fix."
    );
}

#[test]
fn probe_octet_double_roundtrip_decodes() {
    let probe = Probe { a: 0x42, b: 1.0f64 };
    let mut buf = vec![0u8; probe.max_cdr2_size()];
    let n = probe.encode_cdr2_le(&mut buf).unwrap();
    buf.truncate(n);

    let (decoded, consumed) = Probe::decode_cdr2_le(&buf).unwrap();
    assert_eq!(consumed, buf.len());
    assert_eq!(decoded, probe);
}
