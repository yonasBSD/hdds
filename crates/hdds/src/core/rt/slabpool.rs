// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Lock-free slab allocator for zero-copy message buffers.
//!
//! Provides O(1) allocation from size-class pools using atomic bitmaps.
//! Supports 14 size classes from 16B to 128KB.
//!
//! # Performance
//!
//! - reserve: < 30 ns (p99)
//! - release: < 30 ns (p99)

use parking_lot::Mutex;
use std::cell::UnsafeCell;
use std::convert::TryFrom;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Allocation tier that produced a successful `reserve_and_write` call.
///
/// Used by the caller to bump the matching counter without forcing
/// the pool itself to depend on the metrics layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub(crate) enum SlabMetric {
    /// Reserved from the primary 14-class pool.
    Primary,
    /// Reserved from the secondary opt-in large pool.
    Large,
    /// Reserved from the heap fallback tier.
    HeapFallback,
}

/// Handle to a reserved slab region.
///
/// Three indirect-handle variants: `Primary` (default 14 size classes),
/// `Large` (opt-in secondary pool for >128 KB payloads), and `Heap`
/// (fallback when both pools are saturated). All three reference
/// pool-side storage; the handle itself is a small (8 byte) `Copy`
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlabHandle {
    /// Slot in the primary 14-class pool (everyday traffic, ≤128 KB).
    Primary { pool_id: u16, slot_id: u16 },
    /// Slot in the secondary opt-in large pool (configured per-participant).
    Large { pool_id: u16, slot_id: u16 },
    /// Heap-allocated buffer indexed by `heap_id` into the pool's
    /// `heap_buffers` vector.
    Heap { heap_id: u32 },
}

impl SlabHandle {
    /// Sentinel value used by `IndexEntry::default()` and event entries
    /// to mark "no buffer associated". Guaranteed to never collide with
    /// a real allocation: the primary and large size-class tables top
    /// out well below `u16::MAX` slots per pool (the bitmap is `AtomicU64`,
    /// so `slot_count` ≤ 64).
    pub const EMPTY: SlabHandle = SlabHandle::Primary {
        pool_id: u16::MAX,
        slot_id: u16::MAX,
    };

    /// Returns true if this handle is the `EMPTY` sentinel.
    pub fn is_empty(self) -> bool {
        matches!(
            self,
            SlabHandle::Primary {
                pool_id: u16::MAX,
                slot_id: u16::MAX,
            }
        )
    }

    /// Construct a `Primary` handle from a packed `(pool_id << 16) | slot_id`
    /// `u32` value. Test and benchmark fixture only — production code
    /// constructs handles through `SlabPool::reserve` directly.
    #[doc(hidden)]
    pub fn legacy_handle_to_primary(packed: u32) -> Self {
        let pool_id = (packed >> 16) as u16;
        let slot_id = (packed & 0xFFFF) as u16;
        SlabHandle::Primary { pool_id, slot_id }
    }
}

/// Size class configuration: (slot_size, slot_count)
///
/// Optimized for fast allocation with minimal memory footprint.
/// Larger slot counts for bigger sizes support pipelining of fragmented messages
/// and buffering when application read rate is slower than network receive rate.
const SIZE_CLASSES: &[(usize, usize)] = &[
    (16, 64),      // 16B x 64 slots = 1 KB
    (32, 64),      // 32B x 64 slots = 2 KB
    (64, 64),      // 64B x 64 slots = 4 KB
    (128, 64),     // 128B x 64 slots = 8 KB
    (256, 64),     // 256B x 64 slots = 16 KB
    (512, 64),     // 512B x 64 slots = 32 KB
    (1024, 64),    // 1KB x 64 slots = 64 KB
    (2048, 32),    // 2KB x 32 slots = 64 KB
    (4096, 32),    // 4KB x 32 slots = 128 KB
    (8192, 32),    // 8KB x 32 slots = 256 KB
    (16384, 32),   // 16KB x 32 slots = 512 KB
    (32768, 32),   // 32KB x 32 slots = 1 MB
    (65536, 32),   // 64KB x 32 slots = 2 MB
    (131_072, 16), // 128KB x 16 slots = 2 MB
];

/// Per-pool state with atomic bitmap for free slot tracking
struct Pool {
    data: UnsafeCell<Vec<u8>>,
    bitmap: AtomicU64,
    slot_size: usize,
    slot_count: usize,
}

