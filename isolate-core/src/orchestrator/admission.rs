//! Admission control and quota budget tracking for multi-tenant orchestration.
//!
//! Provides hard enforcement of resource quotas with time-windowed budget
//! accounting (CPU-seconds, memory-byte-seconds, sandbox-count).
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::orchestrator::admission::{AdmissionController, QuotaBudget};
//!
//! let mut controller = AdmissionController::new();
//! controller.set_budget("tenant-a", QuotaBudget {
//!     cpu_seconds_per_hour: 3600.0,
//!     memory_gb_seconds_per_hour: 128.0,
//!     max_sandboxes_per_hour: 1000,
//!     ..Default::default()
//! });
//!
//! // Check before admitting a new sandbox
//! let decision = controller.check("tenant-a", &request)?;
//! if decision.admitted {
//!     // proceed with sandbox creation
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Quota budget for a tenant, defining resource limits per time window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaBudget {
    /// Maximum CPU-seconds per hour.
    pub cpu_seconds_per_hour: f64,
    /// Maximum memory-GB-seconds per hour.
    pub memory_gb_seconds_per_hour: f64,
    /// Maximum sandbox creations per hour.
    pub max_sandboxes_per_hour: u64,
    /// Maximum concurrent sandboxes at any instant.
    pub max_concurrent: u32,
    /// Maximum single sandbox memory in bytes.
    pub max_sandbox_memory: u64,
    /// Maximum single sandbox fuel.
    pub max_sandbox_fuel: Option<u64>,
    /// Burst allowance: percentage over budget allowed temporarily (0.0 = no burst).
    pub burst_allowance: f64,
}

impl Default for QuotaBudget {
    fn default() -> Self {
        Self {
            cpu_seconds_per_hour: 3600.0,
            memory_gb_seconds_per_hour: 128.0,
            max_sandboxes_per_hour: 1000,
            max_concurrent: 10,
            max_sandbox_memory: 256 * 1024 * 1024, // 256 MB
            max_sandbox_fuel: None,
            burst_allowance: 0.1, // 10% burst
        }
    }
}

/// A resource request for admission control.
#[derive(Debug, Clone)]
pub struct AdmissionRequest {
    /// Requested memory in bytes.
    pub memory_bytes: u64,
    /// Requested fuel (CPU budget).
    pub fuel: Option<u64>,
    /// Estimated duration.
    pub estimated_duration: Option<Duration>,
    /// Priority override (higher = more important).
    pub priority: u8,
    /// Request metadata for audit.
    pub labels: HashMap<String, String>,
}

impl Default for AdmissionRequest {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024, // 64 MB
            fuel: None,
            estimated_duration: None,
            priority: 5,
            labels: HashMap::new(),
        }
    }
}

/// Result of an admission check.
#[derive(Debug, Clone)]
pub struct AdmissionDecision {
    /// Whether the request was admitted.
    pub admitted: bool,
    /// Reason for denial (if not admitted).
    pub denial_reason: Option<DenialReason>,
    /// Current quota usage as a percentage (0.0 - 1.0+).
    pub quota_usage: QuotaUsage,
    /// Wait estimate if queued.
    pub estimated_wait: Option<Duration>,
}

/// Reason a request was denied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DenialReason {
    /// Tenant not registered.
    TenantNotFound,
    /// Tenant is suspended.
    TenantSuspended,
    /// Concurrent sandbox limit reached.
    ConcurrentLimitReached { current: u32, limit: u32 },
    /// Hourly sandbox count exhausted.
    HourlySandboxLimitReached { current: u64, limit: u64 },
    /// CPU budget exhausted.
    CpuBudgetExhausted { used: f64, limit: f64 },
    /// Memory budget exhausted.
    MemoryBudgetExhausted { used: f64, limit: f64 },
    /// Single sandbox exceeds memory limit.
    SandboxMemoryExceedsLimit { requested: u64, limit: u64 },
    /// Single sandbox exceeds fuel limit.
    SandboxFuelExceedsLimit { requested: u64, limit: u64 },
}

