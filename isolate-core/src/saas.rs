#![allow(dead_code)]
//! Multi-tenant SaaS service layer for the Isolate platform.
//!
//! Provides API key management, tenant lifecycle, usage tracking,
//! and a top-level service that ties authentication and sandbox
//! creation together.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// API Scopes
// ---------------------------------------------------------------------------

/// Permission scopes that can be granted to an API key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApiScope {
    /// Create new sandboxes.
    SandboxCreate,
    /// Run existing sandboxes.
    SandboxRun,
    /// Read sandbox state and output.
    SandboxRead,
    /// Full administrative access.
    Admin,
}

// ---------------------------------------------------------------------------
// API Key
// ---------------------------------------------------------------------------

/// An API key with associated metadata.
///
/// The plaintext key is **never** stored; only a SHA-256 hash is persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Unique identifier for this key.
    pub key_id: String,
    /// SHA-256 hash of the plaintext key (hex-encoded).
    pub key_hash: String,
    /// Tenant that owns this key.
    pub tenant_id: String,
    /// Human-readable name.
    pub name: String,
    /// When the key was created.
    pub created_at: DateTime<Utc>,
    /// Optional expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Rate limit in requests per minute.
    pub rate_limit: u32,
    /// Granted scopes.
    pub scopes: Vec<ApiScope>,
    /// Whether the key is active.
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// API Key Manager
// ---------------------------------------------------------------------------

/// Manages API key lifecycle: generation, validation, and revocation.
#[derive(Debug, Clone)]
pub struct ApiKeyManager {
    keys: Arc<RwLock<HashMap<String, ApiKey>>>,
    /// Maps key_hash → key_id for fast lookup during validation.
    hash_index: Arc<RwLock<HashMap<String, String>>>,
}

impl ApiKeyManager {
    /// Create a new, empty key manager.
    pub fn new() -> Self {
        Self {
            keys: Arc::new(RwLock::new(HashMap::new())),
            hash_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate a new API key for the given tenant.
    ///
    /// Returns the persisted [`ApiKey`] together with the **plaintext** key
    /// string that must be shown to the user exactly once.
    pub fn generate(
        &self,
        tenant_id: &str,
        name: &str,
        scopes: Vec<ApiScope>,
    ) -> (ApiKey, String) {
        let plaintext = format!("iso_{}", Uuid::new_v4().as_simple());
        let key_hash = hash_key(&plaintext);
        let key_id = Uuid::new_v4().to_string();

        let api_key = ApiKey {
            key_id: key_id.clone(),
            key_hash: key_hash.clone(),
            tenant_id: tenant_id.to_string(),
            name: name.to_string(),
            created_at: Utc::now(),
            expires_at: None,
            rate_limit: 60,
            scopes,
            is_active: true,
        };

        self.keys.write().unwrap().insert(key_id.clone(), api_key.clone());
        self.hash_index.write().unwrap().insert(key_hash, key_id);

        (api_key, plaintext)
    }

    /// Validate a plaintext key string and return the corresponding [`ApiKey`].
    pub fn validate(&self, key_str: &str) -> Result<ApiKey> {
        let key_hash = hash_key(key_str);
        let keys = self.keys.read().unwrap();
        let index = self.hash_index.read().unwrap();

        let key_id = index
            .get(&key_hash)
            .ok_or_else(|| Error::InvalidConfig("Invalid API key".into()))?;

        let api_key = keys
            .get(key_id)
            .ok_or_else(|| Error::InvalidConfig("API key not found".into()))?;

        if !api_key.is_active {
            return Err(Error::InvalidConfig("API key is revoked".into()));
        }

        if let Some(exp) = api_key.expires_at {
            if Utc::now() > exp {
                return Err(Error::InvalidConfig("API key has expired".into()));
            }
        }

        Ok(api_key.clone())
    }

    /// Revoke an API key by its id.
    pub fn revoke(&self, key_id: &str) -> Result<()> {
        let mut keys = self.keys.write().unwrap();
        let api_key = keys
            .get_mut(key_id)
            .ok_or_else(|| Error::InvalidConfig("API key not found".into()))?;
        api_key.is_active = false;
        Ok(())
    }

    /// List all keys belonging to a tenant.
    pub fn list_for_tenant(&self, tenant_id: &str) -> Vec<ApiKey> {
        self.keys
            .read()
            .unwrap()
            .values()
            .filter(|k| k.tenant_id == tenant_id)
            .cloned()
            .collect()
    }
}

impl Default for ApiKeyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tenant Plan
// ---------------------------------------------------------------------------

/// Billing plan tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plan {
    /// Free tier with limited resources.
    Free,
    /// Professional tier.
    Pro,
    /// Enterprise tier with custom limits.
    Enterprise,
}

// ---------------------------------------------------------------------------
// Tenant Limits
// ---------------------------------------------------------------------------

/// Resource limits for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantLimits {
    /// Maximum concurrent sandboxes.
    pub max_sandboxes: u64,
    /// Maximum total memory across all sandboxes (bytes).
    pub max_memory: u64,
    /// Maximum number of API keys.
    pub max_api_keys: u64,
}

