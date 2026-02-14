//! Policy-as-code engine for declarative security policies.
//!
//! Evaluates security policies at sandbox creation time, enabling
//! administrators to enforce organizational security rules declaratively.

use super::types::Capability;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Action to take when a policy matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyAction {
    /// Allow the request.
    Allow,
    /// Deny the request.
    Deny,
    /// Log the match but do not enforce.
    Audit,
}

/// A single rule within a policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyRule {
    /// Sandbox must have this capability.
    RequireCapability(Capability),
    /// Sandbox must NOT have this capability.
    DenyCapability(Capability),
    /// Memory limit ceiling in bytes.
    MaxMemory(usize),
    /// Fuel limit ceiling.
    MaxFuel(u64),
    /// Timeout ceiling.
    MaxTimeout(Duration),
    /// Only these module hashes are allowed.
    ModuleAllowlist(Vec<String>),
    /// These module hashes are denied.
    ModuleDenylist(Vec<String>),
    /// Sandbox must have label key=value.
    RequireLabel(String, String),
    /// Custom rule for future extension.
    Custom {
        /// Rule name.
        name: String,
        /// Check expression (reserved for future use).
        check: String,
    },
}

/// A declarative security policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    /// Unique identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Description of the policy's purpose.
    pub description: String,
    /// Rules that must all match (AND logic).
    pub rules: Vec<PolicyRule>,
    /// Priority (higher = evaluated first).
    pub priority: u32,
    /// Action to take when all rules match.
    pub action: PolicyAction,
    /// Whether this policy is active.
    pub enabled: bool,
}

/// Builder for constructing [`Policy`] instances.
#[derive(Debug, Default)]
pub struct PolicyBuilder {
    id: Option<String>,
    name: Option<String>,
    description: String,
    rules: Vec<PolicyRule>,
    priority: u32,
    action: Option<PolicyAction>,
    enabled: bool,
}

impl PolicyBuilder {
    /// Set the policy ID.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the policy name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the policy description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Add a rule to the policy.
    pub fn rule(mut self, rule: PolicyRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Set the priority (higher = evaluated first).
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Set the action to take when rules match.
    pub fn action(mut self, action: PolicyAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Set whether the policy is enabled (default: true).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Build the policy. Returns an error if `id`, `name`, or `action` are missing.
    pub fn build(self) -> crate::Result<Policy> {
        let id = self
            .id
            .ok_or_else(|| crate::Error::Policy("policy id is required".into()))?;
        let name = self
            .name
            .ok_or_else(|| crate::Error::Policy("policy name is required".into()))?;
        let action = self
            .action
            .ok_or_else(|| crate::Error::Policy("policy action is required".into()))?;

        Ok(Policy {
            id,
            name,
            description: self.description,
            rules: self.rules,
            priority: self.priority,
            action,
            enabled: self.enabled,
        })
    }
}

impl Policy {
    /// Create a new [`PolicyBuilder`].
    pub fn builder() -> PolicyBuilder {
        PolicyBuilder {
            enabled: true,
            ..Default::default()
        }
    }
}

/// A sandbox creation request to evaluate against policies.
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    /// Hash of the WASM module.
    pub module_hash: String,
    /// Requested capabilities.
    pub capabilities: Vec<Capability>,
    /// Requested memory limit in bytes.
    pub memory_limit: usize,
    /// Requested fuel limit.
    pub fuel: Option<u64>,
    /// Requested timeout.
    pub timeout: Option<Duration>,
    /// Labels attached to the sandbox.
    pub labels: HashMap<String, String>,
}

/// Information about a single policy that matched during evaluation.
#[derive(Debug, Clone)]
pub struct MatchedPolicy {
    /// ID of the matched policy.
    pub policy_id: String,
    /// Name of the matched policy.
    pub policy_name: String,
    /// Action of the matched policy.
    pub action: PolicyAction,
    /// Index of the first rule that caused the match.
    pub rule_index: usize,
}

/// Result of evaluating a request against all policies.
#[derive(Debug, Clone)]
pub struct PolicyDecision {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Policies that matched the request.
    pub matched_policies: Vec<MatchedPolicy>,
    /// Reason for denial, if any.
    pub denied_reason: Option<String>,
    /// Audit log entries.
    pub audit_entries: Vec<String>,
}

/// Engine that evaluates security policies at sandbox creation time.
#[derive(Debug, Default)]
pub struct PolicyEngine {
    policies: Vec<Policy>,
}

impl PolicyEngine {
    /// Create a new, empty policy engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a policy to the engine.
    pub fn add_policy(&mut self, policy: Policy) -> crate::Result<()> {
        if self.policies.iter().any(|p| p.id == policy.id) {
            return Err(crate::Error::Policy(format!(
                "duplicate policy id: {}",
                policy.id
            )));
        }
        self.policies.push(policy);
        Ok(())
    }

