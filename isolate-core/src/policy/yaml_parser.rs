//! YAML-based policy file parser and validator.
//!
//! Parses declarative YAML policy files into the internal policy engine
//! representation, with validation, dry-run mode, and versioning.
//!
//! # Policy File Format
//!
//! ```yaml
//! version: "1.0"
//! name: "production-sandbox-policy"
//! rules:
//!   - id: allow-stdout
//!     effect: allow
//!     actions: ["stdio:stdout", "stdio:stderr"]
//!     description: "Allow standard output for all sandboxes"
//!
//!   - id: deny-network-untrusted
//!     effect: deny
//!     actions: ["net:*"]
//!     conditions:
//!       - field: subject.trust_level
//!         operator: lt
//!         value: 3
//! ```

use super::rules::{Effect, Operator, PolicyRule, PolicySet, Value};
use crate::error::{Error, Result};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A complete YAML policy file definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyFile {
    /// Policy format version.
    pub version: String,
    /// Policy name.
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// Policy rules.
    pub rules: Vec<YamlRule>,
    /// Default effect when no rules match.
    #[serde(default = "default_effect")]
    pub default_effect: String,
    /// Policy metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

fn default_effect() -> String {
    "deny".to_string()
}

/// A policy rule in YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlRule {
    /// Unique rule identifier.
    pub id: String,
    /// Effect: "allow" or "deny".
    pub effect: String,
    /// Actions this rule applies to (supports wildcards).
    pub actions: Vec<String>,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: String,
    /// Conditions that must be met.
    #[serde(default)]
    pub conditions: Vec<YamlCondition>,
    /// Priority (higher = evaluated first).
    #[serde(default)]
    pub priority: i32,
}

/// A condition in YAML format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlCondition {
    /// Attribute path (e.g., "subject.trust_level").
    pub field: String,
    /// Comparison operator.
    pub operator: String,
    /// Value to compare against.
    pub value: serde_json::Value,
}

/// Result of validating a policy file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether the policy is valid.
    pub valid: bool,
    /// Validation errors.
    pub errors: Vec<String>,
    /// Validation warnings.
    pub warnings: Vec<String>,
    /// Number of rules parsed.
    pub rule_count: usize,
}

/// Result of a dry-run evaluation against a YAML policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDryRunResult {
    /// The action being evaluated.
    pub action: String,
    /// The final effect.
    pub effect: String,
    /// Rules that matched.
    pub matching_rules: Vec<String>,
    /// Rules that did not match (with reason).
    pub non_matching_rules: Vec<(String, String)>,
}

/// Parse a YAML policy file string into a PolicyFile.
pub fn parse_policy_yaml(yaml_str: &str) -> Result<PolicyFile> {
    serde_json::from_str::<PolicyFile>(yaml_str)
        .or_else(|_| {
            // Try parsing as YAML-style JSON
            // In production, this would use serde_yaml
            // For now, we support JSON format which is a valid YAML subset
            serde_json::from_str::<PolicyFile>(yaml_str)
        })
        .map_err(|e| Error::Policy(format!("Failed to parse policy file: {}", e)))
}

/// Parse a JSON policy file string into a PolicyFile.
pub fn parse_policy_json(json_str: &str) -> Result<PolicyFile> {
    serde_json::from_str::<PolicyFile>(json_str)
        .map_err(|e| Error::Policy(format!("Failed to parse policy JSON: {}", e)))
}

/// Validate a policy file for correctness.
pub fn validate_policy(policy: &PolicyFile) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check version
    if policy.version != "1.0" {
        warnings.push(format!("Unknown policy version '{}', expected '1.0'", policy.version));
    }

    // Check name
    if policy.name.is_empty() {
        errors.push("Policy name is required".to_string());
    }

    // Validate rules
    let mut seen_ids = std::collections::HashSet::new();
    for rule in &policy.rules {
        // Check unique ID
        if !seen_ids.insert(&rule.id) {
            errors.push(format!("Duplicate rule ID: '{}'", rule.id));
        }

        // Check effect
        if rule.effect != "allow" && rule.effect != "deny" {
            errors.push(format!(
                "Rule '{}': invalid effect '{}', expected 'allow' or 'deny'",
                rule.id, rule.effect
            ));
        }

        // Check actions
        if rule.actions.is_empty() {
            errors.push(format!("Rule '{}': at least one action is required", rule.id));
        }

        // Validate conditions
        for (i, cond) in rule.conditions.iter().enumerate() {
            if cond.field.is_empty() {
                errors.push(format!("Rule '{}', condition {}: field is required", rule.id, i));
            }
            if !["eq", "ne", "lt", "gt", "le", "ge", "contains", "in", "matches"]
                .contains(&cond.operator.as_str())
            {
                errors.push(format!(
                    "Rule '{}', condition {}: unknown operator '{}'",
                    rule.id, i, cond.operator
                ));
            }
        }

        // Warn about broad wildcards
        if rule.actions.iter().any(|a| a == "*") && rule.effect == "allow" {
            warnings.push(format!(
                "Rule '{}': allows all actions '*' — consider restricting to specific actions",
                rule.id
            ));
        }
    }

    // Check default effect
    if policy.default_effect != "allow" && policy.default_effect != "deny" {
        errors.push(format!(
            "Invalid default_effect '{}', expected 'allow' or 'deny'",
            policy.default_effect
        ));
    }

    ValidationResult { valid: errors.is_empty(), errors, warnings, rule_count: policy.rules.len() }
}

