//! Benchmark suite and reporting for sandbox execution performance.
//!
//! Self-contained module for measuring sandbox performance and producing
//! structured reports with statistical analysis.
//!
//! # Example
//!
//! ```rust,no_run
//! use isolate_core::benchmark::{BenchmarkSuite, BenchmarkScenario};
//! use isolate_core::sandbox_profile::SandboxProfile;
//!
//! # async fn example() -> isolate_core::Result<()> {
//! let wasm_bytes = std::fs::read("module.wasm")?;
//!
//! let suite = BenchmarkSuite::builder("my benchmarks")
//!     .warmup(3)
//!     .iterations(10)
//!     .scenario(
//!         BenchmarkScenario::new("basic", &wasm_bytes)
//!             .description("Basic execution")
//!             .profile(SandboxProfile::AiCodeExecution),
//!     )
//!     .build();
//!
//! let report = suite.run().await?;
//! println!("{}", report.to_text());
//! # Ok(())
//! # }
//! ```

#![allow(missing_docs)]
use crate::capability::Capability;
use crate::config::SandboxConfig;
use crate::error::Result;
use crate::sandbox::Sandbox;
use crate::sandbox_profile::SandboxProfile;

use std::time::{Duration, Instant};

/// A single benchmark scenario describing what to measure.
pub struct BenchmarkScenario {
    pub name: String,
    pub description: String,
    pub wasm_bytes: Vec<u8>,
    pub stdin: Vec<u8>,
    pub profile: SandboxProfile,
}

impl BenchmarkScenario {
    /// Create a new scenario with the given name and WASM bytes.
    pub fn new(name: impl Into<String>, wasm_bytes: &[u8]) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            wasm_bytes: wasm_bytes.to_vec(),
            stdin: Vec::new(),
            profile: SandboxProfile::AiCodeExecution,
        }
    }

    /// Set the description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// Set the stdin input.
    pub fn stdin(mut self, data: Vec<u8>) -> Self {
        self.stdin = data;
        self
    }

    /// Set the sandbox profile.
    pub fn profile(mut self, profile: SandboxProfile) -> Self {
        self.profile = profile;
        self
    }
}

/// Builder for [`BenchmarkSuite`].
pub struct BenchmarkSuiteBuilder {
    name: String,
    scenarios: Vec<BenchmarkScenario>,
    warmup_iterations: u32,
    measure_iterations: u32,
}

impl BenchmarkSuiteBuilder {
    fn new(name: String) -> Self {
        Self { name, scenarios: Vec::new(), warmup_iterations: 3, measure_iterations: 10 }
    }

    /// Set the number of warmup iterations.
    pub fn warmup(mut self, n: u32) -> Self {
        self.warmup_iterations = n;
        self
    }

    /// Set the number of measurement iterations.
    pub fn iterations(mut self, n: u32) -> Self {
        self.measure_iterations = n;
        self
    }

    /// Add a benchmark scenario.
    pub fn scenario(mut self, s: BenchmarkScenario) -> Self {
        self.scenarios.push(s);
        self
    }

    /// Build the suite.
    pub fn build(self) -> BenchmarkSuite {
        BenchmarkSuite {
            name: self.name,
            scenarios: self.scenarios,
            warmup_iterations: self.warmup_iterations,
            measure_iterations: self.measure_iterations,
        }
    }
}

/// A collection of benchmark scenarios with configurable warmup and measurement.
pub struct BenchmarkSuite {
    name: String,
    scenarios: Vec<BenchmarkScenario>,
    warmup_iterations: u32,
    measure_iterations: u32,
}

impl BenchmarkSuite {
    /// Create a builder for a new suite.
    pub fn builder(name: impl Into<String>) -> BenchmarkSuiteBuilder {
        BenchmarkSuiteBuilder::new(name.into())
    }

