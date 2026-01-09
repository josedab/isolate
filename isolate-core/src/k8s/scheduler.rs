//! Resource-aware sandbox scheduler for Kubernetes.
//!
//! This module provides scheduling logic for distributing sandboxes across
//! Kubernetes nodes based on resource availability, affinity rules, and
//! custom constraints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Node resource information for scheduling decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResources {
    /// Node name.
    pub name: String,
    /// Total memory in bytes.
    pub memory_total: u64,
    /// Available memory in bytes.
    pub memory_available: u64,
    /// Total CPU in millicores.
    pub cpu_total: u64,
    /// Available CPU in millicores.
    pub cpu_available: u64,
    /// Number of sandboxes currently running on this node.
    pub sandbox_count: u32,
    /// Maximum sandboxes allowed on this node.
    pub max_sandboxes: u32,
    /// Node labels for affinity matching.
    pub labels: HashMap<String, String>,
    /// Node taints.
    pub taints: Vec<Taint>,
    /// Is the node ready?
    pub ready: bool,
    /// Is the node schedulable?
    pub schedulable: bool,
}

impl NodeResources {
    /// Check if the node can accommodate the requested resources.
    pub fn can_fit(&self, request: &ResourceRequest) -> bool {
        if !self.ready || !self.schedulable {
            return false;
        }

        if self.sandbox_count >= self.max_sandboxes {
            return false;
        }

        if let Some(mem) = request.memory_bytes {
            if mem > self.memory_available {
                return false;
            }
        }

        if let Some(cpu) = request.cpu_millicores {
            if cpu > self.cpu_available {
                return false;
            }
        }

        true
    }

    /// Calculate a score for this node (higher is better).
    pub fn score(&self, request: &ResourceRequest, strategy: &SchedulingStrategy) -> i64 {
        if !self.can_fit(request) {
            return i64::MIN;
        }

        match strategy {
            SchedulingStrategy::LeastLoaded => {
                // Prefer nodes with most available resources
                let mem_score = self.memory_available as i64;
                let cpu_score = self.cpu_available as i64;
                let sandbox_score = (self.max_sandboxes - self.sandbox_count) as i64 * 1000;
                mem_score / 1024 + cpu_score + sandbox_score
            }
            SchedulingStrategy::MostLoaded => {
                // Prefer nodes with least available resources (bin packing)
                let mem_score = -(self.memory_available as i64);
                let cpu_score = -(self.cpu_available as i64);
                let sandbox_score = self.sandbox_count as i64 * 1000;
                mem_score / 1024 + cpu_score + sandbox_score
            }
            SchedulingStrategy::Random => {
                // Use a simple hash for deterministic "random" distribution
                let hash = self.name.bytes().fold(0i64, |acc, b| acc.wrapping_add(b as i64));
                hash
            }
            SchedulingStrategy::RoundRobin { current_index } => {
                // Score based on index position (used externally)
                -(*current_index as i64)
            }
        }
    }

    /// Check if the node matches the given affinity rules.
    pub fn matches_affinity(&self, affinity: &NodeAffinity) -> bool {
        // Check required match expressions
        for expr in &affinity.required_expressions {
            if !self.matches_expression(expr) {
                return false;
            }
        }

        true
    }

    /// Check if the node matches a label expression.
    fn matches_expression(&self, expr: &LabelExpression) -> bool {
        let node_value = self.labels.get(&expr.key);

        match expr.operator {
            LabelOperator::In => {
                node_value.map_or(false, |v| expr.values.contains(v))
            }
            LabelOperator::NotIn => {
                node_value.map_or(true, |v| !expr.values.contains(v))
            }
            LabelOperator::Exists => node_value.is_some(),
            LabelOperator::DoesNotExist => node_value.is_none(),
        }
    }

