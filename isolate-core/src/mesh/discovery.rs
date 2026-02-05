//! Node discovery and capability-aware sandbox placement.
//!
//! Provides service discovery for mesh nodes and intelligent placement
//! of sandboxes based on node capabilities, load, and affinity rules.
//!
//! # Discovery Methods
//!
//! - **Static**: Configured list of seed nodes
//! - **DNS**: SRV record-based discovery
//! - **Multicast**: Local network auto-discovery (development)
//!
//! # Placement
//!
//! Sandboxes are placed on nodes based on:
//! - Node capabilities (GPU, high-memory, specific WASI features)
//! - Current load and available resources
//! - Affinity/anti-affinity rules
//! - Geographic proximity

#![allow(dead_code)]

use super::{NodeAddr, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Discovery method for finding mesh nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscoveryMethod {
    /// Static list of known node addresses.
    Static { addresses: Vec<SocketAddr> },
    /// DNS SRV record lookup.
    Dns { service_name: String, domain: String },
    /// Multicast-based local discovery.
    Multicast { group: String, port: u16 },
}

impl Default for DiscoveryMethod {
    fn default() -> Self {
        DiscoveryMethod::Static {
            addresses: Vec::new(),
        }
    }
}

/// Configuration for the discovery service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Discovery method.
    pub method: DiscoveryMethod,
    /// How often to refresh the node list.
    pub refresh_interval: Duration,
    /// Timeout for discovery probes.
    pub probe_timeout: Duration,
    /// Maximum nodes to discover.
    pub max_nodes: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            method: DiscoveryMethod::default(),
            refresh_interval: Duration::from_secs(30),
            probe_timeout: Duration::from_secs(5),
            max_nodes: 1000,
        }
    }
}

/// Capabilities advertised by a node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeCapabilities {
    /// Available WASI features.
    pub wasi_features: HashSet<String>,
    /// Whether this node has GPU access.
    pub gpu: bool,
    /// Maximum memory available (bytes).
    pub max_memory: u64,
    /// Number of CPU cores.
    pub cpu_cores: u32,
    /// Region identifier.
    pub region: Option<String>,
    /// Availability zone.
    pub zone: Option<String>,
    /// Custom labels.
    pub labels: HashMap<String, String>,
}

impl NodeCapabilities {
    /// Check if this node satisfies the given requirements.
    pub fn satisfies(&self, requirements: &PlacementRequirements) -> bool {
        // Check required capabilities
        for cap in &requirements.required_capabilities {
            if !self.wasi_features.contains(cap) {
                return false;
            }
        }

        // Check GPU requirement
        if requirements.requires_gpu && !self.gpu {
            return false;
        }

        // Check memory requirement
        if let Some(min_mem) = requirements.min_memory {
            if self.max_memory < min_mem {
                return false;
            }
        }

        // Check region affinity
        if let Some(ref required_region) = requirements.preferred_region {
            if let Some(ref node_region) = self.region {
                if node_region != required_region {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

/// Requirements for placing a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlacementRequirements {
    /// Required WASI capabilities.
    pub required_capabilities: HashSet<String>,
    /// Whether GPU is required.
    pub requires_gpu: bool,
    /// Minimum memory in bytes.
    pub min_memory: Option<u64>,
    /// Preferred region.
    pub preferred_region: Option<String>,
    /// Node labels that must match.
    pub label_selectors: HashMap<String, String>,
    /// Nodes to avoid (anti-affinity).
    pub excluded_nodes: HashSet<NodeId>,
}

/// A discovered node with its metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    /// Node address.
    pub addr: NodeAddr,
    /// Node capabilities.
    pub capabilities: NodeCapabilities,
    /// Current load (0.0 = idle, 1.0 = fully loaded).
    pub load: f64,
    /// Number of active sandboxes.
    pub active_sandboxes: u32,
    /// When this node was last seen.
    #[serde(skip)]
    pub last_seen: Option<Instant>,
    /// Whether the node is healthy.
    pub healthy: bool,
}

/// Result of a placement decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    /// Selected node.
    pub node_id: NodeId,
    /// Score explaining why this node was chosen.
    pub score: f64,
    /// Reasons for the decision.
    pub reasons: Vec<String>,
    /// Alternative nodes considered.
    pub alternatives: Vec<(NodeId, f64)>,
}

/// The discovery service — maintains a view of available nodes.
pub struct DiscoveryService {
    config: DiscoveryConfig,
    nodes: RwLock<HashMap<NodeId, DiscoveredNode>>,
}

impl DiscoveryService {
    /// Create a new discovery service.
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            config,
            nodes: RwLock::new(HashMap::new()),
        }
    }

