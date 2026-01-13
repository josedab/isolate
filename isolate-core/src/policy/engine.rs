//! Policy evaluation engine.

use super::rules::{Effect, PolicyRule, PolicySet, Value};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The result of a policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecision {
    /// The final effect (allow/deny).
    pub effect: Effect,
    /// The rule that determined the decision (None = default).
    pub determining_rule: Option<String>,
    /// Evaluation trace for audit purposes.
    pub trace: EvalTrace,
}

impl PolicyDecision {
    /// Check if the decision allows the action.
    pub fn is_allowed(&self) -> bool {
        self.effect == Effect::Allow
    }

    /// Check if the decision denies the action.
    pub fn is_denied(&self) -> bool {
        self.effect == Effect::Deny
    }
}

/// Evaluation trace for auditing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalTrace {
    /// Individual rule evaluations.
    pub entries: Vec<EvalTraceEntry>,
}

/// A single rule evaluation in the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTraceEntry {
    /// Rule ID.
    pub rule_id: String,
    /// Whether the rule matched.
    pub matched: bool,
    /// The effect of the rule (if matched).
    pub effect: Option<Effect>,
    /// Priority of the rule.
    pub priority: i32,
}

/// Default policy when no rules match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultPolicy {
    /// Allow by default (open).
    Allow,
    /// Deny by default (restrictive).
    Deny,
}

/// Conflict resolution strategy when allow and deny rules both match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Deny wins over allow (most restrictive).
    DenyOverrides,
    /// Allow wins over deny (most permissive).
    AllowOverrides,
    /// Highest priority rule wins.
    PriorityBased,
}

/// The policy evaluation engine.
pub struct PolicyEngine {
    /// All loaded policy sets.
    policy_sets: Vec<PolicySet>,
    /// Standalone rules not in a set.
    standalone_rules: Vec<PolicyRule>,
    /// Default policy when no rules match.
    default_policy: DefaultPolicy,
    /// Conflict resolution strategy.
    conflict_resolution: ConflictResolution,
    /// Whether to record evaluation traces.
    enable_tracing: bool,
}

impl PolicyEngine {
    /// Create a new policy engine with default-deny.
    pub fn new() -> Self {
        Self {
            policy_sets: Vec::new(),
            standalone_rules: Vec::new(),
            default_policy: DefaultPolicy::Deny,
            conflict_resolution: ConflictResolution::DenyOverrides,
            enable_tracing: true,
        }
    }

    /// Create a new policy engine with custom settings.
    pub fn with_config(
        default_policy: DefaultPolicy,
        conflict_resolution: ConflictResolution,
    ) -> Self {
        Self { default_policy, conflict_resolution, ..Self::new() }
    }

    /// Set the default policy.
    pub fn set_default_policy(&mut self, policy: DefaultPolicy) {
        self.default_policy = policy;
    }

    /// Set the conflict resolution strategy.
    pub fn set_conflict_resolution(&mut self, strategy: ConflictResolution) {
        self.conflict_resolution = strategy;
    }

    /// Enable or disable evaluation tracing.
    pub fn set_tracing(&mut self, enabled: bool) {
        self.enable_tracing = enabled;
    }

