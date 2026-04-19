// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Periodic Heartbeat Scheduler for RTPS Reliable QoS
//!
//! This module implements a dedicated thread that sends HEARTBEAT messages
//! at regular intervals, independent of write() calls. This is required for
//! RTPS 2.5 conformance (Section 8.4.7.2) to enable recovery after bursts.
//!
//! ## Problem Solved
//!
//! Without periodic heartbeats, a writer that bursts data and then goes idle
//! will never trigger ACKNACK responses from readers, causing permanent loss.
//!
//! ## Protocol Flow
//!
//! ```text
//! Writer                              Reader
//!   ├──DATA(1-10000) burst────────────▶  (some lost)
//!   │                                   │
//!   │  (writer idle, thread continues)  │
//!   │                                   │
//!   ├──HEARTBEAT(first=1,last=10000)──▶  (every 100ms)
//!   │                                   │
//!   ◀──────────ACKNACK(missing=[...])──┤
//!   │                                   │
//!   ├──DATA retransmit────────────────▶
//! ```

use crate::protocol::builder::{self, RtpsEndpointContext};
use crate::reliability::HistoryCache;
use crate::transport::UdpTransport;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Default heartbeat period in milliseconds (RTPS recommendation: 100ms)
pub const DEFAULT_HEARTBEAT_PERIOD_MS: u64 = 100;

/// Shared state between the writer and the heartbeat scheduler thread.
#[derive(Debug)]
pub struct HeartbeatSchedulerState {
    /// Current highest sequence number written
    pub last_seq: AtomicU64,
    /// Stop flag to terminate the thread
    pub stop: AtomicBool,
    /// Heartbeat counter (monotonically increasing per RTPS spec)
    pub count: AtomicU32,
    /// v252: For VOLATILE writers, sequence number snapshotted at the time the
    /// first reader matched via SEDP. Subsequent HEARTBEATs use `max(cache.
    /// oldest, first_eligible_seq + 1)` as `firstSN` so the matched reader
    /// never NACKs for samples that existed before it subscribed. 0 means
    /// "not yet set" (either no match has fired, or the writer is not
    /// VOLATILE and this field is intentionally left unused).
    ///
    /// Known limitation: this is a single writer-wide floor, not per-reader.
    /// For a late-joining second VOLATILE reader, we currently advertise the
    /// floor captured at the first match; samples produced between first and
    /// second match are visible to the second reader. Passing per-reader
    /// state requires a StatefulWriter refactor (RTPS 8.4.7) tracked for a
    /// follow-up session. In the single-reader case — which covers the
    /// `Test_Durability_16` scenario and the common point-to-point topology
    /// — the current floor is spec-correct.
    pub first_eligible_seq: AtomicU64,
}

impl HeartbeatSchedulerState {
    /// Create new shared state.
    pub fn new() -> Self {
        Self {
            last_seq: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            count: AtomicU32::new(1),
            first_eligible_seq: AtomicU64::new(0),
        }
    }

    /// Update the last sequence number (called by writer on each write).
    pub fn update_seq(&self, seq: u64) {
        self.last_seq.store(seq, Ordering::Release);
    }

    /// Signal the thread to stop.
    pub fn signal_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Check if stop was signaled.
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// Get and increment the heartbeat counter.
    pub fn next_count(&self) -> u32 {
        self.count.fetch_add(1, Ordering::Relaxed)
    }

