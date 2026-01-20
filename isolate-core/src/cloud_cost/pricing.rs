//! Spot pricing tracker.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// A price point for a provider/region combination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricePoint {
    pub provider_id: String,
    pub region_id: String,
    pub price_per_execution: f64,
    pub timestamp: u64,
    pub tier: PricingTier,
}

/// Pricing tier (on-demand vs. spot vs. reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingTier {
    OnDemand,
    Spot,
    Reserved,
}

impl Default for PricingTier {
    fn default() -> Self {
        Self::OnDemand
    }
}

/// Tracks pricing across providers and regions.
#[derive(Clone)]
pub struct PricingTracker {
    inner: Arc<PricingTrackerInner>,
}

struct PricingTrackerInner {
    /// Key: "provider:region"
    current_prices: RwLock<HashMap<String, PricePoint>>,
    /// Historical prices for trend analysis.
    history: RwLock<Vec<PricePoint>>,
}

impl PricingTracker {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PricingTrackerInner {
                current_prices: RwLock::new(HashMap::new()),
                history: RwLock::new(Vec::new()),
            }),
        }
    }

    /// Update the current price for a provider/region.
    pub fn update_price(&self, provider_id: &str, region_id: &str, price: f64) {
        let key = format!("{}:{}", provider_id, region_id);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let point = PricePoint {
            provider_id: provider_id.to_string(),
            region_id: region_id.to_string(),
            price_per_execution: price,
            timestamp: ts,
            tier: PricingTier::OnDemand,
        };

        self.inner.current_prices.write().insert(key, point.clone());
        self.inner.history.write().push(point);
    }

    /// Update with explicit tier and timestamp.
    pub fn update_price_full(&self, provider_id: &str, region_id: &str, price: f64, tier: PricingTier, timestamp: u64) {
        let key = format!("{}:{}", provider_id, region_id);
        let point = PricePoint {
            provider_id: provider_id.to_string(),
            region_id: region_id.to_string(),
            price_per_execution: price,
            timestamp,
            tier,
        };

        self.inner.current_prices.write().insert(key, point.clone());
        self.inner.history.write().push(point);
    }

    /// Get current price for a provider/region.
    pub fn get_price(&self, provider_id: &str, region_id: &str) -> Option<PricePoint> {
        let key = format!("{}:{}", provider_id, region_id);
        self.inner.current_prices.read().get(&key).cloned()
    }

    /// Get all current prices sorted by cost (cheapest first).
    pub fn cheapest_options(&self) -> Vec<PricePoint> {
        let prices = self.inner.current_prices.read();
        let mut sorted: Vec<PricePoint> = prices.values().cloned().collect();
        sorted.sort_by(|a, b| a.price_per_execution.partial_cmp(&b.price_per_execution).unwrap_or(std::cmp::Ordering::Equal));
        sorted
    }

    /// Get average price for a provider across all regions.
    pub fn average_price(&self, provider_id: &str) -> Option<f64> {
        let prices = self.inner.current_prices.read();
        let provider_prices: Vec<f64> = prices.values()
            .filter(|p| p.provider_id == provider_id)
            .map(|p| p.price_per_execution)
            .collect();

        if provider_prices.is_empty() {
            None
        } else {
            Some(provider_prices.iter().sum::<f64>() / provider_prices.len() as f64)
        }
    }

    /// Number of tracked price points.
    pub fn tracked_count(&self) -> usize {
        self.inner.current_prices.read().len()
    }

    /// Price history length.
    pub fn history_len(&self) -> usize {
        self.inner.history.read().len()
    }
}

impl Default for PricingTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_and_get_price() {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.005);
        let price = tracker.get_price("aws", "us-east-1").unwrap();
        assert!((price.price_per_execution - 0.005).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cheapest_options() {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.010);
        tracker.update_price("gcp", "us-central1", 0.005);
        tracker.update_price("azure", "eastus", 0.008);

        let options = tracker.cheapest_options();
        assert_eq!(options.len(), 3);
        assert_eq!(options[0].provider_id, "gcp"); // cheapest
    }

    #[test]
    fn test_average_price() {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.010);
        tracker.update_price("aws", "eu-west-1", 0.012);

        let avg = tracker.average_price("aws").unwrap();
        assert!((avg - 0.011).abs() < 0.001);
    }

    #[test]
    fn test_nonexistent_price() {
        let tracker = PricingTracker::new();
        assert!(tracker.get_price("aws", "us-east-1").is_none());
        assert!(tracker.average_price("aws").is_none());
    }

    #[test]
    fn test_price_update_replaces() {
        let tracker = PricingTracker::new();
        tracker.update_price("aws", "us-east-1", 0.010);
        tracker.update_price("aws", "us-east-1", 0.005);

        let price = tracker.get_price("aws", "us-east-1").unwrap();
        assert!((price.price_per_execution - 0.005).abs() < f64::EPSILON);
        assert_eq!(tracker.tracked_count(), 1);
        assert_eq!(tracker.history_len(), 2); // history keeps both
    }

    #[test]
    fn test_pricing_tiers() {
        let tracker = PricingTracker::new();
        tracker.update_price_full("aws", "us-east-1", 0.003, PricingTier::Spot, 1000);
        let price = tracker.get_price("aws", "us-east-1").unwrap();
        assert_eq!(price.tier, PricingTier::Spot);
    }
}
