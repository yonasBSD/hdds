// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Bitset CDR2 serialization for different bit widths (u8/u16/u32/u64).
//!
//!

use super::super::dheader::{decode_dheader_at, encode_dheader_at};
use super::super::helpers::checked_usize;
use super::super::primitives::{
    align_offset, decode_u16, decode_u32, decode_u8, encode_u16, encode_u8, encode_vec_sorted,
};
use super::super::type_identifier::decode_type_identifier_internal;
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::type_object::{
    BitsetTypeFlag, CompleteBitsetHeader, CompleteBitsetType, CompleteTypeDetail,
    MinimalBitsetHeader, MinimalBitsetType, MinimalTypeDetail,
};

use super::bitfield::{decode_complete_bitfield_internal, decode_minimal_bitfield_internal};

// ============================================================================
// BitsetHeader CDR2 Encoding/Decoding
// ============================================================================

// Complete/MinimalBitsetHeader — `@extensibility(APPENDABLE)` per XTypes v1.3 spec lines 13321, 13327.
impl Cdr2Encode for CompleteBitsetHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload (flag + optional TypeIdentifier + detail)
        4 + 3 + 1 + 32 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            if let Some(ref base) = self.base_type {
                encode_u8(1, dst, offset)?;
                base.encode_cdr2_le_at(dst, offset)?;
            } else {
                encode_u8(0, dst, offset)?;
            }
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteBitsetHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_bitset_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteBitsetHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_complete_bitset_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteBitsetHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let base_type_present = decode_u8(src, offset)?;
        let base_type = if base_type_present == 1 {
            Some(decode_type_identifier_internal(src, offset)?)
        } else {
            None
        };
        let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteBitsetHeader { base_type, detail })
    })
}

impl Cdr2Encode for MinimalBitsetHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload (flag + optional TypeIdentifier + detail)
        4 + 3 + 1 + 32 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            if let Some(ref base) = self.base_type {
                encode_u8(1, dst, offset)?;
                base.encode_cdr2_le_at(dst, offset)?;
            } else {
                encode_u8(0, dst, offset)?;
            }
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalBitsetHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_bitset_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalBitsetHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_minimal_bitset_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalBitsetHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let base_type_present = decode_u8(src, offset)?;
        let base_type = if base_type_present == 1 {
            Some(decode_type_identifier_internal(src, offset)?)
        } else {
            None
        };
        // MinimalTypeDetail is empty, but decode it for consistency
        let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalBitsetHeader { base_type, detail })
    })
}

// ============================================================================
// CompleteBitsetType / MinimalBitsetType CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteBitsetType {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3
            + 4
            + self.header.max_cdr2_size()
            + 4
            + self
                .field_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30):
    /// DHEADER + payload-as-FINAL.
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_u16(self.bitset_flags.0, dst, offset)?;
            self.header.encode_cdr2_le_at(dst, offset)?;
            // XTypes v1.3 §7.3.4.5 R15: CompleteBitfieldSeq must be emitted
            // in ascending `position` order so the EquivalenceHash is
            // bitwise-identical across vendors.
            encode_vec_sorted(
                &self.field_seq,
                dst,
                offset,
                |f| f.common.position,
                |field, dst, offset| field.encode_cdr2_le_at(dst, offset),
            )?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteBitsetType {
    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let bitset_flags = BitsetTypeFlag(decode_u16(src, offset)?);
            let header = decode_complete_bitset_header_internal(src, offset)?;
            let field_len = decode_u32(src, offset)?;
            let capacity = checked_usize(field_len, "bitfield sequence length")?;
            let mut field_seq = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                *offset = align_offset(*offset, 4);
                let field = decode_complete_bitfield_internal(src, offset)?;
                field_seq.push(field);
            }
            Ok(CompleteBitsetType {
                bitset_flags,
                header,
                field_seq,
            })
        })
    }
}

