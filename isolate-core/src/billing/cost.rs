use super::tenant::TenantUsage;

/// Per-unit pricing configuration for resource consumption.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnitPricing {
    /// Cost per 1M fuel units consumed.
    pub cost_per_million_fuel: f64,
    /// Cost per GB-second of memory usage.
    pub cost_per_gb_second: f64,
    /// Cost per GB of data read.
    pub cost_per_gb_read: f64,
    /// Cost per GB of data written.
    pub cost_per_gb_write: f64,
    /// Cost per 1000 executions.
    pub cost_per_thousand_executions: f64,
    /// Cost per wall-time hour.
    pub cost_per_wall_hour: f64,
}

impl Default for UnitPricing {
    fn default() -> Self {
        Self {
            cost_per_million_fuel: 0.10,
            cost_per_gb_second: 0.0000166667, // ~$0.06/GB-hour
            cost_per_gb_read: 0.01,
            cost_per_gb_write: 0.02,
            cost_per_thousand_executions: 0.20,
            cost_per_wall_hour: 0.05,
        }
    }
}

/// Tiered pricing with volume discounts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PricingTier {
    pub name: String,
    pub min_executions: u64,
    pub discount_pct: f64,
}

impl PricingTier {
    pub fn standard_tiers() -> Vec<PricingTier> {
        vec![
            PricingTier { name: "Free".into(), min_executions: 0, discount_pct: 0.0 },
            PricingTier { name: "Growth".into(), min_executions: 10_000, discount_pct: 10.0 },
            PricingTier { name: "Scale".into(), min_executions: 100_000, discount_pct: 25.0 },
            PricingTier {
                name: "Enterprise".into(),
                min_executions: 1_000_000,
                discount_pct: 40.0,
            },
        ]
    }

    /// Find the applicable tier for a given execution count.
    pub fn find_tier(tiers: &[PricingTier], executions: u64) -> Option<&PricingTier> {
        tiers.iter().rev().find(|t| executions >= t.min_executions)
    }
}

/// Itemized cost breakdown for a tenant's usage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CostBreakdown {
    pub fuel_cost: f64,
    pub memory_cost: f64,
    pub read_cost: f64,
    pub write_cost: f64,
    pub execution_cost: f64,
    pub wall_time_cost: f64,
    pub subtotal: f64,
    pub discount_pct: f64,
    pub discount_amount: f64,
    pub total_cost: f64,
}

/// Calculator that transforms tenant usage into cost breakdowns.
pub struct CostCalculator {
    pricing: UnitPricing,
    tiers: Vec<PricingTier>,
}

impl CostCalculator {
    pub fn new(pricing: UnitPricing) -> Self {
        Self { pricing, tiers: PricingTier::standard_tiers() }
    }

    pub fn with_tiers(mut self, tiers: Vec<PricingTier>) -> Self {
        self.tiers = tiers;
        self
    }

