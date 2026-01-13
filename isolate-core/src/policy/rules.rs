//! Policy rule definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The effect of a policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Allow the action.
    Allow,
    /// Deny the action.
    Deny,
}

impl std::fmt::Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Deny => write!(f, "deny"),
        }
    }
}

/// Comparison operators for conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// String contains.
    Contains,
    /// String starts with.
    StartsWith,
    /// String ends with.
    EndsWith,
    /// Value is in a list.
    In,
    /// Glob pattern match.
    Matches,
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eq => write!(f, "=="),
            Self::Ne => write!(f, "!="),
            Self::Lt => write!(f, "<"),
            Self::Le => write!(f, "<="),
            Self::Gt => write!(f, ">"),
            Self::Ge => write!(f, ">="),
            Self::Contains => write!(f, "contains"),
            Self::StartsWith => write!(f, "starts_with"),
            Self::EndsWith => write!(f, "ends_with"),
            Self::In => write!(f, "in"),
            Self::Matches => write!(f, "matches"),
        }
    }
}

/// A value in a policy condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// String value.
    String(String),
    /// Integer value.
    Int(i64),
    /// Boolean value.
    Bool(bool),
    /// Float value.
    Float(f64),
    /// List of values.
    List(Vec<Value>),
}

impl Value {
    /// Try to get as string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Try to get as boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Try to get as float.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Int(i) => write!(f, "{}", i),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Float(fl) => write!(f, "{}", fl),
            Self::List(l) => {
                write!(f, "[")?;
                for (i, v) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
        }
    }
}

/// A condition that must be met for a policy rule to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// The attribute path to evaluate (e.g., "tenant.trust_level").
    pub attribute: String,
    /// The comparison operator.
    pub operator: Operator,
    /// The value to compare against.
    pub value: Value,
}

impl Condition {
    /// Create a new condition.
    pub fn new(attribute: impl Into<String>, operator: Operator, value: Value) -> Self {
        Self { attribute: attribute.into(), operator, value }
    }

    /// Evaluate this condition against a context.
    pub fn evaluate(&self, context: &HashMap<String, Value>) -> bool {
        let Some(actual) = context.get(&self.attribute) else {
            return false;
        };

        match &self.operator {
            Operator::Eq => actual == &self.value,
            Operator::Ne => actual != &self.value,
            Operator::Lt => compare_values(actual, &self.value)
                .map(|o| o == std::cmp::Ordering::Less)
                .unwrap_or(false),
            Operator::Le => compare_values(actual, &self.value)
                .map(|o| o != std::cmp::Ordering::Greater)
                .unwrap_or(false),
            Operator::Gt => compare_values(actual, &self.value)
                .map(|o| o == std::cmp::Ordering::Greater)
                .unwrap_or(false),
            Operator::Ge => compare_values(actual, &self.value)
                .map(|o| o != std::cmp::Ordering::Less)
                .unwrap_or(false),
            Operator::Contains => match (actual, &self.value) {
                (Value::String(a), Value::String(b)) => a.contains(b.as_str()),
                _ => false,
            },
            Operator::StartsWith => match (actual, &self.value) {
                (Value::String(a), Value::String(b)) => a.starts_with(b.as_str()),
                _ => false,
            },
            Operator::EndsWith => match (actual, &self.value) {
                (Value::String(a), Value::String(b)) => a.ends_with(b.as_str()),
                _ => false,
            },
            Operator::In => match &self.value {
                Value::List(list) => list.contains(actual),
                _ => false,
            },
            Operator::Matches => match (actual, &self.value) {
                (Value::String(a), Value::String(pattern)) => glob_match(pattern, a),
                _ => false,
            },
        }
    }
}

/// Compare two values, returning ordering if comparable.
fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

/// Simple glob matching (supports * and ?).
fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars = pattern.chars().peekable();
    let t_chars = text.chars().peekable();

    let pattern_parts: Vec<&str> = pattern.split('*').collect();
    if pattern_parts.len() == 1 {
        // No wildcards, check char-by-char with ? support
        if pattern.len() != text.len() {
            return false;
        }
        return p_chars.zip(t_chars).all(|(p, t)| p == '?' || p == t);
    }

    let mut remaining = text;
    for (i, part) in pattern_parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // First part must match at start
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == pattern_parts.len() - 1 {
            // Last part must match at end
            if !remaining.ends_with(part) {
                return false;
            }
            return true;
        } else {
            // Middle parts must appear somewhere
            if let Some(pos) = remaining.find(part) {
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
    }

    true
}

