//! Cost Estimation Engine
//!
//! Maps resource usage to real monetary costs across cloud providers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Cloud provider pricing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CloudProvider {
    Aws,
    Gcp,
    Azure,
    Custom { name: String },
}

/// Pricing tier for compute resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingTier {
    pub provider: CloudProvider,
    pub region: String,
    pub vcpu_per_hour_usd: f64,
    pub memory_gb_per_hour_usd: f64,
    pub io_per_gb_usd: f64,
    pub network_egress_per_gb_usd: f64,
    pub request_per_million_usd: f64,
}

/// Cost breakdown for a sandbox execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub compute_usd: f64,
    pub memory_usd: f64,
    pub io_usd: f64,
    pub network_usd: f64,
    pub total_usd: f64,
    pub duration: Duration,
}

/// A recorded cost entry for a sandbox execution.
#[derive(Debug, Clone)]
pub struct CostRecord {
    pub sandbox_id: String,
    pub breakdown: CostBreakdown,
    pub timestamp: SystemTime,
}

/// Summary of all recorded costs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_usd: f64,
    pub avg_per_execution: f64,
    pub max_execution: f64,
    pub execution_count: usize,
}

/// Cost estimator that calculates per-sandbox costs.
pub struct CostEstimator {
    pricing: HashMap<String, PricingTier>,
    default_region: String,
    history: Vec<CostRecord>,
}

impl CostEstimator {
    /// Create a new cost estimator with default pricing for the given region.
    pub fn new(default_region: impl Into<String>) -> Self {
        let default_region = default_region.into();
        let mut pricing = HashMap::new();

        // AWS us-east-1 defaults
        pricing.insert(
            "aws-us-east-1".to_string(),
            PricingTier {
                provider: CloudProvider::Aws,
                region: "us-east-1".to_string(),
                vcpu_per_hour_usd: 0.0416,
                memory_gb_per_hour_usd: 0.0052,
                io_per_gb_usd: 0.08,
                network_egress_per_gb_usd: 0.09,
                request_per_million_usd: 0.20,
            },
        );

        // GCP us-central1 defaults
        pricing.insert(
            "gcp-us-central1".to_string(),
            PricingTier {
                provider: CloudProvider::Gcp,
                region: "us-central1".to_string(),
                vcpu_per_hour_usd: 0.0440,
                memory_gb_per_hour_usd: 0.0055,
                io_per_gb_usd: 0.10,
                network_egress_per_gb_usd: 0.12,
                request_per_million_usd: 0.40,
            },
        );

        // Azure eastus defaults
        pricing.insert(
            "azure-eastus".to_string(),
            PricingTier {
                provider: CloudProvider::Azure,
                region: "eastus".to_string(),
                vcpu_per_hour_usd: 0.0430,
                memory_gb_per_hour_usd: 0.0054,
                io_per_gb_usd: 0.09,
                network_egress_per_gb_usd: 0.087,
                request_per_million_usd: 0.20,
            },
        );

        Self { pricing, default_region, history: Vec::new() }
    }

    /// Add or update a pricing tier.
    pub fn add_pricing(&mut self, tier: PricingTier) {
        let key = format!(
            "{}-{}",
            match &tier.provider {
                CloudProvider::Aws => "aws",
                CloudProvider::Gcp => "gcp",
                CloudProvider::Azure => "azure",
                CloudProvider::Custom { name } => name.as_str(),
            },
            tier.region
        );
        self.pricing.insert(key, tier);
    }

    /// Estimate cost for a sandbox execution.
    pub fn estimate(
        &self,
        duration: Duration,
        memory_bytes: u64,
        io_bytes: u64,
        network_bytes: u64,
        region: Option<&str>,
    ) -> CostBreakdown {
        let region_key = region.unwrap_or(&self.default_region);
        let tier = match self.pricing.get(region_key) {
            Some(t) => t,
            None => {
                return CostBreakdown { duration, ..Default::default() };
            }
        };

        let hours = duration.as_secs_f64() / 3600.0;
        let memory_gb = memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let io_gb = io_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let network_gb = network_bytes as f64 / (1024.0 * 1024.0 * 1024.0);

        let compute_usd = tier.vcpu_per_hour_usd * hours;
        let memory_usd = tier.memory_gb_per_hour_usd * memory_gb * hours;
        let io_usd = tier.io_per_gb_usd * io_gb;
        let network_usd = tier.network_egress_per_gb_usd * network_gb;
        let total_usd = compute_usd + memory_usd + io_usd + network_usd;

        CostBreakdown { compute_usd, memory_usd, io_usd, network_usd, total_usd, duration }
    }

    /// Record a cost entry for a sandbox.
    pub fn record(&mut self, sandbox_id: impl Into<String>, breakdown: CostBreakdown) {
        self.history.push(CostRecord {
            sandbox_id: sandbox_id.into(),
            breakdown,
            timestamp: SystemTime::now(),
        });
    }

    /// Get total cost across all recorded executions.
    pub fn total_cost(&self) -> f64 {
        self.history.iter().map(|r| r.breakdown.total_usd).sum()
    }

    /// Get total cost for a specific sandbox.
    pub fn cost_by_sandbox(&self, sandbox_id: &str) -> f64 {
        self.history
            .iter()
            .filter(|r| r.sandbox_id == sandbox_id)
            .map(|r| r.breakdown.total_usd)
            .sum()
    }

