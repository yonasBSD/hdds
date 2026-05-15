// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Annotation type definitions
//!
//!
//! Complete and Minimal annotation types, headers, and parameters.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.10 (Annotation Types)

use super::dheader::{decode_dheader_at, encode_dheader_at};
use super::helpers::checked_usize;
use super::primitives::{
    align_offset, decode_bool, decode_i32, decode_option, decode_string, decode_u16, decode_u32,
    decode_u8, encode_bool, encode_i32, encode_option, encode_string, encode_u16, encode_u32,
    encode_u8, encode_vec_sorted,
};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::type_object::compute_name_hash;
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

// Annotation Headers — `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
impl Cdr2Encode for CompleteAnnotationHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.detail.encode_cdr2_le_at(dst, offset)
        })
    }
}

impl Cdr2Decode for CompleteAnnotationHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
            Ok(CompleteAnnotationHeader { detail })
        })
    }
}

impl Cdr2Encode for MinimalAnnotationHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.detail.encode_cdr2_le_at(dst, offset)
        })
    }
}

impl Cdr2Decode for MinimalAnnotationHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;
            Ok(MinimalAnnotationHeader { detail })
        })
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let member_flags = AnnotationParameterFlag::decode_cdr2_le_at(src, offset)?;
        let member_type_id = TypeIdentifier::decode_cdr2_le_at(src, offset)?;
        Ok(CommonAnnotationParameter {
            member_flags,
            member_type_id,
        })
    }
}

// CompleteAnnotationParameter — `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
impl Cdr2Encode for CompleteAnnotationParameter {
    fn max_cdr2_size(&self) -> usize {
        let default_value_size = match &self.default_value {
            None => 4, // bool flag
            Some(v) => 4 + v.max_cdr2_size(),
        };
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.common.max_cdr2_size() + 4 + self.name.len() + 1 + default_value_size
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.common.encode_cdr2_le_at(dst, offset)?;
            encode_string(&self.name, dst, offset)?;
            encode_option(&self.default_value, dst, offset, |value, dst, offset| {
                value.encode_cdr2_le_at(dst, offset)
            })?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteAnnotationParameter {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
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
        })
    }
}

