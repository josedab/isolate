//! Production configuration and GA readiness for WASI Preview 2.
//!
//! Provides production-hardened defaults, deployment profiles, and GA validation
//! for running WASI Preview 2 components in production environments.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::wasi2::production::{ProductionConfig, DeploymentProfile};
//!
//! let config = ProductionConfig::builder()
//!     .profile(DeploymentProfile::Production)
//!     .max_component_size(50 * 1024 * 1024)
//!     .require_stable_interfaces(true)
//!     .build();
//!
//! let report = config.validate_for_deployment(&component_interfaces);
//! assert!(report.is_ga_ready);
//! ```

use super::readiness::{InterfaceStability, ReadinessAssessment, StabilityLevel};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Deployment profile presets for different environments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentProfile {
    /// Development: relaxed limits, experimental interfaces allowed.
    Development,
    /// Staging: production-like limits, preview interfaces allowed.
    Staging,
    /// Production: strict limits, only stable+preview interfaces.
    Production,
}

impl std::fmt::Display for DeploymentProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Development => write!(f, "development"),
            Self::Staging => write!(f, "staging"),
            Self::Production => write!(f, "production"),
        }
    }
}

/// Production configuration for WASI Preview 2 GA deployments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionConfig {
    /// Deployment profile.
    pub profile: DeploymentProfile,
    /// Maximum component binary size in bytes.
    pub max_component_size: usize,
    /// Maximum composition depth for nested components.
    pub max_composition_depth: usize,
    /// Whether to reject components using experimental interfaces.
    pub require_stable_interfaces: bool,
    /// Minimum allowed stability level for interfaces.
    pub minimum_stability: StabilityLevel,
    /// Default resource limits for components.
    pub default_memory_limit: usize,
    /// Default execution timeout.
    pub default_timeout: Duration,
    /// Default fuel budget.
    pub default_fuel: u64,
    /// Enable component caching.
    pub enable_caching: bool,
    /// Maximum cached components.
    pub max_cache_entries: usize,
    /// Enable metrics collection.
    pub enable_metrics: bool,
    /// Enable execution tracing.
    pub enable_tracing: bool,
}

impl Default for ProductionConfig {
    fn default() -> Self {
        Self::for_profile(DeploymentProfile::Production)
    }
}

impl ProductionConfig {
    /// Create configuration for a specific deployment profile.
    pub fn for_profile(profile: DeploymentProfile) -> Self {
        match profile {
            DeploymentProfile::Development => Self {
                profile,
                max_component_size: 256 * 1024 * 1024,
                max_composition_depth: 20,
                require_stable_interfaces: false,
                minimum_stability: StabilityLevel::Experimental,
                default_memory_limit: 256 * 1024 * 1024,
                default_timeout: Duration::from_secs(300),
                default_fuel: 10_000_000_000,
                enable_caching: false,
                max_cache_entries: 10,
                enable_metrics: false,
                enable_tracing: true,
            },
            DeploymentProfile::Staging => Self {
                profile,
                max_component_size: 100 * 1024 * 1024,
                max_composition_depth: 10,
                require_stable_interfaces: false,
                minimum_stability: StabilityLevel::Preview,
                default_memory_limit: 128 * 1024 * 1024,
                default_timeout: Duration::from_secs(60),
                default_fuel: 5_000_000_000,
                enable_caching: true,
                max_cache_entries: 100,
                enable_metrics: true,
                enable_tracing: true,
            },
            DeploymentProfile::Production => Self {
                profile,
                max_component_size: 50 * 1024 * 1024,
                max_composition_depth: 5,
                require_stable_interfaces: true,
                minimum_stability: StabilityLevel::Preview,
                default_memory_limit: 64 * 1024 * 1024,
                default_timeout: Duration::from_secs(30),
                default_fuel: 1_000_000_000,
                enable_caching: true,
                max_cache_entries: 500,
                enable_metrics: true,
                enable_tracing: false,
            },
        }
    }

