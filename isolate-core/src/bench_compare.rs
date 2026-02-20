
//! Comparative benchmark suite with CI regression detection.
//!
//! Measures Isolate's performance against baseline targets (Firecracker, gVisor,
//! Wasmer, native processes) and produces CI-friendly reports in Markdown and JSON.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Baseline performance targets for a comparison runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonTarget {
    /// Name of the comparison target (e.g. "Firecracker").
    pub name: String,
    /// Description of the target runtime.
    pub description: String,
    /// Expected cold start latency.
    pub cold_start_target: Duration,
    /// Expected per-execution overhead.
    pub execution_overhead_target: Duration,
    /// Expected memory overhead in bytes.
    pub memory_overhead_bytes: usize,
    /// Expected throughput in requests per second.
    pub throughput_rps_target: f64,
}

/// Pre-defined baselines for comparison.
pub struct ComparisonBaseline;

impl ComparisonBaseline {
    /// Firecracker microVM baseline (~125ms cold start, ~30MB memory).
    pub fn firecracker() -> ComparisonTarget {
        ComparisonTarget {
            name: "Firecracker".into(),
            description: "Firecracker microVM (~125ms cold start, ~30MB memory)".into(),
            cold_start_target: Duration::from_millis(125),
            execution_overhead_target: Duration::from_millis(2),
            memory_overhead_bytes: 30 * 1024 * 1024,
            throughput_rps_target: 8.0,
        }
    }

    /// gVisor baseline (~50ms cold start, ~15MB memory).
    pub fn gvisor() -> ComparisonTarget {
        ComparisonTarget {
            name: "gVisor".into(),
            description: "gVisor container sandbox (~50ms cold start, ~15MB memory)".into(),
            cold_start_target: Duration::from_millis(50),
            execution_overhead_target: Duration::from_millis(1),
            memory_overhead_bytes: 15 * 1024 * 1024,
            throughput_rps_target: 20.0,
        }
    }

    /// Wasmer baseline (~10ms cold start, ~5MB memory).
    pub fn wasmer() -> ComparisonTarget {
        ComparisonTarget {
            name: "Wasmer".into(),
            description: "Wasmer WASM runtime (~10ms cold start, ~5MB memory)".into(),
            cold_start_target: Duration::from_millis(10),
            execution_overhead_target: Duration::from_micros(500),
            memory_overhead_bytes: 5 * 1024 * 1024,
            throughput_rps_target: 100.0,
        }
    }

    /// Native process fork baseline (~5ms, ~8MB memory).
    pub fn native_process() -> ComparisonTarget {
        ComparisonTarget {
            name: "Native Process".into(),
            description: "Native process fork (~5ms cold start, ~8MB memory)".into(),
            cold_start_target: Duration::from_millis(5),
            execution_overhead_target: Duration::from_micros(100),
            memory_overhead_bytes: 8 * 1024 * 1024,
            throughput_rps_target: 200.0,
        }
    }

    /// Returns all pre-defined baselines.
    pub fn all() -> Vec<ComparisonTarget> {
        vec![
            Self::firecracker(),
            Self::gvisor(),
            Self::wasmer(),
            Self::native_process(),
        ]
    }
}

/// Result of a single benchmark measurement with raw samples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Cold start latencies in microseconds.
    pub cold_start_us: Vec<u64>,
    /// Execution latencies in microseconds.
    pub execution_us: Vec<u64>,
    /// Memory usage samples in bytes.
    pub memory_bytes: Vec<usize>,
    /// Measured throughput in requests per second.
    pub throughput_rps: f64,
}

impl BenchmarkResult {
    /// Mean of `cold_start_us` in microseconds.
    pub fn mean_cold_start_us(&self) -> f64 {
        mean_u64(&self.cold_start_us)
    }

    /// Median of `cold_start_us` in microseconds.
    pub fn median_cold_start_us(&self) -> f64 {
        median_u64(&self.cold_start_us)
    }

