//! Health checking system for the admin dashboard.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Status of a system component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentStatus {
    /// Component is healthy.
    Healthy,
    /// Component is degraded but functional.
    Degraded,
    /// Component is unhealthy.
    Unhealthy,
    /// Component status is unknown.
    Unknown,
}

impl ComponentStatus {
    /// Check if the component is operational (healthy or degraded).
    pub fn is_operational(&self) -> bool {
        matches!(self, ComponentStatus::Healthy | ComponentStatus::Degraded)
    }
}

impl std::fmt::Display for ComponentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentStatus::Healthy => write!(f, "healthy"),
            ComponentStatus::Degraded => write!(f, "degraded"),
            ComponentStatus::Unhealthy => write!(f, "unhealthy"),
            ComponentStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Health information for a single component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    /// Component name.
    pub name: String,
    /// Current status.
    pub status: ComponentStatus,
    /// Human-readable message.
    pub message: Option<String>,
    /// Last check time.
    pub last_checked: DateTime<Utc>,
    /// Response time of last check.
    pub response_time_ms: Option<f64>,
}

/// Overall health report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Overall system status.
    pub status: ComponentStatus,
    /// Individual component statuses.
    pub components: Vec<ComponentHealth>,
    /// Timestamp of this report.
    pub timestamp: DateTime<Utc>,
    /// Uptime duration.
    pub uptime: Duration,
    /// Version string.
    pub version: String,
}

impl HealthReport {
    /// Check if the overall system is healthy.
    pub fn is_healthy(&self) -> bool {
        self.status == ComponentStatus::Healthy
    }

    /// Check if the system is operational.
    pub fn is_operational(&self) -> bool {
        self.status.is_operational()
    }

    /// Get unhealthy components.
    pub fn unhealthy_components(&self) -> Vec<&ComponentHealth> {
        self.components.iter().filter(|c| !c.status.is_operational()).collect()
    }
}

/// The health checking subsystem.
pub struct HealthChecker {
    /// Components being monitored.
    components: Arc<RwLock<HashMap<String, ComponentHealth>>>,
    /// Check interval.
    check_interval: Duration,
    /// When the system started.
    started_at: std::time::Instant,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new(check_interval: Duration) -> Self {
        let checker = Self {
            components: Arc::new(RwLock::new(HashMap::new())),
            check_interval,
            started_at: std::time::Instant::now(),
        };

        // Register default components
        checker.register_component("runtime", ComponentStatus::Healthy);
        checker.register_component("wasm_engine", ComponentStatus::Healthy);

        checker
    }

    /// Register or update a component's status.
    pub fn register_component(&self, name: impl Into<String>, status: ComponentStatus) {
        let name = name.into();
        let health = ComponentHealth {
            name: name.clone(),
            status,
            message: None,
            last_checked: Utc::now(),
            response_time_ms: None,
        };
        self.components.write().insert(name, health);
    }

    /// Update a component's status with a message.
    pub fn update_component(&self, name: &str, status: ComponentStatus, message: Option<String>) {
        if let Some(component) = self.components.write().get_mut(name) {
            component.status = status;
            component.message = message;
            component.last_checked = Utc::now();
        }
    }

    /// Perform a health check and return the report.
    pub fn check(&self) -> HealthReport {
        let components: Vec<ComponentHealth> = self.components.read().values().cloned().collect();

        // Determine overall status from component statuses
        let overall = if components.iter().any(|c| c.status == ComponentStatus::Unhealthy) {
            ComponentStatus::Unhealthy
        } else if components.iter().any(|c| c.status == ComponentStatus::Degraded) {
            ComponentStatus::Degraded
        } else if components.iter().all(|c| c.status == ComponentStatus::Healthy) {
            ComponentStatus::Healthy
        } else {
            ComponentStatus::Unknown
        };

        HealthReport {
            status: overall,
            components,
            timestamp: Utc::now(),
            uptime: self.started_at.elapsed(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Get the check interval.
    pub fn check_interval(&self) -> Duration {
        self.check_interval
    }

    /// Get uptime.
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }
}

/// A health check that can be run against a component.
#[derive(Debug, Clone)]
pub struct HealthCheck {
    /// Component name.
    pub name: String,
    /// Timeout for the check.
    pub timeout: Duration,
    /// Description of what is being checked.
    pub description: String,
}

impl HealthCheck {
    /// Create a new health check.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self { name: name.into(), timeout: Duration::from_secs(5), description: description.into() }
    }

    /// Set the timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_status() {
        assert!(ComponentStatus::Healthy.is_operational());
        assert!(ComponentStatus::Degraded.is_operational());
        assert!(!ComponentStatus::Unhealthy.is_operational());
        assert!(!ComponentStatus::Unknown.is_operational());
    }

    #[test]
    fn test_health_checker_defaults() {
        let checker = HealthChecker::new(Duration::from_secs(10));
        let report = checker.check();

        assert_eq!(report.status, ComponentStatus::Healthy);
        assert_eq!(report.components.len(), 2); // runtime + wasm_engine
        assert!(report.is_healthy());
    }

    #[test]
    fn test_health_checker_degraded() {
        let checker = HealthChecker::new(Duration::from_secs(10));
        checker.update_component(
            "wasm_engine",
            ComponentStatus::Degraded,
            Some("High memory usage".to_string()),
        );

        let report = checker.check();
        assert_eq!(report.status, ComponentStatus::Degraded);
        assert!(report.is_operational());
        assert!(!report.is_healthy());
    }

    #[test]
    fn test_health_checker_unhealthy() {
        let checker = HealthChecker::new(Duration::from_secs(10));
        checker.update_component("runtime", ComponentStatus::Unhealthy, None);

        let report = checker.check();
        assert_eq!(report.status, ComponentStatus::Unhealthy);
        assert!(!report.is_operational());
    }

    #[test]
    fn test_health_checker_custom_component() {
        let checker = HealthChecker::new(Duration::from_secs(10));
        checker.register_component("database", ComponentStatus::Healthy);

        let report = checker.check();
        assert_eq!(report.components.len(), 3);
    }

    #[test]
    fn test_unhealthy_components() {
        let checker = HealthChecker::new(Duration::from_secs(10));
        checker.register_component("broken", ComponentStatus::Unhealthy);

        let report = checker.check();
        let unhealthy = report.unhealthy_components();
        assert_eq!(unhealthy.len(), 1);
        assert_eq!(unhealthy[0].name, "broken");
    }

    #[test]
    fn test_health_check_definition() {
        let check = HealthCheck::new("db", "Check database connectivity")
            .with_timeout(Duration::from_secs(3));

        assert_eq!(check.name, "db");
        assert_eq!(check.timeout, Duration::from_secs(3));
    }
}
