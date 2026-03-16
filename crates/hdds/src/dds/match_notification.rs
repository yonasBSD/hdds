// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Middleware-level match notification (DDS spec 2.2.2.4).
//!
//! Bridges discovery events to writer/reader listeners, firing
//! `on_publication_matched` and `on_subscription_matched` callbacks
//! when compatible remote endpoints are discovered.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

use crate::core::discovery::multicast::{
    DiscoveryFsm, DiscoveryListener, EndpointInfo, EndpointKind,
};
use crate::core::discovery::Matcher;
use crate::core::discovery::GUID;
use crate::dds::qos::QoS;

/// Type-erased match callback.
/// Args: (total_count, total_count_change, current_count, current_count_change, last_remote_guid)
type MatchCallback = Box<dyn Fn(u32, i32, u32, i32, Option<GUID>) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalKind {
    Writer,
    Reader,
}

struct MatchEntry {
    id: u64,
    topic: String,
    qos: QoS,
    kind: LocalKind,
    callback: MatchCallback,
    matched_remotes: Mutex<HashSet<GUID>>,
    total_count: AtomicU32,
}

/// Token returned when registering a match callback.
/// Unregisters from the registry on drop.
pub(crate) struct MatchToken {
    registry: Weak<MatchNotificationRegistry>,
    id: u64,
}

impl Drop for MatchToken {
    fn drop(&mut self) {
        if let Some(reg) = self.registry.upgrade() {
            reg.unregister(self.id);
        }
    }
}

impl std::fmt::Debug for MatchToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchToken")
            .field("id", &self.id)
            .finish()
    }
}

/// Registry for match notification callbacks.
///
/// Implements `DiscoveryListener` to receive remote endpoint discoveries
/// and fire `on_publication_matched` / `on_subscription_matched` callbacks
/// on local writers/readers with compatible QoS.
pub(crate) struct MatchNotificationRegistry {
    entries: RwLock<Vec<MatchEntry>>,
    discovery_fsm: Weak<DiscoveryFsm>,
    local_guid_prefix: [u8; 12],
    next_id: AtomicU64,
}

impl MatchNotificationRegistry {
    pub fn new(fsm: &Arc<DiscoveryFsm>, local_guid_prefix: [u8; 12]) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            discovery_fsm: Arc::downgrade(fsm),
            local_guid_prefix,
            next_id: AtomicU64::new(1),
        }
    }

    /// Register a local writer for match notifications.
    ///
    /// The callback fires when a compatible remote reader is discovered.
    /// Returns a MatchToken that unregisters on drop.
    pub fn register_writer(
        self: &Arc<Self>,
        topic: String,
        qos: QoS,
        callback: impl Fn(u32, i32, u32, i32, Option<GUID>) + Send + Sync + 'static,
    ) -> MatchToken {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = MatchEntry {
            id,
            topic: topic.clone(),
            qos: qos.clone(),
            kind: LocalKind::Writer,
            callback: Box::new(callback),
            matched_remotes: Mutex::new(HashSet::new()),
            total_count: AtomicU32::new(0),
        };
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.push(entry);
        }
        self.catch_up(id, LocalKind::Writer, &topic, &qos);
        MatchToken {
            registry: Arc::downgrade(self),
            id,
        }
    }

    /// Register a local reader for match notifications.
    ///
    /// The callback fires when a compatible remote writer is discovered.
    /// Returns a MatchToken that unregisters on drop.
    pub fn register_reader(
        self: &Arc<Self>,
        topic: String,
        qos: QoS,
        callback: impl Fn(u32, i32, u32, i32, Option<GUID>) + Send + Sync + 'static,
    ) -> MatchToken {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = MatchEntry {
            id,
            topic: topic.clone(),
            qos: qos.clone(),
            kind: LocalKind::Reader,
            callback: Box::new(callback),
            matched_remotes: Mutex::new(HashSet::new()),
            total_count: AtomicU32::new(0),
        };
        {
            let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
            entries.push(entry);
        }
        self.catch_up(id, LocalKind::Reader, &topic, &qos);
        MatchToken {
            registry: Arc::downgrade(self),
            id,
        }
    }

    fn unregister(&self, id: u64) {
        let mut entries = self.entries.write().unwrap_or_else(|e| e.into_inner());
        entries.retain(|e| e.id != id);
    }

    /// Catch-up: scan existing remote endpoints for matches with a newly registered entry.
    fn catch_up(&self, entry_id: u64, kind: LocalKind, topic: &str, local_qos: &QoS) {
        let fsm = match self.discovery_fsm.upgrade() {
            Some(fsm) => fsm,
            None => return,
        };

        let remote_endpoints = match kind {
            LocalKind::Writer => fsm.find_readers_for_topic(topic),
            LocalKind::Reader => fsm.find_writers_for_topic(topic),
        };

        for remote in &remote_endpoints {
            if remote.endpoint_guid.prefix == self.local_guid_prefix {
                continue;
            }
            let compatible = match kind {
                LocalKind::Writer => Matcher::is_compatible(&remote.qos, local_qos),
                LocalKind::Reader => Matcher::is_compatible(local_qos, &remote.qos),
            };
            if compatible {
                self.notify_entry(entry_id, remote.endpoint_guid);
            }
        }
    }

    fn notify_entry(&self, entry_id: u64, remote_guid: GUID) {
        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        for entry in entries.iter() {
            if entry.id == entry_id {
                let mut matched =
                    entry.matched_remotes.lock().unwrap_or_else(|e| e.into_inner());
                if matched.insert(remote_guid) {
                    let total = entry.total_count.fetch_add(1, Ordering::Relaxed) + 1;
                    let current = matched.len() as u32;
                    drop(matched);
                    (entry.callback)(total, 1, current, 1, Some(remote_guid));
                }
                return;
            }
        }
    }
}