    /// 95th percentile of `cold_start_us`.
    pub fn p95_cold_start_us(&self) -> f64 {
        percentile_u64(&self.cold_start_us, 95.0)
    }

    /// 99th percentile of `cold_start_us`.
    pub fn p99_cold_start_us(&self) -> f64 {
        percentile_u64(&self.cold_start_us, 99.0)
    }

    /// Standard deviation of `cold_start_us`.
    pub fn std_dev_cold_start_us(&self) -> f64 {
        std_dev_u64(&self.cold_start_us)
    }

    /// Mean of `execution_us`.
    pub fn mean_execution_us(&self) -> f64 {
        mean_u64(&self.execution_us)
    }

    /// Median of `execution_us`.
    pub fn median_execution_us(&self) -> f64 {
        median_u64(&self.execution_us)
    }

    /// 95th percentile of `execution_us`.
    pub fn p95_execution_us(&self) -> f64 {
        percentile_u64(&self.execution_us, 95.0)
    }

    /// 99th percentile of `execution_us`.
    pub fn p99_execution_us(&self) -> f64 {
        percentile_u64(&self.execution_us, 99.0)
    }

    /// Standard deviation of `execution_us`.
    pub fn std_dev_execution_us(&self) -> f64 {
        std_dev_u64(&self.execution_us)
    }

    /// Mean of `memory_bytes`.
    pub fn mean_memory_bytes(&self) -> f64 {
        mean_usize(&self.memory_bytes)
    }

    /// Median of `memory_bytes`.
    pub fn median_memory_bytes(&self) -> f64 {
        median_usize(&self.memory_bytes)
    }

    /// 95th percentile of `memory_bytes`.
    pub fn p95_memory_bytes(&self) -> f64 {
        percentile_usize(&self.memory_bytes, 95.0)
    }

    /// 99th percentile of `memory_bytes`.
    pub fn p99_memory_bytes(&self) -> f64 {
        percentile_usize(&self.memory_bytes, 99.0)
    }

    /// Standard deviation of `memory_bytes`.
    pub fn std_dev_memory_bytes(&self) -> f64 {
        std_dev_usize(&self.memory_bytes)
    }
}

/// Verdict on how Isolate compares to a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonVerdict {
    /// Isolate is faster / uses less resources.
    Faster,
    /// Performance is comparable (within 20% of target).
    Comparable,
    /// Isolate is slower / uses more resources.
    Slower,
}

impl std::fmt::Display for ComparisonVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Faster => write!(f, "✅ Faster"),
            Self::Comparable => write!(f, "⚡ Comparable"),
            Self::Slower => write!(f, "⚠️ Slower"),
        }
    }
}

/// How Isolate compares to a single target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonResult {
    /// Ratio of Isolate cold start to target cold start (< 1.0 means Isolate is faster).
    pub cold_start_ratio: f64,
    /// Ratio of Isolate execution overhead to target.
    pub execution_ratio: f64,
    /// Ratio of Isolate memory usage to target.
    pub memory_ratio: f64,
    /// Overall verdict.
    pub verdict: ComparisonVerdict,
}

impl ComparisonResult {
    /// Compute a comparison between Isolate results and a target.
    pub fn compute(result: &BenchmarkResult, target: &ComparisonTarget) -> Self {
        let cold_start_ratio = if target.cold_start_target.as_micros() > 0 {
            result.mean_cold_start_us() / target.cold_start_target.as_micros() as f64
        } else {
            0.0
        };

        let execution_ratio = if target.execution_overhead_target.as_micros() > 0 {
            result.mean_execution_us() / target.execution_overhead_target.as_micros() as f64
        } else {
            0.0
        };

        let memory_ratio = if target.memory_overhead_bytes > 0 {
            result.mean_memory_bytes() / target.memory_overhead_bytes as f64
        } else {
            0.0
        };

        let avg_ratio = (cold_start_ratio + execution_ratio + memory_ratio) / 3.0;
        let verdict = if avg_ratio < 0.8 {
            ComparisonVerdict::Faster
        } else if avg_ratio <= 1.2 {
            ComparisonVerdict::Comparable
        } else {
            ComparisonVerdict::Slower
        };

        Self {
            cold_start_ratio,
            execution_ratio,
            memory_ratio,
            verdict,
        }
    }
}

