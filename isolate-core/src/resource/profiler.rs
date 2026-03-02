//! Sandbox Resource Profiler & Cloud Cost Estimator
//!
//! Collects per-sandbox resource usage profiles and projects cloud costs using
//! configurable pricing models. Also provides optimization recommendations.

#![allow(missing_docs)]
use super::metering::ResourceUsage;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Cloud pricing models
// ---------------------------------------------------------------------------

/// Cloud provider identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CloudProvider {
    AwsLambda,
    GcpCloudRun,
    AzureFunctions,
    Custom(String),
}

impl std::fmt::Display for CloudProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AwsLambda => write!(f, "AWS Lambda"),
            Self::GcpCloudRun => write!(f, "GCP Cloud Run"),
            Self::AzureFunctions => write!(f, "Azure Functions"),
            Self::Custom(name) => write!(f, "{}", name),
        }
    }
}

/// Per-unit pricing for a cloud compute offering.
#[derive(Debug, Clone)]
pub struct PricingModel {
    /// Provider name.
    pub provider: CloudProvider,
    /// Cost per GB-second of memory.
    pub memory_gb_second: f64,
    /// Cost per vCPU-second.
    pub vcpu_second: f64,
    /// Cost per invocation / request.
    pub invocation: f64,
    /// Free-tier seconds per month (GB-seconds).
    pub free_tier_gb_seconds: f64,
    /// Cost per GB of network egress.
    pub network_egress_gb: f64,
}

impl PricingModel {
    /// AWS Lambda pricing (us-east-1, as of 2024).
    pub fn aws_lambda() -> Self {
        Self {
            provider: CloudProvider::AwsLambda,
            memory_gb_second: 0.0000166667,
            vcpu_second: 0.0000133334,
            invocation: 0.0000002,
            free_tier_gb_seconds: 400_000.0,
            network_egress_gb: 0.09,
        }
    }

    /// GCP Cloud Run pricing.
    pub fn gcp_cloud_run() -> Self {
        Self {
            provider: CloudProvider::GcpCloudRun,
            memory_gb_second: 0.00000250,
            vcpu_second: 0.00002400,
            invocation: 0.0000004,
            free_tier_gb_seconds: 180_000.0,
            network_egress_gb: 0.12,
        }
    }

    /// Azure Functions pricing.
    pub fn azure_functions() -> Self {
        Self {
            provider: CloudProvider::AzureFunctions,
            memory_gb_second: 0.000016,
            vcpu_second: 0.000016,
            invocation: 0.0000002,
            free_tier_gb_seconds: 400_000.0,
            network_egress_gb: 0.087,
        }
    }
}

// ---------------------------------------------------------------------------
// Resource profile
// ---------------------------------------------------------------------------

/// A single sandbox execution profile.
#[derive(Debug, Clone)]
pub struct ExecutionProfile {
    /// Sandbox or execution identifier.
    pub id: String,
    /// Resource usage snapshot.
    pub usage: ResourceUsage,
    /// Memory limit that was configured.
    pub memory_limit: usize,
    /// Fuel limit that was configured (0 = unlimited).
    pub fuel_limit: u64,
}

/// Aggregate profile statistics across multiple executions.
#[derive(Debug, Clone)]
pub struct ProfileSummary {
    /// Number of executions profiled.
    pub count: usize,
    /// Average wall time.
    pub avg_wall_time: Duration,
    /// P50 wall time.
    pub p50_wall_time: Duration,
    /// P99 wall time.
    pub p99_wall_time: Duration,
    /// Average peak memory (bytes).
    pub avg_peak_memory: usize,
    /// Max peak memory across all executions.
    pub max_peak_memory: usize,
    /// Average fuel consumed.
    pub avg_fuel: u64,
    /// Max fuel consumed.
    pub max_fuel: u64,
    /// Average I/O bytes (read + write).
    pub avg_io_bytes: u64,
    /// Total I/O bytes across all executions.
    pub total_io_bytes: u64,
}