    /// Run all scenarios and produce a report.
    pub async fn run(&self) -> Result<BenchmarkReport> {
        let suite_start = Instant::now();
        let mut results = Vec::with_capacity(self.scenarios.len());

        for scenario in &self.scenarios {
            // Warmup
            for _ in 0..self.warmup_iterations {
                let config = Self::build_config(scenario)?;
                let mut sandbox = Sandbox::create(config).await?;
                let _ = sandbox.run(&scenario.stdin).await?;
            }

            // Measure
            let mut times = Vec::with_capacity(self.measure_iterations as usize);
            for _ in 0..self.measure_iterations {
                let config = Self::build_config(scenario)?;
                let iter_start = Instant::now();
                let mut sandbox = Sandbox::create(config).await?;
                let _ = sandbox.run(&scenario.stdin).await?;
                times.push(iter_start.elapsed());
            }

            results.push(Self::compute_result(&scenario.name, &times));
        }

        let total_duration = suite_start.elapsed();
        let timestamp = chrono::Utc::now().to_rfc3339();

        Ok(BenchmarkReport { suite_name: self.name.clone(), results, total_duration, timestamp })
    }

    fn build_config(scenario: &BenchmarkScenario) -> Result<SandboxConfig> {
        SandboxConfig::builder()
            .module(&scenario.wasm_bytes)?
            .use_profile(scenario.profile.clone())
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .build()
    }

    fn compute_result(name: &str, times: &[Duration]) -> BenchmarkResult {
        let iterations = times.len() as u32;
        let total_time: Duration = times.iter().sum();
        let mean_nanos = total_time.as_nanos() as f64 / iterations as f64;
        let mean_time = Duration::from_nanos(mean_nanos as u64);

        let mut sorted: Vec<Duration> = times.to_vec();
        sorted.sort();

        let min_time = sorted.first().copied().unwrap_or_default();
        let max_time = sorted.last().copied().unwrap_or_default();
        let median_time = calculate_percentile(&sorted, 50.0);
        let p95_time = calculate_percentile(&sorted, 95.0);
        let p99_time = calculate_percentile(&sorted, 99.0);
        let std_dev = calculate_std_dev(times, mean_time);

        let throughput =
            if mean_time.as_secs_f64() > 0.0 { 1.0 / mean_time.as_secs_f64() } else { 0.0 };

        BenchmarkResult {
            scenario_name: name.to_string(),
            iterations,
            total_time,
            min_time,
            max_time,
            mean_time,
            median_time,
            p95_time,
            p99_time,
            std_dev,
            throughput,
        }
    }
}

/// Results for a single benchmark scenario.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub scenario_name: String,
    pub iterations: u32,
    pub total_time: Duration,
    pub min_time: Duration,
    pub max_time: Duration,
    pub mean_time: Duration,
    pub median_time: Duration,
    pub p95_time: Duration,
    pub p99_time: Duration,
    pub std_dev: Duration,
    /// Executions per second.
    pub throughput: f64,
}

impl std::fmt::Display for BenchmarkResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: mean={:.3}ms, median={:.3}ms, p95={:.3}ms, stddev={:.3}ms ({:.1} exec/s)",
            self.scenario_name,
            self.mean_time.as_secs_f64() * 1000.0,
            self.median_time.as_secs_f64() * 1000.0,
            self.p95_time.as_secs_f64() * 1000.0,
            self.std_dev.as_secs_f64() * 1000.0,
            self.throughput,
        )
    }
}

/// Full report from running a benchmark suite.
#[derive(Debug, Clone)]
pub struct BenchmarkReport {
    pub suite_name: String,
    pub results: Vec<BenchmarkResult>,
    pub total_duration: Duration,
    /// ISO 8601 timestamp.
    pub timestamp: String,
}

