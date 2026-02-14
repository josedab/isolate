//! Lightweight distributed sandbox scheduler.
//!
//! Provides the data model and scheduling logic for distributing sandboxes
//! across a cluster of Isolate nodes. This module does NOT implement actual
//! network communication — it provides the scheduler, node registry, and
//! placement logic.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Node types
// ---------------------------------------------------------------------------

/// Describes the static capacity of a node in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapacity {
    /// Maximum number of concurrent sandboxes.
    pub max_sandboxes: u32,
    /// Maximum memory available in bytes.
    pub max_memory_bytes: u64,
    /// Number of CPU cores available.
    pub cpu_cores: u32,
}

/// Current resource usage of a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLoad {
    /// Number of currently active sandboxes.
    pub active_sandboxes: u32,
    /// Memory currently in use (bytes).
    pub memory_used_bytes: u64,
    /// CPU utilization as a fraction in the range `0.0..=1.0`.
    pub cpu_utilization: f64,
}

/// Health status of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Node is operating normally.
    Healthy,
    /// Node is experiencing issues but still accepting work.
    Degraded,
    /// Node is being drained and should not receive new work.
    Draining,
    /// Node is unreachable or shut down.
    Offline,
}

/// Information about a single node in the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Unique identifier of the node.
    pub id: String,
    /// Network address (e.g. `host:port`).
    pub address: String,
    /// Static capacity of the node.
    pub capacity: NodeCapacity,
    /// Current resource usage.
    pub current_load: NodeLoad,
    /// Health status.
    pub status: NodeStatus,
    /// Timestamp of the last heartbeat received from this node.
    pub last_heartbeat: DateTime<Utc>,
    /// Arbitrary key-value labels for filtering / affinity.
    pub labels: HashMap<String, String>,
    /// Optional availability zone.
    pub zone: Option<String>,
}

// ---------------------------------------------------------------------------
// Node registry
// ---------------------------------------------------------------------------

/// Thread-safe registry of cluster nodes.
pub struct NodeRegistry {
    nodes: DashMap<String, NodeInfo>,
}

impl NodeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            nodes: DashMap::new(),
        }
    }

    /// Register (or re-register) a node.
    pub fn register(&self, node: NodeInfo) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Remove a node from the registry. Returns `true` if the node existed.
    pub fn deregister(&self, id: &str) -> bool {
        self.nodes.remove(id).is_some()
    }

    /// Update load metrics and heartbeat timestamp for a node.
    pub fn update_heartbeat(&self, id: &str, load: NodeLoad) {
        if let Some(mut entry) = self.nodes.get_mut(id) {
            entry.current_load = load;
            entry.last_heartbeat = Utc::now();
        }
    }

    /// Retrieve a snapshot of a node's info.
    pub fn get(&self, id: &str) -> Option<NodeInfo> {
        self.nodes.get(id).map(|e| e.value().clone())
    }

    /// Return all nodes that are `Healthy`.
    pub fn healthy_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .filter(|e| e.value().status == NodeStatus::Healthy)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Mark a node as `Draining`.
    pub fn mark_draining(&self, id: &str) {
        if let Some(mut entry) = self.nodes.get_mut(id) {
            entry.status = NodeStatus::Draining;
        }
    }

    /// Mark a node as `Offline`.
    pub fn mark_offline(&self, id: &str) {
        if let Some(mut entry) = self.nodes.get_mut(id) {
            entry.status = NodeStatus::Offline;
        }
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Scheduling types
// ---------------------------------------------------------------------------

/// Strategy used by the scheduler to pick a target node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulingStrategy {
    /// Pick the node with the lowest CPU utilization.
    LeastLoaded,
    /// Pack sandboxes tightly — pick the node with the least remaining capacity.
    BinPacking,
    /// Rotate through healthy nodes in order.
    RoundRobin,
    /// Prefer a requested node, falling back to `LeastLoaded`.
    AffinityBased,
}

/// A request to place a sandbox on a cluster node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRequest {
    /// Hash of the WASM module to schedule.
    pub module_hash: String,
    /// Memory required by the sandbox (bytes).
    pub memory_required: u64,
    /// Labels that a candidate node must match.
    pub labels: HashMap<String, String>,
    /// Preferred availability zone.
    pub preferred_zone: Option<String>,
    /// Preferred node (used by `AffinityBased`).
    pub affinity_node: Option<String>,
}