// ---------------------------------------------------------------------------
// Cost estimate
// ---------------------------------------------------------------------------

/// Estimated cloud cost for a workload.
#[derive(Debug, Clone)]
pub struct CostEstimate {
    /// Cloud provider.
    pub provider: CloudProvider,
    /// Cost per single invocation.
    pub per_invocation: f64,
    /// Projected monthly cost at the given request rate.
    pub monthly_cost: f64,
    /// Monthly requests assumed.
    pub monthly_requests: u64,
    /// Memory GB-seconds per invocation.
    pub gb_seconds_per_invocation: f64,
    /// vCPU-seconds per invocation.
    pub vcpu_seconds_per_invocation: f64,
    /// Free-tier savings included in monthly_cost.
    pub free_tier_savings: f64,
}

// ---------------------------------------------------------------------------
// Optimization recommendation
// ---------------------------------------------------------------------------

/// A resource optimization recommendation.
#[derive(Debug, Clone)]
pub struct Recommendation {
    /// Category (memory, fuel, timeout, pool).
    pub category: String,
    /// Human-readable recommendation.
    pub message: String,
    /// Potential monthly savings (if estimable).
    pub estimated_savings: Option<f64>,
}

// ---------------------------------------------------------------------------
// Resource Profiler
// ---------------------------------------------------------------------------

/// Collects execution profiles and produces cost estimates and recommendations.
pub struct ResourceProfiler {
    profiles: Vec<ExecutionProfile>,
    pricing_models: Vec<PricingModel>,
}

impl ResourceProfiler {
    /// Creates a profiler with default pricing models (AWS, GCP, Azure).
    pub fn new() -> Self {
        Self {
            profiles: Vec::new(),
            pricing_models: vec![
                PricingModel::aws_lambda(),
                PricingModel::gcp_cloud_run(),
                PricingModel::azure_functions(),
            ],
        }
    }

    /// Creates a profiler with custom pricing models.
    pub fn with_pricing(pricing_models: Vec<PricingModel>) -> Self {
        Self { profiles: Vec::new(), pricing_models }
    }

    /// Records an execution profile.
    pub fn record(&mut self, profile: ExecutionProfile) {
        self.profiles.push(profile);
    }

    /// Returns the number of recorded profiles.
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Computes aggregate statistics across all recorded profiles.
    pub fn summary(&self) -> ProfileSummary {
        if self.profiles.is_empty() {
            return ProfileSummary {
                count: 0,
                avg_wall_time: Duration::ZERO,
                p50_wall_time: Duration::ZERO,
                p99_wall_time: Duration::ZERO,
                avg_peak_memory: 0,
                max_peak_memory: 0,
                avg_fuel: 0,
                max_fuel: 0,
                avg_io_bytes: 0,
                total_io_bytes: 0,
            };
        }

        let n = self.profiles.len();

        let mut wall_times: Vec<Duration> =
            self.profiles.iter().map(|p| p.usage.wall_time).collect();
        wall_times.sort();

        let total_wall: Duration = wall_times.iter().sum();
        let avg_wall = total_wall / n as u32;

        let p50_wall = wall_times[n / 2];
        let p99_wall = wall_times[(n * 99 / 100).min(n - 1)];

        let total_peak_mem: usize = self.profiles.iter().map(|p| p.usage.peak_memory).sum();
        let max_peak_mem = self.profiles.iter().map(|p| p.usage.peak_memory).max().unwrap_or(0);

        let total_fuel: u64 = self.profiles.iter().map(|p| p.usage.fuel_consumed).sum();
        let max_fuel = self.profiles.iter().map(|p| p.usage.fuel_consumed).max().unwrap_or(0);

        let total_io: u64 =
            self.profiles.iter().map(|p| p.usage.bytes_read + p.usage.bytes_written).sum();

        ProfileSummary {
            count: n,
            avg_wall_time: avg_wall,
            p50_wall_time: p50_wall,
            p99_wall_time: p99_wall,
            avg_peak_memory: total_peak_mem / n,
            max_peak_memory: max_peak_mem,
            avg_fuel: total_fuel / n as u64,
            max_fuel,
            avg_io_bytes: total_io / n as u64,
            total_io_bytes: total_io,
        }
    }

