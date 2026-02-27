//! Cost calculation engine.

use serde::{Deserialize, Serialize};

use super::pricing::PricingTracker;

/// Cost estimate for a sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub provider_id: String,
    pub region_id: String,
    pub execution_cost_cents: f64,
    pub data_transfer_cents: f64,
    pub total_cents: f64,
}

/// Calculates execution costs based on current pricing.
pub struct CostCalculator {
    tracker: PricingTracker,
    data_transfer_rate: f64, // cents per MB
}

impl CostCalculator {
    pub fn new(tracker: PricingTracker) -> Self {
        Self {
            tracker,
            data_transfer_rate: 0.09, // $0.09/GB = 0.009 cents/MB
        }
    }

    pub fn with_data_rate(mut self, rate_cents_per_mb: f64) -> Self {
        self.data_transfer_rate = rate_cents_per_mb;
        self
    }

    /// Estimate cost for an execution.
    pub fn estimate(
        &self,
        provider_id: &str,
        region_id: &str,
        data_mb: f64,
    ) -> Option<CostEstimate> {
        let price = self.tracker.get_price(provider_id, region_id)?;
        let exec_cost = price.price_per_execution * 100.0; // convert to cents
        let transfer_cost = data_mb * self.data_transfer_rate;

        Some(CostEstimate {
            provider_id: provider_id.to_string(),
            region_id: region_id.to_string(),
            execution_cost_cents: exec_cost,
            data_transfer_cents: transfer_cost,
            total_cents: exec_cost + transfer_cost,
        })
    }

    /// Estimate costs across all tracked providers/regions.
    pub fn estimate_all(&self, data_mb: f64) -> Vec<CostEstimate> {
        let options = self.tracker.cheapest_options();
        options
            .iter()
            .filter_map(|p| self.estimate(&p.provider_id, &p.region_id, data_mb))
            .collect()
    }

    /// Find cheapest option.
    pub fn cheapest(&self, data_mb: f64) -> Option<CostEstimate> {
        let mut all = self.estimate_all(data_mb);
        all.sort_by(|a, b| {
            a.total_cents.partial_cmp(&b.total_cents).unwrap_or(std::cmp::Ordering::Equal)
        });
        all.into_iter().next()
    }

    /// Get pricing tracker reference.
    pub fn tracker(&self) -> &PricingTracker {
        &self.tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_tracker() -> PricingTracker {
        let t = PricingTracker::new();
        t.update_price("aws", "us-east-1", 0.005);
        t.update_price("gcp", "us-central1", 0.004);
        t.update_price("azure", "eastus", 0.006);
        t
    }

    #[test]
    fn test_estimate_cost() {
        let calc = CostCalculator::new(setup_tracker());
        let est = calc.estimate("aws", "us-east-1", 1.0).unwrap();
        assert!((est.execution_cost_cents - 0.5).abs() < 0.01);
        assert!(est.data_transfer_cents > 0.0);
        assert!(est.total_cents > est.execution_cost_cents);
    }

    #[test]
    fn test_cheapest() {
        let calc = CostCalculator::new(setup_tracker());
        let cheapest = calc.cheapest(1.0).unwrap();
        assert_eq!(cheapest.provider_id, "gcp"); // 0.004 is cheapest
    }

    #[test]
    fn test_estimate_all() {
        let calc = CostCalculator::new(setup_tracker());
        let all = calc.estimate_all(0.5);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_nonexistent_provider() {
        let calc = CostCalculator::new(PricingTracker::new());
        assert!(calc.estimate("aws", "us-east-1", 1.0).is_none());
    }

    #[test]
    fn test_custom_data_rate() {
        let calc = CostCalculator::new(setup_tracker()).with_data_rate(0.5);
        let est = calc.estimate("aws", "us-east-1", 10.0).unwrap();
        assert!((est.data_transfer_cents - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_zero_data_transfer() {
        let calc = CostCalculator::new(setup_tracker());
        let est = calc.estimate("gcp", "us-central1", 0.0).unwrap();
        assert!((est.data_transfer_cents).abs() < f64::EPSILON);
        assert!((est.total_cents - est.execution_cost_cents).abs() < f64::EPSILON);
    }
}
