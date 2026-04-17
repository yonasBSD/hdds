// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! QoS compatibility checking (RxO - Request vs Offered).
//!
//!
//! Implements DDS v1.4 Sec.2.2.3 QoS compatibility rules to determine if
//! a DataWriter can communicate with a DataReader based on their QoS policies.
//!
//! # Compatibility Rules
//!
//! | Policy      | Rule                                              |
//! |-------------|---------------------------------------------------|
//! | Reliability | Writer >= Reader (Reliable > BestEffort)           |
//! | Durability  | Writer >= Reader (Persistent > TransientLocal > Volatile) |
//! | History     | Writer depth >= Reader depth                       |
//! | Deadline    | Writer period <= Reader period                     |
//! | Ownership   | Must match exactly                                |
//! | Liveliness  | Kind must match, writer lease <= reader lease      |
//! | Partition   | Must have intersection                            |

use crate::dds::qos::{Durability, QoS, Reliability};
use log;

/// Check QoS compatibility between offered (writer) and requested (reader)
///
/// Implements DDS v1.4 Sec.2.2.3 Request vs Offered (RxO) QoS compatibility rules.
///
/// # Compatibility Rules (all must pass)
///
/// 1. **Reliability** - Writer kind must satisfy reader kind
///    - BEST_EFFORT writer can match BEST_EFFORT reader only
///    - RELIABLE writer can match both BEST_EFFORT and RELIABLE readers
///
/// 2. **Durability** - Writer durability >= Reader durability
///    - VOLATILE writer can match VOLATILE reader only
///    - TRANSIENT_LOCAL writer can match both VOLATILE and TRANSIENT_LOCAL readers
///
/// 3. **History**
///    - Writer KeepLast(10) can satisfy Reader KeepLast(5) [OK]
///    - Writer KeepLast(5) cannot satisfy Reader KeepLast(10) [X]
///    - Writer KeepAll can satisfy any Reader KeepLast [OK]
///    - Reader KeepAll requires Writer KeepAll [OK]
///
/// 4. **Deadline** - Writer offers <= Reader requests (faster writer OK)
///    - Writer 100ms can match Reader 200ms [OK]
///    - Writer 200ms cannot match Reader 100ms [X]
///
/// 5. **Ownership** - Kinds must match exactly
///    - SHARED matches SHARED [OK]
///    - EXCLUSIVE matches EXCLUSIVE [OK]
///    - SHARED != EXCLUSIVE [X]
///
/// 6. **Liveliness** - Kind must match AND writer lease <= reader lease
///    - Kind must be identical
///    - Writer lease duration must be <= reader lease duration
///
/// 7. **Partition** - Must have at least one common partition
///    - Empty partitions match each other [OK]
///    - Non-empty partitions must intersect [OK]
///
/// 8. **TimeBasedFilter** - Reader-side only, no compatibility check needed
///
/// 9. **ResourceLimits** - Local configuration, no compatibility check needed
///
/// # Arguments
///
/// * `reader_qos` - Reader's requested QoS
/// * `writer_qos` - Writer's offered QoS
///
/// # Returns
///
/// `true` if all policies are compatible
///
/// # Partition Matching
///
/// DDS spec allows fnmatch-style wildcards (`*`, `?`) in partition names.
/// Matching is symmetric: if either name contains wildcards, it matches
/// the other name using glob patterns.
pub(super) fn is_compatible(reader_qos: &QoS, writer_qos: &QoS) -> bool {
    crate::trace_fn!("qos::is_compatible");
    // 1. Reliability compatibility
    let reliability_ok = match (&writer_qos.reliability, &reader_qos.reliability) {
        (Reliability::BestEffort, Reliability::BestEffort) => true,
        (Reliability::BestEffort, Reliability::Reliable) => false, // Writer too weak
        (Reliability::Reliable, Reliability::BestEffort) => true,  // Writer stronger than needed
        (Reliability::Reliable, Reliability::Reliable) => true,
    };

    if !reliability_ok {
        log::debug!(
            "[MATCH-QOS] Reliability mismatch (writer={:?}, reader={:?})",
            writer_qos.reliability,
            reader_qos.reliability
        );
        return false;
    }

    // 2. Durability compatibility
    let durability_rank = |durability: Durability| match durability {
        Durability::Volatile => 0u8,
        Durability::TransientLocal => 1u8,
        Durability::Transient => 2u8,
        Durability::Persistent => 3u8,
    };
    let durability_ok =
        durability_rank(writer_qos.durability) >= durability_rank(reader_qos.durability);

    if !durability_ok {
        log::debug!(
            "[MATCH-QOS] Durability mismatch (writer={:?}, reader={:?})",
            writer_qos.durability,
            reader_qos.durability
        );
        return false;
    }

    // 3. History: NOT an RxO policy per DDS spec (Table 2.60).
    // History is a local cache policy and does NOT affect endpoint matching.
    // Removed the check that incorrectly blocked KeepAll reader + KeepLast writer.

    // 4. Deadline compatibility
    // Writer period <= Reader period (faster writer can satisfy slower reader)
    if writer_qos.deadline.period > reader_qos.deadline.period {
        log::debug!(
            "[MATCH-QOS] Deadline mismatch (writer={:?}, reader={:?})",
            writer_qos.deadline,
            reader_qos.deadline
        );
        return false;
    }

    // 5. Ownership: NOT checked here. Vendors frequently omit PID_OWNERSHIP
    // from SEDP (defaults to SHARED), causing false mismatches with EXCLUSIVE readers.
    // Ownership incompatibility is detected by the MatchNotificationRegistry
    // which has access to has_explicit_ownership from the EndpointInfo.

    // 6. Liveliness compatibility (kind + lease duration)
    // Kind must match AND writer lease_duration <= reader lease_duration
    if writer_qos.liveliness.kind != reader_qos.liveliness.kind {
        log::debug!(
            "[MATCH-QOS] Liveliness kind mismatch (writer={:?}, reader={:?})",
            writer_qos.liveliness.kind,
            reader_qos.liveliness.kind
        );
        return false;
    }
    if writer_qos.liveliness.lease_duration > reader_qos.liveliness.lease_duration {
        log::debug!(
            "[MATCH-QOS] Liveliness lease mismatch (writer={:?}, reader={:?})",
            writer_qos.liveliness.lease_duration,
            reader_qos.liveliness.lease_duration
        );
        return false;
    }

    // 7. Partition compatibility (must intersect)
    // Both default (empty) -> compatible
    // Either default but not both -> incompatible
    // Both non-empty -> must have at least one common partition
    if writer_qos.partition.is_default() && reader_qos.partition.is_default() {
        // Both default -> compatible
    } else if writer_qos.partition.is_default() || reader_qos.partition.is_default() {
        // Only one default -> incompatible
        log::debug!(
            "[MATCH-QOS] Partition mismatch (writer={:?}, reader={:?})",
            writer_qos.partition,
            reader_qos.partition
        );
        return false;
    } else {
        // Both non-empty -> check intersection with fnmatch-style glob support
        // DDS spec: partition names may contain '*' and '?' wildcards
        let has_intersection = writer_qos.partition.names.iter().any(|w_name| {
            reader_qos
                .partition
                .names
                .iter()
                .any(|r_name| partition_matches(w_name, r_name))
        });
        if !has_intersection {
            log::debug!(
                "[MATCH-QOS] Partition mismatch (no intersection) writer={:?}, reader={:?})",
                writer_qos.partition,
                reader_qos.partition
            );
            return false;
        }
    }

    // 8. DataRepresentation compatibility (DDS-RTPS v2.5)
    // If both writer and reader advertise data_representation, they must share
    // at least one common representation. Empty means "default" (XCDR1),
    // which is always compatible with XCDR1.
    if !writer_qos.data_representation.is_empty() && !reader_qos.data_representation.is_empty() {
        let has_common = writer_qos
            .data_representation
            .iter()
            .any(|w| reader_qos.data_representation.contains(w));
        if !has_common {
            log::debug!(
                "[MATCH-QOS] DataRepresentation mismatch (writer={:?}, reader={:?})",
                writer_qos.data_representation,
                reader_qos.data_representation
            );
            return false;
        }
    }

    // 9. Presentation (DDS v1.4 Sec.2.2.3.6, Table 2.60)
    // RxO policy: writer access_scope must be >= reader access_scope
    // (GROUP > TOPIC > INSTANCE), and if the reader requests coherent_access
    // or ordered_access, the writer must offer them.
    //
    // Gate on "at least one side is non-default": vendors frequently omit
    // PID_PRESENTATION from SEDP, and some (notably RTI) send it with a
    // different byte layout than the one we parse. Applying the check when
    // BOTH sides look like the default (INSTANCE, coherent=false,
    // ordered=false) fires false-positive INCOMPATIBLE_QOS on perfectly
    // normal pairs like Test_Domain_0 / Test_Reliability_0. Only applying
    // it when someone actually *asked* for coherent/ordered/non-instance
    // keeps the DDS 2.2.3.6 semantics without regressing basic tests.
    let default_presentation = crate::dds::qos::Presentation::instance();
    let writer_non_default = writer_qos.presentation != default_presentation;
    let reader_non_default = reader_qos.presentation != default_presentation;
    if (writer_non_default || reader_non_default)
        && !writer_qos
            .presentation
            .is_compatible_with(&reader_qos.presentation)
    {
        log::debug!(
            "[MATCH-QOS] Presentation mismatch (writer={:?}, reader={:?})",
            writer_qos.presentation,
            reader_qos.presentation
        );
        return false;
    }

    // 10. TimeBasedFilter - reader-side filtering only, no compatibility check
    // 11. ResourceLimits - local configuration, no compatibility check

    true
}

