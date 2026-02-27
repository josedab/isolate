//! Enterprise policy governance: versioning, tenancy, inheritance, and runtime hooks.
//!
//! Extends the core policy engine with enterprise-grade governance capabilities:
//! - Policy versioning with rollback support
//! - Multi-tenant policy isolation
//! - Policy inheritance and composition hierarchies
//! - Runtime enforcement hooks for sandbox integration
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::policy::governance::*;
//!
//! // Create a versioned policy store
//! let mut store = PolicyVersionStore::new();
//! let v1 = store.commit("initial policy set", rules.clone());
//!
//! // Set up tenant-scoped policies
//! let mut tenant_mgr = TenantPolicyManager::new();
//! tenant_mgr.set_tenant_policy("acme-corp", rules);
//!
//! // Create inheritance chain: global → org → team
//! let mut hierarchy = PolicyHierarchy::new(global_rules);
//! hierarchy.add_layer("org:acme", org_rules);
//! hierarchy.add_layer("team:engineering", team_rules);
//! let effective = hierarchy.effective_rules();
//! ```

use super::engine::{ConflictResolution, DefaultPolicy, EvalTrace, PolicyDecision, PolicyEngine};
use super::rules::{Effect, PolicyRule, Value};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Policy Versioning
// ---------------------------------------------------------------------------

/// Unique identifier for a policy version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyVersionId(pub String);

impl PolicyVersionId {
    /// Generate a new version ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for PolicyVersionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for PolicyVersionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A versioned snapshot of policy rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersion {
    /// Version identifier.
    pub id: PolicyVersionId,
    /// Version number (monotonically increasing).
    pub version_number: u64,
    /// Commit message describing the change.
    pub message: String,
    /// The rules at this version.
    pub rules: Vec<PolicyRule>,
    /// When this version was created.
    pub created_at: DateTime<Utc>,
    /// Who created this version (principal ID).
    pub created_by: Option<String>,
    /// Parent version (if any).
    pub parent_version: Option<PolicyVersionId>,
}

/// Store for versioned policy history with rollback support.
pub struct PolicyVersionStore {
    /// All versions, ordered by version number.
    versions: Vec<PolicyVersion>,
    /// Current active version index.
    active_index: Option<usize>,
    /// Next version number.
    next_version: u64,
}

impl PolicyVersionStore {
    /// Create a new empty version store.
    pub fn new() -> Self {
        Self { versions: Vec::new(), active_index: None, next_version: 1 }
    }

    /// Commit a new version of policies.
    pub fn commit(
        &mut self,
        message: impl Into<String>,
        rules: Vec<PolicyRule>,
    ) -> PolicyVersionId {
        self.commit_by(message, rules, None)
    }

    /// Commit a new version with author attribution.
    pub fn commit_by(
        &mut self,
        message: impl Into<String>,
        rules: Vec<PolicyRule>,
        author: Option<String>,
    ) -> PolicyVersionId {
        let id = PolicyVersionId::new();
        let parent = self.active_version().map(|v| v.id.clone());
        let version_number = self.next_version;
        self.next_version += 1;

        let version = PolicyVersion {
            id: id.clone(),
            version_number,
            message: message.into(),
            rules,
            created_at: Utc::now(),
            created_by: author,
            parent_version: parent,
        };

        self.versions.push(version);
        self.active_index = Some(self.versions.len() - 1);
        id
    }

    /// Get the currently active version.
    pub fn active_version(&self) -> Option<&PolicyVersion> {
        self.active_index.and_then(|i| self.versions.get(i))
    }

    /// Get the active rules.
    pub fn active_rules(&self) -> Vec<PolicyRule> {
        self.active_version().map(|v| v.rules.clone()).unwrap_or_default()
    }

    /// Rollback to a specific version.
    pub fn rollback(&mut self, version_id: &PolicyVersionId) -> Result<(), String> {
        let index = self
            .versions
            .iter()
            .position(|v| v.id == *version_id)
            .ok_or_else(|| format!("Version {} not found", version_id))?;
        self.active_index = Some(index);
        Ok(())
    }

    /// Rollback to the previous version.
    pub fn rollback_one(&mut self) -> Result<(), String> {
        match self.active_index {
            Some(0) => Err("Already at the earliest version".to_string()),
            Some(i) => {
                self.active_index = Some(i - 1);
                Ok(())
            }
            None => Err("No versions to rollback".to_string()),
        }
    }