/// A policy rule defining access control.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Unique rule identifier.
    pub id: String,
    /// Human-readable description.
    pub description: Option<String>,
    /// Whether this rule allows or denies.
    pub effect: Effect,
    /// Action patterns this rule applies to (glob patterns).
    pub actions: Vec<String>,
    /// Resource patterns this rule applies to (glob patterns).
    pub resources: Vec<String>,
    /// Principal patterns (who is performing the action).
    pub principals: Vec<String>,
    /// Conditions that must all be true for this rule to apply.
    pub conditions: Vec<Condition>,
    /// Priority (higher = evaluated first, default 0).
    pub priority: i32,
    /// Whether this rule is enabled.
    pub enabled: bool,
}

impl PolicyRule {
    /// Create a new rule builder.
    pub fn builder(id: impl Into<String>) -> PolicyRuleBuilder {
        PolicyRuleBuilder::new(id)
    }

    /// Check if this rule matches the given request.
    pub fn matches(
        &self,
        action: &str,
        resource: &str,
        principal: &str,
        context: &HashMap<String, Value>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        // Check action patterns
        let action_matches = self.actions.is_empty()
            || self.actions.iter().any(|pattern| glob_match(pattern, action));

        // Check resource patterns
        let resource_matches = self.resources.is_empty()
            || self.resources.iter().any(|pattern| glob_match(pattern, resource));

        // Check principal patterns
        let principal_matches = self.principals.is_empty()
            || self.principals.iter().any(|pattern| glob_match(pattern, principal));

        // Check conditions
        let conditions_met = self.conditions.iter().all(|c| c.evaluate(context));

        action_matches && resource_matches && principal_matches && conditions_met
    }
}

/// Builder for PolicyRule.
#[derive(Debug)]
pub struct PolicyRuleBuilder {
    id: String,
    description: Option<String>,
    effect: Effect,
    actions: Vec<String>,
    resources: Vec<String>,
    principals: Vec<String>,
    conditions: Vec<Condition>,
    priority: i32,
    enabled: bool,
}

impl PolicyRuleBuilder {
    /// Create a new builder with the given rule ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: None,
            effect: Effect::Deny,
            actions: Vec::new(),
            resources: Vec::new(),
            principals: Vec::new(),
            conditions: Vec::new(),
            priority: 0,
            enabled: true,
        }
    }

    /// Set the rule description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the effect (allow/deny).
    pub fn effect(mut self, effect: Effect) -> Self {
        self.effect = effect;
        self
    }

    /// Add an action pattern.
    pub fn action(mut self, pattern: impl Into<String>) -> Self {
        self.actions.push(pattern.into());
        self
    }

    /// Add a resource pattern.
    pub fn resource(mut self, pattern: impl Into<String>) -> Self {
        self.resources.push(pattern.into());
        self
    }

    /// Add a principal pattern.
    pub fn principal(mut self, pattern: impl Into<String>) -> Self {
        self.principals.push(pattern.into());
        self
    }

    /// Add a condition.
    pub fn condition(
        mut self,
        attribute: impl Into<String>,
        operator: Operator,
        value: Value,
    ) -> Self {
        self.conditions.push(Condition::new(attribute, operator, value));
        self
    }

    /// Set the priority.
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set whether the rule is enabled.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Build the policy rule.
    pub fn build(self) -> PolicyRule {
        PolicyRule {
            id: self.id,
            description: self.description,
            effect: self.effect,
            actions: self.actions,
            resources: self.resources,
            principals: self.principals,
            conditions: self.conditions,
            priority: self.priority,
            enabled: self.enabled,
        }
    }
}

/// A named set of policy rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySet {
    /// Set name.
    pub name: String,
    /// Description.
    pub description: Option<String>,
    /// Rules in this set.
    pub rules: Vec<PolicyRule>,
    /// Version of the policy set.
    pub version: String,
}

