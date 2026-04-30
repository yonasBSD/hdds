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

use crate::core::rt::slabpool_metrics::SlabPoolMetrics;
use crate::qos::MemoryPolicy;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::convert::TryFrom;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicU64, Ordering};

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
        // `packed >> 16` and `packed & 0xFFFF` are both bounded by 0xFFFF
        // = u16::MAX, so try_from never fails on a well-formed u32.
        let pool_id = u16::try_from(packed >> 16).unwrap_or(u16::MAX);
        let slot_id = u16::try_from(packed & 0xFFFF).unwrap_or(u16::MAX);
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

/// Combined storage for the heap fallback tier. Single mutex avoids
/// the deadlock-by-ordering class that two independent locks would
/// introduce; tier 3 contention is exceptional by design.
struct HeapStore {
    /// `heap_id` -> live buffer, or `None` when the slot has been released.
    buffers: Vec<Option<Box<[u8]>>>,
    /// LIFO stack of `heap_id` values reusable by future allocations.
    free_list: Vec<u32>,
}

/// Memory pool for zero-copy message buffers.
///
/// Three allocation tiers are tried in order: the primary 14-class pool
/// (lock-free bitmap CAS), an opt-in secondary pool sized for large
/// payloads, and a heap fallback bounded by `max_heap_bytes`.
pub struct SlabPool {
    pools: Vec<Pool>,
    /// Secondary pool, configured at participant start via `MemoryPolicy`.
    /// Empty = disabled.
    large_pools: Vec<Pool>,
    /// Heap fallback storage; mutated under a single mutex.
    heap_store: Mutex<HeapStore>,
    /// Allocation counters and the heap-bytes gauge. The
    /// `heap_bytes_used` field is the CAS-protected accountant for
    /// `SlabHandle::Heap` allocations.
    metrics: SlabPoolMetrics,
    /// Ceiling for the heap fallback tier (per `MemoryPolicy.max_heap_bytes`).
    max_heap_bytes: usize,
}

impl SlabPool {
    /// Build a slab pool with the default `MemoryPolicy` (no large tier,
    /// 16 MB heap fallback ceiling).
    pub fn new() -> Self {
        Self::with_policy(&MemoryPolicy::default())
    }

    /// Build a slab pool from an explicit allocation policy.
    pub fn with_policy(policy: &MemoryPolicy) -> Self {
        let pools = SIZE_CLASSES
            .iter()
            .map(|&(size, count)| Pool::new(size, count))
            .collect();
        let large_pools = policy
            .large_pool
            .classes
            .iter()
            .map(|&(size, count)| Pool::new(size, count))
            .collect();

        Self {
            pools,
            large_pools,
            heap_store: Mutex::new(HeapStore {
                buffers: Vec::new(),
                free_list: Vec::new(),
            }),
            metrics: SlabPoolMetrics::new(),
            max_heap_bytes: policy.max_heap_bytes,
        }
    }

    /// Snapshot accessor for the pool's allocation counters.
    #[must_use]
    pub fn metrics(&self) -> &SlabPoolMetrics {
        &self.metrics
    }

