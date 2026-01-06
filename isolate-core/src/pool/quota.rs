//! Tenant quota management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Tenant resource quotas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    /// Maximum total memory in bytes.
    pub max_memory: u64,
    /// Maximum number of concurrent sandboxes.
    pub max_sandboxes: u32,
    /// Maximum CPU time per sandbox in milliseconds.
    pub max_cpu_time_ms: u64,
    /// Maximum I/O bytes per sandbox.
    pub max_io_bytes: u64,
    /// Maximum requests per second.
    pub max_rps: u32,
    /// Maximum sandboxes per minute (rate limit).
    pub max_sandboxes_per_minute: u32,
    /// Priority level (higher = more priority).
    pub priority: u8,
    /// Whether the tenant is enabled.
    pub enabled: bool,
}

impl Default for TenantQuota {
    fn default() -> Self {
        Self {
            max_memory: 1024 * 1024 * 1024, // 1GB
            max_sandboxes: 100,
            max_cpu_time_ms: 30_000,         // 30 seconds
            max_io_bytes: 100 * 1024 * 1024, // 100MB
            max_rps: 100,
            max_sandboxes_per_minute: 1000,
            priority: 5,
            enabled: true,
        }
    }
}

impl TenantQuota {
    /// Create a new quota with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an unlimited quota.
    pub fn unlimited() -> Self {
        Self {
            max_memory: u64::MAX,
            max_sandboxes: u32::MAX,
            max_cpu_time_ms: u64::MAX,
            max_io_bytes: u64::MAX,
            max_rps: u32::MAX,
            max_sandboxes_per_minute: u32::MAX,
            priority: 5,
            enabled: true,
        }
    }

    /// Create a minimal quota for testing.
    pub fn minimal() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024, // 64MB
            max_sandboxes: 5,
            max_cpu_time_ms: 5_000,         // 5 seconds
            max_io_bytes: 10 * 1024 * 1024, // 10MB
            max_rps: 10,
            max_sandboxes_per_minute: 60,
            priority: 1,
            enabled: true,
        }
    }

    /// Set maximum memory.
    pub fn with_max_memory(mut self, bytes: u64) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set maximum concurrent sandboxes.
    pub fn with_max_sandboxes(mut self, count: u32) -> Self {
        self.max_sandboxes = count;
        self
    }

    /// Set maximum CPU time per sandbox.
    pub fn with_max_cpu_time_ms(mut self, ms: u64) -> Self {
        self.max_cpu_time_ms = ms;
        self
    }

    /// Set maximum I/O bytes per sandbox.
    pub fn with_max_io_bytes(mut self, bytes: u64) -> Self {
        self.max_io_bytes = bytes;
        self
    }

    /// Set maximum requests per second.
    pub fn with_max_rps(mut self, rps: u32) -> Self {
        self.max_rps = rps;
        self
    }

    /// Set maximum sandboxes per minute.
    pub fn with_max_sandboxes_per_minute(mut self, count: u32) -> Self {
        self.max_sandboxes_per_minute = count;
        self
    }

    /// Set priority level.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Enable or disable the tenant.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Current resource usage for a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Current memory usage in bytes.
    pub memory_bytes: u64,
    /// Current number of active sandboxes.
    pub active_sandboxes: u32,
    /// Total sandboxes created.
    pub total_sandboxes: u64,
    /// Total CPU time consumed in milliseconds.
    pub total_cpu_time_ms: u64,
    /// Total I/O bytes consumed.
    pub total_io_bytes: u64,
    /// Requests in the current second.
    pub current_rps: u32,
    /// Sandboxes created in the current minute.
    pub sandboxes_this_minute: u32,
    /// Last update timestamp.
    pub last_updated: Option<DateTime<Utc>>,
}

