// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Union type definitions
//!
//! Complete and Minimal union types, headers, and members.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.2 (Union Types)

use super::dheader::{decode_dheader_at, encode_dheader_at};
use super::members::{decode_complete_union_member_internal, decode_minimal_union_member_internal};
use super::primitives::{decode_u16, encode_u16, encode_vec_sorted};
use super::type_identifier::decode_type_identifier_internal;
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// UnionHeader CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteUnionHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 32 + self.detail.max_cdr2_size()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.discriminator.encode_cdr2_le_at(dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteUnionHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_union_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteUnionHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_complete_union_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteUnionHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let discriminator = decode_type_identifier_internal(src, offset)?;
        let detail =
            super::helpers::decode_detail_with_reencoding::<CompleteTypeDetail>(src, offset)?;
        Ok(CompleteUnionHeader {
            discriminator,
            detail,
        })
    })
}

impl Cdr2Encode for MinimalUnionHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 32 + self.detail.max_cdr2_size()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.discriminator.encode_cdr2_le_at(dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalUnionHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_union_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalUnionHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_minimal_union_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalUnionHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let discriminator = decode_type_identifier_internal(src, offset)?;
        // MinimalTypeDetail is empty (encodes as 0 bytes)
        let detail =
            super::helpers::decode_detail_with_reencoding::<MinimalTypeDetail>(src, offset)?;
        Ok(MinimalUnionHeader {
            discriminator,
            detail,
        })
    })
}

// ============================================================================
// CompleteUnionType / MinimalUnionType CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteUnionType {
    fn max_cdr2_size(&self) -> usize {
        super::helpers::max_size_type_with_members(&self.header, &self.member_seq)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.union_flags.0, dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        // XTypes v1.3 §7.3.4.5 R7: CompleteUnionMemberSeq must be emitted
        // in ascending `member_id` order so the EquivalenceHash is
        // bitwise-identical across vendors. member_id semantics follow
        // §7.3.1.2 (sequential for @final/@appendable, hash for @mutable).
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

impl Cdr2Decode for CompleteUnionType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let union_flags = UnionTypeFlag(decode_u16(src, offset)?);

        // Decode header using internal helper
        let header = decode_complete_union_header_internal(src, offset)?;

        // Decode member_seq using helper for proper alignment
        let member_seq = super::helpers::decode_member_sequence(
            src,
            offset,
            decode_complete_union_member_internal,
        )?;

        Ok(CompleteUnionType {
            union_flags,
            header,
            member_seq,
        })
    }
}

impl Cdr2Encode for MinimalUnionType {
    fn max_cdr2_size(&self) -> usize {
        super::helpers::max_size_type_with_members(&self.header, &self.member_seq)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.union_flags.0, dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        // XTypes v1.3 §7.3.4.5 R8: MinimalUnionMemberSeq ordering — same
        // member_id key as R7 above, drives the MinimalEquivalenceHash
        // used by the discovery assignability check.
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

impl Cdr2Decode for MinimalUnionType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let union_flags = UnionTypeFlag(decode_u16(src, offset)?);

        // Decode header using internal helper
        let header = decode_minimal_union_header_internal(src, offset)?;

        // Decode member_seq using helper for proper alignment
        let member_seq = super::helpers::decode_member_sequence(
            src,
            offset,
            decode_minimal_union_member_internal,
        )?;

        Ok(MinimalUnionType {
            union_flags,
            header,
            member_seq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::{MemberFlag, TypeIdentifier, TypeKind, UnionTypeFlag};

    fn make_minimal_member(member_id: u32, label: i32, name_hash: u32) -> MinimalUnionMember {
        MinimalUnionMember {
            common: CommonUnionMember {
                member_id,
                member_flags: MemberFlag::empty(),
                member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
                label_seq: vec![label],
            },
            detail: MinimalMemberDetail { name_hash },
        }
    }

    fn minimal_header() -> MinimalUnionHeader {
        MinimalUnionHeader {
            discriminator: TypeIdentifier::primitive(TypeKind::TK_INT32),
            detail: MinimalTypeDetail::new(),
        }
    }

    /// XTypes v1.3 §7.3.4.5 R8: MinimalUnionMemberSeq must be emitted in
    /// ascending `member_id` order. Same guarantee as struct R5/R6, with
    /// label_seq retained verbatim per member (the sort touches only the
    /// outer Vec).
    #[test]
    fn minimal_union_members_emit_sorted_by_member_id() {
        let header = minimal_header();

        let unsorted = MinimalUnionType {
            union_flags: UnionTypeFlag(0),
            header: header.clone(),
            member_seq: vec![
                make_minimal_member(4, 40, 0x44),
                make_minimal_member(1, 10, 0x11),
                make_minimal_member(3, 30, 0x33),
            ],
        };
        let sorted = MinimalUnionType {
            union_flags: UnionTypeFlag(0),
            header,
            member_seq: vec![
                make_minimal_member(1, 10, 0x11),
                make_minimal_member(3, 30, 0x33),
                make_minimal_member(4, 40, 0x44),
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
             (R8 enforcement)"
        );

        // Round-trip: decode the unsorted-input bytes and confirm we
        // recover member_ids in sorted order with their labels intact.
        let (decoded, used) = MinimalUnionType::decode_cdr2_le(&buf_unsorted[..len_unsorted])
            .expect("decode round-trip");
        assert_eq!(used, len_unsorted, "decoder consumes full input");
        let ids: Vec<u32> = decoded
            .member_seq
            .iter()
            .map(|m| m.common.member_id)
            .collect();
        assert_eq!(ids, vec![1, 3, 4], "decoded member_ids are sorted");
        let labels: Vec<i32> = decoded
            .member_seq
            .iter()
            .map(|m| m.common.label_seq[0])
            .collect();
        assert_eq!(
            labels,
            vec![10, 30, 40],
            "labels follow their owning member after sort"
        );
    }

    /// XTypes v1.3 §7.3.4.5 R7: same guarantee for CompleteUnionType.
    #[test]
    fn complete_union_members_emit_sorted_by_member_id() {
        let header = CompleteUnionHeader {
            discriminator: TypeIdentifier::primitive(TypeKind::TK_INT32),
            detail: CompleteTypeDetail {
                type_name: "Choice".to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let make = |member_id: u32, name: &str| CompleteUnionMember {
            common: CommonUnionMember {
                member_id,
                member_flags: MemberFlag::empty(),
                member_type_id: TypeIdentifier::primitive(TypeKind::TK_INT32),
                label_seq: vec![member_id as i32],
            },
            detail: CompleteMemberDetail {
                name: name.to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let unsorted = CompleteUnionType {
            union_flags: UnionTypeFlag(0),
            header: header.clone(),
            member_seq: vec![make(5, "e"), make(0, "a"), make(2, "c")],
        };
        let sorted = CompleteUnionType {
            union_flags: UnionTypeFlag(0),
            header,
            member_seq: vec![make(0, "a"), make(2, "c"), make(5, "e")],
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
             (R7 enforcement)"
        );
    }
}
