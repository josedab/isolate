//! Hierarchical multi-tenant quota management.
//!
//! Provides a tree-based namespace hierarchy (org → team → project) where
//! resource quotas are checked and usage is propagated up the tree.



use super::quota::TenantQuota;
use crate::error::{Error, Result};
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// A resource request to check against the hierarchy.
#[derive(Debug, Clone)]
pub struct ResourceRequest {
    /// Memory in bytes to allocate.
    pub memory_bytes: u64,
    /// Number of sandboxes to allocate.
    pub sandbox_count: u32,
}

impl ResourceRequest {
    /// Create a new resource request.
    pub fn new(memory_bytes: u64, sandbox_count: u32) -> Self {
        Self { memory_bytes, sandbox_count }
    }
}

/// Result of a quota check against the hierarchy.
#[derive(Debug, Clone)]
pub enum QuotaCheckResult {
    /// The request is allowed.
    Allowed,
    /// The request is denied.
    Denied {
        /// Why the request was denied.
        reason: String,
        /// Which namespace denied the request.
        namespace: String,
    },
}

impl QuotaCheckResult {
    /// Returns true if the request was allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, QuotaCheckResult::Allowed)
    }

    /// Returns true if the request was denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, QuotaCheckResult::Denied { .. })
    }
}

/// Aggregated usage tracking with atomic counters.
#[derive(Debug)]
struct AggregatedUsage {
    memory_bytes: AtomicU64,
    active_sandboxes: AtomicU64,
}

impl AggregatedUsage {
    fn new() -> Self {
        Self {
            memory_bytes: AtomicU64::new(0),
            active_sandboxes: AtomicU64::new(0),
        }
    }
}

impl Default for AggregatedUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// A node in the tenant hierarchy (org → team → project).
pub struct Namespace {
    /// Name of this namespace segment.
    pub name: String,
    /// Full path of the parent namespace, or `None` for root nodes.
    pub parent: Option<String>,
    /// Quota assigned to this namespace.
    pub quota: TenantQuota,
    /// Aggregated usage tracking.
    usage: AggregatedUsage,
}

impl Namespace {
    /// Create a new namespace node.
    fn new(name: String, parent: Option<String>, quota: TenantQuota) -> Self {
        Self {
            name,
            parent,
            quota,
            usage: AggregatedUsage::new(),
        }
    }

    /// Get current memory usage.
    pub fn memory_usage(&self) -> u64 {
        self.usage.memory_bytes.load(Ordering::SeqCst)
    }

    /// Get current active sandbox count.
    pub fn active_sandboxes(&self) -> u64 {
        self.usage.active_sandboxes.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Namespace")
            .field("name", &self.name)
            .field("parent", &self.parent)
            .field("memory_usage", &self.memory_usage())
            .field("active_sandboxes", &self.active_sandboxes())
            .finish_non_exhaustive()
    }
}

/// Hierarchical quota manager for multi-tenant namespace trees.
///
/// Namespaces are addressed by slash-separated paths (e.g. `"org/team/project"`).
/// Quota checks walk up the tree so that every ancestor's limits are respected.
pub struct QuotaHierarchy {
    namespaces: DashMap<String, Namespace>,
}

impl QuotaHierarchy {
    /// Create an empty hierarchy.
    pub fn new() -> Self {
        Self { namespaces: DashMap::new() }
    }

    /// Create a namespace at the given path.
    ///
    /// All ancestor namespaces must already exist. The path uses `/` as a
    /// separator (e.g. `"org/team/project"`).
    pub fn create_namespace(&self, path: &str, quota: TenantQuota) -> Result<()> {
        if path.is_empty() {
            return Err(Error::InvalidConfig("namespace path cannot be empty".into()));
        }

        if self.namespaces.contains_key(path) {
            return Err(Error::InvalidConfig(format!(
                "namespace '{}' already exists",
                path
            )));
        }

        let parent = parent_path(path);

        // Ensure the parent exists (if there is one).
        if let Some(ref p) = parent {
            if !self.namespaces.contains_key(p.as_str()) {
                return Err(Error::InvalidConfig(format!(
                    "parent namespace '{}' does not exist",
                    p
                )));
            }
        }

        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        self.namespaces.insert(path.to_string(), Namespace::new(name, parent, quota));
        Ok(())
    }