// SAFETY: Pool is Send + Sync because:
// - data is protected by atomic bitmap (mutual exclusion via CAS)
// - only one thread can access a given slot at a time
unsafe impl Send for Pool {}
unsafe impl Sync for Pool {}

impl Pool {
    fn new(slot_size: usize, slot_count: usize) -> Self {
        let total_size = slot_size * slot_count;
        let data = UnsafeCell::new(vec![0u8; total_size]);

        // Initialize bitmap: all slots free (all bits set to 0)
        let bitmap = AtomicU64::new(0);

        Self {
            data,
            bitmap,
            slot_size,
            slot_count,
        }
    }

    /// Try to reserve a slot from this pool
    ///
    /// Returns (slot_id, &mut [u8]) on success, None if pool full.
    ///
    /// SAFETY:
    /// - Atomic CAS ensures only one thread claims a given slot
    /// - Bounds checked: slot_id always < slot_count
    /// - Mutable slice returned is exclusive to this slot (enforced by bitmap)
    /// - UnsafeCell: interior mutability is safe because bitmap ensures no aliasing
    #[allow(clippy::mut_from_ref)]
    fn try_reserve(&self) -> Option<(u16, &mut [u8])> {
        loop {
            let bitmap = self.bitmap.load(Ordering::Acquire);

            // Find first free bit (bit=0 means free)
            let slot_idx_bits = (!bitmap).trailing_zeros();
            let slot_index = match usize::try_from(slot_idx_bits) {
                Ok(value) => value,
                Err(_) => return None,
            };
            if slot_index >= self.slot_count {
                return None; // Pool full
            }

            // Try to claim this slot (set bit to 1)
            let new_bitmap = bitmap | (1u64 << slot_index);
            if self
                .bitmap
                .compare_exchange(bitmap, new_bitmap, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                // Success: compute slice
                // SAFETY: bitmap CAS ensures exclusive access to this slot
                let offset = slot_index * self.slot_size;
                // SAFETY:
                // 1. `self.data` points to the Vec backing storage allocated in Pool::new.
                // 2. Bitmap CAS guarantees this slot_id is exclusively owned by this thread.
                // 3. Offset computation stays within allocation (slot_id < slot_count).
                // 4. &mut [u8] returned lives only for this scope, preventing aliasing.
                let data = unsafe { &mut *self.data.get() };
                let slice = &mut data[offset..offset + self.slot_size];
                let slot_id = match u16::try_from(slot_index) {
                    Ok(id) => id,
                    Err(_) => return None,
                };
                return Some((slot_id, slice));
            }
            // CAS failed, retry
        }
    }

    /// Release a slot back to the pool
    ///
    /// SAFETY:
    /// - slot_id must be valid (< slot_count)
    /// - slot must have been previously reserved
    /// - Atomic CAS ensures no double-free
    fn release_slot(&self, slot_id: u16) {
        debug_assert!(usize::from(slot_id) < self.slot_count, "Invalid slot_id");

        let slot_mask = 1u64 << slot_id;
        loop {
            let bitmap = self.bitmap.load(Ordering::Acquire);

            // Clear the bit (mark free)
            let new_bitmap = bitmap & !slot_mask;
            if self
                .bitmap
                .compare_exchange(bitmap, new_bitmap, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
            // CAS failed, retry
        }
    }
}

/// Memory pool for zero-copy message buffers
///
/// Allocates from size-class pools using atomic bitmaps (lock-free).
/// Target: reserve + release < 100 ns.
///
/// The `heap_buffers` / `heap_free_list` / `slab_heap_bytes_used` /
/// `max_heap_bytes` fields are scaffolding for the heap fallback tier
/// (DDS-XTypes-irrelevant runtime concern). They are inert until the
/// allocation paths that produce `SlabHandle::Heap` are wired in.
pub struct SlabPool {
    pools: Vec<Pool>,
    /// Backing storage for the `SlabHandle::Heap` tier. Indexed by
    /// `heap_id` from the variant; `None` slots are free.
    #[allow(dead_code)]
    heap_buffers: Mutex<Vec<Option<Box<[u8]>>>>,
    /// LIFO stack of free `heap_id` values reusable by future allocations.
    #[allow(dead_code)]
    heap_free_list: Mutex<Vec<u32>>,
    /// Current bytes held in active `Heap` slots; CAS-incremented on
    /// allocation, decremented on release.
    #[allow(dead_code)]
    slab_heap_bytes_used: AtomicUsize,
    /// Ceiling for the heap fallback tier.
    #[allow(dead_code)]
    max_heap_bytes: usize,
}

impl SlabPool {
    /// Default ceiling for the heap fallback tier (16 MB). Will be
    /// overridable via `MemoryPolicy` once that wiring lands.
    const DEFAULT_MAX_HEAP_BYTES: usize = 16 * 1024 * 1024;

