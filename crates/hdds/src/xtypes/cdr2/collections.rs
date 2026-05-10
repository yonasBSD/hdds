// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Collection type definitions
//!
//!
//! Sequence, array, map, and string types.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.5 (Collection Types)

use super::helpers::checked_usize;
use super::primitives::{decode_u16, decode_u32, encode_u16, encode_u32, encode_vec};
use super::type_identifier::decode_type_identifier_internal;
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// Sequence/Array/Map Collection Support (0x06, 0x07, 0x08) CDR2
// ============================================================================

/// CollectionElementFlag - Collection element flags (u16)
impl Cdr2Encode for CollectionElementFlag {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        encode_u16(self.0, dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        4 // u16 + 2-byte alignment padding
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.0, dst, offset)
    }
}

impl Cdr2Decode for CollectionElementFlag {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let flags = decode_u16(src, &mut offset)?;
        Ok((Self(flags), offset))
    }
}

// ============================================================================
// CompleteCollectionHeader / MinimalCollectionHeader CDR2
// ============================================================================

impl Cdr2Encode for CompleteCollectionHeader {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        4 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u32(self.bound, dst, offset)?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteCollectionHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let result = decode_complete_collection_header_internal(src, &mut offset)?;
        Ok((result, offset))
    }
}

/// Internal helper for CompleteCollectionHeader decoding with offset tracking
pub(super) fn decode_complete_collection_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteCollectionHeader, CdrError> {
    // Decode bound
    let bound = decode_u32(src, offset)?;

    // Decode detail
    let (detail, used) = CompleteTypeDetail::decode_cdr2_le(&src[*offset..])?;
    *offset += used;

    Ok(CompleteCollectionHeader { bound, detail })
}

impl Cdr2Encode for MinimalCollectionHeader {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        encode_u32(self.bound, dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        4
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u32(self.bound, dst, offset)
    }
}

impl Cdr2Decode for MinimalCollectionHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let bound = decode_u32(src, &mut offset)?;
        Ok((Self { bound }, offset))
    }
}

// ============================================================================
// CompleteCollectionElement / MinimalCollectionElement CDR2
// ============================================================================

impl Cdr2Encode for CompleteCollectionElement {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        4 + self.type_id.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.flags.0, dst, offset)?;
        self.type_id.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteCollectionElement {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let result = decode_complete_collection_element_internal(src, &mut offset)?;
        Ok((result, offset))
    }
}

/// Internal helper for CompleteCollectionElement decoding with offset tracking
pub(super) fn decode_complete_collection_element_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteCollectionElement, CdrError> {
    // Decode flags
    let flags_value = decode_u16(src, offset)?;
    let flags = CollectionElementFlag(flags_value);

    // Decode type_id
    let type_id = decode_type_identifier_internal(src, offset)?;

    Ok(CompleteCollectionElement { flags, type_id })
}

impl Cdr2Encode for MinimalCollectionElement {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        4 + self.type_id.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.flags.0, dst, offset)?;
        self.type_id.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalCollectionElement {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let result = decode_minimal_collection_element_internal(src, &mut offset)?;
        Ok((result, offset))
    }
}

/// Internal helper for MinimalCollectionElement decoding with offset tracking
pub(super) fn decode_minimal_collection_element_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalCollectionElement, CdrError> {
    // Decode flags
    let flags_value = decode_u16(src, offset)?;
    let flags = CollectionElementFlag(flags_value);

    // Decode type_id
    let type_id = decode_type_identifier_internal(src, offset)?;

    Ok(MinimalCollectionElement { flags, type_id })
}

// ============================================================================
// CompleteSequenceType / MinimalSequenceType CDR2 (0x06)
// ============================================================================

impl Cdr2Encode for CompleteSequenceType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.element.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteSequenceType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let header = decode_complete_collection_header_internal(src, &mut offset)?;

        // Decode element
        let element = decode_complete_collection_element_internal(src, &mut offset)?;

        Ok((CompleteSequenceType { header, element }, offset))
    }
}

impl Cdr2Encode for MinimalSequenceType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.element.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalSequenceType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let (header, used) = MinimalCollectionHeader::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, &mut offset)?;

        Ok((MinimalSequenceType { header, element }, offset))
    }
}

// ============================================================================
// CompleteArrayType / MinimalArrayType CDR2 (0x07)
// ============================================================================

impl Cdr2Encode for CompleteArrayType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.element.max_cdr2_size() + 4 + (self.bound_seq.len() * 4)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.bound_seq, dst, offset, |item, dst, offset| {
            encode_u32(*item, dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteArrayType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let (header, used) = CompleteCollectionHeader::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode element
        let (element, used) = CompleteCollectionElement::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode bound_seq
        let bounds_count = decode_u32(src, &mut offset)?;
        let capacity = checked_usize(bounds_count, "collection bound sequence length")?;
        let mut bound_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            let bound = decode_u32(src, &mut offset)?;
            bound_seq.push(bound);
        }

        Ok((
            CompleteArrayType {
                header,
                element,
                bound_seq,
            },
            offset,
        ))
    }
}

impl Cdr2Encode for MinimalArrayType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.element.max_cdr2_size() + 4 + (self.bound_seq.len() * 4)
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.bound_seq, dst, offset, |item, dst, offset| {
            encode_u32(*item, dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalArrayType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let (header, used) = MinimalCollectionHeader::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, &mut offset)?;

        // Decode bound_seq
        let bounds_count = decode_u32(src, &mut offset)?;
        let capacity = checked_usize(bounds_count, "collection bound sequence length")?;
        let mut bound_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            let bound = decode_u32(src, &mut offset)?;
            bound_seq.push(bound);
        }

        Ok((
            MinimalArrayType {
                header,
                element,
                bound_seq,
            },
            offset,
        ))
    }
}

// ============================================================================
// CompleteMapType / MinimalMapType CDR2 (0x08)
// ============================================================================

impl Cdr2Encode for CompleteMapType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.key.max_cdr2_size() + self.element.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.key.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteMapType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let (header, used) = CompleteCollectionHeader::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode key
        let (key, used) = CompleteCollectionElement::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode element
        let (element, used) = CompleteCollectionElement::decode_cdr2_le(&src[offset..])?;
        offset += used;

        Ok((
            CompleteMapType {
                header,
                key,
                element,
            },
            offset,
        ))
    }
}

impl Cdr2Encode for MinimalMapType {
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;
        self.encode_cdr2_le_at(dst, &mut offset)?;
        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size() + self.key.max_cdr2_size() + self.element.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.key.encode_cdr2_le_at(dst, offset)?;
        self.element.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalMapType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;

        // Decode header
        let (header, used) = MinimalCollectionHeader::decode_cdr2_le(&src[offset..])?;
        offset += used;

        // Decode key
        let key = decode_minimal_collection_element_internal(src, &mut offset)?;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, &mut offset)?;

        Ok((
            MinimalMapType {
                header,
                key,
                element,
            },
            offset,
        ))
    }
}
