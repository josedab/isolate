//! Sandbox routing decisions.

use super::{hash::HashRing, member::Member, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Policy for routing sandbox requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingPolicy {
    /// Route to the hash ring owner.
    HashBased,
    /// Route to least loaded node.
    LeastLoaded,
    /// Route to lowest latency node.
    LowestLatency,
    /// Route randomly.
    Random,
    /// Route locally if possible.
    LocalFirst,
    /// Weighted round-robin.
    WeightedRoundRobin,
}

impl Default for RoutingPolicy {
    fn default() -> Self {
        RoutingPolicy::HashBased
    }
}

/// A routing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Primary node to route to.
    pub primary: NodeId,
    /// Fallback nodes.
    pub fallbacks: Vec<NodeId>,
    /// Whether to replicate.
    pub replicate: bool,
    /// Routing policy used.
    pub policy: RoutingPolicy,
    /// Confidence in the decision.
    pub confidence: f64,
}

impl RoutingDecision {
    /// Create a new routing decision.
    pub fn new(primary: NodeId, policy: RoutingPolicy) -> Self {
        Self { primary, fallbacks: Vec::new(), replicate: false, policy, confidence: 1.0 }
    }

    /// Add fallback nodes.
    pub fn with_fallbacks(mut self, fallbacks: Vec<NodeId>) -> Self {
        self.fallbacks = fallbacks;
        self
    }

    /// Set replication.
    pub fn with_replication(mut self, replicate: bool) -> Self {
        self.replicate = replicate;
        self
    }

    /// Get all target nodes.
    pub fn targets(&self) -> Vec<NodeId> {
        let mut targets = vec![self.primary];
        targets.extend(&self.fallbacks);
        targets
    }
}

/// Node weights for routing.
#[derive(Debug, Clone, Default)]
struct NodeWeight {
    /// Base weight.
    weight: f64,
    /// Load factor (0.0-1.0, lower is better).
    load: f64,
    /// Latency in ms.
    latency_ms: u64,
    /// Success rate (0.0-1.0).
    success_rate: f64,
}

impl NodeWeight {
    /// Calculate effective weight.
    fn effective_weight(&self) -> f64 {
        let load_factor = 1.0 - self.load;
        let latency_factor = 1.0 / (1.0 + self.latency_ms as f64 / 100.0);
        self.weight * load_factor * latency_factor * self.success_rate
    }
}

/// Routes sandbox requests to appropriate nodes.
pub struct SandboxRouter {
    /// Replication factor.
    replication_factor: usize,
    /// Default routing policy.
    default_policy: RoutingPolicy,
    /// Hash ring for consistent hashing.
    hash_ring: Arc<RwLock<HashRing>>,
    /// Node weights.
    weights: Arc<RwLock<HashMap<NodeId, NodeWeight>>>,
    /// Local node ID.
    local_node: Option<NodeId>,
    /// Round-robin counter.
    rr_counter: std::sync::atomic::AtomicUsize,
}

