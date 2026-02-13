//! Policy evaluation engine with condition matching and audit mode.
//!
//! Provides a Rego-like policy evaluation engine that evaluates conditions
//! against runtime context, producing allow/deny decisions with full traces.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Context for policy evaluation (attributes about the request).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvalContext {
    /// Subject attributes (who is making the request).
    pub subject: HashMap<String, ContextValue>,
    /// Resource attributes (what is being accessed).
    pub resource: HashMap<String, ContextValue>,
    /// Action being performed.
    pub action: String,
    /// Environment attributes (when, where).
    pub environment: HashMap<String, ContextValue>,
}

impl EvalContext {
    /// Create a new context.
    pub fn new(action: &str) -> Self {
        Self { action: action.to_string(), ..Default::default() }
    }

    /// Set a subject attribute.
    pub fn with_subject(mut self, key: &str, value: impl Into<ContextValue>) -> Self {
        self.subject.insert(key.to_string(), value.into());
        self
    }

    /// Set a resource attribute.
    pub fn with_resource(mut self, key: &str, value: impl Into<ContextValue>) -> Self {
        self.resource.insert(key.to_string(), value.into());
        self
    }

    /// Set an environment attribute.
    pub fn with_env(mut self, key: &str, value: impl Into<ContextValue>) -> Self {
        self.environment.insert(key.to_string(), value.into());
        self
    }

    /// Resolve an attribute path (e.g., "subject.role" or "resource.path").
    pub fn resolve(&self, path: &str) -> Option<&ContextValue> {
        let parts: Vec<&str> = path.splitn(2, '.').collect();
        if parts.len() != 2 {
            return None;
        }

        let (scope, key) = (parts[0], parts[1]);
        match scope {
            "subject" => self.subject.get(key),
            "resource" => self.resource.get(key),
            "environment" | "env" => self.environment.get(key),
            _ => None,
        }
    }
}

/// A value in a policy context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContextValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    List(Vec<ContextValue>),
}

impl From<&str> for ContextValue {
    fn from(s: &str) -> Self {
        ContextValue::String(s.to_string())
    }
}

impl From<String> for ContextValue {
    fn from(s: String) -> Self {
        ContextValue::String(s)
    }
}

impl From<i64> for ContextValue {
    fn from(v: i64) -> Self {
        ContextValue::Int(v)
    }
}

impl From<bool> for ContextValue {
    fn from(v: bool) -> Self {
        ContextValue::Bool(v)
    }
}

/// Operator for condition matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionOp {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    Contains,
    StartsWith,
    EndsWith,
    In,
    Matches,
}

/// A condition to evaluate against the context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyCondition {
    /// Attribute path (e.g., "subject.role").
    pub attribute: String,
    /// Operator.
    pub op: ConditionOp,
    /// Expected value.
    pub value: ContextValue,
}

impl PolicyCondition {
    /// Create a new condition.
    pub fn new(attribute: &str, op: ConditionOp, value: impl Into<ContextValue>) -> Self {
        Self { attribute: attribute.to_string(), op, value: value.into() }
    }

    /// Evaluate this condition against a context.
    pub fn evaluate(&self, context: &EvalContext) -> bool {
        let actual = match context.resolve(&self.attribute) {
            Some(v) => v,
            None => return false,
        };

        match self.op {
            ConditionOp::Equals => actual == &self.value,
            ConditionOp::NotEquals => actual != &self.value,
            ConditionOp::GreaterThan => compare_values(actual, &self.value) == Some(std::cmp::Ordering::Greater),
            ConditionOp::LessThan => compare_values(actual, &self.value) == Some(std::cmp::Ordering::Less),
            ConditionOp::GreaterOrEqual => {
                matches!(compare_values(actual, &self.value), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))
            }
            ConditionOp::LessOrEqual => {
                matches!(compare_values(actual, &self.value), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
            }
            ConditionOp::Contains => match (actual, &self.value) {
                (ContextValue::String(s), ContextValue::String(sub)) => s.contains(sub.as_str()),
                (ContextValue::List(list), val) => list.contains(val),
                _ => false,
            },
            ConditionOp::StartsWith => match (actual, &self.value) {
                (ContextValue::String(s), ContextValue::String(prefix)) => {
                    s.starts_with(prefix.as_str())
                }
                _ => false,
            },
            ConditionOp::EndsWith => match (actual, &self.value) {
                (ContextValue::String(s), ContextValue::String(suffix)) => {
                    s.ends_with(suffix.as_str())
                }
                _ => false,
            },
            ConditionOp::In => match &self.value {
                ContextValue::List(list) => list.contains(actual),
                _ => false,
            },
            ConditionOp::Matches => match (actual, &self.value) {
                (ContextValue::String(s), ContextValue::String(pattern)) => {
                    glob_match(s, pattern)
                }
                _ => false,
            },
        }
    }
}

