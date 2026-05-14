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
    fn max_cdr2_size(&self) -> usize {
        4 // u16 + 2-byte alignment padding
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u16(self.0, dst, offset)
    }
}

impl Cdr2Decode for CollectionElementFlag {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let flags = decode_u16(src, offset)?;
        Ok(Self(flags))
    }
}

// ============================================================================
// CompleteCollectionHeader / MinimalCollectionHeader CDR2
// ============================================================================

impl Cdr2Encode for CompleteCollectionHeader {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_collection_header_internal(src, offset)
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
    let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;

    Ok(CompleteCollectionHeader { bound, detail })
}

impl Cdr2Encode for MinimalCollectionHeader {
    fn max_cdr2_size(&self) -> usize {
        4
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u32(self.bound, dst, offset)
    }
}

impl Cdr2Decode for MinimalCollectionHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let bound = decode_u32(src, offset)?;
        Ok(Self { bound })
    }
}

// ============================================================================
// CompleteCollectionElement / MinimalCollectionElement CDR2
// ============================================================================

impl Cdr2Encode for CompleteCollectionElement {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_collection_element_internal(src, offset)
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_collection_element_internal(src, offset)
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = decode_complete_collection_header_internal(src, offset)?;

        // Decode element
        let element = decode_complete_collection_element_internal(src, offset)?;

        Ok(CompleteSequenceType { header, element })
    }
}

impl Cdr2Encode for MinimalSequenceType {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = MinimalCollectionHeader::decode_cdr2_le_at(src, offset)?;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, offset)?;

        Ok(MinimalSequenceType { header, element })
    }
}

// ============================================================================
// CompleteArrayType / MinimalArrayType CDR2 (0x07)
// ============================================================================

impl Cdr2Encode for CompleteArrayType {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = CompleteCollectionHeader::decode_cdr2_le_at(src, offset)?;

        // Decode element
        let element = CompleteCollectionElement::decode_cdr2_le_at(src, offset)?;

        // Decode bound_seq
        let bounds_count = decode_u32(src, offset)?;
        let capacity = checked_usize(bounds_count, "collection bound sequence length")?;
        let mut bound_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            let bound = decode_u32(src, offset)?;
            bound_seq.push(bound);
        }

        Ok(CompleteArrayType {
            header,
            element,
            bound_seq,
        })
    }
}

impl Cdr2Encode for MinimalArrayType {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = MinimalCollectionHeader::decode_cdr2_le_at(src, offset)?;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, offset)?;

        // Decode bound_seq
        let bounds_count = decode_u32(src, offset)?;
        let capacity = checked_usize(bounds_count, "collection bound sequence length")?;
        let mut bound_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            let bound = decode_u32(src, offset)?;
            bound_seq.push(bound);
        }

        Ok(MinimalArrayType {
            header,
            element,
            bound_seq,
        })
    }
}

// ============================================================================
// CompleteMapType / MinimalMapType CDR2 (0x08)
// ============================================================================

impl Cdr2Encode for CompleteMapType {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = CompleteCollectionHeader::decode_cdr2_le_at(src, offset)?;

        // Decode key
        let key = CompleteCollectionElement::decode_cdr2_le_at(src, offset)?;

        // Decode element
        let element = CompleteCollectionElement::decode_cdr2_le_at(src, offset)?;

        Ok(CompleteMapType {
            header,
            key,
            element,
        })
    }
}

impl Cdr2Encode for MinimalMapType {
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
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header
        let header = MinimalCollectionHeader::decode_cdr2_le_at(src, offset)?;

        // Decode key
        let key = decode_minimal_collection_element_internal(src, offset)?;

        // Decode element
        let element = decode_minimal_collection_element_internal(src, offset)?;

        Ok(MinimalMapType {
            header,
            key,
            element,
        })
    }
}
