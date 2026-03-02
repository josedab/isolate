//! Metrics and observability.
//!
//! # Cardinality Warning
//!
//! Several metrics in this module use `sandbox_id` as a label. In high-throughput
//! deployments where thousands of sandboxes are created, this produces **unbounded
//! label cardinality** — each unique sandbox_id creates a new time series that is
//! never garbage-collected by the Prometheus registry.
//!
//! **Mitigation strategies:**
//! - Use `module_hash` instead of `sandbox_id` for aggregation (finite set)
//! - Configure Prometheus `metric_relabel_configs` to drop `sandbox_id` labels
//! - Use recording rules to pre-aggregate per-module-hash metrics
//! - Set sandbox pool size limits to bound the maximum cardinality
//!
//! Affected metrics: `memory_usage`, `fuel_consumed`, `sandbox_run_duration`.

use crate::sandbox::SandboxId;
use parking_lot::RwLock;
use prometheus::{
    Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts, Registry,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Global metrics registry.
static REGISTRY: std::sync::OnceLock<MetricsRegistry> = std::sync::OnceLock::new();

/// Get or initialize the global metrics registry.
pub fn global_registry() -> &'static MetricsRegistry {
    REGISTRY.get_or_init(MetricsRegistry::new)
}

/// Metrics registry for Isolate.
pub struct MetricsRegistry {
    registry: Registry,

    // Sandbox metrics
    sandboxes_created: Counter,
    sandboxes_active: Gauge,
    sandbox_runs: CounterVec,
    sandbox_run_duration: HistogramVec,
    sandbox_cold_start: Histogram,

    // Resource metrics
    memory_usage: GaugeVec,
    fuel_consumed: CounterVec,

    // Capability metrics
    capability_checks: CounterVec,
    capability_denials: CounterVec,

    // Module metrics
    module_compilations: Counter,
    module_compile_duration: Histogram,
    module_runs: CounterVec,

    // Pool metrics
    pool_size: Gauge,
    pool_hits: Counter,
    pool_misses: Counter,

    // Rate limiter metrics
    rate_limit_decisions: CounterVec,
}

impl MetricsRegistry {
    /// Create a new metrics registry.
    pub fn new() -> Self {
        let registry = Registry::new();

        // Sandbox counters
        let sandboxes_created =
            Counter::new("isolate_sandboxes_created_total", "Total number of sandboxes created")
                .unwrap();

        let sandboxes_active =
            Gauge::new("isolate_sandboxes_active", "Number of currently active sandboxes").unwrap();

        let sandbox_runs = CounterVec::new(
            Opts::new("isolate_sandbox_runs_total", "Total number of sandbox runs"),
            &["status"],
        )
        .unwrap();

        let sandbox_run_duration = HistogramVec::new(
            HistogramOpts::new(
                "isolate_sandbox_run_duration_seconds",
                "Sandbox run duration in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
            &["status"],
        )
        .unwrap();

        let sandbox_cold_start = Histogram::with_opts(
            HistogramOpts::new(
                "isolate_sandbox_cold_start_seconds",
                "Sandbox cold start time in seconds",
            )
            .buckets(vec![0.0001, 0.0005, 0.001, 0.002, 0.003, 0.005, 0.01, 0.025, 0.05, 0.1]),
        )
        .unwrap();

        // Resource gauges
        let memory_usage = GaugeVec::new(
            Opts::new("isolate_memory_bytes", "Memory usage in bytes"),
            &["sandbox_id", "type"],
        )
        .unwrap();

        let fuel_consumed = CounterVec::new(
            Opts::new("isolate_fuel_consumed_total", "Total fuel consumed"),
            &["sandbox_id"],
        )
        .unwrap();

        // Capability counters
        let capability_checks = CounterVec::new(
            Opts::new("isolate_capability_checks_total", "Total capability checks"),
            &["capability", "result"],
        )
        .unwrap();

        let capability_denials = CounterVec::new(
            Opts::new("isolate_capability_denials_total", "Total capability denials"),
            &["capability"],
        )
        .unwrap();

        // Register all metrics
        registry.register(Box::new(sandboxes_created.clone())).ok();
        registry.register(Box::new(sandboxes_active.clone())).ok();
        registry.register(Box::new(sandbox_runs.clone())).ok();
        registry.register(Box::new(sandbox_run_duration.clone())).ok();
        registry.register(Box::new(sandbox_cold_start.clone())).ok();
        registry.register(Box::new(memory_usage.clone())).ok();
        registry.register(Box::new(fuel_consumed.clone())).ok();
        registry.register(Box::new(capability_checks.clone())).ok();
        registry.register(Box::new(capability_denials.clone())).ok();

        // Module metrics
        let module_compilations =
            Counter::new("isolate_module_compilations_total", "Total WASM module compilations")
                .unwrap();

        let module_compile_duration = Histogram::with_opts(
            HistogramOpts::new(
                "isolate_module_compile_seconds",
                "WASM module compilation time in seconds",
            )
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5]),
        )
        .unwrap();

