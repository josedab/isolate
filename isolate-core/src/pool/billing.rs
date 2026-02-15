//! Usage metering and billing hooks for multi-tenant sandbox orchestration.
//!
//! Tracks per-tenant resource consumption (CPU-seconds, memory-seconds, I/O bytes)
//! and emits billing events for downstream processing.

use super::tenant::TenantId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A billable usage record for a single sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Tenant that owns the execution.
    pub tenant_id: String,
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Execution wall time.
    pub wall_time: Duration,
    /// CPU time consumed (fuel-based approximation).
    pub cpu_time: Duration,
    /// Peak memory in bytes.
    pub peak_memory_bytes: u64,
    /// Memory-seconds (average memory × duration).
    pub memory_seconds: f64,
    /// I/O bytes read.
    pub io_bytes_read: u64,
    /// I/O bytes written.
    pub io_bytes_written: u64,
    /// Timestamp of the execution.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Aggregated usage for a tenant over a billing period.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantUsageSummary {
    /// Total executions in the period.
    pub total_executions: u64,
    /// Total CPU-seconds consumed.
    pub total_cpu_seconds: f64,
    /// Total memory-seconds consumed.
    pub total_memory_seconds: f64,
    /// Total I/O bytes (read + write).
    pub total_io_bytes: u64,
    /// Total wall-time seconds.
    pub total_wall_seconds: f64,
    /// Number of failed executions.
    pub failed_executions: u64,
}

/// Callback for billing events.
pub type BillingCallback = Box<dyn Fn(&UsageRecord) + Send + Sync>;

/// Usage meter that tracks per-tenant consumption and emits billing events.
pub struct UsageMeter {
    /// Per-tenant aggregated usage.
    summaries: HashMap<String, TenantUsageSummary>,
    /// All usage records (bounded ring buffer).
    records: Vec<UsageRecord>,
    /// Maximum records to retain.
    max_records: usize,
    /// Optional billing callback.
    callback: Option<BillingCallback>,
    /// Total executions across all tenants.
    total_executions: u64,
}

impl UsageMeter {
    /// Create a new usage meter.
    pub fn new(max_records: usize) -> Self {
        Self {
            summaries: HashMap::new(),
            records: Vec::new(),
            max_records,
            callback: None,
            total_executions: 0,
        }
    }

    /// Set a callback to be invoked for each usage record.
    pub fn set_callback(&mut self, callback: BillingCallback) {
        self.callback = Some(callback);
    }

    /// Record a usage event.
    pub fn record(&mut self, record: UsageRecord) {
        let tenant = record.tenant_id.clone();
        let summary = self.summaries.entry(tenant).or_default();
        summary.total_executions += 1;
        summary.total_cpu_seconds += record.cpu_time.as_secs_f64();
        summary.total_memory_seconds += record.memory_seconds;
        summary.total_io_bytes += record.io_bytes_read + record.io_bytes_written;
        summary.total_wall_seconds += record.wall_time.as_secs_f64();

        if let Some(cb) = &self.callback {
            cb(&record);
        }

        self.total_executions += 1;

        if self.records.len() >= self.max_records {
            self.records.remove(0);
        }
        self.records.push(record);
    }

    /// Get the aggregated summary for a tenant.
    pub fn tenant_summary(&self, tenant_id: &str) -> Option<&TenantUsageSummary> {
        self.summaries.get(tenant_id)
    }

    /// Get all tenant summaries.
    pub fn all_summaries(&self) -> &HashMap<String, TenantUsageSummary> {
        &self.summaries
    }

    /// Get the total number of recorded executions.
    pub fn total_executions(&self) -> u64 {
        self.total_executions
    }

    /// Get recent records for a tenant.
    pub fn recent_records(&self, tenant_id: &str, limit: usize) -> Vec<&UsageRecord> {
        self.records
            .iter()
            .rev()
            .filter(|r| r.tenant_id == tenant_id)
            .take(limit)
            .collect()
    }

    /// Reset all counters (e.g., at the start of a new billing period).
    pub fn reset(&mut self) {
        self.summaries.clear();
        self.records.clear();
        self.total_executions = 0;
    }
}

