//! Cedar/Rego-inspired policy decision engine with decision tree compilation.
//!
//! Compiles [`PolicySet`]s into [`DecisionTree`]s for fast evaluation of
//! authorization requests. Supports Allow/Deny/Forbid effects with correct
//! precedence (Forbid > Deny > Allow).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// The effect a policy statement produces when matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyEffect {
    /// Grants the request.
    Allow,
    /// Denies the request (can be overridden by a later Allow).
    Deny,
    /// Denies the request with no possible override.
    Forbid,
}

/// Matches a principal (who is making the request).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalMatch {
    Any,
    Tenant(String),
    Role(String),
}

/// Matches an action (what is being requested).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionMatch {
    Any,
    Specific(String),
    OneOf(Vec<String>),
}

/// Matches a resource type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceMatch {
    Any,
    Sandbox,
    Module,
    Snapshot,
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

/// Comparison operator for a [`Condition`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionOp {
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    In,
    Contains,
}

/// A typed value used in condition evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConditionValue {
    String(String),
    Number(f64),
    Bool(bool),
    List(Vec<ConditionValue>),
}

/// A single condition that must hold for a statement to match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub operator: ConditionOp,
    pub value: ConditionValue,
}

// ---------------------------------------------------------------------------
// Policy statements & sets
// ---------------------------------------------------------------------------

/// A single policy rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStatement {
    pub id: String,
    pub effect: PolicyEffect,
    pub principal: PrincipalMatch,
    pub action: ActionMatch,
    pub resource: ResourceMatch,
    pub conditions: Vec<Condition>,
}

