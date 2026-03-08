//! Multi-Cloud Cost Optimizer.
//!
//! Route sandbox executions across cloud providers based on pricing,
//! latency, and compliance requirements.
//!
//! # Features
//!
//! - **Provider Abstraction**: Unified interface for AWS, Azure, GCP, on-prem
//! - **Spot Pricing**: Real-time pricing tracking and forecasting
//! - **Cost Calculator**: Accurate cost estimation per execution
//! - **Routing Optimizer**: Constraint-based optimal provider selection

#![allow(missing_docs)]
pub mod calculator;
pub mod optimizer;
pub mod pricing;
pub mod provider;

pub use calculator::{CostCalculator, CostEstimate};
pub use optimizer::{RoutingConstraints, RoutingDecision, RoutingOptimizer};
pub use pricing::{PricePoint, PricingTier, PricingTracker};
pub use provider::{CloudProvider, ProviderCapabilities, ProviderConfig, ProviderRegion};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_end_to_end_routing() {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.0050); // $0.005/exec
        tracker.update_price("gcp", "us-central1", 0.0045); // cheaper

        let calc = CostCalculator::new(tracker);
        let optimizer = RoutingOptimizer::new(calc);

        let decision = optimizer.route(&RoutingConstraints {
            max_latency_ms: None,
            required_regions: vec![],
            excluded_providers: vec![],
            max_cost_cents: None,
            compliance_tags: vec![],
        });

        assert!(decision.provider.is_some());
    }
}
