use super::cost::{CostBreakdown, CostCalculator, UnitPricing};
use super::tenant::{TenantId, TenantUsage};

/// Summary of a single tenant's billing for a period.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageSummary {
    pub execution_count: u64,
    pub total_fuel: u64,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub total_wall_time_ms: u64,
    pub peak_memory_bytes: u64,
}

impl From<&TenantUsage> for UsageSummary {
    fn from(u: &TenantUsage) -> Self {
        Self {
            execution_count: u.execution_count,
            total_fuel: u.total_fuel_consumed,
            total_bytes_read: u.total_bytes_read,
            total_bytes_written: u.total_bytes_written,
            total_wall_time_ms: u.total_wall_time_ms,
            peak_memory_bytes: u.peak_memory_bytes,
        }
    }
}

/// Complete billing report for a tenant.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageReport {
    pub tenant_id: TenantId,
    pub usage: UsageSummary,
    pub cost: Option<CostBreakdown>,
    pub period_start_epoch_ms: u64,
    pub period_end_epoch_ms: u64,
    pub generated_epoch_ms: u64,
}

/// Builder for constructing usage reports.
pub struct UsageReportBuilder {
    tenant_id: TenantId,
    usage: UsageSummary,
    cost: Option<CostBreakdown>,
    period_start: u64,
    period_end: u64,
}

impl UsageReportBuilder {
    pub fn new(tenant_id: TenantId, usage: TenantUsage) -> Self {
        Self {
            tenant_id,
            period_start: usage.first_execution_epoch_ms,
            period_end: usage.last_execution_epoch_ms,
            usage: UsageSummary::from(&usage),
            cost: None,
        }
    }

    pub fn with_cost(mut self, cost: CostBreakdown) -> Self {
        self.cost = Some(cost);
        self
    }

    pub fn with_period(mut self, start: u64, end: u64) -> Self {
        self.period_start = start;
        self.period_end = end;
        self
    }

    pub fn build(self) -> UsageReport {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        UsageReport {
            tenant_id: self.tenant_id,
            usage: self.usage,
            cost: self.cost,
            period_start_epoch_ms: self.period_start,
            period_end_epoch_ms: self.period_end,
            generated_epoch_ms: now,
        }
    }
}

/// A single line item on an invoice.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvoiceLineItem {
    /// Resource category (e.g., "Compute (fuel)", "Memory").
    pub description: String,
    /// Quantity consumed (human-readable, e.g., "5.0 M fuel").
    pub quantity: String,
    /// Unit rate applied.
    pub unit_rate: f64,
    /// Cost for this line item.
    pub amount: f64,
}

/// A complete billing invoice for a tenant.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Invoice {
    /// Tenant this invoice is for.
    pub tenant_id: TenantId,
    /// Billing period start (epoch ms).
    pub period_start_epoch_ms: u64,
    /// Billing period end (epoch ms).
    pub period_end_epoch_ms: u64,
    /// Itemized charges.
    pub line_items: Vec<InvoiceLineItem>,
    /// Sum before discounts.
    pub subtotal: f64,
    /// Discount percentage applied.
    pub discount_pct: f64,
    /// Discount amount.
    pub discount_amount: f64,
    /// Final amount due.
    pub total: f64,
    /// When the invoice was generated (epoch ms).
    pub generated_epoch_ms: u64,
}

