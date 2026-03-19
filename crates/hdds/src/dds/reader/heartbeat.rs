// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Heartbeat processing for DataReader.
//!
//! Handles incoming RTPS HEARTBEAT messages from writers and triggers
//! ACKNACK responses for missing samples via the NackScheduler.
//!
//! ## RTPS Reliable Protocol Flow
//!
//! ```text
//! Writer                              Reader
//!   ├──DATA(1,2,3,4,5)──────────────────▶  (3 lost)
//!   ├──HEARTBEAT(first=1,last=5)────────▶
//!   │                                   │
//!   ◀──────────ACKNACK(missing={3})─────┤  ← This module sends this
//!   │                                   │
//!   ├──DATA(3) retransmit───────────────▶
//! ```

use crate::core::discovery::multicast::DiscoveryFsm;
use crate::engine::HeartbeatHandler;
use crate::protocol::builder::build_acknack_packet_with_final;
use crate::reliability::{HeartbeatMsg, HeartbeatRx, NackScheduler};
use crate::transport::UdpTransport;
use std::cmp;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Context for sending ACKNACK responses to the writer.
pub(super) struct AcknackContext {
    /// Our GUID prefix (12 bytes)
    pub our_guid_prefix: [u8; 12],
    /// Reader entity ID (4 bytes)
    pub reader_entity_id: [u8; 4],
    /// UDP transport for sending ACKNACK
    pub transport: Arc<UdpTransport>,
    /// v219: Discovery FSM for resolving peer metatraffic address.
    /// ACKNACKs must go to the metatraffic port, not the user data port.
    pub discovery_fsm: Option<Arc<DiscoveryFsm>>,
}

pub(super) struct ReaderHeartbeatHandler {
    heartbeat_rx: Mutex<HeartbeatRx>,
    nack_scheduler: Arc<Mutex<NackScheduler>>,
    last_seen: Mutex<u64>,
    /// ACKNACK context (None for intra-process mode)
    acknack_ctx: Option<AcknackContext>,
    /// ACKNACK counter (monotonically increasing per RTPS spec)
    acknack_count: AtomicU32,
}

impl ReaderHeartbeatHandler {
    pub fn new(nack_scheduler: Arc<Mutex<NackScheduler>>) -> Self {
        Self {
            heartbeat_rx: Mutex::new(HeartbeatRx::new()),
            nack_scheduler,
            last_seen: Mutex::new(0),
            acknack_ctx: None,
            acknack_count: AtomicU32::new(1),
        }
    }

    /// Create with ACKNACK sending capability for cross-process RELIABLE.
    pub fn with_acknack_context(
        nack_scheduler: Arc<Mutex<NackScheduler>>,
        our_guid_prefix: [u8; 12],
        reader_entity_id: [u8; 4],
        transport: Arc<UdpTransport>,
        discovery_fsm: Option<Arc<DiscoveryFsm>>,
    ) -> Self {
        Self {
            heartbeat_rx: Mutex::new(HeartbeatRx::new()),
            nack_scheduler,
            last_seen: Mutex::new(0),
            acknack_ctx: Some(AcknackContext {
                our_guid_prefix,
                reader_entity_id,
                transport,
                discovery_fsm,
            }),
            acknack_count: AtomicU32::new(1),
        }
    }
}

/// v219: Resolve peer's metatraffic unicast address from SPDP discovery data.
///
/// RTPS control messages (HEARTBEAT, ACKNACK, GAP) are exchanged on the
/// metatraffic channel. When a HEARTBEAT arrives on the user-data channel
/// (e.g. compound [DATA+HEARTBEAT] from FastDDS), the UDP source address
/// is the user-data port -- NOT the metatraffic port. Sending an ACKNACK
/// back to the user-data port means FastDDS silently drops it.
///
/// This function looks up the peer's declared metatraffic unicast locator
/// from SPDP, falling back to `source_addr` if the peer is not yet known.
fn resolve_metatraffic_dest(
    peer_guid_prefix: &[u8; 12],
    source_addr: SocketAddr,
    fsm: &Option<Arc<DiscoveryFsm>>,
) -> SocketAddr {
    let Some(ref fsm) = fsm else {
        return source_addr;
    };

    let db = fsm.db();
    if let Ok(guard) = db.read() {
        for (guid, info) in guard.iter() {
            if &guid.as_bytes()[..12] == peer_guid_prefix {
                // Prefer endpoint on the same IP as the source
                for ep in &info.endpoints {
                    if ep.ip() == source_addr.ip() {
                        log::debug!(
                            "[reader] v219: Resolved peer metatraffic {} (was src={})",
                            ep,
                            source_addr
                        );
                        return *ep;
                    }
                }
                // No IP match -- use first available endpoint
                if let Some(ep) = info.endpoints.first() {
                    log::debug!(
                        "[reader] v219: Using first peer metatraffic {} (was src={})",
                        ep,
                        source_addr
                    );
                    return *ep;
                }
            }
        }
    }

    // Peer not in FSM yet (startup race) -- fall back to source
    log::debug!(
        "[reader] v219: Peer not in FSM, fallback to src={} (prefix={:02x?})",
        source_addr,
        &peer_guid_prefix[..4]
    );
    source_addr
}

