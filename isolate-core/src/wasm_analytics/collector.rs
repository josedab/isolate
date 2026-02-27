//! Unified metrics collection and live streaming for the observability dashboard.
//!
//! Provides a centralized metrics collector that aggregates data from all
//! sandbox subsystems and supports real-time streaming export.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::wasm_analytics::collector::{MetricsCollector, MetricEvent};
//!
//! let collector = MetricsCollector::new(CollectorConfig::default());
//! collector.record(MetricEvent::sandbox_created("sandbox-1"));
//! collector.record(MetricEvent::execution_complete("sandbox-1", Duration::from_ms(50), 1000));
//!
//! let snapshot = collector.snapshot();
//! let json = collector.export_json();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Configuration for the metrics collector.
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// Maximum metric events to retain in the ring buffer.
    pub max_events: usize,
    /// Aggregation window for rate calculations.
    pub rate_window: Duration,
    /// Enable per-sandbox metrics (increases memory usage).
    pub per_sandbox_metrics: bool,
    /// Export format for streaming.
    pub export_format: ExportFormat,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            max_events: 10_000,
            rate_window: Duration::from_secs(60),
            per_sandbox_metrics: true,
            export_format: ExportFormat::Json,
        }
    }
}

/// Export format for metrics streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// JSON format.
    Json,
    /// Prometheus text exposition format.
    Prometheus,
    /// OpenTelemetry format.
    OpenTelemetry,
}

/// A metric event recorded by the collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEvent {
    /// Event type.
    pub event_type: MetricEventType,
    /// Sandbox ID (if applicable).
    pub sandbox_id: Option<String>,
    /// Timestamp (milliseconds since collector start).
    pub timestamp_ms: u64,
    /// Associated numeric value.
    pub value: f64,
    /// Labels/tags for the event.
    pub labels: HashMap<String, String>,
}

/// Types of metric events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricEventType {
    /// Sandbox was created.
    SandboxCreated,
    /// Sandbox execution completed.
    ExecutionComplete,
    /// Sandbox execution failed.
    ExecutionFailed,
    /// Sandbox terminated.
    SandboxTerminated,
    /// Resource limit hit (memory, fuel, timeout).
    ResourceLimitHit,
    /// Snapshot created.
    SnapshotCreated,
    /// Snapshot restored.
    SnapshotRestored,
    /// Policy evaluation.
    PolicyEvaluated,
    /// Custom metric.
    Custom,
}

impl MetricEvent {
    /// Create a sandbox creation event.
    pub fn sandbox_created(sandbox_id: impl Into<String>) -> Self {
        Self {
            event_type: MetricEventType::SandboxCreated,
            sandbox_id: Some(sandbox_id.into()),
            timestamp_ms: 0, // Set by collector
            value: 1.0,
            labels: HashMap::new(),
        }
    }

    /// Create an execution complete event.
    pub fn execution_complete(
        sandbox_id: impl Into<String>,
        duration: Duration,
        fuel_consumed: u64,
    ) -> Self {
        let mut labels = HashMap::new();
        labels.insert("fuel".to_string(), fuel_consumed.to_string());
        Self {
            event_type: MetricEventType::ExecutionComplete,
            sandbox_id: Some(sandbox_id.into()),
            timestamp_ms: 0,
            value: duration.as_secs_f64(),
            labels,
        }
    }

    /// Create an execution failed event.
    pub fn execution_failed(sandbox_id: impl Into<String>, error: impl Into<String>) -> Self {
        let mut labels = HashMap::new();
        labels.insert("error".to_string(), error.into());
        Self {
            event_type: MetricEventType::ExecutionFailed,
            sandbox_id: Some(sandbox_id.into()),
            timestamp_ms: 0,
            value: 1.0,
            labels,
        }
    }

