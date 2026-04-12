// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

use super::cache::{CachedSample, InstanceHandle, SampleCache};
use super::subscriber::DisposeEvent;
use crate::core::rt;
use crate::dds::listener::{DataReaderListener, RequestedDeadlineMissedStatus};
use crate::dds::{qos::History, BindToken, QoS, Result, StatusCondition, StatusMask, DDS};
use crate::engine::subscriber::DisposeKind;
use crate::engine::TopicRegistry;
use crate::protocol::builder;
use crate::qos::time_based_filter::TimeBasedFilterChecker;
use crate::reliability::{NackScheduler, ReliableMetrics};
use crate::telemetry;
use crate::telemetry::metrics::current_time_ns;
use crate::transport::UdpTransport;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A typed DDS DataReader that subscribes to samples on a topic.
///
/// `DataReader<T>` receives data samples of type `T` from matching [`DataWriter<T>`](crate::DataWriter)
/// instances. It deserializes incoming CDR-encoded data and provides access via `try_take()`.
///
/// # Type Parameter
///
/// * `T` - The data type, must implement [`DDS`](crate::dds::DDS) (derive with `#[derive(hdds::DDS)]`)
///
/// # Example
///
/// ```rust,no_run
/// use hdds::{Participant, QoS, Result, DDS};
///
/// #[derive(DDS, Debug)]
/// struct Temperature {
///     sensor_id: u32,
///     value: f32,
/// }
///
/// fn main() -> Result<()> {
///     let participant = Participant::builder("temp_monitor")
///         .domain_id(0)
///         .build()?;
///
///     let reader = participant.create_reader::<Temperature>(
///         "sensors/temp",
///         QoS::reliable(),
///     )?;
///
///     loop {
///         if let Some(sample) = reader.try_take()? {
///             println!("Sensor {} = {} degC", sample.sensor_id, sample.value);
///         }
///     }
/// }
/// ```
///
/// # Thread Safety
///
/// `DataReader<T>` is `Send + Sync` when `T` is `Send + Sync`.
///
/// # See Also
///
/// - `ReaderBuilder` - For advanced configuration
/// - [`DataWriter`](crate::DataWriter) - Publishing counterpart
pub struct DataReader<T: DDS> {
    topic: String,
    qos: QoS,
    ring: Arc<rt::IndexRing>,
    /// Sample cache for read/take operations (DDS standard API).
    cache: SampleCache<T>,
    #[allow(dead_code)]
    registry: Option<Arc<TopicRegistry>>,
    nack_scheduler: Option<Arc<Mutex<NackScheduler>>>,
    transport: Option<Arc<UdpTransport>>,
    #[allow(dead_code)]
    reliable_metrics: Option<Arc<ReliableMetrics>>,
    status_condition: Arc<StatusCondition>,
    /// BindToken for intra-process auto-binding (unregisters on drop)
    _bind_token: Option<BindToken>,
    /// Security plugin suite for decryption (DDS Security v1.1)
    #[cfg(feature = "security")]
    #[allow(dead_code)]
    security: Option<Arc<crate::security::SecurityPluginSuite>>,
    /// Optional listener for reader callbacks (deadline missed, etc.)
    listener: Option<Arc<dyn DataReaderListener<T>>>,
    /// Match notification token (fires on_subscription_matched; unregisters on drop)
    _match_token: Option<crate::dds::match_notification::MatchToken>,
    /// Deadline tracker for detecting requested deadline missed events (DDS spec 2.2.2.4.2.12).
    deadline_tracker: Mutex<crate::qos::deadline::ReaderDeadlineTracker>,
    deadline_missed_total: AtomicU32,
    /// Per-instance TIME_BASED_FILTER enforcement (DDS spec 2.2.2.4.2.11).
    time_filter_checkers: Mutex<HashMap<InstanceHandle, TimeBasedFilterChecker>>,
    /// Lifespan duration for sample expiration (DDS spec 2.2.2.4.2.20).
    lifespan_duration: Duration,
    /// Shared dispose event queue from ReaderSubscriber (P1.2).
    dispose_events: Arc<Mutex<Vec<DisposeEvent>>>,
    _phantom: core::marker::PhantomData<T>,
}