impl ResourceUsage {
    /// Create new empty usage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if usage exceeds quota.
    pub fn exceeds_quota(&self, quota: &TenantQuota) -> Option<QuotaError> {
        if !quota.enabled {
            return Some(QuotaError::TenantDisabled);
        }

        if self.memory_bytes > quota.max_memory {
            return Some(QuotaError::MemoryExceeded {
                used: self.memory_bytes,
                limit: quota.max_memory,
            });
        }

        if self.active_sandboxes >= quota.max_sandboxes {
            return Some(QuotaError::SandboxLimitExceeded {
                used: self.active_sandboxes,
                limit: quota.max_sandboxes,
            });
        }

        if self.current_rps >= quota.max_rps {
            return Some(QuotaError::RateLimitExceeded {
                current: self.current_rps,
                limit: quota.max_rps,
            });
        }

        if self.sandboxes_this_minute >= quota.max_sandboxes_per_minute {
            return Some(QuotaError::MinuteRateLimitExceeded {
                current: self.sandboxes_this_minute,
                limit: quota.max_sandboxes_per_minute,
            });
        }

        None
    }

    /// Calculate memory utilization percentage.
    pub fn memory_utilization(&self, quota: &TenantQuota) -> f64 {
        if quota.max_memory == 0 || quota.max_memory == u64::MAX {
            return 0.0;
        }
        (self.memory_bytes as f64 / quota.max_memory as f64) * 100.0
    }

    /// Calculate sandbox utilization percentage.
    pub fn sandbox_utilization(&self, quota: &TenantQuota) -> f64 {
        if quota.max_sandboxes == 0 || quota.max_sandboxes == u32::MAX {
            return 0.0;
        }
        (self.active_sandboxes as f64 / quota.max_sandboxes as f64) * 100.0
    }
}

/// Atomic resource usage for concurrent updates.
pub struct AtomicResourceUsage {
    memory_bytes: AtomicU64,
    active_sandboxes: AtomicU64,
    total_sandboxes: AtomicU64,
    total_cpu_time_ms: AtomicU64,
    total_io_bytes: AtomicU64,
    current_rps: AtomicU64,
    sandboxes_this_minute: AtomicU64,
}

impl AtomicResourceUsage {
    /// Create new atomic usage counters.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            memory_bytes: AtomicU64::new(0),
            active_sandboxes: AtomicU64::new(0),
            total_sandboxes: AtomicU64::new(0),
            total_cpu_time_ms: AtomicU64::new(0),
            total_io_bytes: AtomicU64::new(0),
            current_rps: AtomicU64::new(0),
            sandboxes_this_minute: AtomicU64::new(0),
        })
    }

    /// Add memory usage.
    pub fn add_memory(&self, bytes: u64) -> u64 {
        self.memory_bytes.fetch_add(bytes, Ordering::SeqCst) + bytes
    }

    /// Release memory usage.
    pub fn release_memory(&self, bytes: u64) -> u64 {
        let prev = self.memory_bytes.fetch_sub(bytes, Ordering::SeqCst);
        prev.saturating_sub(bytes)
    }

    /// Increment active sandboxes.
    pub fn add_sandbox(&self) -> u32 {
        (self.active_sandboxes.fetch_add(1, Ordering::SeqCst) + 1) as u32
    }

    /// Decrement active sandboxes.
    pub fn remove_sandbox(&self) -> u32 {
        let prev = self.active_sandboxes.fetch_sub(1, Ordering::SeqCst);
        prev.saturating_sub(1) as u32
    }

    /// Increment total sandboxes and sandboxes this minute.
    pub fn record_sandbox_created(&self) {
        self.total_sandboxes.fetch_add(1, Ordering::SeqCst);
        self.sandboxes_this_minute.fetch_add(1, Ordering::SeqCst);
    }

    /// Add CPU time.
    pub fn add_cpu_time(&self, ms: u64) {
        self.total_cpu_time_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Add I/O bytes.
    pub fn add_io_bytes(&self, bytes: u64) {
        self.total_io_bytes.fetch_add(bytes, Ordering::SeqCst);
    }

    /// Increment RPS counter.
    pub fn increment_rps(&self) -> u32 {
        (self.current_rps.fetch_add(1, Ordering::SeqCst) + 1) as u32
    }

    /// Reset per-second counters.
    pub fn reset_rps(&self) {
        self.current_rps.store(0, Ordering::SeqCst);
    }

    /// Reset per-minute counters.
    pub fn reset_minute_counters(&self) {
        self.sandboxes_this_minute.store(0, Ordering::SeqCst);
    }

    /// Get a snapshot of current usage.
    pub fn snapshot(&self) -> ResourceUsage {
        ResourceUsage {
            memory_bytes: self.memory_bytes.load(Ordering::SeqCst),
            active_sandboxes: self.active_sandboxes.load(Ordering::SeqCst) as u32,
            total_sandboxes: self.total_sandboxes.load(Ordering::SeqCst),
            total_cpu_time_ms: self.total_cpu_time_ms.load(Ordering::SeqCst),
            total_io_bytes: self.total_io_bytes.load(Ordering::SeqCst),
            current_rps: self.current_rps.load(Ordering::SeqCst) as u32,
            sandboxes_this_minute: self.sandboxes_this_minute.load(Ordering::SeqCst) as u32,
            last_updated: Some(Utc::now()),
        }
    }

    /// Check if usage exceeds quota.
    pub fn exceeds_quota(&self, quota: &TenantQuota) -> Option<QuotaError> {
        self.snapshot().exceeds_quota(quota)
    }
}

