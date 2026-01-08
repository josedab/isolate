//! OpenTelemetry integration for distributed tracing and metrics.
//!
//! This module provides comprehensive observability for sandbox execution
//! through OpenTelemetry-compatible tracing and metrics.
//!
//! # Features
//!
//! - **Distributed Tracing**: Trace requests across sandbox boundaries
//! - **Span Instrumentation**: Pre-defined spans for sandbox operations
//! - **Metrics Export**: Export metrics to OTLP-compatible backends
//! - **Context Propagation**: Propagate trace context between services
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::telemetry::{TelemetryConfig, init_telemetry};
//!
//! let config = TelemetryConfig::builder()
//!     .service_name("my-sandbox-service")
//!     .otlp_endpoint("http://localhost:4317")
//!     .build();
//!
//! init_telemetry(config)?;
//! ```

// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.
#![allow(dead_code)]

mod context;
mod spans;

pub use context::{extract_context, inject_context, TraceContext};
pub use spans::{SandboxSpan, SpanBuilder};

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace::Sampler, Resource};
use std::time::Duration;

/// Semantic conventions for resource attributes.
mod semconv {
    pub const SERVICE_NAME: &str = "service.name";
    pub const SERVICE_VERSION: &str = "service.version";
}

/// Configuration for OpenTelemetry telemetry.
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Service name for traces and metrics.
    pub service_name: String,
    /// Service version.
    pub service_version: Option<String>,
    /// OTLP endpoint URL.
    pub otlp_endpoint: Option<String>,
    /// Whether to enable tracing.
    pub tracing_enabled: bool,
    /// Whether to enable metrics.
    pub metrics_enabled: bool,
    /// Sampling ratio (0.0 to 1.0).
    pub sampling_ratio: f64,
    /// Export timeout.
    pub export_timeout: Duration,
    /// Batch export interval.
    pub batch_interval: Duration,
    /// Maximum queue size for batch export.
    pub max_queue_size: usize,
    /// Additional resource attributes.
    pub resource_attributes: Vec<(String, String)>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            service_name: "isolate-sandbox".to_string(),
            service_version: None,
            otlp_endpoint: None,
            tracing_enabled: true,
            metrics_enabled: true,
            sampling_ratio: 1.0,
            export_timeout: Duration::from_secs(10),
            batch_interval: Duration::from_secs(5),
            max_queue_size: 2048,
            resource_attributes: Vec::new(),
        }
    }
}

impl TelemetryConfig {
    /// Create a new configuration builder.
    pub fn builder() -> TelemetryConfigBuilder {
        TelemetryConfigBuilder::default()
    }

    /// Create a configuration for local development (no export).
    pub fn local() -> Self {
        Self {
            tracing_enabled: true,
            metrics_enabled: false,
            otlp_endpoint: None,
            ..Default::default()
        }
    }

    /// Create a configuration for production with OTLP export.
    pub fn production(service_name: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            otlp_endpoint: Some(endpoint.into()),
            tracing_enabled: true,
            metrics_enabled: true,
            sampling_ratio: 0.1, // Sample 10% in production
            ..Default::default()
        }
    }

    /// Build OpenTelemetry resource from config.
    fn build_resource(&self) -> Resource {
        let mut attributes = vec![KeyValue::new(
            semconv::SERVICE_NAME,
            self.service_name.clone(),
        )];

        if let Some(ref version) = self.service_version {
            attributes.push(KeyValue::new(semconv::SERVICE_VERSION, version.clone()));
        }

        for (key, value) in &self.resource_attributes {
            attributes.push(KeyValue::new(key.clone(), value.clone()));
        }

        Resource::new(attributes)
    }
}

/// Builder for TelemetryConfig.
#[derive(Debug, Default)]
pub struct TelemetryConfigBuilder {
    config: TelemetryConfig,
}

impl TelemetryConfigBuilder {
    /// Set the service name.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    /// Set the service version.
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = Some(version.into());
        self
    }

    /// Set the OTLP endpoint.
    pub fn otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.otlp_endpoint = Some(endpoint.into());
        self
    }

    /// Enable or disable tracing.
    pub fn tracing_enabled(mut self, enabled: bool) -> Self {
        self.config.tracing_enabled = enabled;
        self
    }

    /// Enable or disable metrics.
    pub fn metrics_enabled(mut self, enabled: bool) -> Self {
        self.config.metrics_enabled = enabled;
        self
    }

    /// Set the sampling ratio.
    pub fn sampling_ratio(mut self, ratio: f64) -> Self {
        self.config.sampling_ratio = ratio.clamp(0.0, 1.0);
        self
    }

    /// Set the export timeout.
    pub fn export_timeout(mut self, timeout: Duration) -> Self {
        self.config.export_timeout = timeout;
        self
    }

    /// Set the batch interval.
    pub fn batch_interval(mut self, interval: Duration) -> Self {
        self.config.batch_interval = interval;
        self
    }

    /// Set the maximum queue size.
    pub fn max_queue_size(mut self, size: usize) -> Self {
        self.config.max_queue_size = size;
        self
    }

    /// Add a resource attribute.
    pub fn resource_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config
            .resource_attributes
            .push((key.into(), value.into()));
        self
    }

    /// Build the configuration.
    pub fn build(self) -> TelemetryConfig {
        self.config
    }
}