impl<T: DDS> DataReader<T> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        topic: String,
        qos: QoS,
        ring: Arc<rt::IndexRing>,
        registry: Option<Arc<TopicRegistry>>,
        nack_scheduler: Option<Arc<Mutex<NackScheduler>>>,
        transport: Option<Arc<UdpTransport>>,
        reliable_metrics: Option<Arc<ReliableMetrics>>,
        status_condition: Arc<StatusCondition>,
        bind_token: Option<BindToken>,
        listener: Option<Arc<dyn DataReaderListener<T>>>,
        match_token: Option<crate::dds::match_notification::MatchToken>,
        #[cfg(feature = "security")] security: Option<Arc<crate::security::SecurityPluginSuite>>,
        dispose_events: Arc<Mutex<Vec<DisposeEvent>>>,
    ) -> Self {
        // Capture QoS values before moving qos into struct
        let deadline_period = qos.deadline.period;
        let lifespan_dur = qos.lifespan.duration;

        // Determine cache size from history QoS
        let cache_size = match qos.history {
            History::KeepLast(depth) => depth as usize,
            History::KeepAll => 1024, // Default for KeepAll
        };

        Self {
            topic,
            qos,
            ring,
            cache: SampleCache::new(cache_size),
            registry,
            nack_scheduler,
            transport,
            reliable_metrics,
            status_condition,
            _bind_token: bind_token,
            #[cfg(feature = "security")]
            security,
            listener,
            _match_token: match_token,
            deadline_tracker: Mutex::new(crate::qos::deadline::ReaderDeadlineTracker::new(
                deadline_period,
            )),
            deadline_missed_total: AtomicU32::new(0),
            time_filter_checkers: Mutex::new(HashMap::new()),
            lifespan_duration: lifespan_dur,
            dispose_events,
            _phantom: core::marker::PhantomData,
        }
    }

    #[must_use]
    pub fn qos(&self) -> &QoS {
        &self.qos
    }

    #[must_use]
    pub fn topic_name(&self) -> &str {
        &self.topic
    }

    /// Check if requested deadline was missed and fire listener callback.
    ///
    /// Called at the start of each take()/read() to detect if no sample
    /// has been received within the configured deadline period.
    /// (DDS spec 2.2.2.4.2.12 - RequestedDeadlineMissed)
    fn check_requested_deadline(&self) {
        let mut tracker = self
            .deadline_tracker
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if tracker.is_missed() {
            let total = self.deadline_missed_total.fetch_add(1, Ordering::Relaxed) + 1;
            tracker.check();
            if let Some(ref listener) = self.listener {
                listener.on_requested_deadline_missed(RequestedDeadlineMissedStatus {
                    total_count: total,
                    total_count_change: 1,
                    last_instance_handle: None,
                });
            }
            log::debug!(
                "[reader] Requested deadline missed on topic='{}' (total={})",
                self.topic,
                total
            );
        }
    }

    #[must_use]
    pub fn get_status_condition(&self) -> Arc<StatusCondition> {
        Arc::clone(&self.status_condition)
    }

    pub fn bind_to_writer(&self, writer_merger: Arc<rt::TopicMerger>) {
        let ring = Arc::clone(&self.ring);
        let status_condition = Arc::clone(&self.status_condition);
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            status_condition.set_active_statuses(StatusMask::DATA_AVAILABLE);
        });

        let registration = rt::MergerReader::new(ring, notify);
        writer_merger.add_reader(registration);
    }

    /// Take a sample from the reader (legacy API).
    ///
    /// # Deprecated
    /// Use [`take()`](Self::take) instead, which uses the new cache-based
    /// architecture supporting both `read()` and `take()` semantics.
    #[deprecated(since = "0.9.0", note = "use take() instead")]
    pub fn try_take(&self) -> Result<Option<T>> {
        log::debug!(
            "[READER-TAKE] enter topic='{}' ring_len={}",
            self.topic,
            self.ring.len()
        );
        let read_start_ns = current_time_ns();

        self.enforce_history();

        let entry = match self.ring.pop() {
            Some(entry) => entry,
            None => return Ok(None),
        };
        log::debug!(
            "[READER] pop topic='{}' seq={} len={} handle={:?}",
            self.topic,
            entry.seq,
            entry.len,
            entry.handle
        );

        if let Some(scheduler) = &self.nack_scheduler {
            let mut sched = match scheduler.lock() {
                Ok(lock) => lock,
                Err(err) => {
                    log::debug!(
                        "[Reader::try_take] nack_scheduler lock poisoned; recovering. {:?}",
                        err
                    );
                    err.into_inner()
                }
            };
            sched.on_receive(u64::from(entry.seq));
        }

        let slab_pool = rt::get_slab_pool();
        let buf = slab_pool.get_buffer(entry.handle);
        let data_len = entry.len as usize;
        let slice = &buf[..data_len];
        let decode_result = T::decode_cdr2(slice);
        slab_pool.release(entry.handle);

        let msg = match decode_result {
            Ok(msg) => {
                log::debug!("[READER] decoded topic='{}' len={}", self.topic, data_len);
                msg
            }
            Err(e) => {
                let preview_len = if data_len > 32 { 32 } else { data_len };
                log::error!(
                    "[READER-DECODE] failed topic='{}' len={} error={} first_bytes={:02x?}",
                    self.topic,
                    data_len,
                    e,
                    &slice[..preview_len]
                );
                return Err(e);
            }
        };

        if let Some(metrics) = telemetry::get_metrics_opt() {
            metrics.increment_received(1);
            metrics.add_latency_sample(entry.timestamp_ns, read_start_ns);
        }

        self.maybe_send_nack();

        if self.ring.is_empty() {
            self.status_condition.clear_active_statuses();
        }

        Ok(Some(msg))
    }

    pub fn take_batch(&self, max: usize) -> Result<Vec<T>> {
        let mut messages = Vec::with_capacity(max);
        for _ in 0..max {
            match self.take()? {
                Some(sample) => messages.push(sample),
                None => break,
            }
        }
        Ok(messages)
    }

    // =========================================================================
    // DDS Standard API (v0.9+)
    // =========================================================================

    /// Take a single sample, removing it from the cache (DDS take semantics).
    ///
    /// This is the DDS-standard `take()` operation. It:
    /// 1. Pumps any pending samples from the network ring to the cache
    /// 2. Removes and returns the oldest sample from the cache
    ///
    /// # Returns
    /// - `Ok(Some(sample))` if a sample was available
    /// - `Ok(None)` if no samples available
    /// - `Err(_)` if decoding failed
    ///
    /// # Example
    /// ```rust,no_run
    /// # use hdds::{Participant, QoS, Result};
    /// # fn main() -> Result<()> {
    /// # let participant = Participant::builder("test").build()?;
    /// # let reader = participant.create_reader::<MyType>("topic", QoS::default())?;
    /// while let Some(sample) = reader.take()? {
    ///     println!("Got: {:?}", sample);
    /// }
    /// # Ok(())
    /// # }
    /// # #[derive(hdds::DDS, Debug)] struct MyType { x: i32 }
    /// ```
    pub fn take(&self) -> Result<Option<T>> {
        // Check requested deadline before reading (DDS spec: fire callback on miss)
        self.check_requested_deadline();

        // Pump pending samples from ring to cache
        self.pump_ring_to_cache()?;

        // Take from cache
        let result = self.cache.take();
        Ok(result)
    }

    /// Take the next sample (DDS standard alias).
    ///
    /// Equivalent to [`take()`](Self::take).
    #[inline]
    pub fn take_next_sample(&self) -> Result<Option<T>> {
        self.take()
    }

    /// Take a single sample for a specific instance (DDS take_instance).
    ///
    /// Returns and removes the oldest sample matching the given instance handle.
    /// For keyless topics, use `InstanceHandle::nil()`.
    ///
    /// # Arguments
    /// * `handle` - The instance handle (key hash) to filter by
    ///
    /// # Returns
    /// - `Ok(Some(sample))` if a matching sample was found and removed
    /// - `Ok(None)` if no matching sample exists
    /// - `Err(_)` if decoding failed while pumping
    pub fn take_instance(&self, handle: InstanceHandle) -> Result<Option<T>> {
        self.pump_ring_to_cache()?;
        Ok(self.cache.take_instance(handle))
    }

    /// Take up to `max` samples for a specific instance, removing them.
    ///
    /// Returns and removes samples matching the given instance handle.
    ///
    /// # Arguments
    /// * `handle` - The instance handle (key hash) to filter by
    /// * `max` - Maximum number of samples to take
    pub fn take_instance_batch(&self, handle: InstanceHandle, max: usize) -> Result<Vec<T>> {
        self.pump_ring_to_cache()?;
        Ok(self.cache.take_instance_batch(handle, max))
    }
}

