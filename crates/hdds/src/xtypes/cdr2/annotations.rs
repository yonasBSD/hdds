// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Annotation type definitions
//!
//!
//! Complete and Minimal annotation types, headers, and parameters.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.10 (Annotation Types)

use super::helpers::checked_usize;
use super::primitives::{
    align_offset, decode_bool, decode_i32, decode_option, decode_string, decode_u16, decode_u32,
    decode_u8, encode_bool, encode_i32, encode_option, encode_string, encode_u16, encode_u32,
    encode_u8, encode_vec,
};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::{TypeIdentifier, TypeKind};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// CompleteAnnotationType / MinimalAnnotationType CDR2 (0x0A)
// ============================================================================

// AnnotationParameterFlag
impl Cdr2Encode for AnnotationParameterFlag {
    fn max_cdr2_size(&self) -> usize {
        4 // 2 bytes + alignment
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.0, dst, offset)
    }
}

impl Cdr2Decode for AnnotationParameterFlag {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let flags = decode_u16(src, offset)?;
        Ok(AnnotationParameterFlag(flags))
    }
}

// AnnotationParameterValue (discriminated union enum).
// Spec source: OMG DDS-XTypes v1.3 §7.3.4.8.10 — the IDL union
// `AnnotationParameterValue switch (octet)` uses TK_* values as case labels.
// HDDS implements 4 of the 17 spec variants; the rest surface as
// `CdrError::Other` on decode (see negative test below).
// `docs/_privates/specs/DDS-XTypes-1.3.txt:12609-12655`.
impl Cdr2Encode for AnnotationParameterValue {
    fn max_cdr2_size(&self) -> usize {
        // Conservative: discriminator + max variant size
        match self {
            AnnotationParameterValue::Boolean(_) => 4 + 4, // disc + bool aligned
            AnnotationParameterValue::Int32(_) => 4 + 4,
            AnnotationParameterValue::String(s) => 4 + 4 + s.len() + 1, // disc + len + str + null
            AnnotationParameterValue::Enumerated(_) => 4 + 4,
        }
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        match self {
            AnnotationParameterValue::Boolean(b) => {
                encode_u8(TypeKind::TK_BOOLEAN.to_u8(), dst, offset)?;
                encode_bool(*b, dst, offset)?;
            }
            AnnotationParameterValue::Int32(i) => {
                encode_u8(TypeKind::TK_INT32.to_u8(), dst, offset)?;
                encode_i32(*i, dst, offset)?;
            }
            AnnotationParameterValue::String(s) => {
                encode_u8(TypeKind::TK_STRING8.to_u8(), dst, offset)?;
                encode_string(s, dst, offset)?;
            }
            AnnotationParameterValue::Enumerated(e) => {
                encode_u8(TypeKind::TK_ENUM.to_u8(), dst, offset)?;
                encode_i32(*e, dst, offset)?;
            }
        }
        Ok(())
    }
}

impl Cdr2Decode for AnnotationParameterValue {
    // @audit-ok: Simple pattern matching (cyclo 11, cogni 1) - discriminator dispatch to union variants
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Spec discriminator labels — see Cdr2Encode for the §7.3.4.8.10
        // citation. Local consts required because match patterns must
        // be const items, not const fn calls.
        const TK_BOOLEAN: u8 = TypeKind::TK_BOOLEAN.to_u8();
        const TK_INT32: u8 = TypeKind::TK_INT32.to_u8();
        const TK_STRING8: u8 = TypeKind::TK_STRING8.to_u8();
        const TK_ENUM: u8 = TypeKind::TK_ENUM.to_u8();

        let discriminator = decode_u8(src, offset)?;

        match discriminator {
            TK_BOOLEAN => Ok(AnnotationParameterValue::Boolean(decode_bool(src, offset)?)),
            TK_INT32 => Ok(AnnotationParameterValue::Int32(decode_i32(src, offset)?)),
            TK_STRING8 => Ok(AnnotationParameterValue::String(decode_string(
                src, offset,
            )?)),
            TK_ENUM => Ok(AnnotationParameterValue::Enumerated(decode_i32(
                src, offset,
            )?)),
            other => Err(CdrError::Other(format!(
                "AnnotationParameterValue discriminator 0x{:02X} not implemented in HDDS \
                 (spec §7.3.4.8.10 lists 17 case labels; HDDS supports TK_BOOLEAN, \
                 TK_INT32, TK_STRING8, TK_ENUM only)",
                other
            ))),
        }
    }
}

// Annotation Headers
impl Cdr2Encode for CompleteAnnotationHeader {
    fn max_cdr2_size(&self) -> usize {
        self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.detail.encode_cdr2_le_at(dst, offset)
    }
}

impl Cdr2Decode for CompleteAnnotationHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteAnnotationHeader { detail })
    }
}

impl Cdr2Encode for MinimalAnnotationHeader {
    fn max_cdr2_size(&self) -> usize {
        self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.detail.encode_cdr2_le_at(dst, offset)
    }
}

