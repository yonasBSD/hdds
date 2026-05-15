// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! PID_TYPE_INFORMATION (0x0075) emission for SEDP discovery.
//!
//! DDS-XTypes v1.3 §7.6.3.2 defines `TypeInformation` as a @mutable struct
//! carrying two `TypeIdentifierWithDependencies` members: minimal (member id
//! 0x1001) and complete (member id 0x1002). Each carries the
//! `TypeIdentifier` and `typeobject_serialized_size` for its respective
//! TypeObject view, plus optional dependent TypeIdentifiers.
//!
//! Fast DDS 3.x `DataReader` requires `PID_TYPE_INFORMATION` on the writer's
//! SEDP DATA(w) for cross-vendor matching even when
//! `TypeConsistencyEnforcement.kind = ALLOW_TYPE_COERCION`. Without it the
//! reader stays unmatched (no `on_subscription_matched` callback fires)
//! despite a valid `PID_TYPE_OBJECT` being present.
//!
//! # Wire format
//!
//! The encoder produces the byte structure that Fast DDS itself emits for
//! the same logical type (verified via `tshark` against
//! `TemperatureSubscriberApp` running on Fast DDS 3.1.2):
//!
//! ```text
//! PID 0x0075, PARAM_LEN = 92
//! ├ DHEADER          u32 LE  = 88   (TypeInformation @mutable content size)
//! ├ Member 0x1001 (minimal)
//! │ ├ EMHEADER1      u32 LE  = 0x40001001  (M=0, LC=4, MID=0x1001)
//! │ ├ NEXTINT        u32 LE  = 36          (member content length, per LC=4)
//! │ ├ INNER_LEN      u32 LE  = 20          (TypeIdentifierWithSize content)
//! │ ├ EK_MINIMAL     u8      = 0xF1        (TypeIdentifier discriminator)
//! │ ├ hash           [u8;14]               (MD5(MinimalTypeObject)[..14])
//! │ ├ pad            u8      = 0           (4-byte alignment)
//! │ ├ size           u32 LE                (typeobject_serialized_size)
//! │ ├ tail_a         u32 LE  = 0xFFFFFFFF  (Fast-DDS-observed sentinel)
//! │ ├ tail_b         u32 LE  = 4           (Fast-DDS-observed sentinel)
//! │ └ tail_c         u32 LE  = 0           (Fast-DDS-observed sentinel)
//! └ Member 0x1002 (complete) -- same layout, EMHEADER MID=0x1002, EK=0xF2
//! ```
//!
//! The three trailing 32-bit values per member (`0xFFFFFFFF`, `4`, `0`) are
//! preserved verbatim from Fast DDS' wire output. The spec text in §7.6.3.2.1
//! describes a `dependent_typeid_count: int32` followed by a
//! `sequence<TypeIdentifierWithSize> dependent_typeids`, but Fast DDS
//! consistently emits this 12-byte tail regardless of dependencies, so the
//! encoder mirrors it for byte-for-byte compatibility with the matching
//! logic on the reader side. The intentional spec-divergence rationale is
//! captured in `docs/_privates/ADR-CHANTIER-1.5-PHASE-0.md` §7bis under
//! "Caveat sentinel".
//!
//! # Hash computation
//!
//! `EquivalenceHash` is the MD5 of the CDR2-serialised TypeObject truncated
//! to 14 bytes (DDS-XTypes v1.3 §7.3.4.8). After Chantier 1.5 b/c/d landed,
//! HDDS' `TypeKind` octet values, `TypeObject` discriminator labels, and
//! `AnnotationParameterValue` discriminator labels all match the spec
//! IDL — so the hashes computed here are spec-compliant and byte-comparable
//! with what Fast DDS, Connext, OpenDDS, or Cyclone DDS would compute for
//! the same logical type.

use crate::core::ser::traits::Cdr2Encode;
use crate::protocol::discovery::types::{
    ParseError, TypeIdentifierWithDependencies, TypeInformation,
};
use crate::xtypes::discriminators::{EK_COMPLETE, EK_MINIMAL};
use crate::xtypes::{
    CompleteStructType, CompleteTypeObject, EquivalenceHash, MinimalMemberDetail,
    MinimalStructHeader, MinimalStructMember, MinimalStructType, MinimalTypeDetail,
    MinimalTypeObject,
};

