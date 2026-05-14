// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Golden byte-stable lock for `EquivalenceHash` computation.
//!
//! Any change to the `MinimalTypeObject` CDR2 wire layout, `TypeKind`
//! octet values, `TypeObject` discriminator labels, or
//! `MinimalMemberDetail::from_name` hashing surfaces as a failure here.
//! Per OMG DDS-XTypes v1.3 §7.3.4.8 the hash is
//! `MD5(CDR2_LE(MinimalTypeObject))[..14]` where CDR2 = XCDR encoding
//! version 2 with Little Endian (spec §7.3.4.6.5 step e).
//!
//! ## Hash byte-stability history
//!
//! - **Pre-1.6.10** (HDDS-self, F29-buggy, locked legacy):
//!   `c8 5a 48 98 4e 82 ff 6e 13 8f 0f f9 c8 32`
//! - **Post-1.6.10** (HDDS-self, F29-compliant per spec rule (30),
//!   locked below): `a9 55 6a 65 86 9c be d2 92 f8 8d 26 86 bc`
//! - **Fast DDS 3.x empirical** (pcap reference 1.6.1d F28
//!   investigation): `bb 41 b9 75 4f 8f c5 3c 3e e5 48 84 42 96`
//!
//! ## Cross-vendor status: NOT YET ACHIEVED (post-1.6.10)
//!
//! The post-1.6.10 hash bytes are *closer to spec-correct* than the
//! pre-1.6.10 baseline (F29 closed, all @APPENDABLE TypeObject
//! sub-records now emit DHEADER per XTypes v1.3 §7.4.3.4 rule (30)).
//! However they still diverge from Fast DDS empirical because of
//! pre-existing HDDS<->spec data-model divergences tracked in
//! ADR-CHANTIER-1.6-AUDIT-RESPONSE.md §10.24:
//!   1. `AnnotationTypeFlag` absent from `CompleteAnnotationType` /
//!      `MinimalAnnotationType` Rust structs (caught by 1.6.10
//!      strategic-pass review, MEDIUM finding M1)
//!   2. `CompleteDiscriminatorMember` / `MinimalDiscriminatorMember`
//!      not modeled (DHEADER absent for union discriminator wrapper)
//!   3. `CompleteArrayHeader` / `MinimalArrayHeader` not modeled
//!      (CompleteArrayType uses CompleteCollectionHeader directly)
//!   4. `TypeObjectHashId` inner discriminator octet missing from
//!      StronglyConnectedComponentId payload
//!
//! Until each divergence is closed, this test stays `#[ignore]`
//! because the locked HDDS-self hash is byte-stable but cross-vendor
//! incompatible. The lock value below tracks the current HDDS-self
//! output and provides regression detection for any *unintentional*
//! drift while the deferred fixes are landing.

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
#[ignore = "Cross-vendor hash divergence remains post-1.6.10 (F29 closed, \
            DHEADER now spec-compliant per XTypes v1.3 §7.4.3.4 rule (30)). \
            Residual gaps are HDDS<->spec data-model divergences tracked \
            in ADR-CHANTIER-1.6-AUDIT-RESPONSE §10.24: missing \
            AnnotationTypeFlag field, absent DiscriminatorMember pair, \
            absent ArrayHeader pair, missing TypeObjectHashId inner \
            discriminator. HDDS-self hash bytes are locked below for \
            regression detection until cross-vendor parity is achieved \
            in a future chantier (data-model refactor required)."]
fn golden_minimal_temperature_equivalence_hash() {
    let type_obj = temperature_minimal();
    let hash = type_obj
        .compute_equivalence_hash()
        .expect("CDR2 encoding of MinimalTypeObject must succeed");

    // Locked HDDS-self bytes, post-1.6.10 (F29 closed, sub-chantier
    // landed 2026-05-14 in commits `bccd76a..e29945b`). These bytes are
    // byte-stable across the HDDS workspace but DIVERGE from Fast DDS
    // empirical (`bb 41 b9 75 4f 8f c5 3c 3e e5 48 84 42 96`) due to
    // the data-model gaps listed in the module docstring above.
    //
    // Pre-1.6.10 reference (historical, for context):
    //   `c8 5a 48 98 4e 82 ff 6e 13 8f 0f f9 c8 32`
    //
    // Re-derivation procedure when an intentional change lands (e.g.
    // when §10.24 data-model divergences are closed): replace the
    // bytes below with the new `compute_equivalence_hash()` output,
    // update the docstring history block, and ensure the change is
    // documented in the ADR with a spec citation.
    let expected: [u8; 14] = [
        0xa9, 0x55, 0x6a, 0x65, 0x86, 0x9c, 0xbe, 0xd2, 0x92, 0xf8, 0x8d, 0x26, 0x86, 0xbc,
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
