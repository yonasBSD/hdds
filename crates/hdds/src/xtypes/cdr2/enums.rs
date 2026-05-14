// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Enumeration type definitions
//!
//!
//! Complete and Minimal enumeration types, headers, and literals.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.8.3 (Enumerated Types)

use super::dheader::{decode_dheader_at, encode_dheader_at};
use super::helpers::checked_usize;
use super::primitives::{
    align_offset, decode_i16, decode_i32, decode_u16, decode_u32, encode_i16, encode_i32,
    encode_u16, encode_vec,
};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// CommonEnumeratedLiteral CDR2 Encoding/Decoding
// ============================================================================

// CommonEnumeratedLiteral — `@extensibility(APPENDABLE)` per XTypes v1.3 spec line 13159.
impl Cdr2Encode for CommonEnumeratedLiteral {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload (i32 + u16)
        4 + 3 + 4 + 2
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_i32(self.value, dst, offset)?;
            encode_u16(self.flags.0, dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CommonEnumeratedLiteral {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_common_enumerated_literal_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CommonEnumeratedLiteral decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_common_enumerated_literal_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CommonEnumeratedLiteral, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let value = decode_i32(src, offset)?;
        let flags = EnumeratedLiteralFlag(decode_u16(src, offset)?);
        Ok(CommonEnumeratedLiteral { value, flags })
    })
}

// ============================================================================
// CompleteEnumeratedLiteral / MinimalEnumeratedLiteral CDR2 Encoding/Decoding
// ============================================================================

// Complete/MinimalEnumeratedLiteral — `@extensibility(APPENDABLE)` per spec lines 13167, 13177.
impl Cdr2Encode for CompleteEnumeratedLiteral {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.common.max_cdr2_size() + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.common.encode_cdr2_le_at(dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteEnumeratedLiteral {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_enumerated_literal_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteEnumeratedLiteral decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_complete_enumerated_literal_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteEnumeratedLiteral, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let common = decode_common_enumerated_literal_internal(src, offset)?;
        let detail = CompleteMemberDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteEnumeratedLiteral { common, detail })
    })
}

impl Cdr2Encode for MinimalEnumeratedLiteral {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + self.common.max_cdr2_size() + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            self.common.encode_cdr2_le_at(dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalEnumeratedLiteral {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_enumerated_literal_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalEnumeratedLiteral decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_minimal_enumerated_literal_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalEnumeratedLiteral, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let common = decode_common_enumerated_literal_internal(src, offset)?;
        let detail = MinimalMemberDetail::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalEnumeratedLiteral { common, detail })
    })
}

// ============================================================================
// EnumeratedHeader CDR2 Encoding/Decoding
// ============================================================================

// Complete/MinimalEnumeratedHeader — `@extensibility(APPENDABLE)` per spec lines 13196, 13203.
impl Cdr2Encode for CompleteEnumeratedHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 2 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_i16(self.bit_bound, dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for CompleteEnumeratedHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_complete_enumerated_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for CompleteEnumeratedHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_complete_enumerated_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<CompleteEnumeratedHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let bit_bound = decode_i16(src, offset)?;
        let detail = CompleteTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(CompleteEnumeratedHeader { bit_bound, detail })
    })
}

impl Cdr2Encode for MinimalEnumeratedHeader {
    fn max_cdr2_size(&self) -> usize {
        // DHEADER (4 bytes + up to 3 pad) + payload
        4 + 3 + 2 + self.detail.max_cdr2_size()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_dheader_at(dst, offset, |dst, offset| {
            encode_i16(self.bit_bound, dst, offset)?;
            self.detail.encode_cdr2_le_at(dst, offset)?;
            Ok(())
        })
    }
}

impl Cdr2Decode for MinimalEnumeratedHeader {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        decode_minimal_enumerated_header_internal(src, offset)
    }
}

/// Internal helper that tracks offset for MinimalEnumeratedHeader decoding.
/// `@extensibility(APPENDABLE)` per XTypes v1.3 Sec.7.4.3.4 rule (30).
pub(super) fn decode_minimal_enumerated_header_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<MinimalEnumeratedHeader, CdrError> {
    decode_dheader_at(src, offset, |src, offset| {
        let bit_bound = decode_i16(src, offset)?;
        // MinimalTypeDetail is empty, but decode it for consistency
        let detail = MinimalTypeDetail::decode_cdr2_le_at(src, offset)?;
        Ok(MinimalEnumeratedHeader { bit_bound, detail })
    })
}

// ============================================================================
// CompleteEnumeratedType / MinimalEnumeratedType CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteEnumeratedType {
    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size()
            + 4
            + self
                .literal_seq
                .iter()
                .map(|l| l.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.literal_seq, dst, offset, |literal, dst, offset| {
            literal.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteEnumeratedType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header using internal helper
        let header = decode_complete_enumerated_header_internal(src, offset)?;

        // Decode literal_seq using internal helper for proper offset tracking
        let literal_len = decode_u32(src, offset)?;
        let capacity = checked_usize(literal_len, "enumeration literal sequence length")?;
        let mut literal_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            let literal = decode_complete_enumerated_literal_internal(src, offset)?;
            literal_seq.push(literal);
        }

        Ok(CompleteEnumeratedType {
            header,
            literal_seq,
        })
    }
}

impl Cdr2Encode for MinimalEnumeratedType {
    fn max_cdr2_size(&self) -> usize {
        self.header.max_cdr2_size()
            + 4
            + self
                .literal_seq
                .iter()
                .map(|l| l.max_cdr2_size())
                .sum::<usize>()
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        self.header.encode_cdr2_le_at(dst, offset)?;
        encode_vec(&self.literal_seq, dst, offset, |literal, dst, offset| {
            literal.encode_cdr2_le_at(dst, offset)
        })?;
        Ok(())
    }
}

impl Cdr2Decode for MinimalEnumeratedType {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        // Decode header using internal helper
        let header = decode_minimal_enumerated_header_internal(src, offset)?;

        // Decode literal_seq using internal helper for proper offset tracking
        let literal_len = decode_u32(src, offset)?;
        let capacity = checked_usize(literal_len, "minimal enumeration literal sequence length")?;
        let mut literal_seq = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            // Align each element to 4 bytes (CDR2 struct alignment in sequences)
            *offset = align_offset(*offset, 4);
            let literal = decode_minimal_enumerated_literal_internal(src, offset)?;
            literal_seq.push(literal);
        }

        Ok(MinimalEnumeratedType {
            header,
            literal_seq,
        })
    }
}
