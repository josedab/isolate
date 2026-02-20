//! Multi-tenant resource pools.
//!
//! This module provides resource pooling with tenant isolation and quotas,
//! enabling efficient resource sharing across multiple tenants.
//!
//! # Features
//!
//! - **Tenant Isolation**: Each tenant has isolated resource quotas
//! - **Resource Quotas**: Limit memory, CPU, and concurrency per tenant
//! - **Fair Scheduling**: Prevent any tenant from monopolizing resources
//! - **Usage Tracking**: Monitor resource usage per tenant

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.


//! # Example
//!
//! ```rust,ignore
//! use isolate_core::pool::{ResourcePool, TenantQuota, TenantId};
//!
//! let pool = ResourcePool::new(PoolConfig::default());
//!
//! // Register a tenant with quotas
//! let quota = TenantQuota::new()
//!     .with_max_memory(1024 * 1024 * 1024)  // 1GB
//!     .with_max_sandboxes(100);
//! pool.register_tenant("tenant-a", quota)?;
//!
//! // Request resources
//! let lease = pool.acquire("tenant-a", ResourceRequest::default())?;
//!
//! // Release when done
//! pool.release(lease);
//! ```

pub mod autoscale;
pub mod billing;
pub mod hierarchy;
pub mod prewarm;
mod quota;
mod tenant;
pub mod warm;

pub use autoscale::{AutoScaleConfig, AutoScaleEvent, AutoScaleSnapshot, AutoScaler};
pub use billing::{FairScheduler, ScheduleDecision, TenantUsageSummary, UsageMeter, UsageRecord};
pub use prewarm::{PreWarmAction, PreWarmConfig, PreWarmDecision, PreWarmEngine, PreWarmStrategy};
pub use quota::{QuotaError, ResourceUsage, TenantQuota};
pub use tenant::{PoolConfig, PoolError, ResourceLease, ResourcePool, TenantId, TenantInfo};
pub use warm::{
    EvictionPolicy, PoolStats, PrecompiledModule, WarmInstance, WarmPool, WarmPoolConfig,
    WarmPoolError,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Just verify exports compile
        let _ = TenantQuota::new();
        let _ = PoolConfig::default();
    }
}