/// PID identifier for `PID_TYPE_INFORMATION` per DDS-XTypes v1.3 §7.6.3.2.
const PID_TYPE_INFORMATION: u16 = 0x0075;
/// Total payload size emitted (matches Fast DDS' wire output).
const PAYLOAD_LEN: u16 = 92;

/// Per-member tail values observed on Fast DDS' wire output. They occupy the
/// trailing 12 bytes after `typeobject_serialized_size` for both the minimal
/// and complete members. See module-level "Wire format" section + ADR §7bis.
const TAIL_A: u32 = 0xFFFF_FFFF;
const TAIL_B: u32 = 4;
const TAIL_C: u32 = 0;

/// Convert a `CompleteStructType` to a `MinimalStructType` per DDS-XTypes
/// v1.3 §7.3.4.5: drop type-detail names, replace member detail with the
/// 4-byte `MinimalMemberDetail::name_hash`. Common member fields and
/// extensibility flags carry over unchanged.
fn complete_struct_to_minimal(complete: &CompleteStructType) -> MinimalStructType {
    let member_seq = complete
        .member_seq
        .iter()
        .map(|m| MinimalStructMember {
            common: m.common.clone(),
            detail: MinimalMemberDetail::from_name(&m.detail.name),
        })
        .collect();

    MinimalStructType {
        struct_flags: complete.struct_flags,
        header: MinimalStructHeader {
            base_type: complete.header.base_type.clone(),
            detail: MinimalTypeDetail::new(),
        },
        member_seq,
    }
}

/// Derive a `MinimalTypeObject` from a `CompleteTypeObject`. Only `Struct`
/// is supported today; other variants return `None` and the caller skips
/// `PID_TYPE_INFORMATION` emission.
fn derive_minimal(complete: &CompleteTypeObject) -> Option<MinimalTypeObject> {
    match complete {
        CompleteTypeObject::Struct(s) => {
            Some(MinimalTypeObject::Struct(complete_struct_to_minimal(s)))
        }
        _ => None,
    }
}

/// CDR2-serialised size of a `CompleteTypeObject`, used for the
/// `typeobject_serialized_size` field in the wire structure.
fn cdr2_size_complete(complete: &CompleteTypeObject) -> Result<u32, ParseError> {
    let mut buf = vec![0u8; complete.max_cdr2_size()];
    let len = complete
        .encode_cdr2_le(&mut buf)
        .map_err(|_| ParseError::EncodingError)?;
    u32::try_from(len).map_err(|_| ParseError::InvalidFormat)
}

/// CDR2-serialised size of a `MinimalTypeObject`.
fn cdr2_size_minimal(minimal: &MinimalTypeObject) -> Result<u32, ParseError> {
    let mut buf = vec![0u8; minimal.max_cdr2_size()];
    let len = minimal
        .encode_cdr2_le(&mut buf)
        .map_err(|_| ParseError::EncodingError)?;
    u32::try_from(len).map_err(|_| ParseError::InvalidFormat)
}