    /// Check if the node tolerates all required taints.
    pub fn tolerates(&self, tolerations: &[Toleration]) -> bool {
        for taint in &self.taints {
            if !tolerations.iter().any(|t| t.matches(taint)) {
                return false;
            }
        }
        true
    }
}

/// Resource request for scheduling.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceRequest {
    /// Memory in bytes.
    pub memory_bytes: Option<u64>,
    /// CPU in millicores.
    pub cpu_millicores: Option<u64>,
    /// Fuel (for sandbox execution).
    pub fuel: Option<u64>,
}

impl ResourceRequest {
    /// Create a new resource request.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set memory requirement.
    pub fn with_memory(mut self, bytes: u64) -> Self {
        self.memory_bytes = Some(bytes);
        self
    }

    /// Set CPU requirement.
    pub fn with_cpu(mut self, millicores: u64) -> Self {
        self.cpu_millicores = Some(millicores);
        self
    }

    /// Parse memory string (e.g., "128Mi", "1Gi").
    pub fn parse_memory(s: &str) -> Option<u64> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        let (num_str, suffix) = if s.ends_with("Ki") {
            (&s[..s.len() - 2], 1024u64)
        } else if s.ends_with("Mi") {
            (&s[..s.len() - 2], 1024u64 * 1024)
        } else if s.ends_with("Gi") {
            (&s[..s.len() - 2], 1024u64 * 1024 * 1024)
        } else if s.ends_with("Ti") {
            (&s[..s.len() - 2], 1024u64 * 1024 * 1024 * 1024)
        } else if s.ends_with('K') || s.ends_with('k') {
            (&s[..s.len() - 1], 1000u64)
        } else if s.ends_with('M') {
            (&s[..s.len() - 1], 1000u64 * 1000)
        } else if s.ends_with('G') {
            (&s[..s.len() - 1], 1000u64 * 1000 * 1000)
        } else if s.ends_with('T') {
            (&s[..s.len() - 1], 1000u64 * 1000 * 1000 * 1000)
        } else {
            (s, 1u64)
        };

        num_str.parse::<u64>().ok().map(|n| n * suffix)
    }

    /// Parse CPU string (e.g., "100m", "1").
    pub fn parse_cpu(s: &str) -> Option<u64> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        if s.ends_with('m') {
            s[..s.len() - 1].parse().ok()
        } else {
            s.parse::<f64>().ok().map(|n| (n * 1000.0) as u64)
        }
    }
}

/// Scheduling strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SchedulingStrategy {
    /// Prefer nodes with most available resources.
    LeastLoaded,
    /// Prefer nodes with least available resources (bin packing).
    MostLoaded,
    /// Random distribution.
    Random,
    /// Round-robin distribution.
    RoundRobin { current_index: usize },
}

impl Default for SchedulingStrategy {
    fn default() -> Self {
        Self::LeastLoaded
    }
}

/// Node affinity rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeAffinity {
    /// Required label expressions (must all match).
    pub required_expressions: Vec<LabelExpression>,
    /// Preferred label expressions (soft preference).
    pub preferred_expressions: Vec<PreferredExpression>,
}

/// A label selector expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelExpression {
    /// Label key.
    pub key: String,
    /// Operator.
    pub operator: LabelOperator,
    /// Values (for In/NotIn operators).
    pub values: Vec<String>,
}

/// A preferred label expression with weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferredExpression {
    /// The expression.
    pub expression: LabelExpression,
    /// Weight (1-100).
    pub weight: i32,
}

/// Label operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelOperator {
    In,
    NotIn,
    Exists,
    DoesNotExist,
}

/// Node taint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Taint {
    /// Taint key.
    pub key: String,
    /// Taint value.
    pub value: Option<String>,
    /// Taint effect.
    pub effect: TaintEffect,
}

/// Taint effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaintEffect {
    NoSchedule,
    PreferNoSchedule,
    NoExecute,
}