    /// Estimates cloud cost for each configured pricing model.
    pub fn estimate_costs(&self, monthly_requests: u64) -> Vec<CostEstimate> {
        let summary = self.summary();
        if summary.count == 0 {
            return Vec::new();
        }

        self.pricing_models
            .iter()
            .map(|model| self.estimate_for_model(model, &summary, monthly_requests))
            .collect()
    }

    /// Estimates cost for a specific provider.
    pub fn estimate_for_provider(
        &self,
        provider: &CloudProvider,
        monthly_requests: u64,
    ) -> Option<CostEstimate> {
        let summary = self.summary();
        if summary.count == 0 {
            return None;
        }
        self.pricing_models
            .iter()
            .find(|m| &m.provider == provider)
            .map(|model| self.estimate_for_model(model, &summary, monthly_requests))
    }

    /// Generates optimization recommendations based on profiled usage.
    pub fn recommendations(&self) -> Vec<Recommendation> {
        let summary = self.summary();
        if summary.count == 0 {
            return Vec::new();
        }

        let mut recs = Vec::new();

        // Memory over-provisioning check
        for p in &self.profiles {
            if p.memory_limit > 0 && p.usage.peak_memory > 0 {
                let utilization = p.usage.peak_memory as f64 / p.memory_limit as f64;
                if utilization < 0.25 {
                    recs.push(Recommendation {
                        category: "memory".to_string(),
                        message: format!(
                            "Memory utilization is only {:.0}%. Consider reducing memory_limit from {} to {}.",
                            utilization * 100.0,
                            format_bytes(p.memory_limit as u64),
                            format_bytes((p.usage.peak_memory as f64 * 2.0) as u64)
                        ),
                        estimated_savings: None,
                    });
                    break; // One recommendation per category
                }
            }
        }

        // Fuel over-provisioning check
        if summary.max_fuel > 0 {
            let fuel_limits: Vec<u64> =
                self.profiles.iter().filter(|p| p.fuel_limit > 0).map(|p| p.fuel_limit).collect();
            if !fuel_limits.is_empty() {
                let avg_limit: u64 = fuel_limits.iter().sum::<u64>() / fuel_limits.len() as u64;
                if avg_limit > 0 && summary.avg_fuel < avg_limit / 4 {
                    recs.push(Recommendation {
                        category: "fuel".to_string(),
                        message: format!(
                            "Average fuel consumption ({}) is <25% of fuel limit ({}). Consider lowering.",
                            summary.avg_fuel, avg_limit
                        ),
                        estimated_savings: None,
                    });
                }
            }
        }

        // Timeout suggestion
        if summary.p99_wall_time > Duration::ZERO {
            let suggested_timeout = summary.p99_wall_time.mul_f64(2.0);
            recs.push(Recommendation {
                category: "timeout".to_string(),
                message: format!(
                    "P99 wall time is {:.1}ms. Recommended wall_time_limit: {:.0}ms (2x P99).",
                    summary.p99_wall_time.as_secs_f64() * 1000.0,
                    suggested_timeout.as_secs_f64() * 1000.0
                ),
                estimated_savings: None,
            });
        }

        // Pool warm size suggestion
        if summary.avg_wall_time < Duration::from_millis(10) && summary.count >= 10 {
            recs.push(Recommendation {
                category: "pool".to_string(),
                message: "Fast executions (<10ms avg). Consider a warm pool to reduce cold-start overhead.".to_string(),
                estimated_savings: None,
            });
        }

        recs
    }