impl Default for UsageMeter {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// Priority-based fair scheduler for tenant sandbox requests.
///
/// Ensures no single tenant monopolizes resources by applying
/// weighted fair queuing based on tenant priority and current usage.
pub struct FairScheduler {
    /// Per-tenant scheduling state.
    tenants: HashMap<String, SchedulerTenantState>,
    /// Global request counter.
    total_requests: u64,
}

struct SchedulerTenantState {
    priority: u32,
    pending_requests: u64,
    completed_requests: u64,
    virtual_time: f64,
    last_scheduled: Option<Instant>,
}

/// Result of a scheduling decision.
#[derive(Debug, Clone)]
pub struct ScheduleDecision {
    /// The tenant that should be served next.
    pub tenant_id: String,
    /// Whether this request should be admitted.
    pub admitted: bool,
    /// Reason for the decision.
    pub reason: String,
    /// Suggested delay before processing (for backpressure).
    pub delay: Duration,
}

impl FairScheduler {
    /// Create a new fair scheduler.
    pub fn new() -> Self {
        Self { tenants: HashMap::new(), total_requests: 0 }
    }

    /// Register or update a tenant's priority.
    pub fn set_tenant_priority(&mut self, tenant_id: impl Into<String>, priority: u32) {
        let tenant_id = tenant_id.into();
        self.tenants.entry(tenant_id).or_insert_with(|| SchedulerTenantState {
            priority,
            pending_requests: 0,
            completed_requests: 0,
            virtual_time: 0.0,
            last_scheduled: None,
        }).priority = priority;
    }

    /// Submit a request for scheduling.
    pub fn submit(&mut self, tenant_id: &str, max_pending: u64) -> ScheduleDecision {
        self.total_requests += 1;

        let state = self.tenants.entry(tenant_id.to_string()).or_insert_with(|| {
            SchedulerTenantState {
                priority: 5,
                pending_requests: 0,
                completed_requests: 0,
                virtual_time: 0.0,
                last_scheduled: None,
            }
        });

        // Apply backpressure if tenant has too many pending requests
        if state.pending_requests >= max_pending {
            return ScheduleDecision {
                tenant_id: tenant_id.to_string(),
                admitted: false,
                reason: format!(
                    "backpressure: {} pending >= {} max",
                    state.pending_requests, max_pending
                ),
                delay: Duration::from_millis(100),
            };
        }

        // Weighted fair scheduling: lower virtual_time gets priority
        let weight = (state.priority as f64).max(1.0);
        state.virtual_time += 1.0 / weight;
        state.pending_requests += 1;
        state.last_scheduled = Some(Instant::now());

        ScheduleDecision {
            tenant_id: tenant_id.to_string(),
            admitted: true,
            reason: "admitted".to_string(),
            delay: Duration::ZERO,
        }
    }

    /// Mark a request as completed.
    pub fn complete(&mut self, tenant_id: &str) {
        if let Some(state) = self.tenants.get_mut(tenant_id) {
            state.pending_requests = state.pending_requests.saturating_sub(1);
            state.completed_requests += 1;
        }
    }

    /// Pick the next tenant to serve (lowest virtual time wins).
    pub fn next_tenant(&self) -> Option<&str> {
        self.tenants
            .iter()
            .filter(|(_, s)| s.pending_requests > 0)
            .min_by(|(_, a), (_, b)| a.virtual_time.partial_cmp(&b.virtual_time).unwrap())
            .map(|(id, _)| id.as_str())
    }

    /// Get the total number of pending requests across all tenants.
    pub fn total_pending(&self) -> u64 {
        self.tenants.values().map(|s| s.pending_requests).sum()
    }

    /// Get pending request count for a specific tenant.
    pub fn tenant_pending(&self, tenant_id: &str) -> u64 {
        self.tenants.get(tenant_id).map(|s| s.pending_requests).unwrap_or(0)
    }
}

impl Default for FairScheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_record() {
        let mut meter = UsageMeter::new(100);

        meter.record(UsageRecord {
            tenant_id: "tenant-a".to_string(),
            sandbox_id: "sb-1".to_string(),
            wall_time: Duration::from_secs(2),
            cpu_time: Duration::from_millis(500),
            peak_memory_bytes: 64 * 1024 * 1024,
            memory_seconds: 128.0,
            io_bytes_read: 1024,
            io_bytes_written: 512,
            timestamp: chrono::Utc::now(),
        });

        assert_eq!(meter.total_executions(), 1);
        let summary = meter.tenant_summary("tenant-a").unwrap();
        assert_eq!(summary.total_executions, 1);
        assert!(summary.total_cpu_seconds > 0.0);
    }