// Read operations require T: Clone (samples are copied, not moved)
impl<T: DDS + Clone> DataReader<T> {
    /// Read a single sample without removing it (DDS read semantics).
    ///
    /// This is the DDS-standard `read()` operation. It:
    /// 1. Pumps any pending samples from the network ring to the cache
    /// 2. Returns a clone of the next unread sample (marks it as READ)
    ///
    /// Unlike [`take()`](Self::take), `read()` does not remove the sample from
    /// the cache. The same sample can be taken later with `take()`.
    ///
    /// # Returns
    /// - `Ok(Some(sample))` if an unread sample was available
    /// - `Ok(None)` if no unread samples available
    /// - `Err(_)` if decoding failed
    ///
    /// # Example
    /// ```rust,no_run
    /// # use hdds::{Participant, QoS, Result};
    /// # fn main() -> Result<()> {
    /// # let participant = Participant::builder("test").build()?;
    /// # let reader = participant.create_reader::<MyType>("topic", QoS::default())?;
    /// // Read samples (non-destructive)
    /// while let Some(sample) = reader.read()? {
    ///     println!("Peeked: {:?}", sample);
    /// }
    /// // Samples still in cache, can be taken later
    /// while let Some(sample) = reader.take()? {
    ///     println!("Took: {:?}", sample);
    /// }
    /// # Ok(())
    /// # }
    /// # #[derive(hdds::DDS, Debug, Clone)] struct MyType { x: i32 }
    /// ```
    pub fn read(&self) -> Result<Option<T>> {
        // Check requested deadline before reading (DDS spec: fire callback on miss)
        self.check_requested_deadline();

        // Pump pending samples from ring to cache
        self.pump_ring_to_cache()?;

        // Read from cache (non-destructive)
        Ok(self.cache.read())
    }