    /// Renders a text report of profile summary and cost estimates.
    pub fn render_report(&self, monthly_requests: u64) -> String {
        let summary = self.summary();
        let costs = self.estimate_costs(monthly_requests);
        let recs = self.recommendations();

        let mut out = String::new();
        out.push_str("═══════════════════════════════════════════════\n");
        out.push_str("       RESOURCE PROFILE & COST REPORT\n");
        out.push_str("═══════════════════════════════════════════════\n\n");

        out.push_str(&format!("Executions profiled: {}\n", summary.count));
        out.push_str(&format!(
            "Avg wall time:      {:.2}ms\n",
            summary.avg_wall_time.as_secs_f64() * 1000.0
        ));
        out.push_str(&format!(
            "P99 wall time:      {:.2}ms\n",
            summary.p99_wall_time.as_secs_f64() * 1000.0
        ));
        out.push_str(&format!(
            "Avg peak memory:    {}\n",
            format_bytes(summary.avg_peak_memory as u64)
        ));
        out.push_str(&format!(
            "Max peak memory:    {}\n",
            format_bytes(summary.max_peak_memory as u64)
        ));
        out.push_str(&format!("Avg fuel consumed:  {}\n", summary.avg_fuel));
        out.push_str(&format!("Total I/O:          {}\n\n", format_bytes(summary.total_io_bytes)));

        if !costs.is_empty() {
            out.push_str(&format!("Cost estimates ({} requests/month):\n", monthly_requests));
            out.push_str("───────────────────────────────────────────────\n");
            for c in &costs {
                out.push_str(&format!(
                    "  {:<20} ${:.6}/req  ${:.2}/month\n",
                    c.provider.to_string(),
                    c.per_invocation,
                    c.monthly_cost
                ));
            }
            out.push('\n');
        }

        if !recs.is_empty() {
            out.push_str("Recommendations:\n");
            out.push_str("───────────────────────────────────────────────\n");
            for r in &recs {
                out.push_str(&format!("  [{}] {}\n", r.category, r.message));
            }
        }

        out.push_str("═══════════════════════════════════════════════\n");
        out
    }

    // -- private helpers --

    fn estimate_for_model(
        &self,
        model: &PricingModel,
        summary: &ProfileSummary,
        monthly_requests: u64,
    ) -> CostEstimate {
        let gb_seconds = (summary.avg_peak_memory as f64 / (1024.0 * 1024.0 * 1024.0))
            * summary.avg_wall_time.as_secs_f64();

        let vcpu_seconds = summary.avg_wall_time.as_secs_f64();

        let per_invocation = (gb_seconds * model.memory_gb_second)
            + (vcpu_seconds * model.vcpu_second)
            + model.invocation;

        let total_gb_seconds = gb_seconds * monthly_requests as f64;
        let free_tier_savings =
            total_gb_seconds.min(model.free_tier_gb_seconds) * model.memory_gb_second;

        let monthly_cost = (per_invocation * monthly_requests as f64) - free_tier_savings;

        CostEstimate {
            provider: model.provider.clone(),
            per_invocation,
            monthly_cost: monthly_cost.max(0.0),
            monthly_requests,
            gb_seconds_per_invocation: gb_seconds,
            vcpu_seconds_per_invocation: vcpu_seconds,
            free_tier_savings,
        }
    }
}