impl Cdr2Decode for MinimalAnnotationHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalAnnotationHeader { detail })
    }
}

// CommonAnnotationParameter
impl Cdr2Encode for CommonAnnotationParameter {
    fn max_cdr2_size(&self) -> usize {
        self.member_flags.max_cdr2_size() + self.member_type_id.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.member_flags.encode_cdr2_le_at(dst, offset)?;
        self.member_type_id.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CommonAnnotationParameter {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let member_flags = AnnotationParameterFlag::decode_cdr2_le_at(src, offset)?;
        let member_type_id = TypeIdentifier::decode_cdr2_le_at(src, offset)?;
        Ok(CommonAnnotationParameter {
            member_flags,
            member_type_id,
        })
    }
}

// CompleteAnnotationParameter
impl Cdr2Encode for CompleteAnnotationParameter {
    fn max_cdr2_size(&self) -> usize {
        let default_value_size = match &self.default_value {
            None => 4, // bool flag
            Some(v) => 4 + v.max_cdr2_size(),
        };
        self.common.max_cdr2_size() + 4 + self.name.len() + 1 + default_value_size
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.common.encode_cdr2_le_at(dst, offset)?;
        encode_string(&self.name, dst, offset)?;
        encode_option(&self.default_value, dst, offset, |value, dst, offset| {
            value.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteAnnotationParameter {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let common = CommonAnnotationParameter::decode_cdr2_le_at(src, offset)?;
        let name = decode_string(src, offset)?;
        let default_value = decode_option(src, offset, |src, offset| {
            AnnotationParameterValue::decode_cdr2_le_at(src, offset)
        })?;
        Ok(CompleteAnnotationParameter {
            common,
            name,
            default_value,
        })
    }
}

// MinimalAnnotationParameter
impl Cdr2Encode for MinimalAnnotationParameter {
    fn max_cdr2_size(&self) -> usize {
        let default_value_size = match &self.default_value {
            None => 4, // bool flag
            Some(v) => 4 + v.max_cdr2_size(),
        };
        self.common.max_cdr2_size() + 4 + default_value_size
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.common.encode_cdr2_le_at(dst, offset)?;
        encode_u32(self.name_hash, dst, offset)?;
        encode_option(&self.default_value, dst, offset, |value, dst, offset| {
            value.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalAnnotationParameter {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let common = CommonAnnotationParameter::decode_cdr2_le_at(src, offset)?;
        let name_hash = decode_u32(src, offset)?;
        let default_value = decode_option(src, offset, |src, offset| {
            AnnotationParameterValue::decode_cdr2_le_at(src, offset)
        })?;
        Ok(MinimalAnnotationParameter {
            common,
            name_hash,
            default_value,
        })
    }
}

// Complete/Minimal AnnotationType
impl Cdr2Encode for CompleteAnnotationType {
    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size()
            + 4
            + self
                .member_seq
                .iter()
                .map(|p| p.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(
            &self.member_seq,
            dst,
            offset,
            |param: &CompleteAnnotationParameter, dst, offset| param.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteAnnotationType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let header = CompleteAnnotationHeader::decode_cdr2_le_at(src, offset)?;
        // Decode member_seq (Vec<CompleteAnnotationParameter>)
        let param_len = decode_u32(src, offset)?;
        let capacity = checked_usize(param_len, "annotation parameter sequence length")?;
        let mut member_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            // v232: Bounds check after alignment to prevent panic on malformed data
            if *offset > src.len() {
                return Err(CdrError::UnexpectedEof);
            }
            let param = CompleteAnnotationParameter::decode_cdr2_le_at(src, offset)?;
            member_seq.push(param);
        }
        Ok(CompleteAnnotationType { header, member_seq })
    }
}

impl Cdr2Encode for MinimalAnnotationType {
    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size()
            + 4
            + self
                .member_seq
                .iter()
                .map(|p| p.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(
            &self.member_seq,
            dst,
            offset,
            |param: &MinimalAnnotationParameter, dst, offset| param.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalAnnotationType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let header = MinimalAnnotationHeader::decode_cdr2_le_at(src, offset)?;
        // Decode member_seq (Vec<MinimalAnnotationParameter>)
        let param_len = decode_u32(src, offset)?;
        let capacity = checked_usize(param_len, "minimal annotation parameter sequence length")?;
        let mut member_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            // v232 parity with CompleteAnnotationType: bounds check after
            // alignment to prevent panic on malformed data.
            if *offset > src.len() {
                return Err(CdrError::UnexpectedEof);
            }
            let param = MinimalAnnotationParameter::decode_cdr2_le_at(src, offset)?;
            member_seq.push(param);
        }
        Ok(MinimalAnnotationType { header, member_seq })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the wire-level discriminator byte emitted by each supported
    /// `AnnotationParameterValue` variant against the OMG DDS-XTypes
    /// v1.3 §7.3.4.8.10 IDL union case labels. Any drift in these
    /// bytes would silently break cross-vendor annotation matching, so
    /// the assertions are the spec citation reified as a test.
    #[test]
    fn annotation_param_value_discriminators_match_xtypes_v1_3_spec() {
        let mut buf = [0u8; 64];

        let len = AnnotationParameterValue::Boolean(true)
            .encode_cdr2_le(&mut buf)
            .expect("encode bool");
        assert_eq!(buf[0], 0x01, "Boolean must be TK_BOOLEAN=0x01");
        assert!(len >= 2);

        let len = AnnotationParameterValue::Int32(42)
            .encode_cdr2_le(&mut buf)
            .expect("encode i32");
        assert_eq!(buf[0], 0x04, "Int32 must be TK_INT32=0x04");
        assert!(len >= 5);

        let len = AnnotationParameterValue::String("x".to_string())
            .encode_cdr2_le(&mut buf)
            .expect("encode string");
        assert_eq!(buf[0], 0x20, "String must be TK_STRING8=0x20");
        assert!(len >= 5);

        let len = AnnotationParameterValue::Enumerated(7)
            .encode_cdr2_le(&mut buf)
            .expect("encode enum");
        assert_eq!(buf[0], 0x40, "Enumerated must be TK_ENUM=0x40");
        assert!(len >= 5);
    }

    /// Round-trip the 4 supported variants through encode + decode to
    /// confirm symmetry of the spec discriminator change.
    #[test]
    fn annotation_param_value_round_trip_supported_variants() {
        let cases = [
            AnnotationParameterValue::Boolean(true),
            AnnotationParameterValue::Boolean(false),
            AnnotationParameterValue::Int32(0),
            AnnotationParameterValue::Int32(-1),
            AnnotationParameterValue::Int32(i32::MAX),
            AnnotationParameterValue::String(String::new()),
            AnnotationParameterValue::String("hello".to_string()),
            AnnotationParameterValue::Enumerated(0),
            AnnotationParameterValue::Enumerated(255),
        ];
        for case in cases {
            let mut buf = vec![0u8; 256];
            let len = case.encode_cdr2_le(&mut buf).expect("encode succeeds");
            let (decoded, used) =
                AnnotationParameterValue::decode_cdr2_le(&buf[..len]).expect("decode succeeds");
            assert_eq!(decoded, case, "round-trip preserves value");
            assert_eq!(used, len, "decoder consumes all bytes");
        }
    }

    /// Garde-fou per Olivier brief 1.5c: every spec TK_* label that HDDS
    /// does NOT implement as an `AnnotationParameterValue` variant must
    /// surface as `CdrError::Other` on decode (never silently mapped to
    /// a wrong variant). Explicit cases instead of a wildcard so any
    /// future enum addition forces an intentional update of this test.
    #[test]
    fn annotation_param_value_unsupported_discriminators_reject_cleanly() {
        // Supported set per Chantier 1.5 scope α (4 of 17 spec variants):
        // TK_BOOLEAN=0x01, TK_INT32=0x04, TK_STRING8=0x20, TK_ENUM=0x40.
        // Every other byte in {spec TK_* set ∪ random} must Err.
        let unsupported_spec_discriminators: &[u8] = &[
            0x00, // TK_NONE — also collides with legacy HDDS Boolean wire byte
            0x02, // TK_BYTE — collides with legacy HDDS String wire byte
            0x03, // TK_INT16 — collides with legacy HDDS Enumerated wire byte
            0x05, // TK_INT64
            0x06, // TK_UINT16
            0x07, // TK_UINT32
            0x08, // TK_UINT64
            0x09, // TK_FLOAT32
            0x0A, // TK_FLOAT64
            0x0B, // TK_FLOAT128
            0x0C, // TK_INT8
            0x0D, // TK_UINT8
            0x10, // TK_CHAR8
            0x11, // TK_CHAR16
            0x21, // TK_STRING16
            0x30, // TK_ALIAS — not a union case label in the spec IDL, must still Err
            0x41, // TK_BITMASK — same
            0x51, // TK_STRUCTURE — same
            0xFF, // arbitrary out-of-spec byte
            0x7F, // arbitrary out-of-spec byte
        ];

        for &disc in unsupported_spec_discriminators {
            // Pad with zeros after the discriminator so the decoder
            // doesn't fail early on UnexpectedEof for the supported
            // arms that read further bytes — we want the discriminator
            // dispatch itself to surface Err.
            let buf = [disc, 0, 0, 0, 0, 0, 0, 0];
            match AnnotationParameterValue::decode_cdr2_le(&buf) {
                Err(CdrError::Other(msg)) => {
                    assert!(
                        msg.contains(&format!("0x{:02X}", disc)),
                        "discriminator 0x{:02X}: error message should mention the byte, got: {}",
                        disc,
                        msg
                    );
                }
                Err(other) => panic!(
                    "discriminator 0x{:02X}: expected CdrError::Other, got {:?}",
                    disc, other
                ),
                Ok((variant, _)) => panic!(
                    "discriminator 0x{:02X}: expected Err, got Ok({:?})",
                    disc, variant
                ),
            }
        }
    }
}
