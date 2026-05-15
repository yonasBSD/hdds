// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! CDR version negotiation at write-time.
//!
//! Implements the single-writer CDR version selection algorithm from
//! DDS-XTypes v1.3 §7.6.3.1 DataRepresentationQosPolicy.

#![allow(dead_code)]

use crate::core::discovery::multicast::fsm::EndpointInfo;
use crate::core::ser::CdrError;
use crate::core::types::TypeDescriptor;
use crate::dds::CdrVersion;

/// DDS Table 2.60 policy ID for DATA_REPRESENTATION.
pub(crate) const POLICY_ID_DATA_REPRESENTATION: u32 = 23;

/// Wire code for XCDR v1 per DDS-XTypes v1.3 §7.6.2.1.2.
const XCDR1_CODE: u16 = 0x0000;
/// Wire code for XCDR v2 per DDS-XTypes v1.3 §7.6.2.1.2.
const XCDR2_CODE: u16 = 0x0002;

/// QoS incompatibility signal carrying the DDS policy identifier
/// per DDS v1.4 Table 2.60 (23 = DATA_REPRESENTATION).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IncompatibleQos {
    pub policy_id: u32,
}

/// Compute the effective CDR encoding version for a `write()` call
/// per DDS-XTypes v1.3 §7.6.3.1 (DataRepresentationQosPolicy matching).
///
/// A single version is chosen from the writer's `offered` sequence so
/// that at least one matched reader can decode it; preference follows
/// the writer's declared order. Readers with an empty `data_representation`
/// sequence are treated as accepting any representation.
///
/// # Arguments
///
/// * `offered` — writer's offered CDR codes (empty = writer unconstrained).
/// * `matched` — matched readers whose `qos.data_representation` carries
///   their accepted codes.
///
/// # Returns
///
/// * `Ok(CdrVersion)` when a compatible version is found.
/// * `Err(IncompatibleQos { policy_id = 23 })` when `offered` and the
///   union of matched readers' accepted sets have no overlap.
///
/// # Defaults
///
/// * `offered` empty (writer didn't constrain): expanded to the HDDS
///   profile default `[XCDR2, XCDR1]`, matching what
///   `build_sedp_rtps_packet` advertises. The local matching decision then
///   stays consistent with the SEDP-advertised offered list a remote
///   observer sees.
/// * Reader with empty `data_representation`: per DDS-XTypes v1.3
///   §7.6.3.1.2, treated as if the list contained only XCDR_V1 (back-compat
///   with DDS-XTypes 1.1). Vendors that omit `PID_DATA_REPRESENTATION`
///   (e.g. Fast DDS default profile) therefore force the writer to pick
///   the XCDR1 code from its offered list, so the wire encoding matches
///   what the reader is prepared to decode.
/// * No matched readers: pick the writer's first effective offered code.
pub(crate) fn compute_effective_cdr_version(
    offered: &[u16],
    matched: &[EndpointInfo],
) -> Result<CdrVersion, IncompatibleQos> {
    const HDDS_DEFAULT_OFFERED: [u16; 2] = [XCDR2_CODE, XCDR1_CODE];
    const SPEC_DEFAULT_ACCEPTED: [u16; 1] = [XCDR1_CODE];

    let effective_offered: &[u16] = if offered.is_empty() {
        &HDDS_DEFAULT_OFFERED
    } else {
        offered
    };

    if matched.is_empty() {
        return code_to_version(effective_offered[0]);
    }

    for &code in effective_offered {
        let accepted = matched.iter().any(|r| {
            let effective = if r.qos.data_representation.is_empty() {
                &SPEC_DEFAULT_ACCEPTED[..]
            } else {
                &r.qos.data_representation[..]
            };
            effective.contains(&code)
        });
        if accepted {
            return code_to_version(code);
        }
    }

    Err(IncompatibleQos {
        policy_id: POLICY_ID_DATA_REPRESENTATION,
    })
}

fn code_to_version(code: u16) -> Result<CdrVersion, IncompatibleQos> {
    match code {
        XCDR1_CODE => Ok(CdrVersion::Xcdr1),
        XCDR2_CODE => Ok(CdrVersion::Xcdr2),
        _ => Err(IncompatibleQos {
            policy_id: POLICY_ID_DATA_REPRESENTATION,
        }),
    }
}

