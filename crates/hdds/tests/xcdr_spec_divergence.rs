// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com
//
// Differential probe test on the XCDR1 / XCDR2 alignment divergence for the
// IDL type `@final struct Probe { octet a; double b; };`.
//
// Per OMG DDS-XTypes v1.3 Sec.7.4.1.1.1 Table 31, XCDR v1 aligns 8-byte
// primitives on 8, producing 16 bytes (1 + 7 padding + 8).
// Per Sec.7.4.2 + Sec.7.4.3.2.2 Table 37 `MAXALIGN(VERSION2) = 4`, XCDR v2
// caps alignment at 4 for 8-byte primitives, producing 12 bytes
// (1 + 3 padding + 8).
//
// The `Probe` impl below is copied verbatim from `hddsgen gen rust Probe.idl`
// (hddsgen v1.0.10), minus the DDS trait impl -- only the `Cdr2Encode` /
// `Cdr2Decode` impls are needed to observe the wire bytes produced by the
// current codegen.

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

    fn encode_cdr2_le_at(
        &self,
        dst: &mut [u8],
        offset: &mut usize,
    ) -> Result<(), CdrError> {
        let len = self.encode_cdr2_le(&mut dst[*offset..])?;
        *offset += len;
        Ok(())
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
    let n = probe
        .encode_cdr2_le(&mut buf)
        .expect("Probe encode succeeds");
    buf.truncate(n);

    let xcdr1 = xcdr1_reference_bytes();
    let xcdr2 = xcdr2_spec_correct_reference_bytes();

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

    if !matches_xcdr1 && !matches_xcdr2 {
        panic!(
            "HDDS output matches neither reference pattern.\n\
             Got:   {:02X?}\n\
             XCDR1: {:02X?}\n\
             XCDR2: {:02X?}",
            buf, xcdr1, xcdr2,
        );
    }

    // Lock the current hand-written impl's output to the XCDR1 pattern.
    // A subsequent spec-correct rewrite of the copied impl to XCDR2 alignment
    // must flip this assertion to `xcdr2` and rename the test accordingly.
    assert_eq!(
        buf.as_slice(),
        xcdr1.as_slice(),
        "Probe wire bytes must match the XCDR1 pattern (16 bytes) \
         until the impl adopts the XCDR2 alignment cap."
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
