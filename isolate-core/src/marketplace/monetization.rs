//! Marketplace monetization engine.
//!
//! Listing pricing, usage metering, and revenue sharing for module publishers.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Pricing model for a marketplace listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PricingModel {
    /// Free and open source.
    Free,
    /// One-time purchase.
    OneTime { price_cents: u64 },
    /// Recurring subscription per period.
    Subscription { price_cents_per_month: u64 },
    /// Pay per invocation.
    UsageBased { price_cents_per_1k_invocations: u64 },
}

impl Default for PricingModel {
    fn default() -> Self {
        Self::Free
    }
}

/// Revenue share configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueShare {
    /// Publisher's share as a percentage (0.0-1.0).
    pub publisher_share: f64,
    /// Platform's share as a percentage (0.0-1.0).
    pub platform_share: f64,
}

impl Default for RevenueShare {
    fn default() -> Self {
        Self {
            publisher_share: 0.70,
            platform_share: 0.30,
        }
    }
}

/// A listing in the marketplace with pricing info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonetizedListing {
    pub module_id: String,
    pub publisher_id: String,
    pub pricing: PricingModel,
    pub revenue_share: RevenueShare,
    pub total_revenue_cents: u64,
    pub total_purchases: u64,
}

/// Usage record for metering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub module_id: String,
    pub buyer_id: String,
    pub invocations: u64,
    pub timestamp: u64,
}

/// Revenue breakdown for a publisher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherPayout {
    pub publisher_id: String,
    pub total_revenue_cents: u64,
    pub publisher_earnings_cents: u64,
    pub platform_fee_cents: u64,
    pub modules: Vec<String>,
}

/// Monetization engine managing pricing, usage, and revenue.
#[derive(Clone)]
pub struct MonetizationEngine {
    inner: Arc<MonetizationInner>,
}

struct MonetizationInner {
    listings: RwLock<HashMap<String, MonetizedListing>>,
    usage: RwLock<Vec<UsageRecord>>,
    default_revenue_share: RevenueShare,
}

impl MonetizationEngine {
    pub fn new(default_revenue_share: RevenueShare) -> Self {
        Self {
            inner: Arc::new(MonetizationInner {
                listings: RwLock::new(HashMap::new()),
                usage: RwLock::new(Vec::new()),
                default_revenue_share,
            }),
        }
    }

    /// Create a new monetized listing.
    pub fn create_listing(&self, module_id: &str, publisher_id: &str, pricing: PricingModel) {
        self.inner.listings.write().insert(
            module_id.to_string(),
            MonetizedListing {
                module_id: module_id.to_string(),
                publisher_id: publisher_id.to_string(),
                pricing,
                revenue_share: self.inner.default_revenue_share.clone(),
                total_revenue_cents: 0,
                total_purchases: 0,
            },
        );
    }

    /// Record a purchase and calculate revenue.
    pub fn record_purchase(&self, module_id: &str) -> Option<u64> {
        let mut listings = self.inner.listings.write();
        let listing = listings.get_mut(module_id)?;

        let amount = match &listing.pricing {
            PricingModel::Free => 0,
            PricingModel::OneTime { price_cents } => *price_cents,
            PricingModel::Subscription { price_cents_per_month } => *price_cents_per_month,
            PricingModel::UsageBased { .. } => 0, // billed via usage
        };

        listing.total_revenue_cents += amount;
        listing.total_purchases += 1;
        Some(amount)
    }

    /// Record usage for metered billing.
    pub fn record_usage(&self, module_id: &str, buyer_id: &str, invocations: u64, timestamp: u64) {
        self.inner.usage.write().push(UsageRecord {
            module_id: module_id.to_string(),
            buyer_id: buyer_id.to_string(),
            invocations,
            timestamp,
        });

        // Also update revenue if usage-based
        let mut listings = self.inner.listings.write();
        if let Some(listing) = listings.get_mut(module_id) {
            if let PricingModel::UsageBased { price_cents_per_1k_invocations } = &listing.pricing {
                let cost = (invocations as f64 / 1000.0 * *price_cents_per_1k_invocations as f64) as u64;
                listing.total_revenue_cents += cost;
            }
        }
    }

    /// Get listing info.
    pub fn get_listing(&self, module_id: &str) -> Option<MonetizedListing> {
        self.inner.listings.read().get(module_id).cloned()
    }

