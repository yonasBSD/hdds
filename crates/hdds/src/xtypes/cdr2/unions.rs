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
use super::primitives::{decode_u16, encode_u16, encode_vec};
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
        encode_vec(&self.member_seq, dst, offset, |member, dst, offset| {
            member.encode_cdr2_le_at(dst, offset)
        })?;
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
        encode_vec(&self.member_seq, dst, offset, |member, dst, offset| {
            member.encode_cdr2_le_at(dst, offset)
        })?;
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