    /// Add a standalone rule.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.standalone_rules.push(rule);
    }

    /// Add a policy set.
    pub fn add_policy_set(&mut self, set: PolicySet) {
        self.policy_sets.push(set);
    }

    /// Remove a rule by ID.
    pub fn remove_rule(&mut self, id: &str) -> bool {
        let initial_len = self.standalone_rules.len();
        self.standalone_rules.retain(|r| r.id != id);
        self.standalone_rules.len() < initial_len
    }

    /// Get the total number of rules (standalone + from sets).
    pub fn rule_count(&self) -> usize {
        self.standalone_rules.len() + self.policy_sets.iter().map(|s| s.rules.len()).sum::<usize>()
    }

    /// Collect all rules sorted by priority (descending).
    fn all_rules_sorted(&self) -> Vec<&PolicyRule> {
        let mut rules: Vec<&PolicyRule> = self
            .standalone_rules
            .iter()
            .chain(self.policy_sets.iter().flat_map(|s| &s.rules))
            .collect();

        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        rules
    }

    /// Evaluate a policy decision for the given request.
    pub fn evaluate(
        &self,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> PolicyDecision {
        let rules = self.all_rules_sorted();
        let mut trace = EvalTrace::default();
        let mut matching_allows: Vec<(i32, &str)> = Vec::new();
        let mut matching_denies: Vec<(i32, &str)> = Vec::new();

        for rule in &rules {
            let matched = rule.matches(action, resource, principal, context);

            if self.enable_tracing {
                trace.entries.push(EvalTraceEntry {
                    rule_id: rule.id.clone(),
                    matched,
                    effect: if matched { Some(rule.effect) } else { None },
                    priority: rule.priority,
                });
            }

            if matched {
                match rule.effect {
                    Effect::Allow => matching_allows.push((rule.priority, &rule.id)),
                    Effect::Deny => matching_denies.push((rule.priority, &rule.id)),
                }
            }
        }

        // Resolve conflicts
        let (effect, determining_rule) = if matching_allows.is_empty() && matching_denies.is_empty()
        {
            // No rules matched, use default
            let eff = match self.default_policy {
                DefaultPolicy::Allow => Effect::Allow,
                DefaultPolicy::Deny => Effect::Deny,
            };
            (eff, None)
        } else {
            match self.conflict_resolution {
                ConflictResolution::DenyOverrides => {
                    if let Some((_, id)) = matching_denies.first() {
                        (Effect::Deny, Some(id.to_string()))
                    } else if let Some((_, id)) = matching_allows.first() {
                        (Effect::Allow, Some(id.to_string()))
                    } else {
                        unreachable!()
                    }
                }
                ConflictResolution::AllowOverrides => {
                    if let Some((_, id)) = matching_allows.first() {
                        (Effect::Allow, Some(id.to_string()))
                    } else if let Some((_, id)) = matching_denies.first() {
                        (Effect::Deny, Some(id.to_string()))
                    } else {
                        unreachable!()
                    }
                }
                ConflictResolution::PriorityBased => {
                    let best_allow = matching_allows.first().map(|(p, id)| (*p, *id));
                    let best_deny = matching_denies.first().map(|(p, id)| (*p, *id));

                    match (best_allow, best_deny) {
                        (Some((ap, aid)), Some((dp, did))) => {
                            if dp >= ap {
                                (Effect::Deny, Some(did.to_string()))
                            } else {
                                (Effect::Allow, Some(aid.to_string()))
                            }
                        }
                        (Some((_, id)), None) => (Effect::Allow, Some(id.to_string())),
                        (None, Some((_, id))) => (Effect::Deny, Some(id.to_string())),
                        (None, None) => unreachable!(),
                    }
                }
            }
        };

        PolicyDecision { effect, determining_rule, trace }
    }

    /// Evaluate whether a specific capability action is allowed.
    pub fn is_allowed(
        &self,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> bool {
        self.evaluate(action, resource, principal, context).is_allowed()
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::rules::{Operator, PolicyRule};

    fn empty_ctx() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[test]
    fn test_default_deny() {
        let engine = PolicyEngine::new();
        let decision = engine.evaluate("stdio:stdout", "", "", &empty_ctx());
        assert!(decision.is_denied());
        assert!(decision.determining_rule.is_none());
    }

    #[test]
    fn test_default_allow() {
        let engine =
            PolicyEngine::with_config(DefaultPolicy::Allow, ConflictResolution::DenyOverrides);
        let decision = engine.evaluate("stdio:stdout", "", "", &empty_ctx());
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_simple_allow_rule() {
        let mut engine = PolicyEngine::new();
        engine.add_rule(
            PolicyRule::builder("allow-stdio").effect(Effect::Allow).action("stdio:*").build(),
        );

        assert!(engine.is_allowed("stdio:stdout", "", "", &empty_ctx()));
        assert!(!engine.is_allowed("net:http", "", "", &empty_ctx()));
    }

    #[test]
    fn test_deny_overrides() {
        let mut engine = PolicyEngine::new();
        engine.set_conflict_resolution(ConflictResolution::DenyOverrides);

        engine.add_rule(PolicyRule::builder("allow-all").effect(Effect::Allow).action("*").build());
        engine.add_rule(
            PolicyRule::builder("deny-network").effect(Effect::Deny).action("net:*").build(),
        );

        assert!(engine.is_allowed("stdio:stdout", "", "", &empty_ctx()));
        assert!(!engine.is_allowed("net:http", "", "", &empty_ctx()));
    }

    #[test]
    fn test_allow_overrides() {
        let mut engine = PolicyEngine::new();
        engine.set_conflict_resolution(ConflictResolution::AllowOverrides);

        engine.add_rule(PolicyRule::builder("deny-all").effect(Effect::Deny).action("*").build());
        engine.add_rule(
            PolicyRule::builder("allow-stdio").effect(Effect::Allow).action("stdio:*").build(),
        );

        assert!(engine.is_allowed("stdio:stdout", "", "", &empty_ctx()));
        // net:http: deny-all matches, allow-stdio doesn't → allow overrides still needs an allow match
        assert!(!engine.is_allowed("net:http", "", "", &empty_ctx()));
    }

    #[test]
    fn test_priority_based_resolution() {
        let mut engine = PolicyEngine::new();
        engine.set_conflict_resolution(ConflictResolution::PriorityBased);

        engine.add_rule(
            PolicyRule::builder("deny-all").effect(Effect::Deny).action("*").priority(1).build(),
        );
        engine.add_rule(
            PolicyRule::builder("allow-admin")
                .effect(Effect::Allow)
                .action("*")
                .principal("admin")
                .priority(10)
                .build(),
        );

        // Admin gets higher priority allow
        assert!(engine.is_allowed("net:http", "", "admin", &empty_ctx()));
        // Non-admin gets denied
        assert!(!engine.is_allowed("net:http", "", "user", &empty_ctx()));
    }

    #[test]
    fn test_conditional_policy() {
        let mut engine = PolicyEngine::new();
        engine.add_rule(
            PolicyRule::builder("allow-trusted")
                .effect(Effect::Allow)
                .action("net:*")
                .condition("trust_level", Operator::Ge, Value::Int(3))
                .build(),
        );

        let mut ctx = HashMap::new();
        ctx.insert("trust_level".to_string(), Value::Int(5));
        assert!(engine.is_allowed("net:http", "", "", &ctx));

        ctx.insert("trust_level".to_string(), Value::Int(1));
        assert!(!engine.is_allowed("net:http", "", "", &ctx));
    }

    #[test]
    fn test_policy_set() {
        let mut engine = PolicyEngine::new();

        let mut set = PolicySet::new("production");
        set.add_rule(
            PolicyRule::builder("prod-stdio").effect(Effect::Allow).action("stdio:*").build(),
        );
        set.add_rule(
            PolicyRule::builder("prod-fs-read").effect(Effect::Allow).action("fs:read:*").build(),
        );
        engine.add_policy_set(set);

        assert_eq!(engine.rule_count(), 2);
        assert!(engine.is_allowed("stdio:stdout", "", "", &empty_ctx()));
        assert!(engine.is_allowed("fs:read:/data", "", "", &empty_ctx()));
        assert!(!engine.is_allowed("fs:write:/data", "", "", &empty_ctx()));
    }

    #[test]
    fn test_eval_trace() {
        let mut engine = PolicyEngine::new();
        engine.add_rule(PolicyRule::builder("r1").effect(Effect::Allow).action("stdio:*").build());
        engine.add_rule(PolicyRule::builder("r2").effect(Effect::Deny).action("net:*").build());

        let decision = engine.evaluate("stdio:stdout", "", "", &empty_ctx());
        assert_eq!(decision.trace.entries.len(), 2);

        let r1_trace = decision.trace.entries.iter().find(|e| e.rule_id == "r1").unwrap();
        assert!(r1_trace.matched);
        assert_eq!(r1_trace.effect, Some(Effect::Allow));
    }

    #[test]
    fn test_remove_rule() {
        let mut engine = PolicyEngine::new();
        engine.add_rule(PolicyRule::builder("r1").effect(Effect::Allow).action("*").build());
        assert_eq!(engine.rule_count(), 1);

        assert!(engine.remove_rule("r1"));
        assert_eq!(engine.rule_count(), 0);

        assert!(!engine.remove_rule("nonexistent"));
    }

    #[test]
    fn test_resource_and_principal_matching() {
        let mut engine = PolicyEngine::new();
        engine.add_rule(
            PolicyRule::builder("allow-data")
                .effect(Effect::Allow)
                .action("fs:read:*")
                .resource("/data/*")
                .principal("service-*")
                .build(),
        );

        assert!(engine.is_allowed(
            "fs:read:/data/file.txt",
            "/data/file.txt",
            "service-api",
            &empty_ctx()
        ));
        assert!(!engine.is_allowed(
            "fs:read:/data/file.txt",
            "/secret/file.txt",
            "service-api",
            &empty_ctx()
        ));
        assert!(!engine.is_allowed(
            "fs:read:/data/file.txt",
            "/data/file.txt",
            "user-123",
            &empty_ctx()
        ));
    }
}