    /// Register a node (used by static discovery or gossip propagation).
    pub fn register_node(&self, node: DiscoveredNode) {
        let mut nodes = self.nodes.write().expect("discovery lock poisoned");
        if nodes.len() < self.config.max_nodes {
            nodes.insert(node.addr.id, node);
        }
    }

    /// Remove a node.
    pub fn deregister_node(&self, id: &NodeId) -> bool {
        let mut nodes = self.nodes.write().expect("discovery lock poisoned");
        nodes.remove(id).is_some()
    }

    /// Update a node's load information.
    pub fn update_load(&self, id: &NodeId, load: f64, active_sandboxes: u32) {
        let mut nodes = self.nodes.write().expect("discovery lock poisoned");
        if let Some(node) = nodes.get_mut(id) {
            node.load = load;
            node.active_sandboxes = active_sandboxes;
            node.last_seen = Some(Instant::now());
        }
    }

    /// Mark a node as unhealthy.
    pub fn mark_unhealthy(&self, id: &NodeId) {
        let mut nodes = self.nodes.write().expect("discovery lock poisoned");
        if let Some(node) = nodes.get_mut(id) {
            node.healthy = false;
        }
    }

    /// Get all healthy nodes.
    pub fn healthy_nodes(&self) -> Vec<DiscoveredNode> {
        let nodes = self.nodes.read().expect("discovery lock poisoned");
        nodes.values().filter(|n| n.healthy).cloned().collect()
    }

    /// Get all nodes.
    pub fn all_nodes(&self) -> Vec<DiscoveredNode> {
        let nodes = self.nodes.read().expect("discovery lock poisoned");
        nodes.values().cloned().collect()
    }

    /// Find the best node for a sandbox with given requirements.
    pub fn place_sandbox(&self, requirements: &PlacementRequirements) -> Option<PlacementDecision> {
        let nodes = self.nodes.read().expect("discovery lock poisoned");

        let mut candidates: Vec<(NodeId, f64, Vec<String>)> = nodes
            .values()
            .filter(|n| {
                n.healthy
                    && n.capabilities.satisfies(requirements)
                    && !requirements.excluded_nodes.contains(&n.addr.id)
            })
            .map(|n| {
                let (score, reasons) = score_node(n, requirements);
                (n.addr.id, score, reasons)
            })
            .collect();

        // Sort by score descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        candidates.first().map(|(id, score, reasons)| {
            let alternatives: Vec<(NodeId, f64)> = candidates
                .iter()
                .skip(1)
                .take(3)
                .map(|(id, score, _)| (*id, *score))
                .collect();

            PlacementDecision {
                node_id: *id,
                score: *score,
                reasons: reasons.clone(),
                alternatives,
            }
        })
    }

    /// Find nodes matching a label selector.
    pub fn nodes_with_labels(&self, labels: &HashMap<String, String>) -> Vec<DiscoveredNode> {
        let nodes = self.nodes.read().expect("discovery lock poisoned");
        nodes
            .values()
            .filter(|n| {
                labels.iter().all(|(k, v)| {
                    n.capabilities.labels.get(k).map_or(false, |nv| nv == v)
                })
            })
            .cloned()
            .collect()
    }

    /// Remove stale nodes that haven't been seen within the timeout.
    pub fn evict_stale(&self, timeout: Duration) -> usize {
        let mut nodes = self.nodes.write().expect("discovery lock poisoned");
        let before = nodes.len();
        nodes.retain(|_, n| {
            n.last_seen.map_or(true, |seen| seen.elapsed() < timeout)
        });
        before - nodes.len()
    }

    /// Get the number of tracked nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.read().expect("discovery lock poisoned").len()
    }
}