impl Default for AtomicResourceUsage {
    fn default() -> Self {
        Self {
            memory_bytes: AtomicU64::new(0),
            active_sandboxes: AtomicU64::new(0),
            total_sandboxes: AtomicU64::new(0),
            total_cpu_time_ms: AtomicU64::new(0),
            total_io_bytes: AtomicU64::new(0),
            current_rps: AtomicU64::new(0),
            sandboxes_this_minute: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for AtomicResourceUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AtomicResourceUsage")
            .field("memory_bytes", &self.memory_bytes.load(Ordering::SeqCst))
            .field(
                "active_sandboxes",
                &self.active_sandboxes.load(Ordering::SeqCst),
            )
            .field(
                "total_sandboxes",
                &self.total_sandboxes.load(Ordering::SeqCst),
            )
            .finish_non_exhaustive()
    }
}

/// Quota-related errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum QuotaError {
    /// Tenant is disabled.
    #[error("Tenant is disabled")]
    TenantDisabled,

    /// Memory limit exceeded.
    #[error("Memory limit exceeded: {used} / {limit} bytes")]
    MemoryExceeded { used: u64, limit: u64 },

    /// Sandbox limit exceeded.
    #[error("Sandbox limit exceeded: {used} / {limit}")]
    SandboxLimitExceeded { used: u32, limit: u32 },

    /// Rate limit exceeded.
    #[error("Rate limit exceeded: {current} / {limit} RPS")]
    RateLimitExceeded { current: u32, limit: u32 },

    /// Minute rate limit exceeded.
    #[error("Minute rate limit exceeded: {current} / {limit} per minute")]
    MinuteRateLimitExceeded { current: u32, limit: u32 },

    /// CPU time limit exceeded.
    #[error("CPU time limit exceeded: {used} / {limit} ms")]
    CpuTimeExceeded { used: u64, limit: u64 },

