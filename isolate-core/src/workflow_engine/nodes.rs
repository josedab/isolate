//! Node types for the workflow engine.

use serde::{Deserialize, Serialize};

/// Unique identifier for a workflow node.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: Into<String>> From<T> for NodeId {
    fn from(s: T) -> Self {
        Self(s.into())
    }
}

/// A workflow node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: Option<String>,
    pub retry_count: u32,
}

impl Node {
    pub fn new(id: impl Into<NodeId>, kind: NodeKind) -> Self {
        Self {
            id: id.into(),
            kind,
            label: None,
            retry_count: 0,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_retries(mut self, count: u32) -> Self {
        self.retry_count = count;
        self
    }
}

/// The kind of operation a node performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    /// Execute a WASM sandbox.
    Sandbox {
        module_name: String,
        fuel_limit: u64,
    },
    /// Apply a data transformation.
    Transform {
        transform: TransformFn,
    },
    /// Conditional branching.
    Condition {
        condition: ConditionFn,
    },
    /// Fan out to parallel branches.
    FanOut {
        split_path: String,
    },
    /// Merge parallel branches.
    FanIn {
        merge_strategy: MergeStrategy,
    },
    /// HTTP request node.
    Http {
        method: String,
        url_template: String,
    },
    /// No-op passthrough.
    Passthrough,
}

/// How to transform data between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformFn {
    /// Extract a field using JSON path-like syntax.
    JsonPath(String),
    /// Format using a template string.
    Template(String),
    /// Map key→value rename.
    Rename { from: String, to: String },
    /// Identity (pass through unchanged).
    Identity,
}

/// Condition evaluation functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionFn {
    /// Check if a field equals a value.
    Equals { field: String, value: serde_json::Value },
    /// Check if a field is greater than a number.
    GreaterThan { field: String, value: f64 },
    /// Check if a field exists.
    Exists { field: String },
    /// Always true (for testing).
    Always,
    /// Always false (for testing).
    Never,
}

/// How to merge parallel results in FanIn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeStrategy {
    /// Collect all results into an array.
    Collect,
    /// Take the first completed result.
    First,
    /// Concatenate string results.
    Concat,
}

impl ConditionFn {
    /// Evaluate the condition against a JSON value.
    pub fn evaluate(&self, data: &serde_json::Value) -> bool {
        match self {
            Self::Equals { field, value } => {
                data.get(field).map_or(false, |v| v == value)
            }
            Self::GreaterThan { field, value } => {
                data.get(field)
                    .and_then(|v| v.as_f64())
                    .map_or(false, |v| v > *value)
            }
            Self::Exists { field } => data.get(field).is_some(),
            Self::Always => true,
            Self::Never => false,
        }
    }
}

impl TransformFn {
    /// Apply the transform to a JSON value.
    pub fn apply(&self, data: &serde_json::Value) -> serde_json::Value {
        match self {
            Self::JsonPath(path) => {
                // Simple dot-separated path extraction (e.g., "$.input" → extract "input")
                let key = path.trim_start_matches("$.");
                data.get(key).cloned().unwrap_or(serde_json::Value::Null)
            }
            Self::Template(template) => {
                // Simple {{key}} replacement
                let mut result = template.clone();
                if let Some(obj) = data.as_object() {
                    for (k, v) in obj {
                        let placeholder = format!("{{{{{}}}}}", k);
                        let val_str = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        result = result.replace(&placeholder, &val_str);
                    }
                }
                serde_json::Value::String(result)
            }
            Self::Rename { from, to } => {
                let mut obj = data.as_object().cloned().unwrap_or_default();
                if let Some(val) = obj.remove(from) {
                    obj.insert(to.clone(), val);
                }
                serde_json::Value::Object(obj)
            }
            Self::Identity => data.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_creation() {
        let node = Node::new("test-node", NodeKind::Passthrough)
            .with_label("Test")
            .with_retries(3);
        assert_eq!(node.id.as_str(), "test-node");
        assert_eq!(node.label.as_deref(), Some("Test"));
        assert_eq!(node.retry_count, 3);
    }

    #[test]
    fn test_condition_equals() {
        let cond = ConditionFn::Equals {
            field: "status".into(),
            value: serde_json::json!("ok"),
        };
        assert!(cond.evaluate(&serde_json::json!({"status": "ok"})));
        assert!(!cond.evaluate(&serde_json::json!({"status": "error"})));
    }

    #[test]
    fn test_condition_greater_than() {
        let cond = ConditionFn::GreaterThan {
            field: "count".into(),
            value: 10.0,
        };
        assert!(cond.evaluate(&serde_json::json!({"count": 15})));
        assert!(!cond.evaluate(&serde_json::json!({"count": 5})));
    }

    #[test]
    fn test_condition_exists() {
        let cond = ConditionFn::Exists { field: "name".into() };
        assert!(cond.evaluate(&serde_json::json!({"name": "test"})));
        assert!(!cond.evaluate(&serde_json::json!({"other": "test"})));
    }

    #[test]
    fn test_transform_json_path() {
        let t = TransformFn::JsonPath("$.name".into());
        let result = t.apply(&serde_json::json!({"name": "alice"}));
        assert_eq!(result, serde_json::json!("alice"));
    }

    #[test]
    fn test_transform_template() {
        let t = TransformFn::Template("Hello {{name}}!".into());
        let result = t.apply(&serde_json::json!({"name": "world"}));
        assert_eq!(result, serde_json::json!("Hello world!"));
    }

    #[test]
    fn test_transform_rename() {
        let t = TransformFn::Rename { from: "old".into(), to: "new".into() };
        let result = t.apply(&serde_json::json!({"old": 42}));
        assert_eq!(result, serde_json::json!({"new": 42}));
    }

    #[test]
    fn test_transform_identity() {
        let data = serde_json::json!({"key": "value"});
        let result = TransformFn::Identity.apply(&data);
        assert_eq!(result, data);
    }

    #[test]
    fn test_node_id_display() {
        let id = NodeId::new("my-node");
        assert_eq!(id.to_string(), "my-node");
    }
}
