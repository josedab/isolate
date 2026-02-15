//! Constraint-based routing optimizer.

use serde::{Deserialize, Serialize};

use super::calculator::{CostCalculator, CostEstimate};

/// Constraints for routing decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConstraints {
    pub max_latency_ms: Option<u32>,
    pub required_regions: Vec<String>,
    pub excluded_providers: Vec<String>,
    pub max_cost_cents: Option<f64>,
    pub compliance_tags: Vec<String>,
}

impl Default for RoutingConstraints {
    fn default() -> Self {
        Self {
            max_latency_ms: None,
            required_regions: Vec::new(),
            excluded_providers: Vec::new(),
            max_cost_cents: None,
            compliance_tags: Vec::new(),
        }
    }
}

/// A routing decision with reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub provider: Option<String>,
    pub region: Option<String>,
    pub estimated_cost: Option<CostEstimate>,
    pub reason: String,
    pub alternatives_considered: usize,
    pub constraints_satisfied: bool,
}

/// Optimizes routing of sandbox executions across providers.
pub struct RoutingOptimizer {
    calculator: CostCalculator,
}

impl RoutingOptimizer {
    pub fn new(calculator: CostCalculator) -> Self {
        Self { calculator }
    }

    /// Make a routing decision based on constraints.
    pub fn route(&self, constraints: &RoutingConstraints) -> RoutingDecision {
        self.route_with_data(constraints, 1.0)
    }

    /// Make a routing decision with specific data size.
    pub fn route_with_data(&self, constraints: &RoutingConstraints, data_mb: f64) -> RoutingDecision {
        let all_options = self.calculator.estimate_all(data_mb);

        if all_options.is_empty() {
            return RoutingDecision {
                provider: None,
                region: None,
                estimated_cost: None,
                reason: "No pricing data available".into(),
                alternatives_considered: 0,
                constraints_satisfied: false,
            };
        }

        // Filter by constraints
        let filtered: Vec<&CostEstimate> = all_options.iter()
            .filter(|e| !constraints.excluded_providers.contains(&e.provider_id))
            .filter(|e| {
                constraints.required_regions.is_empty()
                || constraints.required_regions.iter().any(|r| e.region_id.contains(r))
            })
            .filter(|e| {
                constraints.max_cost_cents.map_or(true, |max| e.total_cents <= max)
            })
            .collect();

        let total_considered = all_options.len();

        if filtered.is_empty() {
            return RoutingDecision {
                provider: None,
                region: None,
                estimated_cost: None,
                reason: format!("No options satisfy constraints (considered {})", total_considered),
                alternatives_considered: total_considered,
                constraints_satisfied: false,
            };
        }

        // Select cheapest from filtered options
        let best = filtered.iter()
            .min_by(|a, b| a.total_cents.partial_cmp(&b.total_cents).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        RoutingDecision {
            provider: Some(best.provider_id.clone()),
            region: Some(best.region_id.clone()),
            estimated_cost: Some((*best).clone()),
            reason: format!(
                "Selected {} ({}) at {:.4}¢ — cheapest of {} options",
                best.provider_id, best.region_id, best.total_cents, filtered.len()
            ),
            alternatives_considered: total_considered,
            constraints_satisfied: true,
        }
    }

    /// Compare costs across all providers for a given workload.
    pub fn compare(&self, data_mb: f64) -> Vec<CostEstimate> {
        let mut all = self.calculator.estimate_all(data_mb);
        all.sort_by(|a, b| a.total_cents.partial_cmp(&b.total_cents).unwrap_or(std::cmp::Ordering::Equal));
        all
    }

    /// Calculate potential savings vs. most expensive option.
    pub fn savings_vs_worst(&self, data_mb: f64) -> Option<f64> {
        let comparison = self.compare(data_mb);
        if comparison.len() < 2 {
            return None;
        }
        let cheapest = comparison.first()?.total_cents;
        let most_expensive = comparison.last()?.total_cents;
        if most_expensive == 0.0 {
            return None;
        }
        Some((most_expensive - cheapest) / most_expensive * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_cost::pricing::PricingTracker;

    fn setup_optimizer() -> RoutingOptimizer {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.010);
        tracker.update_price("gcp", "us-central1", 0.005);
        tracker.update_price("azure", "eastus", 0.008);
        tracker.update_price("aws", "eu-west-1", 0.012);
        RoutingOptimizer::new(CostCalculator::new(tracker))
    }

    #[test]
    fn test_route_no_constraints() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints::default());
        assert!(decision.constraints_satisfied);
        assert_eq!(decision.provider.as_deref(), Some("gcp")); // cheapest
    }

    #[test]
    fn test_route_exclude_provider() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints {
            excluded_providers: vec!["gcp".into()],
            ..Default::default()
        });
        assert!(decision.constraints_satisfied);
        assert_ne!(decision.provider.as_deref(), Some("gcp"));
    }

    #[test]
    fn test_route_required_region() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints {
            required_regions: vec!["eu".into()],
            ..Default::default()
        });
        assert!(decision.constraints_satisfied);
        assert!(decision.region.unwrap().contains("eu"));
    }

    #[test]
    fn test_route_max_cost() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints {
            max_cost_cents: Some(0.01), // very tight budget
            ..Default::default()
        });
        // Might not find any option cheap enough
        if decision.constraints_satisfied {
            assert!(decision.estimated_cost.unwrap().total_cents <= 0.01);
        }
    }

    #[test]
    fn test_route_no_options() {
        let optimizer = RoutingOptimizer::new(CostCalculator::new(PricingTracker::new()));
        let decision = optimizer.route(&RoutingConstraints::default());
        assert!(!decision.constraints_satisfied);
        assert!(decision.provider.is_none());
    }

    #[test]
    fn test_route_impossible_constraints() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints {
            excluded_providers: vec!["aws".into(), "gcp".into(), "azure".into()],
            ..Default::default()
        });
        assert!(!decision.constraints_satisfied);
    }

    #[test]
    fn test_compare() {
        let optimizer = setup_optimizer();
        let comparison = optimizer.compare(1.0);
        assert_eq!(comparison.len(), 4);
        assert!(comparison[0].total_cents <= comparison[1].total_cents);
    }

    #[test]
    fn test_savings_calculation() {
        let optimizer = setup_optimizer();
        let savings = optimizer.savings_vs_worst(1.0).unwrap();
        assert!(savings > 0.0);
        assert!(savings < 100.0);
    }

    #[test]
    fn test_decision_metadata() {
        let optimizer = setup_optimizer();
        let decision = optimizer.route(&RoutingConstraints::default());
        assert_eq!(decision.alternatives_considered, 4);
        assert!(decision.reason.contains("cheapest"));
    }
}