impl Default for ResourceProfiler {
    fn default() -> Self {
        Self::new()
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Export execution profiles as Chrome DevTools Trace Event Format.
///
/// The output can be loaded in `chrome://tracing` or Perfetto UI.
pub fn export_chrome_trace(profiles: &[ExecutionProfile]) -> serde_json::Value {
    let mut events = Vec::new();

    for (i, profile) in profiles.iter().enumerate() {
        // Duration event for each execution
        events.push(serde_json::json!({
            "name": format!("run_{}", profile.id),
            "cat": "sandbox",
            "ph": "X",
            "ts": i as u64 * profile.usage.wall_time.as_micros() as u64,
            "dur": profile.usage.wall_time.as_micros() as u64,
            "pid": 1,
            "tid": 1,
            "args": {
                "fuel_consumed": profile.usage.fuel_consumed,
                "peak_memory": profile.usage.peak_memory,
                "bytes_read": profile.usage.bytes_read,
                "bytes_written": profile.usage.bytes_written,
            }
        }));

        // Counter event for memory
        events.push(serde_json::json!({
            "name": "peak_memory",
            "cat": "memory",
            "ph": "C",
            "ts": i as u64 * profile.usage.wall_time.as_micros() as u64,
            "pid": 1,
            "args": { "bytes": profile.usage.peak_memory }
        }));
    }

    serde_json::json!({
        "traceEvents": events,
        "displayTimeUnit": "us"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_usage(wall_ms: u64, peak_mem: usize, fuel: u64) -> ResourceUsage {
        ResourceUsage {
            peak_memory: peak_mem,
            current_memory: peak_mem / 2,
            fuel_consumed: fuel,
            cpu_time: Duration::from_millis(wall_ms),
            wall_time: Duration::from_millis(wall_ms),
            bytes_read: 1024,
            bytes_written: 512,
            io_operations: 10,
            io_read_ops: 5,
            io_write_ops: 5,
            fuel_per_function: Default::default(),
            memory_timeline: Default::default(),
        }
    }

    fn sample_profile(id: &str, wall_ms: u64, peak_mem: usize, fuel: u64) -> ExecutionProfile {
        ExecutionProfile {
            id: id.to_string(),
            usage: sample_usage(wall_ms, peak_mem, fuel),
            memory_limit: peak_mem * 4,
            fuel_limit: fuel * 4,
        }
    }

    #[test]
    fn test_profiler_creation() {
        let p = ResourceProfiler::new();
        assert_eq!(p.profile_count(), 0);
    }

    #[test]
    fn test_record_profile() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("exec-1", 5, 1024 * 1024, 50_000));
        assert_eq!(p.profile_count(), 1);
    }

    #[test]
    fn test_summary_empty() {
        let p = ResourceProfiler::new();
        let s = p.summary();
        assert_eq!(s.count, 0);
        assert_eq!(s.avg_wall_time, Duration::ZERO);
    }

    #[test]
    fn test_summary_single_profile() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 10, 2 * 1024 * 1024, 100_000));