    /// Calculate payout for a publisher across all their modules.
    pub fn calculate_payout(&self, publisher_id: &str) -> PublisherPayout {
        let listings = self.inner.listings.read();
        let mut total_revenue = 0u64;
        let mut modules = Vec::new();

        for listing in listings.values() {
            if listing.publisher_id == publisher_id {
                total_revenue += listing.total_revenue_cents;
                modules.push(listing.module_id.clone());
            }
        }

        let share = listings
            .values()
            .find(|l| l.publisher_id == publisher_id)
            .map(|l| l.revenue_share.publisher_share)
            .unwrap_or(self.inner.default_revenue_share.publisher_share);

        let publisher_earnings = (total_revenue as f64 * share) as u64;

        PublisherPayout {
            publisher_id: publisher_id.to_string(),
            total_revenue_cents: total_revenue,
            publisher_earnings_cents: publisher_earnings,
            platform_fee_cents: total_revenue - publisher_earnings,
            modules,
        }
    }

    /// Get total platform revenue across all listings.
    pub fn total_platform_revenue(&self) -> u64 {
        self.inner.listings.read().values().map(|l| l.total_revenue_cents).sum()
    }

    /// Get usage records for a specific module.
    pub fn usage_for_module(&self, module_id: &str) -> Vec<UsageRecord> {
        self.inner.usage.read().iter()
            .filter(|u| u.module_id == module_id)
            .cloned()
            .collect()
    }

    /// Count all listings.
    pub fn listing_count(&self) -> usize {
        self.inner.listings.read().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_listing() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("free-mod", "pub-1", PricingModel::Free);
        let amount = engine.record_purchase("free-mod").unwrap();
        assert_eq!(amount, 0);
    }

    #[test]
    fn test_one_time_purchase() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("paid-mod", "pub-1", PricingModel::OneTime { price_cents: 9900 });
        let amount = engine.record_purchase("paid-mod").unwrap();
        assert_eq!(amount, 9900);

        let listing = engine.get_listing("paid-mod").unwrap();
        assert_eq!(listing.total_revenue_cents, 9900);
        assert_eq!(listing.total_purchases, 1);
    }

    #[test]
    fn test_usage_based_billing() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("metered", "pub-2", PricingModel::UsageBased {
            price_cents_per_1k_invocations: 50,
        });

        engine.record_usage("metered", "buyer-1", 5000, 1000);
        let listing = engine.get_listing("metered").unwrap();
        assert_eq!(listing.total_revenue_cents, 250); // 5000/1000 * 50

        let usage = engine.usage_for_module("metered");
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].invocations, 5000);
    }

    #[test]
    fn test_publisher_payout() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("mod-a", "alice", PricingModel::OneTime { price_cents: 10000 });
        engine.create_listing("mod-b", "alice", PricingModel::OneTime { price_cents: 5000 });
        engine.record_purchase("mod-a");
        engine.record_purchase("mod-b");

        let payout = engine.calculate_payout("alice");
        assert_eq!(payout.total_revenue_cents, 15000);
        assert_eq!(payout.publisher_earnings_cents, 10500); // 70%
        assert_eq!(payout.platform_fee_cents, 4500); // 30%
        assert_eq!(payout.modules.len(), 2);
    }

    #[test]
    fn test_nonexistent_listing() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        assert!(engine.record_purchase("nope").is_none());
        assert!(engine.get_listing("nope").is_none());
    }

    #[test]
    fn test_total_platform_revenue() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("x", "p1", PricingModel::OneTime { price_cents: 1000 });
        engine.create_listing("y", "p2", PricingModel::OneTime { price_cents: 2000 });
        engine.record_purchase("x");
        engine.record_purchase("y");
        assert_eq!(engine.total_platform_revenue(), 3000);
    }

    #[test]
    fn test_subscription_pricing() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        engine.create_listing("sub", "pub", PricingModel::Subscription {
            price_cents_per_month: 2900,
        });
        engine.record_purchase("sub");
        engine.record_purchase("sub");
        let listing = engine.get_listing("sub").unwrap();
        assert_eq!(listing.total_revenue_cents, 5800);
        assert_eq!(listing.total_purchases, 2);
    }

    #[test]
    fn test_listing_count() {
        let engine = MonetizationEngine::new(RevenueShare::default());
        assert_eq!(engine.listing_count(), 0);
        engine.create_listing("a", "p", PricingModel::Free);
        engine.create_listing("b", "p", PricingModel::Free);
        assert_eq!(engine.listing_count(), 2);
    }
}