impl TenantLimits {
    /// Default limits for a given plan.
    pub fn for_plan(plan: &Plan) -> Self {
        match plan {
            Plan::Free => Self {
                max_sandboxes: 5,
                max_memory: 256 * 1024 * 1024,
                max_api_keys: 3,
            },
            Plan::Pro => Self {
                max_sandboxes: 50,
                max_memory: 2 * 1024 * 1024 * 1024,
                max_api_keys: 20,
            },
            Plan::Enterprise => Self {
                max_sandboxes: 500,
                max_memory: 16 * 1024 * 1024 * 1024,
                max_api_keys: 100,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tenant
// ---------------------------------------------------------------------------

/// A multi-tenant organisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// Unique tenant identifier.
    pub tenant_id: String,
    /// Human-readable name.
    pub name: String,
    /// Current billing plan.
    pub plan: Plan,
    /// When the tenant was created.
    pub created_at: DateTime<Utc>,
    /// Resource limits derived from the plan.
    pub usage_limits: TenantLimits,
    /// Whether the tenant is active.
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Tenant Manager
// ---------------------------------------------------------------------------

/// Manages tenant lifecycle.
#[derive(Debug, Clone)]
pub struct TenantManager {
    tenants: Arc<RwLock<HashMap<String, Tenant>>>,
}

impl TenantManager {
    /// Create a new, empty tenant manager.
    pub fn new() -> Self {
        Self {
            tenants: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new tenant with the specified plan.
    pub fn create(&self, name: &str, plan: Plan) -> Tenant {
        let tenant_id = Uuid::new_v4().to_string();
        let tenant = Tenant {
            tenant_id: tenant_id.clone(),
            name: name.to_string(),
            plan: plan.clone(),
            created_at: Utc::now(),
            usage_limits: TenantLimits::for_plan(&plan),
            is_active: true,
        };
        self.tenants.write().unwrap().insert(tenant_id, tenant.clone());
        tenant
    }

    /// Get a tenant by id.
    pub fn get(&self, tenant_id: &str) -> Option<Tenant> {
        self.tenants.read().unwrap().get(tenant_id).cloned()
    }

    /// Update the billing plan for a tenant.
    pub fn update_plan(&self, tenant_id: &str, plan: Plan) -> Result<()> {
        let mut tenants = self.tenants.write().unwrap();
        let tenant = tenants
            .get_mut(tenant_id)
            .ok_or_else(|| Error::InvalidConfig("Tenant not found".into()))?;
        tenant.plan = plan.clone();
        tenant.usage_limits = TenantLimits::for_plan(&plan);
        Ok(())
    }

    /// List all tenants.
    pub fn list(&self) -> Vec<Tenant> {
        self.tenants.read().unwrap().values().cloned().collect()
    }

    /// Delete (deactivate) a tenant.
    pub fn delete(&self, tenant_id: &str) -> Result<()> {
        let mut tenants = self.tenants.write().unwrap();
        let tenant = tenants
            .get_mut(tenant_id)
            .ok_or_else(|| Error::InvalidConfig("Tenant not found".into()))?;
        tenant.is_active = false;
        Ok(())
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Usage Record / Summary
// ---------------------------------------------------------------------------

/// A single usage data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    /// Owning tenant.
    pub tenant_id: String,
    /// When the usage was recorded.
    pub timestamp: DateTime<Utc>,
    /// Number of sandboxes created in this event.
    pub sandbox_count: u64,
    /// Fuel consumed.
    pub fuel_consumed: u64,
    /// Memory consumed (bytes).
    pub memory_bytes: u64,
    /// API calls made.
    pub api_calls: u64,
}

/// Aggregated usage for a billing period.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSummary {
    /// Total sandboxes created.
    pub total_sandboxes: u64,
    /// Total fuel consumed.
    pub total_fuel: u64,
    /// Peak memory usage (bytes).
    pub peak_memory: u64,
    /// Total API calls.
    pub total_api_calls: u64,
}

// ---------------------------------------------------------------------------
// Usage Tracker
// ---------------------------------------------------------------------------

/// Tracks per-tenant resource usage.
#[derive(Debug, Clone)]
pub struct UsageTracker {
    records: Arc<RwLock<Vec<UsageRecord>>>,
    tenant_manager: TenantManager,
}

impl UsageTracker {
    /// Create a new tracker backed by the given tenant manager.
    pub fn new(tenant_manager: TenantManager) -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::new())),
            tenant_manager,
        }
    }

    /// Record a usage event.
    pub fn record(&self, record: UsageRecord) {
        self.records.write().unwrap().push(record);
    }

    /// Get the aggregated usage for the current billing period.
    pub fn get_current_period(&self, tenant_id: &str) -> UsageSummary {
        let records = self.records.read().unwrap();
        let mut summary = UsageSummary::default();
        for r in records.iter().filter(|r| r.tenant_id == tenant_id) {
            summary.total_sandboxes += r.sandbox_count;
            summary.total_fuel += r.fuel_consumed;
            summary.total_api_calls += r.api_calls;
            if r.memory_bytes > summary.peak_memory {
                summary.peak_memory = r.memory_bytes;
            }
        }
        summary
    }

    /// Check whether the tenant is within their plan limits.
    pub fn check_limits(&self, tenant_id: &str) -> Result<()> {
        let tenant = self
            .tenant_manager
            .get(tenant_id)
            .ok_or_else(|| Error::InvalidConfig("Tenant not found".into()))?;

        if !tenant.is_active {
            return Err(Error::InvalidConfig("Tenant is inactive".into()));
        }

        let summary = self.get_current_period(tenant_id);
        if summary.total_sandboxes >= tenant.usage_limits.max_sandboxes {
            return Err(Error::InvalidConfig("Sandbox limit exceeded".into()));
        }
        if summary.peak_memory >= tenant.usage_limits.max_memory {
            return Err(Error::InvalidConfig("Memory limit exceeded".into()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Auth Context
// ---------------------------------------------------------------------------

/// Authentication context produced after a successful API key validation.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The validated API key.
    pub api_key: ApiKey,
    /// The tenant that owns the key.
    pub tenant: Tenant,
}

/// Opaque sandbox identifier returned by [`SaasService::create_sandbox_for_tenant`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxId(pub String);

// ---------------------------------------------------------------------------
// SaaS Service
// ---------------------------------------------------------------------------

/// Top-level service that ties API key management, tenants, and usage tracking
/// together.
pub struct SaasService {
    /// API key manager.
    pub api_keys: ApiKeyManager,
    /// Tenant manager.
    pub tenants: TenantManager,
    /// Usage tracker.
    pub usage: UsageTracker,
}

impl SaasService {
    /// Create a new SaaS service with fresh managers.
    pub fn new() -> Self {
        let tenants = TenantManager::new();
        let usage = UsageTracker::new(tenants.clone());
        Self {
            api_keys: ApiKeyManager::new(),
            tenants,
            usage,
        }
    }

    /// Authenticate a request using a plaintext API key.
    pub fn authenticate(&self, api_key_str: &str) -> Result<AuthContext> {
        let api_key = self.api_keys.validate(api_key_str)?;
        let tenant = self
            .tenants
            .get(&api_key.tenant_id)
            .ok_or_else(|| Error::InvalidConfig("Tenant not found for key".into()))?;

        if !tenant.is_active {
            return Err(Error::InvalidConfig("Tenant is inactive".into()));
        }

        Ok(AuthContext { api_key, tenant })
    }

    /// Create a sandbox on behalf of an authenticated tenant.
    ///
    /// `_config` is intentionally unused — it is a placeholder for the real
    /// sandbox configuration that would be wired up in production.
    pub fn create_sandbox_for_tenant(
        &self,
        auth: &AuthContext,
        _config: &str,
    ) -> Result<SandboxId> {
        if !auth.api_key.scopes.contains(&ApiScope::SandboxCreate) {
            return Err(Error::InvalidConfig(
                "API key lacks sandbox:create scope".into(),
            ));
        }

        self.usage.check_limits(&auth.tenant.tenant_id)?;

        let sandbox_id = SandboxId(Uuid::new_v4().to_string());

        self.usage.record(UsageRecord {
            tenant_id: auth.tenant.tenant_id.clone(),
            timestamp: Utc::now(),
            sandbox_count: 1,
            fuel_consumed: 0,
            memory_bytes: 0,
            api_calls: 1,
        });

        Ok(sandbox_id)
    }
}

impl Default for SaasService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hash_key(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    hex::encode(hasher.finalize())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- ApiKeyManager ------------------------------------------------------

    #[test]
    fn test_generate_key_returns_plaintext() {
        let mgr = ApiKeyManager::new();
        let (key, plaintext) = mgr.generate("t1", "test", vec![ApiScope::SandboxCreate]);
        assert!(plaintext.starts_with("iso_"));
        assert!(!key.key_hash.is_empty());
        assert_eq!(key.tenant_id, "t1");
    }

    #[test]
    fn test_validate_valid_key() {
        let mgr = ApiKeyManager::new();
        let (_key, plaintext) = mgr.generate("t1", "k1", vec![ApiScope::SandboxRun]);
        let validated = mgr.validate(&plaintext).unwrap();
        assert_eq!(validated.name, "k1");
    }

    #[test]
    fn test_validate_invalid_key() {
        let mgr = ApiKeyManager::new();
        assert!(mgr.validate("bogus").is_err());
    }

    #[test]
    fn test_revoke_key() {
        let mgr = ApiKeyManager::new();
        let (key, plaintext) = mgr.generate("t1", "k1", vec![]);
        mgr.revoke(&key.key_id).unwrap();
        assert!(mgr.validate(&plaintext).is_err());
    }

    #[test]
    fn test_revoke_nonexistent_key() {
        let mgr = ApiKeyManager::new();
        assert!(mgr.revoke("no-such-id").is_err());
    }

    #[test]
    fn test_list_for_tenant() {
        let mgr = ApiKeyManager::new();
        mgr.generate("t1", "a", vec![]);
        mgr.generate("t1", "b", vec![]);
        mgr.generate("t2", "c", vec![]);
        assert_eq!(mgr.list_for_tenant("t1").len(), 2);
        assert_eq!(mgr.list_for_tenant("t2").len(), 1);
        assert_eq!(mgr.list_for_tenant("t3").len(), 0);
    }

    #[test]
    fn test_key_hash_is_sha256() {
        let mgr = ApiKeyManager::new();
        let (key, plaintext) = mgr.generate("t1", "k", vec![]);
        assert_eq!(key.key_hash, hash_key(&plaintext));
    }

    #[test]
    fn test_key_default_rate_limit() {
        let mgr = ApiKeyManager::new();
        let (key, _) = mgr.generate("t1", "k", vec![]);
        assert_eq!(key.rate_limit, 60);
    }

    // -- TenantManager ------------------------------------------------------

    #[test]
    fn test_create_tenant() {
        let mgr = TenantManager::new();
        let t = mgr.create("Acme", Plan::Pro);
        assert_eq!(t.name, "Acme");
        assert_eq!(t.plan, Plan::Pro);
        assert!(t.is_active);
    }

    #[test]
    fn test_get_tenant() {
        let mgr = TenantManager::new();
        let t = mgr.create("Foo", Plan::Free);
        let fetched = mgr.get(&t.tenant_id).unwrap();
        assert_eq!(fetched.name, "Foo");
    }

    #[test]
    fn test_get_nonexistent_tenant() {
        let mgr = TenantManager::new();
        assert!(mgr.get("nope").is_none());
    }

    #[test]
    fn test_update_plan() {
        let mgr = TenantManager::new();
        let t = mgr.create("X", Plan::Free);
        mgr.update_plan(&t.tenant_id, Plan::Enterprise).unwrap();
        let updated = mgr.get(&t.tenant_id).unwrap();
        assert_eq!(updated.plan, Plan::Enterprise);
        assert_eq!(updated.usage_limits.max_sandboxes, 500);
    }

    #[test]
    fn test_update_plan_nonexistent() {
        let mgr = TenantManager::new();
        assert!(mgr.update_plan("nope", Plan::Pro).is_err());
    }

    #[test]
    fn test_list_tenants() {
        let mgr = TenantManager::new();
        mgr.create("A", Plan::Free);
        mgr.create("B", Plan::Pro);
        assert_eq!(mgr.list().len(), 2);
    }

    #[test]
    fn test_delete_tenant() {
        let mgr = TenantManager::new();
        let t = mgr.create("D", Plan::Free);
        mgr.delete(&t.tenant_id).unwrap();
        let d = mgr.get(&t.tenant_id).unwrap();
        assert!(!d.is_active);
    }

    #[test]
    fn test_delete_nonexistent() {
        let mgr = TenantManager::new();
        assert!(mgr.delete("nope").is_err());
    }

    // -- UsageTracker -------------------------------------------------------

    #[test]
    fn test_record_and_summarise() {
        let tm = TenantManager::new();
        let t = tm.create("U", Plan::Pro);
        let tracker = UsageTracker::new(tm);
        tracker.record(UsageRecord {
            tenant_id: t.tenant_id.clone(),
            timestamp: Utc::now(),
            sandbox_count: 3,
            fuel_consumed: 100,
            memory_bytes: 1024,
            api_calls: 5,
        });
        let s = tracker.get_current_period(&t.tenant_id);
        assert_eq!(s.total_sandboxes, 3);
        assert_eq!(s.total_fuel, 100);
        assert_eq!(s.peak_memory, 1024);
        assert_eq!(s.total_api_calls, 5);
    }

    #[test]
    fn test_check_limits_within() {
        let tm = TenantManager::new();
        let t = tm.create("L", Plan::Pro);
        let tracker = UsageTracker::new(tm);
        assert!(tracker.check_limits(&t.tenant_id).is_ok());
    }

    #[test]
    fn test_check_limits_exceeded() {
        let tm = TenantManager::new();
        let t = tm.create("L", Plan::Free);
        let tracker = UsageTracker::new(tm);
        // Free plan allows 5 sandboxes
        for _ in 0..5 {
            tracker.record(UsageRecord {
                tenant_id: t.tenant_id.clone(),
                timestamp: Utc::now(),
                sandbox_count: 1,
                fuel_consumed: 0,
                memory_bytes: 0,
                api_calls: 1,
            });
        }
        assert!(tracker.check_limits(&t.tenant_id).is_err());
    }

    #[test]
    fn test_check_limits_inactive_tenant() {
        let tm = TenantManager::new();
        let t = tm.create("I", Plan::Free);
        tm.delete(&t.tenant_id).unwrap();
        let tracker = UsageTracker::new(tm);
        assert!(tracker.check_limits(&t.tenant_id).is_err());
    }

    // -- SaasService --------------------------------------------------------

    #[test]
    fn test_authenticate_success() {
        let svc = SaasService::new();
        let tenant = svc.tenants.create("Svc", Plan::Pro);
        let (_key, plaintext) =
            svc.api_keys.generate(&tenant.tenant_id, "k", vec![ApiScope::Admin]);
        let ctx = svc.authenticate(&plaintext).unwrap();
        assert_eq!(ctx.tenant.tenant_id, tenant.tenant_id);
    }

    #[test]
    fn test_authenticate_invalid_key() {
        let svc = SaasService::new();
        assert!(svc.authenticate("bad_key").is_err());
    }

    #[test]
    fn test_authenticate_inactive_tenant() {
        let svc = SaasService::new();
        let t = svc.tenants.create("Dead", Plan::Free);
        let (_key, plain) = svc.api_keys.generate(&t.tenant_id, "k", vec![]);
        svc.tenants.delete(&t.tenant_id).unwrap();
        assert!(svc.authenticate(&plain).is_err());
    }

    #[test]
    fn test_create_sandbox_success() {
        let svc = SaasService::new();
        let t = svc.tenants.create("S", Plan::Pro);
        let (_, plain) =
            svc.api_keys.generate(&t.tenant_id, "k", vec![ApiScope::SandboxCreate]);
        let ctx = svc.authenticate(&plain).unwrap();
        let id = svc.create_sandbox_for_tenant(&ctx, "{}").unwrap();
        assert!(!id.0.is_empty());
    }

    #[test]
    fn test_create_sandbox_missing_scope() {
        let svc = SaasService::new();
        let t = svc.tenants.create("S", Plan::Pro);
        let (_, plain) =
            svc.api_keys.generate(&t.tenant_id, "k", vec![ApiScope::SandboxRead]);
        let ctx = svc.authenticate(&plain).unwrap();
        assert!(svc.create_sandbox_for_tenant(&ctx, "{}").is_err());
    }

    // -- TenantLimits -------------------------------------------------------

    #[test]
    fn test_tenant_limits_free() {
        let l = TenantLimits::for_plan(&Plan::Free);
        assert_eq!(l.max_sandboxes, 5);
        assert_eq!(l.max_api_keys, 3);
    }

    #[test]
    fn test_tenant_limits_enterprise() {
        let l = TenantLimits::for_plan(&Plan::Enterprise);
        assert_eq!(l.max_sandboxes, 500);
        assert_eq!(l.max_api_keys, 100);
    }
}
