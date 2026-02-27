//! Multi-Tenant Orchestrator.
//!
//! Fair-share scheduling, tenant isolation, and resource quota management
//! for multi-tenant sandbox execution.
//!
//! # Features
//!
//! - **Tenant Registry**: Register tenants with resource quotas
//! - **Fair-Share Scheduler**: Priority queues with deficit round-robin
//! - **Auto-Scaling**: Configurable min/max instances per tenant
//! - **Observability**: Per-tenant metrics and quota tracking
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::orchestrator::{Orchestrator, OrchestratorConfig, TenantConfig};
//!
//! let orch = Orchestrator::new(OrchestratorConfig::default());
//!
//! orch.register_tenant("tenant-a", TenantConfig {
//!     max_concurrent: 10,
//!     max_memory_bytes: 1024 * 1024 * 1024,
//!     priority: 5,
//!     ..Default::default()
//! })?;
//!
//! let ticket = orch.submit("tenant-a", sandbox_config)?;
//! let result = orch.wait(ticket).await?;
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.

pub mod admission;
#[cfg(any(feature = "platform", feature = "platform-workflow"))]
pub mod pipeline_exec;
mod scheduler;

pub use admission::{
    AdmissionController, AdmissionDecision, AdmissionRequest, DenialReason, QuotaBudget, QuotaUsage,
};
pub use scheduler::{
    Orchestrator, OrchestratorConfig, OrchestratorStats, SubmitTicket, TenantConfig, TenantMetrics,
    TenantStatus,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.max_global_concurrent, 100);
    }
}