    /// Benchmark fixture that mirrors `reserve_and_write` under a
    /// stable public surface. Returns the handle only (the tier tag
    /// would force `SlabMetric` to be public). Use only from
    /// `crates/hdds/benches/`; production code calls
    /// `reserve_and_write` directly.
    #[doc(hidden)]
    pub fn bench_reserve_and_write(&self, payload: &[u8]) -> Option<SlabHandle> {
        self.reserve_and_write(payload).map(|(handle, _)| handle)
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
    pub(crate) fn reserve_and_write(&self, payload: &[u8]) -> Option<(SlabHandle, SlabMetric)> {
        if let Some(handle) = self.try_reserve_primary_and_copy(payload) {
            self.metrics
                .primary_hit_total
                .fetch_add(1, Ordering::Relaxed);
            return Some((handle, SlabMetric::Primary));
        }
        if let Some(handle) = self.try_reserve_large_and_copy(payload) {
            self.metrics.large_hit_total.fetch_add(1, Ordering::Relaxed);
            return Some((handle, SlabMetric::Large));
        }
        if let Some(handle) = self.try_reserve_heap_and_copy(payload) {
            self.metrics
                .heap_fallback_total
                .fetch_add(1, Ordering::Relaxed);
            return Some((handle, SlabMetric::HeapFallback));
        }
        self.metrics
            .insert_dropped_total
            .fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Walk the primary size classes from best-fit upward, reserve the
    /// first available slot, and `copy_from_slice` the payload into it.
    /// Returns `None` if every primary class is full.
    fn try_reserve_primary_and_copy(&self, payload: &[u8]) -> Option<SlabHandle> {
        let len = payload.len();
        let start_idx = SIZE_CLASSES.iter().position(|&(size, _)| size >= len)?;

        for pool_idx in start_idx..self.pools.len() {
            if let Some(handle) = self.try_claim_slot(&self.pools, pool_idx, payload, false) {
                return Some(handle);
            }
        }
        None
    }

    /// Walk the secondary (`Large`) size classes and reserve+copy on hit.
    /// Returns `None` if the large pool is disabled or every class is full.
    fn try_reserve_large_and_copy(&self, payload: &[u8]) -> Option<SlabHandle> {
        if self.large_pools.is_empty() {
            return None;
        }
        let len = payload.len();
        let start_idx = self
            .large_pools
            .iter()
            .position(|pool| pool.slot_size >= len)?;

        for pool_idx in start_idx..self.large_pools.len() {
            if let Some(handle) = self.try_claim_slot(&self.large_pools, pool_idx, payload, true) {
                return Some(handle);
            }
        }
        None
    }

    /// Try to reserve a slot in `pools[pool_idx]` and copy the payload
    /// into it. Returns `Some(handle)` on success with the matching
    /// variant tag, `None` if the pool was full at this class.
    fn try_claim_slot(
        &self,
        pools: &[Pool],
        pool_idx: usize,
        payload: &[u8],
        is_large: bool,
    ) -> Option<SlabHandle> {
        let len = payload.len();
        let (slot_id, slice) = pools[pool_idx].try_reserve()?;
        slice[..len].copy_from_slice(payload);
        let pool_id = match u16::try_from(pool_idx) {
            Ok(id) => id,
            Err(_) => {
                // Defensive: pools.len() is bounded by SIZE_CLASSES (14) for
                // the primary tier and by LargePoolConfig for the secondary
                // tier, both well below u16::MAX. If a future change crosses
                // that bound the caller will simply skip to the next class.
                pools[pool_idx].release_slot(slot_id);
                return None;
            }
        };
        Some(if is_large {
            SlabHandle::Large { pool_id, slot_id }
        } else {
            SlabHandle::Primary { pool_id, slot_id }
        })
    }

    /// Allocate a heap-tier buffer for `payload`, enforcing `max_heap_bytes`.
    /// Returns `None` if the ceiling would be crossed.
    fn try_reserve_heap_and_copy(&self, payload: &[u8]) -> Option<SlabHandle> {
        let len = payload.len();

        // Atomic accounting (CAS-loop). On success, `metrics.heap_bytes_used`
        // has been incremented by `len` and the caller is responsible for
        // either publishing a Heap handle (matched by a future `release`
        // that decrements) or rolling back via `fetch_sub` on failure.
        loop {
            let prev = self.metrics.heap_bytes_used.load(Ordering::Acquire);
            let next = prev.checked_add(len)?;
            if next > self.max_heap_bytes {
                return None;
            }
            if self
                .metrics
                .heap_bytes_used
                .compare_exchange(prev, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
            // CAS lost the race; re-read and retry.
        }

        // SAFETY:
        // 1. `Box::new_uninit_slice(len)` allocates `len` `MaybeUninit<u8>`
        //    bytes; layout is identical to `[u8; len]`.
        // 2. `copy_nonoverlapping` writes every byte of `dst`, so the slice
        //    is fully initialised before we cast.
        // 3. `Box::from_raw(Box::into_raw(boxed) as *mut [u8])` reclaims
        //    ownership of the same allocation with the layout unchanged.
        let init_box: Box<[u8]> = unsafe {
            let mut boxed: Box<[MaybeUninit<u8>]> = Box::new_uninit_slice(len);
            let dst = boxed.as_mut_ptr().cast::<u8>();
            std::ptr::copy_nonoverlapping(payload.as_ptr(), dst, len);
            Box::from_raw(Box::into_raw(boxed) as *mut [u8])
        };

        let heap_id = {
            let mut store = self.heap_store.lock();
            if let Some(reused) = store.free_list.pop() {
                store.buffers[reused as usize] = Some(init_box);
                reused
            } else {
                let new_id = match u32::try_from(store.buffers.len()) {
                    Ok(id) => id,
                    Err(_) => {
                        // Heap-id space exhausted; roll back the accounting
                        // and surface as exhaustion to the caller.
                        self.metrics
                            .heap_bytes_used
                            .fetch_sub(len, Ordering::AcqRel);
                        return None;
                    }
                };
                store.buffers.push(Some(init_box));
                new_id
            }
        };

        Some(SlabHandle::Heap { heap_id })
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
    pub fn get_buffer(&self, handle: SlabHandle) -> Cow<'_, [u8]> {
        match handle {
            SlabHandle::Primary { pool_id, slot_id } => {
                Cow::Borrowed(slot_slice(&self.pools, pool_id, slot_id))
            }
            SlabHandle::Large { pool_id, slot_id } => {
                Cow::Borrowed(slot_slice(&self.large_pools, pool_id, slot_id))
            }
            SlabHandle::Heap { heap_id } => {
                let store = self.heap_store.lock();
                let buf = store
                    .buffers
                    .get(heap_id as usize)
                    .and_then(|slot| slot.as_ref())
                    .map(|b| b.to_vec())
                    .unwrap_or_default();
                Cow::Owned(buf)
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
                debug_assert!(pool_idx < self.pools.len(), "Invalid Primary pool_id");
                self.pools[pool_idx].release_slot(slot_id);
            }
            SlabHandle::Large { pool_id, slot_id } => {
                let pool_idx = usize::from(pool_id);
                debug_assert!(pool_idx < self.large_pools.len(), "Invalid Large pool_id");
                self.large_pools[pool_idx].release_slot(slot_id);
            }
            SlabHandle::Heap { heap_id } => {
                // Take the box out under the lock so the buffer drop and the
                // accounting decrement happen outside the critical section.
                let taken = {
                    let mut store = self.heap_store.lock();
                    match store.buffers.get_mut(heap_id as usize) {
                        Some(slot) => slot.take().inspect(|_| {
                            store.free_list.push(heap_id);
                        }),
                        None => None,
                    }
                };
                if let Some(buf) = taken {
                    let len = buf.len();
                    drop(buf);
                    self.metrics
                        .heap_bytes_used
                        .fetch_sub(len, Ordering::AcqRel);
                }
                // Idempotent: a second release on the same heap_id is a no-op.
            }
        }
    }
}

/// Borrow the size-class slot referenced by a slab handle. Used by both
/// `Primary` and `Large` paths in `get_buffer`.
fn slot_slice(pools: &[Pool], pool_id: u16, slot_id: u16) -> &[u8] {
    let pool_idx = usize::from(pool_id);
    let slot_idx = usize::from(slot_id);
    debug_assert!(pool_idx < pools.len(), "Invalid pool_id");
    let pool = &pools[pool_idx];
    debug_assert!(slot_idx < pool.slot_count, "Invalid slot_id");
    let offset = slot_idx * pool.slot_size;
    // SAFETY:
    // 1. pool.data was allocated once during Pool::new and never freed
    //    while Pool alive.
    // 2. Slot is allocated (bitmap bit set) so slice lies within
    //    initialized memory.
    // 3. We only create an immutable slice (&[u8]), so concurrent
    //    readers are allowed.
    // 4. Offset math is bounded by slot_count and slot_size.
    let data = unsafe { &*pool.data.get() };
    &data[offset..offset + pool.slot_size]
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qos::LargePoolConfig;

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
    fn test_reserve_and_write_primary_tier() {
        // Three-path integration test, per design §6 #1 — small payload
        // must land in the Primary tier without consulting Large or Heap.
        let pool = SlabPool::new();
        let payload = vec![0xABu8; 256];
        let (handle, metric) = pool
            .reserve_and_write(&payload)
            .expect("primary tier should accept a 256-byte payload");
        assert_eq!(metric, SlabMetric::Primary);
        let buf = pool.get_buffer(handle);
        assert_eq!(&buf[..payload.len()], payload.as_slice());
        pool.release(handle);
    }

    #[test]
    fn test_reserve_and_write_large_tier() {
        // Three-path integration test, per design §6 #1 — payload that
        // exceeds the primary 128 KB ceiling lands in the Large tier when
        // it is configured, not in Heap fallback.
        //
        // Custom Large config (single 256 KB class × 2 slots) keeps the
        // calibration robust against future SIZE_CLASSES changes; the
        // 200 KB payload picks this class regardless of primary layout.
        let policy = MemoryPolicy {
            large_pool: LargePoolConfig {
                classes: vec![(262_144, 2)],
            },
            max_heap_bytes: 16 * 1024 * 1024,
        };
        let pool = SlabPool::with_policy(&policy);
        let payload = vec![0xCDu8; 200_000];
        let (handle, metric) = pool
            .reserve_and_write(&payload)
            .expect("large tier should accept a 200 KB payload");
        assert_eq!(metric, SlabMetric::Large);
        let buf = pool.get_buffer(handle);
        assert_eq!(&buf[..payload.len()], payload.as_slice());
        pool.release(handle);
    }

    #[test]
    fn test_reserve_and_write_heap_fallback_after_large_saturated() {
        // Three-path integration test, per design §6 #1 — payload that
        // outgrows both the Primary and the Large tiers must succeed via
        // Heap fallback, not silently drop.
        //
        // Custom Large config (single 256 KB class × 2 slots, total 512 KB)
        // saturates fast; the third 200 KB allocation must reach Heap.
        let policy = MemoryPolicy {
            large_pool: LargePoolConfig {
                classes: vec![(262_144, 2)],
            },
            max_heap_bytes: 4 * 1024 * 1024,
        };
        let pool = SlabPool::with_policy(&policy);
        let payload = vec![0xEFu8; 200_000];

        let (h1, m1) = pool.reserve_and_write(&payload).expect("first large slot");
        let (h2, m2) = pool.reserve_and_write(&payload).expect("second large slot");
        assert_eq!(m1, SlabMetric::Large);
        assert_eq!(m2, SlabMetric::Large);

        let (h3, m3) = pool
            .reserve_and_write(&payload)
            .expect("heap fallback after Large saturation");
        assert_eq!(m3, SlabMetric::HeapFallback);
        let buf = pool.get_buffer(h3);
        assert_eq!(&buf[..payload.len()], payload.as_slice());

        pool.release(h1);
        pool.release(h2);
        pool.release(h3);
    }

    #[test]
    fn test_reserve_and_write_heap_fallback_when_primary_saturated() {
        // Saturate the largest primary class (128 KB × 16 slots) and prove
        // the 17th 100 KB allocation lands in the heap fallback tier with
        // its bytes intact. Mirrors the LargeData_0 bench scenario (writer
        // sends N > 16 samples of 100 KB, primary class exhausts, heap
        // fallback must succeed instead of silently dropping).
        let pool = SlabPool::new();
        let mut primary_handles = Vec::new();
        for _ in 0..16 {
            let payload = vec![0u8; 100_000];
            let (h, m) = pool
                .reserve_and_write(&payload)
                .expect("primary slot should be available");
            assert_eq!(m, SlabMetric::Primary);
            primary_handles.push(h);
        }

        let payload = vec![0xCDu8; 100_000];
        let (h_heap, m_heap) = pool
            .reserve_and_write(&payload)
            .expect("heap fallback should succeed once primary is full");
        assert_eq!(m_heap, SlabMetric::HeapFallback);

        let buf = pool.get_buffer(h_heap);
        assert_eq!(buf.len(), 100_000);
        assert!(buf.iter().all(|&b| b == 0xCD));

        pool.release(h_heap);
        for h in primary_handles {
            pool.release(h);
        }
    }

    #[test]
    fn test_heap_budget_exhaustion_increments_drop_counter() {
        // Test plan §6 #3: when reserve_and_write fails at every tier
        // (heap budget exhausted in this case), the drop counter must
        // increment. The warn! emission is asserted by inspection of
        // the production code path, not by a captured log assertion
        // (no log-capture crate is currently authorised in workspace
        // dev-dependencies).
        let policy = MemoryPolicy {
            large_pool: LargePoolConfig::default(),
            max_heap_bytes: 1024 * 1024,
        };
        let pool = SlabPool::with_policy(&policy);
        let mut primary_handles = Vec::new();
        for _ in 0..16 {
            let payload = vec![0u8; 100_000];
            let (h, _m) = pool
                .reserve_and_write(&payload)
                .expect("primary slot should be available");
            primary_handles.push(h);
        }
        let payload = vec![0u8; 800_000];
        let (h_heap, _) = pool
            .reserve_and_write(&payload)
            .expect("first heap allocation under ceiling");

        let drops_before = pool.metrics().insert_dropped_total.load(Ordering::Relaxed);
        let overflow = vec![0u8; 800_000];
        assert!(
            pool.reserve_and_write(&overflow).is_none(),
            "heap budget exhausted; reserve_and_write must return None"
        );
        let drops_after = pool.metrics().insert_dropped_total.load(Ordering::Relaxed);
        assert_eq!(
            drops_after,
            drops_before + 1,
            "insert_dropped_total must increment on full-tier exhaustion",
        );

        pool.release(h_heap);
        for h in primary_handles {
            pool.release(h);
        }
    }

    #[test]
    fn test_reserve_and_write_metrics_per_tier() {
        // Each tier hit should bump its own counter, not the others.
        let policy = MemoryPolicy {
            large_pool: LargePoolConfig {
                classes: vec![(262_144, 2)],
            },
            max_heap_bytes: 4 * 1024 * 1024,
        };
        let pool = SlabPool::with_policy(&policy);
        let (primary_h, _) = pool.reserve_and_write(&[0u8; 256]).expect("primary");
        let (large_h, _) = pool.reserve_and_write(&[0u8; 200_000]).expect("large 1");
        let (large_h2, _) = pool.reserve_and_write(&[0u8; 200_000]).expect("large 2");
        let (heap_h, _) = pool
            .reserve_and_write(&[0u8; 200_000])
            .expect("heap fallback");

        let (primary, large, heap, dropped, _used) = pool.metrics().snapshot();
        assert_eq!(primary, 1);
        assert_eq!(large, 2);
        assert_eq!(heap, 1);
        assert_eq!(dropped, 0);

        pool.release(primary_h);
        pool.release(large_h);
        pool.release(large_h2);
        pool.release(heap_h);
    }

    #[test]
    fn test_reserve_and_write_heap_ceiling_enforced() {
        // 1 MB heap ceiling: two 400 KB heap allocations should succeed,
        // a third 400 KB allocation must fail without corrupting state.
        let pool = SlabPool::with_policy(&MemoryPolicy {
            large_pool: LargePoolConfig::default(),
            max_heap_bytes: 1024 * 1024,
        });
        let mut primary_handles = Vec::new();
        for _ in 0..16 {
            let payload = vec![0u8; 100_000];
            let (h, _m) = pool
                .reserve_and_write(&payload)
                .expect("primary slot should be available");
            primary_handles.push(h);
        }

        let payload = vec![0u8; 400_000];
        let (h1, m1) = pool
            .reserve_and_write(&payload)
            .expect("first heap allocation under ceiling");
        let (h2, m2) = pool
            .reserve_and_write(&payload)
            .expect("second heap allocation under ceiling");
        assert_eq!(m1, SlabMetric::HeapFallback);
        assert_eq!(m2, SlabMetric::HeapFallback);

        // Third 400 KB would push to 1.2 MB > 1 MB ceiling.
        let blocked = pool.reserve_and_write(&payload);
        assert!(blocked.is_none(), "ceiling must reject allocation");

        pool.release(h1);
        pool.release(h2);
        for h in primary_handles {
            pool.release(h);
        }
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