// MinimalAnnotationParameter — `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
impl Cdr2Encode for MinimalAnnotationParameter {
    fn max_cdr2_size(&self) -> usize {
        let default_value_size = match &self.default_value {
            None => 4, // bool flag
            Some(v) => 4 + v.max_cdr2_size(),
        };
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.common.max_cdr2_size() + 4 + default_value_size
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.common.encode_cdr2_le_at(dst, offset)?;
            encode_u32(self.name_hash, dst, offset)?;
            encode_option(&self.default_value, dst, offset, |value, dst, offset| {
                value.encode_cdr2_le_at(dst, offset)
            })?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalAnnotationParameter {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
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
        // XTypes v1.3 §7.3.4.5 R3: AnnotationParameterSeq must be emitted
        // in ascending paramname_hash order. Complete keeps the human
        // name but the wire-position invariant still applies, so sort by
        // the same hash function used to build MinimalAnnotationParameter
        // — this keeps Complete[i] and Minimal[i] referring to the same
        // parameter for any consumer that decodes both representations.
        //
        // `compute_name_hash` returns 0 under `--no-default-features`
        // (md-5 dep is xtypes-only), which makes the sort degenerate to
        // source order — same wire output as the pre-1.7e baseline. No
        // cfg branch needed: stable sort with constant key is a no-op.
        encode_vec_sorted(
            &self.member_seq,
            dst,
            offset,
            |param: &CompleteAnnotationParameter| compute_name_hash(&param.name),
            |param: &CompleteAnnotationParameter, dst, offset| param.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteAnnotationType {
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
        // XTypes v1.3 §7.3.4.5 R10: MinimalAnnotationParameterSeq must
        // be emitted in ascending name_hash order so the resulting
        // MinimalEquivalenceHash is stable across vendors. The
        // `param.name_hash` field is populated at construction time;
        // under `--no-default-features` all values are 0, the stable
        // sort degenerates to source order, and the wire bytes match
        // the pre-1.7e baseline.
        encode_vec_sorted(
            &self.member_seq,
            dst,
            offset,
            |param: &MinimalAnnotationParameter| param.name_hash,
            |param: &MinimalAnnotationParameter, dst, offset| param.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalAnnotationType {
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

    #[cfg(feature = "xtypes")]
    fn make_minimal_param(name: &str) -> MinimalAnnotationParameter {
        MinimalAnnotationParameter {
            common: CommonAnnotationParameter {
                member_flags: AnnotationParameterFlag(0),
                member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
            },
            name_hash: compute_name_hash(name),
            default_value: None,
        }
    }

    /// XTypes v1.3 §7.3.4.5 R10: MinimalAnnotationParameterSeq must be
    /// emitted in ascending name_hash order. The MD5-derived hash makes
    /// the sort key opaque, so the test seeds names whose hashes happen
    /// to differ and asserts that byte output is invariant under input
    /// permutations.
    ///
    /// Gated on `xtypes` because the assertion (two permutations of the
    /// same set yield identical bytes) depends on distinct non-zero
    /// hashes. Under `--no-default-features` all `compute_name_hash`
    /// values collapse to 0, the stable sort preserves source order,
    /// and the two permutations encode to different bytes — a correct
    /// outcome for that degenerate configuration, but not what this
    /// test is meant to lock.
    #[cfg(feature = "xtypes")]
    #[test]
    fn minimal_annotation_params_emit_sorted_by_name_hash() {
        let header = MinimalAnnotationHeader {
            detail: MinimalTypeDetail::new(),
        };

        let names = ["zeta", "alpha", "mu", "kappa"];
        let mut perm_a: Vec<MinimalAnnotationParameter> =
            names.iter().map(|n| make_minimal_param(n)).collect();
        let mut perm_b = perm_a.clone();
        perm_b.reverse(); // different source order, same set

        let type_a = MinimalAnnotationType {
            header: header.clone(),
            member_seq: perm_a.clone(),
        };
        let type_b = MinimalAnnotationType {
            header,
            member_seq: perm_b.clone(),
        };

        let mut buf_a = vec![0u8; type_a.max_cdr2_size()];
        let len_a = type_a.encode_cdr2_le(&mut buf_a).expect("encode perm_a");
        let mut buf_b = vec![0u8; type_b.max_cdr2_size()];
        let len_b = type_b.encode_cdr2_le(&mut buf_b).expect("encode perm_b");

        assert_eq!(len_a, len_b, "wire length must match across permutations");
        assert_eq!(
            &buf_a[..len_a],
            &buf_b[..len_b],
            "two source permutations must produce identical bytes \
             (R10 enforcement)"
        );

        // Round-trip: the decoded member_seq must be in ascending
        // name_hash order.
        let (decoded, used) =
            MinimalAnnotationType::decode_cdr2_le(&buf_a[..len_a]).expect("decode round-trip");
        assert_eq!(used, len_a, "decoder consumes full input");
        let hashes: Vec<u32> = decoded.member_seq.iter().map(|p| p.name_hash).collect();
        let mut sorted_hashes = hashes.clone();
        sorted_hashes.sort();
        assert_eq!(
            hashes, sorted_hashes,
            "decoded name_hashes are in ascending order"
        );

        // Sanity: with 4 distinct names whose hashes are unlikely to
        // collide, all 4 should be present.
        assert_eq!(decoded.member_seq.len(), 4);

        // Also sort both perm Vecs locally by name_hash and confirm
        // the in-memory order would match the wire after sort. Drops
        // the unused mut warnings.
        perm_a.sort_by_key(|p| p.name_hash);
        perm_b.sort_by_key(|p| p.name_hash);
        assert_eq!(perm_a, perm_b);
    }

    /// XTypes v1.3 §7.3.4.5 R3/R9 applied to Complete: sort by
    /// compute_name_hash(name) so Complete[i] corresponds to
    /// Minimal[i] for the same parameter regardless of how the
    /// producer populated the Vec. Gated on `xtypes` for the same
    /// reason as the Minimal sibling above.
    #[cfg(feature = "xtypes")]
    #[test]
    fn complete_annotation_params_emit_sorted_by_name_hash() {
        let header = CompleteAnnotationHeader {
            detail: CompleteTypeDetail {
                type_name: "MyAnnot".to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        fn make_complete(name: &str) -> CompleteAnnotationParameter {
            CompleteAnnotationParameter {
                common: CommonAnnotationParameter {
                    member_flags: AnnotationParameterFlag(0),
                    member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
                },
                name: name.to_string(),
                default_value: None,
            }
        }

        let names = ["sigma", "delta", "omega"];
        let perm_a: Vec<_> = names.iter().map(|n| make_complete(n)).collect();
        let mut perm_b = perm_a.clone();
        perm_b.reverse();

        let type_a = CompleteAnnotationType {
            header: header.clone(),
            member_seq: perm_a,
        };
        let type_b = CompleteAnnotationType {
            header,
            member_seq: perm_b,
        };

        let mut buf_a = vec![0u8; type_a.max_cdr2_size()];
        let len_a = type_a.encode_cdr2_le(&mut buf_a).expect("encode perm_a");
        let mut buf_b = vec![0u8; type_b.max_cdr2_size()];
        let len_b = type_b.encode_cdr2_le(&mut buf_b).expect("encode perm_b");

        assert_eq!(len_a, len_b, "wire length must match across permutations");
        assert_eq!(
            &buf_a[..len_a],
            &buf_b[..len_b],
            "two source permutations must produce identical Complete bytes \
             (R3/R9 enforcement)"
        );
    }
}