impl BenchmarkReport {
    /// Render a human-readable text report.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Benchmark Suite: {}\nTimestamp: {}\n\n",
            self.suite_name, self.timestamp
        ));

        for r in &self.results {
            out.push_str(&format!(
                "  {}\n    iterations: {}, total: {:.3}ms\n    min: {:.3}ms  max: {:.3}ms  mean: {:.3}ms  median: {:.3}ms\n    p95: {:.3}ms  p99: {:.3}ms  stddev: {:.3}ms\n    throughput: {:.1} exec/s\n\n",
                r.scenario_name,
                r.iterations,
                r.total_time.as_secs_f64() * 1000.0,
                r.min_time.as_secs_f64() * 1000.0,
                r.max_time.as_secs_f64() * 1000.0,
                r.mean_time.as_secs_f64() * 1000.0,
                r.median_time.as_secs_f64() * 1000.0,
                r.p95_time.as_secs_f64() * 1000.0,
                r.p99_time.as_secs_f64() * 1000.0,
                r.std_dev.as_secs_f64() * 1000.0,
                r.throughput,
            ));
        }

        out.push_str(&format!(
            "Total duration: {:.3}ms\n",
            self.total_duration.as_secs_f64() * 1000.0
        ));
        out
    }

    /// Render a JSON report.
    pub fn to_json(&self) -> String {
        let results: Vec<serde_json::Value> = self
            .results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "scenario_name": r.scenario_name,
                    "iterations": r.iterations,
                    "total_time_ms": r.total_time.as_secs_f64() * 1000.0,
                    "min_time_ms": r.min_time.as_secs_f64() * 1000.0,
                    "max_time_ms": r.max_time.as_secs_f64() * 1000.0,
                    "mean_time_ms": r.mean_time.as_secs_f64() * 1000.0,
                    "median_time_ms": r.median_time.as_secs_f64() * 1000.0,
                    "p95_time_ms": r.p95_time.as_secs_f64() * 1000.0,
                    "p99_time_ms": r.p99_time.as_secs_f64() * 1000.0,
                    "std_dev_ms": r.std_dev.as_secs_f64() * 1000.0,
                    "throughput": r.throughput,
                })
            })
            .collect();

        let report = serde_json::json!({
            "suite_name": self.suite_name,
            "timestamp": self.timestamp,
            "total_duration_ms": self.total_duration.as_secs_f64() * 1000.0,
            "results": results,
        });

        serde_json::to_string_pretty(&report).unwrap_or_default()
    }

    /// Return the result with the lowest mean time.
    pub fn fastest(&self) -> Option<&BenchmarkResult> {
        self.results.iter().min_by_key(|r| r.mean_time)
    }

    /// Return the result with the highest mean time.
    pub fn slowest(&self) -> Option<&BenchmarkResult> {
        self.results.iter().max_by_key(|r| r.mean_time)
    }

    /// Render a comparison table of all scenarios.
    pub fn comparison_table(&self) -> String {
        if self.results.is_empty() {
            return String::from("No results to compare.\n");
        }

        let name_width =
            self.results.iter().map(|r| r.scenario_name.len()).max().unwrap_or(8).max(8);

        let mut out = String::new();
        out.push_str(&format!(
            "{:<width$}  {:>10}  {:>10}  {:>10}  {:>10}  {:>12}\n",
            "Scenario",
            "Mean",
            "Median",
            "P95",
            "Std Dev",
            "Throughput",
            width = name_width,
        ));
        out.push_str(&"-".repeat(name_width + 60));
        out.push('\n');

        for r in &self.results {
            out.push_str(&format!(
                "{:<width$}  {:>9.3}ms  {:>9.3}ms  {:>9.3}ms  {:>9.3}ms  {:>9.1} e/s\n",
                r.scenario_name,
                r.mean_time.as_secs_f64() * 1000.0,
                r.median_time.as_secs_f64() * 1000.0,
                r.p95_time.as_secs_f64() * 1000.0,
                r.std_dev.as_secs_f64() * 1000.0,
                r.throughput,
                width = name_width,
            ));
        }

        out
    }
}

/// Calculate a percentile from a sorted slice of durations.
pub fn calculate_percentile(sorted_times: &[Duration], percentile: f64) -> Duration {
    if sorted_times.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((percentile / 100.0) * (sorted_times.len() as f64 - 1.0)).round().max(0.0) as usize;
    sorted_times[idx.min(sorted_times.len() - 1)]
}

/// Calculate the standard deviation of a set of durations.
pub fn calculate_std_dev(times: &[Duration], mean: Duration) -> Duration {
    if times.len() < 2 {
        return Duration::ZERO;
    }
    let mean_nanos = mean.as_nanos() as f64;
    let variance: f64 = times
        .iter()
        .map(|t| {
            let diff = t.as_nanos() as f64 - mean_nanos;
            diff * diff
        })
        .sum::<f64>()
        / (times.len() - 1) as f64;
    Duration::from_nanos(variance.sqrt() as u64)
}

