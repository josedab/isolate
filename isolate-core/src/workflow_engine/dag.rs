//! DAG-based workflow definition and validation.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::nodes::{Node, NodeId};

/// A directed edge between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub label: Option<String>,
}

/// A validated DAG workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    execution_order: Vec<String>,
}

impl Workflow {
    /// Get the topologically sorted execution order.
    pub fn execution_order(&self) -> &[String] {
        &self.execution_order
    }

    /// Get a node by ID.
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Get outgoing edges from a node.
    pub fn outgoing(&self, node_id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.from.as_str() == node_id).collect()
    }

    /// Get incoming edges to a node.
    pub fn incoming(&self, node_id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.to.as_str() == node_id).collect()
    }

    /// Find root nodes (no incoming edges).
    pub fn roots(&self) -> Vec<&str> {
        let targets: HashSet<&str> = self.edges.iter().map(|e| e.to.as_str()).collect();
        self.nodes.keys().filter(|id| !targets.contains(id.as_str())).map(|s| s.as_str()).collect()
    }

    /// Find leaf nodes (no outgoing edges).
    pub fn leaves(&self) -> Vec<&str> {
        let sources: HashSet<&str> = self.edges.iter().map(|e| e.from.as_str()).collect();
        self.nodes.keys().filter(|id| !sources.contains(id.as_str())).map(|s| s.as_str()).collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// Builder for constructing workflows.
pub struct WorkflowBuilder {
    name: String,
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
}

impl WorkflowBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), nodes: HashMap::new(), edges: Vec::new() }
    }

    pub fn add_node(mut self, node: Node) -> Self {
        self.nodes.insert(node.id.as_str().to_string(), node);
        self
    }

    pub fn add_edge(mut self, from: &str, to: &str) -> Result<Self, WorkflowError> {
        if !self.nodes.contains_key(from) {
            return Err(WorkflowError::NodeNotFound(from.to_string()));
        }
        if !self.nodes.contains_key(to) {
            return Err(WorkflowError::NodeNotFound(to.to_string()));
        }
        self.edges.push(Edge { from: NodeId::new(from), to: NodeId::new(to), label: None });
        Ok(self)
    }

    pub fn add_labeled_edge(
        mut self,
        from: &str,
        to: &str,
        label: &str,
    ) -> Result<Self, WorkflowError> {
        if !self.nodes.contains_key(from) {
            return Err(WorkflowError::NodeNotFound(from.to_string()));
        }
        if !self.nodes.contains_key(to) {
            return Err(WorkflowError::NodeNotFound(to.to_string()));
        }
        self.edges.push(Edge {
            from: NodeId::new(from),
            to: NodeId::new(to),
            label: Some(label.to_string()),
        });
        Ok(self)
    }

    /// Build and validate the workflow (checks for cycles).
    pub fn build(self) -> Result<Workflow, WorkflowError> {
        if self.nodes.is_empty() {
            return Err(WorkflowError::EmptyWorkflow);
        }

        let order = self.topological_sort()?;

        Ok(Workflow {
            name: self.name,
            nodes: self.nodes,
            edges: self.edges,
            execution_order: order,
        })
    }

    fn topological_sort(&self) -> Result<Vec<String>, WorkflowError> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        for id in self.nodes.keys() {
            in_degree.insert(id.as_str(), 0);
        }
        for edge in &self.edges {
            *in_degree.entry(edge.to.as_str()).or_insert(0) += 1;
        }

        let mut queue: Vec<&str> =
            in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(&id, _)| id).collect();
        queue.sort(); // deterministic ordering

        let mut result = Vec::new();

        while let Some(node) = queue.pop() {
            result.push(node.to_string());
            for edge in &self.edges {
                if edge.from.as_str() == node {
                    if let Some(deg) = in_degree.get_mut(edge.to.as_str()) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(edge.to.as_str());
                            queue.sort();
                        }
                    }
                }
            }
        }

        if result.len() != self.nodes.len() {
            return Err(WorkflowError::CycleDetected);
        }

        Ok(result)
    }
}

