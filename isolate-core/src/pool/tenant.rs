//! Resource pool implementation.

use super::quota::{AtomicResourceUsage, QuotaError, ResourceUsage, TenantQuota};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// Tenant identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

impl TenantId {
    /// Create a new tenant ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for TenantId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for TenantId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&TenantId> for TenantId {
    fn from(id: &TenantId) -> Self {
        id.clone()
    }
}

/// Tenant information.
#[derive(Debug, Clone)]
pub struct TenantInfo {
    /// Tenant ID.
    pub id: TenantId,
    /// Tenant quotas.
    pub quota: TenantQuota,
    /// Current resource usage.
    pub usage: ResourceUsage,
    /// Registration timestamp.
    pub registered_at: DateTime<Utc>,
    /// Last activity timestamp.
    pub last_activity: Option<DateTime<Utc>>,
    /// Tenant metadata.
    pub metadata: std::collections::HashMap<String, String>,
}

/// Internal tenant state.
struct TenantState {
    quota: TenantQuota,
    usage: Arc<AtomicResourceUsage>,
    registered_at: DateTime<Utc>,
    last_activity: AtomicU64,
    metadata: std::collections::HashMap<String, String>,
}

impl TenantState {
    fn new(quota: TenantQuota) -> Self {
        Self {
            quota,
            usage: AtomicResourceUsage::new(),
            registered_at: Utc::now(),
            last_activity: AtomicU64::new(0),
            metadata: std::collections::HashMap::new(),
        }
    }

    fn update_activity(&self) {
        let now = Utc::now().timestamp() as u64;
        self.last_activity.store(now, Ordering::SeqCst);
    }

    fn last_activity_time(&self) -> Option<DateTime<Utc>> {
        let ts = self.last_activity.load(Ordering::SeqCst);
        if ts == 0 {
            None
        } else {
            DateTime::from_timestamp(ts as i64, 0)
        }
    }

    fn to_info(&self, id: TenantId) -> TenantInfo {
        TenantInfo {
            id,
            quota: self.quota.clone(),
            usage: self.usage.snapshot(),
            registered_at: self.registered_at,
            last_activity: self.last_activity_time(),
            metadata: self.metadata.clone(),
        }
    }
}

/// Resource pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum total memory across all tenants.
    pub max_total_memory: u64,
    /// Maximum total sandboxes across all tenants.
    pub max_total_sandboxes: u32,
    /// Maximum number of tenants.
    pub max_tenants: u32,
    /// Default quota for new tenants.
    pub default_quota: TenantQuota,
    /// Whether to allow unknown tenants.
    pub allow_unknown_tenants: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_total_memory: 16 * 1024 * 1024 * 1024, // 16GB
            max_total_sandboxes: 10000,
            max_tenants: 1000,
            default_quota: TenantQuota::default(),
            allow_unknown_tenants: false,
        }
    }
}

impl PoolConfig {
    /// Create a new pool configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum total memory.
    pub fn with_max_total_memory(mut self, bytes: u64) -> Self {
        self.max_total_memory = bytes;
        self
    }

    /// Set maximum total sandboxes.
    pub fn with_max_total_sandboxes(mut self, count: u32) -> Self {
        self.max_total_sandboxes = count;
        self
    }

    /// Set maximum tenants.
    pub fn with_max_tenants(mut self, count: u32) -> Self {
        self.max_tenants = count;
        self
    }

    /// Set default quota.
    pub fn with_default_quota(mut self, quota: TenantQuota) -> Self {
        self.default_quota = quota;
        self
    }

    /// Set whether to allow unknown tenants.
    pub fn with_allow_unknown_tenants(mut self, allow: bool) -> Self {
        self.allow_unknown_tenants = allow;
        self
    }
}

/// A resource lease acquired from the pool.
#[derive(Debug)]
pub struct ResourceLease {
    /// Lease ID.
    pub id: Uuid,
    /// Tenant ID.
    pub tenant_id: TenantId,
    /// Memory reserved.
    pub memory_bytes: u64,
    /// Acquired timestamp.
    pub acquired_at: DateTime<Utc>,
    /// Reference to tenant usage for cleanup.
    tenant_usage: Arc<AtomicResourceUsage>,
    /// Reference to global usage for cleanup.
    global_usage: Arc<AtomicResourceUsage>,
}