    /// Create a builder for custom configuration.
    pub fn builder() -> ProductionConfigBuilder {
        ProductionConfigBuilder::new()
    }

    /// Validate a component's interfaces against this configuration.
    pub fn validate_for_deployment(&self, interfaces: &[String]) -> GaReadinessReport {
        let readiness = ReadinessAssessment::evaluate(interfaces);
        let mut issues = Vec::new();
        let mut recommendations = Vec::new();

        // Check interface stability
        for (iface, level) in &readiness.interface_levels {
            if *level < self.minimum_stability {
                issues.push(GaIssue {
                    severity: IssueSeverity::Error,
                    category: IssueCategory::Stability,
                    message: format!(
                        "Interface '{}' has stability '{}' but minimum '{}' is required",
                        iface, level, self.minimum_stability
                    ),
                });
            }
        }

        // Check for experimental interfaces in production
        if self.require_stable_interfaces
            && readiness.minimum_stability == StabilityLevel::Experimental
        {
            issues.push(GaIssue {
                severity: IssueSeverity::Error,
                category: IssueCategory::Stability,
                message: "Component uses experimental interfaces not allowed in production"
                    .to_string(),
            });
        }

        // Add profile-specific recommendations
        if self.profile == DeploymentProfile::Production {
            if !self.enable_metrics {
                recommendations.push("Enable metrics for production observability".to_string());
            }
            if interfaces.iter().any(|i| i.contains("http")) {
                recommendations.push(
                    "Configure network timeouts and rate limits for HTTP interfaces".to_string(),
                );
            }
        }

        let is_ga_ready = issues.iter().all(|i| i.severity != IssueSeverity::Error);

        GaReadinessReport {
            is_ga_ready,
            profile: self.profile,
            readiness_assessment: readiness,
            issues,
            recommendations,
        }
    }

    /// Validate a component binary size against limits.
    pub fn validate_component_size(&self, size: usize) -> Result<(), String> {
        if size > self.max_component_size {
            Err(format!(
                "Component size {} bytes exceeds maximum {} bytes for {} profile",
                size, self.max_component_size, self.profile
            ))
        } else {
            Ok(())
        }
    }
}

/// Builder for ProductionConfig.
#[derive(Debug)]
pub struct ProductionConfigBuilder {
    config: ProductionConfig,
}

impl ProductionConfigBuilder {
    fn new() -> Self {
        Self { config: ProductionConfig::default() }
    }

    /// Set the deployment profile (applies profile defaults).
    pub fn profile(mut self, profile: DeploymentProfile) -> Self {
        self.config = ProductionConfig::for_profile(profile);
        self
    }

    /// Set maximum component size.
    pub fn max_component_size(mut self, size: usize) -> Self {
        self.config.max_component_size = size;
        self
    }

    /// Set maximum composition depth.
    pub fn max_composition_depth(mut self, depth: usize) -> Self {
        self.config.max_composition_depth = depth;
        self
    }

    /// Set whether to require stable interfaces.
    pub fn require_stable_interfaces(mut self, require: bool) -> Self {
        self.config.require_stable_interfaces = require;
        self
    }

    /// Set minimum stability level.
    pub fn minimum_stability(mut self, level: StabilityLevel) -> Self {
        self.config.minimum_stability = level;
        self
    }

    /// Set default memory limit.
    pub fn default_memory_limit(mut self, limit: usize) -> Self {
        self.config.default_memory_limit = limit;
        self
    }

    /// Set default timeout.
    pub fn default_timeout(mut self, timeout: Duration) -> Self {
        self.config.default_timeout = timeout;
        self
    }

    /// Set default fuel budget.
    pub fn default_fuel(mut self, fuel: u64) -> Self {
        self.config.default_fuel = fuel;
        self
    }

    /// Enable or disable caching.
    pub fn enable_caching(mut self, enable: bool) -> Self {
        self.config.enable_caching = enable;
        self
    }