/// Convert a PolicyFile into a PolicySet usable by the PolicyEngine.
pub fn compile_policy(policy: &PolicyFile) -> Result<PolicySet> {
    let mut policy_set = PolicySet::new(&policy.name);

    for yaml_rule in &policy.rules {
        let effect = match yaml_rule.effect.as_str() {
            "allow" => Effect::Allow,
            "deny" => Effect::Deny,
            other => return Err(Error::Policy(format!("Invalid effect: {}", other))),
        };

        let mut builder =
            PolicyRule::builder(&yaml_rule.id).effect(effect).priority(yaml_rule.priority);

        for action in &yaml_rule.actions {
            builder = builder.action(action);
        }

        if !yaml_rule.description.is_empty() {
            builder = builder.description(&yaml_rule.description);
        }

        for cond in &yaml_rule.conditions {
            let operator = parse_operator(&cond.operator)?;
            let value = json_to_value(&cond.value);
            builder = builder.condition(&cond.field, operator, value);
        }

        policy_set.add_rule(builder.build());
    }

    Ok(policy_set)
}

fn parse_operator(op: &str) -> Result<Operator> {
    match op {
        "eq" => Ok(Operator::Eq),
        "ne" => Ok(Operator::Ne),
        "lt" => Ok(Operator::Lt),
        "gt" => Ok(Operator::Gt),
        "le" => Ok(Operator::Le),
        "ge" => Ok(Operator::Ge),
        "contains" => Ok(Operator::Contains),
        "in" => Ok(Operator::In),
        "matches" => Ok(Operator::Matches),
        other => Err(Error::Policy(format!("Unknown operator: {}", other))),
    }
}

fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        _ => Value::String(json.to_string()),
    }
}