    /// v252: CAS-set `first_eligible_seq` on the first call only. Subsequent
    /// calls (with a non-zero current value) are no-ops, so the floor is
    /// captured at the moment of the *first* reader match event and never
    /// slides further. Returns `true` if this call installed the value.
    pub fn bump_first_eligible(&self, value: u64) -> bool {
        self.first_eligible_seq
            .compare_exchange(0, value, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// v252: Compute the `firstSN` to advertise in HEARTBEAT, respecting the
    /// VOLATILE floor when present. When `first_eligible_seq` is 0 (never
    /// set), returns `cache_oldest` unchanged (VOLATILE writers pre-match, or
    /// non-VOLATILE writers). Otherwise returns `max(cache_oldest,
    /// first_eligible + 1)` so the reader never sees historical seqs.
    pub fn heartbeat_first_seq(&self, cache_oldest: u64) -> u64 {
        let floor = self.first_eligible_seq.load(Ordering::Acquire);
        if floor == 0 {
            cache_oldest
        } else {
            cache_oldest.max(floor + 1)
        }
    }
}

impl Default for HeartbeatSchedulerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle to the heartbeat scheduler thread.
///
/// When dropped, signals the thread to stop and waits for it to join.
pub struct HeartbeatSchedulerHandle {
    state: Arc<HeartbeatSchedulerState>,
    thread: Option<JoinHandle<()>>,
}

impl HeartbeatSchedulerHandle {
    /// Get a reference to the shared state (for updating last_seq from writer).
    pub fn state(&self) -> &Arc<HeartbeatSchedulerState> {
        &self.state
    }
}

impl Drop for HeartbeatSchedulerHandle {
    fn drop(&mut self) {
        // Signal stop
        self.state.signal_stop();

        // Wait for thread to finish
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Spawn a periodic heartbeat scheduler thread.
///
/// Returns a handle that manages the thread lifecycle. The thread will
/// send HEARTBEAT messages every `period_ms` milliseconds until the
/// handle is dropped.
///
/// # Arguments
///
/// * `transport` - UDP transport for sending heartbeats
/// * `history_cache` - History cache to get first_seq
/// * `rtps_endpoint` - RTPS context for building packets
/// * `period_ms` - Heartbeat period in milliseconds
///
/// # Returns
///
/// A handle that owns the thread. Drop the handle to stop the thread.
pub fn spawn_heartbeat_scheduler(
    transport: Arc<UdpTransport>,
    history_cache: Arc<HistoryCache>,
    rtps_endpoint: RtpsEndpointContext,
    period_ms: u64,
) -> HeartbeatSchedulerHandle {
    let state = Arc::new(HeartbeatSchedulerState::new());
    let state_clone = Arc::clone(&state);
    let period = Duration::from_millis(period_ms);

    #[allow(clippy::expect_used)] // thread spawn failure is unrecoverable
    let thread = thread::Builder::new()
        .name("hdds-heartbeat".into())
        .spawn(move || {
            heartbeat_loop(transport, history_cache, rtps_endpoint, state_clone, period);
        })
        .expect("failed to spawn heartbeat thread");

    HeartbeatSchedulerHandle {
        state,
        thread: Some(thread),
    }
}

/// Main heartbeat loop - runs until stop is signaled.
fn heartbeat_loop(
    transport: Arc<UdpTransport>,
    history_cache: Arc<HistoryCache>,
    ctx: RtpsEndpointContext,
    state: Arc<HeartbeatSchedulerState>,
    period: Duration,
) {
    log::debug!(
        "[heartbeat] Starting periodic heartbeat thread (period={:?})",
        period
    );

    while !state.should_stop() {
        thread::sleep(period);

        if state.should_stop() {
            break;
        }

        // Get sequence range
        let last_seq = state.last_seq.load(Ordering::Acquire);
        if last_seq == 0 {
            // No data written yet, skip heartbeat
            continue;
        }

        let cache_oldest = history_cache.oldest_seq().unwrap_or(1);
        // v252: Apply VOLATILE firstSN floor captured at first match (if any).
        let first_seq = state.heartbeat_first_seq(cache_oldest);
        let count = state.next_count();

        // Build and send HEARTBEAT
        let packet = builder::build_heartbeat_packet_with_context(&ctx, first_seq, last_seq, count);

        if let Err(e) = transport.send(&packet) {
            log::debug!("[heartbeat] Failed to send HEARTBEAT: {}", e);
        } else {
            log::trace!(
                "[heartbeat] Sent HEARTBEAT first={} last={} count={}",
                first_seq,
                last_seq,
                count
            );
        }
    }

    log::debug!("[heartbeat] Heartbeat thread stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_update_seq() {
        let state = HeartbeatSchedulerState::new();
        assert_eq!(state.last_seq.load(Ordering::Relaxed), 0);

        state.update_seq(42);
        assert_eq!(state.last_seq.load(Ordering::Relaxed), 42);

        state.update_seq(100);
        assert_eq!(state.last_seq.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn test_state_stop_signal() {
        let state = HeartbeatSchedulerState::new();
        assert!(!state.should_stop());

        state.signal_stop();
        assert!(state.should_stop());
    }

    #[test]
    fn test_state_count_increment() {
        let state = HeartbeatSchedulerState::new();

        assert_eq!(state.next_count(), 1);
        assert_eq!(state.next_count(), 2);
        assert_eq!(state.next_count(), 3);
    }

    /// v252: `bump_first_eligible` installs the VOLATILE floor on the first
    /// call only; subsequent bumps must not slide the value.
    #[test]
    fn bump_first_eligible_is_first_match_wins() {
        let state = HeartbeatSchedulerState::new();
        assert_eq!(state.first_eligible_seq.load(Ordering::Acquire), 0);

        assert!(state.bump_first_eligible(32), "first bump must install");
        assert_eq!(state.first_eligible_seq.load(Ordering::Acquire), 32);

        assert!(
            !state.bump_first_eligible(100),
            "second bump must be a no-op even with a larger value"
        );
        assert_eq!(
            state.first_eligible_seq.load(Ordering::Acquire),
            32,
            "floor must stay at the first-match snapshot"
        );

        assert!(
            !state.bump_first_eligible(5),
            "third bump must be a no-op too"
        );
        assert_eq!(state.first_eligible_seq.load(Ordering::Acquire), 32);
    }

    /// v252: without a floor, `heartbeat_first_seq` passes `cache_oldest`
    /// through; with a floor, it returns `max(cache_oldest, floor + 1)`.
    #[test]
    fn heartbeat_first_seq_applies_volatile_floor() {
        let state = HeartbeatSchedulerState::new();
        // No floor yet: identity on cache_oldest.
        assert_eq!(state.heartbeat_first_seq(1), 1);
        assert_eq!(state.heartbeat_first_seq(50), 50);

        // Floor installed at 32 → first advertised seq must be 33 at minimum.
        state.bump_first_eligible(32);
        assert_eq!(
            state.heartbeat_first_seq(1),
            33,
            "cache_oldest=1 must be lifted to floor+1"
        );
        assert_eq!(
            state.heartbeat_first_seq(33),
            33,
            "cache_oldest=33 must stay at 33 (tied with floor+1)"
        );
        assert_eq!(
            state.heartbeat_first_seq(50),
            50,
            "cache_oldest=50 must stay at 50 (already above floor+1)"
        );
    }
}
