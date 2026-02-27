use std::fmt;

/// Unique identifier for a billing tenant.
#[derive(Debug, Clone, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct TenantId(String);

impl TenantId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Aggregated resource usage for a single tenant.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TenantUsage {
    pub tenant_id: TenantId,
    pub execution_count: u64,
    pub total_wall_time_ms: u64,
    pub total_fuel_consumed: u64,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub peak_memory_bytes: u64,
    pub first_execution_epoch_ms: u64,
    pub last_execution_epoch_ms: u64,
}

impl TenantUsage {
    fn new(tenant_id: TenantId) -> Self {
        Self {
            tenant_id,
            execution_count: 0,
            total_wall_time_ms: 0,
            total_fuel_consumed: 0,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 0,
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 0,
        }
    }
}

/// Thread-safe tracker for per-tenant resource usage.
///
/// Uses `DashMap` for lock-free concurrent access across sandbox executions.
pub struct TenantUsageTracker {
    tenants: dashmap::DashMap<TenantId, TenantUsage>,
}

impl TenantUsageTracker {
    pub fn new() -> Self {
        Self { tenants: dashmap::DashMap::new() }
    }

    /// Record a sandbox execution for a tenant.
    pub fn record_execution(
        &self,
        tenant_id: &TenantId,
        wall_time: std::time::Duration,
        fuel_consumed: u64,
        bytes_read: u64,
        bytes_written: u64,
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut entry = self
            .tenants
            .entry(tenant_id.clone())
            .or_insert_with(|| TenantUsage::new(tenant_id.clone()));

        let usage = entry.value_mut();
        usage.execution_count += 1;
        usage.total_wall_time_ms += wall_time.as_millis() as u64;
        usage.total_fuel_consumed += fuel_consumed;
        usage.total_bytes_read += bytes_read;
        usage.total_bytes_written += bytes_written;
        usage.last_execution_epoch_ms = now;
        if usage.first_execution_epoch_ms == 0 {
            usage.first_execution_epoch_ms = now;
        }
    }

    /// Record peak memory for a tenant execution.
    pub fn record_peak_memory(&self, tenant_id: &TenantId, peak_bytes: u64) {
        if let Some(mut entry) = self.tenants.get_mut(tenant_id) {
            if peak_bytes > entry.peak_memory_bytes {
                entry.peak_memory_bytes = peak_bytes;
            }
        }
    }

    /// Get usage snapshot for a tenant.
    pub fn get_usage(&self, tenant_id: &TenantId) -> Option<TenantUsage> {
        self.tenants.get(tenant_id).map(|e| e.value().clone())
    }

    /// Get usage for all tenants.
    pub fn all_tenants(&self) -> Vec<TenantUsage> {
        self.tenants.iter().map(|e| e.value().clone()).collect()
    }

    /// Reset usage counters for a tenant (e.g., after billing cycle).
    pub fn reset(&self, tenant_id: &TenantId) {
        if let Some(mut entry) = self.tenants.get_mut(tenant_id) {
            let new_usage = TenantUsage::new(tenant_id.clone());
            *entry.value_mut() = new_usage;
        }
    }

    /// Number of tracked tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }
}

impl Default for TenantUsageTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_tenant_id_equality() {
        let t1 = TenantId::new("abc");
        let t2 = TenantId::new("abc");
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_record_and_retrieve() {
        let tracker = TenantUsageTracker::new();
        let tid = TenantId::new("test");
        tracker.record_execution(&tid, Duration::from_millis(100), 5000, 1024, 512);

        let usage = tracker.get_usage(&tid).unwrap();
        assert_eq!(usage.execution_count, 1);
        assert_eq!(usage.total_wall_time_ms, 100);
        assert_eq!(usage.total_fuel_consumed, 5000);
        assert_eq!(usage.total_bytes_read, 1024);
        assert_eq!(usage.total_bytes_written, 512);
    }

    #[test]
    fn test_accumulation() {
        let tracker = TenantUsageTracker::new();
        let tid = TenantId::new("accum");
        tracker.record_execution(&tid, Duration::from_millis(100), 1000, 100, 50);
        tracker.record_execution(&tid, Duration::from_millis(200), 2000, 200, 100);

        let usage = tracker.get_usage(&tid).unwrap();
        assert_eq!(usage.execution_count, 2);
        assert_eq!(usage.total_wall_time_ms, 300);
        assert_eq!(usage.total_fuel_consumed, 3000);
    }

    #[test]
    fn test_peak_memory_tracking() {
        let tracker = TenantUsageTracker::new();
        let tid = TenantId::new("mem");
        tracker.record_execution(&tid, Duration::from_millis(10), 100, 0, 0);
        tracker.record_peak_memory(&tid, 4096);
        tracker.record_peak_memory(&tid, 2048);

        let usage = tracker.get_usage(&tid).unwrap();
        assert_eq!(usage.peak_memory_bytes, 4096); // keeps max
    }

    #[test]
    fn test_reset() {
        let tracker = TenantUsageTracker::new();
        let tid = TenantId::new("reset");
        tracker.record_execution(&tid, Duration::from_secs(1), 100, 50, 25);
        tracker.reset(&tid);

        let usage = tracker.get_usage(&tid).unwrap();
        assert_eq!(usage.execution_count, 0);
    }

    #[test]
    fn test_all_tenants() {
        let tracker = TenantUsageTracker::new();
        tracker.record_execution(&TenantId::new("a"), Duration::from_millis(1), 1, 0, 0);
        tracker.record_execution(&TenantId::new("b"), Duration::from_millis(1), 1, 0, 0);
        assert_eq!(tracker.tenant_count(), 2);
        assert_eq!(tracker.all_tenants().len(), 2);
    }

    #[test]
    fn test_missing_tenant_returns_none() {
        let tracker = TenantUsageTracker::new();
        assert!(tracker.get_usage(&TenantId::new("nope")).is_none());
    }
}
