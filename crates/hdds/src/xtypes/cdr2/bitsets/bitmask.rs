// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Bitmask encoding for Complete/Minimal types.
//!

use super::super::dheader::{decode_dheader_at, encode_dheader_at};
use super::super::helpers::checked_usize;
use super::super::primitives::{
    align_offset, decode_i16, decode_u32, encode_i16, encode_vec_sorted,
};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::type_object::{
    CompleteBitmaskHeader, CompleteBitmaskType, CompleteTypeDetail, MinimalBitmaskHeader,
    MinimalBitmaskType, MinimalTypeDetail,
};

use super::bitflag::{decode_complete_bitflag_internal, decode_minimal_bitflag_internal};

// ============================================================================
// BitmaskHeader CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteBitmaskHeader {
    fn max_cdr2_size(&self) -> usize {
        4 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_i16(self.bit_bound, dst, offset)?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteBitmaskHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_bitmask_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteBitmaskHeader decoding
pub(super) fn decode_complete_bitmask_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteBitmaskHeader, CdrError> {
    let bit_bound = decode_i16(src, offset)?;
    let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;

    Ok(CompleteBitmaskHeader { bit_bound, detail })
}

impl Cdr2Encode for MinimalBitmaskHeader {
    fn max_cdr2_size(&self) -> usize {
        4 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_i16(self.bit_bound, dst, offset)?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalBitmaskHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_bitmask_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalBitmaskHeader decoding
pub(super) fn decode_minimal_bitmask_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalBitmaskHeader, CdrError> {
    let bit_bound = decode_i16(src, offset)?;

    // MinimalTypeDetail is empty, but decode it for consistency
    let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;

    Ok(MinimalBitmaskHeader { bit_bound, detail })
}

// ============================================================================
// CompleteBitmaskType / MinimalBitmaskType CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteBitmaskType {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3
            + self.header.max_cdr2_size()
            + 4
            + self
                .flag_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30):
    /// DHEADER + payload-as-FINAL.
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.header.encode_cdr2_le_at(dst, offset)?;
            // XTypes v1.3 §7.3.4.5 R13: CompleteBitflagSeq must be emitted
            // in ascending `position` order so the EquivalenceHash is
            // bitwise-identical across vendors.
            encode_vec_sorted(
                &self.flag_seq,
                dst,
                offset,
                |f| f.common.position,
                |flag, dst, offset| flag.encode_cdr2_le_at(dst, offset),
            )?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteBitmaskType {
    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let header = decode_complete_bitmask_header_internal(src, offset)?;
            let flag_len = decode_u32(src, offset)?;
            let capacity = checked_usize(flag_len, "bitflag sequence length")?;
            let mut flag_seq = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                *offset = align_offset(*offset, 4);
                let flag = decode_complete_bitflag_internal(src, offset)?;
                flag_seq.push(flag);
            }
            Ok(CompleteBitmaskType { header, flag_seq })
        })
    }
}

impl Cdr2Encode for MinimalBitmaskType {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3
            + self.header.max_cdr2_size()
            + 4
            + self
                .flag_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30):
    /// DHEADER + payload-as-FINAL.
    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.header.encode_cdr2_le_at(dst, offset)?;
            // XTypes v1.3 §7.3.4.5 R14: MinimalBitflagSeq ordering — same
            // `position` key as R13 above, drives the
            // MinimalEquivalenceHash for bitmask types.
            encode_vec_sorted(
                &self.flag_seq,
                dst,
                offset,
                |f| f.common.position,
                |flag, dst, offset| flag.encode_cdr2_le_at(dst, offset),
            )?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalBitmaskType {
    /// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_dheader_at(src, offset, |src, offset| {
            let header = decode_minimal_bitmask_header_internal(src, offset)?;
            let flag_len = decode_u32(src, offset)?;
            let capacity = checked_usize(flag_len, "minimal bitflag sequence length")?;
            let mut flag_seq = Vec::with_capacity(capacity);
            for _ in 0..capacity {
                *offset = align_offset(*offset, 4);
                let flag = decode_minimal_bitflag_internal(src, offset)?;
                flag_seq.push(flag);
            }
            Ok(MinimalBitmaskType { header, flag_seq })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::type_object::{
        BitflagFlag, CommonBitflag, CompleteBitflag, CompleteMemberDetail, MinimalBitflag,
        MinimalMemberDetail,
    };

    fn make_minimal_flag(position: u16, name_hash: u32) -> MinimalBitflag {
        MinimalBitflag {
            common: CommonBitflag {
                position,
                flags: BitflagFlag(0),
            },
            detail: MinimalMemberDetail { name_hash },
        }
    }

    /// XTypes v1.3 §7.3.4.5 R14: MinimalBitflagSeq must be emitted in
    /// ascending `position` order. Two source Vecs that differ only in
    /// element order must produce byte-identical wire output, and the
    /// decoded positions must come back sorted.
    #[test]
    fn minimal_bitmask_flags_emit_sorted_by_position() {
        let header = MinimalBitmaskHeader {
            bit_bound: 32,
            detail: MinimalTypeDetail::new(),
        };

        let unsorted = MinimalBitmaskType {
            header: header.clone(),
            flag_seq: vec![
                make_minimal_flag(31, 0xFF),
                make_minimal_flag(0, 0x01),
                make_minimal_flag(15, 0x0F),
                make_minimal_flag(7, 0x07),
            ],
        };
        let sorted = MinimalBitmaskType {
            header,
            flag_seq: vec![
                make_minimal_flag(0, 0x01),
                make_minimal_flag(7, 0x07),
                make_minimal_flag(15, 0x0F),
                make_minimal_flag(31, 0xFF),
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
             (R14 enforcement)"
        );

        let (decoded, used) = MinimalBitmaskType::decode_cdr2_le(&buf_unsorted[..len_unsorted])
            .expect("decode round-trip");
        assert_eq!(used, len_unsorted, "decoder consumes full input");
        let positions: Vec<u16> = decoded.flag_seq.iter().map(|f| f.common.position).collect();
        assert_eq!(
            positions,
            vec![0, 7, 15, 31],
            "decoded positions are sorted"
        );
    }

    /// XTypes v1.3 §7.3.4.5 R13: same guarantee for CompleteBitmaskType.
    #[test]
    fn complete_bitmask_flags_emit_sorted_by_position() {
        let header = CompleteBitmaskHeader {
            bit_bound: 16,
            detail: CompleteTypeDetail {
                type_name: "Flags".to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let make = |position: u16, name: &str| CompleteBitflag {
            common: CommonBitflag {
                position,
                flags: BitflagFlag(0),
            },
            detail: CompleteMemberDetail {
                name: name.to_string(),
                ann_builtin: None,
                ann_custom: None,
            },
        };

        let unsorted = CompleteBitmaskType {
            header: header.clone(),
            flag_seq: vec![make(10, "j"), make(2, "c"), make(5, "f")],
        };
        let sorted = CompleteBitmaskType {
            header,
            flag_seq: vec![make(2, "c"), make(5, "f"), make(10, "j")],
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
             (R13 enforcement)"
        );
    }
}