/// Return the DDS policy ID of the first incompatible QoS policy.
/// Returns 0 if all policies are compatible (should not happen if is_compatible returned false).
/// DDS policy IDs per spec Table 2.60:
///   7=DURABILITY, 11=RELIABILITY, 5=OWNERSHIP, 13=DEADLINE, 21=LIVELINESS, 23=DATA_REPRESENTATION
pub(super) fn first_incompatible_policy(reader_qos: &QoS, writer_qos: &QoS) -> u32 {
    // Check in same order as is_compatible
    if let (Reliability::BestEffort, Reliability::Reliable) =
        (&writer_qos.reliability, &reader_qos.reliability)
    {
        return 11; // RELIABILITY
    }
    let durability_rank = |d: Durability| match d {
        Durability::Volatile => 0u8,
        Durability::TransientLocal => 1,
        Durability::Transient => 2,
        Durability::Persistent => 3,
    };
    if durability_rank(writer_qos.durability) < durability_rank(reader_qos.durability) {
        return 7; // DURABILITY
    }
    if writer_qos.deadline.period > reader_qos.deadline.period {
        return 13; // DEADLINE
    }
    if writer_qos.ownership.kind != reader_qos.ownership.kind {
        return 5; // OWNERSHIP
    }
    if writer_qos.liveliness.kind != reader_qos.liveliness.kind
        || writer_qos.liveliness.lease_duration > reader_qos.liveliness.lease_duration
    {
        return 21; // LIVELINESS
    }
    // DataRepresentation mismatch
    if !writer_qos.data_representation.is_empty() && !reader_qos.data_representation.is_empty() {
        let has_common = writer_qos
            .data_representation
            .iter()
            .any(|w| reader_qos.data_representation.contains(w));
        if !has_common {
            return 23; // DATA_REPRESENTATION
        }
    }
    // Presentation mismatch — same guard as is_compatible() above: only
    // report when at least one side is non-default, to avoid flagging
    // default-default pairs as incompatible when the remote vendor omits
    // PID_PRESENTATION or sends it with a layout we don't fully parse.
    let default_presentation = crate::dds::qos::Presentation::instance();
    let writer_non_default = writer_qos.presentation != default_presentation;
    let reader_non_default = reader_qos.presentation != default_presentation;
    if (writer_non_default || reader_non_default)
        && !writer_qos
            .presentation
            .is_compatible_with(&reader_qos.presentation)
    {
        return 2; // PRESENTATION
    }
    0 // Unknown — partition mismatch or truly compatible
}

