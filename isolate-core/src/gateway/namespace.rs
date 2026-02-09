//! Multi-tenant namespace isolation.
//!
//! Provides per-tenant resource quotas, API key authentication scoping,
//! audit log partitioning, and blast radius containment for multi-tenant
//! sandbox deployments.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// Unique tenant namespace identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamespaceId(pub String);

impl NamespaceId {
    /// Create a new namespace ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for NamespaceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Per-tenant resource quota configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    /// Maximum concurrent sandbox executions.
    pub max_concurrent_sandboxes: u32,
    /// Maximum total memory across all sandboxes (bytes).
    pub max_total_memory: u64,
    /// Maximum CPU millicores across all sandboxes.
    pub max_total_cpu: u32,
    /// Maximum sandbox executions per minute.
    pub rate_limit_per_minute: u32,
    /// Maximum stored WASM modules.
    pub max_modules: u32,
    /// Maximum total storage for modules (bytes).
    pub max_storage_bytes: u64,
}

impl Default for TenantQuota {
    fn default() -> Self {
        Self {
            max_concurrent_sandboxes: 10,
            max_total_memory: 4 * 1024 * 1024 * 1024, // 4GB
            max_total_cpu: 4000,                        // 4 cores
            rate_limit_per_minute: 60,
            max_modules: 50,
            max_storage_bytes: 1024 * 1024 * 1024, // 1GB
        }
    }
}

/// Tracks current resource usage for a tenant.
#[derive(Debug)]
struct TenantUsage {
    concurrent_sandboxes: AtomicU64,
    total_memory: AtomicU64,
    total_cpu: AtomicU64,
    executions_this_minute: AtomicU64,
    minute_epoch: AtomicU64,
    stored_modules: AtomicU64,
    storage_bytes: AtomicU64,
}

impl TenantUsage {
    fn new() -> Self {
        Self {
            concurrent_sandboxes: AtomicU64::new(0),
            total_memory: AtomicU64::new(0),
            total_cpu: AtomicU64::new(0),
            executions_this_minute: AtomicU64::new(0),
            minute_epoch: AtomicU64::new(Self::current_minute()),
            stored_modules: AtomicU64::new(0),
            storage_bytes: AtomicU64::new(0),
        }
    }

    fn current_minute() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 60
    }

    fn rate_count(&self) -> u64 {
        let now = Self::current_minute();
        let stored = self.minute_epoch.load(Ordering::Relaxed);
        if now != stored {
            self.executions_this_minute.store(0, Ordering::Relaxed);
            self.minute_epoch.store(now, Ordering::Relaxed);
        }
        self.executions_this_minute.load(Ordering::Relaxed)
    }

    fn snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            concurrent_sandboxes: self.concurrent_sandboxes.load(Ordering::Relaxed) as u32,
            total_memory: self.total_memory.load(Ordering::Relaxed),
            total_cpu: self.total_cpu.load(Ordering::Relaxed) as u32,
            executions_this_minute: self.rate_count() as u32,
            stored_modules: self.stored_modules.load(Ordering::Relaxed) as u32,
            storage_bytes: self.storage_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of tenant resource usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub concurrent_sandboxes: u32,
    pub total_memory: u64,
    pub total_cpu: u32,
    pub executions_this_minute: u32,
    pub stored_modules: u32,
    pub storage_bytes: u64,
}

/// Namespace isolation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamespaceStatus {
    /// Active and accepting requests.
    Active,
    /// Suspended (e.g., for billing issues).
    Suspended,
    /// Read-only (can view but not create).
    ReadOnly,
    /// Being deleted.
    Terminating,
}

/// A tenant's namespace configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub id: NamespaceId,
    pub display_name: String,
    pub status: NamespaceStatus,
    pub quota: TenantQuota,
    /// Isolation level for blast radius containment.
    pub isolation: IsolationLevel,
    pub created_at_epoch_s: u64,
    pub labels: HashMap<String, String>,
}

/// Blast radius containment level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    /// Shared engine, separate instances (default, most efficient).
    Shared,
    /// Dedicated engine per tenant (stronger isolation).
    DedicatedEngine,
    /// Dedicated process per tenant (strongest isolation).
    DedicatedProcess,
}

impl Default for IsolationLevel {
    fn default() -> Self {
        Self::Shared
    }
}

/// Partitioned audit log entry for a tenant action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceAuditEntry {
    pub namespace_id: NamespaceId,
    pub timestamp_epoch_ms: u64,
    pub action: AuditAction,
    pub actor: String,
    pub details: String,
    pub request_id: Option<String>,
}

