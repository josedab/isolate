//! System overview and operational insights.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Severity level for system alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SystemAlertLevel {
    /// Informational alert.
    Info,
    /// Warning - something needs attention.
    Warning,
    /// Critical - immediate action required.
    Critical,
}

impl std::fmt::Display for SystemAlertLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemAlertLevel::Info => write!(f, "info"),
            SystemAlertLevel::Warning => write!(f, "warning"),
            SystemAlertLevel::Critical => write!(f, "critical"),
        }
    }
}

/// A system alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAlert {
    /// Alert severity.
    pub level: SystemAlertLevel,
    /// Alert message.
    pub message: String,
    /// When the alert was raised.
    pub timestamp: DateTime<Utc>,
    /// Whether the alert has been acknowledged.
    pub acknowledged: bool,
}

/// Summary of a sandbox's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSummary {
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Module hash.
    pub module_hash: String,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// Number of executions.
    pub execution_count: u64,
}

/// Resource usage overview.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceOverview {
    /// Current memory usage in bytes.
    pub memory_bytes: u64,
    /// Peak memory usage in bytes.
    pub peak_memory_bytes: u64,
    /// Active sandbox count.
    pub active_sandboxes: u64,
    /// Peak active sandboxes.
    pub peak_active_sandboxes: u64,
    /// Total executions.
    pub total_executions: u64,
}

/// Complete system overview for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOverview {
    /// When this overview was generated.
    pub timestamp: DateTime<Utc>,
    /// Total sandboxes created.
    pub total_sandboxes_created: u64,
    /// Currently active sandboxes.
    pub active_sandboxes: u64,
    /// Total executions.
    pub total_executions: u64,
    /// Successful executions.
    pub successful_executions: u64,
    /// Failed executions.
    pub failed_executions: u64,
    /// Success rate (0.0 - 1.0).
    pub success_rate: f64,
    /// Average execution duration.
    pub avg_execution_duration: Duration,
    /// Resource usage.
    pub resources: ResourceOverview,
    /// Recent alerts.
    pub recent_alerts: Vec<SystemAlert>,
    /// Active sandbox summaries.
    pub sandbox_summaries: Vec<SandboxSummary>,
}

/// Collects system overview data.
pub struct OverviewCollector {
    /// Active sandboxes.
    sandboxes: Arc<RwLock<Vec<SandboxSummary>>>,
    /// System alerts.
    alerts: Arc<RwLock<VecDeque<SystemAlert>>>,
    /// Maximum alerts to retain.
    max_alerts: usize,
    /// Counters.
    total_created: AtomicU64,
    total_executions: AtomicU64,
    successful_executions: AtomicU64,
    failed_executions: AtomicU64,
    total_execution_duration_ms: AtomicU64,
    /// Resource tracking.
    current_memory: AtomicU64,
    peak_memory: AtomicU64,
    current_active: AtomicU64,
    peak_active: AtomicU64,
    /// History window.
    _history_window: Duration,
}

impl OverviewCollector {
    /// Create a new overview collector.
    pub fn new(max_alerts: usize, history_window: Duration) -> Self {
        Self {
            sandboxes: Arc::new(RwLock::new(Vec::new())),
            alerts: Arc::new(RwLock::new(VecDeque::new())),
            max_alerts,
            total_created: AtomicU64::new(0),
            total_executions: AtomicU64::new(0),
            successful_executions: AtomicU64::new(0),
            failed_executions: AtomicU64::new(0),
            total_execution_duration_ms: AtomicU64::new(0),
            current_memory: AtomicU64::new(0),
            peak_memory: AtomicU64::new(0),
            current_active: AtomicU64::new(0),
            peak_active: AtomicU64::new(0),
            _history_window: history_window,
        }
    }

