// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Struct type definitions
//!
//! Complete and Minimal struct types, headers, and members.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.1 (Struct Types)

use super::dheader::{decode_dheader_at, encode_dheader_at};
use super::members::{
    decode_complete_struct_member_internal, decode_minimal_struct_member_internal,
};
use super::primitives::{decode_option, decode_u16, encode_option, encode_u16, encode_vec_sorted};
use super::type_identifier::decode_type_identifier_internal;
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// StructHeader CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteStructHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 32 + self.detail.max_cdr2_size()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_option(&self.base_type, dst, offset, |type_id, dst, offset| {
                type_id.encode_cdr2_le_at(dst, offset)
            })?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteStructHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_struct_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteStructHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_complete_struct_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteStructHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let base_type = decode_option(src, offset, |src, offset| {
            decode_type_identifier_internal(src, offset)
        })?;
        let detail =
            super::helpers::decode_detail_with_reencoding::<CompleteTypeDetail>(src, offset)?;
        Ok(CompleteStructHeader { base_type, detail })
    })
}

impl Cdr2Encode for MinimalStructHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 32 + self.detail.max_cdr2_size()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_option(&self.base_type, dst, offset, |type_id, dst, offset| {
                type_id.encode_cdr2_le_at(dst, offset)
            })?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalStructHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_struct_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalStructHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_minimal_struct_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalStructHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let base_type = decode_option(src, offset, |src, offset| {
            decode_type_identifier_internal(src, offset)
        })?;
        // MinimalTypeDetail is empty (encodes as 0 bytes)
        let detail =
            super::helpers::decode_detail_with_reencoding::<MinimalTypeDetail>(src, offset)?;
        Ok(MinimalStructHeader { base_type, detail })
    })
}

// ============================================================================
// CompleteStructType / MinimalStructType CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteStructType {
    fn max_cdr2_size(&self) -> usize {
        super::helpers::max_size_type_with_members(&self.header, &self.member_seq)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.struct_flags.0, dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        // XTypes v1.3 §7.3.4.5 R5: CompleteStructMemberSeq must be emitted
        // in ascending `member_id` order (acting as member_index for
        // @final/@appendable per §7.3.1.2; hash-assigned for @mutable but
        // ascending sort still satisfies the spec ordering predicate).
        encode_vec_sorted(
            &self.member_seq,
            dst,
            offset,
            |m| m.common.member_id,
            |member, dst, offset| member.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteStructType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let struct_flags = StructTypeFlag(decode_u16(src, offset)?);

        // Decode header using internal helper
        let header = decode_complete_struct_header_internal(src, offset)?;

        // Decode member_seq using helper for proper alignment
        let member_seq = super::helpers::decode_member_sequence(
            src,
            offset,
            decode_complete_struct_member_internal,
        )?;

        Ok(CompleteStructType {
            struct_flags,
            header,
            member_seq,
        })
    }
}