    /// Rollback to the previous version and return its rules for verification.
    pub fn rollback_and_verify(&mut self) -> Result<Vec<PolicyRule>, String> {
        self.rollback_one()?;
        Ok(self.active_rules())
    }

    /// Get version history.
    pub fn history(&self) -> &[PolicyVersion] {
        &self.versions
    }

    /// Get the number of versions.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    /// Diff two versions (returns rules added and removed).
    pub fn diff(&self, from: &PolicyVersionId, to: &PolicyVersionId) -> Result<PolicyDiff, String> {
        let from_ver = self
            .versions
            .iter()
            .find(|v| v.id == *from)
            .ok_or_else(|| format!("Version {} not found", from))?;
        let to_ver = self
            .versions
            .iter()
            .find(|v| v.id == *to)
            .ok_or_else(|| format!("Version {} not found", to))?;

        let from_ids: std::collections::HashSet<&str> =
            from_ver.rules.iter().map(|r| r.id.as_str()).collect();
        let to_ids: std::collections::HashSet<&str> =
            to_ver.rules.iter().map(|r| r.id.as_str()).collect();

        let added: Vec<String> = to_ids.difference(&from_ids).map(|s| s.to_string()).collect();
        let removed: Vec<String> = from_ids.difference(&to_ids).map(|s| s.to_string()).collect();

        let from_effects: HashMap<&str, Effect> =
            from_ver.rules.iter().map(|r| (r.id.as_str(), r.effect)).collect();
        let to_effects: HashMap<&str, Effect> =
            to_ver.rules.iter().map(|r| (r.id.as_str(), r.effect)).collect();

        let mut unchanged = Vec::new();
        let mut modified = Vec::new();
        for id in from_ids.intersection(&to_ids) {
            if from_effects.get(id) == to_effects.get(id) {
                unchanged.push(id.to_string());
            } else {
                modified.push(id.to_string());
            }
        }

        Ok(PolicyDiff { from: from.clone(), to: to.clone(), added, removed, unchanged, modified })
    }
}

impl Default for PolicyVersionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Diff between two policy versions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDiff {
    /// Source version.
    pub from: PolicyVersionId,
    /// Target version.
    pub to: PolicyVersionId,
    /// Rule IDs added in target.
    pub added: Vec<String>,
    /// Rule IDs removed from source.
    pub removed: Vec<String>,
    /// Rule IDs present in both with the same effect.
    pub unchanged: Vec<String>,
    /// Rule IDs present in both but with different effects.
    pub modified: Vec<String>,
}

// ---------------------------------------------------------------------------
// Multi-Tenant Policy Isolation
// ---------------------------------------------------------------------------

/// Tenant identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S: Into<String>> From<S> for TenantId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// Per-tenant policy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantPolicy {
    /// Tenant identifier.
    pub tenant_id: TenantId,
    /// Rules specific to this tenant.
    pub rules: Vec<PolicyRule>,
    /// Conflict resolution override (None = use global).
    pub conflict_resolution: Option<ConflictResolution>,
    /// Whether this tenant can override global policies.
    pub can_override_global: bool,
    /// Maximum rules allowed for this tenant.
    pub max_rules: usize,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
    /// Last updated timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Manager for tenant-scoped policies.
pub struct TenantPolicyManager {
    /// Global rules applied to all tenants.
    global_rules: Vec<PolicyRule>,
    /// Per-tenant policies.
    tenants: HashMap<TenantId, TenantPolicy>,
    /// Default conflict resolution.
    default_conflict_resolution: ConflictResolution,
    /// Default maximum rules per tenant.
    default_max_rules: usize,
}

impl TenantPolicyManager {
    /// Create a new tenant policy manager.
    pub fn new() -> Self {
        Self {
            global_rules: Vec::new(),
            tenants: HashMap::new(),
            default_conflict_resolution: ConflictResolution::DenyOverrides,
            default_max_rules: 100,
        }
    }

    /// Set global rules applied to all tenants.
    pub fn set_global_rules(&mut self, rules: Vec<PolicyRule>) {
        self.global_rules = rules;
    }

