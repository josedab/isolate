//! # Multi-Tenancy Billing & Metering
//!
//! Fine-grained resource usage tracking with per-tenant cost allocation,
//! billing provider integration, and usage dashboards.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌───────────────┐
//! │ ResourceMeter│────▶│ BillingMeter │────▶│ CostCalculator│
//! └─────────────┘     └──────────────┘     └───────────────┘
//!                            │
//!                            ▼
//!                     ┌──────────────┐     ┌───────────────┐
//!                     │ TenantUsage  │────▶│ UsageReport   │
//!                     └──────────────┘     └───────────────┘
//! ```

#![allow(missing_docs)]
mod cost;
mod meter;
mod report;
mod tenant;

pub use cost::{CostCalculator, PricingTier, UnitPricing};
pub use meter::{BillingEvent, BillingMeter, SharedBillingMeter};
pub use report::{UsageReport, UsageReportBuilder, UsageSummary};
pub use tenant::{TenantId, TenantUsage, TenantUsageTracker};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_end_to_end_billing_flow() {
        let pricing = UnitPricing::default();
        let calculator = CostCalculator::new(pricing);
        let tracker = TenantUsageTracker::new();

        let tenant = TenantId::new("tenant-1");
        tracker.record_execution(&tenant, Duration::from_millis(500), 1_000_000, 4096, 1024);
        tracker.record_execution(&tenant, Duration::from_millis(300), 500_000, 2048, 512);

        let usage = tracker.get_usage(&tenant).unwrap();
        assert_eq!(usage.execution_count, 2);
        assert_eq!(usage.total_fuel_consumed, 1_500_000);

        let cost = calculator.calculate(&usage);
        assert!(cost.total_cost > 0.0);
    }

    #[test]
    fn test_multi_tenant_isolation() {
        let tracker = TenantUsageTracker::new();
        let t1 = TenantId::new("t1");
        let t2 = TenantId::new("t2");

        tracker.record_execution(&t1, Duration::from_secs(1), 100, 50, 25);
        tracker.record_execution(&t2, Duration::from_secs(2), 200, 100, 50);

        let u1 = tracker.get_usage(&t1).unwrap();
        let u2 = tracker.get_usage(&t2).unwrap();

        assert_eq!(u1.total_fuel_consumed, 100);
        assert_eq!(u2.total_fuel_consumed, 200);
        assert_eq!(u1.execution_count, 1);
        assert_eq!(u2.execution_count, 1);
    }

    #[test]
    fn test_usage_report_generation() {
        let tracker = TenantUsageTracker::new();
        let tenant = TenantId::new("report-tenant");
        tracker.record_execution(&tenant, Duration::from_millis(100), 50_000, 1024, 512);

        let usage = tracker.get_usage(&tenant).unwrap();
        let pricing = UnitPricing::default();
        let calculator = CostCalculator::new(pricing);

        let report = UsageReportBuilder::new(tenant.clone(), usage)
            .with_cost(calculator.calculate(&tracker.get_usage(&tenant).unwrap()))
            .build();

        assert_eq!(report.tenant_id, tenant);
        assert_eq!(report.usage.execution_count, 1);
    }
}