    /// Create a resource limit hit event.
    pub fn resource_limit(sandbox_id: impl Into<String>, limit_type: impl Into<String>) -> Self {
        let mut labels = HashMap::new();
        labels.insert("limit_type".to_string(), limit_type.into());
        Self {
            event_type: MetricEventType::ResourceLimitHit,
            sandbox_id: Some(sandbox_id.into()),
            timestamp_ms: 0,
            value: 1.0,
            labels,
        }
    }

    /// Create a custom metric event.
    pub fn custom(name: impl Into<String>, value: f64) -> Self {
        let mut labels = HashMap::new();
        labels.insert("name".to_string(), name.into());
        Self {
            event_type: MetricEventType::Custom,
            sandbox_id: None,
            timestamp_ms: 0,
            value,
            labels,
        }
    }
}

/// Aggregated metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Total sandboxes created.
    pub total_created: u64,
    /// Total executions completed.
    pub total_completed: u64,
    /// Total executions failed.
    pub total_failed: u64,
    /// Total resource limit hits.
    pub total_resource_limits: u64,
    /// Current creation rate (per second).
    pub creation_rate: f64,
    /// Current completion rate (per second).
    pub completion_rate: f64,
    /// Current failure rate (per second).
    pub failure_rate: f64,
    /// Average execution duration (seconds).
    pub avg_execution_duration_s: f64,
    /// P99 execution duration (seconds).
    pub p99_execution_duration_s: f64,
    /// Total fuel consumed.
    pub total_fuel_consumed: u64,
    /// Events in the buffer.
    pub buffered_events: usize,
    /// Uptime in seconds.
    pub uptime_seconds: f64,
}

/// Centralized metrics collector.
pub struct MetricsCollector {
    config: CollectorConfig,
    /// Ring buffer of recent events.
    events: parking_lot::Mutex<Vec<MetricEvent>>,
    /// Global counters.
    total_created: AtomicU64,
    total_completed: AtomicU64,
    total_failed: AtomicU64,
    total_resource_limits: AtomicU64,
    total_fuel: AtomicU64,
    /// Execution durations for percentile calculation.
    durations: parking_lot::Mutex<Vec<f64>>,
    /// Collector start time.
    started_at: Instant,
}

impl MetricsCollector {
    /// Create a new metrics collector.
    pub fn new(config: CollectorConfig) -> Self {
        Self {
            config,
            events: parking_lot::Mutex::new(Vec::new()),
            total_created: AtomicU64::new(0),
            total_completed: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            total_resource_limits: AtomicU64::new(0),
            total_fuel: AtomicU64::new(0),
            durations: parking_lot::Mutex::new(Vec::new()),
            started_at: Instant::now(),
        }
    }