/// The outcome of a successful placement decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    /// Id of the chosen node.
    pub node_id: String,
    /// Human-readable reason for the decision.
    pub reason: String,
    /// Numeric score (higher is better).
    pub score: f64,
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Distributes sandbox placement across cluster nodes.
pub struct Scheduler {
    strategy: SchedulingStrategy,
    registry: Arc<NodeRegistry>,
    round_robin_counter: AtomicUsize,
}

impl Scheduler {
    /// Create a new scheduler with the given strategy and node registry.
    pub fn new(strategy: SchedulingStrategy, registry: Arc<NodeRegistry>) -> Self {
        Self {
            strategy,
            registry,
            round_robin_counter: AtomicUsize::new(0),
        }
    }

    /// Schedule a single placement request.
    pub fn schedule(&self, request: &PlacementRequest) -> Result<PlacementDecision> {
        let candidates = self.filter_candidates(request);
        if candidates.is_empty() {
            return Err(Error::Orchestrator(
                "No healthy nodes available for scheduling".to_string(),
            ));
        }

        match &self.strategy {
            SchedulingStrategy::LeastLoaded => self.schedule_least_loaded(&candidates),
            SchedulingStrategy::BinPacking => self.schedule_bin_packing(&candidates),
            SchedulingStrategy::RoundRobin => self.schedule_round_robin(&candidates),
            SchedulingStrategy::AffinityBased => {
                self.schedule_affinity(request, &candidates)
            }
        }
    }

    /// Schedule a batch of placement requests, returning one result per request.
    pub fn schedule_batch(
        &self,
        requests: &[PlacementRequest],
    ) -> Vec<Result<PlacementDecision>> {
        requests.iter().map(|r| self.schedule(r)).collect()
    }

    // -- private helpers ----------------------------------------------------

    /// Filter healthy nodes that satisfy the request's resource and label
    /// requirements.
    fn filter_candidates(&self, request: &PlacementRequest) -> Vec<NodeInfo> {
        self.registry
            .healthy_nodes()
            .into_iter()
            .filter(|node| {
                // Check remaining memory
                let remaining_mem =
                    node.capacity.max_memory_bytes.saturating_sub(node.current_load.memory_used_bytes);
                if remaining_mem < request.memory_required {
                    return false;
                }

                // Check sandbox capacity
                if node.current_load.active_sandboxes >= node.capacity.max_sandboxes {
                    return false;
                }

                // Check label requirements
                for (k, v) in &request.labels {
                    match node.labels.get(k) {
                        Some(nv) if nv == v => {}
                        _ => return false,
                    }
                }

                true
            })
            .collect()
    }

