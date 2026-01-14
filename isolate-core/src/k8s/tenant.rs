//! Multi-tenancy and namespace isolation for Kubernetes deployments.
//!
//! Provides tenant management, quota enforcement, RBAC-style permissions,
//! and usage tracking for multi-tenant Isolate clusters.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tenant definition for multi-tenant isolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub quota: TenantQuota,
    pub labels: HashMap<String, String>,
    pub isolation_level: IsolationLevel,
    pub status: TenantStatus,
}

/// Resource quotas for a tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantQuota {
    pub max_sandboxes: u32,
    pub max_memory_mb: u64,
    pub max_cpu_fuel_per_hour: u64,
    pub max_storage_mb: u64,
    pub max_network_egress_mb: u64,
}

/// Level of isolation between tenants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    Shared,
    Namespace,
    Node,
    Cluster,
}

/// Current status of a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TenantStatus {
    Active,
    Suspended,
    PendingDeletion,
}

/// RBAC role for tenant access control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantRole {
    pub name: String,
    pub tenant_id: String,
    pub permissions: Vec<Permission>,
}

/// Permission actions for tenant RBAC.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    CreateSandbox,
    DeleteSandbox,
    ViewSandbox,
    ManageQuota,
    ViewMetrics,
    ManageSecrets,
    Admin,
}

/// Current resource usage for a tenant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TenantUsage {
    pub active_sandboxes: u32,
    pub memory_used_mb: u64,
    pub fuel_used_per_hour: u64,
    pub storage_used_mb: u64,
}

/// Error returned when a quota would be exceeded.
pub struct QuotaExceeded {
    pub resource: String,
    pub limit: u64,
    pub requested: u64,
    pub current: u64,
}

impl std::fmt::Display for QuotaExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "quota exceeded for {}: limit={}, current={}, requested={}",
            self.resource, self.limit, self.current, self.requested
        )
    }
}

impl std::fmt::Debug for QuotaExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaExceeded")
            .field("resource", &self.resource)
            .field("limit", &self.limit)
            .field("requested", &self.requested)
            .field("current", &self.current)
            .finish()
    }
}

/// Manages tenants, quotas, RBAC roles, and usage tracking.
pub struct TenantManager {
    tenants: HashMap<String, Tenant>,
    roles: Vec<TenantRole>,
    usage: HashMap<String, TenantUsage>,
}

impl TenantManager {
    /// Create a new empty tenant manager.
    pub fn new() -> Self {
        Self { tenants: HashMap::new(), roles: Vec::new(), usage: HashMap::new() }
    }

    /// Register a new tenant.
    pub fn register_tenant(&mut self, tenant: Tenant) {
        let id = tenant.id.clone();
        self.usage.entry(id.clone()).or_insert_with(TenantUsage::default);
        self.tenants.insert(id, tenant);
    }

    /// Remove a tenant by ID. Returns the removed tenant if found.
    pub fn remove_tenant(&mut self, id: &str) -> Option<Tenant> {
        self.usage.remove(id);
        self.roles.retain(|r| r.tenant_id != id);
        self.tenants.remove(id)
    }

    /// Get a tenant by ID.
    pub fn get_tenant(&self, id: &str) -> Option<&Tenant> {
        self.tenants.get(id)
    }

    /// List all tenants.
    pub fn list_tenants(&self) -> Vec<&Tenant> {
        self.tenants.values().collect()
    }

    /// Update resource usage for a tenant.
    pub fn update_usage(&mut self, tenant_id: &str, usage: TenantUsage) {
        self.usage.insert(tenant_id.to_string(), usage);
    }

    /// Check whether a resource request would exceed the tenant's quota.
    pub fn check_quota(
        &self,
        tenant_id: &str,
        requested_sandboxes: u32,
        requested_memory_mb: u64,
    ) -> Result<(), QuotaExceeded> {
        let tenant = match self.tenants.get(tenant_id) {
            Some(t) => t,
            None => return Ok(()),
        };
        let usage = self.usage.get(tenant_id).cloned().unwrap_or_default();

        if usage.active_sandboxes + requested_sandboxes > tenant.quota.max_sandboxes {
            return Err(QuotaExceeded {
                resource: "sandboxes".to_string(),
                limit: tenant.quota.max_sandboxes as u64,
                requested: requested_sandboxes as u64,
                current: usage.active_sandboxes as u64,
            });
        }

        if usage.memory_used_mb + requested_memory_mb > tenant.quota.max_memory_mb {
            return Err(QuotaExceeded {
                resource: "memory_mb".to_string(),
                limit: tenant.quota.max_memory_mb,
                requested: requested_memory_mb,
                current: usage.memory_used_mb,
            });
        }

        Ok(())
    }

    /// Assign an RBAC role.
    pub fn assign_role(&mut self, role: TenantRole) {
        self.roles.push(role);
    }

    /// Check whether a user has a specific permission for a tenant.
    pub fn check_permission(&self, tenant_id: &str, user: &str, permission: &Permission) -> bool {
        self.roles.iter().any(|r| {
            r.tenant_id == tenant_id
                && r.name == user
                && (r.permissions.contains(permission)
                    || r.permissions.contains(&Permission::Admin))
        })
    }