/// Toleration for a taint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Toleration {
    /// Key to match (empty matches all).
    pub key: Option<String>,
    /// Operator (Equal or Exists).
    pub operator: TolerationOperator,
    /// Value to match.
    pub value: Option<String>,
    /// Effect to match (empty matches all).
    pub effect: Option<TaintEffect>,
}

impl Toleration {
    /// Check if this toleration matches a taint.
    pub fn matches(&self, taint: &Taint) -> bool {
        // Check key
        if let Some(key) = &self.key {
            if key != &taint.key {
                return false;
            }
        }

        // Check effect
        if let Some(effect) = &self.effect {
            if effect != &taint.effect {
                return false;
            }
        }

        // Check value based on operator
        match self.operator {
            TolerationOperator::Equal => {
                self.value == taint.value
            }
            TolerationOperator::Exists => true,
        }
    }
}

/// Toleration operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolerationOperator {
    Equal,
    Exists,
}

impl Default for TolerationOperator {
    fn default() -> Self {
        Self::Equal
    }
}

/// Pod anti-affinity for sandbox distribution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PodAntiAffinity {
    /// Required anti-affinity rules.
    pub required: Vec<AntiAffinityTerm>,
    /// Preferred anti-affinity rules.
    pub preferred: Vec<WeightedAntiAffinityTerm>,
}

/// Anti-affinity term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiAffinityTerm {
    /// Label selector for pods to avoid.
    pub label_selector: HashMap<String, String>,
    /// Topology key (e.g., "kubernetes.io/hostname").
    pub topology_key: String,
}

/// Weighted anti-affinity term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedAntiAffinityTerm {
    /// The term.
    pub term: AntiAffinityTerm,
    /// Weight (1-100).
    pub weight: i32,
}

/// The sandbox scheduler.
pub struct SandboxScheduler {
    /// Available nodes.
    nodes: Vec<NodeResources>,
    /// Scheduling strategy.
    strategy: SchedulingStrategy,
    /// Round-robin index (if using round-robin).
    rr_index: usize,
}

impl SandboxScheduler {
    /// Create a new scheduler.
    pub fn new(strategy: SchedulingStrategy) -> Self {
        Self {
            nodes: Vec::new(),
            strategy,
            rr_index: 0,
        }
    }

    /// Update the list of available nodes.
    pub fn update_nodes(&mut self, nodes: Vec<NodeResources>) {
        self.nodes = nodes;
    }

    /// Add a node.
    pub fn add_node(&mut self, node: NodeResources) {
        // Remove existing node with same name
        self.nodes.retain(|n| n.name != node.name);
        self.nodes.push(node);
    }

    /// Remove a node.
    pub fn remove_node(&mut self, name: &str) {
        self.nodes.retain(|n| n.name != name);
    }

    /// Schedule a sandbox to a node.
    pub fn schedule(
        &mut self,
        request: &ResourceRequest,
        affinity: Option<&NodeAffinity>,
        tolerations: &[Toleration],
    ) -> Option<String> {
        let mut candidates: Vec<(&NodeResources, i64)> = self
            .nodes
            .iter()
            .filter(|node| {
                // Filter by affinity
                if let Some(aff) = affinity {
                    if !node.matches_affinity(aff) {
                        return false;
                    }
                }

                // Filter by tolerations
                if !node.tolerates(tolerations) {
                    return false;
                }

                // Filter by resources
                node.can_fit(request)
            })
            .map(|node| {
                let mut score = node.score(request, &self.strategy);

                // Add weight from preferred affinity expressions
                if let Some(aff) = affinity {
                    for pref in &aff.preferred_expressions {
                        if node.matches_expression(&pref.expression) {
                            score += pref.weight as i64 * 100;
                        }
                    }
                }

                (node, score)
            })
            .collect();

        // Sort by score (descending)
        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        // Handle round-robin
        if matches!(self.strategy, SchedulingStrategy::RoundRobin { .. }) {
            if !candidates.is_empty() {
                let idx = self.rr_index % candidates.len();
                self.rr_index = self.rr_index.wrapping_add(1);
                return Some(candidates[idx].0.name.clone());
            }
        }

        // Return the best candidate
        candidates.first().map(|(node, _)| node.name.clone())
    }