impl ResourceLease {
    fn new(
        tenant_id: TenantId,
        memory_bytes: u64,
        tenant_usage: Arc<AtomicResourceUsage>,
        global_usage: Arc<AtomicResourceUsage>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            memory_bytes,
            acquired_at: Utc::now(),
            tenant_usage,
            global_usage,
        }
    }

    /// Get the lease duration.
    pub fn duration(&self) -> chrono::Duration {
        Utc::now().signed_duration_since(self.acquired_at)
    }
}

impl Drop for ResourceLease {
    fn drop(&mut self) {
        // Release resources when lease is dropped
        self.tenant_usage.release_memory(self.memory_bytes);
        self.tenant_usage.remove_sandbox();
        self.global_usage.release_memory(self.memory_bytes);
        self.global_usage.remove_sandbox();
    }
}

/// Pool statistics.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total registered tenants.
    pub total_tenants: u32,
    /// Active tenants (with active sandboxes).
    pub active_tenants: u32,
    /// Total memory in use.
    pub total_memory_used: u64,
    /// Total active sandboxes.
    pub total_active_sandboxes: u32,
    /// Total sandboxes created.
    pub total_sandboxes_created: u64,
}

/// Multi-tenant resource pool.
pub struct ResourcePool {
    config: PoolConfig,
    tenants: DashMap<TenantId, TenantState>,
    global_usage: Arc<AtomicResourceUsage>,
}

impl ResourcePool {
    /// Create a new resource pool.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            tenants: DashMap::new(),
            global_usage: AtomicResourceUsage::new(),
        }
    }

    /// Get the pool configuration.
    pub fn config(&self) -> &PoolConfig {
        &self.config
    }

    /// Register a new tenant.
    pub fn register_tenant(
        &self,
        id: impl Into<TenantId>,
        quota: TenantQuota,
    ) -> Result<(), PoolError> {
        let id = id.into();

        if self.tenants.len() >= self.config.max_tenants as usize {
            return Err(PoolError::TooManyTenants {
                max: self.config.max_tenants,
            });
        }

        if self.tenants.contains_key(&id) {
            return Err(PoolError::TenantAlreadyExists(id));
        }

        self.tenants.insert(id, TenantState::new(quota));
        Ok(())
    }

    /// Update a tenant's quota.
    pub fn update_quota(
        &self,
        id: impl Into<TenantId>,
        quota: TenantQuota,
    ) -> Result<(), PoolError> {
        let id = id.into();
        let mut tenant = self
            .tenants
            .get_mut(&id)
            .ok_or_else(|| PoolError::TenantNotFound(id.clone()))?;
        tenant.quota = quota;
        Ok(())
    }

    /// Remove a tenant.
    pub fn remove_tenant(&self, id: impl Into<TenantId>) -> Result<TenantInfo, PoolError> {
        let id = id.into();
        let (_, state) = self
            .tenants
            .remove(&id)
            .ok_or_else(|| PoolError::TenantNotFound(id.clone()))?;
        Ok(state.to_info(id))
    }

    /// Get tenant information.
    pub fn get_tenant(&self, id: impl Into<TenantId>) -> Option<TenantInfo> {
        let id = id.into();
        self.tenants.get(&id).map(|t| t.to_info(id))
    }

    /// Check if a tenant exists.
    pub fn has_tenant(&self, id: impl Into<TenantId>) -> bool {
        let id = id.into();
        self.tenants.contains_key(&id)
    }

    /// List all tenant IDs.
    pub fn tenant_ids(&self) -> Vec<TenantId> {
        self.tenants.iter().map(|r| r.key().clone()).collect()
    }

    /// Acquire a resource lease.
    pub fn acquire(
        &self,
        tenant_id: impl Into<TenantId>,
        memory_bytes: u64,
    ) -> Result<ResourceLease, PoolError> {
        let tenant_id = tenant_id.into();

        // Get or create tenant
        let tenant = if let Some(t) = self.tenants.get(&tenant_id) {
            t
        } else if self.config.allow_unknown_tenants {
            self.register_tenant(tenant_id.clone(), self.config.default_quota.clone())?;
            self.tenants.get(&tenant_id).unwrap()
        } else {
            return Err(PoolError::TenantNotFound(tenant_id));
        };

        // Check tenant quota
        if let Some(quota_error) = tenant.usage.exceeds_quota(&tenant.quota) {
            return Err(PoolError::Quota(quota_error));
        }

        // Check global limits
        let global_usage = self.global_usage.snapshot();
        if global_usage.memory_bytes + memory_bytes > self.config.max_total_memory {
            return Err(PoolError::GlobalMemoryExceeded {
                requested: memory_bytes,
                available: self
                    .config
                    .max_total_memory
                    .saturating_sub(global_usage.memory_bytes),
            });
        }

        if global_usage.active_sandboxes >= self.config.max_total_sandboxes {
            return Err(PoolError::GlobalSandboxLimitExceeded {
                limit: self.config.max_total_sandboxes,
            });
        }

        // Reserve resources
        tenant.usage.add_memory(memory_bytes);
        tenant.usage.add_sandbox();
        tenant.usage.record_sandbox_created();
        tenant.usage.increment_rps();
        tenant.update_activity();

        self.global_usage.add_memory(memory_bytes);
        self.global_usage.add_sandbox();
        self.global_usage.record_sandbox_created();

        Ok(ResourceLease::new(
            tenant_id,
            memory_bytes,
            Arc::clone(&tenant.usage),
            Arc::clone(&self.global_usage),
        ))
    }

    /// Get pool statistics.
    pub fn stats(&self) -> PoolStats {
        let global = self.global_usage.snapshot();
        let active_tenants = self
            .tenants
            .iter()
            .filter(|t| t.usage.snapshot().active_sandboxes > 0)
            .count() as u32;

        PoolStats {
            total_tenants: self.tenants.len() as u32,
            active_tenants,
            total_memory_used: global.memory_bytes,
            total_active_sandboxes: global.active_sandboxes,
            total_sandboxes_created: global.total_sandboxes,
        }
    }

    /// Reset per-second rate limits for all tenants.
    pub fn reset_rps_counters(&self) {
        for tenant in self.tenants.iter() {
            tenant.usage.reset_rps();
        }
    }

    /// Reset per-minute counters for all tenants.
    pub fn reset_minute_counters(&self) {
        for tenant in self.tenants.iter() {
            tenant.usage.reset_minute_counters();
        }
    }

    /// Get all tenant info.
    pub fn all_tenants(&self) -> Vec<TenantInfo> {
        self.tenants
            .iter()
            .map(|r| r.to_info(r.key().clone()))
            .collect()
    }
}