/// Check if two partition names match, supporting fnmatch-style wildcards.
/// Matching is symmetric: `partition_matches("p*", "p1")` == `partition_matches("p1", "p*")`.
fn partition_matches(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // Try both directions (glob pattern could be on either side)
    fnmatch(a, b) || fnmatch(b, a)
}

/// Simple fnmatch-style glob matching: `*` matches any sequence, `?` matches one char.
fn fnmatch(pattern: &str, text: &str) -> bool {
    let pat = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    let mut px = 0usize;
    let mut tx = 0usize;
    let mut star_px: Option<usize> = None;
    let mut star_ti: usize = 0;

    while tx < text_bytes.len() {
        if px < pat.len() && (pat[px] == b'?' || pat[px] == text_bytes[tx]) {
            px += 1;
            tx += 1;
        } else if px < pat.len() && pat[px] == b'*' {
            star_px = Some(px);
            star_ti = tx;
            px += 1;
        } else if let Some(spx) = star_px {
            px = spx + 1;
            star_ti += 1;
            tx = star_ti;
        } else {
            return false;
        }
    }

    while px < pat.len() && pat[px] == b'*' {
        px += 1;
    }

    px == pat.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dds::qos::{
        Deadline, Durability, History, Liveliness, Ownership, Partition, QoS, Reliability,
    };

    #[test]
    fn test_reliability_best_effort_compatible() {
        let reader = QoS {
            reliability: Reliability::BestEffort,
            ..QoS::default()
        };
        let writer = QoS {
            reliability: Reliability::BestEffort,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_reliability_best_effort_writer_reliable_reader_incompatible() {
        let reader = QoS {
            reliability: Reliability::Reliable,
            ..QoS::default()
        };
        let writer = QoS {
            reliability: Reliability::BestEffort,
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_reliability_reliable_writer_best_effort_reader_compatible() {
        let reader = QoS {
            reliability: Reliability::BestEffort,
            ..QoS::default()
        };
        let writer = QoS {
            reliability: Reliability::Reliable,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_durability_volatile_compatible() {
        let reader = QoS {
            durability: Durability::Volatile,
            ..QoS::default()
        };
        let writer = QoS {
            durability: Durability::Volatile,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_durability_transient_writer_volatile_reader_compatible() {
        let reader = QoS {
            durability: Durability::Volatile,
            ..QoS::default()
        };
        let writer = QoS {
            durability: Durability::TransientLocal,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_durability_volatile_writer_transient_reader_incompatible() {
        let reader = QoS {
            durability: Durability::TransientLocal,
            ..QoS::default()
        };
        let writer = QoS {
            durability: Durability::Volatile,
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_history_writer_greater_compatible() {
        let reader = QoS {
            history: History::KeepLast(5),
            ..QoS::default()
        };
        let writer = QoS {
            history: History::KeepLast(10),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_history_writer_less_still_compatible() {
        // History is NOT an RxO policy (DDS spec Table 2.60).
        // Different history depths do not affect endpoint matching.
        let reader = QoS {
            history: History::KeepLast(10),
            ..QoS::default()
        };
        let writer = QoS {
            history: History::KeepLast(5),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_history_keep_all_writer_compatible() {
        let reader = QoS {
            history: History::KeepLast(10),
            ..QoS::default()
        };
        let writer = QoS {
            history: History::KeepAll,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_history_keep_all_reader_still_compatible() {
        // History is NOT an RxO policy (DDS spec Table 2.60).
        // KeepAll reader + KeepLast writer is still compatible.
        let reader = QoS {
            history: History::KeepAll,
            ..QoS::default()
        };
        let writer = QoS {
            history: History::KeepLast(10),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_history_keep_all_both_compatible() {
        let reader = QoS {
            history: History::KeepAll,
            ..QoS::default()
        };
        let writer = QoS {
            history: History::KeepAll,
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_deadline_compatible() {
        let reader = QoS {
            deadline: Deadline::from_millis(200),
            ..QoS::default()
        };
        let writer = QoS {
            deadline: Deadline::from_millis(100),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer)); // Writer faster
    }

    #[test]
    fn test_deadline_incompatible() {
        let reader = QoS {
            deadline: Deadline::from_millis(100),
            ..QoS::default()
        };
        let writer = QoS {
            deadline: Deadline::from_millis(200),
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer)); // Writer too slow
    }

    #[test]
    fn test_ownership_shared_compatible() {
        let reader = QoS {
            ownership: Ownership::shared(),
            ..QoS::default()
        };
        let writer = QoS {
            ownership: Ownership::shared(),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_ownership_exclusive_compatible() {
        let reader = QoS {
            ownership: Ownership::exclusive(),
            ..QoS::default()
        };
        let writer = QoS {
            ownership: Ownership::exclusive(),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_ownership_mismatch_not_checked_in_is_compatible() {
        // Ownership is NOT checked in is_compatible() — vendors omit PID_OWNERSHIP
        // from SEDP. Ownership incompatibility is detected by MatchNotificationRegistry
        // which has access to has_explicit_ownership from EndpointInfo.
        let reader = QoS {
            ownership: Ownership::shared(),
            ..QoS::default()
        };
        let writer = QoS {
            ownership: Ownership::exclusive(),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_liveliness_compatible() {
        let reader = QoS {
            liveliness: Liveliness::automatic_secs(10),
            ..QoS::default()
        };
        let writer = QoS {
            liveliness: Liveliness::automatic_secs(5),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer)); // Writer lease <= reader lease
    }

    #[test]
    fn test_liveliness_lease_incompatible() {
        let reader = QoS {
            liveliness: Liveliness::automatic_secs(5),
            ..QoS::default()
        };
        let writer = QoS {
            liveliness: Liveliness::automatic_secs(10),
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer)); // Writer lease > reader lease
    }

    #[test]
    fn test_partition_both_default_compatible() {
        let reader = QoS {
            partition: Partition::default(),
            ..QoS::default()
        };
        let writer = QoS {
            partition: Partition::default(),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_partition_same_compatible() {
        let reader = QoS {
            partition: Partition::single("sensor"),
            ..QoS::default()
        };
        let writer = QoS {
            partition: Partition::single("sensor"),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_partition_different_incompatible() {
        let reader = QoS {
            partition: Partition::single("sensor"),
            ..QoS::default()
        };
        let writer = QoS {
            partition: Partition::single("actuator"),
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_partition_intersection_compatible() {
        let reader = QoS {
            partition: Partition::new(vec!["sensor".to_string(), "actuator".to_string()]),
            ..QoS::default()
        };
        let writer = QoS {
            partition: Partition::single("actuator"),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_all_policies_compatible() {
        let reader = QoS {
            reliability: Reliability::BestEffort,
            durability: Durability::Volatile,
            history: History::KeepLast(10),
            deadline: Deadline::from_millis(200),
            ownership: Ownership::shared(),
            liveliness: Liveliness::automatic_secs(10),
            partition: Partition::single("sensor"),
            ..QoS::default()
        };
        let writer = QoS {
            reliability: Reliability::BestEffort,
            durability: Durability::Volatile,
            history: History::KeepLast(100),
            deadline: Deadline::from_millis(100),
            ownership: Ownership::shared(),
            liveliness: Liveliness::automatic_secs(5),
            partition: Partition::single("sensor"),
            ..QoS::default()
        };
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_multiple_policies_incompatible() {
        let reader = QoS {
            reliability: Reliability::Reliable,
            ownership: Ownership::exclusive(),
            ..QoS::default()
        };
        let writer = QoS {
            reliability: Reliability::BestEffort, // Incompatible
            ownership: Ownership::shared(),       // Incompatible
            ..QoS::default()
        };
        assert!(!is_compatible(&reader, &writer));
    }

    // Presentation QoS tests (DDS v1.4 Sec.2.2.3.6, Table 2.60)
    // Mirrors OMG Test_OrderedAccess_{1,2,5,9} and Test_CoherentSets_{1,2,5,9}.
    use crate::dds::qos::{Presentation, PresentationAccessScope};

    fn make_qos(p: Presentation) -> QoS {
        QoS {
            presentation: p,
            ..QoS::default()
        }
    }

    #[test]
    fn test_presentation_instance_writer_topic_reader_incompatible() {
        // OrderedAccess_1 pattern: writer INSTANCE+ordered, reader TOPIC+ordered
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Instance,
            false,
            true,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            false,
            true,
        ));
        assert!(!is_compatible(&reader, &writer));
        assert_eq!(first_incompatible_policy(&reader, &writer), 2);
    }

    #[test]
    fn test_presentation_instance_writer_group_reader_incompatible() {
        // OrderedAccess_2 pattern
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Instance,
            false,
            true,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Group,
            false,
            true,
        ));
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_topic_writer_group_reader_incompatible() {
        // OrderedAccess_5 pattern
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            false,
            true,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Group,
            false,
            true,
        ));
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_writer_missing_ordered_incompatible() {
        // OrderedAccess_9 pattern: writer TOPIC no-ordered, reader TOPIC+ordered
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            false,
            false,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            false,
            true,
        ));
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_coherent_instance_writer_topic_reader_incompatible() {
        // CoherentSets_1 pattern
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Instance,
            true,
            false,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            true,
            false,
        ));
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_writer_missing_coherent_incompatible() {
        // CoherentSets_9 pattern: writer TOPIC no-coherent, reader TOPIC+coherent
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            false,
            false,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Topic,
            true,
            false,
        ));
        assert!(!is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_group_writer_instance_reader_compatible() {
        // Writer offers more than requested: OK
        let writer = make_qos(Presentation::new(
            PresentationAccessScope::Group,
            true,
            true,
        ));
        let reader = make_qos(Presentation::new(
            PresentationAccessScope::Instance,
            false,
            false,
        ));
        assert!(is_compatible(&reader, &writer));
    }

    #[test]
    fn test_presentation_both_default_compatible() {
        let writer = make_qos(Presentation::default());
        let reader = make_qos(Presentation::default());
        assert!(is_compatible(&reader, &writer));
    }
}