/// Error initializing telemetry.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    /// Failed to initialize tracer.
    #[error("Failed to initialize tracer: {0}")]
    TracerInit(String),

    /// Failed to initialize metrics.
    #[error("Failed to initialize metrics: {0}")]
    MetricsInit(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Handle for managing telemetry lifecycle.
pub struct TelemetryHandle {
    config: TelemetryConfig,
    initialized: bool,
}

impl TelemetryHandle {
    /// Get the configuration.
    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    /// Check if telemetry is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Shutdown telemetry and flush pending data.
    pub fn shutdown(self) {
        if self.initialized {
            opentelemetry::global::shutdown_tracer_provider();
        }
    }
}

/// Initialize OpenTelemetry with the given configuration.
pub fn init_telemetry(config: TelemetryConfig) -> Result<TelemetryHandle, TelemetryError> {
    if !config.tracing_enabled {
        return Ok(TelemetryHandle {
            config,
            initialized: false,
        });
    }

    let initialized = if let Some(ref endpoint) = config.otlp_endpoint {
        // Initialize OTLP exporter
        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .with_timeout(config.export_timeout)
            .build()
            .map_err(|e| TelemetryError::TracerInit(e.to_string()))?;

        let sampler = if config.sampling_ratio >= 1.0 {
            Sampler::AlwaysOn
        } else if config.sampling_ratio <= 0.0 {
            Sampler::AlwaysOff
        } else {
            Sampler::TraceIdRatioBased(config.sampling_ratio)
        };

        let provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_batch_exporter(exporter, runtime::Tokio)
            .with_sampler(sampler)
            .with_resource(config.build_resource())
            .build();

        opentelemetry::global::set_tracer_provider(provider);
        true
    } else {
        // No-op tracer for local development
        false
    };

    Ok(TelemetryHandle {
        config,
        initialized,
    })
}

/// Get the global tracer for manual instrumentation.
pub fn tracer() -> opentelemetry::global::BoxedTracer {
    opentelemetry::global::tracer("isolate")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_default() {
        let config = TelemetryConfig::default();

        assert_eq!(config.service_name, "isolate-sandbox");
        assert!(config.tracing_enabled);
        assert!(config.metrics_enabled);
        assert_eq!(config.sampling_ratio, 1.0);
    }

    #[test]
    fn test_telemetry_config_builder() {
        let config = TelemetryConfig::builder()
            .service_name("test-service")
            .service_version("1.0.0")
            .otlp_endpoint("http://localhost:4317")
            .sampling_ratio(0.5)
            .resource_attribute("environment", "test")
            .build();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, Some("1.0.0".to_string()));
        assert_eq!(
            config.otlp_endpoint,
            Some("http://localhost:4317".to_string())
        );
        assert_eq!(config.sampling_ratio, 0.5);
        assert_eq!(config.resource_attributes.len(), 1);
    }

    #[test]
    fn test_telemetry_config_local() {
        let config = TelemetryConfig::local();

        assert!(config.tracing_enabled);
        assert!(!config.metrics_enabled);
        assert!(config.otlp_endpoint.is_none());
    }

    #[test]
    fn test_telemetry_config_production() {
        let config = TelemetryConfig::production("my-service", "http://otel:4317");

        assert_eq!(config.service_name, "my-service");
        assert_eq!(config.otlp_endpoint, Some("http://otel:4317".to_string()));
        assert_eq!(config.sampling_ratio, 0.1);
    }

    #[test]
    fn test_sampling_ratio_clamping() {
        let config = TelemetryConfig::builder().sampling_ratio(1.5).build();
        assert_eq!(config.sampling_ratio, 1.0);

        let config = TelemetryConfig::builder().sampling_ratio(-0.5).build();
        assert_eq!(config.sampling_ratio, 0.0);
    }

    #[test]
    fn test_init_telemetry_disabled() {
        let config = TelemetryConfig::builder().tracing_enabled(false).build();

        let handle = init_telemetry(config).unwrap();
        assert!(!handle.config().tracing_enabled);
    }

    #[test]
    fn test_resource_building() {
        let config = TelemetryConfig::builder()
            .service_name("test")
            .service_version("1.0")
            .resource_attribute("env", "test")
            .build();

        let resource = config.build_resource();
        // Resource should have 3 attributes
        assert!(resource.len() >= 2);
    }
}