/// Return predefined benchmark scenarios using test fixtures.
pub fn standard_scenarios() -> Vec<BenchmarkScenario> {
    let minimal_wasm = include_bytes!("../tests/fixtures/minimal.wasm");
    let hello_wasm = include_bytes!("../tests/fixtures/hello.wasm");
    let exit_42_wasm = include_bytes!("../tests/fixtures/exit_42.wasm");

    vec![
        BenchmarkScenario::new("cold_start", minimal_wasm)
            .description("Measures minimal.wasm cold start time")
            .profile(SandboxProfile::AiCodeExecution),
        BenchmarkScenario::new("hello_world", hello_wasm)
            .description("Measures hello.wasm with stdout capture")
            .profile(SandboxProfile::AiCodeExecution),
        BenchmarkScenario::new("exit_code", exit_42_wasm)
            .description("Measures exit_42.wasm non-zero exit handling")
            .profile(SandboxProfile::AiCodeExecution),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");
    const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");

    #[test]
    fn test_suite_builder() {
        let suite = BenchmarkSuite::builder("test suite")
            .warmup(5)
            .iterations(20)
            .scenario(BenchmarkScenario::new("s1", MINIMAL_WASM))
            .build();

        assert_eq!(suite.name, "test suite");
        assert_eq!(suite.warmup_iterations, 5);
        assert_eq!(suite.measure_iterations, 20);
        assert_eq!(suite.scenarios.len(), 1);
    }

    #[test]
    fn test_scenario_creation() {
        let scenario = BenchmarkScenario::new("test", MINIMAL_WASM)
            .description("a test")
            .stdin(b"input".to_vec())
            .profile(SandboxProfile::Playground);

        assert_eq!(scenario.name, "test");
        assert_eq!(scenario.description, "a test");
        assert_eq!(scenario.stdin, b"input");
        assert_eq!(scenario.profile, SandboxProfile::Playground);
    }

    #[test]
    fn test_percentile_calculation() {
        let times: Vec<Duration> = (1..=100).map(|i| Duration::from_millis(i)).collect();

        assert_eq!(calculate_percentile(&times, 0.0), Duration::from_millis(1));
        // 50th percentile of 1..=100: index = round(0.5 * 99) = 50 → value 51
        assert_eq!(calculate_percentile(&times, 50.0), Duration::from_millis(51));
        assert_eq!(calculate_percentile(&times, 95.0), Duration::from_millis(95));
        assert_eq!(calculate_percentile(&times, 99.0), Duration::from_millis(99));
        assert_eq!(calculate_percentile(&times, 100.0), Duration::from_millis(100));
    }

    #[test]
    fn test_percentile_empty() {
        assert_eq!(calculate_percentile(&[], 50.0), Duration::ZERO);
    }

    #[test]
    fn test_percentile_single() {
        let times = vec![Duration::from_millis(42)];
        assert_eq!(calculate_percentile(&times, 50.0), Duration::from_millis(42));
    }

    #[test]
    fn test_std_dev_calculation() {
        // All same values → std dev = 0
        let times = vec![Duration::from_millis(10); 5];
        let mean = Duration::from_millis(10);
        assert_eq!(calculate_std_dev(&times, mean), Duration::ZERO);
    }

    #[test]
    fn test_std_dev_varied() {
        let times =
            vec![Duration::from_millis(10), Duration::from_millis(20), Duration::from_millis(30)];
        let mean = Duration::from_millis(20);
        let sd = calculate_std_dev(&times, mean);
        // sample std dev of [10,20,30] = 10ms
        assert!(sd.as_millis() >= 9 && sd.as_millis() <= 11);
    }

    #[test]
    fn test_std_dev_single_element() {
        let times = vec![Duration::from_millis(5)];
        assert_eq!(calculate_std_dev(&times, Duration::from_millis(5)), Duration::ZERO);
    }

    #[test]
    fn test_benchmark_result_display() {
        let result = BenchmarkResult {
            scenario_name: "test".to_string(),
            iterations: 10,
            total_time: Duration::from_millis(100),
            min_time: Duration::from_millis(8),
            max_time: Duration::from_millis(15),
            mean_time: Duration::from_millis(10),
            median_time: Duration::from_millis(10),
            p95_time: Duration::from_millis(14),
            p99_time: Duration::from_millis(15),
            std_dev: Duration::from_millis(2),
            throughput: 100.0,
        };

        let text = format!("{}", result);
        assert!(text.contains("test"));
        assert!(text.contains("mean="));
        assert!(text.contains("exec/s"));
    }

    #[test]
    fn test_report_to_text_non_empty() {
        let report = make_test_report();
        let text = report.to_text();
        assert!(!text.is_empty());
        assert!(text.contains("test suite"));
        assert!(text.contains("scenario_a"));
    }

    #[test]
    fn test_report_to_json_valid() {
        let report = make_test_report();
        let json = report.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["suite_name"], "test suite");
        assert!(parsed["results"].is_array());
    }

    #[test]
    fn test_report_fastest() {
        let report = make_two_result_report();
        let fastest = report.fastest().expect("has results");
        assert_eq!(fastest.scenario_name, "fast");
    }

    #[test]
    fn test_report_slowest() {
        let report = make_two_result_report();
        let slowest = report.slowest().expect("has results");
        assert_eq!(slowest.scenario_name, "slow");
    }

    #[test]
    fn test_report_fastest_empty() {
        let report = BenchmarkReport {
            suite_name: "empty".into(),
            results: vec![],
            total_duration: Duration::ZERO,
            timestamp: "2024-01-01T00:00:00Z".into(),
        };
        assert!(report.fastest().is_none());
    }

    #[test]
    fn test_comparison_table() {
        let report = make_two_result_report();
        let table = report.comparison_table();
        assert!(table.contains("fast"));
        assert!(table.contains("slow"));
        assert!(table.contains("Scenario"));
    }

    #[test]
    fn test_comparison_table_empty() {
        let report = BenchmarkReport {
            suite_name: "empty".into(),
            results: vec![],
            total_duration: Duration::ZERO,
            timestamp: "2024-01-01T00:00:00Z".into(),
        };
        let table = report.comparison_table();
        assert!(table.contains("No results"));
    }

    #[test]
    fn test_standard_scenarios_count() {
        let scenarios = standard_scenarios();
        assert_eq!(scenarios.len(), 3);
        assert_eq!(scenarios[0].name, "cold_start");
        assert_eq!(scenarios[1].name, "hello_world");
        assert_eq!(scenarios[2].name, "exit_code");
    }

    #[tokio::test]
    async fn test_run_simple_suite() {
        let suite = BenchmarkSuite::builder("simple")
            .warmup(0)
            .iterations(2)
            .scenario(
                BenchmarkScenario::new("hello", HELLO_WASM)
                    .description("hello world")
                    .profile(SandboxProfile::AiCodeExecution),
            )
            .build();

        let report = suite.run().await.expect("suite should succeed");
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].scenario_name, "hello");
        assert_eq!(report.results[0].iterations, 2);
        assert!(report.results[0].mean_time > Duration::ZERO);
        assert!(report.results[0].throughput > 0.0);
    }

    #[tokio::test]
    async fn test_warmup_excluded_from_results() {
        let suite = BenchmarkSuite::builder("warmup test")
            .warmup(3)
            .iterations(2)
            .scenario(BenchmarkScenario::new("minimal", MINIMAL_WASM))
            .build();

        let report = suite.run().await.expect("suite should succeed");
        // Only measurement iterations count
        assert_eq!(report.results[0].iterations, 2);
    }

    // --- helpers ---

    fn make_result(name: &str, mean_ms: u64) -> BenchmarkResult {
        BenchmarkResult {
            scenario_name: name.to_string(),
            iterations: 10,
            total_time: Duration::from_millis(mean_ms * 10),
            min_time: Duration::from_millis(mean_ms.saturating_sub(2)),
            max_time: Duration::from_millis(mean_ms + 2),
            mean_time: Duration::from_millis(mean_ms),
            median_time: Duration::from_millis(mean_ms),
            p95_time: Duration::from_millis(mean_ms + 1),
            p99_time: Duration::from_millis(mean_ms + 2),
            std_dev: Duration::from_millis(1),
            throughput: 1000.0 / mean_ms as f64,
        }
    }

    fn make_test_report() -> BenchmarkReport {
        BenchmarkReport {
            suite_name: "test suite".into(),
            results: vec![make_result("scenario_a", 10)],
            total_duration: Duration::from_millis(200),
            timestamp: "2024-01-01T00:00:00Z".into(),
        }
    }

    fn make_two_result_report() -> BenchmarkReport {
        BenchmarkReport {
            suite_name: "comparison".into(),
            results: vec![make_result("fast", 5), make_result("slow", 50)],
            total_duration: Duration::from_millis(550),
            timestamp: "2024-01-01T00:00:00Z".into(),
        }
    }
}