    /// Enable or disable metrics.
    pub fn enable_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> ProductionConfig {
        self.config
    }
}

/// GA readiness report for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaReadinessReport {
    /// Whether the component is ready for GA deployment.
    pub is_ga_ready: bool,
    /// The deployment profile used for validation.
    pub profile: DeploymentProfile,
    /// Underlying readiness assessment.
    pub readiness_assessment: ReadinessAssessment,
    /// Issues found during validation.
    pub issues: Vec<GaIssue>,
    /// Recommendations for deployment.
    pub recommendations: Vec<String>,
}

impl GaReadinessReport {
    /// Get a summary string.
    pub fn summary(&self) -> String {
        let status = if self.is_ga_ready { "READY" } else { "NOT READY" };
        let errors = self.issues.iter().filter(|i| i.severity == IssueSeverity::Error).count();
        let warnings = self.issues.iter().filter(|i| i.severity == IssueSeverity::Warning).count();
        format!(
            "GA Status: {} | Profile: {} | Errors: {} | Warnings: {} | Recommendations: {}",
            status,
            self.profile,
            errors,
            warnings,
            self.recommendations.len()
        )
    }
}

/// An issue found during GA readiness validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaIssue {
    /// Issue severity.
    pub severity: IssueSeverity,
    /// Issue category.
    pub category: IssueCategory,
    /// Human-readable message.
    pub message: String,
}

/// Severity of a GA readiness issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    /// Informational only.
    Info,
    /// Potential concern.
    Warning,
    /// Blocks GA readiness.
    Error,
}

/// Category of a GA readiness issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCategory {
    /// Interface stability.
    Stability,
    /// Resource limits.
    Resources,
    /// Security configuration.
    Security,
    /// Performance concerns.
    Performance,
}

/// Feature gate status for WASI Preview 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wasi2FeatureStatus {
    /// Whether WASI Preview 2 is GA.
    pub is_ga: bool,
    /// GA version string.
    pub version: String,
    /// Per-interface GA status.
    pub interface_status: HashMap<String, InterfaceGaStatus>,
}

/// GA status for a specific WASI interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceGaStatus {
    /// Interface name.
    pub name: String,
    /// Stability level.
    pub stability: StabilityLevel,
    /// Whether it's included in GA.
    pub in_ga: bool,
    /// Notes about this interface.
    pub notes: String,
}

impl Wasi2FeatureStatus {
    /// Get the current WASI Preview 2 GA status.
    pub fn current() -> Self {
        let all_interfaces = InterfaceStability::all();
        let mut interface_status = HashMap::new();

        for (name, stability) in &all_interfaces {
            let in_ga = *stability >= StabilityLevel::Preview;
            let notes = match *stability {
                StabilityLevel::Stable => "Fully stable, API frozen".to_string(),
                StabilityLevel::Preview => "API stabilizing, suitable for production".to_string(),
                StabilityLevel::Experimental => "Not included in GA, use with caution".to_string(),
            };
            interface_status.insert(
                name.to_string(),
                InterfaceGaStatus { name: name.to_string(), stability: *stability, in_ga, notes },
            );
        }

        let ga_count = interface_status.values().filter(|s| s.in_ga).count();
        let total = interface_status.len();

        Self {
            is_ga: ga_count as f64 / total as f64 > 0.75,
            version: "0.2.0".to_string(),
            interface_status,
        }
    }

