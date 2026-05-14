// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Type and member detail metadata
//!
//!
//! TypeDetail and MemberDetail provide optional descriptive information.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.6 (TypeDetail)

use super::primitives::{
    decode_option, decode_string, decode_u32, encode_option, encode_string, encode_u32,
};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use std::convert::TryFrom;

#[allow(clippy::wildcard_imports)]
use crate::xtypes::type_object::*;

// ============================================================================
// Detail Structures CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for CompleteTypeDetail {
    fn max_cdr2_size(&self) -> usize {
        // Conservative estimate: name (256) + annotations (512)
        1024
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_string(&self.type_name, dst, offset)?;
        encode_option(&self.ann_builtin, dst, offset, |ann, dst, offset| {
            encode_option(&ann.verbatim, dst, offset, |s: &String, dst, offset| {
                encode_string(s.as_str(), dst, offset)
            })
        })?;
        encode_option(&self.ann_custom, dst, offset, |vec, dst, offset| {
            let len = u32::try_from(vec.len())
                .map_err(|_| CdrError::Other("Annotation list exceeds u32::MAX entries".into()))?;
            encode_u32(len, dst, offset)?;
            Ok(())
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteTypeDetail {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let type_name = decode_string(src, offset)?;

        let ann_builtin = decode_option(src, offset, |src, offset| {
            let verbatim = decode_option(src, offset, decode_string)?;
            Ok(AppliedBuiltinTypeAnnotations { verbatim })
        })?;

        let ann_custom = decode_option(src, offset, |src, offset| {
            let _len = decode_u32(src, offset)?;
            // For now, return empty vec (will add AppliedAnnotation decoding later)
            Ok(vec![])
        })?;

        Ok(CompleteTypeDetail {
            type_name,
            ann_builtin,
            ann_custom,
        })
    }
}

impl Cdr2Encode for MinimalTypeDetail {
    fn max_cdr2_size(&self) -> usize {
        0
    }

    fn encode_cdr2_le_at(&self, _dst: &mut [u8], _offset: &mut usize) -> Result<(), CdrError> {
        Ok(())
    }
}

impl Cdr2Decode for MinimalTypeDetail {
    fn decode_cdr2_le(_src: &[u8]) -> Result<(Self, usize), CdrError> {
        Ok((MinimalTypeDetail {}, 0))
    }

    fn decode_cdr2_le_at(_src: &[u8], _offset: &mut usize) -> Result<Self, CdrError> {
        // MinimalTypeDetail is empty — no bytes consumed, offset unchanged.
        Ok(MinimalTypeDetail {})
    }
}

impl Cdr2Encode for CompleteMemberDetail {
    // @audit-ok: Sequential encoding (cyclo 12, cogni 0) - linear option encoding without branching logic

    fn max_cdr2_size(&self) -> usize {
        512
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_string(&self.name, dst, offset)?;
        encode_option(&self.ann_builtin, dst, offset, |ann, dst, offset| {
            encode_option(&ann.unit, dst, offset, |s: &String, dst, offset| {
                encode_string(s.as_str(), dst, offset)
            })?;
            encode_option(&ann.hash_id, dst, offset, |s: &String, dst, offset| {
                encode_string(s.as_str(), dst, offset)
            })
        })?;
        encode_option(&self.ann_custom, dst, offset, |vec, dst, offset| {
            let len = u32::try_from(vec.len()).map_err(|_| {
                CdrError::Other("Member annotation list exceeds u32::MAX entries".into())
            })?;
            encode_u32(len, dst, offset)?;
            Ok(())
        })?;
        Ok(())
    }
}

impl Cdr2Decode for CompleteMemberDetail {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let name = decode_string(src, offset)?;

        let ann_builtin = decode_option(src, offset, |src, offset| {
            let unit = decode_option(src, offset, decode_string)?;
            let hash_id = decode_option(src, offset, decode_string)?;
            Ok(AppliedBuiltinMemberAnnotations {
                unit,
                min: None,
                max: None,
                hash_id,
            })
        })?;

        let ann_custom = decode_option(src, offset, |src, offset| {
            let _len = decode_u32(src, offset)?;
            Ok(vec![])
        })?;

        Ok(CompleteMemberDetail {
            name,
            ann_builtin,
            ann_custom,
        })
    }
}

impl Cdr2Encode for MinimalMemberDetail {
    fn max_cdr2_size(&self) -> usize {
        8 // 4 bytes + alignment
    }

    fn encode_cdr2_le_at(&self, dst: &mut [u8], offset: &mut usize) -> Result<(), CdrError> {
        encode_u32(self.name_hash, dst, offset)
    }
}

impl Cdr2Decode for MinimalMemberDetail {
    fn decode_cdr2_le_at(src: &[u8], offset: &mut usize) -> Result<Self, CdrError> {
        let name_hash = decode_u32(src, offset)?;
        Ok(MinimalMemberDetail { name_hash })
    }
}