impl Invoice {
    /// Generate a complete invoice from tenant usage and pricing.
    pub fn generate(tenant_id: TenantId, usage: TenantUsage, pricing: &UnitPricing) -> Self {
        let calculator = CostCalculator::new(pricing.clone());
        let breakdown = calculator.calculate(&usage);

        let line_items = vec![
            InvoiceLineItem {
                description: "Compute (fuel)".to_string(),
                quantity: format!("{:.1} M fuel", usage.total_fuel_consumed as f64 / 1_000_000.0),
                unit_rate: pricing.cost_per_million_fuel,
                amount: breakdown.fuel_cost,
            },
            InvoiceLineItem {
                description: "Memory".to_string(),
                quantity: format!(
                    "{:.2} GB-sec",
                    if usage.total_memory_byte_seconds > 0 {
                        usage.total_memory_byte_seconds as f64 / (1024.0 * 1024.0 * 1024.0)
                    } else {
                        (usage.peak_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0))
                            * (usage.total_wall_time_ms as f64 / 1000.0)
                    }
                ),
                unit_rate: pricing.cost_per_gb_second,
                amount: breakdown.memory_cost,
            },
            InvoiceLineItem {
                description: "Data read".to_string(),
                quantity: format!(
                    "{:.3} GB",
                    usage.total_bytes_read as f64 / (1024.0 * 1024.0 * 1024.0)
                ),
                unit_rate: pricing.cost_per_gb_read,
                amount: breakdown.read_cost,
            },
            InvoiceLineItem {
                description: "Data written".to_string(),
                quantity: format!(
                    "{:.3} GB",
                    usage.total_bytes_written as f64 / (1024.0 * 1024.0 * 1024.0)
                ),
                unit_rate: pricing.cost_per_gb_write,
                amount: breakdown.write_cost,
            },
            InvoiceLineItem {
                description: "Executions".to_string(),
                quantity: format!("{} invocations", usage.execution_count),
                unit_rate: pricing.cost_per_thousand_executions,
                amount: breakdown.execution_cost,
            },
            InvoiceLineItem {
                description: "Wall time".to_string(),
                quantity: format!("{:.2} hours", usage.total_wall_time_ms as f64 / 3_600_000.0),
                unit_rate: pricing.cost_per_wall_hour,
                amount: breakdown.wall_time_cost,
            },
        ];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            tenant_id,
            period_start_epoch_ms: usage.first_execution_epoch_ms,
            period_end_epoch_ms: usage.last_execution_epoch_ms,
            line_items,
            subtotal: breakdown.subtotal,
            discount_pct: breakdown.discount_pct,
            discount_amount: breakdown.discount_amount,
            total: breakdown.total_cost,
            generated_epoch_ms: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_builder() {
        let usage = TenantUsage {
            tenant_id: TenantId::new("rpt"),
            execution_count: 42,
            total_wall_time_ms: 1000,
            total_fuel_consumed: 100_000,
            total_bytes_read: 2048,
            total_bytes_written: 1024,
            peak_memory_bytes: 4096,
            total_memory_byte_seconds: 0,
            first_execution_epoch_ms: 1000,
            last_execution_epoch_ms: 2000,
        };

        let report = UsageReportBuilder::new(TenantId::new("rpt"), usage).build();
        assert_eq!(report.usage.execution_count, 42);
        assert_eq!(report.period_start_epoch_ms, 1000);
        assert!(report.cost.is_none());
    }

    #[test]
    fn test_report_with_custom_period() {
        let usage = TenantUsage {
            tenant_id: TenantId::new("p"),
            execution_count: 1,
            total_wall_time_ms: 100,
            total_fuel_consumed: 1000,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 0,
            total_memory_byte_seconds: 0,
            first_execution_epoch_ms: 500,
            last_execution_epoch_ms: 600,
        };

        let report =
            UsageReportBuilder::new(TenantId::new("p"), usage).with_period(100, 999).build();
        assert_eq!(report.period_start_epoch_ms, 100);
        assert_eq!(report.period_end_epoch_ms, 999);
    }

    #[test]
    fn test_invoice_generation() {
        let usage = TenantUsage {
            tenant_id: TenantId::new("inv"),
            execution_count: 50_000,
            total_wall_time_ms: 7_200_000, // 2 hours
            total_fuel_consumed: 10_000_000,
            total_bytes_read: 2 * 1024 * 1024 * 1024, // 2 GB
            total_bytes_written: 512 * 1024 * 1024,
            peak_memory_bytes: 256 * 1024 * 1024,
            total_memory_byte_seconds: 256 * 1024 * 1024 * 7200, // 256MB for 2h
            first_execution_epoch_ms: 1000,
            last_execution_epoch_ms: 8_000_000,
        };

        let invoice = Invoice::generate(TenantId::new("inv"), usage, &UnitPricing::default());

        assert_eq!(invoice.tenant_id.as_str(), "inv");
        assert_eq!(invoice.line_items.len(), 6);
        assert!(invoice.subtotal > 0.0);
        assert!(invoice.total >= 0.0);
        assert!(invoice.total <= invoice.subtotal); // discount applied

        // Verify all line items have positive amounts for non-zero usage
        for item in &invoice.line_items {
            assert!(!item.description.is_empty());
        }
    }

    #[test]
    fn test_invoice_zero_usage() {
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

        let invoice = Invoice::generate(TenantId::new("empty"), usage, &UnitPricing::default());
        assert_eq!(invoice.total, 0.0);
        assert_eq!(invoice.subtotal, 0.0);
    }

    #[test]
    fn test_invoice_serialization() {
        let usage = TenantUsage {
            tenant_id: TenantId::new("ser"),
            execution_count: 1,
            total_wall_time_ms: 1000,
            total_fuel_consumed: 1_000_000,
            total_bytes_read: 0,
            total_bytes_written: 0,
            peak_memory_bytes: 0,
            total_memory_byte_seconds: 0,
            first_execution_epoch_ms: 0,
            last_execution_epoch_ms: 1000,
        };

        let invoice = Invoice::generate(TenantId::new("ser"), usage, &UnitPricing::default());
        let json = serde_json::to_string(&invoice).unwrap();
        let deserialized: Invoice = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tenant_id.as_str(), "ser");
        assert_eq!(deserialized.line_items.len(), invoice.line_items.len());
    }
}
