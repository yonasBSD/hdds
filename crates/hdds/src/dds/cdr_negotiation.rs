// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! CDR version negotiation at write-time.
//!
//! Implements the single-writer CDR version selection algorithm from
//! DDS-XTypes v1.3 §7.6.3.1 DataRepresentationQosPolicy.

#![allow(dead_code)]

use crate::core::discovery::multicast::fsm::EndpointInfo;
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
/// * `offered` empty + `matched` empty → `Xcdr2` (HDDS profile default).
/// * `offered` empty + `matched` non-empty, all readers unconstrained
///   → `Xcdr2`.
/// * `offered` empty + `matched` non-empty with constrained readers
///   → first code from the iteration of matched readers' accepted lists.
pub(crate) fn compute_effective_cdr_version(
    offered: &[u16],
    matched: &[EndpointInfo],
) -> Result<CdrVersion, IncompatibleQos> {
    if offered.is_empty() && matched.is_empty() {
        return Ok(CdrVersion::Xcdr2);
    }

    if offered.is_empty() {
        // Writer unconstrained: take from reader side, preferring Xcdr2 when
        // any reader is also unconstrained.
        if matched.iter().any(|r| r.qos.data_representation.is_empty()) {
            return Ok(CdrVersion::Xcdr2);
        }
        if let Some(code) = matched
            .iter()
            .flat_map(|r| r.qos.data_representation.iter().copied())
            .next()
        {
            return code_to_version(code);
        }
        return Ok(CdrVersion::Xcdr2);
    }

    if matched.is_empty() {
        return code_to_version(offered[0]);
    }

    // Both sides constrain the choice: walk offered in declared order and
    // return the first code accepted by at least one matched reader. Readers
    // with an empty accepted sequence accept any offered code.
    for &code in offered {
        let accepted = matched.iter().any(|r| {
            r.qos.data_representation.is_empty() || r.qos.data_representation.contains(&code)
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
    if offered.is_empty() && accepted.is_empty() {
        return Ok(CdrVersion::Xcdr2);
    }
    if offered.is_empty() {
        return code_to_version(accepted[0]);
    }
    if accepted.is_empty() {
        return code_to_version(offered[0]);
    }
    for &code in offered {
        if accepted.contains(&code) {
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
    fn empty_offered_reader_unconstrained_defaults_to_xcdr2() {
        let matched = [reader_with(vec![])];
        assert_eq!(
            compute_effective_cdr_version(&[], &matched),
            Ok(CdrVersion::Xcdr2)
        );
    }

    #[test]
    fn reader_with_empty_accepted_matches_any_offered() {
        let matched = [reader_with(vec![])];
        assert_eq!(
            compute_effective_cdr_version(&[XCDR1_CODE], &matched),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            compute_effective_cdr_version(&[XCDR2_CODE], &matched),
            Ok(CdrVersion::Xcdr2)
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
    fn pair_empty_offered_empty_accepted_defaults_to_xcdr2() {
        assert_eq!(pair_effective_cdr_version(&[], &[]), Ok(CdrVersion::Xcdr2));
    }

    #[test]
    fn pair_writer_offered_no_reader_accepted_returns_offered_first() {
        assert_eq!(
            pair_effective_cdr_version(&[XCDR1_CODE], &[]),
            Ok(CdrVersion::Xcdr1)
        );
        assert_eq!(
            pair_effective_cdr_version(&[XCDR2_CODE], &[]),
            Ok(CdrVersion::Xcdr2)
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
}
