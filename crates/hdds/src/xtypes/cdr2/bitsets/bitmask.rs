// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Bitmask encoding for Complete/Minimal types.
//!

use super::super::helpers::checked_usize;
use super::super::primitives::{align_offset, decode_i16, decode_u32, encode_i16, encode_vec};
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
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

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
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

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
        // Conservative estimate
        self.header.max_cdr2_size()
            + 4
            + self
                .flag_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.flag_seq, dst, offset, |flag, dst, offset| {
            flag.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteBitmaskType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header using internal helper
        let header = decode_complete_bitmask_header_internal(src, offset)?;

        // Decode flag_seq using internal helper for proper offset tracking
        let flag_len = decode_u32(src, offset)?;
        let capacity = checked_usize(flag_len, "bitflag sequence length")?;
        let mut flag_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            let flag = decode_complete_bitflag_internal(src, offset)?;
            flag_seq.push(flag);
        }

        Ok(CompleteBitmaskType { header, flag_seq })
    }
}

impl Cdr2Encode for MinimalBitmaskType {
    fn max_cdr2_size(&self) -> usize {
        // Conservative estimate
        self.header.max_cdr2_size()
            + 4
            + self
                .flag_seq
                .iter()
                .map(|f| f.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.flag_seq, dst, offset, |flag, dst, offset| {
            flag.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalBitmaskType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header using internal helper
        let header = decode_minimal_bitmask_header_internal(src, offset)?;

        // Decode flag_seq using internal helper for proper offset tracking
        let flag_len = decode_u32(src, offset)?;
        let capacity = checked_usize(flag_len, "minimal bitflag sequence length")?;
        let mut flag_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            let flag = decode_minimal_bitflag_internal(src, offset)?;
            flag_seq.push(flag);
        }

        Ok(MinimalBitmaskType { header, flag_seq })
    }
}
