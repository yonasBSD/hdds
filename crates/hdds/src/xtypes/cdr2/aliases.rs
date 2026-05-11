// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Alias type definitions
//!
//! Complete and Minimal alias types (type aliases/typedefs).
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.6 (Alias Types)

use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::TypeIdentifier;

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// CompleteAliasType / MinimalAliasType CDR2 (0x09)
// ============================================================================

// Alias Headers
impl Cdr2Encode for CompleteAliasHeader {
    fn max_cdr2_size(&self) -> usize {
        self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.detail.encode_cdr2_le_at(dst, offset)
    }
}

impl Cdr2Decode for CompleteAliasHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteAliasHeader { detail })
    }
}

impl Cdr2Encode for MinimalAliasHeader {
    fn max_cdr2_size(&self) -> usize {
        self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.detail.encode_cdr2_le_at(dst, offset)
    }
}

impl Cdr2Decode for MinimalAliasHeader {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalAliasHeader { detail })
    }
}

// Alias Bodies
impl Cdr2Encode for CommonAliasBody {
    fn max_cdr2_size(&self) -> usize {
        self.related_flags.max_cdr2_size() + self.related_type.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.related_flags.encode_cdr2_le_at(dst, offset)?;
        self.related_type.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CommonAliasBody {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let related_flags = TypeRelationFlag::decode_cdr2_le_at(src, offset)?;
        let related_type = TypeIdentifier::decode_cdr2_le_at(src, offset)?;
        Ok(CommonAliasBody {
            related_flags,
            related_type,
        })
    }
}

impl Cdr2Encode for CompleteAliasBody {
    fn max_cdr2_size(&self) -> usize {
        self.common.max_cdr2_size() + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.common.encode_cdr2_le_at(dst, offset)?;
        self.detail.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteAliasBody {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let common = CommonAliasBody::decode_cdr2_le_at(src, offset)?;
        let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteAliasBody { common, detail })
    }
}

impl Cdr2Encode for MinimalAliasBody {
    fn max_cdr2_size(&self) -> usize {
        self.common.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.common.encode_cdr2_le_at(dst, offset)
    }
}

impl Cdr2Decode for MinimalAliasBody {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let common = CommonAliasBody::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalAliasBody { common })
    }
}

// Complete/Minimal AliasType
impl Cdr2Encode for CompleteAliasType {
    fn max_cdr2_size(&self) -> usize {
        self.alias_flags.max_cdr2_size() + self.header.max_cdr2_size() + self.body.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.alias_flags.encode_cdr2_le_at(dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.body.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteAliasType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let alias_flags = AliasTypeFlag::decode_cdr2_le_at(src, offset)?;
        let header = CompleteAliasHeader::decode_cdr2_le_at(src, offset)?;
        let body = CompleteAliasBody::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteAliasType {
            alias_flags,
            header,
            body,
        })
    }
}

impl Cdr2Encode for MinimalAliasType {
    fn max_cdr2_size(&self) -> usize {
        self.alias_flags.max_cdr2_size() + self.header.max_cdr2_size() + self.body.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.alias_flags.encode_cdr2_le_at(dst, offset)?;
        self.header.encode_cdr2_le_at(dst, offset)?;
        self.body.encode_cdr2_le_at(dst, offset)?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalAliasType {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let value = Self::decode_cdr2_le_at(src, &mut offset)?;
        Ok((value, offset))
    }

    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let alias_flags = AliasTypeFlag::decode_cdr2_le_at(src, offset)?;
        let header = MinimalAliasHeader::decode_cdr2_le_at(src, offset)?;
        let body = MinimalAliasBody::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalAliasType {
            alias_flags,
            header,
            body,
        })
    }
}