    /// Set policies for a specific tenant.
    pub fn set_tenant_policy(
        &mut self,
        tenant_id: impl Into<TenantId>,
        rules: Vec<PolicyRule>,
    ) -> Result<(), String> {
        let tenant_id = tenant_id.into();
        let max_rules =
            self.tenants.get(&tenant_id).map(|t| t.max_rules).unwrap_or(self.default_max_rules);

        if rules.len() > max_rules {
            return Err(format!(
                "Tenant {} exceeds maximum rules: {} > {}",
                tenant_id,
                rules.len(),
                max_rules
            ));
        }

        let now = Utc::now();
        let policy = TenantPolicy {
            tenant_id: tenant_id.clone(),
            rules,
            conflict_resolution: None,
            can_override_global: false,
            max_rules,
            created_at: now,
            updated_at: now,
        };

        self.tenants.insert(tenant_id, policy);
        Ok(())
    }

    /// Build a PolicyEngine with effective rules for a tenant.
    pub fn engine_for_tenant(&self, tenant_id: &TenantId) -> PolicyEngine {
        let conflict_res = self
            .tenants
            .get(tenant_id)
            .and_then(|t| t.conflict_resolution)
            .unwrap_or(self.default_conflict_resolution);

        let mut engine = PolicyEngine::with_config(DefaultPolicy::Deny, conflict_res);

        // Add global rules first
        for rule in &self.global_rules {
            engine.add_rule(rule.clone());
        }

        // Add tenant-specific rules
        if let Some(tenant) = self.tenants.get(tenant_id) {
            for rule in &tenant.rules {
                engine.add_rule(rule.clone());
            }
        }

        engine
    }

    /// Evaluate a policy for a specific tenant.
    pub fn evaluate_for_tenant(
        &self,
        tenant_id: &TenantId,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> PolicyDecision {
        let engine = self.engine_for_tenant(tenant_id);
        engine.evaluate(action, resource, principal, context)
    }

    /// List all tenants.
    pub fn tenants(&self) -> Vec<&TenantId> {
        self.tenants.keys().collect()
    }

    /// Get tenant policy.
    pub fn get_tenant(&self, tenant_id: &TenantId) -> Option<&TenantPolicy> {
        self.tenants.get(tenant_id)
    }

    /// Remove a tenant's policies.
    pub fn remove_tenant(&mut self, tenant_id: &TenantId) -> bool {
        self.tenants.remove(tenant_id).is_some()
    }
}

impl Default for TenantPolicyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Policy Inheritance Hierarchy
// ---------------------------------------------------------------------------

/// A layer in the policy hierarchy (e.g., global → org → team → project).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyLayer {
    /// Layer name (e.g., "org:acme", "team:engineering").
    pub name: String,
    /// Rules at this layer.
    pub rules: Vec<PolicyRule>,
    /// Whether this layer can override parent rules.
    pub can_override: bool,
    /// Priority boost for rules at this layer (higher = more specific).
    pub priority_boost: i32,
}

/// Hierarchical policy composition with inheritance.
pub struct PolicyHierarchy {
    /// Ordered layers from most general to most specific.
    layers: Vec<PolicyLayer>,
}

impl PolicyHierarchy {
    /// Create a new hierarchy with a base (root) layer.
    pub fn new(base_rules: Vec<PolicyRule>) -> Self {
        Self {
            layers: vec![PolicyLayer {
                name: "global".to_string(),
                rules: base_rules,
                can_override: false,
                priority_boost: 0,
            }],
        }
    }

    /// Add a more specific layer to the hierarchy.
    pub fn add_layer(&mut self, name: impl Into<String>, rules: Vec<PolicyRule>) {
        let priority_boost = (self.layers.len() * 100) as i32;
        self.layers.push(PolicyLayer {
            name: name.into(),
            rules,
            can_override: true,
            priority_boost,
        });
    }

    /// Add a layer with custom settings.
    pub fn add_layer_with_config(&mut self, layer: PolicyLayer) {
        self.layers.push(layer);
    }

    /// Compute the effective rules by flattening the hierarchy.
    /// More specific layers get higher priority.
    pub fn effective_rules(&self) -> Vec<PolicyRule> {
        let mut rules = Vec::new();

        for layer in &self.layers {
            for rule in &layer.rules {
                let mut boosted_rule = rule.clone();
                boosted_rule.priority += layer.priority_boost;
                // Prefix rule ID with layer name for traceability
                boosted_rule.id = format!("{}::{}", layer.name, rule.id);
                rules.push(boosted_rule);
            }
        }

        rules
    }