        let module_runs = CounterVec::new(
            Opts::new("isolate_module_runs_total", "Runs per module hash"),
            &["module_hash"],
        )
        .unwrap();

        // Pool metrics
        let pool_size = Gauge::new("isolate_pool_size", "Current warm pool size").unwrap();
        let pool_hits = Counter::new("isolate_pool_hits_total", "Warm pool hits").unwrap();
        let pool_misses = Counter::new("isolate_pool_misses_total", "Warm pool misses").unwrap();

        // Rate limiter metrics
        let rate_limit_decisions = CounterVec::new(
            Opts::new("isolate_rate_limit_total", "Rate limit decisions"),
            &["result"],
        )
        .unwrap();

        registry.register(Box::new(module_compilations.clone())).ok();
        registry.register(Box::new(module_compile_duration.clone())).ok();
        registry.register(Box::new(module_runs.clone())).ok();
        registry.register(Box::new(pool_size.clone())).ok();
        registry.register(Box::new(pool_hits.clone())).ok();
        registry.register(Box::new(pool_misses.clone())).ok();
        registry.register(Box::new(rate_limit_decisions.clone())).ok();

        Self {
            registry,
            sandboxes_created,
            sandboxes_active,
            sandbox_runs,
            sandbox_run_duration,
            sandbox_cold_start,
            memory_usage,
            fuel_consumed,
            capability_checks,
            capability_denials,
            module_compilations,
            module_compile_duration,
            module_runs,
            pool_size,
            pool_hits,
            pool_misses,
            rate_limit_decisions,
        }
    }

    /// Record sandbox creation.
    pub fn record_sandbox_created(&self, cold_start: Duration) {
        self.sandboxes_created.inc();
        self.sandboxes_active.inc();
        self.sandbox_cold_start.observe(cold_start.as_secs_f64());
    }

    /// Record sandbox termination.
    pub fn record_sandbox_terminated(&self) {
        self.sandboxes_active.dec();
    }

    /// Record sandbox run.
    pub fn record_sandbox_run(&self, duration: Duration, success: bool) {
        let status = if success { "success" } else { "failure" };
        self.sandbox_runs.with_label_values(&[status]).inc();
        self.sandbox_run_duration.with_label_values(&[status]).observe(duration.as_secs_f64());
    }

    /// Record memory usage.
    pub fn record_memory_usage(&self, sandbox_id: &str, current: usize, peak: usize) {
        self.memory_usage.with_label_values(&[sandbox_id, "current"]).set(current as f64);
        self.memory_usage.with_label_values(&[sandbox_id, "peak"]).set(peak as f64);
    }

    /// Record fuel consumption.
    pub fn record_fuel(&self, sandbox_id: &str, fuel: u64) {
        self.fuel_consumed.with_label_values(&[sandbox_id]).inc_by(fuel as f64);
    }

    /// Record capability check.
    pub fn record_capability_check(&self, capability: &str, allowed: bool) {
        let result = if allowed { "allowed" } else { "denied" };
        self.capability_checks.with_label_values(&[capability, result]).inc();

        if !allowed {
            self.capability_denials.with_label_values(&[capability]).inc();
        }
    }

    /// Record a module compilation.
    pub fn record_module_compilation(&self, duration: Duration) {
        self.module_compilations.inc();
        self.module_compile_duration.observe(duration.as_secs_f64());
    }

    /// Record a module run.
    pub fn record_module_run(&self, module_hash: &str) {
        self.module_runs.with_label_values(&[module_hash]).inc();
    }

    /// Record pool hit.
    pub fn record_pool_hit(&self) {
        self.pool_hits.inc();
    }

    /// Record pool miss.
    pub fn record_pool_miss(&self) {
        self.pool_misses.inc();
    }

    /// Set the pool size.
    pub fn set_pool_size(&self, size: usize) {
        self.pool_size.set(size as f64);
    }

    /// Record a rate limit decision.
    pub fn record_rate_limit(&self, allowed: bool) {
        let result = if allowed { "allowed" } else { "denied" };
        self.rate_limit_decisions.with_label_values(&[result]).inc();
    }

    /// Get the Prometheus registry.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Gather all metrics as text.
    pub fn gather_text(&self) -> String {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metrics = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metrics, &mut buffer).ok();
        String::from_utf8_lossy(&buffer).into_owned()
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-sandbox metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxMetrics {
    /// Sandbox ID.
    pub sandbox_id: SandboxId,
    /// Creation time.
    #[serde(skip)]
    created_at: Option<Instant>,
    /// Number of runs.
    pub run_count: u64,
    /// Number of successful runs.
    pub success_count: u64,
    /// Number of failed runs.
    pub failure_count: u64,
    /// Total run duration.
    pub total_run_duration: Duration,
    /// Last run duration.
    pub last_run_duration: Option<Duration>,
    /// Last run start time.
    #[serde(skip)]
    last_run_start: Option<Instant>,
}