        let s = p.summary();
        assert_eq!(s.count, 1);
        assert_eq!(s.avg_wall_time, Duration::from_millis(10));
        assert_eq!(s.avg_peak_memory, 2 * 1024 * 1024);
        assert_eq!(s.avg_fuel, 100_000);
    }

    #[test]
    fn test_summary_multiple_profiles() {
        let mut p = ResourceProfiler::new();
        for i in 0..10 {
            p.record(sample_profile(&format!("e{}", i), 5 + i, 1024 * 1024, 10_000));
        }

        let s = p.summary();
        assert_eq!(s.count, 10);
        assert!(s.p50_wall_time >= Duration::from_millis(5));
        assert!(s.p99_wall_time >= s.p50_wall_time);
    }

    #[test]
    fn test_estimate_costs_empty() {
        let p = ResourceProfiler::new();
        assert!(p.estimate_costs(1_000_000).is_empty());
    }

    #[test]
    fn test_estimate_costs_three_providers() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 100, 128 * 1024 * 1024, 500_000));

        let costs = p.estimate_costs(1_000_000);
        assert_eq!(costs.len(), 3);

        for c in &costs {
            assert!(c.per_invocation > 0.0);
            assert!(c.monthly_requests == 1_000_000);
        }
    }

    #[test]
    fn test_estimate_aws_lambda() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 100, 128 * 1024 * 1024, 500_000));

        let est = p.estimate_for_provider(&CloudProvider::AwsLambda, 1_000_000);
        assert!(est.is_some());
        let est = est.unwrap();
        assert!(est.per_invocation > 0.0);
        assert!(est.gb_seconds_per_invocation > 0.0);
    }

    #[test]
    fn test_estimate_unknown_provider() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 100, 128 * 1024 * 1024, 500_000));

        let est = p.estimate_for_provider(&CloudProvider::Custom("Fly.io".to_string()), 100);
        assert!(est.is_none());
    }

    #[test]
    fn test_free_tier_savings() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 10, 64 * 1024 * 1024, 10_000));

        let costs = p.estimate_costs(100); // Very low volume → should get free-tier savings
        for c in &costs {
            assert!(c.free_tier_savings >= 0.0);
        }
    }

    #[test]
    fn test_recommendations_empty() {
        let p = ResourceProfiler::new();
        assert!(p.recommendations().is_empty());
    }

    #[test]
    fn test_recommendations_memory_over_provisioned() {
        let mut p = ResourceProfiler::new();
        // peak_memory = 1MB, memory_limit = 4MB (25% utilization)
        // Our check is <25%, so make it even smaller
        let profile = ExecutionProfile {
            id: "e1".to_string(),
            usage: sample_usage(5, 256 * 1024, 10_000), // 256KB peak
            memory_limit: 128 * 1024 * 1024,            // 128MB limit = <1% utilization
            fuel_limit: 1_000_000,
        };
        p.record(profile);

        let recs = p.recommendations();
        let mem_rec = recs.iter().find(|r| r.category == "memory");
        assert!(mem_rec.is_some(), "Should recommend memory reduction");
    }

    #[test]
    fn test_recommendations_fuel_over_provisioned() {
        let mut p = ResourceProfiler::new();
        let profile = ExecutionProfile {
            id: "e1".to_string(),
            usage: sample_usage(5, 1024 * 1024, 1_000),
            memory_limit: 1024 * 1024,
            fuel_limit: 1_000_000, // 1000x more than consumed
        };
        p.record(profile);

        let recs = p.recommendations();
        let fuel_rec = recs.iter().find(|r| r.category == "fuel");
        assert!(fuel_rec.is_some(), "Should recommend fuel reduction");
    }

    #[test]
    fn test_recommendations_timeout() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 50, 1024 * 1024, 10_000));

        let recs = p.recommendations();
        let timeout_rec = recs.iter().find(|r| r.category == "timeout");
        assert!(timeout_rec.is_some(), "Should suggest timeout based on P99");
    }

    #[test]
    fn test_render_report() {
        let mut p = ResourceProfiler::new();
        p.record(sample_profile("e1", 50, 64 * 1024 * 1024, 100_000));

        let report = p.render_report(1_000_000);
        assert!(report.contains("RESOURCE PROFILE & COST REPORT"));
        assert!(report.contains("AWS Lambda"));
        assert!(report.contains("GCP Cloud Run"));
        assert!(report.contains("Azure Functions"));
        assert!(report.contains("Recommendations"));
    }

    #[test]
    fn test_custom_pricing() {
        let custom = PricingModel {
            provider: CloudProvider::Custom("Fly.io".to_string()),
            memory_gb_second: 0.000010,
            vcpu_second: 0.000020,
            invocation: 0.0,
            free_tier_gb_seconds: 0.0,
            network_egress_gb: 0.02,
        };

        let mut p = ResourceProfiler::with_pricing(vec![custom]);
        p.record(sample_profile("e1", 100, 128 * 1024 * 1024, 500_000));

        let costs = p.estimate_costs(1_000);
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].provider, CloudProvider::Custom("Fly.io".to_string()));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn test_cloud_provider_display() {
        assert_eq!(CloudProvider::AwsLambda.to_string(), "AWS Lambda");
        assert_eq!(CloudProvider::GcpCloudRun.to_string(), "GCP Cloud Run");
        assert_eq!(CloudProvider::AzureFunctions.to_string(), "Azure Functions");
        assert_eq!(CloudProvider::Custom("Edge".to_string()).to_string(), "Edge");
    }
}