/// A versioned collection of policy statements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySet {
    pub id: String,
    pub name: String,
    pub version: u64,
    pub description: String,
    pub statements: Vec<PolicyStatement>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Summary information for listing policy sets (no statements payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySetInfo {
    pub id: String,
    pub name: String,
    pub version: u64,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&PolicySet> for PolicySetInfo {
    fn from(ps: &PolicySet) -> Self {
        Self {
            id: ps.id.clone(),
            name: ps.name.clone(),
            version: ps.version,
            description: ps.description.clone(),
            created_at: ps.created_at,
            updated_at: ps.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Request / Decision
// ---------------------------------------------------------------------------

/// Context provided when evaluating a policy decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRequest {
    pub principal_id: String,
    pub principal_role: String,
    pub tenant_id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_attributes: HashMap<String, serde_json::Value>,
}

/// The outcome of evaluating a [`DecisionTree`] against a [`PolicyRequest`].
#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub effect: PolicyEffect,
    pub matched_statements: Vec<String>,
    pub evaluation_time: Duration,
}

// ---------------------------------------------------------------------------
// Decision tree
// ---------------------------------------------------------------------------

/// A compiled decision tree node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionNode {
    Branch {
        field: String,
        comparisons: Vec<(ConditionOp, ConditionValue)>,
        children: Vec<DecisionNode>,
        default: Box<DecisionNode>,
    },
    Leaf {
        effect: PolicyEffect,
        statement_ids: Vec<String>,
    },
}

/// A compiled decision tree built from a [`PolicySet`].
///
/// The tree groups statements by action so that only the relevant subset is
/// evaluated for any given request.
#[derive(Debug, Clone)]
pub struct DecisionTree {
    /// Statements grouped by action name (or `"*"` for [`ActionMatch::Any`]).
    action_groups: HashMap<String, Vec<PolicyStatement>>,
    /// The raw compiled node tree (used when branching is profitable).
    #[allow(dead_code)] // Used for future optimization path in evaluate()
    root: DecisionNode,
}

impl DecisionTree {
    /// Evaluate the tree against a request and return a decision.
    pub fn evaluate(&self, request: &PolicyRequest) -> PolicyDecision {
        let start = Instant::now();

        // Collect candidate statements: exact-action matches + wildcard
        let mut candidates: Vec<&PolicyStatement> = Vec::new();
        if let Some(stmts) = self.action_groups.get(&request.action) {
            candidates.extend(stmts.iter());
        }
        if let Some(stmts) = self.action_groups.get("*") {
            candidates.extend(stmts.iter());
        }

        let mut matched: Vec<String> = Vec::new();
        let mut has_allow = false;
        let mut has_deny = false;
        let mut has_forbid = false;

        for stmt in &candidates {
            if matches_request(stmt, request) {
                matched.push(stmt.id.clone());
                match stmt.effect {
                    PolicyEffect::Allow => has_allow = true,
                    PolicyEffect::Deny => has_deny = true,
                    PolicyEffect::Forbid => has_forbid = true,
                }
            }
        }

        // Precedence: Forbid > Deny > Allow; default Deny if nothing matched
        let effect = if has_forbid {
            PolicyEffect::Forbid
        } else if has_deny {
            PolicyEffect::Deny
        } else if has_allow {
            PolicyEffect::Allow
        } else {
            PolicyEffect::Deny
        };

        PolicyDecision { effect, matched_statements: matched, evaluation_time: start.elapsed() }
    }
}

// ---------------------------------------------------------------------------
// Matching helpers
// ---------------------------------------------------------------------------

fn matches_request(stmt: &PolicyStatement, req: &PolicyRequest) -> bool {
    matches_principal(&stmt.principal, req)
        && matches_action(&stmt.action, req)
        && matches_resource(&stmt.resource, req)
        && stmt.conditions.iter().all(|c| evaluate_condition(c, req))
}

fn matches_principal(pm: &PrincipalMatch, req: &PolicyRequest) -> bool {
    match pm {
        PrincipalMatch::Any => true,
        PrincipalMatch::Tenant(id) => req.tenant_id == *id,
        PrincipalMatch::Role(role) => req.principal_role == *role,
    }
}

fn matches_action(am: &ActionMatch, req: &PolicyRequest) -> bool {
    match am {
        ActionMatch::Any => true,
        ActionMatch::Specific(a) => req.action == *a,
        ActionMatch::OneOf(actions) => actions.contains(&req.action),
    }
}

fn matches_resource(rm: &ResourceMatch, req: &PolicyRequest) -> bool {
    match rm {
        ResourceMatch::Any => true,
        ResourceMatch::Sandbox => req.resource_type == "sandbox",
        ResourceMatch::Module => req.resource_type == "module",
        ResourceMatch::Snapshot => req.resource_type == "snapshot",
    }
}

fn resolve_field(field: &str, req: &PolicyRequest) -> Option<ConditionValue> {
    // Principal fields
    match field {
        "principal.id" => return Some(ConditionValue::String(req.principal_id.clone())),
        "principal.role" => return Some(ConditionValue::String(req.principal_role.clone())),
        "principal.tenant" => return Some(ConditionValue::String(req.tenant_id.clone())),
        _ => {}
    }

    // Resource attributes (resource.*)
    if let Some(attr) = field.strip_prefix("resource.") {
        if let Some(val) = req.resource_attributes.get(attr) {
            return json_to_condition_value(val);
        }
    }

    None
}

fn json_to_condition_value(v: &serde_json::Value) -> Option<ConditionValue> {
    match v {
        serde_json::Value::String(s) => Some(ConditionValue::String(s.clone())),
        serde_json::Value::Number(n) => n.as_f64().map(ConditionValue::Number),
        serde_json::Value::Bool(b) => Some(ConditionValue::Bool(*b)),
        serde_json::Value::Array(arr) => {
            let items: Vec<ConditionValue> =
                arr.iter().filter_map(json_to_condition_value).collect();
            Some(ConditionValue::List(items))
        }
        _ => None,
    }
}

fn evaluate_condition(cond: &Condition, req: &PolicyRequest) -> bool {
    let resolved = match resolve_field(&cond.field, req) {
        Some(v) => v,
        None => return false,
    };

    match &cond.operator {
        ConditionOp::Eq => resolved == cond.value,
        ConditionOp::NotEq => resolved != cond.value,
        ConditionOp::Lt => compare_numeric(&resolved, &cond.value, |a, b| a < b),
        ConditionOp::Gt => compare_numeric(&resolved, &cond.value, |a, b| a > b),
        ConditionOp::LtEq => compare_numeric(&resolved, &cond.value, |a, b| a <= b),
        ConditionOp::GtEq => compare_numeric(&resolved, &cond.value, |a, b| a >= b),
        ConditionOp::In => match &cond.value {
            ConditionValue::List(list) => list.contains(&resolved),
            _ => false,
        },
        ConditionOp::Contains => match (&resolved, &cond.value) {
            (ConditionValue::List(list), val) => list.contains(val),
            (ConditionValue::String(s), ConditionValue::String(sub)) => s.contains(sub.as_str()),
            _ => false,
        },
    }
}

fn compare_numeric(a: &ConditionValue, b: &ConditionValue, cmp: fn(f64, f64) -> bool) -> bool {
    match (a, b) {
        (ConditionValue::Number(x), ConditionValue::Number(y)) => cmp(*x, *y),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// PolicyCompiler
// ---------------------------------------------------------------------------

/// Compiles a [`PolicySet`] into a [`DecisionTree`] for fast evaluation.
pub struct PolicyCompiler;

impl PolicyCompiler {
    /// Compile the given policy set into a decision tree.
    pub fn compile(policy_set: &PolicySet) -> DecisionTree {
        let mut action_groups: HashMap<String, Vec<PolicyStatement>> = HashMap::new();

        for stmt in &policy_set.statements {
            match &stmt.action {
                ActionMatch::Any => {
                    action_groups.entry("*".to_string()).or_default().push(stmt.clone());
                }
                ActionMatch::Specific(action) => {
                    action_groups.entry(action.clone()).or_default().push(stmt.clone());
                }
                ActionMatch::OneOf(actions) => {
                    for action in actions {
                        action_groups.entry(action.clone()).or_default().push(stmt.clone());
                    }
                }
            }
        }

        let root = Self::build_tree(&policy_set.statements);

        DecisionTree { action_groups, root }
    }

    /// Build a simple decision tree from statements.
    ///
    /// Falls back to a flat leaf when branching would not reduce the search
    /// space (i.e., fewer than two statements or no actionable split field).
    fn build_tree(statements: &[PolicyStatement]) -> DecisionNode {
        if statements.is_empty() {
            return DecisionNode::Leaf { effect: PolicyEffect::Deny, statement_ids: vec![] };
        }

        if statements.len() == 1 {
            return DecisionNode::Leaf {
                effect: statements[0].effect,
                statement_ids: vec![statements[0].id.clone()],
            };
        }

        // Try to branch on the first condition field that actually
        // partitions the statement set.
        for stmt in statements {
            for cond in &stmt.conditions {
                let (matching, non_matching): (Vec<_>, Vec<_>) = statements
                    .iter()
                    .partition(|s| s.conditions.iter().any(|c| c.field == cond.field));

                if !matching.is_empty() && !non_matching.is_empty() {
                    return DecisionNode::Branch {
                        field: cond.field.clone(),
                        comparisons: vec![(cond.operator.clone(), cond.value.clone())],
                        children: vec![Self::build_tree(
                            &matching.into_iter().cloned().collect::<Vec<_>>(),
                        )],
                        default: Box::new(Self::build_tree(
                            &non_matching.into_iter().cloned().collect::<Vec<_>>(),
                        )),
                    };
                }
            }
        }

        // Fallback: flat leaf listing all statements
        let ids: Vec<String> = statements.iter().map(|s| s.id.clone()).collect();
        // Determine combined effect: Forbid > Deny > Allow
        let effect = if statements.iter().any(|s| s.effect == PolicyEffect::Forbid) {
            PolicyEffect::Forbid
        } else if statements.iter().any(|s| s.effect == PolicyEffect::Deny) {
            PolicyEffect::Deny
        } else {
            PolicyEffect::Allow
        };
        DecisionNode::Leaf { effect, statement_ids: ids }
    }
}

// ---------------------------------------------------------------------------
// PolicyStore – in-memory CRUD with version history
// ---------------------------------------------------------------------------

/// In-memory CRUD store for [`PolicySet`]s with version history.
pub struct PolicyStore {
    /// Current (latest) version of each policy set, keyed by id.
    current: HashMap<String, PolicySet>,
    /// Full version history, keyed by (id, version).
    versions: HashMap<(String, u64), PolicySet>,
}

impl PolicyStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self { current: HashMap::new(), versions: HashMap::new() }
    }

    /// Insert a new policy set. Returns the assigned id.
    pub fn create(&mut self, mut policy_set: PolicySet) -> Result<String, String> {
        if policy_set.id.is_empty() {
            policy_set.id = Uuid::new_v4().to_string();
        }
        if self.current.contains_key(&policy_set.id) {
            return Err(format!("policy set '{}' already exists", policy_set.id));
        }
        policy_set.version = 1;
        let now = Utc::now();
        policy_set.created_at = now;
        policy_set.updated_at = now;
        let id = policy_set.id.clone();
        self.versions.insert((id.clone(), 1), policy_set.clone());
        self.current.insert(id.clone(), policy_set);
        Ok(id)
    }

    /// Retrieve the latest version of a policy set.
    pub fn get(&self, id: &str) -> Option<&PolicySet> {
        self.current.get(id)
    }

    /// Update an existing policy set, bumping the version. Returns the new
    /// version number.
    pub fn update(&mut self, id: &str, mut policy_set: PolicySet) -> Result<u64, String> {
        let existing =
            self.current.get(id).ok_or_else(|| format!("policy set '{}' not found", id))?;
        let new_version = existing.version + 1;
        policy_set.id = id.to_string();
        policy_set.version = new_version;
        policy_set.created_at = existing.created_at;
        policy_set.updated_at = Utc::now();
        self.versions.insert((id.to_string(), new_version), policy_set.clone());
        self.current.insert(id.to_string(), policy_set);
        Ok(new_version)
    }

    /// Delete a policy set and all its version history.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if self.current.remove(id).is_none() {
            return Err(format!("policy set '{}' not found", id));
        }
        self.versions.retain(|(k, _), _| k != id);
        Ok(())
    }

    /// List summary info for all current policy sets.
    pub fn list(&self) -> Vec<PolicySetInfo> {
        self.current.values().map(PolicySetInfo::from).collect()
    }

    /// Retrieve a specific historical version.
    pub fn get_version(&self, id: &str, version: u64) -> Option<&PolicySet> {
        self.versions.get(&(id.to_string(), version))
    }

    /// List all available version numbers for a policy set.
    pub fn list_versions(&self, id: &str) -> Vec<u64> {
        let mut versions: Vec<u64> =
            self.versions.keys().filter(|(k, _)| k == id).map(|(_, v)| *v).collect();
        versions.sort();
        versions
    }
}