impl DiscoveryListener for MatchNotificationRegistry {
    fn on_endpoint_discovered(&self, endpoint: EndpointInfo) {
        // Skip local endpoints — intra-process matching is handled by DomainState
        if endpoint.endpoint_guid.prefix == self.local_guid_prefix {
            return;
        }

        let entries = self.entries.read().unwrap_or_else(|e| e.into_inner());
        for entry in entries.iter() {
            if entry.topic != endpoint.topic_name {
                continue;
            }

            let compatible = match (entry.kind, endpoint.kind) {
                (LocalKind::Writer, EndpointKind::Reader) => {
                    Matcher::is_compatible(&endpoint.qos, &entry.qos)
                }
                (LocalKind::Reader, EndpointKind::Writer) => {
                    Matcher::is_compatible(&entry.qos, &endpoint.qos)
                }
                _ => false,
            };

            if compatible {
                let mut matched =
                    entry.matched_remotes.lock().unwrap_or_else(|e| e.into_inner());
                if matched.insert(endpoint.endpoint_guid) {
                    let total = entry.total_count.fetch_add(1, Ordering::Relaxed) + 1;
                    let current = matched.len() as u32;
                    drop(matched);
                    (entry.callback)(total, 1, current, 1, Some(endpoint.endpoint_guid));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn test_qos() -> QoS {
        QoS::best_effort()
    }

    #[test]
    fn register_and_unregister() {
        let fsm = Arc::new(DiscoveryFsm::new(GUID::zero(), 30_000));
        let reg = Arc::new(MatchNotificationRegistry::new(&fsm, [0; 12]));

        let call_count = Arc::new(AtomicU32::new(0));
        let cc = Arc::clone(&call_count);
        let token = reg.register_writer("test".into(), test_qos(), move |_, _, _, _, _| {
            cc.fetch_add(1, Ordering::Relaxed);
        });

        // Should have 1 entry
        assert_eq!(
            reg.entries.read().unwrap_or_else(|e| e.into_inner()).len(),
            1
        );

        // Drop token -> unregisters
        drop(token);
        assert_eq!(
            reg.entries.read().unwrap_or_else(|e| e.into_inner()).len(),
            0
        );
    }
}
