// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! SPDP participant discovery state and metadata.
//!
//! Defines FSM states and `ParticipantInfo` for tracking discovered peers.
//! Each participant transitions through: Idle -> Announced -> Discovered -> Active.
//!
//! # Stale SPDP liveness filter (v248)
//!
//! When a DDS process is killed, its periodic SPDP announcements may still
//! reside in the OS multicast socket buffer.  A new process on the same domain
//! drains those stale packets in a rapid burst (< 5 ms apart).  To prevent
//! promoting a dead participant we use **time-gated probation**:
//!
//!   1. `spdp_count >= 2`, **and**
//!   2. at least `PROBATION_WINDOW_MS` (100 ms) elapsed since creation.
//!
//! Combined with the socket drain at startup (`MulticastListener::run_loop`),
//! this provides a two-layer defense against stale discovery.

use crate::core::discovery::GUID;
use std::net::SocketAddr;
use std::time::Instant;

/// FSM state for discovered participants
///
/// # States
/// - `Idle`: Initial state, no activity
/// - `Announced`: Local participant sent SPDP announce
/// - `Discovered`: Received remote SPDP, not yet bidirectional
/// - `Active`: Bidirectional communication established
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsmState {
    Idle,
    Announced,
    Discovered,
    Active,
}

/// Participant metadata from SPDP discovery
///
/// Tracks remote DDS participant state including lease management
/// and endpoint information.
///
/// # Lease Management
/// - `lease_duration_ms`: Duration before participant expires (default 100s)
/// - `last_seen`: Timestamp of last SPDP packet received
/// - `is_expired()`: Check if lease has expired
/// - `refresh()`: Update last_seen when receiving SPDP
///
/// # Stale SPDP liveness filter (v248)
/// - `created_at`: Timestamp when this entry was first created
/// - `spdp_count`: Number of SPDPs that passed the probation window
/// - `is_confirmed_alive()`: `spdp_count >= 2`
/// - `PROBATION_WINDOW_MS`: Minimum elapsed time before counting refreshes
#[derive(Debug, Clone)]
pub struct ParticipantInfo {
    /// Participant GUID (16 bytes)
    pub guid: GUID,
    /// Unicast locators (endpoints) for this participant
    pub endpoints: Vec<SocketAddr>,
    /// Lease duration in milliseconds (typically 100000 ms = 100s)
    pub lease_duration_ms: u64,
    /// Last time we received SPDP from this participant
    pub last_seen: Instant,
    /// Current FSM state
    pub state: FsmState,
    /// v248: Timestamp when this participant entry was first created.
    /// Used by the time-gated probation to distinguish rapid stale bursts
    /// (< PROBATION_WINDOW_MS apart) from live SPDP refreshes (200ms+ apart).
    pub created_at: Instant,
    /// Number of SPDP announcements counted toward liveness.
    /// A participant is considered "live" after 2+ SPDPs that pass the
    /// time-gated probation window (filters stale buffer residue from killed processes).
    pub spdp_count: u32,
}

impl ParticipantInfo {
    /// v248: Minimum elapsed time (ms) between participant creation and
    /// liveness promotion.  Stale SPDP packets from a killed process arrive
    /// in rapid bursts (< 5 ms).  Live participants send SPDP at 200 ms+
    /// intervals during the startup burst phase (see `spdp_announcer.rs`
    /// `AGGRESSIVE_INTERVAL_MS = 200`).  100 ms is safely between these two
    /// regimes and does not affect interop with FastDDS/RTI/OpenDDS
    /// initial_announcements.
    const PROBATION_WINDOW_MS: u64 = 100;

    /// Create new ParticipantInfo with current timestamp
    ///
    /// # Arguments
    /// - `guid`: Participant GUID from SPDP
    /// - `endpoints`: Unicast locators (IP:port)
    /// - `lease_duration_ms`: Lease duration in milliseconds
    ///
    /// # Examples
    /// ```
    /// use hdds::core::discovery::GUID;
    /// use hdds::core::discovery::multicast::ParticipantInfo;
    /// use std::net::SocketAddr;
    ///
    /// let guid = GUID::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    /// let endpoints = vec!["127.0.0.1:7400".parse::<SocketAddr>()
    ///     .expect("Socket address parsing should succeed")];
    /// let info = ParticipantInfo::new(guid, endpoints, 100_000);
    /// ```
    pub fn new(guid: GUID, endpoints: Vec<SocketAddr>, lease_duration_ms: u64) -> Self {
        let now = Instant::now();
        Self {
            guid,
            endpoints,
            lease_duration_ms,
            last_seen: now,
            created_at: now,
            state: FsmState::Discovered,
            spdp_count: 1,
        }
    }

    /// Check if participant lease has expired
    ///
    /// Returns true if `Instant::now() > last_seen + lease_duration`
    ///
    /// # Examples
    /// ```no_run
    /// # use hdds::core::discovery::GUID;
    /// # use hdds::core::discovery::multicast::ParticipantInfo;
    /// # let guid = GUID::zero();
    /// # let info = ParticipantInfo::new(guid, vec![], 100);
    /// if info.is_expired() {
    ///     // Remove from participant database
    /// }
    /// ```
    pub fn is_expired(&self) -> bool {
        let elapsed = self.last_seen.elapsed();
        elapsed.as_millis() as u64 > self.lease_duration_ms
    }