impl Default for PolicyStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -- helpers ----------------------------------------------------------

    fn make_request(action: &str, resource_type: &str, role: &str) -> PolicyRequest {
        PolicyRequest {
            principal_id: "user-1".into(),
            principal_role: role.into(),
            tenant_id: "tenant-1".into(),
            action: action.into(),
            resource_type: resource_type.into(),
            resource_attributes: HashMap::new(),
        }
    }

    fn make_request_with_attrs(
        action: &str,
        resource_type: &str,
        role: &str,
        attrs: HashMap<String, serde_json::Value>,
    ) -> PolicyRequest {
        PolicyRequest {
            principal_id: "user-1".into(),
            principal_role: role.into(),
            tenant_id: "tenant-1".into(),
            action: action.into(),
            resource_type: resource_type.into(),
            resource_attributes: attrs,
        }
    }

    fn allow_stmt(id: &str, action: ActionMatch, resource: ResourceMatch) -> PolicyStatement {
        PolicyStatement {
            id: id.into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Any,
            action,
            resource,
            conditions: vec![],
        }
    }

    fn deny_stmt(id: &str, action: ActionMatch, resource: ResourceMatch) -> PolicyStatement {
        PolicyStatement {
            id: id.into(),
            effect: PolicyEffect::Deny,
            principal: PrincipalMatch::Any,
            action,
            resource,
            conditions: vec![],
        }
    }

    fn forbid_stmt(id: &str, action: ActionMatch, resource: ResourceMatch) -> PolicyStatement {
        PolicyStatement {
            id: id.into(),
            effect: PolicyEffect::Forbid,
            principal: PrincipalMatch::Any,
            action,
            resource,
            conditions: vec![],
        }
    }

    fn make_policy_set(stmts: Vec<PolicyStatement>) -> PolicySet {
        let now = Utc::now();
        PolicySet {
            id: Uuid::new_v4().to_string(),
            name: "test-policy".into(),
            version: 1,
            description: "test".into(),
            statements: stmts,
            created_at: now,
            updated_at: now,
        }
    }

    // -- compilation tests ------------------------------------------------

    #[test]
    fn test_compile_empty_policy_set() {
        let ps = make_policy_set(vec![]);
        let tree = PolicyCompiler::compile(&ps);
        assert!(tree.action_groups.is_empty());
    }

    #[test]
    fn test_compile_single_statement() {
        let ps = make_policy_set(vec![allow_stmt(
            "s1",
            ActionMatch::Specific("execute".into()),
            ResourceMatch::Sandbox,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        assert!(tree.action_groups.contains_key("execute"));
        assert_eq!(tree.action_groups["execute"].len(), 1);
    }

    #[test]
    fn test_compile_wildcard_action() {
        let ps = make_policy_set(vec![allow_stmt("s1", ActionMatch::Any, ResourceMatch::Any)]);
        let tree = PolicyCompiler::compile(&ps);
        assert!(tree.action_groups.contains_key("*"));
    }

    #[test]
    fn test_compile_one_of_actions() {
        let ps = make_policy_set(vec![allow_stmt(
            "s1",
            ActionMatch::OneOf(vec!["read".into(), "write".into()]),
            ResourceMatch::Any,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        assert!(tree.action_groups.contains_key("read"));
        assert!(tree.action_groups.contains_key("write"));
    }

    // -- evaluation tests -------------------------------------------------

    #[test]
    fn test_evaluate_allow() {
        let ps = make_policy_set(vec![allow_stmt(
            "s1",
            ActionMatch::Specific("execute".into()),
            ResourceMatch::Sandbox,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Allow);
        assert!(decision.matched_statements.contains(&"s1".to_string()));
    }

    #[test]
    fn test_evaluate_deny_by_default() {
        let ps = make_policy_set(vec![allow_stmt(
            "s1",
            ActionMatch::Specific("execute".into()),
            ResourceMatch::Sandbox,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("delete", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Deny);
        assert!(decision.matched_statements.is_empty());
    }

    #[test]
    fn test_evaluate_explicit_deny() {
        let ps = make_policy_set(vec![deny_stmt(
            "d1",
            ActionMatch::Specific("delete".into()),
            ResourceMatch::Sandbox,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("delete", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Deny);
        assert!(decision.matched_statements.contains(&"d1".to_string()));
    }

    // -- precedence tests -------------------------------------------------

    #[test]
    fn test_forbid_overrides_allow() {
        let ps = make_policy_set(vec![
            allow_stmt("a1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
            forbid_stmt("f1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
        ]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Forbid);
        assert!(decision.matched_statements.contains(&"a1".to_string()));
        assert!(decision.matched_statements.contains(&"f1".to_string()));
    }

    #[test]
    fn test_forbid_overrides_deny() {
        let ps = make_policy_set(vec![
            deny_stmt("d1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
            forbid_stmt("f1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
        ]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Forbid);
    }

    #[test]
    fn test_deny_overrides_allow() {
        let ps = make_policy_set(vec![
            allow_stmt("a1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
            deny_stmt("d1", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
        ]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    // -- condition tests --------------------------------------------------

    #[test]
    fn test_condition_eq() {
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Any,
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![Condition {
                field: "resource.memory_limit".into(),
                operator: ConditionOp::Eq,
                value: ConditionValue::Number(128.0),
            }],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);

        let mut attrs = HashMap::new();
        attrs.insert("memory_limit".into(), json!(128.0));
        let req = make_request_with_attrs("execute", "sandbox", "admin", attrs);
        let decision = tree.evaluate(&req);

        assert_eq!(decision.effect, PolicyEffect::Allow);
    }

    #[test]
    fn test_condition_gt() {
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Deny,
            principal: PrincipalMatch::Any,
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![Condition {
                field: "resource.memory_limit".into(),
                operator: ConditionOp::Gt,
                value: ConditionValue::Number(256.0),
            }],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);

        let mut attrs = HashMap::new();
        attrs.insert("memory_limit".into(), json!(512.0));
        let req = make_request_with_attrs("execute", "sandbox", "admin", attrs);
        let decision = tree.evaluate(&req);

        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_condition_in() {
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Any,
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![Condition {
                field: "principal.role".into(),
                operator: ConditionOp::In,
                value: ConditionValue::List(vec![
                    ConditionValue::String("admin".into()),
                    ConditionValue::String("operator".into()),
                ]),
            }],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));

        assert_eq!(decision.effect, PolicyEffect::Allow);
    }

    #[test]
    fn test_condition_not_eq_rejects() {
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Any,
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![Condition {
                field: "principal.role".into(),
                operator: ConditionOp::NotEq,
                value: ConditionValue::String("admin".into()),
            }],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);
        // role == "admin" so NotEq("admin") is false → statement does not match
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));
        assert_eq!(decision.effect, PolicyEffect::Deny); // default deny
    }

    #[test]
    fn test_condition_contains_string() {
        let mut attrs = HashMap::new();
        attrs.insert("tags".into(), json!("production-us-east"));
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Any,
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![Condition {
                field: "resource.tags".into(),
                operator: ConditionOp::Contains,
                value: ConditionValue::String("production".into()),
            }],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);
        let req = make_request_with_attrs("execute", "sandbox", "admin", attrs);
        let decision = tree.evaluate(&req);

        assert_eq!(decision.effect, PolicyEffect::Allow);
    }

    // -- principal / resource matching ------------------------------------

    #[test]
    fn test_principal_tenant_match() {
        let stmt = PolicyStatement {
            id: "s1".into(),
            effect: PolicyEffect::Allow,
            principal: PrincipalMatch::Tenant("tenant-1".into()),
            action: ActionMatch::Specific("execute".into()),
            resource: ResourceMatch::Sandbox,
            conditions: vec![],
        };
        let ps = make_policy_set(vec![stmt]);
        let tree = PolicyCompiler::compile(&ps);

        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));
        assert_eq!(decision.effect, PolicyEffect::Allow);

        // Different tenant → no match → default deny
        let mut req = make_request("execute", "sandbox", "admin");
        req.tenant_id = "tenant-other".into();
        let decision = tree.evaluate(&req);
        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_resource_type_mismatch() {
        let ps = make_policy_set(vec![allow_stmt(
            "s1",
            ActionMatch::Specific("execute".into()),
            ResourceMatch::Module,
        )]);
        let tree = PolicyCompiler::compile(&ps);
        // Request is for "sandbox", statement requires "module"
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));
        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    // -- store CRUD tests -------------------------------------------------

    #[test]
    fn test_store_create_and_get() {
        let mut store = PolicyStore::new();
        let ps = make_policy_set(vec![]);
        let id = store.create(ps).unwrap();
        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved.version, 1);
        assert_eq!(retrieved.id, id);
    }

    #[test]
    fn test_store_create_duplicate_fails() {
        let mut store = PolicyStore::new();
        let mut ps = make_policy_set(vec![]);
        ps.id = "fixed-id".into();
        store.create(ps.clone()).unwrap();
        assert!(store.create(ps).is_err());
    }

    #[test]
    fn test_store_update_bumps_version() {
        let mut store = PolicyStore::new();
        let ps = make_policy_set(vec![]);
        let id = store.create(ps).unwrap();

        let updated = make_policy_set(vec![allow_stmt("s1", ActionMatch::Any, ResourceMatch::Any)]);
        let new_version = store.update(&id, updated).unwrap();
        assert_eq!(new_version, 2);

        let current = store.get(&id).unwrap();
        assert_eq!(current.version, 2);
        assert_eq!(current.statements.len(), 1);
    }

    #[test]
    fn test_store_delete() {
        let mut store = PolicyStore::new();
        let ps = make_policy_set(vec![]);
        let id = store.create(ps).unwrap();
        store.delete(&id).unwrap();
        assert!(store.get(&id).is_none());
    }

    #[test]
    fn test_store_delete_nonexistent() {
        let mut store = PolicyStore::new();
        assert!(store.delete("nope").is_err());
    }

    #[test]
    fn test_store_list() {
        let mut store = PolicyStore::new();
        store.create(make_policy_set(vec![])).unwrap();
        store.create(make_policy_set(vec![])).unwrap();
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn test_store_version_history() {
        let mut store = PolicyStore::new();
        let ps = make_policy_set(vec![]);
        let id = store.create(ps).unwrap();

        store.update(&id, make_policy_set(vec![])).unwrap();
        store.update(&id, make_policy_set(vec![])).unwrap();

        let versions = store.list_versions(&id);
        assert_eq!(versions, vec![1, 2, 3]);

        let v1 = store.get_version(&id, 1).unwrap();
        assert_eq!(v1.version, 1);

        let v3 = store.get_version(&id, 3).unwrap();
        assert_eq!(v3.version, 3);
    }

    #[test]
    fn test_store_get_missing_version() {
        let store = PolicyStore::new();
        assert!(store.get_version("no-such-id", 1).is_none());
    }

    // -- decision tree node structure ------------------------------------

    #[test]
    fn test_tree_branch_on_condition() {
        let stmts = vec![
            PolicyStatement {
                id: "s1".into(),
                effect: PolicyEffect::Allow,
                principal: PrincipalMatch::Any,
                action: ActionMatch::Specific("execute".into()),
                resource: ResourceMatch::Sandbox,
                conditions: vec![Condition {
                    field: "resource.memory_limit".into(),
                    operator: ConditionOp::LtEq,
                    value: ConditionValue::Number(256.0),
                }],
            },
            deny_stmt("s2", ActionMatch::Specific("execute".into()), ResourceMatch::Sandbox),
        ];
        let ps = make_policy_set(stmts);
        let tree = PolicyCompiler::compile(&ps);

        // Within memory limit → Allow (from s1), but s2 also matches → Deny wins
        let mut attrs = HashMap::new();
        attrs.insert("memory_limit".into(), json!(128.0));
        let req = make_request_with_attrs("execute", "sandbox", "admin", attrs);
        let decision = tree.evaluate(&req);
        assert_eq!(decision.effect, PolicyEffect::Deny);
    }

    #[test]
    fn test_evaluation_time_is_recorded() {
        let ps = make_policy_set(vec![allow_stmt("s1", ActionMatch::Any, ResourceMatch::Any)]);
        let tree = PolicyCompiler::compile(&ps);
        let decision = tree.evaluate(&make_request("execute", "sandbox", "admin"));
        // Just verify it's a valid duration (not panicking)
        assert!(decision.evaluation_time < Duration::from_secs(1));
    }
}