fn compare_values(a: &ContextValue, b: &ContextValue) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (ContextValue::Int(a), ContextValue::Int(b)) => Some(a.cmp(b)),
        (ContextValue::Float(a), ContextValue::Float(b)) => a.partial_cmp(b),
        (ContextValue::String(a), ContextValue::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// Simple glob matching (* matches any sequence).
fn glob_match(s: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return s == pattern;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 2 {
        let (prefix, suffix) = (parts[0], parts[1]);
        return s.starts_with(prefix) && s.ends_with(suffix);
    }

    s == pattern
}

/// Effect of a policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEffect {
    Allow,
    Deny,
    AuditOnly,
}

/// A policy rule with conditions and effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRule {
    /// Rule name.
    pub name: String,
    /// Priority (higher = evaluated first).
    pub priority: i32,
    /// Effect if conditions match.
    pub effect: PolicyEffect,
    /// Action pattern to match.
    pub action_pattern: String,
    /// Conditions that must all be true.
    pub conditions: Vec<PolicyCondition>,
    /// Description.
    pub description: String,
    /// Whether this rule is enabled.
    pub enabled: bool,
}

impl EvalRule {
    /// Create a new rule.
    pub fn new(name: &str, effect: PolicyEffect) -> Self {
        Self {
            name: name.to_string(),
            priority: 0,
            effect,
            action_pattern: "*".to_string(),
            conditions: Vec::new(),
            description: String::new(),
            enabled: true,
        }
    }

    /// Set action pattern.
    pub fn action(mut self, pattern: &str) -> Self {
        self.action_pattern = pattern.to_string();
        self
    }

    /// Add a condition.
    pub fn condition(mut self, attr: &str, op: ConditionOp, value: impl Into<ContextValue>) -> Self {
        self.conditions.push(PolicyCondition::new(attr, op, value));
        self
    }

    /// Set priority.
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set description.
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Check if this rule matches the given action.
    fn matches_action(&self, action: &str) -> bool {
        glob_match(action, &self.action_pattern)
    }

    /// Evaluate this rule against a context.
    pub fn evaluate(&self, context: &EvalContext) -> Option<PolicyEffect> {
        if !self.enabled {
            return None;
        }

        if !self.matches_action(&context.action) {
            return None;
        }

        if self.conditions.iter().all(|c| c.evaluate(context)) {
            Some(self.effect)
        } else {
            None
        }
    }
}

/// Result of evaluating all policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResult {
    /// Final decision.
    pub decision: PolicyEffect,
    /// Rules that matched (in evaluation order).
    pub matched_rules: Vec<String>,
    /// Full evaluation trace.
    pub trace: Vec<RuleEvalTrace>,
    /// Evaluation duration.
    pub duration: Duration,
    /// Timestamp.
    pub evaluated_at: SystemTime,
}

/// Trace of a single rule evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvalTrace {
    pub rule_name: String,
    pub action_matched: bool,
    pub conditions_matched: bool,
    pub effect: Option<PolicyEffect>,
}

/// Conflict resolution strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStrategy {
    /// First matching rule wins.
    FirstMatch,
    /// Deny overrides allow.
    DenyOverrides,
    /// Allow overrides deny.
    AllowOverrides,
    /// Highest priority wins.
    PriorityBased,
}

/// Policy evaluator with configurable conflict resolution.
pub struct PolicyEvaluator {
    rules: Vec<EvalRule>,
    strategy: ConflictStrategy,
    default_effect: PolicyEffect,
    audit_mode: bool,
}