    /// I/O limit exceeded.
    #[error("I/O limit exceeded: {used} / {limit} bytes")]
    IoExceeded { used: u64, limit: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_quota_default() {
        let quota = TenantQuota::default();

        assert_eq!(quota.max_memory, 1024 * 1024 * 1024);
        assert_eq!(quota.max_sandboxes, 100);
        assert!(quota.enabled);
    }

    #[test]
    fn test_tenant_quota_builder() {
        let quota = TenantQuota::new()
            .with_max_memory(512 * 1024 * 1024)
            .with_max_sandboxes(50)
            .with_priority(10);

        assert_eq!(quota.max_memory, 512 * 1024 * 1024);
        assert_eq!(quota.max_sandboxes, 50);
        assert_eq!(quota.priority, 10);
    }

    #[test]
    fn test_tenant_quota_unlimited() {
        let quota = TenantQuota::unlimited();

        assert_eq!(quota.max_memory, u64::MAX);
        assert_eq!(quota.max_sandboxes, u32::MAX);
    }

    #[test]
    fn test_tenant_quota_minimal() {
        let quota = TenantQuota::minimal();

        assert_eq!(quota.max_memory, 64 * 1024 * 1024);
        assert_eq!(quota.max_sandboxes, 5);
    }

    #[test]
    fn test_resource_usage_exceeds_quota() {
        let quota = TenantQuota::new()
            .with_max_memory(100)
            .with_max_sandboxes(5);

        let mut usage = ResourceUsage::new();
        assert!(usage.exceeds_quota(&quota).is_none());

        usage.memory_bytes = 150;
        assert!(matches!(
            usage.exceeds_quota(&quota),
            Some(QuotaError::MemoryExceeded { .. })
        ));

        usage.memory_bytes = 50;
        usage.active_sandboxes = 5;
        assert!(matches!(
            usage.exceeds_quota(&quota),
            Some(QuotaError::SandboxLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_usage_disabled_tenant() {
        let quota = TenantQuota::new().with_enabled(false);
        let usage = ResourceUsage::new();

        assert!(matches!(
            usage.exceeds_quota(&quota),
            Some(QuotaError::TenantDisabled)
        ));
    }

    #[test]
    fn test_resource_usage_utilization() {
        let quota = TenantQuota::new()
            .with_max_memory(1000)
            .with_max_sandboxes(10);

        let mut usage = ResourceUsage::new();
        usage.memory_bytes = 500;
        usage.active_sandboxes = 5;

        assert!((usage.memory_utilization(&quota) - 50.0).abs() < 0.01);
        assert!((usage.sandbox_utilization(&quota) - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_atomic_resource_usage() {
        let usage = AtomicResourceUsage::new();

        usage.add_memory(1000);
        assert_eq!(usage.snapshot().memory_bytes, 1000);

        usage.release_memory(500);
        assert_eq!(usage.snapshot().memory_bytes, 500);

        usage.add_sandbox();
        usage.add_sandbox();
        assert_eq!(usage.snapshot().active_sandboxes, 2);

        usage.remove_sandbox();
        assert_eq!(usage.snapshot().active_sandboxes, 1);

        usage.record_sandbox_created();
        assert_eq!(usage.snapshot().total_sandboxes, 1);
        assert_eq!(usage.snapshot().sandboxes_this_minute, 1);
    }

    #[test]
    fn test_atomic_resource_usage_rps() {
        let usage = AtomicResourceUsage::new();

        usage.increment_rps();
        usage.increment_rps();
        assert_eq!(usage.snapshot().current_rps, 2);

        usage.reset_rps();
        assert_eq!(usage.snapshot().current_rps, 0);
    }

    #[test]
    fn test_atomic_resource_usage_minute_reset() {
        let usage = AtomicResourceUsage::new();

        usage.record_sandbox_created();
        usage.record_sandbox_created();
        assert_eq!(usage.snapshot().sandboxes_this_minute, 2);

        usage.reset_minute_counters();
        assert_eq!(usage.snapshot().sandboxes_this_minute, 0);
    }

    #[test]
    fn test_atomic_exceeds_quota() {
        let quota = TenantQuota::new().with_max_memory(100);
        let usage = AtomicResourceUsage::new();

        assert!(usage.exceeds_quota(&quota).is_none());

        usage.add_memory(150);
        assert!(usage.exceeds_quota(&quota).is_some());
    }
}
