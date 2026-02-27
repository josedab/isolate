use super::tenant::TenantId;
use crate::resource::ResourceUsage;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A billing event emitted each time a sandbox execution completes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingEvent {
    pub tenant_id: TenantId,
    pub sandbox_id: String,
    pub timestamp_epoch_ms: u64,
    pub wall_time: Duration,
    pub fuel_consumed: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
    pub peak_memory: u64,
    pub exit_code: i32,
}

impl BillingEvent {
    pub fn from_execution(
        tenant_id: TenantId,
        sandbox_id: impl Into<String>,
        resource_usage: &ResourceUsage,
        exit_code: i32,
    ) -> Self {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

        Self {
            tenant_id,
            sandbox_id: sandbox_id.into(),
            timestamp_epoch_ms: now,
            wall_time: resource_usage.wall_time,
            fuel_consumed: resource_usage.fuel_consumed,
            bytes_read: resource_usage.bytes_read,
            bytes_written: resource_usage.bytes_written,
            peak_memory: resource_usage.peak_memory as u64,
            exit_code,
        }
    }
}

/// Centralized billing meter that aggregates events and tracks global stats.
///
/// Thread-safe via `Arc` internals; clone freely across tasks.
pub struct BillingMeter {
    inner: Arc<BillingMeterInner>,
}

struct BillingMeterInner {
    events: parking_lot::Mutex<Vec<BillingEvent>>,
    total_events: AtomicU64,
    total_fuel: AtomicU64,
    total_bytes_read: AtomicU64,
    total_bytes_written: AtomicU64,
    max_events: usize,
}

impl BillingMeter {
    /// Create a new billing meter with default event buffer (10,000 events).
    pub fn new() -> Self {
        Self::with_capacity(10_000)
    }

    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            inner: Arc::new(BillingMeterInner {
                events: parking_lot::Mutex::new(Vec::with_capacity(max_events.min(1024))),
                total_events: AtomicU64::new(0),
                total_fuel: AtomicU64::new(0),
                total_bytes_read: AtomicU64::new(0),
                total_bytes_written: AtomicU64::new(0),
                max_events,
            }),
        }
    }

    /// Record a billing event.
    pub fn record(&self, event: BillingEvent) {
        self.inner.total_fuel.fetch_add(event.fuel_consumed, Ordering::Relaxed);
        self.inner.total_bytes_read.fetch_add(event.bytes_read, Ordering::Relaxed);
        self.inner.total_bytes_written.fetch_add(event.bytes_written, Ordering::Relaxed);
        self.inner.total_events.fetch_add(1, Ordering::Relaxed);

        let mut events = self.inner.events.lock();
        if events.len() >= self.inner.max_events {
            let half = events.len() / 2;
            events.drain(..half);
        }
        events.push(event);
    }

    /// Drain all buffered events (for export to billing provider).
    pub fn drain_events(&self) -> Vec<BillingEvent> {
        let mut events = self.inner.events.lock();
        std::mem::take(&mut *events)
    }

    /// Total events recorded since creation.
    pub fn total_events(&self) -> u64 {
        self.inner.total_events.load(Ordering::Relaxed)
    }

    /// Global stats snapshot.
    pub fn global_stats(&self) -> BillingGlobalStats {
        BillingGlobalStats {
            total_events: self.inner.total_events.load(Ordering::Relaxed),
            total_fuel: self.inner.total_fuel.load(Ordering::Relaxed),
            total_bytes_read: self.inner.total_bytes_read.load(Ordering::Relaxed),
            total_bytes_written: self.inner.total_bytes_written.load(Ordering::Relaxed),
            buffered_events: self.inner.events.lock().len() as u64,
        }
    }
}

impl Default for BillingMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for BillingMeter {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

/// Thread-safe alias (BillingMeter is already Arc-wrapped internally).
pub type SharedBillingMeter = BillingMeter;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BillingGlobalStats {
    pub total_events: u64,
    pub total_fuel: u64,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub buffered_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceUsage;

    fn make_event(tenant: &str) -> BillingEvent {
        let usage = ResourceUsage {
            wall_time: Duration::from_millis(100),
            fuel_consumed: 50_000,
            bytes_read: 1024,
            bytes_written: 512,
            ..Default::default()
        };
        BillingEvent::from_execution(TenantId::new(tenant), "sandbox-1", &usage, 0)
    }

    #[test]
    fn test_record_and_stats() {
        let meter = BillingMeter::new();
        meter.record(make_event("t1"));
        meter.record(make_event("t2"));

        let stats = meter.global_stats();
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.total_fuel, 100_000);
        assert_eq!(stats.buffered_events, 2);
    }

    #[test]
    fn test_drain_events() {
        let meter = BillingMeter::new();
        meter.record(make_event("t1"));
        meter.record(make_event("t1"));

        let events = meter.drain_events();
        assert_eq!(events.len(), 2);
        assert_eq!(meter.global_stats().buffered_events, 0);
        // Totals remain after drain
        assert_eq!(meter.total_events(), 2);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let meter = BillingMeter::with_capacity(4);
        for i in 0..6 {
            meter.record(make_event(&format!("t{i}")));
        }
        // Should have evicted half when hitting capacity
        let stats = meter.global_stats();
        assert!(stats.buffered_events <= 4);
        assert_eq!(stats.total_events, 6);
    }

    #[test]
    fn test_clone_shares_state() {
        let meter = BillingMeter::new();
        let meter2 = meter.clone();
        meter.record(make_event("t1"));
        assert_eq!(meter2.total_events(), 1);
    }
}