    /// Check whether a resource request is allowed at `path`.
    ///
    /// Walks up the tree checking every ancestor's quota against its aggregated
    /// usage. Returns [`QuotaCheckResult::Denied`] at the first namespace that
    /// would be exceeded.
    pub fn check_quota(&self, path: &str, request: &ResourceRequest) -> Result<QuotaCheckResult> {
        let mut current = Some(path.to_string());

        while let Some(ref ns_path) = current {
            let ns = self
                .namespaces
                .get(ns_path.as_str())
                .ok_or_else(|| Error::InvalidConfig(format!("namespace '{}' not found", ns_path)))?;

            let used_memory = ns.usage.memory_bytes.load(Ordering::SeqCst);
            if used_memory + request.memory_bytes > ns.quota.max_memory {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!(
                        "memory limit exceeded: {} + {} > {}",
                        used_memory, request.memory_bytes, ns.quota.max_memory
                    ),
                    namespace: ns_path.clone(),
                });
            }

            let used_sandboxes = ns.usage.active_sandboxes.load(Ordering::SeqCst);
            if used_sandboxes + request.sandbox_count as u64 > ns.quota.max_sandboxes as u64 {
                return Ok(QuotaCheckResult::Denied {
                    reason: format!(
                        "sandbox limit exceeded: {} + {} > {}",
                        used_sandboxes, request.sandbox_count, ns.quota.max_sandboxes
                    ),
                    namespace: ns_path.clone(),
                });
            }

            current = ns.parent.clone();
        }

        Ok(QuotaCheckResult::Allowed)
    }

    /// Record usage at `path` and propagate to all ancestors.
    pub fn record_usage(&self, path: &str, memory_bytes: u64, sandbox_count: u32) -> Result<()> {
        let mut current = Some(path.to_string());

        while let Some(ref ns_path) = current {
            let ns = self
                .namespaces
                .get(ns_path.as_str())
                .ok_or_else(|| Error::InvalidConfig(format!("namespace '{}' not found", ns_path)))?;

            ns.usage.memory_bytes.fetch_add(memory_bytes, Ordering::SeqCst);
            ns.usage.active_sandboxes.fetch_add(sandbox_count as u64, Ordering::SeqCst);

            current = ns.parent.clone();
        }

        Ok(())
    }

    /// Release usage at `path` and propagate to all ancestors.
    pub fn release_usage(&self, path: &str, memory_bytes: u64, sandbox_count: u32) -> Result<()> {
        let mut current = Some(path.to_string());

        while let Some(ref ns_path) = current {
            let ns = self
                .namespaces
                .get(ns_path.as_str())
                .ok_or_else(|| Error::InvalidConfig(format!("namespace '{}' not found", ns_path)))?;

            // Saturating subtract to avoid underflow.
            let prev_mem = ns.usage.memory_bytes.load(Ordering::SeqCst);
            ns.usage
                .memory_bytes
                .store(prev_mem.saturating_sub(memory_bytes), Ordering::SeqCst);

            let prev_sb = ns.usage.active_sandboxes.load(Ordering::SeqCst);
            ns.usage
                .active_sandboxes
                .store(prev_sb.saturating_sub(sandbox_count as u64), Ordering::SeqCst);

            current = ns.parent.clone();
        }

        Ok(())
    }

    /// Get the effective quota at `path` — the component-wise minimum from the
    /// leaf up to the root.
    pub fn get_effective_quota(&self, path: &str) -> Result<TenantQuota> {
        let mut effective = self
            .namespaces
            .get(path)
            .ok_or_else(|| Error::InvalidConfig(format!("namespace '{}' not found", path)))?
            .quota
            .clone();

        let mut current = self.namespaces.get(path).and_then(|ns| ns.parent.clone());

        while let Some(ref ns_path) = current {
            let ns = self
                .namespaces
                .get(ns_path.as_str())
                .ok_or_else(|| Error::InvalidConfig(format!("namespace '{}' not found", ns_path)))?;

            effective.max_memory = effective.max_memory.min(ns.quota.max_memory);
            effective.max_sandboxes = effective.max_sandboxes.min(ns.quota.max_sandboxes);
            effective.max_cpu_time_ms = effective.max_cpu_time_ms.min(ns.quota.max_cpu_time_ms);
            effective.max_io_bytes = effective.max_io_bytes.min(ns.quota.max_io_bytes);
            effective.max_rps = effective.max_rps.min(ns.quota.max_rps);

            current = ns.parent.clone();
        }

        Ok(effective)
    }

    /// List direct child namespace paths.
    pub fn list_children(&self, path: &str) -> Vec<String> {
        let parent_match = Some(path.to_string());
        self.namespaces
            .iter()
            .filter(|entry| entry.value().parent == parent_match)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Check whether a namespace exists.
    pub fn contains(&self, path: &str) -> bool {
        self.namespaces.contains_key(path)
    }
}