impl Cdr2Encode for MinimalStructType {
    fn max_cdr2_size(&self) -> usize {
        super::helpers::max_size_type_with_members(&self.header, &self.member_seq)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.struct_flags.0, dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        // XTypes v1.3 §7.3.4.5 R6: MinimalStructMemberSeq ordering — same
        // member_id key as R5 above, drives the MinimalEquivalenceHash
        // used by the assignability check between Writer and Reader.
        encode_vec_sorted(
            &self.member_seq,
            dst,
            offset,
            |m| m.common.member_id,
            |member, dst, offset| member.encode_cdr2_le_at(dst, offset),
        )?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalStructType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let struct_flags = StructTypeFlag(decode_u16(src, offset)?);

        // Decode header using internal helper
        let header = decode_minimal_struct_header_internal(src, offset)?;

        // Decode member_seq using helper for proper alignment
        let member_seq = super::helpers::decode_member_sequence(
            src,
            offset,
            decode_minimal_struct_member_internal,
        )?;

        Ok(MinimalStructType {
            struct_flags,
            header,
            member_seq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::{MemberFlag, TypeIdentifier, TypeKind};

    fn make_minimal_member(member_id: u32, name_hash: u32) -> MinimalStructMember {
        MinimalStructMember {
            common: CommonStructMember {
                member_id,
                member_flags: MemberFlag::empty(),
                member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
            },
            detail: MinimalMemberDetail { name_hash },
        }
    }

    fn make_complete_member(member_id: u32, name: &str) -> CompleteStructMember {
        CompleteStructMember {
            common: CommonStructMember {
                member_id,
                member_flags: MemberFlag::empty(),
                member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
            },
            detail: CompleteMemberDetail {
                name: name.to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        }
    }

    fn minimal_header() -> MinimalStructHeader {
        MinimalStructHeader {
            base_type: None,
            detail: MinimalTypeDetail::new(),
        }
    }

    /// XTypes v1.3 §7.3.4.5 R6: MinimalStructMemberSeq must be emitted
    /// in ascending `member_id` order. Two source Vecs that differ only
    /// in element order must produce byte-identical wire output, and
    /// the bytes must round-trip back to a Vec in sorted order.
    #[test]
    fn minimal_struct_members_emit_sorted_by_member_id() {
        let header = minimal_header();

        let unsorted = MinimalStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header: header.clone(),
            member_seq: vec![
                make_minimal_member(5, 0x55),
                make_minimal_member(1, 0x11),
                make_minimal_member(3, 0x33),
                make_minimal_member(2, 0x22),
            ],
        };
        let sorted = MinimalStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header,
            member_seq: vec![
                make_minimal_member(1, 0x11),
                make_minimal_member(2, 0x22),
                make_minimal_member(3, 0x33),
                make_minimal_member(5, 0x55),
            ],
        };

        let mut buf_unsorted = vec![0u8; unsorted.max_cdr2_size()];
        let len_unsorted = unsorted
            .encode_cdr2_le(&mut buf_unsorted)
            .expect("encode unsorted");
        let mut buf_sorted = vec![0u8; sorted.max_cdr2_size()];
        let len_sorted = sorted
            .encode_cdr2_le(&mut buf_sorted)
            .expect("encode sorted");

        assert_eq!(len_unsorted, len_sorted, "wire length must match");
        assert_eq!(
            &buf_unsorted[..len_unsorted],
            &buf_sorted[..len_sorted],
            "unsorted input must produce bytes identical to sorted input \
             (R6 enforcement)"
        );

        // Round-trip: decode the unsorted-input bytes and confirm we
        // recover the sorted member_id sequence (1, 2, 3, 5). This
        // protects against an alignment divergence in encode_vec_sorted
        // that could otherwise pass the bytewise comparison above while
        // silently corrupting the wire format.
        let (decoded, used) = MinimalStructType::decode_cdr2_le(&buf_unsorted[..len_unsorted])
            .expect("decode round-trip");
        assert_eq!(used, len_unsorted, "decoder consumes full input");
        let ids: Vec<u32> = decoded
            .member_seq
            .iter()
            .map(|m| m.common.member_id)
            .collect();
        assert_eq!(ids, vec![1, 2, 3, 5], "decoded member_ids are sorted");
    }

    /// XTypes v1.3 §7.3.4.5 R5: same guarantee for CompleteStructType.
    #[test]
    fn complete_struct_members_emit_sorted_by_member_id() {
        let header = CompleteStructHeader {
            base_type: None,
            detail: CompleteTypeDetail {
                type_name: "Mixed".to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let unsorted = CompleteStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header: header.clone(),
            member_seq: vec![
                make_complete_member(10, "z"),
                make_complete_member(2, "x"),
                make_complete_member(7, "y"),
            ],
        };
        let sorted = CompleteStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header,
            member_seq: vec![
                make_complete_member(2, "x"),
                make_complete_member(7, "y"),
                make_complete_member(10, "z"),
            ],
        };

        let mut buf_unsorted = vec![0u8; unsorted.max_cdr2_size()];
        let len_unsorted = unsorted
            .encode_cdr2_le(&mut buf_unsorted)
            .expect("encode unsorted");
        let mut buf_sorted = vec![0u8; sorted.max_cdr2_size()];
        let len_sorted = sorted
            .encode_cdr2_le(&mut buf_sorted)
            .expect("encode sorted");

        assert_eq!(len_unsorted, len_sorted, "wire length must match");
        assert_eq!(
            &buf_unsorted[..len_unsorted],
            &buf_sorted[..len_sorted],
            "unsorted input must produce bytes identical to sorted input \
             (R5 enforcement)"
        );
    }
}