/// A detected performance regression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Regression {
    /// Name of the regressed metric.
    pub metric_name: String,
    /// Previous value for the metric.
    pub previous_value: f64,
    /// Current value for the metric.
    pub current_value: f64,
    /// Regression percentage (positive means slower/worse).
    pub regression_pct: f64,
}

/// Full comparison report of Isolate against multiple targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Isolate's measured results.
    pub isolate_results: BenchmarkResult,
    /// Comparisons against each target.
    pub comparisons: Vec<(ComparisonTarget, ComparisonResult)>,
}

impl ComparisonReport {
    /// Build a report comparing Isolate results against all provided targets.
    pub fn build(result: BenchmarkResult, targets: &[ComparisonTarget]) -> Self {
        let comparisons = targets
            .iter()
            .map(|t| {
                let cmp = ComparisonResult::compute(&result, t);
                (t.clone(), cmp)
            })
            .collect();
        Self {
            isolate_results: result,
            comparisons,
        }
    }

    /// Render the report as a Markdown table.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Isolate Comparative Benchmark Report\n\n");

        md.push_str("## Isolate Results\n\n");
        md.push_str(&format!(
            "| Metric | Mean | Median | P95 | P99 | Std Dev |\n"
        ));
        md.push_str("| --- | ---: | ---: | ---: | ---: | ---: |\n");
        md.push_str(&format!(
            "| Cold Start (µs) | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
            self.isolate_results.mean_cold_start_us(),
            self.isolate_results.median_cold_start_us(),
            self.isolate_results.p95_cold_start_us(),
            self.isolate_results.p99_cold_start_us(),
            self.isolate_results.std_dev_cold_start_us(),
        ));
        md.push_str(&format!(
            "| Execution (µs) | {:.1} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
            self.isolate_results.mean_execution_us(),
            self.isolate_results.median_execution_us(),
            self.isolate_results.p95_execution_us(),
            self.isolate_results.p99_execution_us(),
            self.isolate_results.std_dev_execution_us(),
        ));
        md.push_str(&format!(
            "| Memory (bytes) | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} |\n",
            self.isolate_results.mean_memory_bytes(),
            self.isolate_results.median_memory_bytes(),
            self.isolate_results.p95_memory_bytes(),
            self.isolate_results.p99_memory_bytes(),
            self.isolate_results.std_dev_memory_bytes(),
        ));
        md.push_str(&format!(
            "| Throughput | {:.1} rps | | | | |\n\n",
            self.isolate_results.throughput_rps,
        ));

        md.push_str("## Comparison Against Targets\n\n");
        md.push_str(
            "| Target | Cold Start Ratio | Exec Ratio | Memory Ratio | Verdict |\n",
        );
        md.push_str("| --- | ---: | ---: | ---: | --- |\n");
        for (target, cmp) in &self.comparisons {
            md.push_str(&format!(
                "| {} | {:.2}x | {:.2}x | {:.2}x | {} |\n",
                target.name,
                cmp.cold_start_ratio,
                cmp.execution_ratio,
                cmp.memory_ratio,
                cmp.verdict,
            ));
        }
        md.push('\n');
        md
    }

    /// Render the report as JSON for CI consumption.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "isolate_results": {
                "cold_start_us_mean": self.isolate_results.mean_cold_start_us(),
                "cold_start_us_median": self.isolate_results.median_cold_start_us(),
                "cold_start_us_p95": self.isolate_results.p95_cold_start_us(),
                "cold_start_us_p99": self.isolate_results.p99_cold_start_us(),
                "execution_us_mean": self.isolate_results.mean_execution_us(),
                "execution_us_median": self.isolate_results.median_execution_us(),
                "memory_bytes_mean": self.isolate_results.mean_memory_bytes(),
                "throughput_rps": self.isolate_results.throughput_rps,
            },
            "comparisons": self.comparisons.iter().map(|(target, cmp)| {
                serde_json::json!({
                    "target": target.name,
                    "cold_start_ratio": cmp.cold_start_ratio,
                    "execution_ratio": cmp.execution_ratio,
                    "memory_ratio": cmp.memory_ratio,
                    "verdict": format!("{:?}", cmp.verdict),
                })
            }).collect::<Vec<_>>(),
        })
    }

    /// Detect regressions compared to previous Isolate results.
    pub fn has_regressions(
        &self,
        previous: &BenchmarkResult,
        threshold_pct: f64,
    ) -> Vec<Regression> {
        RegressionDetector::detect_regressions(
            &self.isolate_results,
            previous,
            threshold_pct,
        )
    }
}