    /// Build a PolicyEngine from the effective rules.
    pub fn build_engine(&self) -> PolicyEngine {
        let mut engine = PolicyEngine::new();
        for rule in self.effective_rules() {
            engine.add_rule(rule);
        }
        engine
    }

    /// Get the layer names.
    pub fn layer_names(&self) -> Vec<&str> {
        self.layers.iter().map(|l| l.name.as_str()).collect()
    }

    /// Get number of layers.
    pub fn depth(&self) -> usize {
        self.layers.len()
    }
}

// ---------------------------------------------------------------------------
// Runtime Enforcement Hook
// ---------------------------------------------------------------------------

/// Result of a runtime policy check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementResult {
    /// Whether the action is allowed.
    pub allowed: bool,
    /// The policy decision.
    pub decision: PolicyDecision,
    /// Tenant that was evaluated (if any).
    pub tenant_id: Option<TenantId>,
    /// Timestamp of the enforcement check.
    pub checked_at: DateTime<Utc>,
    /// Duration of the evaluation.
    pub eval_duration_us: u64,
}

/// Runtime policy enforcement hook for sandbox integration.
///
/// Provides a single entry point for sandboxes to check policy decisions
/// at runtime, integrating versioning, tenancy, and hierarchy.
pub struct PolicyEnforcer {
    /// Version store for policy history.
    versions: PolicyVersionStore,
    /// Tenant manager for multi-tenant isolation.
    tenants: TenantPolicyManager,
    /// Global policy hierarchy.
    hierarchy: Option<PolicyHierarchy>,
    /// Whether enforcement is enabled (can be toggled for maintenance).
    enabled: bool,
    /// Total enforcement checks.
    total_checks: std::sync::atomic::AtomicU64,
    /// Total denials.
    total_denials: std::sync::atomic::AtomicU64,
}