    /// Calculate cost breakdown for tenant usage.
    pub fn calculate(&self, usage: &TenantUsage) -> CostBreakdown {
        let p = &self.pricing;

        let fuel_cost = (usage.total_fuel_consumed as f64 / 1_000_000.0) * p.cost_per_million_fuel;

        // Memory cost from accumulated byte-seconds. Falls back to
        // peak_memory × total_wall_time when per-execution tracking is unavailable.
        let memory_gb_seconds = if usage.total_memory_byte_seconds > 0 {
            usage.total_memory_byte_seconds as f64 / (1024.0 * 1024.0 * 1024.0)
        } else {
            (usage.peak_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
                * (usage.total_wall_time_ms as f64 / 1000.0)
        };
        let memory_cost = memory_gb_seconds * p.cost_per_gb_second;
        let read_cost =
            (usage.total_bytes_read as f64 / (1024.0 * 1024.0 * 1024.0)) * p.cost_per_gb_read;
        let write_cost =
            (usage.total_bytes_written as f64 / (1024.0 * 1024.0 * 1024.0)) * p.cost_per_gb_write;
        let execution_cost =
            (usage.execution_count as f64 / 1000.0) * p.cost_per_thousand_executions;
        let wall_time_cost = (usage.total_wall_time_ms as f64 / 3_600_000.0) * p.cost_per_wall_hour;

        let subtotal =
            fuel_cost + memory_cost + read_cost + write_cost + execution_cost + wall_time_cost;

        let tier = PricingTier::find_tier(&self.tiers, usage.execution_count);
        let discount_pct = tier.map(|t| t.discount_pct).unwrap_or(0.0);
        let discount_amount = subtotal * (discount_pct / 100.0);
        let total_cost = subtotal - discount_amount;

        CostBreakdown {
            fuel_cost,
            memory_cost,
            read_cost,
            write_cost,
            execution_cost,
            wall_time_cost,
            subtotal,
            discount_pct,
            discount_amount,
            total_cost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::TenantId;

    fn sample_usage() -> TenantUsage {
        TenantUsage {
            tenant_id: TenantId::new("test"),
            execution_count: 10_000,
            total_wall_time_ms: 3_600_000, // 1 hour
            total_fuel_consumed: 5_000_000,
            total_bytes_read: 1024 * 1024 * 1024,   // 1 GB
            total_bytes_written: 512 * 1024 * 1024, // 0.5 GB
            peak_memory_bytes: 128 * 1024 * 1024,
            total_memory_byte_seconds: 128 * 1024 * 1024 * 3600, // 128MB for 1 hour
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 0,
        }
    }

    #[test]
    fn test_cost_calculation() {
        let calc = CostCalculator::new(UnitPricing::default());
        let cost = calc.calculate(&sample_usage());

        assert!(cost.fuel_cost > 0.0);
        assert!(cost.memory_cost > 0.0, "memory cost should be calculated from byte-seconds");
        assert!(cost.read_cost > 0.0);
        assert!(cost.write_cost > 0.0);
        assert!(cost.execution_cost > 0.0);
        assert!(cost.total_cost > 0.0);
        assert!(cost.total_cost <= cost.subtotal); // discount applied
    }

    #[test]
    fn test_tiered_discount() {
        let calc = CostCalculator::new(UnitPricing::default());

        let mut usage = sample_usage();
        usage.execution_count = 5; // Free tier, 0% discount
        let free_cost = calc.calculate(&usage);
        assert_eq!(free_cost.discount_pct, 0.0);

        usage.execution_count = 100_000; // Scale tier, 25% discount
        let scale_cost = calc.calculate(&usage);
        assert_eq!(scale_cost.discount_pct, 25.0);
    }

    #[test]
    fn test_zero_usage() {
        let calc = CostCalculator::new(UnitPricing::default());
        let usage = TenantUsage {
            tenant_id: TenantId::new("empty"),
            execution_count: 0,
            total_wall_time_ms: 0,
            total_fuel_consumed: 0,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 0,
            total_memory_byte_seconds: 0,
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 0,
        };
        let cost = calc.calculate(&usage);
        assert_eq!(cost.total_cost, 0.0);
        assert_eq!(cost.memory_cost, 0.0);
    }

    #[test]
    fn test_custom_tiers() {
        let calc = CostCalculator::new(UnitPricing::default()).with_tiers(vec![PricingTier {
            name: "VIP".into(),
            min_executions: 0,
            discount_pct: 50.0,
        }]);
        let cost = calc.calculate(&sample_usage());
        assert_eq!(cost.discount_pct, 50.0);
    }

    #[test]
    fn test_memory_cost_from_byte_seconds() {
        let calc = CostCalculator::new(UnitPricing::default());
        // 1 GB for 1 second = 1 GB-second
        let usage = TenantUsage {
            tenant_id: TenantId::new("mem"),
            execution_count: 1,
            total_wall_time_ms: 1000,
            total_fuel_consumed: 0,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 1024 * 1024 * 1024,
            total_memory_byte_seconds: 1024 * 1024 * 1024, // 1 GB × 1 second
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 0,
        };
        let cost = calc.calculate(&usage);
        let expected_memory_cost = UnitPricing::default().cost_per_gb_second;
        assert!((cost.memory_cost - expected_memory_cost).abs() < 1e-10);
    }

    #[test]
    fn test_memory_cost_fallback_from_peak() {
        let calc = CostCalculator::new(UnitPricing::default());
        // When total_memory_byte_seconds is 0, fall back to peak × wall_time
        let usage = TenantUsage {
            tenant_id: TenantId::new("fallback"),
            execution_count: 1,
            total_wall_time_ms: 1000,
            total_fuel_consumed: 0,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 1024 * 1024 * 1024, // 1 GB peak
            total_memory_byte_seconds: 0,          // no per-execution tracking
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 0,
        };
        let cost = calc.calculate(&usage);
        let expected = UnitPricing::default().cost_per_gb_second;
        assert!((cost.memory_cost - expected).abs() < 1e-10);
    }
}