impl Default for ResourcePool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

impl std::fmt::Debug for ResourcePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourcePool")
            .field("config", &self.config)
            .field("tenant_count", &self.tenants.len())
            .finish()
    }
}

/// Pool-related errors.
#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    /// Too many tenants.
    #[error("Too many tenants (max: {max})")]
    TooManyTenants { max: u32 },

    /// Tenant already exists.
    #[error("Tenant '{0}' already exists")]
    TenantAlreadyExists(TenantId),

    /// Tenant not found.
    #[error("Tenant '{0}' not found")]
    TenantNotFound(TenantId),

    /// Quota error.
    #[error("Quota error: {0}")]
    Quota(#[from] QuotaError),

    /// Global memory limit exceeded.
    #[error("Global memory limit exceeded: requested {requested}, available {available}")]
    GlobalMemoryExceeded { requested: u64, available: u64 },

    /// Global sandbox limit exceeded.
    #[error("Global sandbox limit exceeded (max: {limit})")]
    GlobalSandboxLimitExceeded { limit: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_id() {
        let id = TenantId::new("tenant-a");
        assert_eq!(id.to_string(), "tenant-a");
    }

    #[test]
    fn test_pool_config() {
        let config = PoolConfig::new()
            .with_max_total_memory(8 * 1024 * 1024 * 1024)
            .with_max_tenants(500);

        assert_eq!(config.max_total_memory, 8 * 1024 * 1024 * 1024);
        assert_eq!(config.max_tenants, 500);
    }

    #[test]
    fn test_pool_register_tenant() {
        let pool = ResourcePool::new(PoolConfig::default());

        pool.register_tenant("tenant-a", TenantQuota::default())
            .unwrap();
        assert!(pool.has_tenant("tenant-a"));

        let result = pool.register_tenant("tenant-a", TenantQuota::default());
        assert!(matches!(result, Err(PoolError::TenantAlreadyExists(_))));
    }

    #[test]
    fn test_pool_remove_tenant() {
        let pool = ResourcePool::new(PoolConfig::default());

        pool.register_tenant("tenant-a", TenantQuota::default())
            .unwrap();
        let info = pool.remove_tenant("tenant-a").unwrap();
        assert_eq!(info.id.0, "tenant-a");
        assert!(!pool.has_tenant("tenant-a"));
    }

    #[test]
    fn test_pool_acquire_release() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::default())
            .unwrap();

        let lease = pool.acquire("tenant-a", 1024).unwrap();
        assert_eq!(lease.memory_bytes, 1024);

        let stats = pool.stats();
        assert_eq!(stats.total_active_sandboxes, 1);

        drop(lease);

        let stats = pool.stats();
        assert_eq!(stats.total_active_sandboxes, 0);
    }

    #[test]
    fn test_pool_quota_enforcement() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::new().with_max_sandboxes(2))
            .unwrap();

        let _lease1 = pool.acquire("tenant-a", 100).unwrap();
        let _lease2 = pool.acquire("tenant-a", 100).unwrap();

        let result = pool.acquire("tenant-a", 100);
        assert!(matches!(
            result,
            Err(PoolError::Quota(QuotaError::SandboxLimitExceeded { .. }))
        ));
    }

    #[test]
    fn test_pool_global_limits() {
        let pool = ResourcePool::new(
            PoolConfig::new()
                .with_max_total_memory(1000)
                .with_max_total_sandboxes(2),
        );
        pool.register_tenant("tenant-a", TenantQuota::unlimited())
            .unwrap();

        let _lease1 = pool.acquire("tenant-a", 500).unwrap();
        let _lease2 = pool.acquire("tenant-a", 400).unwrap();

        // Memory limit
        let result = pool.acquire("tenant-a", 200);
        assert!(matches!(
            result,
            Err(PoolError::GlobalMemoryExceeded { .. })
        ));
    }

    #[test]
    fn test_pool_unknown_tenant() {
        let pool = ResourcePool::new(PoolConfig::new().with_allow_unknown_tenants(false));

        let result = pool.acquire("unknown", 100);
        assert!(matches!(result, Err(PoolError::TenantNotFound(_))));
    }

    #[test]
    fn test_pool_auto_register_tenant() {
        let pool = ResourcePool::new(PoolConfig::new().with_allow_unknown_tenants(true));

        let lease = pool.acquire("new-tenant", 100).unwrap();
        assert!(pool.has_tenant("new-tenant"));
        drop(lease);
    }

    #[test]
    fn test_pool_update_quota() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::new().with_max_sandboxes(1))
            .unwrap();

        let _lease1 = pool.acquire("tenant-a", 100).unwrap();
        assert!(pool.acquire("tenant-a", 100).is_err());

        pool.update_quota("tenant-a", TenantQuota::new().with_max_sandboxes(5))
            .unwrap();
        let _lease2 = pool.acquire("tenant-a", 100).unwrap();
    }

    #[test]
    fn test_pool_stats() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::default())
            .unwrap();
        pool.register_tenant("tenant-b", TenantQuota::default())
            .unwrap();

        let _lease1 = pool.acquire("tenant-a", 100).unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total_tenants, 2);
        assert_eq!(stats.active_tenants, 1);
        assert_eq!(stats.total_memory_used, 100);
        assert_eq!(stats.total_active_sandboxes, 1);
    }

    #[test]
    fn test_pool_tenant_info() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::new().with_priority(8))
            .unwrap();

        let _lease = pool.acquire("tenant-a", 100).unwrap();

        let info = pool.get_tenant("tenant-a").unwrap();
        assert_eq!(info.quota.priority, 8);
        assert_eq!(info.usage.active_sandboxes, 1);
    }

    #[test]
    fn test_pool_reset_counters() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("tenant-a", TenantQuota::default())
            .unwrap();

        let _lease = pool.acquire("tenant-a", 100).unwrap();
        pool.reset_rps_counters();
        pool.reset_minute_counters();

        let info = pool.get_tenant("tenant-a").unwrap();
        assert_eq!(info.usage.current_rps, 0);
        assert_eq!(info.usage.sandboxes_this_minute, 0);
    }

    #[test]
    fn test_max_tenants() {
        let pool = ResourcePool::new(PoolConfig::new().with_max_tenants(2));

        pool.register_tenant("a", TenantQuota::default()).unwrap();
        pool.register_tenant("b", TenantQuota::default()).unwrap();

        let result = pool.register_tenant("c", TenantQuota::default());
        assert!(matches!(result, Err(PoolError::TooManyTenants { max: 2 })));
    }

    #[test]
    fn test_all_tenants() {
        let pool = ResourcePool::new(PoolConfig::default());
        pool.register_tenant("a", TenantQuota::default()).unwrap();
        pool.register_tenant("b", TenantQuota::default()).unwrap();

        let tenants = pool.all_tenants();
        assert_eq!(tenants.len(), 2);
    }
}