impl Default for QuotaHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for QuotaHierarchy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaHierarchy")
            .field("namespace_count", &self.namespaces.len())
            .finish()
    }
}

/// Extract the parent path from a `/`-separated namespace path.
fn parent_path(path: &str) -> Option<String> {
    let pos = path.rfind('/')?;
    Some(path[..pos].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gb(n: u64) -> u64 {
        n * 1024 * 1024 * 1024
    }

    fn mb(n: u64) -> u64 {
        n * 1024 * 1024
    }

    // ── hierarchy creation ──────────────────────────────────────────────

    #[test]
    fn test_create_root_namespace() {
        let h = QuotaHierarchy::new();
        h.create_namespace("acme", TenantQuota::new()).expect("create namespace acme");
        assert!(h.contains("acme"));
    }

    #[test]
    fn test_create_nested_namespace() {
        let h = QuotaHierarchy::new();
        h.create_namespace("acme", TenantQuota::new()).expect("create namespace acme");
        h.create_namespace("acme/platform", TenantQuota::new()).expect("create namespace acme/platform");
        h.create_namespace("acme/platform/api", TenantQuota::new()).expect("create namespace acme/platform/api");

        assert!(h.contains("acme"));
        assert!(h.contains("acme/platform"));
        assert!(h.contains("acme/platform/api"));
    }

    #[test]
    fn test_create_namespace_missing_parent() {
        let h = QuotaHierarchy::new();
        let result = h.create_namespace("acme/platform", TenantQuota::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_namespace_duplicate() {
        let h = QuotaHierarchy::new();
        h.create_namespace("acme", TenantQuota::new()).expect("create namespace acme");
        let result = h.create_namespace("acme", TenantQuota::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_create_namespace_empty_path() {
        let h = QuotaHierarchy::new();
        let result = h.create_namespace("", TenantQuota::new());
        assert!(result.is_err());
    }

    // ── list children ───────────────────────────────────────────────────

    #[test]
    fn test_list_children() {
        let h = QuotaHierarchy::new();
        h.create_namespace("acme", TenantQuota::new()).expect("create namespace acme");
        h.create_namespace("acme/alpha", TenantQuota::new()).expect("create namespace acme/alpha");
        h.create_namespace("acme/beta", TenantQuota::new()).expect("create namespace acme/beta");
        h.create_namespace("acme/alpha/svc", TenantQuota::new()).expect("create namespace acme/alpha/svc");

        let mut children = h.list_children("acme");
        children.sort();
        assert_eq!(children, vec!["acme/alpha", "acme/beta"]);

        let grandchildren = h.list_children("acme/alpha");
        assert_eq!(grandchildren, vec!["acme/alpha/svc"]);
    }

    #[test]
    fn test_list_children_empty() {
        let h = QuotaHierarchy::new();
        h.create_namespace("acme", TenantQuota::new()).expect("create namespace acme");
        assert!(h.list_children("acme").is_empty());
    }

    // ── quota propagation & checking ────────────────────────────────────

    #[test]
    fn test_check_quota_allowed() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(gb(4)).with_max_sandboxes(100))
            .expect("create namespace org");
        h.create_namespace(
            "org/team",
            TenantQuota::new().with_max_memory(gb(2)).with_max_sandboxes(50),
        )
        .expect("create namespace org/team");

        let req = ResourceRequest::new(mb(512), 1);
        let result = h.check_quota("org/team", &req).expect("check quota for org/team");
        assert!(result.is_allowed());
    }

    #[test]
    fn test_check_quota_denied_at_leaf() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(gb(4))).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new().with_max_memory(mb(100))).expect("create namespace org/team");

        let req = ResourceRequest::new(mb(200), 0);
        let result = h.check_quota("org/team", &req).expect("check quota for org/team");
        assert!(result.is_denied());

        if let QuotaCheckResult::Denied { namespace, .. } = result {
            assert_eq!(namespace, "org/team");
        }
    }

    #[test]
    fn test_check_quota_denied_at_ancestor() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(mb(100))).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new().with_max_memory(gb(1))).expect("create namespace org/team");

        // Record usage at the org level via a sibling.
        h.create_namespace("org/other", TenantQuota::new().with_max_memory(gb(1))).expect("create namespace org/other");
        h.record_usage("org/other", mb(90), 0).expect("record usage for org/other");

        // Now the org has 90 MB used; requesting 20 MB on team should fail at org.
        let req = ResourceRequest::new(mb(20), 0);
        let result = h.check_quota("org/team", &req).expect("check quota for org/team");
        assert!(result.is_denied());

        if let QuotaCheckResult::Denied { namespace, .. } = result {
            assert_eq!(namespace, "org");
        }
    }

    #[test]
    fn test_check_quota_sandbox_limit() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_sandboxes(5)).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new().with_max_sandboxes(10)).expect("create namespace org/team");

        h.record_usage("org/team", 0, 4).expect("record usage for org/team");

        // 4 + 2 > 5 at the org level
        let req = ResourceRequest::new(0, 2);
        let result = h.check_quota("org/team", &req).expect("check quota for org/team");
        assert!(result.is_denied());

        if let QuotaCheckResult::Denied { namespace, .. } = result {
            assert_eq!(namespace, "org");
        }
    }

    // ── usage tracking ──────────────────────────────────────────────────

    #[test]
    fn test_record_usage_propagates() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new()).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new()).expect("create namespace org/team");
        h.create_namespace("org/team/proj", TenantQuota::new()).expect("create namespace org/team/proj");

        h.record_usage("org/team/proj", 1000, 2).expect("record usage for org/team/proj");

        // Usage should be visible at every level.
        assert_eq!(h.namespaces.get("org/team/proj").expect("namespace org/team/proj should exist").memory_usage(), 1000);
        assert_eq!(h.namespaces.get("org/team").expect("namespace org/team should exist").memory_usage(), 1000);
        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").memory_usage(), 1000);

        assert_eq!(h.namespaces.get("org/team/proj").expect("namespace org/team/proj should exist").active_sandboxes(), 2);
        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").active_sandboxes(), 2);
    }

    #[test]
    fn test_release_usage_propagates() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new()).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new()).expect("create namespace org/team");

        h.record_usage("org/team", 5000, 3).expect("record usage for org/team");
        h.release_usage("org/team", 2000, 1).expect("release usage for org/team");

        assert_eq!(h.namespaces.get("org/team").expect("namespace org/team should exist").memory_usage(), 3000);
        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").memory_usage(), 3000);
        assert_eq!(h.namespaces.get("org/team").expect("namespace org/team should exist").active_sandboxes(), 2);
        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").active_sandboxes(), 2);
    }

    #[test]
    fn test_release_usage_saturates() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new()).expect("create namespace org");

        h.record_usage("org", 100, 1).expect("record usage for org");
        // Release more than recorded — should saturate at zero.
        h.release_usage("org", 500, 5).expect("release usage for org");

        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").memory_usage(), 0);
        assert_eq!(h.namespaces.get("org").expect("namespace org should exist").active_sandboxes(), 0);
    }

    // ── effective quota ─────────────────────────────────────────────────

    #[test]
    fn test_effective_quota_single_node() {
        let h = QuotaHierarchy::new();
        h.create_namespace(
            "org",
            TenantQuota::new().with_max_memory(gb(2)).with_max_sandboxes(50),
        )
        .expect("create namespace org");

        let eff = h.get_effective_quota("org").expect("get effective quota for org");
        assert_eq!(eff.max_memory, gb(2));
        assert_eq!(eff.max_sandboxes, 50);
    }

    #[test]
    fn test_effective_quota_takes_minimum() {
        let h = QuotaHierarchy::new();
        h.create_namespace(
            "org",
            TenantQuota::new()
                .with_max_memory(gb(4))
                .with_max_sandboxes(20)
                .with_max_cpu_time_ms(60_000),
        )
        .expect("create namespace org");
        h.create_namespace(
            "org/team",
            TenantQuota::new()
                .with_max_memory(gb(8))
                .with_max_sandboxes(100)
                .with_max_cpu_time_ms(30_000),
        )
        .expect("create namespace org/team");

        let eff = h.get_effective_quota("org/team").expect("get effective quota for org/team");
        // memory: min(8G, 4G) = 4G
        assert_eq!(eff.max_memory, gb(4));
        // sandboxes: min(100, 20) = 20
        assert_eq!(eff.max_sandboxes, 20);
        // cpu: min(30_000, 60_000) = 30_000
        assert_eq!(eff.max_cpu_time_ms, 30_000);
    }

    #[test]
    fn test_effective_quota_three_levels() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(gb(10))).expect("create namespace org");
        h.create_namespace("org/team", TenantQuota::new().with_max_memory(gb(5))).expect("create namespace org/team");
        h.create_namespace("org/team/proj", TenantQuota::new().with_max_memory(gb(2))).expect("create namespace org/team/proj");

        let eff = h.get_effective_quota("org/team/proj").expect("get effective quota for org/team/proj");
        assert_eq!(eff.max_memory, gb(2));
    }

    #[test]
    fn test_effective_quota_not_found() {
        let h = QuotaHierarchy::new();
        let result = h.get_effective_quota("nonexistent");
        assert!(result.is_err());
    }

    // ── denial when exceeded ────────────────────────────────────────────

    #[test]
    fn test_denied_after_recording_usage() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(mb(100)).with_max_sandboxes(5))
            .expect("create namespace org");
        h.create_namespace(
            "org/team",
            TenantQuota::new().with_max_memory(mb(80)).with_max_sandboxes(5),
        )
        .expect("create namespace org/team");

        h.record_usage("org/team", mb(70), 3).expect("record usage for org/team");

        // 70 + 20 > 80 at team level
        let req = ResourceRequest::new(mb(20), 1);
        let result = h.check_quota("org/team", &req).expect("check quota for org/team");
        assert!(result.is_denied());
    }

    #[test]
    fn test_allowed_after_releasing_usage() {
        let h = QuotaHierarchy::new();
        h.create_namespace("org", TenantQuota::new().with_max_memory(mb(100))).expect("create namespace org");

        h.record_usage("org", mb(90), 0).expect("record usage for org");

        let req = ResourceRequest::new(mb(20), 0);
        assert!(h.check_quota("org", &req).expect("check quota for org").is_denied());

        h.release_usage("org", mb(30), 0).expect("release usage for org");
        assert!(h.check_quota("org", &req).expect("check quota for org").is_allowed());
    }

    // ── parent_path helper ──────────────────────────────────────────────

    #[test]
    fn test_parent_path() {
        assert_eq!(parent_path("org/team/proj"), Some("org/team".to_string()));
        assert_eq!(parent_path("org/team"), Some("org".to_string()));
        assert_eq!(parent_path("org"), None);
    }
}