impl SandboxMetrics {
    /// Create new metrics for a sandbox.
    pub fn new(sandbox_id: SandboxId) -> Self {
        Self {
            sandbox_id,
            created_at: Some(Instant::now()),
            run_count: 0,
            success_count: 0,
            failure_count: 0,
            total_run_duration: Duration::ZERO,
            last_run_duration: None,
            last_run_start: None,
        }
    }

    /// Record the start of a run.
    pub fn record_run_start(&mut self) {
        self.last_run_start = Some(Instant::now());
    }

    /// Record the completion of a run.
    pub fn record_run_complete(&mut self, duration: Duration, success: bool) {
        self.run_count += 1;
        self.total_run_duration += duration;
        self.last_run_duration = Some(duration);
        self.last_run_start = None;

        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }

        // Update global metrics
        global_registry().record_sandbox_run(duration, success);
    }

    /// Get the sandbox age.
    pub fn age(&self) -> Duration {
        self.created_at.map(|t| t.elapsed()).unwrap_or(Duration::ZERO)
    }

    /// Get the average run duration.
    pub fn average_run_duration(&self) -> Option<Duration> {
        if self.run_count > 0 {
            Some(self.total_run_duration / self.run_count as u32)
        } else {
            None
        }
    }

    /// Get the success rate.
    pub fn success_rate(&self) -> f64 {
        if self.run_count > 0 {
            self.success_count as f64 / self.run_count as f64
        } else {
            0.0
        }
    }
}

/// Statistics collector for tracking timing distributions.
#[derive(Debug, Clone)]
pub struct TimingStats {
    inner: Arc<RwLock<TimingStatsInner>>,
}

#[derive(Debug, Default)]
struct TimingStatsInner {
    count: u64,
    sum: Duration,
    min: Option<Duration>,
    max: Option<Duration>,
    samples: Vec<Duration>,
}

impl TimingStats {
    /// Create a new timing stats collector.
    pub fn new() -> Self {
        Self { inner: Arc::new(RwLock::new(TimingStatsInner::default())) }
    }

    /// Record a timing sample.
    pub fn record(&self, duration: Duration) {
        let mut inner = self.inner.write();
        inner.count += 1;
        inner.sum += duration;
        inner.min = Some(inner.min.map_or(duration, |m| m.min(duration)));
        inner.max = Some(inner.max.map_or(duration, |m| m.max(duration)));

        // Keep last 1000 samples for percentile calculation
        if inner.samples.len() < 1000 {
            inner.samples.push(duration);
        } else {
            let idx = inner.count as usize % 1000;
            inner.samples[idx] = duration;
        }
    }

    /// Get the count of samples.
    pub fn count(&self) -> u64 {
        self.inner.read().count
    }

    /// Get the average duration.
    pub fn average(&self) -> Option<Duration> {
        let inner = self.inner.read();
        if inner.count > 0 {
            Some(inner.sum / inner.count as u32)
        } else {
            None
        }
    }

    /// Get the minimum duration.
    pub fn min(&self) -> Option<Duration> {
        self.inner.read().min
    }

    /// Get the maximum duration.
    pub fn max(&self) -> Option<Duration> {
        self.inner.read().max
    }