    /// Remove a policy by ID. Returns `true` if a policy was removed.
    pub fn remove_policy(&mut self, id: &str) -> bool {
        let before = self.policies.len();
        self.policies.retain(|p| p.id != id);
        self.policies.len() < before
    }

    /// List all registered policies.
    pub fn list_policies(&self) -> Vec<&Policy> {
        self.policies.iter().collect()
    }

    /// Evaluate a request against all enabled policies.
    ///
    /// Policies are evaluated in priority order (highest first). A `Deny`
    /// match short-circuits and the request is denied. If no `Deny` matches,
    /// the request is allowed. `Audit` policies are always recorded but never
    /// block the request. `Allow` matches are recorded as well.
    pub fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        let mut sorted: Vec<&Policy> = self.policies.iter().filter(|p| p.enabled).collect();
        sorted.sort_by(|a, b| b.priority.cmp(&a.priority));

        let mut decision = PolicyDecision {
            allowed: true,
            matched_policies: Vec::new(),
            denied_reason: None,
            audit_entries: Vec::new(),
        };

        for policy in &sorted {
            if let Some(rule_index) = self.all_rules_match(&policy.rules, request) {
                let matched = MatchedPolicy {
                    policy_id: policy.id.clone(),
                    policy_name: policy.name.clone(),
                    action: policy.action.clone(),
                    rule_index,
                };

                match &policy.action {
                    PolicyAction::Deny => {
                        decision.allowed = false;
                        decision.denied_reason = Some(format!(
                            "denied by policy '{}' ({})",
                            policy.name, policy.id
                        ));
                        decision.matched_policies.push(matched);
                        return decision;
                    }
                    PolicyAction::Audit => {
                        decision.audit_entries.push(format!(
                            "audit: policy '{}' ({}) matched",
                            policy.name, policy.id
                        ));
                        decision.matched_policies.push(matched);
                    }
                    PolicyAction::Allow => {
                        decision.matched_policies.push(matched);
                    }
                }
            }
        }

        decision
    }

    /// Check if all rules in a policy match the request.
    /// Returns `Some(rule_index)` of the *last* matched rule on success, or `None`.
    fn all_rules_match(&self, rules: &[PolicyRule], request: &PolicyRequest) -> Option<usize> {
        if rules.is_empty() {
            return Some(0);
        }
        let mut last_index = 0;
        for (i, rule) in rules.iter().enumerate() {
            if !self.rule_matches(rule, request) {
                return None;
            }
            last_index = i;
        }
        Some(last_index)
    }