    fn schedule_least_loaded(&self, candidates: &[NodeInfo]) -> Result<PlacementDecision> {
        let best = candidates
            .iter()
            .min_by(|a, b| {
                a.current_load
                    .cpu_utilization
                    .partial_cmp(&b.current_load.cpu_utilization)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap(); // candidates is non-empty

        let score = 1.0 - best.current_load.cpu_utilization;
        Ok(PlacementDecision {
            node_id: best.id.clone(),
            reason: format!(
                "Least loaded node (CPU {:.1}%)",
                best.current_load.cpu_utilization * 100.0
            ),
            score,
        })
    }

    fn schedule_bin_packing(&self, candidates: &[NodeInfo]) -> Result<PlacementDecision> {
        // Pick the node with the *highest* utilization (least remaining capacity)
        // so we pack tightly.
        let best = candidates
            .iter()
            .max_by(|a, b| {
                a.current_load
                    .cpu_utilization
                    .partial_cmp(&b.current_load.cpu_utilization)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();

        let score = best.current_load.cpu_utilization;
        Ok(PlacementDecision {
            node_id: best.id.clone(),
            reason: format!(
                "Bin-packed onto node (CPU {:.1}%)",
                best.current_load.cpu_utilization * 100.0
            ),
            score,
        })
    }

    fn schedule_round_robin(&self, candidates: &[NodeInfo]) -> Result<PlacementDecision> {
        let idx = self.round_robin_counter.fetch_add(1, Ordering::Relaxed) % candidates.len();
        let chosen = &candidates[idx];
        Ok(PlacementDecision {
            node_id: chosen.id.clone(),
            reason: format!("Round-robin selection (index {})", idx),
            score: 1.0,
        })
    }

    fn schedule_affinity(
        &self,
        request: &PlacementRequest,
        candidates: &[NodeInfo],
    ) -> Result<PlacementDecision> {
        // If the caller expressed a node preference and it is among the
        // healthy candidates, use it.
        if let Some(ref preferred) = request.affinity_node {
            if let Some(node) = candidates.iter().find(|n| &n.id == preferred) {
                return Ok(PlacementDecision {
                    node_id: node.id.clone(),
                    reason: "Affinity match on preferred node".to_string(),
                    score: 2.0,
                });
            }
        }

        // Fall back to least-loaded.
        self.schedule_least_loaded(candidates)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, cpu: f64, sandboxes: u32, mem_used: u64) -> NodeInfo {
        NodeInfo {
            id: id.to_string(),
            address: format!("{}:8080", id),
            capacity: NodeCapacity {
                max_sandboxes: 100,
                max_memory_bytes: 1024 * 1024 * 1024,
                cpu_cores: 4,
            },
            current_load: NodeLoad {
                active_sandboxes: sandboxes,
                memory_used_bytes: mem_used,
                cpu_utilization: cpu,
            },
            status: NodeStatus::Healthy,
            last_heartbeat: Utc::now(),
            labels: HashMap::new(),
            zone: None,
        }
    }

    fn simple_request() -> PlacementRequest {
        PlacementRequest {
            module_hash: "abc123".to_string(),
            memory_required: 64 * 1024 * 1024,
            labels: HashMap::new(),
            preferred_zone: None,
            affinity_node: None,
        }
    }

    // -- NodeRegistry tests -------------------------------------------------

    #[test]
    fn test_register_and_get() {
        let reg = NodeRegistry::new();
        let node = make_node("n1", 0.5, 10, 512_000_000);
        reg.register(node.clone());

        let fetched = reg.get("n1").unwrap();
        assert_eq!(fetched.id, "n1");
        assert_eq!(fetched.current_load.active_sandboxes, 10);
    }

    #[test]
    fn test_deregister() {
        let reg = NodeRegistry::new();
        reg.register(make_node("n1", 0.1, 1, 0));

        assert!(reg.deregister("n1"));
        assert!(!reg.deregister("n1"));
        assert!(reg.get("n1").is_none());
    }

    #[test]
    fn test_heartbeat_update() {
        let reg = NodeRegistry::new();
        reg.register(make_node("n1", 0.1, 1, 100));

        let new_load = NodeLoad {
            active_sandboxes: 5,
            memory_used_bytes: 500,
            cpu_utilization: 0.75,
        };
        reg.update_heartbeat("n1", new_load);

        let info = reg.get("n1").unwrap();
        assert_eq!(info.current_load.active_sandboxes, 5);
        assert_eq!(info.current_load.memory_used_bytes, 500);
        assert!((info.current_load.cpu_utilization - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_healthy_nodes() {
        let reg = NodeRegistry::new();
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.register(make_node("n2", 0.2, 2, 0));
        reg.mark_offline("n2");

        let healthy = reg.healthy_nodes();
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, "n1");
    }

    #[test]
    fn test_mark_draining() {
        let reg = NodeRegistry::new();
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.mark_draining("n1");

        let info = reg.get("n1").unwrap();
        assert_eq!(info.status, NodeStatus::Draining);
        assert!(reg.healthy_nodes().is_empty());
    }

    #[test]
    fn test_mark_offline() {
        let reg = NodeRegistry::new();
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.mark_offline("n1");

        let info = reg.get("n1").unwrap();
        assert_eq!(info.status, NodeStatus::Offline);
    }

    // -- Scheduling strategy tests ------------------------------------------

    #[test]
    fn test_least_loaded_strategy() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.8, 10, 0));
        reg.register(make_node("n2", 0.2, 5, 0));
        reg.register(make_node("n3", 0.5, 7, 0));

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);
        let decision = sched.schedule(&simple_request()).unwrap();
        assert_eq!(decision.node_id, "n2");
        assert!(decision.score > 0.0);
    }

    #[test]
    fn test_bin_packing_strategy() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.8, 10, 0));
        reg.register(make_node("n2", 0.2, 5, 0));
        reg.register(make_node("n3", 0.5, 7, 0));

