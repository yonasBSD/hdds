// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! TypeIdentifier - Core type identification for XTypes
//!
//!
//! TypeIdentifier uniquely identifies a type in the DDS type system.
//!
//! # References
//! - XTypes v1.3 Spec: Section 7.3.4.4 (TypeIdentifier)

use super::primitives::{decode_i32, decode_u32, decode_u8, encode_i32, encode_u32, encode_u8};
use crate::core::ser::traits::{Cdr2Decode, Cdr2Encode, CdrError};
use crate::xtypes::discriminators::{
    EK_COMPLETE, EK_MINIMAL, TI_STRING16_LARGE, TI_STRING16_SMALL, TI_STRING8_LARGE,
    TI_STRING8_SMALL, TI_STRONGLY_CONNECTED_COMPONENT,
};
use crate::xtypes::{TypeIdentifier, TypeKind};

// ============================================================================
// TypeIdentifier CDR2 Encoding/Decoding
// ============================================================================

impl Cdr2Encode for TypeIdentifier {
    /// Wire encoding per OMG DDS-XTypes v1.3 §7.3.4.4.
    ///
    /// Each variant produces the discriminator octet defined in the IDL
    /// `union TypeIdentifier switch (octet)` declaration, followed by the
    /// variant payload. Primitive types use the `TypeKind` octet directly
    /// as the discriminator and carry no payload.
    fn encode_cdr2_le(&self, dst: &mut [u8]) -> Result<usize, CdrError> {
        let mut offset = 0;

        match self {
            // §7.3.4.4: primitive `TypeIdentifier`s are encoded as the
            // `TypeKind` octet alone (TK_BOOLEAN..TK_CHAR16 in the
            // 0x00..0x11 range), with no payload following.
            TypeIdentifier::Primitive(kind) => {
                encode_u8(kind.to_u8(), dst, &mut offset)?;
            }
            // §7.3.4.4: `case TI_STRING8_SMALL: StringSTypeDefn { SBound bound; };`
            TypeIdentifier::StringSmall { bound } => {
                encode_u8(TI_STRING8_SMALL, dst, &mut offset)?;
                encode_u8(*bound, dst, &mut offset)?;
            }
            // §7.3.4.4: `case TI_STRING8_LARGE: StringLTypeDefn { LBound bound; };`
            TypeIdentifier::StringLarge { bound } => {
                encode_u8(TI_STRING8_LARGE, dst, &mut offset)?;
                encode_u32(*bound, dst, &mut offset)?;
            }
            // §7.3.4.4: `case TI_STRING16_SMALL: StringSTypeDefn { SBound bound; };`
            TypeIdentifier::WStringSmall { bound } => {
                encode_u8(TI_STRING16_SMALL, dst, &mut offset)?;
                encode_u8(*bound, dst, &mut offset)?;
            }
            // §7.3.4.4: `case TI_STRING16_LARGE: StringLTypeDefn { LBound bound; };`
            TypeIdentifier::WStringLarge { bound } => {
                encode_u8(TI_STRING16_LARGE, dst, &mut offset)?;
                encode_u32(*bound, dst, &mut offset)?;
            }
            // §7.3.4.4: `case EK_MINIMAL: EquivalenceHash equivalence_hash;`
            TypeIdentifier::Minimal(hash) => {
                encode_u8(EK_MINIMAL, dst, &mut offset)?;
                if offset + 14 > dst.len() {
                    return Err(CdrError::BufferTooSmall);
                }
                dst[offset..offset + 14].copy_from_slice(hash.as_bytes());
                offset += 14;
            }
            // §7.3.4.4: `case EK_COMPLETE: EquivalenceHash equivalence_hash;`
            TypeIdentifier::Complete(hash) => {
                encode_u8(EK_COMPLETE, dst, &mut offset)?;
                if offset + 14 > dst.len() {
                    return Err(CdrError::BufferTooSmall);
                }
                dst[offset..offset + 14].copy_from_slice(hash.as_bytes());
                offset += 14;
            }
            // §7.3.4.4: `case TI_STRONGLY_CONNECTED_COMPONENT:
            //               StronglyConnectedComponentId sc_component_id;`
            //
            // The spec layout for `StronglyConnectedComponentId` is
            // `@extensibility(APPENDABLE)` and wraps a `TypeObjectHashId`
            // union (1 discriminator + 14-byte hash) plus two `long`
            // fields. This emitter writes the 14-byte hash + two `i32`s
            // without the spec's outer DHEADER nor the inner
            // `TypeObjectHashId` union discriminator; payload-shape
            // alignment with §7.3.4.4 is tracked separately and only the
            // top-level discriminator is migrated here.
            TypeIdentifier::StronglyConnected(sc) => {
                encode_u8(TI_STRONGLY_CONNECTED_COMPONENT, dst, &mut offset)?;
                if offset + 14 > dst.len() {
                    return Err(CdrError::BufferTooSmall);
                }
                dst[offset..offset + 14].copy_from_slice(sc.sc_component_id.as_bytes());
                offset += 14;
                encode_i32(sc.scc_length, dst, &mut offset)?;
                encode_i32(sc.scc_index, dst, &mut offset)?;
            }
            // `Inline` is not part of the OMG DDS-XTypes v1.3 §7.3.4.4
            // `TypeIdentifier` union and has no on-the-wire encoding.
            // Reject to surface any caller that still constructs it so
            // the variant can be removed in a follow-up.
            TypeIdentifier::Inline(_) => {
                return Err(CdrError::Other(
                    "TypeIdentifier::Inline has no spec-compliant wire encoding".to_string(),
                ));
            }
        }

        Ok(offset)
    }