/// CI integration for detecting performance regressions across runs.
pub struct RegressionDetector;

impl RegressionDetector {
    /// Load a previous baseline from a JSON file.
    pub fn load_baseline(path: &std::path::Path) -> std::io::Result<BenchmarkResult> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Save a baseline to a JSON file.
    pub fn save_baseline(
        path: &std::path::Path,
        results: &BenchmarkResult,
    ) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(results)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, data)
    }

    /// Compare current results against previous results and return regressions
    /// that exceed the given threshold percentage.
    pub fn detect_regressions(
        current: &BenchmarkResult,
        previous: &BenchmarkResult,
        threshold_pct: f64,
    ) -> Vec<Regression> {
        let mut regressions = Vec::new();

        let checks: &[(&str, f64, f64)] = &[
            (
                "cold_start_us_mean",
                previous.mean_cold_start_us(),
                current.mean_cold_start_us(),
            ),
            (
                "cold_start_us_p95",
                previous.p95_cold_start_us(),
                current.p95_cold_start_us(),
            ),
            (
                "execution_us_mean",
                previous.mean_execution_us(),
                current.mean_execution_us(),
            ),
            (
                "execution_us_p95",
                previous.p95_execution_us(),
                current.p95_execution_us(),
            ),
            (
                "memory_bytes_mean",
                previous.mean_memory_bytes(),
                current.mean_memory_bytes(),
            ),
        ];

        for &(name, prev, curr) in checks {
            if prev > 0.0 {
                let pct = ((curr - prev) / prev) * 100.0;
                if pct > threshold_pct {
                    regressions.push(Regression {
                        metric_name: name.to_string(),
                        previous_value: prev,
                        current_value: curr,
                        regression_pct: pct,
                    });
                }
            }
        }

        // Throughput regression is inverted: lower is worse.
        if previous.throughput_rps > 0.0 {
            let pct =
                ((previous.throughput_rps - current.throughput_rps) / previous.throughput_rps)
                    * 100.0;
            if pct > threshold_pct {
                regressions.push(Regression {
                    metric_name: "throughput_rps".to_string(),
                    previous_value: previous.throughput_rps,
                    current_value: current.throughput_rps,
                    regression_pct: pct,
                });
            }
        }

        regressions
    }
}

// ---------------------------------------------------------------------------
// Statistical helpers
// ---------------------------------------------------------------------------

fn mean_u64(values: &[u64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<u64>() as f64 / values.len() as f64
}

fn median_u64(values: &[u64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] as f64 + sorted[mid] as f64) / 2.0
    } else {
        sorted[mid] as f64
    }
}

fn percentile_u64(values: &[u64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((pct / 100.0) * (sorted.len() as f64 - 1.0))
        .round()
        .max(0.0) as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

fn std_dev_u64(values: &[u64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean_u64(values);
    let variance: f64 = values
        .iter()
        .map(|&v| {
            let diff = v as f64 - m;
            diff * diff
        })
        .sum::<f64>()
        / (values.len() - 1) as f64;
    variance.sqrt()
}

fn mean_usize(values: &[usize]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<usize>() as f64 / values.len() as f64
}

fn median_usize(values: &[usize]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] as f64 + sorted[mid] as f64) / 2.0
    } else {
        sorted[mid] as f64
    }
}