impl PolicyEnforcer {
    /// Create a new policy enforcer.
    pub fn new() -> Self {
        Self {
            versions: PolicyVersionStore::new(),
            tenants: TenantPolicyManager::new(),
            hierarchy: None,
            enabled: true,
            total_checks: std::sync::atomic::AtomicU64::new(0),
            total_denials: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Check if an action is allowed.
    pub fn check(
        &self,
        action: &str,
        resource: &str,
        principal: &str,
        tenant_id: Option<&TenantId>,
        context: &HashMap<String, Value>,
    ) -> EnforcementResult {
        let start = std::time::Instant::now();
        self.total_checks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if !self.enabled {
            return EnforcementResult {
                allowed: true,
                decision: PolicyDecision {
                    effect: Effect::Allow,
                    determining_rule: Some("enforcement-disabled".to_string()),
                    trace: EvalTrace::default(),
                },
                tenant_id: tenant_id.cloned(),
                checked_at: Utc::now(),
                eval_duration_us: start.elapsed().as_micros() as u64,
            };
        }

        // Use tenant-specific evaluation if tenant provided
        let decision = if let Some(tid) = tenant_id {
            self.tenants.evaluate_for_tenant(tid, action, resource, principal, context)
        } else if let Some(ref hierarchy) = self.hierarchy {
            let engine = hierarchy.build_engine();
            engine.evaluate(action, resource, principal, context)
        } else {
            // Fall back to active versioned rules
            let rules = self.versions.active_rules();
            let mut engine = PolicyEngine::new();
            for rule in rules {
                engine.add_rule(rule);
            }
            engine.evaluate(action, resource, principal, context)
        };

        if decision.is_denied() {
            self.total_denials.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        EnforcementResult {
            allowed: decision.is_allowed(),
            decision,
            tenant_id: tenant_id.cloned(),
            checked_at: Utc::now(),
            eval_duration_us: start.elapsed().as_micros() as u64,
        }
    }

    /// Enable or disable enforcement.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get the version store for policy management.
    pub fn versions(&self) -> &PolicyVersionStore {
        &self.versions
    }

    /// Get mutable version store.
    pub fn versions_mut(&mut self) -> &mut PolicyVersionStore {
        &mut self.versions
    }

    /// Get the tenant manager.
    pub fn tenants(&self) -> &TenantPolicyManager {
        &self.tenants
    }

    /// Get mutable tenant manager.
    pub fn tenants_mut(&mut self) -> &mut TenantPolicyManager {
        &mut self.tenants
    }

    /// Set the policy hierarchy.
    pub fn set_hierarchy(&mut self, hierarchy: PolicyHierarchy) {
        self.hierarchy = Some(hierarchy);
    }

    /// Get enforcement statistics.
    pub fn stats(&self) -> EnforcementStats {
        let total = self.total_checks.load(std::sync::atomic::Ordering::Relaxed);
        let denied = self.total_denials.load(std::sync::atomic::Ordering::Relaxed);
        EnforcementStats {
            total_checks: total,
            total_denials: denied,
            denial_rate: if total > 0 { denied as f64 / total as f64 } else { 0.0 },
            enabled: self.enabled,
        }
    }
}

impl Default for PolicyEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics from the policy enforcer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementStats {
    /// Total policy checks performed.
    pub total_checks: u64,
    /// Total denied actions.
    pub total_denials: u64,
    /// Denial rate (0.0 - 1.0).
    pub denial_rate: f64,
    /// Whether enforcement is currently enabled.
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn allow_rule(id: &str, action: &str) -> PolicyRule {
        PolicyRule::builder(id).effect(Effect::Allow).action(action).build()
    }

    fn deny_rule(id: &str, action: &str) -> PolicyRule {
        PolicyRule::builder(id).effect(Effect::Deny).action(action).build()
    }

    // -- Version Store Tests --

    #[test]
    fn test_version_store_commit() {
        let mut store = PolicyVersionStore::new();
        let v1 = store.commit("initial", vec![allow_rule("r1", "read")]);
        let _v2 =
            store.commit("add write", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);

        assert_eq!(store.version_count(), 2);
        assert_eq!(store.active_rules().len(), 2);

        let active = store.active_version().unwrap();
        assert_eq!(active.version_number, 2);
        assert_eq!(active.parent_version, Some(v1.clone()));
    }

    #[test]
    fn test_version_store_rollback() {
        let mut store = PolicyVersionStore::new();
        let v1 = store.commit("v1", vec![allow_rule("r1", "read")]);
        let _v2 = store.commit("v2", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);

        assert_eq!(store.active_rules().len(), 2);

        store.rollback(&v1).unwrap();
        assert_eq!(store.active_rules().len(), 1);
    }

    #[test]
    fn test_version_store_rollback_one() {
        let mut store = PolicyVersionStore::new();
        store.commit("v1", vec![allow_rule("r1", "read")]);
        store.commit("v2", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);

        store.rollback_one().unwrap();
        assert_eq!(store.active_rules().len(), 1);
    }

    #[test]
    fn test_version_store_diff() {
        let mut store = PolicyVersionStore::new();
        let v1 = store.commit("v1", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);
        let v2 = store.commit("v2", vec![allow_rule("r1", "read"), allow_rule("r3", "exec")]);

        let diff = store.diff(&v1, &v2).unwrap();
        assert_eq!(diff.added, vec!["r3"]);
        assert_eq!(diff.removed, vec!["r2"]);
        assert_eq!(diff.unchanged, vec!["r1"]);
    }

    // -- Tenant Manager Tests --

    #[test]
    fn test_tenant_policy_isolation() {
        let mut mgr = TenantPolicyManager::new();
        mgr.set_global_rules(vec![allow_rule("global-stdout", "stdio:stdout")]);

        mgr.set_tenant_policy("acme", vec![allow_rule("acme-fs", "fs:read")]).unwrap();
        mgr.set_tenant_policy("beta", vec![deny_rule("beta-net", "net:*")]).unwrap();

        let _ctx: HashMap<String, Value> = HashMap::new();
        let acme_id = TenantId::from("acme");
        let beta_id = TenantId::from("beta");

        // Acme has global + own rules
        let acme_engine = mgr.engine_for_tenant(&acme_id);
        assert_eq!(acme_engine.rule_count(), 2);

        // Beta has global + own rules
        let beta_engine = mgr.engine_for_tenant(&beta_id);
        assert_eq!(beta_engine.rule_count(), 2);
    }

    #[test]
    fn test_tenant_max_rules_enforced() {
        let mut mgr = TenantPolicyManager::new();
        mgr.default_max_rules = 2;

        let result = mgr.set_tenant_policy(
            "acme",
            vec![allow_rule("r1", "a"), allow_rule("r2", "b"), allow_rule("r3", "c")],
        );
        assert!(result.is_err());
    }

    // -- Hierarchy Tests --

    #[test]
    fn test_policy_hierarchy() {
        let hierarchy_rules = vec![allow_rule("global-base", "stdio:*")];
        let mut hierarchy = PolicyHierarchy::new(hierarchy_rules);
        hierarchy.add_layer("org:acme", vec![allow_rule("org-fs", "fs:read")]);
        hierarchy.add_layer("team:eng", vec![deny_rule("team-net", "net:*")]);

        let effective = hierarchy.effective_rules();
        assert_eq!(effective.len(), 3);
        assert_eq!(hierarchy.depth(), 3);

        // Check priority boosting
        let base_rule = effective.iter().find(|r| r.id.contains("global-base")).unwrap();
        let team_rule = effective.iter().find(|r| r.id.contains("team-net")).unwrap();
        assert!(team_rule.priority > base_rule.priority);
    }

    #[test]
    fn test_policy_hierarchy_engine() {
        let mut hierarchy = PolicyHierarchy::new(vec![allow_rule("base", "read")]);
        hierarchy.add_layer("org", vec![deny_rule("org-deny", "write")]);

        let engine = hierarchy.build_engine();
        assert_eq!(engine.rule_count(), 2);
    }

    // -- Enforcer Tests --

    #[test]
    fn test_enforcer_basic() {
        let mut enforcer = PolicyEnforcer::new();
        enforcer.versions_mut().commit("initial", vec![allow_rule("allow-read", "read")]);

        let ctx = HashMap::new();
        let result = enforcer.check("read", "*", "user1", None, &ctx);
        assert!(result.allowed);
    }

    #[test]
    fn test_enforcer_disabled() {
        let mut enforcer = PolicyEnforcer::new();
        enforcer.set_enabled(false);

        let ctx = HashMap::new();
        let result = enforcer.check("anything", "*", "user1", None, &ctx);
        assert!(result.allowed);
    }

    #[test]
    fn test_enforcer_with_tenant() {
        let mut enforcer = PolicyEnforcer::new();
        enforcer
            .tenants_mut()
            .set_tenant_policy("acme", vec![allow_rule("allow-read", "read")])
            .unwrap();

        let ctx = HashMap::new();
        let acme_id = TenantId::from("acme");
        let result = enforcer.check("read", "*", "user1", Some(&acme_id), &ctx);
        assert!(result.allowed);
        assert_eq!(result.tenant_id, Some(acme_id));
    }

    #[test]
    fn test_enforcer_stats() {
        let mut enforcer = PolicyEnforcer::new();
        enforcer.versions_mut().commit("initial", vec![allow_rule("allow-read", "read")]);

        let ctx = HashMap::new();
        enforcer.check("read", "*", "user1", None, &ctx);
        enforcer.check("write", "*", "user1", None, &ctx);

        let stats = enforcer.stats();
        assert_eq!(stats.total_checks, 2);
        assert!(stats.total_denials >= 1); // "write" should be denied (default deny)
    }

    #[test]
    fn test_diff_detects_modified_rules() {
        let mut store = PolicyVersionStore::new();
        let v1 = store.commit("v1", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);
        let v2 = store.commit("v2", vec![deny_rule("r1", "read"), allow_rule("r2", "write")]);

        let diff = store.diff(&v1, &v2).unwrap();
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.modified, vec!["r1"]);
        assert_eq!(diff.unchanged, vec!["r2"]);
    }

    #[test]
    fn test_rollback_and_verify() {
        let mut store = PolicyVersionStore::new();
        let v1_rules = vec![allow_rule("r1", "read")];
        store.commit("v1", v1_rules.clone());
        store.commit("v2", vec![allow_rule("r1", "read"), allow_rule("r2", "write")]);

        let rolled_back_rules = store.rollback_and_verify().unwrap();
        assert_eq!(rolled_back_rules.len(), v1_rules.len());
        assert_eq!(rolled_back_rules[0].id, v1_rules[0].id);
        assert_eq!(rolled_back_rules[0].effect, v1_rules[0].effect);
    }
}