    /// Read up to `max` samples without removing them.
    ///
    /// Returns clones of unread samples and marks them as READ.
    pub fn read_batch(&self, max: usize) -> Result<Vec<T>> {
        self.pump_ring_to_cache()?;
        Ok(self.cache.read_batch(max))
    }

    /// Read the next sample (DDS standard alias).
    ///
    /// Equivalent to [`read()`](Self::read).
    #[inline]
    pub fn read_next_sample(&self) -> Result<Option<T>> {
        self.read()
    }

    /// Read a single sample for a specific instance (DDS read_instance).
    ///
    /// Returns a clone of the first unread sample matching the given instance handle.
    /// The sample is marked as READ but remains in the cache.
    ///
    /// # Arguments
    /// * `handle` - The instance handle (key hash) to filter by
    ///
    /// # Returns
    /// - `Ok(Some(sample))` if a matching unread sample was found
    /// - `Ok(None)` if no matching unread sample exists
    /// - `Err(_)` if decoding failed while pumping
    pub fn read_instance(&self, handle: InstanceHandle) -> Result<Option<T>> {
        self.pump_ring_to_cache()?;
        Ok(self.cache.read_instance(handle))
    }

    /// Read up to `max` samples for a specific instance without removing them.
    ///
    /// Returns clones of unread samples matching the given instance handle.
    ///
    /// # Arguments
    /// * `handle` - The instance handle (key hash) to filter by
    /// * `max` - Maximum number of samples to read
    pub fn read_instance_batch(&self, handle: InstanceHandle, max: usize) -> Result<Vec<T>> {
        self.pump_ring_to_cache()?;
        Ok(self.cache.read_instance_batch(handle, max))
    }
}