/// Resolve a writer's "stable" CDR version, ignoring matched readers.
///
/// Used by the retransmit and history-replay paths, which lack a per-write
/// matched-reader set: the cache stores raw CDR-encoded payload bytes
/// without per-sample version metadata, so wire decisions there must use
/// the writer's first-offered version (or `Xcdr2` for an empty offered
/// list, per HDDS profile defaults).
pub(crate) fn stable_writer_version(qos: &crate::dds::qos::QoS) -> CdrVersion {
    compute_effective_cdr_version(&qos.data_representation, &[]).unwrap_or(CdrVersion::Xcdr2)
}

/// Map a type's canonical (XCDR2) RTPS encapsulation kind to the wire
/// encapsulation kind for the negotiated CDR version, per DDS-RTPS v2.5
/// §10.7 and DDS-XTypes v1.3 §7.6.2.1.2 + §7.6.3.1.2.
///
/// `RtpsEndpointContext::encapsulation_kind` stores the canonical XCDR2
/// form of the type's extensibility (`0x0007` for `@final`, `0x0009` for
/// `@appendable`, `0x000B` for `@mutable`). This helper degrades that
/// canonical kind to the matching XCDR1 wire code when the writer's
/// `effective_cdr_version()` resolves to `Xcdr1`, so the wire encap stays
/// consistent with the bytes produced by `T::encode(buf, version)`.
///
/// # Mappings
///
/// * `(0x0007, Xcdr2)` → `0x0007` PLAIN_CDR2_LE
/// * `(0x0007, Xcdr1)` → `0x0001` PLAIN_CDR_LE
/// * `(0x0009, Xcdr2)` → `0x0009` D_CDR2_LE
/// * `(0x0009, Xcdr1)` → `0x0001` PLAIN_CDR_LE  (XCDR1 has no `@appendable`
///   wire concept; degrades to plain CDR per back-compat).
/// * `(0x000B, Xcdr2)` → `0x000B` PL_CDR2_LE
/// * `(0x000B, Xcdr1)` → `0x0003` PL_CDR_LE
///
/// Any other input is passed through unchanged so legacy callers that
/// already store an XCDR1 wire code (`0x0001` or `0x0003`) keep working.
pub fn encap_kind_for_version(canonical: u16, version: CdrVersion) -> u16 {
    match (canonical, version) {
        (0x0007, CdrVersion::Xcdr2) => 0x0007,
        (0x0007, CdrVersion::Xcdr1) => 0x0001,
        (0x0009, CdrVersion::Xcdr2) => 0x0009,
        (0x0009, CdrVersion::Xcdr1) => 0x0001,
        (0x000B, CdrVersion::Xcdr2) => 0x000B,
        (0x000B, CdrVersion::Xcdr1) => 0x0003,
        (other, _) => other,
    }
}

/// Map an RTPS encapsulation `representation_id` to the matching
/// `CdrVersion` per DDS-RTPS v2.5 §10.7 and DDS-XTypes v1.3 §7.6.2.2.
///
/// Valid ids (big-endian u16 in the first two bytes of the
/// serializedPayload encapsulation header):
///
/// * `0x0000` CDR_BE, `0x0001` CDR_LE, `0x0002` PL_CDR_BE,
///   `0x0003` PL_CDR_LE → XCDR v1
/// * `0x0006` PLAIN_CDR2_BE, `0x0007` PLAIN_CDR2_LE, `0x0008` D_CDR2_BE,
///   `0x0009` D_CDR2_LE, `0x000A` PL_CDR2_BE, `0x000B` PL_CDR2_LE
///   → XCDR v2 (per OMG DDS-XTypes v1.3 §7.6.3.1.2 Table 60)
///
/// Any other value indicates a malformed payload and is reported as
/// `CdrError::InvalidEncoding`.
pub(crate) fn cdr_version_from_representation_id(repr_id: u16) -> Result<CdrVersion, CdrError> {
    match repr_id {
        0x0000..=0x0003 => Ok(CdrVersion::Xcdr1),
        0x0006..=0x000B => Ok(CdrVersion::Xcdr2),
        _ => Err(CdrError::InvalidEncoding),
    }
}

