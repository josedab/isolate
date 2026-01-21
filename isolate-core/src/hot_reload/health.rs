use super::version::VersionId;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Health metrics for a specific module version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionHealth {
    pub version_id: VersionId,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_latency_ms: u64,
}

impl VersionHealth {
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.failed_requests as f64 / self.total_requests as f64 * 100.0
    }

    pub fn avg_latency_ms(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        self.total_latency_ms as f64 / self.total_requests as f64
    }
}

struct VersionHealthCounters {
    total: AtomicU64,
    success: AtomicU64,
    failed: AtomicU64,
    latency_ms: AtomicU64,
}

impl VersionHealthCounters {
    fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            success: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            latency_ms: AtomicU64::new(0),
        }
    }

    fn snapshot(&self, version_id: VersionId) -> VersionHealth {
        VersionHealth {
            version_id,
            total_requests: self.total.load(Ordering::Relaxed),
            successful_requests: self.success.load(Ordering::Relaxed),
            failed_requests: self.failed.load(Ordering::Relaxed),
            total_latency_ms: self.latency_ms.load(Ordering::Relaxed),
        }
    }
}

/// Tracks health metrics per module version for deployment decisions.
pub struct HealthTracker {
    versions: dashmap::DashMap<VersionId, VersionHealthCounters>,
}

impl HealthTracker {
    pub fn new() -> Self {
        Self {
            versions: dashmap::DashMap::new(),
        }
    }

    /// Record a successful execution for a version.
    pub fn record_success(&self, version_id: &VersionId, latency_ms: u64) {
        let counters = self
            .versions
            .entry(version_id.clone())
            .or_insert_with(VersionHealthCounters::new);
        counters.total.fetch_add(1, Ordering::Relaxed);
        counters.success.fetch_add(1, Ordering::Relaxed);
        counters.latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
    }

    /// Record a failed execution for a version.
    pub fn record_failure(&self, version_id: &VersionId, latency_ms: u64) {
        let counters = self
            .versions
            .entry(version_id.clone())
            .or_insert_with(VersionHealthCounters::new);
        counters.total.fetch_add(1, Ordering::Relaxed);
        counters.failed.fetch_add(1, Ordering::Relaxed);
        counters.latency_ms.fetch_add(latency_ms, Ordering::Relaxed);
    }

    /// Get health snapshot for a version.
    pub fn get_health(&self, version_id: &VersionId) -> Option<VersionHealth> {
        self.versions
            .get(version_id)
            .map(|c| c.value().snapshot(version_id.clone()))
    }

    /// Check if a version exceeds the error rate threshold.
    pub fn exceeds_error_threshold(&self, version_id: &VersionId, threshold_pct: f64) -> bool {
        self.get_health(version_id)
            .map(|h| h.error_rate() > threshold_pct)
            .unwrap_or(false)
    }

    /// Remove tracking data for a version.
    pub fn remove(&self, version_id: &VersionId) {
        self.versions.remove(version_id);
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_tracking() {
        let tracker = HealthTracker::new();
        let vid = VersionId::new("v1");

        tracker.record_success(&vid, 10);
        tracker.record_success(&vid, 20);
        tracker.record_failure(&vid, 5);

        let health = tracker.get_health(&vid).unwrap();
        assert_eq!(health.total_requests, 3);
        assert_eq!(health.successful_requests, 2);
        assert_eq!(health.failed_requests, 1);
        assert!((health.error_rate() - 33.33).abs() < 1.0);
    }

    #[test]
    fn test_avg_latency() {
        let tracker = HealthTracker::new();
        let vid = VersionId::new("v1");
        tracker.record_success(&vid, 100);
        tracker.record_success(&vid, 200);

        let health = tracker.get_health(&vid).unwrap();
        assert_eq!(health.avg_latency_ms(), 150.0);
    }

    #[test]
    fn test_error_threshold() {
        let tracker = HealthTracker::new();
        let vid = VersionId::new("v1");

        for _ in 0..90 {
            tracker.record_success(&vid, 10);
        }
        for _ in 0..10 {
            tracker.record_failure(&vid, 10);
        }

        assert!(!tracker.exceeds_error_threshold(&vid, 15.0));
        assert!(tracker.exceeds_error_threshold(&vid, 5.0));
    }

    #[test]
    fn test_empty_health() {
        let tracker = HealthTracker::new();
        assert!(tracker.get_health(&VersionId::new("nope")).is_none());
    }

    #[test]
    fn test_zero_requests_rates() {
        let health = VersionHealth {
            version_id: VersionId::new("v0"),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_latency_ms: 0,
        };
        assert_eq!(health.error_rate(), 0.0);
        assert_eq!(health.avg_latency_ms(), 0.0);
    }
}