    #[test]
    fn test_usage_multiple_tenants() {
        let mut meter = UsageMeter::new(100);

        for i in 0..5 {
            meter.record(UsageRecord {
                tenant_id: "tenant-a".to_string(),
                sandbox_id: format!("sb-{}", i),
                wall_time: Duration::from_secs(1),
                cpu_time: Duration::from_millis(100),
                peak_memory_bytes: 1024,
                memory_seconds: 1.0,
                io_bytes_read: 0,
                io_bytes_written: 0,
                timestamp: chrono::Utc::now(),
            });
        }

        for i in 0..3 {
            meter.record(UsageRecord {
                tenant_id: "tenant-b".to_string(),
                sandbox_id: format!("sb-{}", i),
                wall_time: Duration::from_secs(1),
                cpu_time: Duration::from_millis(200),
                peak_memory_bytes: 2048,
                memory_seconds: 2.0,
                io_bytes_read: 0,
                io_bytes_written: 0,
                timestamp: chrono::Utc::now(),
            });
        }

        assert_eq!(meter.total_executions(), 8);
        assert_eq!(meter.tenant_summary("tenant-a").unwrap().total_executions, 5);
        assert_eq!(meter.tenant_summary("tenant-b").unwrap().total_executions, 3);
    }

    #[test]
    fn test_usage_record_limit() {
        let mut meter = UsageMeter::new(3);

        for i in 0..5 {
            meter.record(UsageRecord {
                tenant_id: "t".to_string(),
                sandbox_id: format!("sb-{}", i),
                wall_time: Duration::ZERO,
                cpu_time: Duration::ZERO,
                peak_memory_bytes: 0,
                memory_seconds: 0.0,
                io_bytes_read: 0,
                io_bytes_written: 0,
                timestamp: chrono::Utc::now(),
            });
        }

        let recent = meter.recent_records("t", 10);
        assert_eq!(recent.len(), 3); // capped at max_records
    }

    #[test]
    fn test_usage_callback() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let counter = Arc::new(AtomicU64::new(0));
        let counter_clone = counter.clone();

        let mut meter = UsageMeter::new(100);
        meter.set_callback(Box::new(move |_record| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        }));

        meter.record(UsageRecord {
            tenant_id: "t".to_string(),
            sandbox_id: "sb".to_string(),
            wall_time: Duration::ZERO,
            cpu_time: Duration::ZERO,
            peak_memory_bytes: 0,
            memory_seconds: 0.0,
            io_bytes_read: 0,
            io_bytes_written: 0,
            timestamp: chrono::Utc::now(),
        });

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_usage_reset() {
        let mut meter = UsageMeter::new(100);
        meter.record(UsageRecord {
            tenant_id: "t".to_string(),
            sandbox_id: "sb".to_string(),
            wall_time: Duration::ZERO,
            cpu_time: Duration::ZERO,
            peak_memory_bytes: 0,
            memory_seconds: 0.0,
            io_bytes_read: 0,
            io_bytes_written: 0,
            timestamp: chrono::Utc::now(),
        });

        meter.reset();
        assert_eq!(meter.total_executions(), 0);
        assert!(meter.tenant_summary("t").is_none());
    }

    #[test]
    fn test_fair_scheduler_admission() {
        let mut scheduler = FairScheduler::new();
        scheduler.set_tenant_priority("high", 10);
        scheduler.set_tenant_priority("low", 1);

        let d1 = scheduler.submit("high", 100);
        assert!(d1.admitted);

        let d2 = scheduler.submit("low", 100);
        assert!(d2.admitted);
    }

    #[test]
    fn test_fair_scheduler_backpressure() {
        let mut scheduler = FairScheduler::new();

        // Submit 3 requests with max_pending=2
        scheduler.submit("tenant-a", 2);
        scheduler.submit("tenant-a", 2);
        let d3 = scheduler.submit("tenant-a", 2);

        assert!(!d3.admitted);
        assert!(d3.reason.contains("backpressure"));
    }

    #[test]
    fn test_fair_scheduler_completion() {
        let mut scheduler = FairScheduler::new();

        scheduler.submit("tenant-a", 10);
        scheduler.submit("tenant-a", 10);
        assert_eq!(scheduler.tenant_pending("tenant-a"), 2);

        scheduler.complete("tenant-a");
        assert_eq!(scheduler.tenant_pending("tenant-a"), 1);
    }

    #[test]
    fn test_fair_scheduler_weighted_ordering() {
        let mut scheduler = FairScheduler::new();
        scheduler.set_tenant_priority("high", 10);
        scheduler.set_tenant_priority("low", 1);

        scheduler.submit("high", 100);
        scheduler.submit("low", 100);

        // High-priority tenant should have lower virtual_time
        let next = scheduler.next_tenant().unwrap();
        assert_eq!(next, "high");
    }

    #[test]
    fn test_fair_scheduler_total_pending() {
        let mut scheduler = FairScheduler::new();

        scheduler.submit("a", 100);
        scheduler.submit("b", 100);
        scheduler.submit("b", 100);

        assert_eq!(scheduler.total_pending(), 3);
    }
}
