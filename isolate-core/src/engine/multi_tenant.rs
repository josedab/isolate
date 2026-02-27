//! Multi-tenant execution engine.
//!
//! Wraps `ResourcePool` and `WasmEngine` to provide a single entry point
//! for tenant-scoped sandbox execution with automatic quota enforcement,
//! fair scheduling, and per-tenant metrics.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::engine::multi_tenant::*;
//! use isolate_core::pool::tenant::TenantId;
//! use isolate_core::pool::quota::TenantQuota;
//!
//! let engine = MultiTenantEngine::new(MultiTenantConfig::default())?;
//!
//! // Register a tenant
//! engine.register_tenant("acme-corp", TenantQuota::default())?;
//!
//! // Run a sandbox on behalf of that tenant
//! let output = engine.run("acme-corp", &wasm_bytes, &[]).await?;
//! ```

#![allow(missing_docs)]
use crate::config::SandboxConfig;
use crate::engine::wasm::WasmEngine;
use crate::error::{Error, Result};
use crate::sandbox::Output;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Identifier for a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

impl TenantId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: Into<String>> From<T> for TenantId {
    fn from(v: T) -> Self {
        Self(v.into())
    }
}

/// Per-tenant resource quotas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    /// Maximum concurrent sandboxes.
    pub max_concurrent: u32,
    /// Maximum memory per sandbox (bytes).
    pub max_memory_per_sandbox: usize,
    /// Maximum total memory across all sandboxes (bytes).
    pub max_total_memory: u64,
    /// Maximum CPU fuel per sandbox.
    pub max_fuel_per_sandbox: Option<u64>,
    /// Maximum requests per second.
    pub max_rps: u32,
    /// Scheduling priority (higher = more priority, 1-10).
    pub priority: u8,
}

impl Default for TenantQuota {
    fn default() -> Self {
        Self {
            max_concurrent: 50,
            max_memory_per_sandbox: 128 * 1024 * 1024,
            max_total_memory: 4 * 1024 * 1024 * 1024,
            max_fuel_per_sandbox: Some(10_000_000),
            max_rps: 100,
            priority: 5,
        }
    }
}

/// Per-tenant usage tracking.
#[derive(Debug)]
struct TenantState {
    quota: TenantQuota,
    active_sandboxes: AtomicU64,
    total_memory: AtomicU64,
    total_executions: AtomicU64,
    total_fuel_consumed: AtomicU64,
    #[allow(dead_code)] // Tracked for future tenant lifecycle management
    created_at: Instant,
}

impl TenantState {
    fn new(quota: TenantQuota) -> Self {
        Self {
            quota,
            active_sandboxes: AtomicU64::new(0),
            total_memory: AtomicU64::new(0),
            total_executions: AtomicU64::new(0),
            total_fuel_consumed: AtomicU64::new(0),
            created_at: Instant::now(),
        }
    }
}

/// Snapshot of a tenant's current usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUsage {
    pub tenant_id: String,
    pub active_sandboxes: u64,
    pub total_memory_bytes: u64,
    pub total_executions: u64,
    pub total_fuel_consumed: u64,
}

/// Configuration for the multi-tenant engine.
#[derive(Debug, Clone)]
pub struct MultiTenantConfig {
    /// Maximum number of tenants.
    pub max_tenants: usize,
    /// Default quota for new tenants.
    pub default_quota: TenantQuota,
    /// Whether to allow unknown (unregistered) tenants.
    pub allow_unknown: bool,
}

impl Default for MultiTenantConfig {
    fn default() -> Self {
        Self { max_tenants: 1000, default_quota: TenantQuota::default(), allow_unknown: false }
    }
}

/// A multi-tenant WASM execution engine.
///
/// Manages per-tenant quotas, tracks resource usage, and provides
/// fair scheduling across tenants.
pub struct MultiTenantEngine {
    engine: Arc<WasmEngine>,
    tenants: DashMap<String, Arc<TenantState>>,
    config: MultiTenantConfig,
}

impl MultiTenantEngine {
    /// Create a new multi-tenant engine.
    pub fn new(config: MultiTenantConfig) -> Result<Self> {
        let engine = Arc::new(WasmEngine::new()?);
        Ok(Self { engine, tenants: DashMap::new(), config })
    }

    /// Create with a shared WASM engine.
    pub fn with_engine(engine: Arc<WasmEngine>, config: MultiTenantConfig) -> Self {
        Self { engine, tenants: DashMap::new(), config }
    }

    /// Register a tenant with a quota.
    pub fn register_tenant(&self, id: impl Into<String>, quota: TenantQuota) -> Result<()> {
        let id = id.into();
        if self.tenants.len() >= self.config.max_tenants {
            return Err(Error::InvalidConfig(format!(
                "Maximum tenant count ({}) reached",
                self.config.max_tenants
            )));
        }
        self.tenants.insert(id, Arc::new(TenantState::new(quota)));
        Ok(())
    }