    /// Refresh last_seen timestamp (called on SPDP reception)
    ///
    /// Updates last_seen to current time, resetting the lease timer.
    /// The `spdp_count` is only incremented when enough time has elapsed
    /// since the participant entry was created (`PROBATION_WINDOW_MS`).
    ///
    /// # Rationale (v248)
    ///
    /// Stale SPDP packets from a killed process arrive in a rapid burst
    /// (all within a few milliseconds) because they are drained from the OS
    /// socket buffer in one shot.  A live participant sends SPDP at 200 ms+
    /// intervals (burst phase) or 3 s (steady state).  By gating the counter
    /// increment on a 100 ms window we ensure stale bursts never promote a
    /// dead participant while live peers pass through on the very first
    /// refresh after the window.
    ///
    /// # Examples
    /// ```no_run
    /// # use hdds::core::discovery::GUID;
    /// # use hdds::core::discovery::multicast::ParticipantInfo;
    /// # let guid = GUID::zero();
    /// # let mut info = ParticipantInfo::new(guid, vec![], 100_000);
    /// // On receiving SPDP packet
    /// info.refresh();
    /// ```
    pub fn refresh(&mut self) {
        self.last_seen = Instant::now();
        // v248: Only count toward liveness promotion if enough time has
        // elapsed since this entry was first created.  Stale packets from
        // killed processes arrive in < 5 ms bursts; live participants space
        // their SPDP announcements by at least 200 ms (burst) or 3 s (normal).
        if self.created_at.elapsed().as_millis() as u64 >= Self::PROBATION_WINDOW_MS {
            self.spdp_count = self.spdp_count.saturating_add(1);
        }
    }

    /// Check if this participant has been confirmed alive (2+ SPDP received).
    /// A stale participant from a killed process only has 1 SPDP (from the OS buffer).
    ///
    /// v248: The time-gated probation in `refresh()` ensures that rapid stale
    /// bursts cannot reach `spdp_count >= 2`.
    #[inline]
    pub fn is_confirmed_alive(&self) -> bool {
        self.spdp_count >= 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_participant_info_new() {
        let guid = GUID::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        let endpoints = vec!["127.0.0.1:7400"
            .parse()
            .expect("Socket address parsing should succeed")];
        let info = ParticipantInfo::new(guid, endpoints.clone(), 100_000);

        assert_eq!(info.guid, guid);
        assert_eq!(info.endpoints.len(), 1);
        assert_eq!(info.lease_duration_ms, 100_000);
        assert_eq!(info.state, FsmState::Discovered);
        assert_eq!(info.spdp_count, 1);
        assert!(!info.is_confirmed_alive());
    }

    #[test]
    fn test_participant_not_expired() {
        let guid = GUID::zero();
        let info = ParticipantInfo::new(guid, vec![], 100_000);

        // Should not be expired immediately
        assert!(!info.is_expired());
    }

    #[test]
    fn test_participant_expired() {
        let guid = GUID::zero();
        let info = ParticipantInfo::new(guid, vec![], 50); // 50ms lease

        // Wait for lease to expire
        std::thread::sleep(std::time::Duration::from_millis(60));

        assert!(info.is_expired());
    }

    #[test]
    fn test_participant_refresh() {
        let guid = GUID::zero();
        let mut info = ParticipantInfo::new(guid, vec![], 100_000);
        assert_eq!(info.spdp_count, 1);

        // Wait a bit (but within probation window)
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Refresh should reset timer but NOT increment spdp_count (within probation)
        info.refresh();

        // Should not be expired
        assert!(!info.is_expired());
    }

    #[test]
    fn test_probation_blocks_rapid_stale_burst() {
        // Simulate stale packets: multiple refresh() calls within < 100ms
        let guid = GUID::zero();
        let mut info = ParticipantInfo::new(guid, vec![], 100_000);
        assert_eq!(info.spdp_count, 1);
        assert!(!info.is_confirmed_alive());

        // Rapid refreshes (simulating stale buffer drain) -- should NOT increment
        for _ in 0..10 {
            info.refresh();
        }
        assert_eq!(
            info.spdp_count, 1,
            "stale burst must not increment spdp_count"
        );
        assert!(
            !info.is_confirmed_alive(),
            "stale burst must not promote participant"
        );
    }

    #[test]
    fn test_probation_allows_live_participant() {
        // Simulate live participant: refresh() after PROBATION_WINDOW_MS
        let guid = GUID::zero();
        let mut info = ParticipantInfo::new(guid, vec![], 100_000);
        assert_eq!(info.spdp_count, 1);
        assert!(!info.is_confirmed_alive());

        // Wait beyond probation window
        std::thread::sleep(std::time::Duration::from_millis(
            ParticipantInfo::PROBATION_WINDOW_MS + 20,
        ));

        info.refresh();
        assert_eq!(info.spdp_count, 2);
        assert!(
            info.is_confirmed_alive(),
            "live participant must be promoted after probation window"
        );
    }

    #[test]
    fn test_probation_mixed_stale_then_live() {
        // Stale burst followed by a legitimate refresh after the window
        let guid = GUID::zero();
        let mut info = ParticipantInfo::new(guid, vec![], 100_000);

        // Rapid stale burst
        for _ in 0..5 {
            info.refresh();
        }
        assert_eq!(info.spdp_count, 1);
        assert!(!info.is_confirmed_alive());

        // Wait beyond probation window, then one legitimate refresh
        std::thread::sleep(std::time::Duration::from_millis(
            ParticipantInfo::PROBATION_WINDOW_MS + 20,
        ));

        info.refresh();
        assert_eq!(info.spdp_count, 2);
        assert!(info.is_confirmed_alive());
    }

    #[test]
    fn test_fsm_state_transitions() {
        let guid = GUID::zero();
        let mut info = ParticipantInfo::new(guid, vec![], 100_000);

        // Starts in Discovered state
        assert_eq!(info.state, FsmState::Discovered);

        // Can transition to Active
        info.state = FsmState::Active;
        assert_eq!(info.state, FsmState::Active);

        // Can transition to Announced
        info.state = FsmState::Announced;
        assert_eq!(info.state, FsmState::Announced);
    }
}
