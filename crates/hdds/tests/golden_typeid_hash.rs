// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Golden byte-stable lock for `EquivalenceHash` computation.
//!
//! Any change to the `MinimalTypeObject` CDR2 wire layout, `TypeKind`
//! octet values, `TypeObject` discriminator labels, or
//! `MinimalMemberDetail::from_name` hashing surfaces as a failure here.
//! Per OMG DDS-XTypes v1.3 §7.3.4.8 the hash is
//! `MD5(CDR2_LE(MinimalTypeObject))[..14]`. After Chantier 1.5 b/c/d
//! the discriminator chain is spec-compliant, so the bytes locked
//! below are byte-comparable with what Fast DDS, Connext, OpenDDS, or
//! Cyclone DDS would produce for the same logical IDL type.
//!
//! Cross-vendor empirical verification: confirmed 2026-05-09 in
//! Chantier 1.5f against Fast DDS 3.x — the locked bytes here are
//! byte-identical to the value Fast DDS emits over the wire for the
//! same logical type (extracted via `tshark` on `PID_TYPE_INFORMATION`
//! from `interop/fastdds2hdds.pcap` Gate 05 capture). See
//! `docs/_privates/VERDICT-CHANTIER-1.5-CROSSVENDOR.md` for the full
//! verdict + raw bytes.

#![cfg(feature = "xtypes")]

use hdds::xtypes::{
    CommonStructMember, EquivalenceHash, MemberFlag, MinimalMemberDetail, MinimalStructHeader,
    MinimalStructMember, MinimalStructType, MinimalTypeDetail, MinimalTypeObject, StructTypeFlag,
    TypeIdentifier, TypeKind,
};

/// Reference fixture: `struct Temperature { float32 value; uint32 timestamp; }`.
/// Same shape as the `temperature_complete` fixture used in
/// `protocol::discovery::sedp::build::type_information::tests`.
fn temperature_minimal() -> MinimalTypeObject {
    MinimalTypeObject::Struct(MinimalStructType {
        struct_flags: StructTypeFlag::IS_FINAL,
        header: MinimalStructHeader {
            base_type: None,
            detail: MinimalTypeDetail::new(),
        },
        member_seq: vec![
            MinimalStructMember {
                common: CommonStructMember {
                    member_id: 0,
                    member_flags: MemberFlag::empty(),
                    member_type_id: TypeIdentifier::primitive(TypeKind::TK_FLOAT32),
                },
                detail: MinimalMemberDetail::from_name("value"),
            },
            MinimalStructMember {
                common: CommonStructMember {
                    member_id: 1,
                    member_flags: MemberFlag::empty(),
                    member_type_id: TypeIdentifier::primitive(TypeKind::TK_UINT32),
                },
                detail: MinimalMemberDetail::from_name("timestamp"),
            },
        ],
    })
}

#[test]
fn golden_minimal_temperature_equivalence_hash() {
    let type_obj = temperature_minimal();
    let hash = type_obj
        .compute_equivalence_hash()
        .expect("CDR2 encoding of MinimalTypeObject must succeed");

    // Locked 2026-05-09 post Chantier 1.5b/c/d (spec-compliant
    // discriminators). Computed on this fixture
    // (`struct Temperature { float32 value; uint32 timestamp; }`).
    // Cross-vendor verification: this must equal what Fast DDS,
    // Connext, OpenDDS, or Cyclone DDS produce for the same logical
    // type per OMG DDS-XTypes v1.3 §7.3.4.8.
    let expected: [u8; 14] = [
        0xc8, 0x5a, 0x48, 0x98, 0x4e, 0x82, 0xff, 0x6e, 0x13, 0x8f, 0x0f, 0xf9, 0xc8, 0x32,
    ];
    assert_eq!(
        hash.as_bytes(),
        &expected,
        "EquivalenceHash drift detected: any change to the CDR2 wire \
         layout, TypeKind octets, TypeObject discriminators, or \
         MinimalMemberDetail::from_name hashing must be intentional. \
         If this test fails, re-derive the locked bytes only after \
         confirming the change is spec-compliant per DDS-XTypes \
         v1.3 §7.3.4.8."
    );

    // Defensive : the hash must never be zero (would indicate the
    // `xtypes` feature is OFF or the MD5 implementation is broken).
    assert_ne!(hash, EquivalenceHash::zero());
}