impl std::fmt::Display for DenialReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TenantNotFound => write!(f, "tenant not found"),
            Self::TenantSuspended => write!(f, "tenant is suspended"),
            Self::ConcurrentLimitReached { current, limit } => {
                write!(f, "concurrent limit reached ({}/{})", current, limit)
            }
            Self::HourlySandboxLimitReached { current, limit } => {
                write!(f, "hourly sandbox limit reached ({}/{})", current, limit)
            }
            Self::CpuBudgetExhausted { used, limit } => {
                write!(f, "CPU budget exhausted ({:.1}s / {:.1}s)", used, limit)
            }
            Self::MemoryBudgetExhausted { used, limit } => {
                write!(f, "memory budget exhausted ({:.1} GB·s / {:.1} GB·s)", used, limit)
            }
            Self::SandboxMemoryExceedsLimit { requested, limit } => {
                write!(f, "sandbox memory {} exceeds limit {}", requested, limit)
            }
            Self::SandboxFuelExceedsLimit { requested, limit } => {
                write!(f, "sandbox fuel {} exceeds limit {}", requested, limit)
            }
        }
    }
}

/// Current quota usage fractions for a tenant.
#[derive(Debug, Clone, Default)]
pub struct QuotaUsage {
    /// CPU-seconds used / limit (0.0 - 1.0+).
    pub cpu_fraction: f64,
    /// Memory-GB-seconds used / limit (0.0 - 1.0+).
    pub memory_fraction: f64,
    /// Sandboxes created this window / limit (0.0 - 1.0+).
    pub sandbox_count_fraction: f64,
    /// Current concurrent / limit (0.0 - 1.0+).
    pub concurrent_fraction: f64,
}

impl QuotaUsage {
    /// Get the highest usage fraction across all dimensions.
    pub fn peak(&self) -> f64 {
        self.cpu_fraction
            .max(self.memory_fraction)
            .max(self.sandbox_count_fraction)
            .max(self.concurrent_fraction)
    }

    /// Check if any dimension is over budget.
    pub fn is_over_budget(&self) -> bool {
        self.peak() > 1.0
    }
}

/// Internal accounting state for a tenant.
struct TenantAccounting {
    budget: QuotaBudget,
    window_start: Instant,
    cpu_seconds_used: f64,
    memory_gb_seconds_used: f64,
    sandbox_count_this_window: u64,
    active_sandboxes: u32,
    suspended: bool,
}

impl TenantAccounting {
    fn new(budget: QuotaBudget) -> Self {
        Self {
            budget,
            window_start: Instant::now(),
            cpu_seconds_used: 0.0,
            memory_gb_seconds_used: 0.0,
            sandbox_count_this_window: 0,
            active_sandboxes: 0,
            suspended: false,
        }
    }

    fn maybe_reset_window(&mut self) {
        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_secs(3600) {
            self.window_start = Instant::now();
            self.cpu_seconds_used = 0.0;
            self.memory_gb_seconds_used = 0.0;
            self.sandbox_count_this_window = 0;
        }
    }

    fn effective_limit(&self, base: f64) -> f64 {
        base * (1.0 + self.budget.burst_allowance)
    }

    fn usage(&self) -> QuotaUsage {
        QuotaUsage {
            cpu_fraction: if self.budget.cpu_seconds_per_hour > 0.0 {
                self.cpu_seconds_used / self.budget.cpu_seconds_per_hour
            } else {
                0.0
            },
            memory_fraction: if self.budget.memory_gb_seconds_per_hour > 0.0 {
                self.memory_gb_seconds_used / self.budget.memory_gb_seconds_per_hour
            } else {
                0.0
            },
            sandbox_count_fraction: if self.budget.max_sandboxes_per_hour > 0 {
                self.sandbox_count_this_window as f64 / self.budget.max_sandboxes_per_hour as f64
            } else {
                0.0
            },
            concurrent_fraction: if self.budget.max_concurrent > 0 {
                self.active_sandboxes as f64 / self.budget.max_concurrent as f64
            } else {
                0.0
            },
        }
    }
}

/// Admission controller enforcing resource quotas for multi-tenant workloads.
pub struct AdmissionController {
    tenants: HashMap<String, TenantAccounting>,
}

impl Default for AdmissionController {
    fn default() -> Self {
        Self::new()
    }
}

impl AdmissionController {
    /// Create a new admission controller.
    pub fn new() -> Self {
        Self { tenants: HashMap::new() }
    }

    /// Set or update the quota budget for a tenant.
    pub fn set_budget(&mut self, tenant_id: &str, budget: QuotaBudget) {
        let entry = self
            .tenants
            .entry(tenant_id.to_string())
            .or_insert_with(|| TenantAccounting::new(budget.clone()));
        entry.budget = budget;
    }