    /// Record a metric event.
    pub fn record(&self, mut event: MetricEvent) {
        event.timestamp_ms = self.started_at.elapsed().as_millis() as u64;

        // Update counters
        match event.event_type {
            MetricEventType::SandboxCreated => {
                self.total_created.fetch_add(1, Ordering::Relaxed);
            }
            MetricEventType::ExecutionComplete => {
                self.total_completed.fetch_add(1, Ordering::Relaxed);
                self.durations.lock().push(event.value);
                if let Some(fuel_str) = event.labels.get("fuel") {
                    if let Ok(fuel) = fuel_str.parse::<u64>() {
                        self.total_fuel.fetch_add(fuel, Ordering::Relaxed);
                    }
                }
            }
            MetricEventType::ExecutionFailed => {
                self.total_failed.fetch_add(1, Ordering::Relaxed);
            }
            MetricEventType::ResourceLimitHit => {
                self.total_resource_limits.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        // Add to ring buffer
        let mut events = self.events.lock();
        events.push(event);
        if events.len() > self.config.max_events {
            let excess = events.len() - self.config.max_events;
            events.drain(..excess);
        }
    }

    /// Get a snapshot of current metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let uptime = self.started_at.elapsed().as_secs_f64();
        let total_created = self.total_created.load(Ordering::Relaxed);
        let total_completed = self.total_completed.load(Ordering::Relaxed);
        let total_failed = self.total_failed.load(Ordering::Relaxed);

        let durations = self.durations.lock();
        let avg_duration = if durations.is_empty() {
            0.0
        } else {
            durations.iter().sum::<f64>() / durations.len() as f64
        };
        let p99_duration = percentile(&durations, 0.99);

        let creation_rate = if uptime > 0.0 { total_created as f64 / uptime } else { 0.0 };
        let completion_rate = if uptime > 0.0 { total_completed as f64 / uptime } else { 0.0 };
        let failure_rate = if uptime > 0.0 { total_failed as f64 / uptime } else { 0.0 };

        MetricsSnapshot {
            total_created,
            total_completed,
            total_failed,
            total_resource_limits: self.total_resource_limits.load(Ordering::Relaxed),
            creation_rate,
            completion_rate,
            failure_rate,
            avg_execution_duration_s: avg_duration,
            p99_execution_duration_s: p99_duration,
            total_fuel_consumed: self.total_fuel.load(Ordering::Relaxed),
            buffered_events: self.events.lock().len(),
            uptime_seconds: uptime,
        }
    }

    /// Export metrics as JSON.
    pub fn export_json(&self) -> String {
        let snapshot = self.snapshot();
        serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".to_string())
    }

    /// Export metrics in Prometheus text exposition format.
    pub fn export_prometheus(&self) -> String {
        let s = self.snapshot();
        let mut lines = Vec::new();

        lines.push("# HELP isolate_sandboxes_created_total Total sandboxes created".to_string());
        lines.push("# TYPE isolate_sandboxes_created_total counter".to_string());
        lines.push(format!("isolate_sandboxes_created_total {}", s.total_created));

        lines.push(
            "# HELP isolate_executions_completed_total Total executions completed".to_string(),
        );
        lines.push("# TYPE isolate_executions_completed_total counter".to_string());
        lines.push(format!("isolate_executions_completed_total {}", s.total_completed));

        lines.push("# HELP isolate_executions_failed_total Total executions failed".to_string());
        lines.push("# TYPE isolate_executions_failed_total counter".to_string());
        lines.push(format!("isolate_executions_failed_total {}", s.total_failed));

        lines.push(
            "# HELP isolate_execution_duration_seconds Average execution duration".to_string(),
        );
        lines.push("# TYPE isolate_execution_duration_seconds gauge".to_string());
        lines.push(format!(
            "isolate_execution_duration_seconds{{quantile=\"avg\"}} {:.6}",
            s.avg_execution_duration_s
        ));
        lines.push(format!(
            "isolate_execution_duration_seconds{{quantile=\"0.99\"}} {:.6}",
            s.p99_execution_duration_s
        ));

        lines.push("# HELP isolate_fuel_consumed_total Total fuel consumed".to_string());
        lines.push("# TYPE isolate_fuel_consumed_total counter".to_string());
        lines.push(format!("isolate_fuel_consumed_total {}", s.total_fuel_consumed));

        lines.push("# HELP isolate_resource_limits_total Total resource limit hits".to_string());
        lines.push("# TYPE isolate_resource_limits_total counter".to_string());
        lines.push(format!("isolate_resource_limits_total {}", s.total_resource_limits));

        lines.join("\n")
    }

    /// Get recent events (last N).
    pub fn recent_events(&self, count: usize) -> Vec<MetricEvent> {
        let events = self.events.lock();
        events.iter().rev().take(count).cloned().collect()
    }

    /// Get events filtered by type.
    pub fn events_by_type(&self, event_type: MetricEventType) -> Vec<MetricEvent> {
        let events = self.events.lock();
        events.iter().filter(|e| e.event_type == event_type).cloned().collect()
    }

    /// Clear all collected metrics.
    pub fn clear(&self) {
        self.events.lock().clear();
        self.durations.lock().clear();
        self.total_created.store(0, Ordering::Relaxed);
        self.total_completed.store(0, Ordering::Relaxed);
        self.total_failed.store(0, Ordering::Relaxed);
        self.total_resource_limits.store(0, Ordering::Relaxed);
        self.total_fuel.store(0, Ordering::Relaxed);
    }
}

/// Calculate a percentile from a sorted slice.
fn percentile(data: &[f64], p: f64) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted = data.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() as f64) * p).ceil() as usize;
    let index = index.clamp(1, sorted.len()) - 1;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_creation() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        let snapshot = collector.snapshot();
        assert_eq!(snapshot.total_created, 0);
        assert_eq!(snapshot.total_completed, 0);
    }

    #[test]
    fn test_record_events() {
        let collector = MetricsCollector::new(CollectorConfig::default());

        collector.record(MetricEvent::sandbox_created("sb-1"));
        collector.record(MetricEvent::sandbox_created("sb-2"));
        collector.record(MetricEvent::execution_complete("sb-1", Duration::from_millis(50), 1000));
        collector.record(MetricEvent::execution_failed("sb-2", "timeout"));

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.total_created, 2);
        assert_eq!(snapshot.total_completed, 1);
        assert_eq!(snapshot.total_failed, 1);
        assert_eq!(snapshot.total_fuel_consumed, 1000);
        assert!(snapshot.avg_execution_duration_s > 0.0);
    }

    #[test]
    fn test_resource_limit_tracking() {
        let collector = MetricsCollector::new(CollectorConfig::default());

        collector.record(MetricEvent::resource_limit("sb-1", "memory"));
        collector.record(MetricEvent::resource_limit("sb-2", "fuel"));

        let snapshot = collector.snapshot();
        assert_eq!(snapshot.total_resource_limits, 2);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let config = CollectorConfig { max_events: 5, ..Default::default() };
        let collector = MetricsCollector::new(config);

        for i in 0..10 {
            collector.record(MetricEvent::sandbox_created(format!("sb-{}", i)));
        }

        assert_eq!(collector.recent_events(100).len(), 5);
    }

    #[test]
    fn test_export_json() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        collector.record(MetricEvent::sandbox_created("sb-1"));

        let json = collector.export_json();
        assert!(json.contains("total_created"));
    }

    #[test]
    fn test_export_prometheus() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        collector.record(MetricEvent::sandbox_created("sb-1"));
        collector.record(MetricEvent::execution_complete("sb-1", Duration::from_millis(100), 5000));

        let prom = collector.export_prometheus();
        assert!(prom.contains("isolate_sandboxes_created_total 1"));
        assert!(prom.contains("isolate_fuel_consumed_total 5000"));
    }

    #[test]
    fn test_events_by_type() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        collector.record(MetricEvent::sandbox_created("sb-1"));
        collector.record(MetricEvent::execution_complete("sb-1", Duration::from_millis(10), 100));
        collector.record(MetricEvent::sandbox_created("sb-2"));

        let created = collector.events_by_type(MetricEventType::SandboxCreated);
        assert_eq!(created.len(), 2);
    }

    #[test]
    fn test_custom_metric() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        collector.record(MetricEvent::custom("queue_depth", 42.0));

        let events = collector.recent_events(1);
        assert_eq!(events[0].event_type, MetricEventType::Custom);
        assert!((events[0].value - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_percentile_calculation() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert!((percentile(&data, 0.5) - 5.0).abs() < f64::EPSILON);
        assert!((percentile(&data, 0.99) - 10.0).abs() < f64::EPSILON);
        assert!((percentile(&[], 0.99)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_percentile_single_element() {
        let data = vec![42.0];
        assert!((percentile(&data, 0.5) - 42.0).abs() < f64::EPSILON);
        assert!((percentile(&data, 0.99) - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_percentile_p50() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&data, 0.5) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clear_metrics() {
        let collector = MetricsCollector::new(CollectorConfig::default());
        collector.record(MetricEvent::sandbox_created("sb-1"));
        assert_eq!(collector.snapshot().total_created, 1);

        collector.clear();
        assert_eq!(collector.snapshot().total_created, 0);
        assert_eq!(collector.recent_events(100).len(), 0);
    }
}