    /// Evaluate a single rule against a request.
    fn rule_matches(&self, rule: &PolicyRule, request: &PolicyRequest) -> bool {
        match rule {
            PolicyRule::RequireCapability(cap) => request.capabilities.contains(cap),
            PolicyRule::DenyCapability(cap) => request.capabilities.contains(cap),
            PolicyRule::MaxMemory(limit) => request.memory_limit > *limit,
            PolicyRule::MaxFuel(limit) => request.fuel.map_or(false, |f| f > *limit),
            PolicyRule::MaxTimeout(limit) => request.timeout.map_or(false, |t| t > *limit),
            PolicyRule::ModuleAllowlist(hashes) => !hashes.contains(&request.module_hash),
            PolicyRule::ModuleDenylist(hashes) => hashes.contains(&request.module_hash),
            PolicyRule::RequireLabel(key, value) => {
                request.labels.get(key).map_or(true, |v| v != value)
            }
            PolicyRule::Custom { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_request() -> PolicyRequest {
        PolicyRequest {
            module_hash: "abc123".into(),
            capabilities: vec![Capability::stdout(), Capability::stderr()],
            memory_limit: 64 * 1024 * 1024, // 64 MB
            fuel: Some(1_000_000),
            timeout: Some(Duration::from_secs(30)),
            labels: HashMap::from([("env".into(), "production".into())]),
        }
    }

    #[test]
    fn test_allow_policy_based_on_capability() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("p1")
                    .name("allow stdout")
                    .rule(PolicyRule::RequireCapability(Capability::stdout()))
                    .action(PolicyAction::Allow)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        assert!(decision.allowed);
        assert_eq!(decision.matched_policies.len(), 1);
        assert_eq!(decision.matched_policies[0].policy_id, "p1");
    }

    #[test]
    fn test_deny_policy_based_on_capability() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("deny-stdout")
                    .name("deny stdout")
                    .rule(PolicyRule::DenyCapability(Capability::stdout()))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);
        assert!(decision.denied_reason.is_some());
        assert!(decision
            .denied_reason
            .unwrap()
            .contains("deny stdout"));
    }

    #[test]
    fn test_memory_ceiling_enforcement() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("mem-limit")
                    .name("memory ceiling")
                    .rule(PolicyRule::MaxMemory(32 * 1024 * 1024)) // 32 MB ceiling
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // Request has 64 MB which exceeds the 32 MB ceiling
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        // Request within ceiling
        let mut req = sample_request();
        req.memory_limit = 16 * 1024 * 1024;
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_module_allowlist() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("allowlist")
                    .name("module allowlist")
                    .rule(PolicyRule::ModuleAllowlist(vec![
                        "trusted1".into(),
                        "trusted2".into(),
                    ]))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // "abc123" is not in the allowlist -> rule matches -> deny
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        // Trusted module -> rule does not match -> allow
        let mut req = sample_request();
        req.module_hash = "trusted1".into();
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_module_denylist() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("denylist")
                    .name("module denylist")
                    .rule(PolicyRule::ModuleDenylist(vec!["malicious".into()]))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        assert!(decision.allowed);

