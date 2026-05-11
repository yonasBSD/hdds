// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Struct type definitions
//!
//! Complete and Minimal struct types, headers, and members.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.1 (Struct Types)

use super::members::{
    decode_complete_struct_member_internal, decode_minimal_struct_member_internal,
};
use super::primitives::{decode_option, decode_u16, encode_option, encode_u16, encode_vec};
use super::type_identifier::decode_type_identifier_internal;
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// StructHeader CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteStructHeader {
    fn max_cdr2_size(&self) -> usize {
        32 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_option(&self.base_type, dst, offset, |type_id, dst, offset| {
            type_id.encode_cdr2_le_at(dst, offset)
        })?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteStructHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_struct_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteStructHeader decoding
pub(super) fn decode_complete_struct_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteStructHeader, CdrError> {
    let base_type = decode_option(src, offset, |src, offset| {
        decode_type_identifier_internal(src, offset)
    })?;

    let detail = super::helpers::decode_detail_with_reencoding::<CompleteTypeDetail>(src, offset)?;

    Ok(CompleteStructHeader { base_type, detail })
}

impl Cdr2Encode for MinimalStructHeader {
    fn max_cdr2_size(&self) -> usize {
        32 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_option(&self.base_type, dst, offset, |type_id, dst, offset| {
            type_id.encode_cdr2_le_at(dst, offset)
        })?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalStructHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_struct_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalStructHeader decoding
pub(super) fn decode_minimal_struct_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalStructHeader, CdrError> {
    let base_type = decode_option(src, offset, |src, offset| {
        decode_type_identifier_internal(src, offset)
    })?;

    // MinimalTypeDetail is empty (encodes as 0 bytes)
    let detail = super::helpers::decode_detail_with_reencoding::<MinimalTypeDetail>(src, offset)?;

    Ok(MinimalStructHeader { base_type, detail })
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
        encode_vec(&self.member_seq, dst, offset, |member, dst, offset| {
            member.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteStructType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

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
        encode_vec(&self.member_seq, dst, offset, |member, dst, offset| {
            member.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalStructType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

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