    /// Suspend a tenant. Returns false if the tenant was not found.
    pub fn suspend_tenant(&mut self, id: &str) -> bool {
        if let Some(t) = self.tenants.get_mut(id) {
            t.status = TenantStatus::Suspended;
            true
        } else {
            false
        }
    }

    /// Activate a tenant. Returns false if the tenant was not found.
    pub fn activate_tenant(&mut self, id: &str) -> bool {
        if let Some(t) = self.tenants.get_mut(id) {
            t.status = TenantStatus::Active;
            true
        } else {
            false
        }
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tenant(id: &str) -> Tenant {
        Tenant {
            id: id.to_string(),
            name: format!("Tenant {}", id),
            namespace: format!("ns-{}", id),
            quota: TenantQuota {
                max_sandboxes: 10,
                max_memory_mb: 1024,
                max_cpu_fuel_per_hour: 100_000,
                max_storage_mb: 5120,
                max_network_egress_mb: 512,
            },
            labels: HashMap::new(),
            isolation_level: IsolationLevel::Namespace,
            status: TenantStatus::Active,
        }
    }

    #[test]
    fn test_register_and_get_tenant() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        let t = mgr.get_tenant("t1").unwrap();
        assert_eq!(t.name, "Tenant t1");
        assert_eq!(t.namespace, "ns-t1");
    }

    #[test]
    fn test_remove_tenant() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        let removed = mgr.remove_tenant("t1");
        assert!(removed.is_some());
        assert!(mgr.get_tenant("t1").is_none());
    }

    #[test]
    fn test_list_tenants() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("a"));
        mgr.register_tenant(make_tenant("b"));
        let list = mgr.list_tenants();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_check_quota_within_limits() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        assert!(mgr.check_quota("t1", 1, 128).is_ok());
    }

    #[test]
    fn test_check_quota_sandboxes_exceeded() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        mgr.update_usage(
            "t1",
            TenantUsage {
                active_sandboxes: 9,
                memory_used_mb: 0,
                fuel_used_per_hour: 0,
                storage_used_mb: 0,
            },
        );
        let err = mgr.check_quota("t1", 2, 0).unwrap_err();
        assert_eq!(err.resource, "sandboxes");
        assert_eq!(err.limit, 10);
    }

    #[test]
    fn test_check_quota_memory_exceeded() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        mgr.update_usage(
            "t1",
            TenantUsage {
                active_sandboxes: 0,
                memory_used_mb: 900,
                fuel_used_per_hour: 0,
                storage_used_mb: 0,
            },
        );
        let err = mgr.check_quota("t1", 0, 200).unwrap_err();
        assert_eq!(err.resource, "memory_mb");
    }

    #[test]
    fn test_assign_role_and_check_permission() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        mgr.assign_role(TenantRole {
            name: "alice".to_string(),
            tenant_id: "t1".to_string(),
            permissions: vec![Permission::CreateSandbox, Permission::ViewSandbox],
        });
        assert!(mgr.check_permission("t1", "alice", &Permission::CreateSandbox));
        assert!(mgr.check_permission("t1", "alice", &Permission::ViewSandbox));
        assert!(!mgr.check_permission("t1", "alice", &Permission::Admin));
        assert!(!mgr.check_permission("t1", "bob", &Permission::CreateSandbox));
    }

    #[test]
    fn test_admin_permission_grants_all() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        mgr.assign_role(TenantRole {
            name: "admin-user".to_string(),
            tenant_id: "t1".to_string(),
            permissions: vec![Permission::Admin],
        });
        assert!(mgr.check_permission("t1", "admin-user", &Permission::CreateSandbox));
        assert!(mgr.check_permission("t1", "admin-user", &Permission::ManageQuota));
    }

    #[test]
    fn test_suspend_and_activate_tenant() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));

        assert!(mgr.suspend_tenant("t1"));
        assert_eq!(mgr.get_tenant("t1").unwrap().status, TenantStatus::Suspended);

        assert!(mgr.activate_tenant("t1"));
        assert_eq!(mgr.get_tenant("t1").unwrap().status, TenantStatus::Active);

        assert!(!mgr.suspend_tenant("nonexistent"));
        assert!(!mgr.activate_tenant("nonexistent"));
    }

    #[test]
    fn test_remove_tenant_cleans_roles() {
        let mut mgr = TenantManager::new();
        mgr.register_tenant(make_tenant("t1"));
        mgr.assign_role(TenantRole {
            name: "alice".to_string(),
            tenant_id: "t1".to_string(),
            permissions: vec![Permission::ViewSandbox],
        });
        mgr.remove_tenant("t1");
        assert!(!mgr.check_permission("t1", "alice", &Permission::ViewSandbox));
    }

    #[test]
    fn test_quota_exceeded_display() {
        let err = QuotaExceeded {
            resource: "sandboxes".to_string(),
            limit: 10,
            requested: 2,
            current: 9,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("sandboxes"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_tenant_serialization() {
        let tenant = make_tenant("ser");
        let json = serde_json::to_string(&tenant).unwrap();
        let back: Tenant = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "ser");
        assert_eq!(back.isolation_level, IsolationLevel::Namespace);
    }
}