    /// Remove a tenant.
    pub fn remove_tenant(&mut self, tenant_id: &str) {
        self.tenants.remove(tenant_id);
    }

    /// Suspend a tenant (all requests will be denied).
    pub fn suspend_tenant(&mut self, tenant_id: &str) -> bool {
        if let Some(acct) = self.tenants.get_mut(tenant_id) {
            acct.suspended = true;
            true
        } else {
            false
        }
    }

    /// Resume a suspended tenant.
    pub fn resume_tenant(&mut self, tenant_id: &str) -> bool {
        if let Some(acct) = self.tenants.get_mut(tenant_id) {
            acct.suspended = false;
            true
        } else {
            false
        }
    }

    /// Check whether a request should be admitted.
    pub fn check(&mut self, tenant_id: &str, request: &AdmissionRequest) -> AdmissionDecision {
        let acct = match self.tenants.get_mut(tenant_id) {
            Some(a) => a,
            None => {
                return AdmissionDecision {
                    admitted: false,
                    denial_reason: Some(DenialReason::TenantNotFound),
                    quota_usage: QuotaUsage::default(),
                    estimated_wait: None,
                }
            }
        };

        // Reset window if expired
        acct.maybe_reset_window();

        if acct.suspended {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::TenantSuspended),
                quota_usage: acct.usage(),
                estimated_wait: None,
            };
        }

        // Check per-sandbox limits
        if request.memory_bytes > acct.budget.max_sandbox_memory {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::SandboxMemoryExceedsLimit {
                    requested: request.memory_bytes,
                    limit: acct.budget.max_sandbox_memory,
                }),
                quota_usage: acct.usage(),
                estimated_wait: None,
            };
        }

        if let (Some(requested_fuel), Some(limit_fuel)) =
            (request.fuel, acct.budget.max_sandbox_fuel)
        {
            if requested_fuel > limit_fuel {
                return AdmissionDecision {
                    admitted: false,
                    denial_reason: Some(DenialReason::SandboxFuelExceedsLimit {
                        requested: requested_fuel,
                        limit: limit_fuel,
                    }),
                    quota_usage: acct.usage(),
                    estimated_wait: None,
                };
            }
        }

        // Check concurrent limit
        if acct.active_sandboxes >= acct.budget.max_concurrent {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::ConcurrentLimitReached {
                    current: acct.active_sandboxes,
                    limit: acct.budget.max_concurrent,
                }),
                quota_usage: acct.usage(),
                estimated_wait: Some(Duration::from_secs(5)), // rough estimate
            };
        }

        // Check hourly sandbox count (with burst)
        let sandbox_limit = acct.effective_limit(acct.budget.max_sandboxes_per_hour as f64) as u64;
        if acct.sandbox_count_this_window >= sandbox_limit {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::HourlySandboxLimitReached {
                    current: acct.sandbox_count_this_window,
                    limit: acct.budget.max_sandboxes_per_hour,
                }),
                quota_usage: acct.usage(),
                estimated_wait: None,
            };
        }

        // Check CPU budget (with burst)
        let cpu_limit = acct.effective_limit(acct.budget.cpu_seconds_per_hour);
        if acct.cpu_seconds_used >= cpu_limit {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::CpuBudgetExhausted {
                    used: acct.cpu_seconds_used,
                    limit: acct.budget.cpu_seconds_per_hour,
                }),
                quota_usage: acct.usage(),
                estimated_wait: None,
            };
        }

        // Check memory budget (with burst)
        let mem_limit = acct.effective_limit(acct.budget.memory_gb_seconds_per_hour);
        if acct.memory_gb_seconds_used >= mem_limit {
            return AdmissionDecision {
                admitted: false,
                denial_reason: Some(DenialReason::MemoryBudgetExhausted {
                    used: acct.memory_gb_seconds_used,
                    limit: acct.budget.memory_gb_seconds_per_hour,
                }),
                quota_usage: acct.usage(),
                estimated_wait: None,
            };
        }

        AdmissionDecision {
            admitted: true,
            denial_reason: None,
            quota_usage: acct.usage(),
            estimated_wait: None,
        }
    }

    /// Record that a sandbox was admitted and started.
    pub fn record_start(&mut self, tenant_id: &str) {
        if let Some(acct) = self.tenants.get_mut(tenant_id) {
            acct.active_sandboxes += 1;
            acct.sandbox_count_this_window += 1;
        }
    }

    /// Record that a sandbox completed, with its resource consumption.
    pub fn record_completion(
        &mut self,
        tenant_id: &str,
        cpu_seconds: f64,
        memory_bytes: u64,
        duration: Duration,
    ) {
        if let Some(acct) = self.tenants.get_mut(tenant_id) {
            acct.active_sandboxes = acct.active_sandboxes.saturating_sub(1);
            acct.cpu_seconds_used += cpu_seconds;
            let memory_gb = memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
            acct.memory_gb_seconds_used += memory_gb * duration.as_secs_f64();
        }
    }

    /// Get the current quota usage for a tenant.
    pub fn usage(&mut self, tenant_id: &str) -> Option<QuotaUsage> {
        let acct = self.tenants.get_mut(tenant_id)?;
        acct.maybe_reset_window();
        Some(acct.usage())
    }

    /// Get the number of registered tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_budget() -> QuotaBudget {
        QuotaBudget {
            cpu_seconds_per_hour: 100.0,
            memory_gb_seconds_per_hour: 50.0,
            max_sandboxes_per_hour: 100,
            max_concurrent: 5,
            max_sandbox_memory: 256 * 1024 * 1024,
            max_sandbox_fuel: Some(10_000_000),
            burst_allowance: 0.1,
        }
    }

    fn default_request() -> AdmissionRequest {
        AdmissionRequest {
            memory_bytes: 64 * 1024 * 1024,
            fuel: Some(1_000_000),
            ..Default::default()
        }
    }

    #[test]
    fn test_admission_controller_creation() {
        let controller = AdmissionController::new();
        assert_eq!(controller.tenant_count(), 0);
    }

    #[test]
    fn test_admit_unknown_tenant() {
        let mut controller = AdmissionController::new();
        let decision = controller.check("unknown", &default_request());
        assert!(!decision.admitted);
        assert_eq!(decision.denial_reason, Some(DenialReason::TenantNotFound));
    }

    #[test]
    fn test_admit_valid_request() {
        let mut controller = AdmissionController::new();
        controller.set_budget("tenant-a", default_budget());

        let decision = controller.check("tenant-a", &default_request());
        assert!(decision.admitted);
        assert!(decision.denial_reason.is_none());
    }

    #[test]
    fn test_concurrent_limit() {
        let mut controller = AdmissionController::new();
        controller.set_budget("tenant-a", QuotaBudget { max_concurrent: 2, ..default_budget() });

        // Start 2 sandboxes
        controller.record_start("tenant-a");
        controller.record_start("tenant-a");

        let decision = controller.check("tenant-a", &default_request());
        assert!(!decision.admitted);
        assert!(matches!(
            decision.denial_reason,
            Some(DenialReason::ConcurrentLimitReached { current: 2, limit: 2 })
        ));

        // Complete one
        controller.record_completion("tenant-a", 1.0, 64 * 1024 * 1024, Duration::from_secs(1));

        let decision = controller.check("tenant-a", &default_request());
        assert!(decision.admitted);
    }

    #[test]
    fn test_sandbox_memory_limit() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget { max_sandbox_memory: 128 * 1024 * 1024, ..default_budget() },
        );

        let big_request = AdmissionRequest { memory_bytes: 256 * 1024 * 1024, ..default_request() };

        let decision = controller.check("tenant-a", &big_request);
        assert!(!decision.admitted);
        assert!(matches!(
            decision.denial_reason,
            Some(DenialReason::SandboxMemoryExceedsLimit { .. })
        ));
    }

    #[test]
    fn test_sandbox_fuel_limit() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget { max_sandbox_fuel: Some(5_000_000), ..default_budget() },
        );

        let big_request = AdmissionRequest { fuel: Some(10_000_000), ..default_request() };

        let decision = controller.check("tenant-a", &big_request);
        assert!(!decision.admitted);
        assert!(matches!(
            decision.denial_reason,
            Some(DenialReason::SandboxFuelExceedsLimit { .. })
        ));
    }

    #[test]
    fn test_hourly_sandbox_limit() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget {
                max_sandboxes_per_hour: 3,
                burst_allowance: 0.0, // no burst
                ..default_budget()
            },
        );

        for _ in 0..3 {
            let decision = controller.check("tenant-a", &default_request());
            assert!(decision.admitted);
            controller.record_start("tenant-a");
            controller.record_completion(
                "tenant-a",
                0.1,
                64 * 1024 * 1024,
                Duration::from_millis(100),
            );
        }

        let decision = controller.check("tenant-a", &default_request());
        assert!(!decision.admitted);
        assert!(matches!(
            decision.denial_reason,
            Some(DenialReason::HourlySandboxLimitReached { current: 3, limit: 3 })
        ));
    }

    #[test]
    fn test_cpu_budget_exhaustion() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget { cpu_seconds_per_hour: 10.0, burst_allowance: 0.0, ..default_budget() },
        );

        // Record heavy CPU usage
        controller.record_start("tenant-a");
        controller.record_completion("tenant-a", 10.0, 64 * 1024 * 1024, Duration::from_secs(10));

        let decision = controller.check("tenant-a", &default_request());
        assert!(!decision.admitted);
        assert!(matches!(decision.denial_reason, Some(DenialReason::CpuBudgetExhausted { .. })));
    }

    #[test]
    fn test_memory_budget_exhaustion() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget {
                memory_gb_seconds_per_hour: 1.0,
                burst_allowance: 0.0,
                ..default_budget()
            },
        );

        // Record heavy memory usage: 1 GB for 2 seconds = 2 GB·s > 1 GB·s limit
        controller.record_start("tenant-a");
        controller.record_completion(
            "tenant-a",
            1.0,
            1024 * 1024 * 1024, // 1 GB
            Duration::from_secs(2),
        );

        let decision = controller.check("tenant-a", &default_request());
        assert!(!decision.admitted);
        assert!(matches!(decision.denial_reason, Some(DenialReason::MemoryBudgetExhausted { .. })));
    }

    #[test]
    fn test_burst_allowance() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget {
                max_sandboxes_per_hour: 10,
                burst_allowance: 0.5, // 50% burst allowed
                ..default_budget()
            },
        );

        for _ in 0..10 {
            controller.record_start("tenant-a");
            controller.record_completion(
                "tenant-a",
                0.1,
                64 * 1024 * 1024,
                Duration::from_millis(10),
            );
        }

        // Should still be admitted due to 50% burst (limit becomes 15)
        let decision = controller.check("tenant-a", &default_request());
        assert!(decision.admitted);
    }

    #[test]
    fn test_suspend_resume() {
        let mut controller = AdmissionController::new();
        controller.set_budget("tenant-a", default_budget());

        assert!(controller.suspend_tenant("tenant-a"));

        let decision = controller.check("tenant-a", &default_request());
        assert!(!decision.admitted);
        assert_eq!(decision.denial_reason, Some(DenialReason::TenantSuspended));

        assert!(controller.resume_tenant("tenant-a"));

        let decision = controller.check("tenant-a", &default_request());
        assert!(decision.admitted);
    }

    #[test]
    fn test_quota_usage() {
        let mut controller = AdmissionController::new();
        controller.set_budget(
            "tenant-a",
            QuotaBudget { cpu_seconds_per_hour: 100.0, max_concurrent: 10, ..default_budget() },
        );

        controller.record_start("tenant-a");
        controller.record_completion("tenant-a", 50.0, 64 * 1024 * 1024, Duration::from_secs(1));

        let usage = controller.usage("tenant-a").unwrap();
        assert!((usage.cpu_fraction - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_denial_reason_display() {
        let reason = DenialReason::ConcurrentLimitReached { current: 5, limit: 5 };
        assert_eq!(reason.to_string(), "concurrent limit reached (5/5)");
    }

    #[test]
    fn test_quota_usage_peak() {
        let usage = QuotaUsage {
            cpu_fraction: 0.3,
            memory_fraction: 0.8,
            sandbox_count_fraction: 0.5,
            concurrent_fraction: 0.2,
        };
        assert!((usage.peak() - 0.8).abs() < 0.001);
        assert!(!usage.is_over_budget());
    }

    #[test]
    fn test_remove_tenant() {
        let mut controller = AdmissionController::new();
        controller.set_budget("tenant-a", default_budget());
        assert_eq!(controller.tenant_count(), 1);

        controller.remove_tenant("tenant-a");
        assert_eq!(controller.tenant_count(), 0);
    }
}
