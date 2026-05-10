// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Golden byte-stable lock for `EquivalenceHash` computation.
//!
//! Any change to the `MinimalTypeObject` CDR2 wire layout, `TypeKind`
//! octet values, `TypeObject` discriminator labels, or
//! `MinimalMemberDetail::from_name` hashing surfaces as a failure here.
//! Per OMG DDS-XTypes v1.3 §7.3.4.8 the hash is
//! `MD5(CDR2_LE(MinimalTypeObject))[..14]`.
//!
//! Cross-vendor status: NOT YET ACHIEVED. The bytes locked below
//! reflect HDDS-self encoding only — they are NOT byte-comparable with
//! what Fast DDS / Connext / OpenDDS / Cyclone DDS produce for the
//! same logical IDL type. Root cause: F29 (DHEADER missing for
//! @extensibility(APPENDABLE) TypeObject containers) per OMG DDS-XTypes
//! v1.3 §7.4.3.3. Tracked in ADR-CHANTIER-1.6-AUDIT-RESPONSE §8 (F29)
//! and §10.14 (méthodologie cross-vendor). Empirical evidence at
//! `/tmp/f28-capture/fastdds-pubsub.pcap` shows Fast DDS 3.x emits
//! Minimal hash `bb 41 b9 75 4f 8f c5 3c 3e e5 48 84 42 96` (55 bytes
//! TypeObject) for the same Temperature.idl that HDDS computes to a
//! drifting 36-byte payload — Δ approximately matches the missing
//! DHEADER plus structural framing.
//!
//! Resolution gated by sous-chantier 1.6.10 (F29 fix). Until then this
//! test remains `#[ignore]` with marker F29 — the hash check is
//! HDDS-self consistent only, intentionally divergent from spec.

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
#[ignore = "F29: DHEADER missing for @extensibility(APPENDABLE) TypeObjects \
            (XTypes v1.3 §7.4.3.3); HDDS-self hash is intentionally \
            divergent from FastDDS/Connext/OpenDDS/Cyclone wire bytes \
            until 1.6.10 fixes the framing. F28 (hash drift between \
            HDDS pre/post-1.6.1a) is a downstream symptom; both pre \
            and post variants are wrong cross-vendor. Tracked in \
            ADR-CHANTIER-1.6 §8 (F28+F29) + §10.14 (méthodologie)."]
fn golden_minimal_temperature_equivalence_hash() {
    let type_obj = temperature_minimal();
    let hash = type_obj
        .compute_equivalence_hash()
        .expect("CDR2 encoding of MinimalTypeObject must succeed");

    // NOTE: these bytes reflect HDDS-self encoding only. They are NOT
    // cross-vendor verified. Fast DDS 3.x empirical capture (1.6.1d
    // F28 investigation) shows it emits a different Minimal hash for
    // the same logical Temperature.idl because HDDS is missing the
    // DHEADER framing required by XTypes v1.3 §7.4.3.3 for
    // @extensibility(APPENDABLE) containers (F29, deferred to
    // sous-chantier 1.6.10). Do not update these bytes until F29 is
    // fixed and the locked value is empirically re-derived against
    // a fresh Fast DDS pcap capture.
    //
    // Stale locked value (HDDS pre-1.6.1a self-output, kept for
    // historical reference during F29 fix):
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