    /// Get all nodes.
    pub fn nodes(&self) -> &[NodeResources] {
        &self.nodes
    }

    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get total available capacity.
    pub fn total_capacity(&self) -> (u64, u64, u32) {
        let memory: u64 = self.nodes.iter().map(|n| n.memory_available).sum();
        let cpu: u64 = self.nodes.iter().map(|n| n.cpu_available).sum();
        let sandboxes: u32 = self
            .nodes
            .iter()
            .map(|n| n.max_sandboxes.saturating_sub(n.sandbox_count))
            .sum();
        (memory, cpu, sandboxes)
    }
}

impl Default for SandboxScheduler {
    fn default() -> Self {
        Self::new(SchedulingStrategy::default())
    }
}

/// Scheduling result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulingDecision {
    /// Selected node name.
    pub node_name: String,
    /// Reason for the decision.
    pub reason: String,
    /// Score of the selected node.
    pub score: i64,
    /// Alternative nodes considered.
    pub alternatives: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node(name: &str, mem_avail: u64, cpu_avail: u64) -> NodeResources {
        NodeResources {
            name: name.to_string(),
            memory_total: 16 * 1024 * 1024 * 1024,
            memory_available: mem_avail,
            cpu_total: 8000,
            cpu_available: cpu_avail,
            sandbox_count: 0,
            max_sandboxes: 100,
            labels: HashMap::new(),
            taints: Vec::new(),
            ready: true,
            schedulable: true,
        }
    }

    #[test]
    fn test_node_can_fit() {
        let node = create_test_node("node1", 1024 * 1024 * 1024, 4000);
        let request = ResourceRequest::new()
            .with_memory(512 * 1024 * 1024)
            .with_cpu(2000);

        assert!(node.can_fit(&request));

        let large_request = ResourceRequest::new()
            .with_memory(2 * 1024 * 1024 * 1024)
            .with_cpu(2000);

        assert!(!node.can_fit(&large_request));
    }

    #[test]
    fn test_scheduler_least_loaded() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::LeastLoaded);

        scheduler.add_node(create_test_node("node1", 1024 * 1024 * 1024, 2000));
        scheduler.add_node(create_test_node("node2", 2048 * 1024 * 1024, 4000));
        scheduler.add_node(create_test_node("node3", 512 * 1024 * 1024, 1000));

        let request = ResourceRequest::new()
            .with_memory(256 * 1024 * 1024)
            .with_cpu(500);

        let result = scheduler.schedule(&request, None, &[]);

        // Should pick node2 (most resources available)
        assert_eq!(result, Some("node2".to_string()));
    }

    #[test]
    fn test_scheduler_most_loaded() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::MostLoaded);

        scheduler.add_node(create_test_node("node1", 1024 * 1024 * 1024, 2000));
        scheduler.add_node(create_test_node("node2", 2048 * 1024 * 1024, 4000));
        scheduler.add_node(create_test_node("node3", 512 * 1024 * 1024, 1000));

        let request = ResourceRequest::new()
            .with_memory(256 * 1024 * 1024)
            .with_cpu(500);

        let result = scheduler.schedule(&request, None, &[]);

        // Should pick node3 (least resources available - bin packing)
        assert_eq!(result, Some("node3".to_string()));
    }

    #[test]
    fn test_scheduler_with_affinity() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::LeastLoaded);

        let mut node1 = create_test_node("node1", 1024 * 1024 * 1024, 2000);
        node1.labels.insert("zone".to_string(), "us-east".to_string());

        let mut node2 = create_test_node("node2", 2048 * 1024 * 1024, 4000);
        node2.labels.insert("zone".to_string(), "us-west".to_string());

        scheduler.add_node(node1);
        scheduler.add_node(node2);

        let request = ResourceRequest::new();
        let affinity = NodeAffinity {
            required_expressions: vec![LabelExpression {
                key: "zone".to_string(),
                operator: LabelOperator::In,
                values: vec!["us-east".to_string()],
            }],
            preferred_expressions: vec![],
        };

        let result = scheduler.schedule(&request, Some(&affinity), &[]);

        // Should pick node1 (matches affinity)
        assert_eq!(result, Some("node1".to_string()));
    }

    #[test]
    fn test_scheduler_with_taints() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::LeastLoaded);

        let mut node1 = create_test_node("node1", 1024 * 1024 * 1024, 2000);
        node1.taints.push(Taint {
            key: "dedicated".to_string(),
            value: Some("sandbox".to_string()),
            effect: TaintEffect::NoSchedule,
        });

        let node2 = create_test_node("node2", 512 * 1024 * 1024, 1000);

        scheduler.add_node(node1);
        scheduler.add_node(node2);

        let request = ResourceRequest::new();

        // Without toleration, should pick node2
        let result = scheduler.schedule(&request, None, &[]);
        assert_eq!(result, Some("node2".to_string()));

        // With toleration, should pick node1 (more resources)
        let tolerations = vec![Toleration {
            key: Some("dedicated".to_string()),
            operator: TolerationOperator::Equal,
            value: Some("sandbox".to_string()),
            effect: Some(TaintEffect::NoSchedule),
        }];

        let result = scheduler.schedule(&request, None, &tolerations);
        assert_eq!(result, Some("node1".to_string()));
    }

    #[test]
    fn test_parse_memory() {
        assert_eq!(ResourceRequest::parse_memory("128Mi"), Some(128 * 1024 * 1024));
        assert_eq!(ResourceRequest::parse_memory("1Gi"), Some(1024 * 1024 * 1024));
        assert_eq!(ResourceRequest::parse_memory("500Ki"), Some(500 * 1024));
        assert_eq!(ResourceRequest::parse_memory("1000"), Some(1000));
        assert_eq!(ResourceRequest::parse_memory("1G"), Some(1_000_000_000));
    }

    #[test]
    fn test_parse_cpu() {
        assert_eq!(ResourceRequest::parse_cpu("100m"), Some(100));
        assert_eq!(ResourceRequest::parse_cpu("1"), Some(1000));
        assert_eq!(ResourceRequest::parse_cpu("0.5"), Some(500));
        assert_eq!(ResourceRequest::parse_cpu("2.5"), Some(2500));
    }

    #[test]
    fn test_round_robin() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::RoundRobin { current_index: 0 });

        scheduler.add_node(create_test_node("node1", 1024 * 1024 * 1024, 2000));
        scheduler.add_node(create_test_node("node2", 1024 * 1024 * 1024, 2000));
        scheduler.add_node(create_test_node("node3", 1024 * 1024 * 1024, 2000));

        let request = ResourceRequest::new();

        let r1 = scheduler.schedule(&request, None, &[]);
        let r2 = scheduler.schedule(&request, None, &[]);
        let r3 = scheduler.schedule(&request, None, &[]);
        let r4 = scheduler.schedule(&request, None, &[]);

        // Should cycle through nodes
        assert!(r1.is_some());
        assert!(r2.is_some());
        assert!(r3.is_some());
        assert_eq!(r4, r1); // Back to first
    }

    #[test]
    fn test_total_capacity() {
        let mut scheduler = SandboxScheduler::new(SchedulingStrategy::LeastLoaded);

        scheduler.add_node(create_test_node("node1", 1024, 1000));
        scheduler.add_node(create_test_node("node2", 2048, 2000));

        let (mem, cpu, sandboxes) = scheduler.total_capacity();
        assert_eq!(mem, 3072);
        assert_eq!(cpu, 3000);
        assert_eq!(sandboxes, 200);
    }
}
