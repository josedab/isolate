//! Workflow execution engine.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::dag::Workflow;
use super::nodes::{ConditionFn, MergeStrategy, NodeKind, TransformFn};

/// Status of a workflow execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Completed,
    Failed,
    PartialSuccess,
}

/// Output from a single node execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeOutput {
    pub node_id: String,
    pub data: serde_json::Value,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of executing a complete workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub workflow_name: String,
    pub status: ExecutionStatus,
    pub node_outputs: Vec<NodeOutput>,
    pub final_output: serde_json::Value,
    pub nodes_executed: usize,
    pub nodes_skipped: usize,
}

/// Engine that executes workflow DAGs.
pub struct WorkflowExecutor;

impl WorkflowExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Execute a workflow with the given input data.
    pub fn execute(&self, workflow: &Workflow, input: serde_json::Value) -> ExecutionResult {
        let order = workflow.execution_order();
        let mut outputs: HashMap<String, serde_json::Value> = HashMap::new();
        let mut node_outputs = Vec::new();
        let mut nodes_executed = 0usize;
        let mut nodes_skipped = 0usize;
        let mut has_failure = false;

        for node_id in order {
            let node = match workflow.get_node(node_id) {
                Some(n) => n,
                None => continue,
            };

            // Gather input: merge outputs from all predecessor nodes
            let incoming = workflow.incoming(node_id);
            let node_input = if incoming.is_empty() {
                input.clone()
            } else {
                self.merge_inputs(&incoming, &outputs)
            };

            // Execute the node
            let result = self.execute_node(node_id, &node.kind, &node_input);

            if result.success {
                outputs.insert(node_id.clone(), result.data.clone());
                nodes_executed += 1;
            } else {
                has_failure = true;
                // Try retries
                let mut succeeded = false;
                for _ in 0..node.retry_count {
                    let retry = self.execute_node(node_id, &node.kind, &node_input);
                    if retry.success {
                        outputs.insert(node_id.clone(), retry.data.clone());
                        nodes_executed += 1;
                        node_outputs.push(retry);
                        succeeded = true;
                        break;
                    }
                }
                if !succeeded {
                    nodes_skipped += 1;
                    outputs.insert(node_id.clone(), serde_json::Value::Null);
                }
            }

            node_outputs.push(result);
        }

        // Final output is from the last executed node
        let final_output = order.last()
            .and_then(|id| outputs.get(id))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let status = if has_failure && nodes_executed == 0 {
            ExecutionStatus::Failed
        } else if has_failure {
            ExecutionStatus::PartialSuccess
        } else {
            ExecutionStatus::Completed
        };

        ExecutionResult {
            workflow_name: workflow.name.clone(),
            status,
            node_outputs,
            final_output,
            nodes_executed,
            nodes_skipped,
        }
    }

    fn merge_inputs(
        &self,
        incoming: &[&super::dag::Edge],
        outputs: &HashMap<String, serde_json::Value>,
    ) -> serde_json::Value {
        if incoming.len() == 1 {
            return outputs.get(incoming[0].from.as_str())
                .cloned()
                .unwrap_or(serde_json::Value::Null);
        }

        // Multiple inputs → merge into object with source node IDs as keys
        let mut merged = serde_json::Map::new();
        for edge in incoming {
            if let Some(val) = outputs.get(edge.from.as_str()) {
                merged.insert(edge.from.as_str().to_string(), val.clone());
            }
        }
        serde_json::Value::Object(merged)
    }

    pub(crate) fn execute_node(
        &self,
        node_id: &str,
        kind: &NodeKind,
        input: &serde_json::Value,
    ) -> NodeOutput {
        match kind {
            NodeKind::Transform { transform } => {
                let data = transform.apply(input);
                NodeOutput {
                    node_id: node_id.to_string(),
                    data,
                    success: true,
                    error: None,
                }
            }
            NodeKind::Condition { condition } => {
                let passed = condition.evaluate(input);
                NodeOutput {
                    node_id: node_id.to_string(),
                    data: serde_json::json!({"passed": passed, "input": input}),
                    success: true,
                    error: None,
                }
            }
            NodeKind::Passthrough => {
                NodeOutput {
                    node_id: node_id.to_string(),
                    data: input.clone(),
                    success: true,
                    error: None,
                }
            }
            NodeKind::FanOut { split_path } => {
                let items = input.get(split_path.as_str())
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(vec![]));
                NodeOutput {
                    node_id: node_id.to_string(),
                    data: items,
                    success: true,
                    error: None,
                }
            }
            NodeKind::FanIn { merge_strategy } => {
                let data = match merge_strategy {
                    MergeStrategy::Collect => {
                        if let Some(obj) = input.as_object() {
                            serde_json::Value::Array(obj.values().cloned().collect())
                        } else {
                            serde_json::Value::Array(vec![input.clone()])
                        }
                    }
                    MergeStrategy::First => {
                        if let Some(obj) = input.as_object() {
                            obj.values().next().cloned().unwrap_or(serde_json::Value::Null)
                        } else {
                            input.clone()
                        }
                    }
                    MergeStrategy::Concat => {
                        if let Some(obj) = input.as_object() {
                            let s: String = obj.values()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join("");
                            serde_json::Value::String(s)
                        } else {
                            input.clone()
                        }
                    }
                };
                NodeOutput {
                    node_id: node_id.to_string(),
                    data,
                    success: true,
                    error: None,
                }
            }
            NodeKind::Sandbox { module_name, .. } => {
                // Simulate sandbox execution (actual WASM execution requires runtime)
                NodeOutput {
                    node_id: node_id.to_string(),
                    data: serde_json::json!({
                        "module": module_name,
                        "input": input,
                        "status": "simulated"
                    }),
                    success: true,
                    error: None,
                }
            }
            NodeKind::Http { method, url_template } => {
                // Simulate HTTP request
                NodeOutput {
                    node_id: node_id.to_string(),
                    data: serde_json::json!({
                        "method": method,
                        "url": url_template,
                        "status": "simulated"
                    }),
                    success: true,
                    error: None,
                }
            }
        }
    }
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_engine::dag::WorkflowBuilder;
    use crate::workflow_engine::nodes::*;

    #[test]
    fn test_execute_passthrough() {
        let wf = WorkflowBuilder::new("pass")
            .add_node(Node::new("n1", NodeKind::Passthrough))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"key": "value"}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.final_output, serde_json::json!({"key": "value"}));
    }

    #[test]
    fn test_execute_transform_chain() {
        let wf = WorkflowBuilder::new("chain")
            .add_node(Node::new("extract", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.name".into()),
            }))
            .add_node(Node::new("format", NodeKind::Transform {
                transform: TransformFn::Template("Hello {{value}}!".into()),
            }))
            .add_edge("extract", "format").unwrap()
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"name": "world"}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.nodes_executed, 2);
    }

    #[test]
    fn test_execute_condition() {
        let wf = WorkflowBuilder::new("cond")
            .add_node(Node::new("check", NodeKind::Condition {
                condition: ConditionFn::Equals {
                    field: "status".into(),
                    value: serde_json::json!("ok"),
                },
            }))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"status": "ok"}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.final_output["passed"], true);
    }

    #[test]
    fn test_execute_fan_out() {
        let wf = WorkflowBuilder::new("fan")
            .add_node(Node::new("split", NodeKind::FanOut {
                split_path: "items".into(),
            }))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"items": [1, 2, 3]}));
        assert_eq!(result.final_output, serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_execute_sandbox_node() {
        let wf = WorkflowBuilder::new("sandbox")
            .add_node(Node::new("run", NodeKind::Sandbox {
                module_name: "test.wasm".into(),
                fuel_limit: 100000,
            }))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"data": "test"}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.final_output["module"], "test.wasm");
    }

    #[test]
    fn test_execute_http_node() {
        let wf = WorkflowBuilder::new("http")
            .add_node(Node::new("fetch", NodeKind::Http {
                method: "GET".into(),
                url_template: "https://api.example.com/data".into(),
            }))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.final_output["method"], "GET");
    }

    #[test]
    fn test_diamond_execution() {
        let wf = WorkflowBuilder::new("diamond")
            .add_node(Node::new("start", NodeKind::Passthrough))
            .add_node(Node::new("left", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.a".into()),
            }))
            .add_node(Node::new("right", NodeKind::Transform {
                transform: TransformFn::JsonPath("$.b".into()),
            }))
            .add_node(Node::new("merge", NodeKind::FanIn {
                merge_strategy: MergeStrategy::Collect,
            }))
            .add_edge("start", "left").unwrap()
            .add_edge("start", "right").unwrap()
            .add_edge("left", "merge").unwrap()
            .add_edge("right", "merge").unwrap()
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!({"a": 1, "b": 2}));
        assert_eq!(result.status, ExecutionStatus::Completed);
        assert_eq!(result.nodes_executed, 4);
    }

    #[test]
    fn test_execution_result_metadata() {
        let wf = WorkflowBuilder::new("meta-test")
            .add_node(Node::new("n1", NodeKind::Passthrough))
            .build().unwrap();

        let exec = WorkflowExecutor::new();
        let result = exec.execute(&wf, serde_json::json!(null));
        assert_eq!(result.workflow_name, "meta-test");
        assert_eq!(result.nodes_executed, 1);
        assert_eq!(result.nodes_skipped, 0);
    }
}
