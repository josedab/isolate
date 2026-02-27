//! Execution metrics instrumentation.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Instrumentation point identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentationPoint {
    SandboxCreate,
    SandboxRun,
    SandboxDestroy,
    CapabilityCheck,
    MemoryAlloc,
    FuelCheckpoint,
    IoOperation,
    FunctionCall,
}

/// Metrics collected from a single sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    pub sandbox_id: String,
    pub module_name: String,
    pub duration_us: u64,
    pub memory_peak_bytes: u64,
    pub fuel_consumed: u64,
    pub io_bytes_read: u64,
    pub io_bytes_written: u64,
    pub exit_code: i32,
}

impl ExecutionMetrics {
    /// Total I/O bytes.
    pub fn total_io(&self) -> u64 {
        self.io_bytes_read + self.io_bytes_written
    }

    /// Duration in milliseconds.
    pub fn duration_ms(&self) -> f64 {
        self.duration_us as f64 / 1000.0
    }

    /// Memory in MB.
    pub fn memory_mb(&self) -> f64 {
        self.memory_peak_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Was the execution successful?
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Collects execution metrics.
#[derive(Clone)]
pub struct MetricsCollector {
    inner: Arc<CollectorInner>,
}

struct CollectorInner {
    metrics: RwLock<Vec<ExecutionMetrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self { inner: Arc::new(CollectorInner { metrics: RwLock::new(Vec::new()) }) }
    }

    /// Record execution metrics.
    pub fn record(&self, metrics: ExecutionMetrics) {
        self.inner.metrics.write().push(metrics);
    }

    /// Get metrics for a specific module.
    pub fn for_module(&self, module_name: &str) -> Vec<ExecutionMetrics> {
        self.inner.metrics.read().iter().filter(|m| m.module_name == module_name).cloned().collect()
    }

    /// Get all collected metrics.
    pub fn all_metrics(&self) -> Vec<ExecutionMetrics> {
        self.inner.metrics.read().clone()
    }

    /// Average duration across all executions.
    pub fn average_duration_us(&self) -> Option<f64> {
        let metrics = self.inner.metrics.read();
        if metrics.is_empty() {
            return None;
        }
        let sum: u64 = metrics.iter().map(|m| m.duration_us).sum();
        Some(sum as f64 / metrics.len() as f64)
    }

    /// Error rate (fraction of non-zero exit codes).
    pub fn error_rate(&self) -> f64 {
        let metrics = self.inner.metrics.read();
        if metrics.is_empty() {
            return 0.0;
        }
        let errors = metrics.iter().filter(|m| m.exit_code != 0).count();
        errors as f64 / metrics.len() as f64
    }

    /// Total executions count.
    pub fn count(&self) -> usize {
        self.inner.metrics.read().len()
    }

    /// Clear all metrics.
    pub fn clear(&self) {
        self.inner.metrics.write().clear();
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metrics(id: &str, module: &str, duration: u64, exit: i32) -> ExecutionMetrics {
        ExecutionMetrics {
            sandbox_id: id.into(),
            module_name: module.into(),
            duration_us: duration,
            memory_peak_bytes: 1024 * 1024,
            fuel_consumed: 50000,
            io_bytes_read: 100,
            io_bytes_written: 200,
            exit_code: exit,
        }
    }

    #[test]
    fn test_record_and_retrieve() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("s1", "app.wasm", 5000, 0));
        c.record(sample_metrics("s2", "app.wasm", 6000, 0));
        assert_eq!(c.count(), 2);
        assert_eq!(c.for_module("app.wasm").len(), 2);
    }

    #[test]
    fn test_average_duration() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("s1", "a", 1000, 0));
        c.record(sample_metrics("s2", "a", 3000, 0));
        assert!((c.average_duration_us().unwrap() - 2000.0).abs() < 0.01);
    }

    #[test]
    fn test_error_rate() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("s1", "a", 1000, 0));
        c.record(sample_metrics("s2", "a", 1000, 1));
        c.record(sample_metrics("s3", "a", 1000, 0));
        assert!((c.error_rate() - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_execution_metrics_helpers() {
        let m = sample_metrics("s1", "a", 5000, 0);
        assert_eq!(m.total_io(), 300);
        assert!((m.duration_ms() - 5.0).abs() < 0.01);
        assert!(m.is_success());
    }

    #[test]
    fn test_empty_collector() {
        let c = MetricsCollector::new();
        assert!(c.average_duration_us().is_none());
        assert_eq!(c.error_rate(), 0.0);
        assert_eq!(c.count(), 0);
    }

    #[test]
    fn test_clear() {
        let c = MetricsCollector::new();
        c.record(sample_metrics("s1", "a", 1000, 0));
        c.clear();
        assert_eq!(c.count(), 0);
    }
}