impl HeartbeatHandler for ReaderHeartbeatHandler {
    fn on_heartbeat(&self, heartbeat_bytes: &[u8], source_addr: Option<SocketAddr>) {
        // Use full packet decoder to extract GUID prefix and entity IDs
        let hb = match HeartbeatMsg::decode_from_packet(heartbeat_bytes) {
            Some(hb) => hb,
            None => {
                // Fallback to legacy decoder for compatibility
                match HeartbeatMsg::decode_cdr2_le(heartbeat_bytes) {
                    Some(hb) => hb,
                    None => {
                        log::debug!("[reader] Failed to decode Heartbeat message");
                        return;
                    }
                }
            }
        };

        let reader_last_seen = {
            let guard = match self.last_seen.lock() {
                Ok(lock) => lock,
                Err(err) => {
                    log::debug!("[Reader::handle_heartbeat] last_seen lock poisoned, recovering");
                    err.into_inner()
                }
            };
            *guard
        };

        let mut heartbeat_rx = match self.heartbeat_rx.lock() {
            Ok(lock) => lock,
            Err(err) => {
                log::debug!("[Reader::handle_heartbeat] heartbeat_rx lock poisoned, recovering");
                err.into_inner()
            }
        };

        // Detect gaps between what we have and what writer has
        let gap_ranges = heartbeat_rx.on_heartbeat(&hb, reader_last_seen);

        // Collect missing sequences for ACKNACK
        let mut missing_seqs: Vec<u64> = Vec::new();
        if let Some(ref ranges) = gap_ranges {
            for range in ranges {
                for seq in range.clone() {
                    missing_seqs.push(seq);
                }
            }
        }

        // Update NackScheduler
        if let Some(ranges) = gap_ranges {
            let mut sched = match self.nack_scheduler.lock() {
                Ok(lock) => lock,
                Err(err) => {
                    log::debug!(
                        "[Reader::handle_heartbeat] nack_scheduler lock poisoned, recovering"
                    );
                    err.into_inner()
                }
            };

            for range in ranges {
                for seq in range {
                    sched.on_receive(seq);
                }
            }
        }

        // Send ACKNACK if we have transport context
        if let Some(ref ctx) = self.acknack_ctx {
            // Always send ACKNACK in response to HEARTBEAT (per RTPS spec)
            // - If we have gaps: bitmap contains missing sequences, Final=false
            // - If synchronized: empty bitmap, Final=true (stops HB/ACKNACK loop)
            let count = self.acknack_count.fetch_add(1, Ordering::Relaxed);

            // We need writer's GUID prefix and entity ID from the HEARTBEAT
            let writer_guid_prefix = hb.writer_guid_prefix;
            let writer_entity_id = hb.writer_entity_id;

            // Determine base sequence for bitmap
            let seq_base = if missing_seqs.is_empty() {
                hb.last_seq + 1 // We have everything up to last_seq
            } else {
                missing_seqs.iter().copied().min().unwrap_or(hb.first_seq)
            };

            // v220: Set Final flag when reader is fully synchronized (no gaps).
            // Per RTPS spec 8.3.7.1.5: Final flag = 1 tells the writer that
            // the reader does not require a response, breaking the HB/ACKNACK
            // ping-pong cycle.
            let final_flag = missing_seqs.is_empty();
            let acknack_packet = build_acknack_packet_with_final(
                ctx.our_guid_prefix,
                writer_guid_prefix,
                ctx.reader_entity_id,
                writer_entity_id,
                seq_base,
                &missing_seqs,
                count,
                final_flag,
            );

            // v219: Route ACKNACK to peer's METATRAFFIC unicast port.
            // The source_addr from the router is the raw UDP source (often the
            // user-data port). RTPS control messages (HB, ACKNACK, GAP) must be
            // sent to the metatraffic channel. Resolve via DiscoveryFsm, falling
            // back to source_addr if the peer is not yet discovered.
            let (send_result, dest) = if let Some(addr) = source_addr {
                let dest = resolve_metatraffic_dest(
                    &writer_guid_prefix,
                    addr,
                    &ctx.discovery_fsm,
                );
                let res = ctx.transport
                    .send_to_endpoint(&acknack_packet, &dest)
                    .map(|_| ());
                (res, Some(dest))
            } else {
                // Fallback to multicast (intra-vendor / legacy path)
                let res = ctx.transport.send(&acknack_packet);
                (res, None)
            };

            if let Err(e) = send_result {
                log::debug!("[reader] Failed to send ACKNACK: {}", e);
            } else {
                log::trace!(
                    "[reader] v220: Sent ACKNACK count={} base={} missing={} final={} dst={:?}",
                    count,
                    seq_base,
                    missing_seqs.len(),
                    final_flag,
                    dest,
                );
            }
        }

        if let Ok(mut guard) = self.last_seen.lock() {
            *guard = cmp::max(*guard, hb.last_seq);
        }
    }
}