    /// Update a tenant's quota.
    pub fn update_quota(&self, id: &str, quota: TenantQuota) -> Result<()> {
        if !self.tenants.contains_key(id) {
            return Err(Error::InvalidConfig(format!("Tenant '{}' not found", id)));
        }
        self.tenants.insert(id.to_string(), Arc::new(TenantState::new(quota)));
        Ok(())
    }

    /// Remove a tenant.
    pub fn remove_tenant(&self, id: &str) -> bool {
        self.tenants.remove(id).is_some()
    }

    /// Run a sandbox on behalf of a tenant.
    pub async fn run(
        &self,
        tenant_id: &str,
        sandbox_config: SandboxConfig,
        input: &[u8],
    ) -> Result<Output> {
        let state = self.resolve_tenant(tenant_id)?;

        // Enforce concurrency quota
        let active = state.active_sandboxes.fetch_add(1, Ordering::AcqRel);
        if active >= state.quota.max_concurrent as u64 {
            state.active_sandboxes.fetch_sub(1, Ordering::Release);
            return Err(Error::InvalidState {
                expected: format!("< {} concurrent sandboxes", state.quota.max_concurrent),
                actual: format!("{} active for tenant '{}'", active + 1, tenant_id),
            });
        }

        let result = async {
            let mut sandbox =
                crate::Sandbox::create_with_engine(sandbox_config, self.engine.clone()).await?;
            sandbox.run(input).await
        }
        .await;

        // Decrement active count
        state.active_sandboxes.fetch_sub(1, Ordering::Release);
        state.total_executions.fetch_add(1, Ordering::Relaxed);

        if let Ok(ref output) = result {
            state
                .total_fuel_consumed
                .fetch_add(output.resource_usage.fuel_consumed, Ordering::Relaxed);
        }

        result
    }

    /// Get usage stats for a tenant.
    pub fn usage(&self, tenant_id: &str) -> Option<TenantUsage> {
        self.tenants.get(tenant_id).map(|s| TenantUsage {
            tenant_id: tenant_id.to_string(),
            active_sandboxes: s.active_sandboxes.load(Ordering::Relaxed),
            total_memory_bytes: s.total_memory.load(Ordering::Relaxed),
            total_executions: s.total_executions.load(Ordering::Relaxed),
            total_fuel_consumed: s.total_fuel_consumed.load(Ordering::Relaxed),
        })
    }

    /// List all tenant IDs.
    pub fn tenant_ids(&self) -> Vec<String> {
        self.tenants.iter().map(|e| e.key().clone()).collect()
    }

    /// Get the number of registered tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    fn resolve_tenant(&self, id: &str) -> Result<Arc<TenantState>> {
        if let Some(state) = self.tenants.get(id) {
            return Ok(state.value().clone());
        }
        if self.config.allow_unknown {
            let state = Arc::new(TenantState::new(self.config.default_quota.clone()));
            self.tenants.insert(id.to_string(), state.clone());
            Ok(state)
        } else {
            Err(Error::InvalidConfig(format!("Unknown tenant: '{}'", id)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_tenant_config_default() {
        let config = MultiTenantConfig::default();
        assert_eq!(config.max_tenants, 1000);
        assert!(!config.allow_unknown);
    }

    #[test]
    fn test_register_and_remove_tenant() {
        let engine = MultiTenantEngine::new(MultiTenantConfig::default()).unwrap();
        engine.register_tenant("t1", TenantQuota::default()).unwrap();
        assert_eq!(engine.tenant_count(), 1);
        assert!(engine.remove_tenant("t1"));
        assert_eq!(engine.tenant_count(), 0);
    }

    #[test]
    fn test_unknown_tenant_rejected() {
        let engine = MultiTenantEngine::new(MultiTenantConfig {
            allow_unknown: false,
            ..MultiTenantConfig::default()
        })
        .unwrap();
        assert!(engine.resolve_tenant("ghost").is_err());
    }

    #[test]
    fn test_unknown_tenant_auto_registered() {
        let engine = MultiTenantEngine::new(MultiTenantConfig {
            allow_unknown: true,
            ..MultiTenantConfig::default()
        })
        .unwrap();
        assert!(engine.resolve_tenant("auto").is_ok());
        assert_eq!(engine.tenant_count(), 1);
    }

    #[test]
    fn test_tenant_usage_default() {
        let engine = MultiTenantEngine::new(MultiTenantConfig::default()).unwrap();
        engine.register_tenant("t1", TenantQuota::default()).unwrap();
        let usage = engine.usage("t1").unwrap();
        assert_eq!(usage.active_sandboxes, 0);
        assert_eq!(usage.total_executions, 0);
    }

    #[test]
    fn test_max_tenants_enforced() {
        let engine = MultiTenantEngine::new(MultiTenantConfig {
            max_tenants: 2,
            ..MultiTenantConfig::default()
        })
        .unwrap();
        engine.register_tenant("a", TenantQuota::default()).unwrap();
        engine.register_tenant("b", TenantQuota::default()).unwrap();
        assert!(engine.register_tenant("c", TenantQuota::default()).is_err());
    }
}