/// Write a single `TypeIdentifierWithDependencies` member into the buffer
/// (member content layout described in the module-level wire diagram).
fn write_member(
    buf: &mut [u8],
    offset: &mut usize,
    member_id: u32,
    discriminator: u8,
    hash: &EquivalenceHash,
    typeobject_serialized_size: u32,
) -> Result<(), ParseError> {
    if *offset + 44 > buf.len() {
        return Err(ParseError::BufferTooSmall);
    }

    // EMHEADER1: M=0, LC=4 (NEXTINT carries member length), MID=member_id
    //   -> (4 << 28) | member_id
    // Per OMG DDS-XTypes v1.3 §7.4.3.4.3 Table 39: LC=4 advertises a
    // following 4-byte NEXTINT u32 with the member content length, which
    // is exactly what this encoder writes at line 159.
    let emheader: u32 = (4u32 << 28) | (member_id & 0x0FFF_FFFF);
    buf[*offset..*offset + 4].copy_from_slice(&emheader.to_le_bytes());
    *offset += 4;

    // NEXTINT (member content length)
    buf[*offset..*offset + 4].copy_from_slice(&36u32.to_le_bytes());
    *offset += 4;

    // INNER_LEN (TypeIdentifierWithSize content length)
    buf[*offset..*offset + 4].copy_from_slice(&20u32.to_le_bytes());
    *offset += 4;

    // TypeIdentifier discriminator + 14-byte hash + 1-byte alignment pad
    buf[*offset] = discriminator;
    *offset += 1;
    buf[*offset..*offset + 14].copy_from_slice(hash.as_bytes());
    *offset += 14;
    buf[*offset] = 0;
    *offset += 1;

    // typeobject_serialized_size
    buf[*offset..*offset + 4].copy_from_slice(&typeobject_serialized_size.to_le_bytes());
    *offset += 4;

    // Trailing sentinel triple observed on Fast DDS' wire output
    buf[*offset..*offset + 4].copy_from_slice(&TAIL_A.to_le_bytes());
    *offset += 4;
    buf[*offset..*offset + 4].copy_from_slice(&TAIL_B.to_le_bytes());
    *offset += 4;
    buf[*offset..*offset + 4].copy_from_slice(&TAIL_C.to_le_bytes());
    *offset += 4;

    Ok(())
}

/// Derive a `TypeInformation` from a `CompleteTypeObject`. Returns `None`
/// when the type cannot produce a `MinimalTypeObject` today (non-Struct
/// variants — Chantier 1.6 will extend coverage), or when the underlying
/// CDR2 encoding fails. Both fields of the returned struct are populated
/// (`minimal` + `complete`) when derivation succeeds; partial derivation
/// is currently not supported and would surface as `None` from this
/// helper.
pub(super) fn derive_type_information(type_obj: &CompleteTypeObject) -> Option<TypeInformation> {
    let minimal_obj = derive_minimal(type_obj)?;
    let complete_hash = type_obj.compute_equivalence_hash().ok()?;
    let minimal_hash = minimal_obj.compute_equivalence_hash().ok()?;
    let complete_size = cdr2_size_complete(type_obj).ok()?;
    let minimal_size = cdr2_size_minimal(&minimal_obj).ok()?;
    Some(TypeInformation {
        minimal: Some(TypeIdentifierWithDependencies {
            hash: minimal_hash,
            typeobject_serialized_size: minimal_size,
        }),
        complete: Some(TypeIdentifierWithDependencies {
            hash: complete_hash,
            typeobject_serialized_size: complete_size,
        }),
    })
}