impl SandboxRouter {
    /// Create a new sandbox router.
    pub fn new(replication_factor: usize) -> Self {
        Self {
            replication_factor,
            default_policy: RoutingPolicy::HashBased,
            hash_ring: Arc::new(RwLock::new(HashRing::new(150))),
            weights: Arc::new(RwLock::new(HashMap::new())),
            local_node: None,
            rr_counter: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Set the local node ID.
    pub fn set_local_node(&mut self, node_id: NodeId) {
        self.local_node = Some(node_id);
    }

    /// Set the default routing policy.
    pub fn set_default_policy(&mut self, policy: RoutingPolicy) {
        self.default_policy = policy;
    }

    /// Add a node to the router.
    pub fn add_node(&self, node_id: NodeId) {
        if let Ok(mut ring) = self.hash_ring.write() {
            ring.add_node(node_id);
        }
        if let Ok(mut weights) = self.weights.write() {
            weights.insert(
                node_id,
                NodeWeight { weight: 1.0, load: 0.0, latency_ms: 0, success_rate: 1.0 },
            );
        }
    }

    /// Remove a node from the router.
    pub fn remove_node(&self, node_id: NodeId) {
        if let Ok(mut ring) = self.hash_ring.write() {
            ring.remove_node(node_id);
        }
        if let Ok(mut weights) = self.weights.write() {
            weights.remove(&node_id);
        }
    }

    /// Update node weight.
    pub fn update_weight(&self, node_id: NodeId, weight: f64) {
        if let Ok(mut weights) = self.weights.write() {
            if let Some(w) = weights.get_mut(&node_id) {
                w.weight = weight;
            }
        }
    }

    /// Update node load.
    pub fn update_load(&self, node_id: NodeId, load: f64) {
        if let Ok(mut weights) = self.weights.write() {
            if let Some(w) = weights.get_mut(&node_id) {
                w.load = load;
            }
        }
    }

    /// Update node latency.
    pub fn update_latency(&self, node_id: NodeId, latency_ms: u64) {
        if let Ok(mut weights) = self.weights.write() {
            if let Some(w) = weights.get_mut(&node_id) {
                w.latency_ms = latency_ms;
            }
        }
    }

    /// Update node metrics from member info.
    pub fn update_from_member(&self, member: &Member) {
        if let Ok(mut weights) = self.weights.write() {
            if let Some(w) = weights.get_mut(&member.id()) {
                w.load = member.health.load_factor();
                w.latency_ms = member.health.rtt_ms;
                w.success_rate = if member.state.is_active() { 1.0 } else { 0.0 };
            }
        }
    }

    /// Route a sandbox request.
    pub fn route(&self, sandbox_id: &str) -> Option<RoutingDecision> {
        self.route_with_policy(sandbox_id, self.default_policy)
    }

    /// Route with a specific policy.
    pub fn route_with_policy(
        &self,
        sandbox_id: &str,
        policy: RoutingPolicy,
    ) -> Option<RoutingDecision> {
        match policy {
            RoutingPolicy::HashBased => self.route_hash_based(sandbox_id),
            RoutingPolicy::LeastLoaded => self.route_least_loaded(),
            RoutingPolicy::LowestLatency => self.route_lowest_latency(),
            RoutingPolicy::Random => self.route_random(),
            RoutingPolicy::LocalFirst => self.route_local_first(sandbox_id),
            RoutingPolicy::WeightedRoundRobin => self.route_weighted_rr(),
        }
    }

    /// Hash-based routing.
    fn route_hash_based(&self, sandbox_id: &str) -> Option<RoutingDecision> {
        let ring = self.hash_ring.read().ok()?;
        let nodes = ring.get_nodes(sandbox_id, self.replication_factor);

        if nodes.is_empty() {
            return None;
        }

        let primary = nodes[0];
        let fallbacks = nodes[1..].to_vec();

        Some(
            RoutingDecision::new(primary, RoutingPolicy::HashBased)
                .with_fallbacks(fallbacks)
                .with_replication(self.replication_factor > 1),
        )
    }

    /// Route to least loaded node.
    fn route_least_loaded(&self) -> Option<RoutingDecision> {
        let weights = self.weights.read().ok()?;

        let (&node_id, _) = weights.iter().min_by(|(_, a), (_, b)| {
            a.load.partial_cmp(&b.load).unwrap_or(std::cmp::Ordering::Equal)
        })?;

        let fallbacks: Vec<NodeId> = weights
            .keys()
            .filter(|&&id| id != node_id)
            .take(self.replication_factor - 1)
            .copied()
            .collect();

        Some(RoutingDecision::new(node_id, RoutingPolicy::LeastLoaded).with_fallbacks(fallbacks))
    }

    /// Route to lowest latency node.
    fn route_lowest_latency(&self) -> Option<RoutingDecision> {
        let weights = self.weights.read().ok()?;

        let (&node_id, _) = weights.iter().min_by_key(|(_, w)| w.latency_ms)?;

        Some(RoutingDecision::new(node_id, RoutingPolicy::LowestLatency))
    }

    /// Random routing.
    fn route_random(&self) -> Option<RoutingDecision> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let weights = self.weights.read().ok()?;
        let nodes: Vec<_> = weights.keys().copied().collect();

        if nodes.is_empty() {
            return None;
        }

        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        let idx = hasher.finish() as usize % nodes.len();

        Some(RoutingDecision::new(nodes[idx], RoutingPolicy::Random))
    }

    /// Local-first routing.
    fn route_local_first(&self, sandbox_id: &str) -> Option<RoutingDecision> {
        if let Some(local) = self.local_node {
            let ring = self.hash_ring.read().ok()?;

            // Check if local is in the hash ring for this sandbox
            let nodes = ring.get_nodes(sandbox_id, self.replication_factor);
            if nodes.contains(&local) {
                let fallbacks: Vec<_> = nodes.into_iter().filter(|&n| n != local).collect();
                return Some(
                    RoutingDecision::new(local, RoutingPolicy::LocalFirst)
                        .with_fallbacks(fallbacks),
                );
            }
        }

        // Fall back to hash-based
        self.route_hash_based(sandbox_id)
    }

    /// Weighted round-robin routing.
    fn route_weighted_rr(&self) -> Option<RoutingDecision> {
        let weights = self.weights.read().ok()?;
        let nodes: Vec<_> = weights.iter().map(|(&id, w)| (id, w.effective_weight())).collect();

        if nodes.is_empty() {
            return None;
        }

        // Simple round-robin (ignoring weights for simplicity)
        let counter = self.rr_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let idx = counter % nodes.len();

        Some(RoutingDecision::new(nodes[idx].0, RoutingPolicy::WeightedRoundRobin))
    }

    /// Get all known nodes.
    pub fn nodes(&self) -> Vec<NodeId> {
        self.weights.read().map(|w| w.keys().copied().collect()).unwrap_or_default()
    }

    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.weights.read().map(|w| w.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routing_decision() {
        let decision = RoutingDecision::new(NodeId::new(1), RoutingPolicy::HashBased)
            .with_fallbacks(vec![NodeId::new(2), NodeId::new(3)])
            .with_replication(true);

        assert_eq!(decision.primary, NodeId::new(1));
        assert_eq!(decision.fallbacks.len(), 2);
        assert!(decision.replicate);
        assert_eq!(decision.targets().len(), 3);
    }

    #[test]
    fn test_router_add_remove_node() {
        let router = SandboxRouter::new(2);

        router.add_node(NodeId::new(1));
        router.add_node(NodeId::new(2));
        assert_eq!(router.node_count(), 2);

        router.remove_node(NodeId::new(1));
        assert_eq!(router.node_count(), 1);
    }

    #[test]
    fn test_router_hash_based() {
        let router = SandboxRouter::new(2);
        router.add_node(NodeId::new(1));
        router.add_node(NodeId::new(2));
        router.add_node(NodeId::new(3));

        let decision = router.route("sandbox-123");
        assert!(decision.is_some());

        let d = decision.unwrap();
        assert_eq!(d.policy, RoutingPolicy::HashBased);
    }

    #[test]
    fn test_router_least_loaded() {
        let router = SandboxRouter::new(1);
        router.add_node(NodeId::new(1));
        router.add_node(NodeId::new(2));

        router.update_load(NodeId::new(1), 0.8);
        router.update_load(NodeId::new(2), 0.2);

        let decision = router.route_with_policy("any", RoutingPolicy::LeastLoaded);
        assert!(decision.is_some());
        assert_eq!(decision.unwrap().primary, NodeId::new(2));
    }

    #[test]
    fn test_router_local_first() {
        let mut router = SandboxRouter::new(2);
        router.add_node(NodeId::new(1));
        router.add_node(NodeId::new(2));
        router.set_local_node(NodeId::new(1));

        let decision = router.route_with_policy("sandbox-123", RoutingPolicy::LocalFirst);
        assert!(decision.is_some());
        // Should prefer local node when it's in the hash ring
    }

    #[test]
    fn test_router_empty() {
        let router = SandboxRouter::new(1);
        let decision = router.route("sandbox-123");
        assert!(decision.is_none());
    }
}