impl PolicySet {
    /// Create a new policy set.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), description: None, rules: Vec::new(), version: "1.0".to_string() }
    }

    /// Add a rule to this set.
    pub fn add_rule(&mut self, rule: PolicyRule) {
        self.rules.push(rule);
    }

    /// Get the number of rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_condition_eq() {
        let cond = Condition::new("role", Operator::Eq, Value::String("admin".into()));
        let mut ctx = HashMap::new();
        ctx.insert("role".to_string(), Value::String("admin".into()));
        assert!(cond.evaluate(&ctx));

        ctx.insert("role".to_string(), Value::String("user".into()));
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_numeric() {
        let cond = Condition::new("trust_level", Operator::Ge, Value::Int(3));
        let mut ctx = HashMap::new();

        ctx.insert("trust_level".to_string(), Value::Int(5));
        assert!(cond.evaluate(&ctx));

        ctx.insert("trust_level".to_string(), Value::Int(2));
        assert!(!cond.evaluate(&ctx));

        ctx.insert("trust_level".to_string(), Value::Int(3));
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_contains() {
        let cond = Condition::new("path", Operator::Contains, Value::String("/data".into()));
        let mut ctx = HashMap::new();
        ctx.insert("path".to_string(), Value::String("/var/data/file.txt".into()));
        assert!(cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_in() {
        let cond = Condition::new(
            "env",
            Operator::In,
            Value::List(vec![Value::String("prod".into()), Value::String("staging".into())]),
        );
        let mut ctx = HashMap::new();
        ctx.insert("env".to_string(), Value::String("prod".into()));
        assert!(cond.evaluate(&ctx));

        ctx.insert("env".to_string(), Value::String("dev".into()));
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_matches() {
        let cond = Condition::new("host", Operator::Matches, Value::String("*.example.com".into()));
        let mut ctx = HashMap::new();
        ctx.insert("host".to_string(), Value::String("api.example.com".into()));
        assert!(cond.evaluate(&ctx));

        ctx.insert("host".to_string(), Value::String("other.com".into()));
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_condition_missing_attribute() {
        let cond = Condition::new("missing", Operator::Eq, Value::Int(1));
        let ctx = HashMap::new();
        assert!(!cond.evaluate(&ctx));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("stdio:*", "stdio:stdout"));
        assert!(glob_match("net:*", "net:http"));
        assert!(glob_match("fs:read:*", "fs:read:/data/file.txt"));
        assert!(!glob_match("fs:read:*", "fs:write:/data/file.txt"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "notexact"));
    }

    #[test]
    fn test_rule_matches() {
        let rule = PolicyRule::builder("test")
            .effect(Effect::Allow)
            .action("stdio:*")
            .principal("tenant-*")
            .build();

        let ctx = HashMap::new();
        assert!(rule.matches("stdio:stdout", "", "tenant-123", &ctx));
        assert!(!rule.matches("net:http", "", "tenant-123", &ctx));
        assert!(!rule.matches("stdio:stdout", "", "admin", &ctx));
    }

    #[test]
    fn test_rule_with_conditions() {
        let rule = PolicyRule::builder("trusted-network")
            .effect(Effect::Allow)
            .action("net:*")
            .condition("trust_level", Operator::Ge, Value::Int(3))
            .build();

        let mut ctx = HashMap::new();
        ctx.insert("trust_level".to_string(), Value::Int(5));
        assert!(rule.matches("net:http", "", "", &ctx));

        ctx.insert("trust_level".to_string(), Value::Int(1));
        assert!(!rule.matches("net:http", "", "", &ctx));
    }

    #[test]
    fn test_disabled_rule() {
        let rule = PolicyRule::builder("disabled")
            .effect(Effect::Allow)
            .action("*")
            .enabled(false)
            .build();

        assert!(!rule.matches("anything", "", "", &HashMap::new()));
    }

    #[test]
    fn test_policy_set() {
        let mut set = PolicySet::new("default");
        set.add_rule(PolicyRule::builder("r1").effect(Effect::Allow).build());
        set.add_rule(PolicyRule::builder("r2").effect(Effect::Deny).build());
        assert_eq!(set.rule_count(), 2);
    }
}