    /// Record a sandbox creation.
    pub fn record_sandbox_created(&self, sandbox_id: &str, module_hash: &str) {
        self.total_created.fetch_add(1, Ordering::Relaxed);
        let active = self.current_active.fetch_add(1, Ordering::Relaxed) + 1;

        // Update peak
        let mut peak = self.peak_active.load(Ordering::Relaxed);
        while active > peak {
            match self.peak_active.compare_exchange_weak(
                peak,
                active,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }

        self.sandboxes.write().push(SandboxSummary {
            sandbox_id: sandbox_id.to_string(),
            module_hash: module_hash.to_string(),
            created_at: Utc::now(),
            execution_count: 0,
        });
    }

    /// Record a sandbox termination.
    pub fn record_sandbox_terminated(&self, sandbox_id: &str) {
        self.current_active.fetch_sub(1, Ordering::Relaxed);
        self.sandboxes.write().retain(|s| s.sandbox_id != sandbox_id);
    }

    /// Record an execution.
    pub fn record_execution(&self, sandbox_id: &str, duration: Duration, success: bool) {
        self.total_executions.fetch_add(1, Ordering::Relaxed);
        self.total_execution_duration_ms.fetch_add(duration.as_millis() as u64, Ordering::Relaxed);

        if success {
            self.successful_executions.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_executions.fetch_add(1, Ordering::Relaxed);
        }

        // Update sandbox execution count
        if let Some(summary) =
            self.sandboxes.write().iter_mut().find(|s| s.sandbox_id == sandbox_id)
        {
            summary.execution_count += 1;
        }
    }

    /// Record resource usage.
    pub fn record_resource_usage(&self, memory_bytes: u64, active_sandboxes: u64) {
        self.current_memory.store(memory_bytes, Ordering::Relaxed);
        self.current_active.store(active_sandboxes, Ordering::Relaxed);

        // Update peaks
        let mut peak = self.peak_memory.load(Ordering::Relaxed);
        while memory_bytes > peak {
            match self.peak_memory.compare_exchange_weak(
                peak,
                memory_bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }
    }

    /// Add a system alert.
    pub fn add_alert(&self, level: SystemAlertLevel, message: impl Into<String>) {
        let alert = SystemAlert {
            level,
            message: message.into(),
            timestamp: Utc::now(),
            acknowledged: false,
        };

        let mut alerts = self.alerts.write();
        alerts.push_back(alert);
        while alerts.len() > self.max_alerts {
            alerts.pop_front();
        }
    }

    /// Get active (unacknowledged) alerts.
    pub fn active_alerts(&self) -> Vec<SystemAlert> {
        self.alerts.read().iter().filter(|a| !a.acknowledged).cloned().collect()
    }

    /// Collect the current system overview.
    pub fn collect(&self) -> SystemOverview {
        let total_executions = self.total_executions.load(Ordering::Relaxed);
        let successful = self.successful_executions.load(Ordering::Relaxed);
        let failed = self.failed_executions.load(Ordering::Relaxed);
        let total_duration_ms = self.total_execution_duration_ms.load(Ordering::Relaxed);

        let success_rate =
            if total_executions > 0 { successful as f64 / total_executions as f64 } else { 0.0 };

        let avg_duration = if total_executions > 0 {
            Duration::from_millis(total_duration_ms / total_executions)
        } else {
            Duration::ZERO
        };

        let current_memory = self.current_memory.load(Ordering::Relaxed);
        let active_sandboxes = self.current_active.load(Ordering::Relaxed);

        SystemOverview {
            timestamp: Utc::now(),
            total_sandboxes_created: self.total_created.load(Ordering::Relaxed),
            active_sandboxes,
            total_executions,
            successful_executions: successful,
            failed_executions: failed,
            success_rate,
            avg_execution_duration: avg_duration,
            resources: ResourceOverview {
                memory_bytes: current_memory,
                peak_memory_bytes: self.peak_memory.load(Ordering::Relaxed),
                active_sandboxes,
                peak_active_sandboxes: self.peak_active.load(Ordering::Relaxed),
                total_executions,
            },
            recent_alerts: self.alerts.read().iter().rev().take(10).cloned().collect(),
            sandbox_summaries: self.sandboxes.read().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_levels() {
        assert!(SystemAlertLevel::Info < SystemAlertLevel::Warning);
        assert!(SystemAlertLevel::Warning < SystemAlertLevel::Critical);
    }

    #[test]
    fn test_overview_collector_empty() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));
        let overview = collector.collect();

        assert_eq!(overview.total_sandboxes_created, 0);
        assert_eq!(overview.active_sandboxes, 0);
        assert_eq!(overview.total_executions, 0);
        assert_eq!(overview.success_rate, 0.0);
    }

    #[test]
    fn test_overview_sandbox_lifecycle() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));

        collector.record_sandbox_created("sb-1", "hash-abc");
        collector.record_sandbox_created("sb-2", "hash-def");

        let overview = collector.collect();
        assert_eq!(overview.total_sandboxes_created, 2);
        assert_eq!(overview.active_sandboxes, 2);
        assert_eq!(overview.sandbox_summaries.len(), 2);

        collector.record_sandbox_terminated("sb-1");

        let overview = collector.collect();
        assert_eq!(overview.active_sandboxes, 1);
        assert_eq!(overview.sandbox_summaries.len(), 1);
    }

    #[test]
    fn test_overview_executions() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));

        collector.record_sandbox_created("sb-1", "hash-abc");
        collector.record_execution("sb-1", Duration::from_millis(100), true);
        collector.record_execution("sb-1", Duration::from_millis(200), true);
        collector.record_execution("sb-1", Duration::from_millis(300), false);

        let overview = collector.collect();
        assert_eq!(overview.total_executions, 3);
        assert_eq!(overview.successful_executions, 2);
        assert_eq!(overview.failed_executions, 1);
        assert!((overview.success_rate - 0.6667).abs() < 0.01);
        assert_eq!(overview.avg_execution_duration, Duration::from_millis(200));
    }

    #[test]
    fn test_overview_resource_tracking() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));

        collector.record_resource_usage(1024, 1);
        collector.record_resource_usage(2048, 2);
        collector.record_resource_usage(512, 1);

        let overview = collector.collect();
        assert_eq!(overview.resources.memory_bytes, 512);
        assert_eq!(overview.resources.peak_memory_bytes, 2048);
    }

    #[test]
    fn test_overview_alerts() {
        let collector = OverviewCollector::new(3, Duration::from_secs(3600));

        collector.add_alert(SystemAlertLevel::Info, "System started");
        collector.add_alert(SystemAlertLevel::Warning, "High load");
        collector.add_alert(SystemAlertLevel::Critical, "OOM risk");
        collector.add_alert(SystemAlertLevel::Info, "Load normalized");

        // Max 3 alerts, oldest should be evicted
        let alerts = collector.active_alerts();
        assert_eq!(alerts.len(), 3);
    }

    #[test]
    fn test_overview_peak_active() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));

        collector.record_sandbox_created("sb-1", "h1");
        collector.record_sandbox_created("sb-2", "h2");
        collector.record_sandbox_created("sb-3", "h3");
        collector.record_sandbox_terminated("sb-1");
        collector.record_sandbox_terminated("sb-2");

        let overview = collector.collect();
        assert_eq!(overview.resources.active_sandboxes, 1);
        assert_eq!(overview.resources.peak_active_sandboxes, 3);
    }

    #[test]
    fn test_sandbox_execution_count() {
        let collector = OverviewCollector::new(100, Duration::from_secs(3600));

        collector.record_sandbox_created("sb-1", "h1");
        collector.record_execution("sb-1", Duration::from_millis(50), true);
        collector.record_execution("sb-1", Duration::from_millis(50), true);

        let overview = collector.collect();
        let sb = overview.sandbox_summaries.iter().find(|s| s.sandbox_id == "sb-1").unwrap();
        assert_eq!(sb.execution_count, 2);
    }
}
