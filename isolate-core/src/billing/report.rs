use super::cost::CostBreakdown;
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
            first_execution_epoch_ms: 500,
            last_execution_epoch_ms: 600,
        };

        let report = UsageReportBuilder::new(TenantId::new("p"), usage)
            .with_period(100, 999)
            .build();
        assert_eq!(report.period_start_epoch_ms, 100);
        assert_eq!(report.period_end_epoch_ms, 999);
    }
}