fn percentile_usize(values: &[usize], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let idx = ((pct / 100.0) * (sorted.len() as f64 - 1.0))
        .round()
        .max(0.0) as usize;
    sorted[idx.min(sorted.len() - 1)] as f64
}

fn std_dev_usize(values: &[usize]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean_usize(values);
    let variance: f64 = values
        .iter()
        .map(|&v| {
            let diff = v as f64 - m;
            diff * diff
        })
        .sum::<f64>()
        / (values.len() - 1) as f64;
    variance.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_result() -> BenchmarkResult {
        BenchmarkResult {
            cold_start_us: vec![3000, 3200, 3100, 2900, 3050],
            execution_us: vec![100, 110, 105, 95, 102],
            memory_bytes: vec![2_000_000, 2_100_000, 2_050_000, 1_950_000, 2_000_000],
            throughput_rps: 500.0,
        }
    }

    fn fast_result() -> BenchmarkResult {
        BenchmarkResult {
            cold_start_us: vec![1000, 1100, 1050],
            execution_us: vec![50, 55, 52],
            memory_bytes: vec![1_000_000, 1_100_000, 1_050_000],
            throughput_rps: 1000.0,
        }
    }

    fn slow_result() -> BenchmarkResult {
        BenchmarkResult {
            cold_start_us: vec![200_000, 210_000, 205_000],
            execution_us: vec![5000, 5500, 5200],
            memory_bytes: vec![50_000_000, 52_000_000, 51_000_000],
            throughput_rps: 5.0,
        }
    }

    // -----------------------------------------------------------------------
    // ComparisonBaseline tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_firecracker_baseline() {
        let fc = ComparisonBaseline::firecracker();
        assert_eq!(fc.name, "Firecracker");
        assert_eq!(fc.cold_start_target, Duration::from_millis(125));
        assert_eq!(fc.memory_overhead_bytes, 30 * 1024 * 1024);
    }

    #[test]
    fn test_gvisor_baseline() {
        let gv = ComparisonBaseline::gvisor();
        assert_eq!(gv.name, "gVisor");
        assert_eq!(gv.cold_start_target, Duration::from_millis(50));
        assert_eq!(gv.memory_overhead_bytes, 15 * 1024 * 1024);
    }

    #[test]
    fn test_wasmer_baseline() {
        let w = ComparisonBaseline::wasmer();
        assert_eq!(w.name, "Wasmer");
        assert_eq!(w.cold_start_target, Duration::from_millis(10));
        assert_eq!(w.memory_overhead_bytes, 5 * 1024 * 1024);
    }

    #[test]
    fn test_native_process_baseline() {
        let np = ComparisonBaseline::native_process();
        assert_eq!(np.name, "Native Process");
        assert_eq!(np.cold_start_target, Duration::from_millis(5));
        assert_eq!(np.memory_overhead_bytes, 8 * 1024 * 1024);
    }

    #[test]
    fn test_all_baselines_count() {
        let all = ComparisonBaseline::all();
        assert_eq!(all.len(), 4);
    }

    // -----------------------------------------------------------------------
    // BenchmarkResult statistical tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_mean_cold_start() {
        let r = sample_result();
        let mean = r.mean_cold_start_us();
        // (3000+3200+3100+2900+3050)/5 = 3050
        assert!((mean - 3050.0).abs() < 0.1);
    }

    #[test]
    fn test_median_cold_start() {
        let r = sample_result();
        let median = r.median_cold_start_us();
        // sorted: 2900, 3000, 3050, 3100, 3200 → median = 3050
        assert!((median - 3050.0).abs() < 0.1);
    }

    #[test]
    fn test_p95_cold_start() {
        let r = sample_result();
        let p95 = r.p95_cold_start_us();
        // sorted: [2900, 3000, 3050, 3100, 3200], idx = round(0.95*4)=round(3.8)=4 → 3200
        assert!((p95 - 3200.0).abs() < 0.1);
    }

    #[test]
    fn test_p99_cold_start() {
        let r = sample_result();
        let p99 = r.p99_cold_start_us();
        assert!((p99 - 3200.0).abs() < 0.1);
    }

    #[test]
    fn test_std_dev_cold_start() {
        let r = sample_result();
        let sd = r.std_dev_cold_start_us();
        assert!(sd > 0.0);
    }

    #[test]
    fn test_mean_execution() {
        let r = sample_result();
        let mean = r.mean_execution_us();
        // (100+110+105+95+102)/5 = 102.4
        assert!((mean - 102.4).abs() < 0.1);
    }

    #[test]
    fn test_mean_memory() {
        let r = sample_result();
        let mean = r.mean_memory_bytes();
        assert!((mean - 2_020_000.0).abs() < 0.1);
    }

    #[test]
    fn test_empty_samples_return_zero() {
        let r = BenchmarkResult {
            cold_start_us: vec![],
            execution_us: vec![],
            memory_bytes: vec![],
            throughput_rps: 0.0,
        };
        assert_eq!(r.mean_cold_start_us(), 0.0);
        assert_eq!(r.median_cold_start_us(), 0.0);
        assert_eq!(r.p95_cold_start_us(), 0.0);
        assert_eq!(r.std_dev_cold_start_us(), 0.0);
    }

    #[test]
    fn test_single_sample_stats() {
        let r = BenchmarkResult {
            cold_start_us: vec![5000],
            execution_us: vec![200],
            memory_bytes: vec![1_000_000],
            throughput_rps: 100.0,
        };
        assert!((r.mean_cold_start_us() - 5000.0).abs() < 0.1);
        assert!((r.median_cold_start_us() - 5000.0).abs() < 0.1);
        assert_eq!(r.std_dev_cold_start_us(), 0.0);
    }

    // -----------------------------------------------------------------------
    // ComparisonResult tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_comparison_faster_verdict() {
        let r = fast_result();
        let target = ComparisonBaseline::firecracker();
        let cmp = ComparisonResult::compute(&r, &target);
        assert!(cmp.cold_start_ratio < 1.0);
        assert_eq!(cmp.verdict, ComparisonVerdict::Faster);
    }

    #[test]
    fn test_comparison_slower_verdict() {
        let r = slow_result();
        let target = ComparisonBaseline::native_process();
        let cmp = ComparisonResult::compute(&r, &target);
        assert!(cmp.cold_start_ratio > 1.0);
        assert_eq!(cmp.verdict, ComparisonVerdict::Slower);
    }

    #[test]
    fn test_comparison_ratios_positive() {
        let r = sample_result();
        let target = ComparisonBaseline::firecracker();
        let cmp = ComparisonResult::compute(&r, &target);
        assert!(cmp.cold_start_ratio > 0.0);
        assert!(cmp.execution_ratio > 0.0);
        assert!(cmp.memory_ratio > 0.0);
    }

    // -----------------------------------------------------------------------
    // ComparisonReport tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_report_build_all_targets() {
        let r = sample_result();
        let report = ComparisonReport::build(r, &ComparisonBaseline::all());
        assert_eq!(report.comparisons.len(), 4);
    }

    #[test]
    fn test_report_to_markdown_contains_headers() {
        let r = sample_result();
        let report = ComparisonReport::build(r, &ComparisonBaseline::all());
        let md = report.to_markdown();
        assert!(md.contains("# Isolate Comparative Benchmark Report"));
        assert!(md.contains("Cold Start"));
        assert!(md.contains("Firecracker"));
        assert!(md.contains("gVisor"));
    }

    #[test]
    fn test_report_to_json_structure() {
        let r = sample_result();
        let report = ComparisonReport::build(r, &ComparisonBaseline::all());
        let json = report.to_json();
        assert!(json["isolate_results"]["cold_start_us_mean"].is_number());
        assert!(json["comparisons"].is_array());
        assert_eq!(json["comparisons"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn test_report_has_regressions_none() {
        let current = sample_result();
        let previous = sample_result();
        let report = ComparisonReport::build(current, &ComparisonBaseline::all());
        let regressions = report.has_regressions(&previous, 10.0);
        assert!(regressions.is_empty());
    }

    #[test]
    fn test_report_has_regressions_detected() {
        let current = slow_result();
        let previous = fast_result();
        let report = ComparisonReport::build(current, &ComparisonBaseline::all());
        let regressions = report.has_regressions(&previous, 10.0);
        assert!(!regressions.is_empty());
    }

    // -----------------------------------------------------------------------
    // RegressionDetector tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_no_regressions_same_data() {
        let a = sample_result();
        let b = sample_result();
        let regressions = RegressionDetector::detect_regressions(&a, &b, 5.0);
        assert!(regressions.is_empty());
    }

    #[test]
    fn test_detect_regressions_cold_start() {
        let previous = fast_result();
        let current = slow_result();
        let regressions =
            RegressionDetector::detect_regressions(&current, &previous, 10.0);
        let cold_start_reg = regressions
            .iter()
            .find(|r| r.metric_name == "cold_start_us_mean");
        assert!(cold_start_reg.is_some());
        assert!(cold_start_reg.unwrap().regression_pct > 10.0);
    }

    #[test]
    fn test_detect_regressions_throughput() {
        let previous = fast_result();
        let current = slow_result();
        let regressions =
            RegressionDetector::detect_regressions(&current, &previous, 10.0);
        let tp_reg = regressions
            .iter()
            .find(|r| r.metric_name == "throughput_rps");
        assert!(tp_reg.is_some());
        assert!(tp_reg.unwrap().regression_pct > 10.0);
    }

    #[test]
    fn test_save_and_load_baseline() {
        let result = sample_result();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");

        RegressionDetector::save_baseline(&path, &result).unwrap();
        let loaded = RegressionDetector::load_baseline(&path).unwrap();

        assert_eq!(loaded.cold_start_us, result.cold_start_us);
        assert_eq!(loaded.execution_us, result.execution_us);
        assert_eq!(loaded.memory_bytes, result.memory_bytes);
        assert!((loaded.throughput_rps - result.throughput_rps).abs() < 0.01);
    }

    #[test]
    fn test_load_baseline_missing_file() {
        let result = RegressionDetector::load_baseline(std::path::Path::new(
            "/nonexistent/path.json",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn test_comparison_verdict_display() {
        assert_eq!(format!("{}", ComparisonVerdict::Faster), "✅ Faster");
        assert_eq!(format!("{}", ComparisonVerdict::Comparable), "⚡ Comparable");
        assert_eq!(format!("{}", ComparisonVerdict::Slower), "⚠️ Slower");
    }

    // -----------------------------------------------------------------------
    // Statistical helper edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_median_even_count() {
        let r = BenchmarkResult {
            cold_start_us: vec![100, 200, 300, 400],
            execution_us: vec![],
            memory_bytes: vec![],
            throughput_rps: 0.0,
        };
        // sorted: 100, 200, 300, 400 → median = (200+300)/2 = 250
        assert!((r.median_cold_start_us() - 250.0).abs() < 0.1);
    }

    #[test]
    fn test_std_dev_two_samples() {
        let r = BenchmarkResult {
            cold_start_us: vec![100, 200],
            execution_us: vec![],
            memory_bytes: vec![],
            throughput_rps: 0.0,
        };
        let sd = r.std_dev_cold_start_us();
        // sample std dev of [100, 200]: sqrt((50^2+50^2)/1) = sqrt(5000) ≈ 70.71
        assert!((sd - 70.71).abs() < 1.0);
    }
}