/// Perform a dry-run evaluation of a policy against a test action.
pub fn dry_run(
    policy: &PolicyFile,
    action: &str,
    context_attrs: &HashMap<String, serde_json::Value>,
) -> PolicyDryRunResult {
    let mut matching_rules = Vec::new();
    let mut non_matching_rules = Vec::new();

    for rule in &policy.rules {
        let action_matches = rule.actions.iter().any(|a| {
            a == action || a == "*" || (a.ends_with('*') && action.starts_with(&a[..a.len() - 1]))
        });

        if !action_matches {
            non_matching_rules.push((rule.id.clone(), "action does not match".to_string()));
            continue;
        }

        let conditions_met = rule.conditions.iter().all(|cond| {
            context_attrs.get(&cond.field).is_some_and(|ctx_val| match cond.operator.as_str() {
                "eq" => ctx_val == &cond.value,
                "ne" => ctx_val != &cond.value,
                "lt" => ctx_val.as_i64().zip(cond.value.as_i64()).is_some_and(|(a, b)| a < b),
                "gt" => ctx_val.as_i64().zip(cond.value.as_i64()).is_some_and(|(a, b)| a > b),
                "le" => ctx_val.as_i64().zip(cond.value.as_i64()).is_some_and(|(a, b)| a <= b),
                "ge" => ctx_val.as_i64().zip(cond.value.as_i64()).is_some_and(|(a, b)| a >= b),
                _ => false,
            })
        });

        if conditions_met {
            matching_rules.push(rule.id.clone());
        } else {
            non_matching_rules.push((rule.id.clone(), "conditions not met".to_string()));
        }
    }

    // Determine final effect
    let effect = if matching_rules.is_empty() {
        policy.default_effect.clone()
    } else {
        // Last matching rule wins (or could use priority-based)
        let last_match_id = matching_rules.last().unwrap();
        policy
            .rules
            .iter()
            .find(|r| r.id == *last_match_id)
            .map(|r| r.effect.clone())
            .unwrap_or_else(|| policy.default_effect.clone())
    };

    PolicyDryRunResult { action: action.to_string(), effect, matching_rules, non_matching_rules }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_policy_json() -> &'static str {
        r#"{
            "version": "1.0",
            "name": "test-policy",
            "description": "Test policy for unit tests",
            "default_effect": "deny",
            "rules": [
                {
                    "id": "allow-stdout",
                    "effect": "allow",
                    "actions": ["stdio:stdout", "stdio:stderr"],
                    "description": "Allow standard output"
                },
                {
                    "id": "deny-network-untrusted",
                    "effect": "deny",
                    "actions": ["net:*"],
                    "conditions": [
                        {
                            "field": "subject.trust_level",
                            "operator": "lt",
                            "value": 3
                        }
                    ]
                },
                {
                    "id": "allow-fs-read",
                    "effect": "allow",
                    "actions": ["fs:read"],
                    "conditions": [
                        {
                            "field": "resource.path",
                            "operator": "eq",
                            "value": "/data"
                        }
                    ],
                    "priority": 10
                }
            ],
            "metadata": {
                "author": "test",
                "environment": "production"
            }
        }"#
    }

    #[test]
    fn test_parse_policy_json() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();
        assert_eq!(policy.name, "test-policy");
        assert_eq!(policy.version, "1.0");
        assert_eq!(policy.rules.len(), 3);
        assert_eq!(policy.default_effect, "deny");
    }

    #[test]
    fn test_validate_policy_valid() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();
        let result = validate_policy(&policy);
        assert!(result.valid, "errors: {:?}", result.errors);
        assert_eq!(result.rule_count, 3);
    }

    #[test]
    fn test_validate_policy_duplicate_ids() {
        let json = r#"{
            "version": "1.0",
            "name": "bad-policy",
            "rules": [
                { "id": "rule-1", "effect": "allow", "actions": ["*"] },
                { "id": "rule-1", "effect": "deny", "actions": ["net:*"] }
            ]
        }"#;
        let policy = parse_policy_json(json).unwrap();
        let result = validate_policy(&policy);
        assert!(!result.valid);
        assert!(result.errors.iter().any(|e| e.contains("Duplicate")));
    }

    #[test]
    fn test_validate_policy_invalid_effect() {
        let json = r#"{
            "version": "1.0",
            "name": "bad",
            "rules": [
                { "id": "r1", "effect": "maybe", "actions": ["*"] }
            ]
        }"#;
        let policy = parse_policy_json(json).unwrap();
        let result = validate_policy(&policy);
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_policy_invalid_operator() {
        let json = r#"{
            "version": "1.0",
            "name": "bad",
            "rules": [
                {
                    "id": "r1",
                    "effect": "allow",
                    "actions": ["*"],
                    "conditions": [
                        { "field": "x", "operator": "banana", "value": 1 }
                    ]
                }
            ]
        }"#;
        let policy = parse_policy_json(json).unwrap();
        let result = validate_policy(&policy);
        assert!(!result.valid);
    }

    #[test]
    fn test_compile_policy() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();
        let policy_set = compile_policy(&policy).unwrap();
        assert_eq!(policy_set.rule_count(), 3);
    }

    #[test]
    fn test_dry_run_matching() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();

        let result = dry_run(&policy, "stdio:stdout", &HashMap::new());
        assert_eq!(result.effect, "allow");
        assert!(result.matching_rules.contains(&"allow-stdout".to_string()));
    }

    #[test]
    fn test_dry_run_no_match() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();

        let result = dry_run(&policy, "unknown:action", &HashMap::new());
        assert_eq!(result.effect, "deny"); // default effect
        assert!(result.matching_rules.is_empty());
    }

    #[test]
    fn test_dry_run_with_conditions() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();

        let mut attrs = HashMap::new();
        attrs.insert("subject.trust_level".to_string(), serde_json::json!(1));

        let result = dry_run(&policy, "net:http", &attrs);
        assert_eq!(result.effect, "deny");
        assert!(result.matching_rules.contains(&"deny-network-untrusted".to_string()));
    }

    #[test]
    fn test_dry_run_condition_not_met() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();

        let mut attrs = HashMap::new();
        attrs.insert("subject.trust_level".to_string(), serde_json::json!(5));

        let result = dry_run(&policy, "net:http", &attrs);
        // Trust level 5 >= 3, so the deny rule doesn't match
        assert_eq!(result.effect, "deny"); // default effect since no allow rule matches
    }

    #[test]
    fn test_wildcard_action_matching() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();

        let mut attrs = HashMap::new();
        attrs.insert("subject.trust_level".to_string(), serde_json::json!(1));

        // "net:*" should match "net:tcp"
        let result = dry_run(&policy, "net:tcp", &attrs);
        assert!(result.matching_rules.contains(&"deny-network-untrusted".to_string()));
    }

    #[test]
    fn test_metadata_preserved() {
        let policy = parse_policy_json(sample_policy_json()).unwrap();
        assert_eq!(policy.metadata.get("author").unwrap(), "test");
        assert_eq!(policy.metadata.get("environment").unwrap(), "production");
    }
}