/// Workflow construction errors.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("node not found: {0}")]
    NodeNotFound(String),
    #[error("workflow contains a cycle")]
    CycleDetected,
    #[error("workflow has no nodes")]
    EmptyWorkflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_engine::nodes::{NodeKind, TransformFn};

    #[test]
    fn test_build_linear_workflow() {
        let wf = WorkflowBuilder::new("test")
            .add_node(Node::new("a", NodeKind::Passthrough))
            .add_node(Node::new("b", NodeKind::Passthrough))
            .add_node(Node::new("c", NodeKind::Passthrough))
            .add_edge("a", "b")
            .unwrap()
            .add_edge("b", "c")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(wf.node_count(), 3);
        assert_eq!(wf.edge_count(), 2);
        assert_eq!(wf.roots().len(), 1);
        assert_eq!(wf.leaves().len(), 1);
    }

    #[test]
    fn test_topological_order() {
        let wf = WorkflowBuilder::new("test")
            .add_node(Node::new("c", NodeKind::Passthrough))
            .add_node(Node::new("a", NodeKind::Passthrough))
            .add_node(Node::new("b", NodeKind::Passthrough))
            .add_edge("a", "b")
            .unwrap()
            .add_edge("b", "c")
            .unwrap()
            .build()
            .unwrap();

        let order = wf.execution_order();
        let a_pos = order.iter().position(|n| n == "a").unwrap();
        let b_pos = order.iter().position(|n| n == "b").unwrap();
        let c_pos = order.iter().position(|n| n == "c").unwrap();
        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let result = WorkflowBuilder::new("cyclic")
            .add_node(Node::new("a", NodeKind::Passthrough))
            .add_node(Node::new("b", NodeKind::Passthrough))
            .add_edge("a", "b")
            .unwrap()
            .add_edge("b", "a")
            .unwrap()
            .build();

        assert!(matches!(result, Err(WorkflowError::CycleDetected)));
    }

    #[test]
    fn test_empty_workflow() {
        let result = WorkflowBuilder::new("empty").build();
        assert!(matches!(result, Err(WorkflowError::EmptyWorkflow)));
    }

    #[test]
    fn test_invalid_edge() {
        let result = WorkflowBuilder::new("bad")
            .add_node(Node::new("a", NodeKind::Passthrough))
            .add_edge("a", "nonexistent");
        assert!(matches!(result, Err(WorkflowError::NodeNotFound(_))));
    }

    #[test]
    fn test_diamond_dag() {
        let wf = WorkflowBuilder::new("diamond")
            .add_node(Node::new("start", NodeKind::Passthrough))
            .add_node(Node::new("left", NodeKind::Passthrough))
            .add_node(Node::new("right", NodeKind::Passthrough))
            .add_node(Node::new("end", NodeKind::Passthrough))
            .add_edge("start", "left")
            .unwrap()
            .add_edge("start", "right")
            .unwrap()
            .add_edge("left", "end")
            .unwrap()
            .add_edge("right", "end")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(wf.roots(), vec!["start"]);
        let mut leaves = wf.leaves();
        leaves.sort();
        assert_eq!(leaves, vec!["end"]);
        assert_eq!(wf.incoming("end").len(), 2);
        assert_eq!(wf.outgoing("start").len(), 2);
    }

    #[test]
    fn test_labeled_edge() {
        let wf = WorkflowBuilder::new("labeled")
            .add_node(Node::new("a", NodeKind::Passthrough))
            .add_node(Node::new("b", NodeKind::Passthrough))
            .add_labeled_edge("a", "b", "on_success")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(wf.edges[0].label.as_deref(), Some("on_success"));
    }

    #[test]
    fn test_get_node() {
        let wf = WorkflowBuilder::new("test")
            .add_node(
                Node::new("n1", NodeKind::Transform { transform: TransformFn::Identity })
                    .with_label("Node One"),
            )
            .build()
            .unwrap();

        let node = wf.get_node("n1").unwrap();
        assert_eq!(node.label.as_deref(), Some("Node One"));
        assert!(wf.get_node("nonexistent").is_none());
    }
}