    /// Get the percentage of interfaces that are GA-ready.
    pub fn ga_percentage(&self) -> f64 {
        let ga = self.interface_status.values().filter(|s| s.in_ga).count();
        let total = self.interface_status.len();
        if total == 0 {
            0.0
        } else {
            (ga as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_production_config_profiles() {
        let dev = ProductionConfig::for_profile(DeploymentProfile::Development);
        let staging = ProductionConfig::for_profile(DeploymentProfile::Staging);
        let prod = ProductionConfig::for_profile(DeploymentProfile::Production);

        assert!(dev.max_component_size > staging.max_component_size);
        assert!(staging.max_component_size > prod.max_component_size);
        assert!(!dev.require_stable_interfaces);
        assert!(prod.require_stable_interfaces);
        assert!(prod.enable_metrics);
    }

    #[test]
    fn test_production_config_builder() {
        let config = ProductionConfig::builder()
            .profile(DeploymentProfile::Staging)
            .max_component_size(200 * 1024 * 1024)
            .enable_metrics(true)
            .build();

        assert_eq!(config.profile, DeploymentProfile::Staging);
        assert_eq!(config.max_component_size, 200 * 1024 * 1024);
        assert!(config.enable_metrics);
    }

    #[test]
    fn test_validate_for_deployment_stable() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        let interfaces = vec![
            "wasi:cli/stdout".to_string(),
            "wasi:cli/stderr".to_string(),
            "wasi:filesystem/types".to_string(),
        ];
        let report = config.validate_for_deployment(&interfaces);
        assert!(report.is_ga_ready);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn test_validate_for_deployment_experimental_blocked() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        let interfaces =
            vec!["wasi:cli/stdout".to_string(), "wasi:http/incoming-handler".to_string()];
        let report = config.validate_for_deployment(&interfaces);
        assert!(!report.is_ga_ready);
        assert!(!report.issues.is_empty());
    }

    #[test]
    fn test_validate_for_deployment_dev_allows_experimental() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Development);
        let interfaces =
            vec!["wasi:cli/stdout".to_string(), "wasi:http/incoming-handler".to_string()];
        let report = config.validate_for_deployment(&interfaces);
        assert!(report.is_ga_ready);
    }

    #[test]
    fn test_validate_component_size() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        assert!(config.validate_component_size(10 * 1024 * 1024).is_ok());
        assert!(config.validate_component_size(100 * 1024 * 1024).is_err());
    }

    #[test]
    fn test_ga_readiness_report_summary() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        let interfaces = vec!["wasi:cli/stdout".to_string()];
        let report = config.validate_for_deployment(&interfaces);
        let summary = report.summary();
        assert!(summary.contains("READY"));
        assert!(summary.contains("production"));
    }

    #[test]
    fn test_wasi2_feature_status() {
        let status = Wasi2FeatureStatus::current();
        assert!(status.ga_percentage() > 50.0);
        assert!(!status.version.is_empty());

        let stdout_status = status.interface_status.get("wasi:cli/stdout").unwrap();
        assert!(stdout_status.in_ga);
        assert_eq!(stdout_status.stability, StabilityLevel::Stable);
    }

    #[test]
    fn test_deployment_profile_display() {
        assert_eq!(DeploymentProfile::Development.to_string(), "development");
        assert_eq!(DeploymentProfile::Staging.to_string(), "staging");
        assert_eq!(DeploymentProfile::Production.to_string(), "production");
    }

    #[test]
    fn test_validate_component_size_error_contains_profile() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        let err = config.validate_component_size(100 * 1024 * 1024).unwrap_err();
        assert!(err.contains("production"));
    }

    #[test]
    fn test_validate_for_deployment_metrics_disabled_recommendation() {
        let config = ProductionConfig::builder()
            .profile(DeploymentProfile::Production)
            .enable_metrics(false)
            .build();
        let interfaces = vec!["wasi:cli/stdout".to_string()];
        let report = config.validate_for_deployment(&interfaces);
        assert!(report.recommendations.iter().any(|r| r.contains("metrics")));
    }

    #[test]
    fn test_validate_for_deployment_http_rate_limit_recommendation() {
        let config = ProductionConfig::for_profile(DeploymentProfile::Production);
        let interfaces =
            vec!["wasi:cli/stdout".to_string(), "wasi:http/incoming-handler".to_string()];
        let report = config.validate_for_deployment(&interfaces);
        assert!(report.recommendations.iter().any(|r| r.contains("rate limit")));
    }
}