        let mut req = sample_request();
        req.module_hash = "malicious".into();
        let decision = engine.evaluate(&req);
        assert!(!decision.allowed);
    }

    #[test]
    fn test_priority_ordering() {
        let mut engine = PolicyEngine::new();

        // Lower priority allow
        engine
            .add_policy(
                Policy::builder()
                    .id("allow-low")
                    .name("low priority allow")
                    .rule(PolicyRule::RequireCapability(Capability::stdout()))
                    .priority(1)
                    .action(PolicyAction::Allow)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // Higher priority deny
        engine
            .add_policy(
                Policy::builder()
                    .id("deny-high")
                    .name("high priority deny")
                    .rule(PolicyRule::DenyCapability(Capability::stdout()))
                    .priority(10)
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        // Deny is evaluated first (higher priority) and short-circuits
        assert!(!decision.allowed);
        assert_eq!(decision.matched_policies[0].policy_id, "deny-high");
    }

    #[test]
    fn test_audit_only_policy() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("audit-net")
                    .name("audit network")
                    .rule(PolicyRule::RequireCapability(Capability::stdout()))
                    .action(PolicyAction::Audit)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        assert!(decision.allowed);
        assert_eq!(decision.audit_entries.len(), 1);
        assert!(decision.audit_entries[0].contains("audit network"));
    }

    #[test]
    fn test_multiple_rules_and_logic() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("multi")
                    .name("multi rule")
                    .rule(PolicyRule::RequireCapability(Capability::stdout()))
                    .rule(PolicyRule::MaxMemory(32 * 1024 * 1024))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // Both rules match (has stdout AND exceeds 32 MB) -> deny
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        // Only one rule matches (has stdout but within memory) -> allow
        let mut req = sample_request();
        req.memory_limit = 16 * 1024 * 1024;
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_disabled_policies_skipped() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("disabled")
                    .name("disabled deny")
                    .rule(PolicyRule::DenyCapability(Capability::stdout()))
                    .action(PolicyAction::Deny)
                    .enabled(false)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let decision = engine.evaluate(&sample_request());
        assert!(decision.allowed);
        assert!(decision.matched_policies.is_empty());
    }

    #[test]
    fn test_add_duplicate_policy_errors() {
        let mut engine = PolicyEngine::new();
        let policy = Policy::builder()
            .id("p1")
            .name("test")
            .action(PolicyAction::Allow)
            .build()
            .unwrap();
        engine.add_policy(policy.clone()).unwrap();
        assert!(engine.add_policy(policy).is_err());
    }

    #[test]
    fn test_remove_policy() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("p1")
                    .name("test")
                    .action(PolicyAction::Allow)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        assert!(engine.remove_policy("p1"));
        assert!(!engine.remove_policy("p1"));
        assert!(engine.list_policies().is_empty());
    }

    #[test]
    fn test_list_policies() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("a")
                    .name("alpha")
                    .action(PolicyAction::Allow)
                    .build()
                    .unwrap(),
            )
            .unwrap();
        engine
            .add_policy(
                Policy::builder()
                    .id("b")
                    .name("beta")
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        let policies = engine.list_policies();
        assert_eq!(policies.len(), 2);
    }

    #[test]
    fn test_builder_missing_fields() {
        assert!(Policy::builder().name("n").action(PolicyAction::Allow).build().is_err());
        assert!(Policy::builder().id("i").action(PolicyAction::Allow).build().is_err());
        assert!(Policy::builder().id("i").name("n").build().is_err());
    }

    #[test]
    fn test_fuel_ceiling() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("fuel")
                    .name("fuel ceiling")
                    .rule(PolicyRule::MaxFuel(500_000))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // 1_000_000 > 500_000 -> deny
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        let mut req = sample_request();
        req.fuel = Some(100_000);
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_timeout_ceiling() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("timeout")
                    .name("timeout ceiling")
                    .rule(PolicyRule::MaxTimeout(Duration::from_secs(10)))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // 30s > 10s -> deny
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        let mut req = sample_request();
        req.timeout = Some(Duration::from_secs(5));
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_require_label() {
        let mut engine = PolicyEngine::new();
        engine
            .add_policy(
                Policy::builder()
                    .id("label")
                    .name("require label")
                    .rule(PolicyRule::RequireLabel("env".into(), "staging".into()))
                    .action(PolicyAction::Deny)
                    .build()
                    .unwrap(),
            )
            .unwrap();

        // Request has env=production, not env=staging -> rule matches -> deny
        let decision = engine.evaluate(&sample_request());
        assert!(!decision.allowed);

        // Request with correct label -> rule does not match -> allow
        let mut req = sample_request();
        req.labels.insert("env".into(), "staging".into());
        let decision = engine.evaluate(&req);
        assert!(decision.allowed);
    }

    #[test]
    fn test_empty_engine_allows_all() {
        let engine = PolicyEngine::new();
        let decision = engine.evaluate(&sample_request());
        assert!(decision.allowed);
        assert!(decision.matched_policies.is_empty());
    }
}