        let sched = Scheduler::new(SchedulingStrategy::BinPacking, reg);
        let decision = sched.schedule(&simple_request()).unwrap();
        assert_eq!(decision.node_id, "n1");
    }

    #[test]
    fn test_round_robin_strategy() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("a", 0.5, 5, 0));
        reg.register(make_node("b", 0.5, 5, 0));

        let sched = Scheduler::new(SchedulingStrategy::RoundRobin, reg);

        let mut ids = Vec::new();
        for _ in 0..4 {
            let d = sched.schedule(&simple_request()).unwrap();
            ids.push(d.node_id);
        }

        // Should alternate between the two nodes (order depends on DashMap
        // iteration order, but we should see both).
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
    }

    #[test]
    fn test_affinity_strategy_preferred_node() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.register(make_node("n2", 0.9, 50, 0));

        let sched = Scheduler::new(SchedulingStrategy::AffinityBased, reg);

        let mut req = simple_request();
        req.affinity_node = Some("n2".to_string());

        let decision = sched.schedule(&req).unwrap();
        assert_eq!(decision.node_id, "n2");
        assert!(decision.reason.contains("Affinity"));
    }

    #[test]
    fn test_affinity_fallback_to_least_loaded() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.register(make_node("n2", 0.9, 50, 0));

        let sched = Scheduler::new(SchedulingStrategy::AffinityBased, reg);

        let mut req = simple_request();
        req.affinity_node = Some("missing-node".to_string());

        let decision = sched.schedule(&req).unwrap();
        assert_eq!(decision.node_id, "n1");
    }

    #[test]
    fn test_no_healthy_nodes_error() {
        let reg = Arc::new(NodeRegistry::new());
        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);

        let result = sched.schedule(&simple_request());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No healthy nodes"));
    }

    #[test]
    fn test_draining_node_excluded() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.1, 1, 0));
        reg.mark_draining("n1");

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);
        let result = sched.schedule(&simple_request());
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_scheduling() {
        let reg = Arc::new(NodeRegistry::new());
        reg.register(make_node("n1", 0.3, 5, 0));
        reg.register(make_node("n2", 0.6, 10, 0));

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);

        let requests = vec![simple_request(), simple_request(), simple_request()];
        let results = sched.schedule_batch(&requests);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.is_ok());
        }
    }

    #[test]
    fn test_zone_preference() {
        let reg = Arc::new(NodeRegistry::new());

        let mut n1 = make_node("n1", 0.5, 5, 0);
        n1.zone = Some("us-east-1".to_string());
        let mut n2 = make_node("n2", 0.5, 5, 0);
        n2.zone = Some("eu-west-1".to_string());

        reg.register(n1);
        reg.register(n2);

        // Zone preference is recorded on the request; the registry stores zone
        // info so higher-level orchestration can filter. Both nodes are still
        // healthy candidates — zone-aware filtering can be layered on top.
        let mut req = simple_request();
        req.preferred_zone = Some("us-east-1".to_string());

        // With a populated registry, scheduling succeeds.
        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);
        let decision = sched.schedule(&req).unwrap();
        assert!(!decision.node_id.is_empty());
    }

    #[test]
    fn test_memory_requirement_filtering() {
        let reg = Arc::new(NodeRegistry::new());
        // Node with almost all memory used
        reg.register(make_node("n1", 0.1, 1, 1024 * 1024 * 1024 - 1));
        // Node with plenty of memory
        reg.register(make_node("n2", 0.5, 5, 0));

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);

        let mut req = simple_request();
        req.memory_required = 512 * 1024 * 1024; // 512 MB

        let decision = sched.schedule(&req).unwrap();
        assert_eq!(decision.node_id, "n2");
    }

    #[test]
    fn test_label_filtering() {
        let reg = Arc::new(NodeRegistry::new());

        let mut n1 = make_node("n1", 0.1, 1, 0);
        n1.labels.insert("gpu".to_string(), "true".to_string());
        reg.register(n1);

        let mut n2 = make_node("n2", 0.1, 1, 0);
        n2.labels.insert("gpu".to_string(), "false".to_string());
        reg.register(n2);

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);

        let mut req = simple_request();
        req.labels.insert("gpu".to_string(), "true".to_string());

        let decision = sched.schedule(&req).unwrap();
        assert_eq!(decision.node_id, "n1");
    }

    #[test]
    fn test_sandbox_capacity_filtering() {
        let reg = Arc::new(NodeRegistry::new());
        // Node at sandbox capacity
        reg.register(make_node("n1", 0.1, 100, 0));
        // Node with room
        reg.register(make_node("n2", 0.5, 50, 0));

        let sched = Scheduler::new(SchedulingStrategy::LeastLoaded, reg);
        let decision = sched.schedule(&simple_request()).unwrap();
        assert_eq!(decision.node_id, "n2");
    }
}