impl<T: DDS> DataReader<T> {
    /// Pump samples from network ring to cache.
    ///
    /// Decodes all pending samples and stores them in the cache.
    fn pump_ring_to_cache(&self) -> Result<()> {
        let slab_pool = rt::get_slab_pool();

        while let Some(entry) = self.ring.pop() {
            let buf = slab_pool.get_buffer(entry.handle);
            let data_len = entry.len as usize;
            let slice = &buf[..data_len];

            let decode_result = T::decode_cdr2(slice);
            slab_pool.release(entry.handle);

            match decode_result {
                Ok(data) => {
                    // Compute instance handle from @key fields
                    let instance_handle = InstanceHandle::new(data.compute_key());

                    // Lifespan check: discard samples older than lifespan duration.
                    // Uses reception timestamp (sufficient on LAN where jitter << lifespan).
                    let max_lifespan = Duration::from_secs(u64::MAX);
                    if self.lifespan_duration != max_lifespan {
                        let age_ns = current_time_ns().saturating_sub(entry.timestamp_ns);
                        if Duration::from_nanos(age_ns) > self.lifespan_duration {
                            log::debug!(
                                "[READER] lifespan expired topic='{}' seq={}",
                                self.topic, entry.seq
                            );
                            continue;
                        }
                    }

                    // TIME_BASED_FILTER: per-instance minimum separation (DDS 2.2.2.4.2.11).
                    if !self.qos.time_based_filter.is_disabled() {
                        let mut checkers = self
                            .time_filter_checkers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        let checker = checkers
                            .entry(instance_handle)
                            .or_insert_with(|| {
                                TimeBasedFilterChecker::new(self.qos.time_based_filter)
                            });
                        if !checker.should_accept() {
                            log::debug!(
                                "[READER] time_based_filter reject topic='{}' seq={}",
                                self.topic, entry.seq
                            );
                            continue;
                        }
                        checker.mark_accepted();
                    }

                    let cached = CachedSample::with_instance(
                        data,
                        entry.seq as u64,
                        entry.timestamp_ns,
                        instance_handle,
                    );
                    self.cache.push(cached);

                    // Record sample reception for deadline tracking
                    self.deadline_tracker
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .on_sample();

                    // Update NACK scheduler if reliable
                    if let Some(scheduler) = &self.nack_scheduler {
                        if let Ok(mut sched) = scheduler.lock() {
                            sched.on_receive(u64::from(entry.seq));
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "[READER] decode failed topic='{}' seq={} len={}: {}",
                        self.topic,
                        entry.seq,
                        data_len,
                        e
                    );
                    return Err(e);
                }
            }
        }

        // Update status condition
        if self.cache.is_empty() {
            self.status_condition.clear_active_statuses();
        }

        Ok(())
    }

    #[must_use]
    pub fn stats(&self) -> ReaderStats {
        ReaderStats::default()
    }

    /// Drain all pending dispose/unregister lifecycle events.
    ///
    /// Returns events in the order they were received from the network.
    /// Each event indicates an instance that was disposed or unregistered
    /// by a remote writer.
    ///
    /// # Example
    /// ```rust,no_run
    /// # use hdds::{Participant, QoS, Result};
    /// # fn main() -> Result<()> {
    /// # let participant = Participant::builder("test").build()?;
    /// # let reader = participant.create_reader::<MyType>("topic", QoS::default())?;
    /// for event in reader.get_dispose_events() {
    ///     println!("Instance {:02x?} was {:?}", &event.key_hash[..4], event.kind);
    /// }
    /// # Ok(())
    /// # }
    /// # #[derive(hdds::DDS, Debug)] struct MyType { x: i32 }
    /// ```
    pub fn get_dispose_events(&self) -> Vec<InstanceDisposeEvent> {
        let mut events = match self.dispose_events.lock() {
            Ok(mut guard) => {
                let drained: Vec<_> = guard.drain(..).collect();
                drained
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.drain(..).collect()
            }
        };

        events
            .drain(..)
            .map(|e| InstanceDisposeEvent {
                key_hash: e.key_hash,
                kind: e.kind,
                seq: e.seq,
            })
            .collect()
    }

    fn enforce_history(&self) {
        let max_samples = match self.qos.history {
            History::KeepLast(depth) => depth as usize,
            History::KeepAll => self.qos.resource_limits.max_samples,
        };

        if max_samples == 0 {
            return;
        }

        let slab_pool = rt::get_slab_pool();

        while self.ring.len() > max_samples {
            if let Some(entry) = self.ring.pop() {
                slab_pool.release(entry.handle);

                if let Some(metrics) = telemetry::get_metrics_opt() {
                    metrics.increment_dropped(1);
                }
            } else {
                break;
            }
        }
    }

    fn maybe_send_nack(&self) {
        let (transport, scheduler) = match (&self.transport, &self.nack_scheduler) {
            (Some(transport), Some(scheduler)) => (transport, scheduler),
            _ => return,
        };

        let mut sched = match scheduler.lock() {
            Ok(lock) => lock,
            Err(err) => {
                log::debug!(
                    "[Reader::maybe_send_nack] nack_scheduler lock poisoned; recovering. {:?}",
                    err
                );
                err.into_inner()
            }
        };

        let Some(gap_ranges) = sched.try_flush() else {
            return;
        };

        let packet = builder::build_acknack_packet_from_ranges(&gap_ranges);
        if let Err(err) = transport.send(&packet) {
            log::debug!("Failed to send ACKNACK: {}", err);
        }

        sched.on_nack_sent();
    }

    #[cfg(test)]
    pub(super) fn nack_scheduler_for_test(&self) -> Option<Arc<Mutex<NackScheduler>>> {
        self.nack_scheduler.clone()
    }

    #[cfg(test)]
    pub(super) fn ring_for_test(&self) -> &Arc<rt::IndexRing> {
        &self.ring
    }
}

/// Information about a dispose/unregister instance lifecycle event.
///
/// Returned by [`DataReader::get_dispose_events()`] when a remote writer
/// disposes or unregisters an instance.
#[derive(Debug, Clone)]
pub struct InstanceDisposeEvent {
    /// 16-byte key hash identifying the disposed/unregistered instance.
    pub key_hash: [u8; 16],
    /// The kind of lifecycle change.
    pub kind: DisposeKind,
    /// RTPS writer sequence number.
    pub seq: u64,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct ReaderStats {
    pub messages_received: u64,
    pub bytes_received: u64,
    pub drops: u64,
}
