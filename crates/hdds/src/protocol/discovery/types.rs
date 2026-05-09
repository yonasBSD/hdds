// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

use crate::core::discovery::GUID;
use crate::xtypes::{CompleteTypeObject, EquivalenceHash};
use std::net::SocketAddr;

/// One member of `PID_TYPE_INFORMATION` — the wire-level
/// `TypeIdentifierWithDependencies` per OMG DDS-XTypes v1.3 §7.6.3.2.1.
///
/// HDDS currently stores only the discriminating hash and the
/// `typeobject_serialized_size`; the `dependent_typeids` sequence is
/// not retained on RX (Fast DDS emits a 12-byte sentinel triple in
/// place of the spec-prescribed sequence — see ADR §7bis "Caveat
/// sentinel" for the divergence rationale).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeIdentifierWithDependencies {
    /// `EquivalenceHash` extracted from the inner TypeIdentifier
    /// (`MD5(CDR2(TypeObject))[..14]` per §7.3.4.8).
    pub hash: EquivalenceHash,
    /// CDR2-serialised size of the corresponding TypeObject view.
    pub typeobject_serialized_size: u32,
}

/// Parsed `PID_TYPE_INFORMATION` (0x0075) payload per OMG DDS-XTypes v1.3
/// §7.6.3.2. Stores the two `TypeIdentifierWithDependencies` members:
/// minimal (member id 0x1001) and complete (member id 0x1002). Either may
/// be `None` if the peer omitted the corresponding TypeObject view.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypeInformation {
    pub minimal: Option<TypeIdentifierWithDependencies>,
    pub complete: Option<TypeIdentifierWithDependencies>,
}

/// Parse error types for SPDP/SEDP helpers.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    TruncatedData,
    InvalidEncapsulation,
    InvalidFormat,
    BufferTooSmall,
    EncodingError,
}

/// SEDP (Simple Endpoint Discovery Protocol) parsed data.
#[derive(Debug, Clone)]
pub struct SedpData {
    pub topic_name: String,
    pub type_name: String,
    pub participant_guid: GUID, // v110: Added for PID_PARTICIPANT_GUID (0x0050) - FastDDS interop requirement
    pub endpoint_guid: GUID,
    pub qos_hash: u64,
    pub qos: Option<crate::dds::QoS>, // v60: Added to use actual QoS values instead of hardcoding!
    pub type_object: Option<CompleteTypeObject>,
    /// Parsed `PID_TYPE_INFORMATION` (0x0075). Populated by `parse.rs` when
    /// a peer SEDP DATA(w/r) carries the PID; `None` when absent. The
    /// matcher does not consume this field today (name-based matching is
    /// the active path); the field is retained for future XTypes
    /// Assignability runtime work and for diagnostic logging.
    pub type_information: Option<TypeInformation>,
    pub unicast_locators: Vec<SocketAddr>,
    /// User data for capability advertisement (e.g., SHM transport)
    /// Format for SHM: "shm=1;host_id=XXXXXXXX;v=1"
    pub user_data: Option<String>,
    /// Whether PID_RELIABILITY was explicitly present in the SEDP data.
    /// Used to distinguish "not set" (apply spec default) from "set to BEST_EFFORT".
    pub has_explicit_reliability: bool,
    /// Whether PID_OWNERSHIP was explicitly present in the SEDP data.
    pub has_explicit_ownership: bool,
    /// Whether PID_OWNERSHIP_STRENGTH was explicitly present.
    /// If present, the writer is definitely EXCLUSIVE (SHARED doesn't have strength).
    pub has_ownership_strength: bool,
}