impl Cdr2Encode for MinimalBitsetType {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3
            + 4
            + self.header.max_cdr2_size()
            + 4
            + self
                .field_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30):
    /// DHEADER + payload-as-FINAL.
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_u16(self.bitset_flags.0, dst, offset)?;
            self.header.encode_cdr2_le_at(dst, offset)?;
            // XTypes v1.3 §7.3.4.5 R15 (Minimal): MinimalBitfieldSeq
            // ordering — same `position` key as Complete above.
            encode_vec_sorted(
                &self.field_seq,
                dst,
                offset,
                |f| f.common.position,
                |field, dst, offset| field.encode_cdr2_le_at(dst, offset),
            )?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalBitsetType {
    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let bitset_flags = BitsetTypeFlag(decode_u16(src, offset)?);
            let header = decode_minimal_bitset_header_internal(src, offset)?;
            let field_len = decode_u32(src, offset)?;
            let capacity = checked_usize(field_len, "minimal bitfield sequence length")?;
            let mut field_seq = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                *offset = align_offset(*offset, 4);
                let field = decode_minimal_bitfield_internal(src, offset)?;
                field_seq.push(field);
            }
            Ok(MinimalBitsetType {
                bitset_flags,
                header,
                field_seq,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::type_object::{
        BitfieldFlag, CommonBitfield, CompleteBitfield, CompleteMemberDetail, MinimalBitfield,
        MinimalMemberDetail,
    };
    use crate::xtypes::{TypeIdentifier, TypeKind};

    fn make_minimal_field(position: u16, bit_count: u8, name_hash: u32) -> MinimalBitfield {
        MinimalBitfield {
            common: CommonBitfield {
                position,
                flags: BitfieldFlag(0),
                bit_count,
                holder_type: TypeIdentifier::primitive(TypeKind::TK_UINT32),
            },
            detail: MinimalMemberDetail { name_hash },
        }
    }

    /// XTypes v1.3 §7.3.4.5 R15: MinimalBitfieldSeq must be emitted in
    /// ascending `position` order. Round-trip ensures bit_count and
    /// holder_type stay attached to their owning field after the sort.
    #[test]
    fn minimal_bitset_fields_emit_sorted_by_position() {
        let header = MinimalBitsetHeader {
            base_type: None,
            detail: MinimalTypeDetail::new(),
        };

        let unsorted = MinimalBitsetType {
            bitset_flags: BitsetTypeFlag(0),
            header: header.clone(),
            field_seq: vec![
                make_minimal_field(16, 8, 0x10),
                make_minimal_field(0, 4, 0x00),
                make_minimal_field(8, 4, 0x08),
            ],
        };
        let sorted = MinimalBitsetType {
            bitset_flags: BitsetTypeFlag(0),
            header,
            field_seq: vec![
                make_minimal_field(0, 4, 0x00),
                make_minimal_field(8, 4, 0x08),
                make_minimal_field(16, 8, 0x10),
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
             (R15 Minimal enforcement)"
        );

        let (decoded, used) = MinimalBitsetType::decode_cdr2_le(&buf_unsorted[..len_unsorted])
            .expect("decode round-trip");
        assert_eq!(used, len_unsorted, "decoder consumes full input");
        let positions: Vec<u16> = decoded
            .field_seq
            .iter()
            .map(|f| f.common.position)
            .collect();
        assert_eq!(positions, vec![0, 8, 16], "decoded positions are sorted");
        let bit_counts: Vec<u8> = decoded
            .field_seq
            .iter()
            .map(|f| f.common.bit_count)
            .collect();
        assert_eq!(
            bit_counts,
            vec![4, 4, 8],
            "bit_count follows its owning field after sort"
        );
    }

    /// XTypes v1.3 §7.3.4.5 R15: same guarantee for CompleteBitsetType.
    #[test]
    fn complete_bitset_fields_emit_sorted_by_position() {
        let header = CompleteBitsetHeader {
            base_type: None,
            detail: CompleteTypeDetail {
                type_name: "Flags".to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let make = |position: u16, name: &str| CompleteBitfield {
            common: CommonBitfield {
                position,
                flags: BitfieldFlag(0),
                bit_count: 1,
                holder_type: TypeIdentifier::primitive(TypeKind::TK_BOOLEAN),
            },
            detail: CompleteMemberDetail {
                name: name.to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let unsorted = CompleteBitsetType {
            bitset_flags: BitsetTypeFlag(0),
            header: header.clone(),
            field_seq: vec![make(20, "u"), make(1, "b"), make(12, "m")],
        };
        let sorted = CompleteBitsetType {
            bitset_flags: BitsetTypeFlag(0),
            header,
            field_seq: vec![make(1, "b"), make(12, "m"), make(20, "u")],
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
             (R15 Complete enforcement)"
        );
    }
}