    fn max_cdr2_size(&self) -> usize {
        match self {
            // Inline produces no wire bytes (encode returns Err); reserve
            // a conservative bound so callers that pre-size buffers from
            // `max_cdr2_size` do not over-allocate beyond the spec
            // variants below.
            TypeIdentifier::Inline(_) => 1,
            // Discriminator (1) + worst case (StronglyConnected: 14-byte
            // hash + alignment padding + two i32 fields).
            _ => 32,
        }
    }
}

impl Cdr2Decode for TypeIdentifier {
    fn decode_cdr2_le(src: &[u8]) -> Result<(Self, usize), CdrError> {
        let mut offset = 0;
        let result = decode_type_identifier_internal(src, &mut offset)?;
        Ok((result, offset))
    }
}

/// Wire decoding of a `TypeIdentifier` per OMG DDS-XTypes v1.3 §7.3.4.4.
///
/// Symmetric with [`Cdr2Encode::encode_cdr2_le`]: the discriminator octet
/// dispatches to the matching variant per the IDL `union TypeIdentifier
/// switch (octet)` declaration. Primitive `TypeIdentifier`s are decoded
/// directly from the `TypeKind` octet (TK_NONE..TK_CHAR16, 0x00..0x11)
/// with no payload.
///
/// Bytes outside the spec discriminator set surface as `CdrError::Other`.
/// In particular, `EK_BOTH` (0xF3) is not currently emitted by HDDS and is
/// rejected here rather than silently aliased to `Minimal` or `Complete`.
pub(super) fn decode_type_identifier_internal(
    src: &[u8],
    offset: &mut usize,
) -> Result<TypeIdentifier, CdrError> {
    let discriminator = decode_u8(src, offset)?;

    match discriminator {
        // §7.3.4.4: when a `TypeIdentifier` carries a primitive type, the
        // discriminator octet IS the `TypeKind` value (TK_NONE..TK_CHAR16,
        // 0x00..0x11) and there is no payload following.
        0x00..=0x11 => {
            let kind = TypeKind::from_u8(discriminator).ok_or_else(|| {
                CdrError::Other(format!(
                    "TypeIdentifier primitive discriminator 0x{:02X} is not a known TypeKind",
                    discriminator
                ))
            })?;
            Ok(TypeIdentifier::Primitive(kind))
        }
        // §7.3.4.4: `case TI_STRING8_SMALL: StringSTypeDefn { SBound bound; };`
        TI_STRING8_SMALL => {
            let bound = decode_u8(src, offset)?;
            Ok(TypeIdentifier::StringSmall { bound })
        }
        // §7.3.4.4: `case TI_STRING8_LARGE: StringLTypeDefn { LBound bound; };`
        TI_STRING8_LARGE => {
            let bound = decode_u32(src, offset)?;
            Ok(TypeIdentifier::StringLarge { bound })
        }
        // §7.3.4.4: `case TI_STRING16_SMALL: StringSTypeDefn { SBound bound; };`
        TI_STRING16_SMALL => {
            let bound = decode_u8(src, offset)?;
            Ok(TypeIdentifier::WStringSmall { bound })
        }
        // §7.3.4.4: `case TI_STRING16_LARGE: StringLTypeDefn { LBound bound; };`
        TI_STRING16_LARGE => {
            let bound = decode_u32(src, offset)?;
            Ok(TypeIdentifier::WStringLarge { bound })
        }
        // §7.3.4.4: `case TI_STRONGLY_CONNECTED_COMPONENT:
        //               StronglyConnectedComponentId sc_component_id;`
        // Symmetric with the encoder: 14-byte hash + two `i32` fields,
        // without the spec's outer DHEADER nor the inner
        // `TypeObjectHashId` union discriminator. Payload-shape alignment
        // with §7.3.4.4 is tracked separately.
        TI_STRONGLY_CONNECTED_COMPONENT => {
            if *offset + 14 > src.len() {
                return Err(CdrError::UnexpectedEof);
            }
            let hash_bytes: [u8; 14] = src[*offset..*offset + 14]
                .try_into()
                .map_err(|_| CdrError::UnexpectedEof)?;
            *offset += 14;

            let scc_length = decode_i32(src, offset)?;
            let scc_index = decode_i32(src, offset)?;

            Ok(TypeIdentifier::StronglyConnected(
                crate::xtypes::type_id::StronglyConnectedComponentId {
                    sc_component_id: hash_bytes.into(),
                    scc_length,
                    scc_index,
                },
            ))
        }
        // §7.3.4.4: `case EK_MINIMAL: EquivalenceHash equivalence_hash;`
        EK_MINIMAL => {
            if *offset + 14 > src.len() {
                return Err(CdrError::UnexpectedEof);
            }
            let hash_bytes: [u8; 14] = src[*offset..*offset + 14]
                .try_into()
                .map_err(|_| CdrError::UnexpectedEof)?;
            *offset += 14;
            Ok(TypeIdentifier::Minimal(hash_bytes.into()))
        }
        // §7.3.4.4: `case EK_COMPLETE: EquivalenceHash equivalence_hash;`
        EK_COMPLETE => {
            if *offset + 14 > src.len() {
                return Err(CdrError::UnexpectedEof);
            }
            let hash_bytes: [u8; 14] = src[*offset..*offset + 14]
                .try_into()
                .map_err(|_| CdrError::UnexpectedEof)?;
            *offset += 14;
            Ok(TypeIdentifier::Complete(hash_bytes.into()))
        }
        // §7.3.4.4 reserves `EK_BOTH` (0xF3) as a forward-compat marker
        // for types whose Minimal and Complete `TypeObject`s share a
        // hash. HDDS does not currently emit this value; reject it
        // explicitly rather than silently picking one of the equivalent
        // interpretations.
        _ => Err(CdrError::Other(format!(
            "Invalid TypeIdentifier discriminator: 0x{:02X}",
            discriminator
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::EquivalenceHash;

    /// `TypeIdentifier::Minimal` must encode the spec EK_MINIMAL discriminator
    /// (0xF1 per DDS-XTypes v1.3 §7.3.4.4 IDL), not the in-memory
    /// `EquivalenceKind::Minimal` value (0x10) which is unrelated to the wire.
    #[test]
    fn typeid_minimal_writes_ek_minimal_spec_byte() {
        let id = TypeIdentifier::Minimal(EquivalenceHash::from_bytes([0xAA; 14]));
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0xF1);
        assert_eq!(written, 15);
        assert_eq!(&buf[1..15], &[0xAA; 14]);
    }

    /// `TypeIdentifier::Complete` must encode the spec EK_COMPLETE
    /// discriminator (0xF2 per DDS-XTypes v1.3 §7.3.4.4 IDL).
    #[test]
    fn typeid_complete_writes_ek_complete_spec_byte() {
        let id = TypeIdentifier::Complete(EquivalenceHash::from_bytes([0xBB; 14]));
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0xF2);
        assert_eq!(written, 15);
        assert_eq!(&buf[1..15], &[0xBB; 14]);
    }

    /// Per DDS-XTypes v1.3 §7.3.4.4, primitive `TypeIdentifier`s are encoded
    /// as a single `TypeKind` octet. This test pins the wire size at 1 byte
    /// to lock against re-introducing the legacy 2-byte `[0x01, kind]`
    /// wrapper that diverged from spec.
    #[test]
    fn typeid_primitive_encodes_as_typekind_octet_only() {
        let id = TypeIdentifier::Primitive(TypeKind::TK_FLOAT32);
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(written, 1, "primitive must emit 1 byte total");
        assert_eq!(buf[0], 0x09, "TK_FLOAT32 octet per §7.3.4.4");
    }

    /// `TypeIdentifier::StringSmall` -> TI_STRING8_SMALL (0x70) +
    /// `StringSTypeDefn { SBound bound }` per §7.3.4.4.
    #[test]
    fn typeid_string_small_writes_ti_string8_small() {
        let id = TypeIdentifier::StringSmall { bound: 0x42 };
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0x70);
        assert_eq!(buf[1], 0x42);
        assert_eq!(written, 2);
    }

    /// `TypeIdentifier::WStringLarge` -> TI_STRING16_LARGE (0x73) +
    /// `StringLTypeDefn { LBound bound }` per §7.3.4.4. The LBound is a
    /// 4-byte CDR2-aligned u32, so the discriminator octet is followed by
    /// 3 padding bytes before the bound payload.
    #[test]
    fn typeid_wstring_large_writes_ti_string16_large_aligned() {
        let id = TypeIdentifier::WStringLarge { bound: 1024 };
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0x73);
        assert_eq!(written, 8);
        assert_eq!(u32::from_le_bytes(buf[4..8].try_into().unwrap()), 1024);
    }

    /// `TypeIdentifier::StronglyConnected` -> TI_STRONGLY_CONNECTED_COMPONENT
    /// (0xB0) per §7.3.4.4. Only the top-level discriminator is migrated;
    /// the inner payload still diverges from spec (no DHEADER, no inner
    /// `TypeObjectHashId` union discriminator) — flagged for a future fix.
    /// The wire size is locked at 24 bytes: 1 octet discriminator + 14-byte
    /// hash (offset 1..15) + 1 padding byte for CDR2 4-byte alignment of the
    /// following `i32` (offset 15) + 4-byte `scc_length` + 4-byte `scc_index`.
    #[test]
    fn typeid_strongly_connected_writes_ti_scc_discriminator() {
        let id = TypeIdentifier::StronglyConnected(
            crate::xtypes::type_id::StronglyConnectedComponentId {
                sc_component_id: EquivalenceHash::from_bytes([0xCC; 14]),
                scc_length: 3,
                scc_index: 1,
            },
        );
        let mut buf = [0u8; 32];
        let written = id.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0xB0);
        assert_eq!(
            written, 24,
            "SCC wire size must be exactly 24 bytes (1 discriminator + 14 hash + 1 pad + 2*4 i32)"
        );
    }

    /// `Inline` is an HDDS-internal variant with no spec encoding;
    /// the encoder must surface this rather than producing wire bytes.
    #[test]
    fn typeid_inline_returns_error() {
        use crate::xtypes::{
            CompleteStructHeader, CompleteStructType, CompleteTypeDetail, CompleteTypeObject,
            StructTypeFlag,
        };
        let type_obj = CompleteTypeObject::Struct(CompleteStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header: CompleteStructHeader {
                base_type: None,
                detail: CompleteTypeDetail::new("Probe"),
            },
            member_seq: vec![],
        });
        let id = TypeIdentifier::Inline(Box::new(type_obj));
        let mut buf = [0u8; 256];
        let result = id.encode_cdr2_le(&mut buf);
        match result {
            Err(CdrError::Other(msg)) => assert!(msg.contains("Inline")),
            other => panic!("expected CdrError::Other for Inline, got {:?}", other),
        }
    }

    /// Sanity: every spec discriminator that the encoder emits is a valid
    /// `TypeIdentifier` discriminator per §7.3.4.4 IDL. Catches accidental
    /// import of an `EquivalenceKind` variant cast as `u8`, which would
    /// produce a stray 0x10 / 0x20 byte rather than 0xF1 / 0xF2.
    #[test]
    fn typeid_encoder_uses_only_spec_discriminator_octets() {
        let cases: [(TypeIdentifier, u8); 7] = [
            (
                TypeIdentifier::Primitive(TypeKind::TK_BOOLEAN),
                TypeKind::TK_BOOLEAN.to_u8(),
            ),
            (TypeIdentifier::StringSmall { bound: 1 }, 0x70),
            (TypeIdentifier::StringLarge { bound: 256 }, 0x71),
            (TypeIdentifier::WStringSmall { bound: 1 }, 0x72),
            (TypeIdentifier::WStringLarge { bound: 256 }, 0x73),
            (TypeIdentifier::Minimal(EquivalenceHash::zero()), 0xF1),
            (TypeIdentifier::Complete(EquivalenceHash::zero()), 0xF2),
        ];
        for (id, expected) in cases {
            let mut buf = [0u8; 32];
            id.encode_cdr2_le(&mut buf).expect("encode succeeds");
            assert_eq!(buf[0], expected, "discriminator mismatch for {:?}", id);
        }
    }

    /// Pre-migration HDDS used `0x06` as the discriminator for hash-based
    /// `Minimal` `TypeIdentifier`s, with a 14-byte payload. Per OMG
    /// DDS-XTypes v1.3 §7.3.4.4, byte `0x06` is `TK_UINT16`, a primitive
    /// `TypeIdentifier` with no payload. This test locks the spec
    /// reinterpretation: a buffer that under the legacy decoder produced
    /// `Minimal(hash)` must now produce `Primitive(TK_UINT16)` consuming
    /// only one byte. It guards against accidental re-introduction of the
    /// legacy hash branch.
    #[test]
    fn typeid_decoder_rejects_legacy_byte_06_as_minimal() {
        // 1 discriminator + 14 fake-hash bytes (legacy layout).
        let mut buf = [0u8; 15];
        buf[0] = 0x06;
        for byte in buf.iter_mut().take(15).skip(1) {
            *byte = 0xAA;
        }
        let (decoded, consumed) =
            TypeIdentifier::decode_cdr2_le(&buf).expect("primitive decode succeeds");
        match decoded {
            TypeIdentifier::Primitive(TypeKind::TK_UINT16) => {}
            other => panic!(
                "expected Primitive(TK_UINT16) for byte 0x06, got {:?}",
                other
            ),
        }
        assert_eq!(consumed, 1, "primitive must consume exactly 1 byte");
    }

    /// Pre-migration HDDS used `0x09` as the discriminator for the
    /// HDDS-only `Inline` variant. Per §7.3.4.4, byte `0x09` is
    /// `TK_FLOAT32`. Locks the spec reinterpretation and guards against
    /// the legacy branch being restored.
    #[test]
    fn typeid_decoder_rejects_legacy_byte_09_as_inline() {
        let buf = [0x09u8];
        let (decoded, consumed) =
            TypeIdentifier::decode_cdr2_le(&buf).expect("primitive decode succeeds");
        match decoded {
            TypeIdentifier::Primitive(TypeKind::TK_FLOAT32) => {}
            other => panic!(
                "expected Primitive(TK_FLOAT32) for byte 0x09, got {:?}",
                other
            ),
        }
        assert_eq!(consumed, 1, "primitive must consume exactly 1 byte");
    }

    /// Bytes outside the spec discriminator set (primitives 0x00..0x11,
    /// TI_STRING* 0x70..0x73, TI_SCC 0xB0, EK_MINIMAL/EK_COMPLETE 0xF1/0xF2)
    /// must surface as `CdrError::Other`. `0x50` falls in the spec gap
    /// between `TK_CHAR16` (0x11) and the `TI_STRING8_*` block (0x70).
    #[test]
    fn typeid_decoder_rejects_spec_gap_byte() {
        let buf = [0x50u8, 0x00, 0x00, 0x00];
        match TypeIdentifier::decode_cdr2_le(&buf) {
            Err(CdrError::Other(msg)) => assert!(
                msg.contains("0x50"),
                "expected error to mention discriminator 0x50, got: {msg}"
            ),
            other => panic!("expected CdrError::Other for byte 0x50, got {:?}", other),
        }
    }

    /// `EK_BOTH` (0xF3) is a forward-compat marker per §7.3.4.4 IDL.
    /// HDDS does not currently emit it; the decoder must reject it
    /// rather than silently aliasing to `Minimal` or `Complete`.
    #[test]
    fn typeid_decoder_rejects_ek_both_forward_compat() {
        let mut buf = [0u8; 15];
        buf[0] = 0xF3;
        match TypeIdentifier::decode_cdr2_le(&buf) {
            Err(CdrError::Other(msg)) => assert!(
                msg.contains("0xF3"),
                "expected error to mention EK_BOTH (0xF3), got: {msg}"
            ),
            other => panic!("expected CdrError::Other for EK_BOTH, got {:?}", other),
        }
    }

    /// Symmetric round-trip: encoder emits 0xF1 + 14-byte hash, decoder
    /// consumes the same bytes and reconstructs the `Minimal` variant.
    /// Guards against asymmetric drift between the two halves.
    #[test]
    fn typeid_minimal_encode_decode_round_trip_uses_spec_byte() {
        let original = TypeIdentifier::Minimal(EquivalenceHash::from_bytes([0x55; 14]));
        let mut buf = [0u8; 32];
        let written = original.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0xF1, "encoder must emit EK_MINIMAL discriminator");

        let (decoded, consumed) =
            TypeIdentifier::decode_cdr2_le(&buf[..written]).expect("decode succeeds");
        assert_eq!(consumed, written);
        assert_eq!(decoded, original);
    }

    /// Symmetric round-trip for `TI_STRONGLY_CONNECTED_COMPONENT` (0xB0):
    /// encoder writes 14-byte hash + 1 padding byte + two `i32`s (24 bytes
    /// total), decoder consumes the same bytes and reconstructs the same
    /// `StronglyConnectedComponentId`. Guards against asymmetric drift on
    /// the SCC payload alignment between the two halves.
    #[test]
    fn typeid_strongly_connected_round_trip() {
        let original = TypeIdentifier::StronglyConnected(
            crate::xtypes::type_id::StronglyConnectedComponentId {
                sc_component_id: EquivalenceHash::from_bytes([0xCC; 14]),
                scc_length: 7,
                scc_index: 3,
            },
        );
        let mut buf = [0u8; 32];
        let written = original.encode_cdr2_le(&mut buf).expect("encode succeeds");
        assert_eq!(buf[0], 0xB0);

        let (decoded, consumed) =
            TypeIdentifier::decode_cdr2_le(&buf[..written]).expect("decode succeeds");
        assert_eq!(consumed, written);
        assert_eq!(decoded, original);
    }

    /// Per OMG DDS-XTypes v1.3 §7.3.4 IDL TypeKinds block, the
    /// primitive-range bytes `0x0E` and `0x0F` are gaps (TK_UINT8 ends
    /// at 0x0D, TK_CHAR8 starts at 0x10). The decoder must reject them
    /// rather than silently widening the accepted set if a future
    /// `TypeKind` variant is added in that range without coordination
    /// with this match arm.
    #[test]
    fn typeid_decoder_rejects_undefined_primitive_bytes() {
        for byte in [0x0E_u8, 0x0F] {
            let buf = [byte];
            let result = TypeIdentifier::decode_cdr2_le(&buf);
            assert!(
                matches!(result, Err(CdrError::Other(_))),
                "byte 0x{:02X} must be rejected (undefined primitive TypeKind), got {:?}",
                byte,
                result
            );
        }
    }
}