impl PolicyEvaluator {
    /// Create a new evaluator with deny-by-default.
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            strategy: ConflictStrategy::DenyOverrides,
            default_effect: PolicyEffect::Deny,
            audit_mode: false,
        }
    }

    /// Set conflict resolution strategy.
    pub fn with_strategy(mut self, strategy: ConflictStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set default effect when no rules match.
    pub fn with_default(mut self, effect: PolicyEffect) -> Self {
        self.default_effect = effect;
        self
    }

    /// Enable audit mode (log decisions but don't enforce).
    pub fn with_audit_mode(mut self, enabled: bool) -> Self {
        self.audit_mode = enabled;
        self
    }

    /// Add a rule.
    pub fn add_rule(&mut self, rule: EvalRule) {
        self.rules.push(rule);
        // Sort by priority descending
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Get rule count.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Evaluate all rules against a context.
    pub fn evaluate(&self, context: &EvalContext) -> EvalResult {
        let start = std::time::Instant::now();
        let mut trace = Vec::new();
        let mut matched_rules = Vec::new();
        let mut allow_count = 0;
        let mut deny_count = 0;
        let mut first_match: Option<PolicyEffect> = None;

        for rule in &self.rules {
            let action_matched = rule.matches_action(&context.action);
            let conditions_matched = action_matched && rule.conditions.iter().all(|c| c.evaluate(context));
            let effect = if conditions_matched { Some(rule.effect) } else { None };

            trace.push(RuleEvalTrace {
                rule_name: rule.name.clone(),
                action_matched,
                conditions_matched,
                effect,
            });

            if let Some(eff) = effect {
                matched_rules.push(rule.name.clone());
                match eff {
                    PolicyEffect::Allow => allow_count += 1,
                    PolicyEffect::Deny => deny_count += 1,
                    PolicyEffect::AuditOnly => {}
                }
                if first_match.is_none() {
                    first_match = Some(eff);
                }
            }
        }

        let decision = match self.strategy {
            ConflictStrategy::FirstMatch => first_match.unwrap_or(self.default_effect),
            ConflictStrategy::DenyOverrides => {
                if deny_count > 0 {
                    PolicyEffect::Deny
                } else if allow_count > 0 {
                    PolicyEffect::Allow
                } else {
                    self.default_effect
                }
            }
            ConflictStrategy::AllowOverrides => {
                if allow_count > 0 {
                    PolicyEffect::Allow
                } else if deny_count > 0 {
                    PolicyEffect::Deny
                } else {
                    self.default_effect
                }
            }
            ConflictStrategy::PriorityBased => first_match.unwrap_or(self.default_effect),
        };

        // In audit mode, always allow but log the would-be decision
        let final_decision = if self.audit_mode { PolicyEffect::Allow } else { decision };

        EvalResult {
            decision: final_decision,
            matched_rules,
            trace,
            duration: start.elapsed(),
            evaluated_at: SystemTime::now(),
        }
    }
}

impl Default for PolicyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Compliance report summarizing policy evaluations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub generated_at: SystemTime,
    pub total_evaluations: u64,
    pub allow_count: u64,
    pub deny_count: u64,
    pub audit_count: u64,
    pub top_denied_actions: Vec<(String, u64)>,
    pub top_triggered_rules: Vec<(String, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_resolve() {
        let ctx = EvalContext::new("fs:read")
            .with_subject("role", "admin")
            .with_resource("path", "/data/secrets")
            .with_env("time_of_day", "morning");

        assert_eq!(ctx.resolve("subject.role"), Some(&ContextValue::String("admin".to_string())));
        assert_eq!(ctx.resolve("resource.path"), Some(&ContextValue::String("/data/secrets".to_string())));
        assert!(ctx.resolve("subject.nonexistent").is_none());
        assert!(ctx.resolve("invalid").is_none());
    }

    #[test]
    fn test_condition_equals() {
        let cond = PolicyCondition::new("subject.role", ConditionOp::Equals, "admin");
        let ctx = EvalContext::new("test").with_subject("role", "admin");
        assert!(cond.evaluate(&ctx));

        let ctx = EvalContext::new("test").with_subject("role", "user");
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_numeric() {
        let cond = PolicyCondition::new("subject.trust_level", ConditionOp::GreaterOrEqual, ContextValue::Int(3));
        let ctx = EvalContext::new("test").with_subject("trust_level", 5i64);
        assert!(cond.evaluate(&ctx));

        let ctx = EvalContext::new("test").with_subject("trust_level", 2i64);
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_contains() {
        let cond = PolicyCondition::new("resource.path", ConditionOp::Contains, "/data");
        let ctx = EvalContext::new("test").with_resource("path", "/var/data/file.txt");
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_matches_glob() {
        let cond = PolicyCondition::new("resource.path", ConditionOp::Matches, "/data/*");
        let ctx = EvalContext::new("test").with_resource("path", "/data/file.txt");
        assert!(cond.evaluate(&ctx));

        let ctx = EvalContext::new("test").with_resource("path", "/other/file.txt");
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_eval_rule() {
        let rule = EvalRule::new("allow-admin", PolicyEffect::Allow)
            .action("fs:*")
            .condition("subject.role", ConditionOp::Equals, "admin");

        let ctx = EvalContext::new("fs:read").with_subject("role", "admin");
        assert_eq!(rule.evaluate(&ctx), Some(PolicyEffect::Allow));

        let ctx = EvalContext::new("fs:read").with_subject("role", "user");
        assert_eq!(rule.evaluate(&ctx), None);

        let ctx = EvalContext::new("net:connect").with_subject("role", "admin");
        assert_eq!(rule.evaluate(&ctx), None);
    }

    #[test]
    fn test_evaluator_deny_overrides() {
        let mut evaluator = PolicyEvaluator::new()
            .with_strategy(ConflictStrategy::DenyOverrides);

        evaluator.add_rule(EvalRule::new("allow-all", PolicyEffect::Allow).action("*"));
        evaluator.add_rule(
            EvalRule::new("deny-secrets", PolicyEffect::Deny)
                .action("fs:*")
                .condition("resource.path", ConditionOp::StartsWith, "/secrets"),
        );

        // Regular file: allowed
        let ctx = EvalContext::new("fs:read").with_resource("path", "/data/file.txt");
        let result = evaluator.evaluate(&ctx);
        assert_eq!(result.decision, PolicyEffect::Allow);

        // Secrets: denied (deny overrides allow)
        let ctx = EvalContext::new("fs:read").with_resource("path", "/secrets/key.pem");
        let result = evaluator.evaluate(&ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_evaluator_default_deny() {
        let evaluator = PolicyEvaluator::new();

        let ctx = EvalContext::new("anything");
        let result = evaluator.evaluate(&ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_evaluator_audit_mode() {
        let mut evaluator = PolicyEvaluator::new().with_audit_mode(true);
        evaluator.add_rule(EvalRule::new("deny-all", PolicyEffect::Deny).action("*"));

        let ctx = EvalContext::new("fs:read");
        let result = evaluator.evaluate(&ctx);

        // In audit mode, always allows
        assert_eq!(result.decision, PolicyEffect::Allow);
        // But trace shows the deny matched
        assert!(!result.matched_rules.is_empty());
    }

    #[test]
    fn test_evaluator_priority() {
        let mut evaluator = PolicyEvaluator::new()
            .with_strategy(ConflictStrategy::PriorityBased);

        evaluator.add_rule(
            EvalRule::new("low-deny", PolicyEffect::Deny)
                .action("*")
                .priority(1),
        );
        evaluator.add_rule(
            EvalRule::new("high-allow", PolicyEffect::Allow)
                .action("*")
                .priority(10),
        );

        let ctx = EvalContext::new("fs:read");
        let result = evaluator.evaluate(&ctx);
        assert_eq!(result.decision, PolicyEffect::Allow);
    }

    #[test]
    fn test_eval_trace() {
        let mut evaluator = PolicyEvaluator::new();
        evaluator.add_rule(EvalRule::new("rule-1", PolicyEffect::Allow).action("fs:read"));
        evaluator.add_rule(EvalRule::new("rule-2", PolicyEffect::Deny).action("net:*"));

        let ctx = EvalContext::new("fs:read");
        let result = evaluator.evaluate(&ctx);

        assert_eq!(result.trace.len(), 2);
        assert!(result.trace[0].action_matched);
        assert!(!result.trace[1].action_matched);
    }
}
