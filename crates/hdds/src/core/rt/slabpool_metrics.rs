// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! SlabPool allocation metrics.
//!
//! Atomic counters for per-tier hit rates, drop events, and the heap
//! fallback gauge.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// SlabPool allocation metrics.
///
/// Tracks the three reservation tiers (primary, large, heap fallback),
/// the drop counter for `reserve_and_write` calls that exhaust every
/// tier, and the heap-bytes gauge bounded by `MemoryPolicy.max_heap_bytes`.
#[derive(Debug)]
pub struct SlabPoolMetrics {
    /// Total reservations served from the primary slab pool (hot path).
    pub primary_hit_total: AtomicU64,
    /// Total reservations served from the secondary large pool.
    pub large_hit_total: AtomicU64,
    /// Total reservations served from the heap fallback tier.
    pub heap_fallback_total: AtomicU64,
    /// Total reservations that failed at every tier (heap budget exhausted
    /// or payload exceeds all configured pools).
    pub insert_dropped_total: AtomicU64,
    /// Current bytes held by active `SlabHandle::Heap` buffers.
    /// Bounded by `MemoryPolicy.max_heap_bytes`.
    pub heap_bytes_used: AtomicUsize,
}

impl SlabPoolMetrics {
    #[must_use]
    pub fn new() -> Self {
        Self {
            primary_hit_total: AtomicU64::new(0),
            large_hit_total: AtomicU64::new(0),
            heap_fallback_total: AtomicU64::new(0),
            insert_dropped_total: AtomicU64::new(0),
            heap_bytes_used: AtomicUsize::new(0),
        }
    }

    /// Snapshot of all counters.
    ///
    /// Returns `(primary_hit, large_hit, heap_fallback, insert_dropped,
    /// heap_bytes_used)`.
    #[must_use]
    pub fn snapshot(&self) -> (u64, u64, u64, u64, usize) {
        (
            self.primary_hit_total.load(Ordering::Relaxed),
            self.large_hit_total.load(Ordering::Relaxed),
            self.heap_fallback_total.load(Ordering::Relaxed),
            self.insert_dropped_total.load(Ordering::Relaxed),
            self.heap_bytes_used.load(Ordering::Relaxed),
        )
    }
}

impl Default for SlabPoolMetrics {
    fn default() -> Self {
        Self::new()
    }
}