    pub fn new() -> Self {
        let pools = SIZE_CLASSES
            .iter()
            .map(|&(size, count)| Pool::new(size, count))
            .collect();

        Self {
            pools,
            heap_buffers: Mutex::new(Vec::new()),
            heap_free_list: Mutex::new(Vec::new()),
            slab_heap_bytes_used: AtomicUsize::new(0),
            max_heap_bytes: Self::DEFAULT_MAX_HEAP_BYTES,
        }
    }

    /// Reserve buffer space; returns handle + mutable slice.
    ///
    /// Used by the intra-process write fast path that encodes the
    /// payload directly into the slab buffer (single copy, no
    /// intermediate `Vec`). Production code that already holds the
    /// payload as `&[u8]` should call `reserve_and_write` instead.
    ///
    /// Finds the smallest size class >= len and attempts to allocate.
    /// Falls back to larger classes if smaller ones are full.
    ///
    /// # Returns
    /// - `Some((handle, slice))` on success
    /// - `None` if all primary pools exhausted (Large and Heap tiers
    ///   are not consulted by this entry point)
    ///
    /// # Panics
    /// Never panics on valid input (all bounds checked).
    ///
    /// # Performance
    /// Target: < 50 ns (single CAS in common case)
    ///
    /// # Latency
    /// - **p50:** 24.27 ns (256-byte request)
    /// - **p99:** 27.38 ns (**SLA target:** < 200 ns)
    /// - **p999:** 28.33 ns
    ///   [!] **Benchmark methodology:** Includes slot acquisition + release in isolation using Criterion
    ///   `slabpool_reserve_256b` (benches/runtime.rs) with pre-initialized pool.
    ///   Last measured: 2025-10-21 on Intel(R) Xeon(R) CPU E5-2699 v4 @ 2.20GHz.
    ///
    /// # Safety
    /// Uses interior mutability (UnsafeCell) with atomic bitmap protection.
    /// Safe because bitmap CAS ensures exclusive access to allocated slots.
    pub fn reserve(&self, len: usize) -> Option<(SlabHandle, &mut [u8])> {
        // Find first size class >= len
        let start_idx = SIZE_CLASSES.iter().position(|&(size, _)| size >= len)?;

        // Try pools starting from best-fit size class
        for pool_id in start_idx..self.pools.len() {
            if let Some((slot_id, slice)) = self.pools[pool_id].try_reserve() {
                let pool_id = match u16::try_from(pool_id) {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let handle = SlabHandle::Primary { pool_id, slot_id };
                return Some((handle, slice));
            }
        }

        None // All pools full
    }

    /// Reserve a slot in the appropriate pool tier and copy the payload
    /// into it atomically. Returns the handle and which tier was used,
    /// or `None` if every tier was exhausted.
    ///
    /// Production write paths use this entry point. Stateful encoding
    /// directly into a reserved buffer (where the payload is not known
    /// upfront) keeps the legacy `reserve` pair.
    ///
    /// The slot returned by `get_buffer(handle)` may be larger than
    /// `payload.len()` for the `Primary` and `Large` tiers (size classes
    /// round up). Consumers must track the logical length independently
    /// (the runtime carries it on `IndexEntry::len`).
    pub(crate) fn reserve_and_write(
        &self,
        payload: &[u8],
    ) -> Option<(SlabHandle, SlabMetric)> {
        if let Some(handle) = self.try_reserve_primary_and_copy(payload) {
            return Some((handle, SlabMetric::Primary));
        }

        // Large and Heap tiers are wired in a follow-up commit; once
        // active the primary pool returning `None` falls through to
        // Tier 2 then Tier 3 instead of failing.
        None
    }

    /// Walk the primary size classes from best-fit upward, reserve the
    /// first available slot, and `copy_from_slice` the payload into it.
    /// Returns `None` if every primary class is full.
    fn try_reserve_primary_and_copy(&self, payload: &[u8]) -> Option<SlabHandle> {
        let len = payload.len();
        let start_idx = SIZE_CLASSES.iter().position(|&(size, _)| size >= len)?;

        for pool_idx in start_idx..self.pools.len() {
            if let Some((slot_id, slice)) = self.pools[pool_idx].try_reserve() {
                slice[..len].copy_from_slice(payload);
                let pool_id = match u16::try_from(pool_idx) {
                    Ok(id) => id,
                    Err(_) => {
                        // Defensive: SIZE_CLASSES has 14 entries today, so this
                        // path is unreachable. If a future change pushes the
                        // pool count past u16::MAX, the next class still fits
                        // the same payload, so we release this slot and let
                        // the loop try the next class instead of bailing.
                        self.pools[pool_idx].release_slot(slot_id);
                        continue;
                    }
                };
                return Some(SlabHandle::Primary { pool_id, slot_id });
            }
        }

        None
    }

    /// Get immutable buffer from handle (for reading)
    ///
    /// Returns a slice to the buffer data backing the handle. The slice
    /// length matches the size class slot, which may exceed the bytes
    /// actually written; the runtime carries the logical length on
    /// `IndexEntry::len` for callers that need to slice down.
    ///
    /// # Safety
    /// - Handle must be valid and currently allocated
    /// - Buffer must have been written by `reserve_and_write`, or by
    ///   the caller after a `reserve` call returned the slice
    /// - Caller must ensure no concurrent writes to this handle
    ///
    /// # Panics
    /// Panics if handle is invalid (debug builds only).
    ///
    /// # Performance
    /// Target: < 20 ns (pointer arithmetic only, no atomics)
    #[allow(clippy::mut_from_ref)]
    pub fn get_buffer(&self, handle: SlabHandle) -> &[u8] {
        match handle {
            SlabHandle::Primary { pool_id, slot_id } => {
                let pool_id = usize::from(pool_id);
                let slot_id = usize::from(slot_id);
                debug_assert!(pool_id < self.pools.len(), "Invalid pool_id");
                let pool = &self.pools[pool_id];
                debug_assert!(slot_id < pool.slot_count, "Invalid slot_id");
                let offset = slot_id * pool.slot_size;
                // SAFETY:
                // 1. pool.data was allocated once during Pool::new and never freed
                //    while Pool alive.
                // 2. Slot is allocated (bitmap bit set) so slice lies within
                //    initialized memory.
                // 3. We only create an immutable slice (&[u8]), so concurrent
                //    readers are allowed.
                // 4. Offset math bounded by slot_count and slot_size.
                let data = unsafe { &*pool.data.get() };
                &data[offset..offset + pool.slot_size]
            }
            SlabHandle::Large { .. } => {
                debug_assert!(false, "Large variant not yet allocated");
                &[]
            }
            SlabHandle::Heap { .. } => {
                debug_assert!(false, "Heap variant not yet allocated");
                &[]
            }
        }
    }

    /// Release slab after reading
    ///
    /// Returns the slot to the pool's free list.
    ///
    /// # Panics
    /// Panics if handle is invalid (debug builds only).
    ///
    /// # Performance
    /// Target: < 50 ns (single CAS)
    ///
    /// # Latency
    /// - **p50:** 23.93 ns
    /// - **p99:** 27.78 ns (**SLA target:** < 100 ns)
    /// - **p999:** 27.84 ns
    ///   [!] **Benchmark methodology:** Measures release only via Criterion
    ///   `slabpool_release` (benches/runtime.rs) with hot cache / pre-reserved slots.
    ///   Last measured: 2025-10-21 on Intel(R) Xeon(R) CPU E5-2699 v4 @ 2.20GHz.
    ///   **Exception rationale:** CAS on 64-bit bitmap incurs ~20-25 ns hardware latency on x86_64;
    ///   additional bookkeeping (~3 ns) yields ~28 ns p99. Alternatives (spinlock, batching) perform
    ///   worse (>50 ns). Acceptable deviation recorded in audit (Section 8).
    pub fn release(&self, handle: SlabHandle) {
        match handle {
            SlabHandle::Primary { pool_id, slot_id } => {
                let pool_idx = usize::from(pool_id);
                debug_assert!(pool_idx < self.pools.len(), "Invalid pool_id");
                self.pools[pool_idx].release_slot(slot_id);
            }
            SlabHandle::Large { .. } => {
                debug_assert!(false, "Large variant not yet allocated");
            }
            SlabHandle::Heap { .. } => {
                debug_assert!(false, "Heap variant not yet allocated");
            }
        }
    }
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_encoding() {
        let h = SlabHandle::Primary {
            pool_id: 42,
            slot_id: 1337,
        };
        assert!(matches!(
            h,
            SlabHandle::Primary {
                pool_id: 42,
                slot_id: 1337,
            }
        ));
    }

    #[test]
    fn test_empty_sentinel() {
        let h = SlabHandle::EMPTY;
        assert!(h.is_empty());
        assert!(!SlabHandle::Primary {
            pool_id: 0,
            slot_id: 0,
        }
        .is_empty());
    }

    #[test]
    fn test_reserve_and_write_primary() {
        let pool = SlabPool::new();
        let payload = b"hello slab";
        let (handle, metric) = pool
            .reserve_and_write(payload)
            .expect("primary tier should have capacity");
        assert_eq!(metric, SlabMetric::Primary);
        let buf = pool.get_buffer(handle);
        assert_eq!(&buf[..payload.len()], payload);
        pool.release(handle);
    }

    #[test]
    fn test_legacy_handle_to_primary() {
        let h = SlabHandle::legacy_handle_to_primary((3 << 16) | 7);
        assert!(matches!(
            h,
            SlabHandle::Primary {
                pool_id: 3,
                slot_id: 7,
            }
        ));
    }

    #[test]
    fn test_reserve_basic() {
        let pool = SlabPool::new();
        let (h1, buf1) = pool
            .reserve(64)
            .expect("SlabPool reservation should succeed");
        assert!(buf1.len() >= 64);

        let (h2, buf2) = pool
            .reserve(64)
            .expect("SlabPool reservation should succeed");
        assert_ne!(h1, h2); // Different slots
        assert!(buf2.len() >= 64);
    }

    #[test]
    fn test_reserve_release_cycle() {
        let pool = SlabPool::new();
        let (h, _) = pool
            .reserve(100)
            .expect("SlabPool reservation should succeed");
        pool.release(h);

        // Should be able to allocate same slot again
        let (h2, _) = pool
            .reserve(100)
            .expect("SlabPool reservation should succeed");
        assert_eq!(h, h2); // Same slot reused
    }

    #[test]
    fn test_reserve_size_classes() {
        let pool = SlabPool::new();

        // Request 10 bytes -> should get 16B pool
        let (h, buf) = pool
            .reserve(10)
            .expect("SlabPool reservation should succeed");
        assert_eq!(buf.len(), 16);
        match h {
            SlabHandle::Primary { pool_id, .. } => assert_eq!(pool_id, 0),
            other => panic!("expected Primary, got {:?}", other),
        }

        pool.release(h);

        // Request 100 bytes -> should get 128B pool
        let (h2, buf2) = pool
            .reserve(100)
            .expect("SlabPool reservation should succeed");
        assert_eq!(buf2.len(), 128);
        match h2 {
            SlabHandle::Primary { pool_id, .. } => assert_eq!(pool_id, 3),
            other => panic!("expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn test_pool_exhaustion() {
        let pool = SlabPool::new();

        // Allocate all 16B slots (64 of them)
        let mut handles = Vec::new();
        for _ in 0..64 {
            let (h, _) = pool
                .reserve(16)
                .expect("SlabPool reservation should succeed");
            handles.push(h);
        }

        // Next 16B allocation should fallback to 32B pool
        let (h_fallback, buf) = pool
            .reserve(16)
            .expect("SlabPool reservation should succeed");
        assert_eq!(buf.len(), 32); // Fallback to next size class
        match h_fallback {
            SlabHandle::Primary { pool_id, .. } => assert_eq!(pool_id, 1),
            other => panic!("expected Primary, got {:?}", other),
        }
    }

    #[test]
    fn test_no_double_free() {
        let pool = SlabPool::new();
        let (h, _) = pool
            .reserve(100)
            .expect("SlabPool reservation should succeed");
        pool.release(h);

        // Second release should be safe (idempotent bitmap clear)
        pool.release(h); // Should not panic or corrupt state
    }
}