/// Emit `PID_TYPE_INFORMATION` (0x0075) for the given `TypeInformation`.
///
/// Both members must be present (`minimal` + `complete`) — the wire
/// layout reserves 92 bytes total for both, and Fast DDS / Connext / etc.
/// expect the EMHEADER1 sequence `0x1001` followed by `0x1002`. Partial
/// `TypeInformation` is rejected with a soft `Ok(false)` so the rest of
/// the SEDP packet remains intact (caller must use `derive_type_information`
/// upstream to populate both fields, or skip the PID emission).
pub(super) fn write_type_information(
    info: &TypeInformation,
    buf: &mut [u8],
    offset: &mut usize,
) -> Result<bool, ParseError> {
    let (minimal, complete) = match (info.minimal.as_ref(), info.complete.as_ref()) {
        (Some(m), Some(c)) => (m, c),
        _ => return Ok(false),
    };

    let total = 4 + PAYLOAD_LEN as usize;
    if *offset + total > buf.len() {
        return Err(ParseError::BufferTooSmall);
    }

    // PID header (4 bytes)
    buf[*offset..*offset + 2].copy_from_slice(&PID_TYPE_INFORMATION.to_le_bytes());
    buf[*offset + 2..*offset + 4].copy_from_slice(&PAYLOAD_LEN.to_le_bytes());
    *offset += 4;

    // DHEADER: TypeInformation content size = 88 bytes (PAYLOAD_LEN - DHEADER)
    let dheader: u32 = u32::from(PAYLOAD_LEN) - 4;
    buf[*offset..*offset + 4].copy_from_slice(&dheader.to_le_bytes());
    *offset += 4;

    // Member 0x1001: minimal
    write_member(
        buf,
        offset,
        0x1001,
        EK_MINIMAL,
        &minimal.hash,
        minimal.typeobject_serialized_size,
    )?;
    // Member 0x1002: complete
    write_member(
        buf,
        offset,
        0x1002,
        EK_COMPLETE,
        &complete.hash,
        complete.typeobject_serialized_size,
    )?;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xtypes::{
        CommonStructMember, CompleteMemberDetail, CompleteStructHeader, CompleteStructMember,
        CompleteStructType, CompleteTypeDetail, MemberFlag, StructTypeFlag, TypeIdentifier,
        TypeKind,
    };

    fn temperature_complete() -> CompleteTypeObject {
        CompleteTypeObject::Struct(CompleteStructType {
            struct_flags: StructTypeFlag::IS_FINAL,
            header: CompleteStructHeader {
                base_type: None,
                detail: CompleteTypeDetail::new("Temperature"),
            },
            member_seq: vec![
                CompleteStructMember {
                    common: CommonStructMember {
                        member_id: 0,
                        member_flags: MemberFlag::empty(),
                        member_type_id: TypeIdentifier::primitive(TypeKind::TK_FLOAT32),
                    },
                    detail: CompleteMemberDetail::new("value"),
                },
                CompleteStructMember {
                    common: CommonStructMember {
                        member_id: 1,
                        member_flags: MemberFlag::empty(),
                        member_type_id: TypeIdentifier::primitive(TypeKind::TK_UINT32),
                    },
                    detail: CompleteMemberDetail::new("timestamp"),
                },
            ],
        })
    }

    /// Convenience: derive the `TypeInformation` for the standard
    /// Temperature fixture used across tests below.
    fn temperature_info() -> TypeInformation {
        derive_type_information(&temperature_complete())
            .expect("Struct fixture must derive a TypeInformation")
    }

    #[test]
    fn type_information_payload_is_92_bytes() {
        let info = temperature_info();
        let mut buf = [0u8; 256];
        let mut offset = 0;

        let written = write_type_information(&info, &mut buf, &mut offset)
            .expect("write should succeed for struct types");
        assert!(written, "Struct types must emit PID_TYPE_INFORMATION");
        assert_eq!(offset, 4 + 92, "PID header (4) + payload (92) bytes total");
    }

    #[test]
    fn type_information_pid_and_length_header() {
        let info = temperature_info();
        let mut buf = [0u8; 256];
        let mut offset = 0;

        write_type_information(&info, &mut buf, &mut offset).expect("emit succeeds");

        assert_eq!(&buf[0..2], &PID_TYPE_INFORMATION.to_le_bytes());
        assert_eq!(&buf[2..4], &PAYLOAD_LEN.to_le_bytes());
        // DHEADER = 88
        assert_eq!(&buf[4..8], &88u32.to_le_bytes());
    }

    #[test]
    fn type_information_emheaders_match_fastdds() {
        let info = temperature_info();
        let mut buf = [0u8; 256];
        let mut offset = 0;

        write_type_information(&info, &mut buf, &mut offset).expect("emit succeeds");

        // Member 0x1001 EMHEADER1 starts at offset 8 (after PID header + DHEADER).
        let emheader_minimal = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
        assert_eq!(
            emheader_minimal, 0x4000_1001,
            "minimal EMHEADER must be 0x40001001 (M=0, LC=4, MID=0x1001)"
        );

        // Member 0x1002 EMHEADER1 starts at offset 8 + 44.
        let emheader_complete = u32::from_le_bytes([buf[52], buf[53], buf[54], buf[55]]);
        assert_eq!(
            emheader_complete, 0x4000_1002,
            "complete EMHEADER must be 0x40001002 (M=0, LC=4, MID=0x1002)"
        );
    }

    #[test]
    fn type_information_discriminators_are_spec_compliant() {
        let info = temperature_info();
        let mut buf = [0u8; 256];
        let mut offset = 0;

        write_type_information(&info, &mut buf, &mut offset).expect("emit succeeds");

        // Minimal TypeIdentifier discriminator at offset 8 + 12 = 20.
        assert_eq!(buf[20], EK_MINIMAL, "minimal must use EK_MINIMAL (0xF1)");
        assert_eq!(
            buf[20], 0xF1,
            "EK_MINIMAL must equal 0xF1 per spec §7.3.4.4"
        );
        // Complete TypeIdentifier discriminator at offset 8 + 44 + 12 = 64.
        assert_eq!(buf[64], EK_COMPLETE, "complete must use EK_COMPLETE (0xF2)");
        assert_eq!(
            buf[64], 0xF2,
            "EK_COMPLETE must equal 0xF2 per spec §7.3.4.4"
        );
    }

    #[test]
    fn derive_minimal_returns_none_for_non_struct() {
        // Contract: `derive_minimal` (and therefore `derive_type_information`)
        // returns `Some` only for the Struct variant; every other
        // `CompleteTypeObject` must soft-skip PID_TYPE_INFORMATION emission
        // today (Chantier 1.6 will extend coverage to collections /
        // annotations).
        use crate::xtypes::{
            AliasTypeFlag, CommonAliasBody, CompleteAliasBody, CompleteAliasHeader,
            CompleteAliasType, TypeRelationFlag,
        };

        // Some(_) for Struct -- baseline.
        let struct_to = temperature_complete();
        assert!(
            derive_minimal(&struct_to).is_some(),
            "Struct variant must produce a MinimalTypeObject"
        );
        assert!(
            derive_type_information(&struct_to).is_some(),
            "Struct variant must produce a TypeInformation"
        );

        // None for Alias -- one of the simplest non-Struct variants and
        // typical of what the SEDP path will encounter for typedef'd
        // user types.
        let alias_to = CompleteTypeObject::Alias(CompleteAliasType {
            alias_flags: AliasTypeFlag::empty(),
            header: CompleteAliasHeader {
                detail: CompleteTypeDetail::new("MyAlias"),
            },
            body: CompleteAliasBody {
                common: CommonAliasBody {
                    related_flags: TypeRelationFlag::empty(),
                    related_type: TypeIdentifier::primitive(TypeKind::TK_INT32),
                },
                detail: CompleteTypeDetail::new("MyAlias"),
            },
        });
        assert!(
            derive_minimal(&alias_to).is_none(),
            "Alias variant must NOT produce a MinimalTypeObject (soft-skip)"
        );
        assert!(
            derive_type_information(&alias_to).is_none(),
            "Alias variant must NOT produce a TypeInformation (soft-skip)"
        );

        // The end-to-end soft-skip: a `TypeInformation` with both fields
        // missing must surface as `Ok(false)` and leave the buffer
        // untouched.
        let empty_info = TypeInformation::default();
        let mut buf = [0xAAu8; 256];
        let mut offset = 0;
        let written = write_type_information(&empty_info, &mut buf, &mut offset)
            .expect("partial TypeInformation soft-skip must not be a fatal error");
        assert!(!written, "default TypeInformation must soft-skip emission");
        assert_eq!(offset, 0, "offset must not advance on soft-skip");
        assert_eq!(buf[0], 0xAA, "buffer must remain untouched on soft-skip");
    }

    /// TX/RX symmetry: encode a TypeInformation, then decode the wire
    /// bytes (skipping the 4-byte PID header) and assert the parsed
    /// struct equals the input. Confirms the build/parse pair is a
    /// faithful round-trip, including the FastDDS-mimic 12-byte sentinel
    /// which the parser must consume without error.
    #[test]
    fn type_information_round_trip_via_parse() {
        let info = temperature_info();
        let mut buf = [0u8; 256];
        let mut offset = 0;
        let written = write_type_information(&info, &mut buf, &mut offset)
            .expect("emit succeeds for Struct fixture");
        assert!(written);

        // The PID header occupies bytes 0..4; the parser receives the
        // remaining `length` bytes (DHEADER + 2 members = 92 bytes).
        let payload = &buf[4..offset];
        let parsed = crate::protocol::discovery::sedp::parse::parse_type_information(payload)
            .expect("parser accepts well-formed wire bytes");

        assert_eq!(parsed, info, "round-trip must preserve TypeInformation");
    }
}