    /// Get a percentile (0-100).
    pub fn percentile(&self, p: f64) -> Option<Duration> {
        let inner = self.inner.read();
        if inner.samples.is_empty() {
            return None;
        }

        let mut sorted = inner.samples.clone();
        sorted.sort();

        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        Some(sorted[idx.min(sorted.len() - 1)])
    }
}

impl Default for TimingStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_metrics() {
        let id = SandboxId::new();
        let mut metrics = SandboxMetrics::new(id);

        metrics.record_run_start();
        metrics.record_run_complete(Duration::from_millis(100), true);

        assert_eq!(metrics.run_count, 1);
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.failure_count, 0);
        assert_eq!(metrics.success_rate(), 1.0);
        assert_eq!(metrics.average_run_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn test_timing_stats() {
        let stats = TimingStats::new();

        stats.record(Duration::from_millis(10));
        stats.record(Duration::from_millis(20));
        stats.record(Duration::from_millis(30));

        assert_eq!(stats.count(), 3);
        assert_eq!(stats.min(), Some(Duration::from_millis(10)));
        assert_eq!(stats.max(), Some(Duration::from_millis(30)));
        assert_eq!(stats.average(), Some(Duration::from_millis(20)));
    }

    #[test]
    fn test_metrics_registry() {
        let registry = MetricsRegistry::new();

        registry.record_sandbox_created(Duration::from_millis(5));
        registry.record_sandbox_run(Duration::from_millis(100), true);
        registry.record_capability_check("fs:read", true);
        registry.record_capability_check("net:http", false);

        let text = registry.gather_text();
        assert!(text.contains("isolate_sandboxes_created_total"));
        assert!(text.contains("isolate_sandbox_runs_total"));
    }

    #[test]
    fn test_sandbox_metrics_mixed_outcomes() {
        let id = SandboxId::new();
        let mut metrics = SandboxMetrics::new(id);

        metrics.record_run_complete(Duration::from_millis(50), true);
        metrics.record_run_complete(Duration::from_millis(150), false);
        metrics.record_run_complete(Duration::from_millis(100), true);

        assert_eq!(metrics.run_count, 3);
        assert_eq!(metrics.success_count, 2);
        assert_eq!(metrics.failure_count, 1);
        assert_eq!(metrics.last_run_duration, Some(Duration::from_millis(100)));
        assert_eq!(metrics.total_run_duration, Duration::from_millis(300));
    }

    #[test]
    fn test_sandbox_metrics_zero_runs() {
        let id = SandboxId::new();
        let metrics = SandboxMetrics::new(id);

        assert_eq!(metrics.run_count, 0);
        assert_eq!(metrics.success_rate(), 0.0);
        assert_eq!(metrics.average_run_duration(), None);
        assert!(metrics.last_run_duration.is_none());
    }

    #[test]
    fn test_timing_stats_empty() {
        let stats = TimingStats::new();

        assert_eq!(stats.count(), 0);
        assert_eq!(stats.min(), None);
        assert_eq!(stats.max(), None);
        assert_eq!(stats.average(), None);
        assert_eq!(stats.percentile(50.0), None);
    }

    #[test]
    fn test_timing_stats_percentile() {
        let stats = TimingStats::new();

        for i in 1..=100 {
            stats.record(Duration::from_millis(i));
        }

        let p50 = stats.percentile(50.0).unwrap();
        assert!(p50 >= Duration::from_millis(45) && p50 <= Duration::from_millis(55));

        let p99 = stats.percentile(99.0).unwrap();
        assert!(p99 >= Duration::from_millis(95));
    }

    #[test]
    fn test_timing_stats_single_sample() {
        let stats = TimingStats::new();
        stats.record(Duration::from_millis(42));

        assert_eq!(stats.count(), 1);
        assert_eq!(stats.min(), Some(Duration::from_millis(42)));
        assert_eq!(stats.max(), Some(Duration::from_millis(42)));
        assert_eq!(stats.average(), Some(Duration::from_millis(42)));
        assert_eq!(stats.percentile(0.0), Some(Duration::from_millis(42)));
        assert_eq!(stats.percentile(100.0), Some(Duration::from_millis(42)));
    }

    #[test]
    fn test_metrics_capability_denial_tracking() {
        let registry = MetricsRegistry::new();

        registry.record_capability_check("fs:write", false);
        registry.record_capability_check("fs:write", false);
        registry.record_capability_check("fs:read", true);

        let text = registry.gather_text();
        assert!(text.contains("isolate_capability_denials_total"));
    }
}