/// Types mixing variable-size containers with 8-byte aligned primitives
/// require native XCDR1 encoding per DDS-XTypes v1.3 §7.4.3.4.1 Table 15
/// (natural alignment for 8-byte elements, distinct from the cap-at-4
/// alignment of PLAIN_CDR2). Until that native path is implemented,
/// callers must reject XCDR1 negotiation for these types and surface
/// INCOMPATIBLE_QOS (policy ID 23) rather than silently fall back to the
/// XCDR2 wire format via the `Cdr2Encode` trait default.
///
/// This is a conservative check: it flags any type that combines the
/// `is_variable_size` flag with `alignment >= 8`, even when the
/// variable-size member does not itself host an 8-byte primitive.
/// Refining it requires walking `fields` recursively; that refinement is
/// deliberately deferred until the native path exists.
pub(crate) fn type_requires_native_xcdr1(desc: &TypeDescriptor) -> bool {
    desc.is_variable_size && desc.alignment >= 8
}

/// Pairwise DataRepresentation compatibility check per DDS-XTypes v1.3
/// §7.6.3.1, matching a single remote endpoint against a local offered
/// sequence. Equivalent to calling `compute_effective_cdr_version` with a
/// single-reader matched slice, but avoids constructing an `EndpointInfo`
/// at the match-time call site.
///
/// * `offered` — writer-side offered codes (empty = writer unconstrained).
/// * `accepted` — reader-side accepted codes (empty = reader unconstrained).
pub(crate) fn pair_effective_cdr_version(
    offered: &[u16],
    accepted: &[u16],
) -> Result<CdrVersion, IncompatibleQos> {
    const HDDS_DEFAULT_OFFERED: [u16; 2] = [XCDR2_CODE, XCDR1_CODE];
    const SPEC_DEFAULT_ACCEPTED: [u16; 1] = [XCDR1_CODE];

    let effective_offered: &[u16] = if offered.is_empty() {
        &HDDS_DEFAULT_OFFERED
    } else {
        offered
    };
    let effective_accepted: &[u16] = if accepted.is_empty() {
        &SPEC_DEFAULT_ACCEPTED
    } else {
        accepted
    };

    for &code in effective_offered {
        if effective_accepted.contains(&code) {
            return code_to_version(code);
        }
    }

    Err(IncompatibleQos {
        policy_id: POLICY_ID_DATA_REPRESENTATION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::discovery::multicast::fsm::{EndpointInfo, EndpointKind};
    use crate::core::discovery::GUID;
    use crate::dds::qos::QoS;

    fn reader_with(accepted: Vec<u16>) -> EndpointInfo {
        let qos = QoS {
            data_representation: accepted,
            ..QoS::default()
        };
        let guid = GUID::from_bytes([0; 16]);
        EndpointInfo {
            endpoint_guid: guid,
            participant_guid: guid,
            topic_name: String::new(),
            type_name: String::new(),
            qos,
            kind: EndpointKind::Reader,
            type_object: None,
            has_explicit_ownership: false,
            has_ownership_strength: false,
        }
    }

    #[test]
    fn empty_offered_empty_matched_defaults_to_xcdr2() {
        assert_eq!(
            compute_effective_cdr_version(&[], &[]),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn xcdr2_offered_no_matched_returns_xcdr2() {
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE], &[]),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn xcdr1_offered_no_matched_returns_xcdr1() {
        assert_eq!(
            compute_effective_cdr_version(&[XCDR1_CODE], &[]),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn xcdr2_first_reader_accepts_xcdr2() {
        let matched = [reader_with(vec![XCDR2_CODE])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE, XCDR1_CODE], &matched),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn xcdr1_second_preference_when_reader_only_accepts_xcdr1() {
        let matched = [reader_with(vec![XCDR1_CODE])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE, XCDR1_CODE], &matched),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn first_offered_version_with_any_accepting_reader_wins() {
        let matched = [reader_with(vec![XCDR1_CODE]), reader_with(vec![XCDR2_CODE])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE, XCDR1_CODE], &matched),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn empty_intersection_yields_incompatible_qos() {
        let matched = [reader_with(vec![XCDR1_CODE])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE], &matched),
            Err(IncompatibleQos {
                policy_id: POLICY_ID_DATA_REPRESENTATION
            })
        );
    }

    #[test]
    fn empty_offered_reader_constrained_reader_imposes() {
        let matched = [reader_with(vec![XCDR1_CODE])];
        assert_eq!(
            compute_effective_cdr_version(&[], &matched),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn empty_offered_empty_accepted_picks_xcdr1_per_back_compat() {
        // DDS-XTypes v1.3 §7.6.3.1.2: empty reader data_representation is
        // interpreted as [XCDR_V1] (back-compat). Writer empty offered
        // expands to HDDS' default [XCDR2, XCDR1]; intersection picks the
        // first writer code accepted by the back-compat reader = XCDR1.
        let matched = [reader_with(vec![])];
        assert_eq!(
            compute_effective_cdr_version(&[], &matched),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn reader_with_empty_accepted_uses_back_compat_xcdr1() {
        // Empty reader = [XCDR1] per DDS-XTypes v1.3 §7.6.3.1.2.
        // Writer offered=[XCDR1] -> intersection {XCDR1} -> Xcdr1.
        // Writer offered=[XCDR2] -> intersection empty -> INCOMPATIBLE.
        let matched = [reader_with(vec![])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR1_CODE], &matched),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE], &matched),
            Err(IncompatibleQos {
                policy_id: POLICY_ID_DATA_REPRESENTATION
            })
        );
    }

    fn desc(alignment: u8, is_variable_size: bool) -> TypeDescriptor {
        TypeDescriptor {
            type_id: 0,
            type_name: "Synthetic",
            size_bytes: 0,
            alignment,
            is_variable_size,
            fields: &[],
        }
    }

    #[test]
    fn cdr_version_from_representation_id_maps_xcdr1_codes() {
        assert_eq!(
            cdr_version_from_representation_id(0x0000),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0001),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0002),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0003),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn cdr_version_from_representation_id_maps_xcdr2_codes() {
        assert_eq!(
            cdr_version_from_representation_id(0x0006),
            Ok(CdrVersion::Xcdr2)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0007),
            Ok(CdrVersion::Xcdr2)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0008),
            Ok(CdrVersion::Xcdr2)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x0009),
            Ok(CdrVersion::Xcdr2)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x000A),
            Ok(CdrVersion::Xcdr2)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x000B),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn cdr_version_from_representation_id_rejects_invalid_codes() {
        assert_eq!(
            cdr_version_from_representation_id(0x0004),
            Err(CdrError::InvalidEncoding)
        );
        assert_eq!(
            cdr_version_from_representation_id(0x00FF),
            Err(CdrError::InvalidEncoding)
        );
        assert_eq!(
            cdr_version_from_representation_id(0xFFFF),
            Err(CdrError::InvalidEncoding)
        );
    }

    #[test]
    fn type_requires_native_xcdr1_true_for_variable_8_byte_aligned() {
        assert!(type_requires_native_xcdr1(&desc(8, true)));
    }

    #[test]
    fn type_requires_native_xcdr1_false_for_variable_4_byte_aligned() {
        assert!(!type_requires_native_xcdr1(&desc(4, true)));
    }

    #[test]
    fn type_requires_native_xcdr1_false_for_fixed_8_byte_aligned() {
        assert!(!type_requires_native_xcdr1(&desc(8, false)));
    }

    #[test]
    fn type_requires_native_xcdr1_false_for_fixed_1_byte_aligned() {
        assert!(!type_requires_native_xcdr1(&desc(1, false)));
    }

    #[test]
    fn pair_empty_offered_empty_accepted_picks_xcdr1() {
        // Writer empty -> HDDS default [XCDR2, XCDR1].
        // Reader empty -> back-compat [XCDR1] per DDS-XTypes v1.3 §7.6.3.1.2.
        // Intersection picks XCDR1.
        assert_eq!(pair_effective_cdr_version(&[], &[]), Ok(CdrVersion::Xcdr1));
    }

    #[test]
    fn pair_writer_offered_empty_reader_uses_back_compat() {
        // Reader empty -> [XCDR1] (back-compat). Writer XCDR1 only -> match.
        assert_eq!(
            pair_effective_cdr_version(&[XCDR1_CODE], &[]),
            Ok(CdrVersion::Xcdr1)
        );
        // Writer XCDR2 only vs back-compat [XCDR1] reader -> incompatible.
        assert_eq!(
            pair_effective_cdr_version(&[XCDR2_CODE], &[]),
            Err(IncompatibleQos {
                policy_id: POLICY_ID_DATA_REPRESENTATION
            })
        );
    }

    #[test]
    fn pair_reader_accepted_no_writer_offered_reader_imposes() {
        assert_eq!(
            pair_effective_cdr_version(&[], &[XCDR1_CODE]),
            Ok(CdrVersion::Xcdr1)
        );
    }

    #[test]
    fn pair_both_constrained_picks_first_offered_in_accepted() {
        assert_eq!(
            pair_effective_cdr_version(&[XCDR2_CODE, XCDR1_CODE], &[XCDR1_CODE]),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            pair_effective_cdr_version(&[XCDR2_CODE, XCDR1_CODE], &[XCDR1_CODE, XCDR2_CODE]),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn pair_empty_intersection_yields_incompatible_qos() {
        assert_eq!(
            pair_effective_cdr_version(&[XCDR2_CODE], &[XCDR1_CODE]),
            Err(IncompatibleQos {
                policy_id: POLICY_ID_DATA_REPRESENTATION
            })
        );
    }

    #[test]
    fn unknown_code_in_offered_is_incompatible() {
        // 0x0001 is not a valid CDR version code per §7.6.2.1.2.
        let matched = [reader_with(vec![0x0001])];
        assert_eq!(
            compute_effective_cdr_version(&[0x0001], &matched),
            Err(IncompatibleQos {
                policy_id: POLICY_ID_DATA_REPRESENTATION
            })
        );
    }

    #[test]
    fn encap_kind_for_version_final_xcdr2_keeps_plain_cdr2_le() {
        assert_eq!(
            encap_kind_for_version(0x0007, CdrVersion::Xcdr2),
            0x0007,
            "@final + Xcdr2 should stay PLAIN_CDR2_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_final_xcdr1_degrades_to_plain_cdr_le() {
        assert_eq!(
            encap_kind_for_version(0x0007, CdrVersion::Xcdr1),
            0x0001,
            "@final + Xcdr1 should degrade to PLAIN_CDR_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_appendable_xcdr2_keeps_d_cdr2_le() {
        assert_eq!(
            encap_kind_for_version(0x0009, CdrVersion::Xcdr2),
            0x0009,
            "@appendable + Xcdr2 should stay D_CDR2_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_appendable_xcdr1_degrades_to_plain_cdr_le() {
        // XCDR1 has no @appendable wire concept (no DHEADER); fall back to plain.
        assert_eq!(
            encap_kind_for_version(0x0009, CdrVersion::Xcdr1),
            0x0001,
            "@appendable + Xcdr1 should degrade to PLAIN_CDR_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_mutable_xcdr2_keeps_pl_cdr2_le() {
        assert_eq!(
            encap_kind_for_version(0x000B, CdrVersion::Xcdr2),
            0x000B,
            "@mutable + Xcdr2 should stay PL_CDR2_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_mutable_xcdr1_degrades_to_pl_cdr_le() {
        assert_eq!(
            encap_kind_for_version(0x000B, CdrVersion::Xcdr1),
            0x0003,
            "@mutable + Xcdr1 should degrade to PL_CDR_LE"
        );
    }

    #[test]
    fn encap_kind_for_version_passes_through_legacy_xcdr1_codes() {
        // Legacy callers that already store an XCDR1 wire code keep it.
        assert_eq!(encap_kind_for_version(0x0001, CdrVersion::Xcdr1), 0x0001);
        assert_eq!(encap_kind_for_version(0x0001, CdrVersion::Xcdr2), 0x0001);
        assert_eq!(encap_kind_for_version(0x0003, CdrVersion::Xcdr1), 0x0003);
    }

    #[test]
    fn stable_writer_version_empty_qos_defaults_to_xcdr2() {
        let qos = QoS::default();
        assert_eq!(stable_writer_version(&qos), CdrVersion::Xcdr2);
    }

    #[test]
    fn stable_writer_version_honors_first_offered_xcdr1() {
        let qos = QoS {
            data_representation: vec![XCDR1_CODE, XCDR2_CODE],
            ..QoS::default()
        };
        assert_eq!(stable_writer_version(&qos), CdrVersion::Xcdr1);
    }

    #[test]
    fn stable_writer_version_honors_first_offered_xcdr2() {
        let qos = QoS {
            data_representation: vec![XCDR2_CODE, XCDR1_CODE],
            ..QoS::default()
        };
        assert_eq!(stable_writer_version(&qos), CdrVersion::Xcdr2);
    }
}
