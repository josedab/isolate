//! Low-Code Sandbox Orchestrator.
//!
//! Visual workflow engine for chaining sandbox executions with
//! data transformations, conditions, and fan-out/fan-in patterns.
//!
//! # Features
//!
//! - **DAG Workflows**: Define directed acyclic graphs of sandbox executions
//! - **Node Types**: Sandbox, Transform, Condition, FanOut, FanIn, HTTP
//! - **Data Flow**: Typed data passing between nodes with transformations
//! - **Execution Engine**: Topological execution with error handling

#![allow(missing_docs)]
pub mod dag;
pub mod executor;
pub mod metrics;
pub mod nodes;

pub use dag::{Edge, Workflow, WorkflowBuilder, WorkflowError};
pub use executor::{ExecutionResult, ExecutionStatus, NodeOutput, WorkflowExecutor};
pub use metrics::{MetricsAwareExecutor, MetricsCollector, PipelineMetrics, StageMetrics};
pub use nodes::{ConditionFn, Node, NodeId, NodeKind, TransformFn};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_workflow() {
        let wf = WorkflowBuilder::new("linear-test")
            .add_node(Node::new(
                "start",
                NodeKind::Transform { transform: TransformFn::JsonPath("$.input".into()) },
            ))
            .add_node(Node::new(
                "end",
                NodeKind::Transform {
                    transform: TransformFn::Template("result: {{value}}".into()),
                },
            ))
            .add_edge("start", "end")
            .unwrap()
            .build()
            .unwrap();

        let executor = WorkflowExecutor::new();
        let result = executor.execute(&wf, serde_json::json!({"input": "hello"}));
        assert_eq!(result.status, ExecutionStatus::Completed);
    }
}