    /// Get a summary of all recorded costs.
    pub fn cost_summary(&self) -> CostSummary {
        let execution_count = self.history.len();
        if execution_count == 0 {
            return CostSummary {
                total_usd: 0.0,
                avg_per_execution: 0.0,
                max_execution: 0.0,
                execution_count: 0,
            };
        }

        let total_usd: f64 = self.history.iter().map(|r| r.breakdown.total_usd).sum();
        let max_execution =
            self.history.iter().map(|r| r.breakdown.total_usd).fold(0.0_f64, f64::max);

        CostSummary {
            total_usd,
            avg_per_execution: total_usd / execution_count as f64,
            max_execution,
            execution_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimator() -> CostEstimator {
        CostEstimator::new("aws-us-east-1")
    }

    #[test]
    fn test_estimate_basic_cost() {
        let est = estimator();
        let breakdown = est.estimate(
            Duration::from_secs(3600),
            1024 * 1024 * 1024, // 1 GB
            0,
            0,
            None,
        );
        assert!(breakdown.compute_usd > 0.0);
        assert!(breakdown.memory_usd > 0.0);
        assert_eq!(breakdown.io_usd, 0.0);
        assert_eq!(breakdown.network_usd, 0.0);
        assert!(
            (breakdown.total_usd - (breakdown.compute_usd + breakdown.memory_usd)).abs() < 1e-10
        );
    }

    #[test]
    fn test_estimate_with_io_and_network() {
        let est = estimator();
        let breakdown = est.estimate(
            Duration::from_secs(60),
            512 * 1024 * 1024,       // 512 MB
            10 * 1024 * 1024 * 1024, // 10 GB I/O
            1024 * 1024 * 1024,      // 1 GB network
            None,
        );
        assert!(breakdown.io_usd > 0.0);
        assert!(breakdown.network_usd > 0.0);
        let expected_total =
            breakdown.compute_usd + breakdown.memory_usd + breakdown.io_usd + breakdown.network_usd;
        assert!((breakdown.total_usd - expected_total).abs() < 1e-10);
    }

    #[test]
    fn test_estimate_unknown_region_returns_zero() {
        let est = estimator();
        let breakdown = est.estimate(Duration::from_secs(60), 1024, 1024, 1024, Some("unknown"));
        assert_eq!(breakdown.total_usd, 0.0);
    }

    #[test]
    fn test_record_and_total_cost() {
        let mut est = estimator();
        let b1 = CostBreakdown { total_usd: 1.50, ..Default::default() };
        let b2 = CostBreakdown { total_usd: 2.50, ..Default::default() };
        est.record("sandbox-1", b1);
        est.record("sandbox-2", b2);
        assert!((est.total_cost() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_cost_by_sandbox() {
        let mut est = estimator();
        est.record("sb-a", CostBreakdown { total_usd: 1.0, ..Default::default() });
        est.record("sb-b", CostBreakdown { total_usd: 2.0, ..Default::default() });
        est.record("sb-a", CostBreakdown { total_usd: 3.0, ..Default::default() });
        assert!((est.cost_by_sandbox("sb-a") - 4.0).abs() < 1e-10);
        assert!((est.cost_by_sandbox("sb-b") - 2.0).abs() < 1e-10);
        assert!((est.cost_by_sandbox("sb-c") - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_cost_summary_empty() {
        let est = estimator();
        let summary = est.cost_summary();
        assert_eq!(summary.execution_count, 0);
        assert_eq!(summary.total_usd, 0.0);
    }

    #[test]
    fn test_cost_summary_with_records() {
        let mut est = estimator();
        est.record("sb-1", CostBreakdown { total_usd: 1.0, ..Default::default() });
        est.record("sb-2", CostBreakdown { total_usd: 3.0, ..Default::default() });
        est.record("sb-3", CostBreakdown { total_usd: 2.0, ..Default::default() });
        let summary = est.cost_summary();
        assert_eq!(summary.execution_count, 3);
        assert!((summary.total_usd - 6.0).abs() < 1e-10);
        assert!((summary.avg_per_execution - 2.0).abs() < 1e-10);
        assert!((summary.max_execution - 3.0).abs() < 1e-10);
    }

    #[test]
    fn test_add_custom_pricing() {
        let mut est = estimator();
        est.add_pricing(PricingTier {
            provider: CloudProvider::Custom { name: "mycloud".to_string() },
            region: "local".to_string(),
            vcpu_per_hour_usd: 0.01,
            memory_gb_per_hour_usd: 0.001,
            io_per_gb_usd: 0.01,
            network_egress_per_gb_usd: 0.01,
            request_per_million_usd: 0.10,
        });
        let breakdown = est.estimate(
            Duration::from_secs(3600),
            1024 * 1024 * 1024,
            0,
            0,
            Some("mycloud-local"),
        );
        assert!(breakdown.compute_usd > 0.0);
    }

    #[test]
    fn test_gcp_region_pricing() {
        let est = estimator();
        let breakdown = est.estimate(
            Duration::from_secs(3600),
            1024 * 1024 * 1024,
            0,
            0,
            Some("gcp-us-central1"),
        );
        assert!(breakdown.compute_usd > 0.0);
        // GCP is slightly more expensive for compute
        let aws_breakdown = est.estimate(
            Duration::from_secs(3600),
            1024 * 1024 * 1024,
            0,
            0,
            Some("aws-us-east-1"),
        );
        assert!(breakdown.compute_usd > aws_breakdown.compute_usd);
    }
}