/// Score a node for placement (higher = better).
fn score_node(node: &DiscoveredNode, requirements: &PlacementRequirements) -> (f64, Vec<String>) {
    let mut score = 100.0;
    let mut reasons = Vec::new();

    // Penalize high load
    let load_penalty = node.load * 50.0;
    score -= load_penalty;
    reasons.push(format!("Load factor: -{:.1} (load={:.2})", load_penalty, node.load));

    // Penalize many active sandboxes
    let sb_penalty = (node.active_sandboxes as f64) * 0.5;
    score -= sb_penalty;
    reasons.push(format!("Sandbox count: -{:.1} (n={})", sb_penalty, node.active_sandboxes));

    // Bonus for matching region
    if let Some(ref pref_region) = requirements.preferred_region {
        if node.capabilities.region.as_deref() == Some(pref_region) {
            score += 20.0;
            reasons.push("Region match: +20.0".to_string());
        }
    }

    // Bonus for extra memory headroom
    if let Some(min_mem) = requirements.min_memory {
        if node.capabilities.max_memory > min_mem * 2 {
            score += 10.0;
            reasons.push("Memory headroom: +10.0".to_string());
        }
    }

    // Bonus for GPU when not required but available
    if !requirements.requires_gpu && node.capabilities.gpu {
        score += 5.0;
        reasons.push("GPU available: +5.0".to_string());
    }

    (score.max(0.0), reasons)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: u64, load: f64, region: Option<&str>) -> DiscoveredNode {
        DiscoveredNode {
            addr: NodeAddr::new(NodeId::new(id), "127.0.0.1:9000".parse().unwrap()),
            capabilities: NodeCapabilities {
                max_memory: 8 * 1024 * 1024 * 1024,
                cpu_cores: 4,
                region: region.map(|s| s.to_string()),
                gpu: false,
                wasi_features: {
                    let mut s = HashSet::new();
                    s.insert("filesystem".to_string());
                    s.insert("sockets".to_string());
                    s
                },
                ..Default::default()
            },
            load,
            active_sandboxes: (load * 10.0) as u32,
            last_seen: Some(Instant::now()),
            healthy: true,
        }
    }

    #[test]
    fn test_register_and_list_nodes() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.2, Some("us-east-1")));
        svc.register_node(make_node(2, 0.5, Some("us-west-2")));

        assert_eq!(svc.node_count(), 2);
        assert_eq!(svc.healthy_nodes().len(), 2);
    }

    #[test]
    fn test_deregister_node() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.2, None));
        assert!(svc.deregister_node(&NodeId::new(1)));
        assert_eq!(svc.node_count(), 0);
    }

    #[test]
    fn test_placement_prefers_lower_load() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.8, None));
        svc.register_node(make_node(2, 0.1, None));
        svc.register_node(make_node(3, 0.5, None));

        let decision = svc.place_sandbox(&PlacementRequirements::default()).unwrap();
        // Node 2 has lowest load, should be selected
        assert_eq!(decision.node_id, NodeId::new(2));
        assert!(decision.score > 0.0);
    }

    #[test]
    fn test_placement_respects_capabilities() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));

        let mut gpu_node = make_node(2, 0.3, None);
        gpu_node.capabilities.gpu = true;

        svc.register_node(gpu_node);

        let mut reqs = PlacementRequirements::default();
        reqs.requires_gpu = true;

        let decision = svc.place_sandbox(&reqs).unwrap();
        assert_eq!(decision.node_id, NodeId::new(2));
    }

    #[test]
    fn test_placement_no_eligible_nodes() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));

        let mut reqs = PlacementRequirements::default();
        reqs.requires_gpu = true;

        assert!(svc.place_sandbox(&reqs).is_none());
    }

    #[test]
    fn test_placement_respects_exclusions() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));
        svc.register_node(make_node(2, 0.2, None));

        let mut reqs = PlacementRequirements::default();
        reqs.excluded_nodes.insert(NodeId::new(1));

        let decision = svc.place_sandbox(&reqs).unwrap();
        assert_eq!(decision.node_id, NodeId::new(2));
    }

    #[test]
    fn test_placement_region_affinity() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, Some("us-east-1")));
        svc.register_node(make_node(2, 0.1, Some("eu-west-1")));

        let mut reqs = PlacementRequirements::default();
        reqs.preferred_region = Some("eu-west-1".to_string());

        let decision = svc.place_sandbox(&reqs).unwrap();
        assert_eq!(decision.node_id, NodeId::new(2));
    }

    #[test]
    fn test_capability_satisfaction() {
        let caps = NodeCapabilities {
            wasi_features: {
                let mut s = HashSet::new();
                s.insert("filesystem".to_string());
                s.insert("sockets".to_string());
                s
            },
            gpu: true,
            max_memory: 16 * 1024 * 1024 * 1024,
            ..Default::default()
        };

        let mut reqs = PlacementRequirements::default();
        reqs.required_capabilities.insert("filesystem".to_string());
        reqs.requires_gpu = true;
        reqs.min_memory = Some(8 * 1024 * 1024 * 1024);
        assert!(caps.satisfies(&reqs));

        reqs.required_capabilities.insert("http".to_string());
        assert!(!caps.satisfies(&reqs));
    }

    #[test]
    fn test_mark_unhealthy() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));

        svc.mark_unhealthy(&NodeId::new(1));
        assert_eq!(svc.healthy_nodes().len(), 0);
        assert_eq!(svc.all_nodes().len(), 1);
    }

    #[test]
    fn test_update_load() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));

        svc.update_load(&NodeId::new(1), 0.9, 50);

        let nodes = svc.all_nodes();
        assert_eq!(nodes[0].load, 0.9);
        assert_eq!(nodes[0].active_sandboxes, 50);
    }

    #[test]
    fn test_evict_stale() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        let mut old_node = make_node(1, 0.1, None);
        old_node.last_seen = Some(Instant::now() - Duration::from_secs(120));
        svc.register_node(old_node);
        svc.register_node(make_node(2, 0.2, None)); // fresh

        let evicted = svc.evict_stale(Duration::from_secs(60));
        assert_eq!(evicted, 1);
        assert_eq!(svc.node_count(), 1);
    }

    #[test]
    fn test_nodes_with_labels() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        let mut node = make_node(1, 0.1, None);
        node.capabilities.labels.insert("tier".to_string(), "premium".to_string());
        svc.register_node(node);
        svc.register_node(make_node(2, 0.2, None));

        let mut labels = HashMap::new();
        labels.insert("tier".to_string(), "premium".to_string());

        let matches = svc.nodes_with_labels(&labels);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_placement_alternatives() {
        let svc = DiscoveryService::new(DiscoveryConfig::default());
        svc.register_node(make_node(1, 0.1, None));
        svc.register_node(make_node(2, 0.3, None));
        svc.register_node(make_node(3, 0.5, None));

        let decision = svc.place_sandbox(&PlacementRequirements::default()).unwrap();
        assert!(!decision.alternatives.is_empty());
        // Alternatives should be ordered by score descending
        assert!(decision.alternatives[0].1 >= decision.alternatives.last().unwrap().1);
    }
}