/// Types of auditable actions within a namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditAction {
    /// Sandbox was created.
    SandboxCreated,
    /// Sandbox execution completed.
    SandboxExecuted,
    /// Sandbox was deleted.
    SandboxDeleted,
    /// Module was uploaded.
    ModuleUploaded,
    /// Module was deleted.
    ModuleDeleted,
    /// API key was created.
    ApiKeyCreated,
    /// API key was revoked.
    ApiKeyRevoked,
    /// Quota was updated.
    QuotaUpdated,
    /// Namespace status changed.
    StatusChanged,
}

/// Quota check result.
#[derive(Debug, Clone)]
pub enum QuotaCheckResult {
    /// Within quota, proceed.
    Allowed,
    /// Quota exceeded, with reason.
    Denied(String),
}

/// Multi-tenant namespace manager.
///
/// Manages tenant namespaces, enforces quotas, tracks usage, and
/// provides partitioned audit logging.
pub struct NamespaceManager {
    namespaces: dashmap::DashMap<NamespaceId, Namespace>,
    usage: dashmap::DashMap<NamespaceId, TenantUsage>,
    audit_log: parking_lot::Mutex<Vec<NamespaceAuditEntry>>,
}

impl NamespaceManager {
    /// Create a new namespace manager.
    pub fn new() -> Self {
        Self {
            namespaces: dashmap::DashMap::new(),
            usage: dashmap::DashMap::new(),
            audit_log: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// Create a new tenant namespace.
    pub fn create_namespace(
        &self,
        id: impl Into<String>,
        display_name: impl Into<String>,
        quota: TenantQuota,
    ) -> NamespaceId {
        let ns_id = NamespaceId::new(id);
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let ns = Namespace {
            id: ns_id.clone(),
            display_name: display_name.into(),
            status: NamespaceStatus::Active,
            quota,
            isolation: IsolationLevel::default(),
            created_at_epoch_s: now,
            labels: HashMap::new(),
        };

        self.namespaces.insert(ns_id.clone(), ns);
        self.usage.insert(ns_id.clone(), TenantUsage::new());
        self.record_audit(&ns_id, AuditAction::StatusChanged, "system", "namespace created");
        ns_id
    }

    /// Check if a sandbox execution is allowed under the tenant's quota.
    pub fn check_sandbox_quota(
        &self,
        ns_id: &NamespaceId,
        memory_bytes: u64,
        cpu_millicores: u32,
    ) -> QuotaCheckResult {
        let ns = match self.namespaces.get(ns_id) {
            Some(ns) => ns,
            None => return QuotaCheckResult::Denied("namespace not found".into()),
        };

        if ns.status != NamespaceStatus::Active {
            return QuotaCheckResult::Denied(format!("namespace is {}", match ns.status {
                NamespaceStatus::Suspended => "suspended",
                NamespaceStatus::ReadOnly => "read-only",
                NamespaceStatus::Terminating => "terminating",
                NamespaceStatus::Active => "active",
            }));
        }

        let usage = match self.usage.get(ns_id) {
            Some(u) => u,
            None => return QuotaCheckResult::Denied("usage tracking not initialized".into()),
        };

        let current = usage.snapshot();

        if current.concurrent_sandboxes >= ns.quota.max_concurrent_sandboxes {
            return QuotaCheckResult::Denied(format!(
                "concurrent sandbox limit reached ({}/{})",
                current.concurrent_sandboxes, ns.quota.max_concurrent_sandboxes
            ));
        }

        if current.total_memory + memory_bytes > ns.quota.max_total_memory {
            return QuotaCheckResult::Denied("total memory quota exceeded".into());
        }

        if current.total_cpu + cpu_millicores > ns.quota.max_total_cpu {
            return QuotaCheckResult::Denied("total CPU quota exceeded".into());
        }

        if current.executions_this_minute >= ns.quota.rate_limit_per_minute {
            return QuotaCheckResult::Denied("rate limit exceeded".into());
        }

        QuotaCheckResult::Allowed
    }

    /// Record the start of a sandbox execution (allocates quota).
    pub fn record_sandbox_start(
        &self,
        ns_id: &NamespaceId,
        memory_bytes: u64,
        cpu_millicores: u32,
    ) {
        if let Some(usage) = self.usage.get(ns_id) {
            usage.concurrent_sandboxes.fetch_add(1, Ordering::Relaxed);
            usage.total_memory.fetch_add(memory_bytes, Ordering::Relaxed);
            usage.total_cpu.fetch_add(cpu_millicores as u64, Ordering::Relaxed);
            usage.executions_this_minute.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record the end of a sandbox execution (releases quota).
    pub fn record_sandbox_end(
        &self,
        ns_id: &NamespaceId,
        memory_bytes: u64,
        cpu_millicores: u32,
    ) {
        if let Some(usage) = self.usage.get(ns_id) {
            usage.concurrent_sandboxes.fetch_sub(1, Ordering::Relaxed);
            let prev_mem = usage.total_memory.fetch_sub(memory_bytes, Ordering::Relaxed);
            if prev_mem < memory_bytes {
                usage.total_memory.store(0, Ordering::Relaxed);
            }
            let prev_cpu = usage.total_cpu.fetch_sub(cpu_millicores as u64, Ordering::Relaxed);
            if prev_cpu < cpu_millicores as u64 {
                usage.total_cpu.store(0, Ordering::Relaxed);
            }
        }
    }

    /// Get current usage for a namespace.
    pub fn get_usage(&self, ns_id: &NamespaceId) -> Option<UsageSnapshot> {
        self.usage.get(ns_id).map(|u| u.snapshot())
    }

    /// Update namespace status (e.g., suspend, resume).
    pub fn set_status(&self, ns_id: &NamespaceId, status: NamespaceStatus) -> bool {
        if let Some(mut ns) = self.namespaces.get_mut(ns_id) {
            let old = ns.status;
            ns.status = status;
            self.record_audit(
                ns_id,
                AuditAction::StatusChanged,
                "system",
                &format!("{:?} -> {:?}", old, status),
            );
            true
        } else {
            false
        }
    }

    /// Update quota for a namespace.
    pub fn update_quota(&self, ns_id: &NamespaceId, quota: TenantQuota) -> bool {
        if let Some(mut ns) = self.namespaces.get_mut(ns_id) {
            ns.quota = quota;
            self.record_audit(ns_id, AuditAction::QuotaUpdated, "system", "quota updated");
            true
        } else {
            false
        }
    }

    /// Set isolation level for a namespace.
    pub fn set_isolation(&self, ns_id: &NamespaceId, level: IsolationLevel) -> bool {
        if let Some(mut ns) = self.namespaces.get_mut(ns_id) {
            ns.isolation = level;
            true
        } else {
            false
        }
    }

    /// Get namespace configuration.
    pub fn get_namespace(&self, ns_id: &NamespaceId) -> Option<Namespace> {
        self.namespaces.get(ns_id).map(|ns| ns.clone())
    }

    /// List all namespace IDs.
    pub fn list_namespaces(&self) -> Vec<NamespaceId> {
        self.namespaces.iter().map(|e| e.key().clone()).collect()
    }

    /// Record an audit log entry for a namespace.
    pub fn record_audit(
        &self,
        ns_id: &NamespaceId,
        action: AuditAction,
        actor: &str,
        details: &str,
    ) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.audit_log.lock().push(NamespaceAuditEntry {
            namespace_id: ns_id.clone(),
            timestamp_epoch_ms: now,
            action,
            actor: actor.to_string(),
            details: details.to_string(),
            request_id: None,
        });
    }

    /// Get audit log entries for a specific namespace (partitioned view).
    pub fn audit_log_for(&self, ns_id: &NamespaceId) -> Vec<NamespaceAuditEntry> {
        self.audit_log
            .lock()
            .iter()
            .filter(|e| e.namespace_id == *ns_id)
            .cloned()
            .collect()
    }

    /// Total audit log entries across all namespaces.
    pub fn total_audit_entries(&self) -> usize {
        self.audit_log.lock().len()
    }

    /// Delete a namespace and all its tracking data.
    pub fn delete_namespace(&self, ns_id: &NamespaceId) -> bool {
        let removed = self.namespaces.remove(ns_id).is_some();
        self.usage.remove(ns_id);
        removed
    }
}

impl Default for NamespaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_namespace() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("tenant-1", "Tenant One", TenantQuota::default());
        assert_eq!(ns_id, NamespaceId::new("tenant-1"));

        let ns = mgr.get_namespace(&ns_id).unwrap();
        assert_eq!(ns.display_name, "Tenant One");
        assert_eq!(ns.status, NamespaceStatus::Active);
    }

    #[test]
    fn test_quota_enforcement() {
        let mgr = NamespaceManager::new();
        let mut quota = TenantQuota::default();
        quota.max_concurrent_sandboxes = 2;
        let ns_id = mgr.create_namespace("t1", "Test", quota);

        // First two should be allowed
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 64 * 1024 * 1024, 500),
            QuotaCheckResult::Allowed
        ));
        mgr.record_sandbox_start(&ns_id, 64 * 1024 * 1024, 500);

        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 64 * 1024 * 1024, 500),
            QuotaCheckResult::Allowed
        ));
        mgr.record_sandbox_start(&ns_id, 64 * 1024 * 1024, 500);

        // Third should be denied
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 64 * 1024 * 1024, 500),
            QuotaCheckResult::Denied(_)
        ));
    }

    #[test]
    fn test_quota_release() {
        let mgr = NamespaceManager::new();
        let mut quota = TenantQuota::default();
        quota.max_concurrent_sandboxes = 1;
        let ns_id = mgr.create_namespace("t1", "Test", quota);

        mgr.record_sandbox_start(&ns_id, 1024, 500);
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 1024, 500),
            QuotaCheckResult::Denied(_)
        ));

        mgr.record_sandbox_end(&ns_id, 1024, 500);
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 1024, 500),
            QuotaCheckResult::Allowed
        ));
    }

    #[test]
    fn test_memory_quota() {
        let mgr = NamespaceManager::new();
        let mut quota = TenantQuota::default();
        quota.max_total_memory = 100 * 1024 * 1024; // 100MB
        let ns_id = mgr.create_namespace("t1", "Test", quota);

        // 80MB should be fine
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 80 * 1024 * 1024, 500),
            QuotaCheckResult::Allowed
        ));
        mgr.record_sandbox_start(&ns_id, 80 * 1024 * 1024, 500);

        // Another 30MB should exceed
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 30 * 1024 * 1024, 500),
            QuotaCheckResult::Denied(_)
        ));
    }

    #[test]
    fn test_suspended_namespace() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("t1", "Test", TenantQuota::default());

        mgr.set_status(&ns_id, NamespaceStatus::Suspended);
        assert!(matches!(
            mgr.check_sandbox_quota(&ns_id, 1024, 500),
            QuotaCheckResult::Denied(_)
        ));
    }

    #[test]
    fn test_namespace_not_found() {
        let mgr = NamespaceManager::new();
        let fake_id = NamespaceId::new("nonexistent");
        assert!(matches!(
            mgr.check_sandbox_quota(&fake_id, 1024, 500),
            QuotaCheckResult::Denied(_)
        ));
    }

    #[test]
    fn test_audit_log_partitioning() {
        let mgr = NamespaceManager::new();
        let ns1 = mgr.create_namespace("t1", "One", TenantQuota::default());
        let ns2 = mgr.create_namespace("t2", "Two", TenantQuota::default());

        mgr.record_audit(&ns1, AuditAction::SandboxCreated, "user-1", "created sb-1");
        mgr.record_audit(&ns2, AuditAction::SandboxCreated, "user-2", "created sb-2");
        mgr.record_audit(&ns1, AuditAction::SandboxExecuted, "user-1", "executed sb-1");

        let ns1_log = mgr.audit_log_for(&ns1);
        let ns2_log = mgr.audit_log_for(&ns2);

        // ns1 has creation + sandbox_created + sandbox_executed = 3
        assert_eq!(ns1_log.len(), 3);
        // ns2 has creation + sandbox_created = 2
        assert_eq!(ns2_log.len(), 2);

        // Partitioned: ns1 log shouldn't contain ns2 entries
        assert!(ns1_log.iter().all(|e| e.namespace_id == ns1));
        assert!(ns2_log.iter().all(|e| e.namespace_id == ns2));
    }

    #[test]
    fn test_isolation_level() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("t1", "Test", TenantQuota::default());

        let ns = mgr.get_namespace(&ns_id).unwrap();
        assert_eq!(ns.isolation, IsolationLevel::Shared);

        mgr.set_isolation(&ns_id, IsolationLevel::DedicatedEngine);
        let ns = mgr.get_namespace(&ns_id).unwrap();
        assert_eq!(ns.isolation, IsolationLevel::DedicatedEngine);
    }

    #[test]
    fn test_usage_tracking() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("t1", "Test", TenantQuota::default());

        let usage = mgr.get_usage(&ns_id).unwrap();
        assert_eq!(usage.concurrent_sandboxes, 0);

        mgr.record_sandbox_start(&ns_id, 1024, 500);
        let usage = mgr.get_usage(&ns_id).unwrap();
        assert_eq!(usage.concurrent_sandboxes, 1);
        assert_eq!(usage.total_memory, 1024);
        assert_eq!(usage.total_cpu, 500);
    }

    #[test]
    fn test_list_namespaces() {
        let mgr = NamespaceManager::new();
        mgr.create_namespace("t1", "One", TenantQuota::default());
        mgr.create_namespace("t2", "Two", TenantQuota::default());

        let ids = mgr.list_namespaces();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_delete_namespace() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("t1", "Test", TenantQuota::default());
        assert!(mgr.delete_namespace(&ns_id));
        assert!(mgr.get_namespace(&ns_id).is_none());
        assert!(mgr.get_usage(&ns_id).is_none());
    }

    #[test]
    fn test_update_quota() {
        let mgr = NamespaceManager::new();
        let ns_id = mgr.create_namespace("t1", "Test", TenantQuota::default());

        let mut new_quota = TenantQuota::default();
        new_quota.max_concurrent_sandboxes = 100;
        mgr.update_quota(&ns_id, new_quota);

        let ns = mgr.get_namespace(&ns_id).unwrap();
        assert_eq!(ns.quota.max_concurrent_sandboxes, 100);
    }
}
