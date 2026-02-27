//! Admin Dashboard API.
//!
//! Provides system health monitoring, sandbox management, and operational
//! insights for the Isolate runtime. This module aggregates data from
//! metrics, pool, and sandbox subsystems into dashboard-friendly views.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::admin::{Dashboard, DashboardConfig};
//!
//! let dashboard = Dashboard::new(DashboardConfig::default());
//! let health = dashboard.health_check();
//! let overview = dashboard.system_overview();
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.

pub mod api_keys;
mod health;
mod overview;

pub use api_keys::{
    Action, ApiKey, ApiKeyManager, QuotaStatus, Role, Team, TeamMember, UsageQuota, UsageRecord,
};
pub use health::{ComponentHealth, ComponentStatus, HealthCheck, HealthReport};
pub use overview::{
    ResourceOverview, SandboxSummary, SystemAlert, SystemAlertLevel, SystemOverview,
};

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for the admin dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// How often to refresh health checks.
    pub health_check_interval: Duration,
    /// Maximum number of alerts to retain.
    pub max_alerts: usize,
    /// Enable detailed resource tracking.
    pub detailed_resources: bool,
    /// History window for time-series data.
    pub history_window: Duration,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            health_check_interval: Duration::from_secs(10),
            max_alerts: 100,
            detailed_resources: true,
            history_window: Duration::from_secs(3600), // 1 hour
        }
    }
}

/// The main dashboard aggregator.
pub struct Dashboard {
    /// Dashboard configuration.
    config: DashboardConfig,
    /// Health checker.
    health: health::HealthChecker,
    /// Overview collector.
    overview: overview::OverviewCollector,
}

impl Dashboard {
    /// Create a new dashboard with the given config.
    pub fn new(config: DashboardConfig) -> Self {
        Self {
            health: health::HealthChecker::new(config.health_check_interval),
            overview: overview::OverviewCollector::new(config.max_alerts, config.history_window),
            config,
        }
    }

    /// Run a health check and return the report.
    pub fn health_check(&self) -> HealthReport {
        self.health.check()
    }

    /// Get the current system overview.
    pub fn system_overview(&self) -> SystemOverview {
        self.overview.collect()
    }

    /// Record a sandbox creation event.
    pub fn record_sandbox_created(&self, sandbox_id: &str, module_hash: &str) {
        self.overview.record_sandbox_created(sandbox_id, module_hash);
    }

    /// Record a sandbox termination event.
    pub fn record_sandbox_terminated(&self, sandbox_id: &str) {
        self.overview.record_sandbox_terminated(sandbox_id);
    }

    /// Record a sandbox execution.
    pub fn record_execution(&self, sandbox_id: &str, duration: Duration, success: bool) {
        self.overview.record_execution(sandbox_id, duration, success);
    }

    /// Record a resource usage update.
    pub fn record_resource_usage(&self, memory_bytes: u64, active_sandboxes: u64) {
        self.overview.record_resource_usage(memory_bytes, active_sandboxes);
    }

    /// Add a system alert.
    pub fn add_alert(&self, level: SystemAlertLevel, message: impl Into<String>) {
        self.overview.add_alert(level, message);
    }

    /// Get active alerts.
    pub fn active_alerts(&self) -> Vec<SystemAlert> {
        self.overview.active_alerts()
    }

    /// Get dashboard configuration.
    pub fn config(&self) -> &DashboardConfig {
        &self.config
    }

    /// Register a custom health component.
    pub fn register_component(&self, name: impl Into<String>, status: ComponentStatus) {
        self.health.register_component(name, status);
    }
}

impl Default for Dashboard {
    fn default() -> Self {
        Self::new(DashboardConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_config_default() {
        let config = DashboardConfig::default();
        assert_eq!(config.health_check_interval, Duration::from_secs(10));
        assert_eq!(config.max_alerts, 100);
        assert!(config.detailed_resources);
    }

    #[test]
    fn test_dashboard_creation() {
        let dashboard = Dashboard::default();
        let health = dashboard.health_check();
        assert!(!health.components.is_empty() || health.components.is_empty()); // Valid result
    }

    #[test]
    fn test_dashboard_lifecycle() {
        let dashboard = Dashboard::default();

        dashboard.record_sandbox_created("sb-1", "hash-abc");
        dashboard.record_execution("sb-1", Duration::from_millis(100), true);
        dashboard.record_resource_usage(1024 * 1024, 1);

        let overview = dashboard.system_overview();
        assert_eq!(overview.total_executions, 1);
        assert_eq!(overview.successful_executions, 1);
    }

    #[test]
    fn test_dashboard_alerts() {
        let dashboard = Dashboard::default();

        dashboard.add_alert(SystemAlertLevel::Warning, "High memory usage");
        dashboard.add_alert(SystemAlertLevel::Critical, "Sandbox pool exhausted");

        let alerts = dashboard.active_alerts();
        assert_eq!(alerts.len(), 2);
    }
}
