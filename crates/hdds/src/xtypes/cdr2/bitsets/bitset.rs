// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Bitset CDR2 serialization for different bit widths (u8/u16/u32/u64).
//!
//!

use super::super::dheader::{decode_dheader_at, encode_dheader_at};
use super::super::helpers::checked_usize;
use super::super::primitives::{
    align_offset, decode_u16, decode_u32, decode_u8, encode_u16, encode_u8, encode_vec,
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

impl Cdr2Encode for CompleteBitsetHeader {
    fn max_cdr2_size(&self) -> usize {
        1 + 32 + self.detail.max_cdr2_size() // flag + optional TypeIdentifier + detail
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        if let Some(ref base) = self.base_type {
            encode_u8(1, dst, offset)?;
            base.encode_cdr2_le_at(dst, offset)?;
        } else {
            encode_u8(0, dst, offset)?;
        }
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteBitsetHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_bitset_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteBitsetHeader decoding
pub(super) fn decode_complete_bitset_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteBitsetHeader, CdrError> {
    // Decode base_type (Option<TypeIdentifier>)
    let base_type_present = decode_u8(src, offset)?;
    let base_type = if base_type_present == 1 {
        Some(decode_type_identifier_internal(src, offset)?)
    } else {
        None
    };

    let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;

    Ok(CompleteBitsetHeader { base_type, detail })
}

impl Cdr2Encode for MinimalBitsetHeader {
    fn max_cdr2_size(&self) -> usize {
        1 + 32 + self.detail.max_cdr2_size() // flag + optional TypeIdentifier + detail
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        if let Some(ref base) = self.base_type {
            encode_u8(1, dst, offset)?;
            base.encode_cdr2_le_at(dst, offset)?;
        } else {
            encode_u8(0, dst, offset)?;
        }
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalBitsetHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_bitset_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalBitsetHeader decoding
pub(super) fn decode_minimal_bitset_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalBitsetHeader, CdrError> {
    // Decode base_type (Option<TypeIdentifier>)
    let base_type_present = decode_u8(src, offset)?;
    let base_type = if base_type_present == 1 {
        Some(decode_type_identifier_internal(src, offset)?)
    } else {
        None
    };

    // MinimalTypeDetail is empty, but decode it for consistency
    let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;

    Ok(MinimalBitsetHeader { base_type, detail })
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
            encode_vec(&self.field_seq, dst, offset, |field, dst, offset| {
                field.encode_cdr2_le_at(dst, offset)
            })?;
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
            encode_vec(&self.field_seq, dst, offset, |field, dst, offset| {
                field.encode_cdr2_le_at(dst, offset)
            })?;
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
