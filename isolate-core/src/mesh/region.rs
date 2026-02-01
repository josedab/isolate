//! Data locality and region-aware routing.
//!
//! Provides multi-region support for sandbox placement, allowing routing
//! decisions to account for geographic proximity and latency constraints.

use super::NodeId;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// A geographic region where nodes can be deployed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    /// Human-readable name of the region.
    pub name: String,
    /// Short region code (e.g. "us-east-1").
    pub code: String,
    /// Latitude of the region.
    pub latitude: f64,
    /// Longitude of the region.
    pub longitude: f64,
}

impl Region {
    /// Create a new region.
    pub fn new(
        name: impl Into<String>,
        code: impl Into<String>,
        latitude: f64,
        longitude: f64,
    ) -> Self {
        Self { name: name.into(), code: code.into(), latitude, longitude }
    }
}

/// Policy for region-aware routing decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionPolicy {
    /// Prefer routing to the local region, but allow cross-region.
    PreferLocal,
    /// Only route within the local region.
    StrictLocal,
    /// Route to whichever region has the lowest latency.
    LowestLatency,
    /// Custom policy identified by name.
    Custom(String),
}

impl Default for RegionPolicy {
    fn default() -> Self {
        RegionPolicy::PreferLocal
    }
}

/// Constraints for data locality when placing a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataLocalityConstraint {
    /// If set, the sandbox must run in this specific region.
    pub required_region: Option<String>,
    /// Preferred regions in order of priority.
    pub preferred_regions: Vec<String>,
    /// Maximum acceptable cross-region latency in milliseconds.
    pub max_cross_region_latency_ms: u64,
}

/// Tracks the topology of regions and inter-region latencies.
pub struct RegionTopology {
    /// Known regions keyed by region code.
    regions: Arc<RwLock<HashMap<String, Region>>>,
    /// Node-to-region mapping.
    node_regions: Arc<RwLock<HashMap<NodeId, String>>>,
    /// Inter-region latencies in ms, keyed by (from, to) region codes.
    latencies: Arc<RwLock<HashMap<(String, String), u64>>>,
}

impl RegionTopology {
    /// Create a new empty region topology.
    pub fn new() -> Self {
        Self {
            regions: Arc::new(RwLock::new(HashMap::new())),
            node_regions: Arc::new(RwLock::new(HashMap::new())),
            latencies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a region.
    pub fn add_region(&self, region: Region) {
        if let Ok(mut regions) = self.regions.write() {
            regions.insert(region.code.clone(), region);
        }
    }

    /// Assign a node to a region.
    pub fn assign_node(&self, node_id: NodeId, region_code: &str) {
        if let Ok(mut mapping) = self.node_regions.write() {
            mapping.insert(node_id, region_code.to_string());
        }
    }

    /// Set the latency between two regions.
    pub fn set_latency(&self, from: &str, to: &str, latency_ms: u64) {
        if let Ok(mut latencies) = self.latencies.write() {
            latencies.insert((from.to_string(), to.to_string()), latency_ms);
            latencies.insert((to.to_string(), from.to_string()), latency_ms);
        }
    }

    /// Estimate latency between two regions.
    pub fn estimate_latency(&self, from: &str, to: &str) -> Duration {
        if from == to {
            return Duration::from_millis(1);
        }
        let latency_ms = self
            .latencies
            .read()
            .ok()
            .and_then(|l| l.get(&(from.to_string(), to.to_string())).copied())
            .unwrap_or(100);
        Duration::from_millis(latency_ms)
    }

    /// Get all nodes in a specific region.
    pub fn nodes_in_region(&self, region: &str) -> Vec<NodeId> {
        self.node_regions
            .read()
            .map(|mapping| {
                mapping.iter().filter(|(_, r)| r.as_str() == region).map(|(&id, _)| id).collect()
            })
            .unwrap_or_default()
    }

    /// Get the region code for a node.
    pub fn node_region(&self, node_id: NodeId) -> Option<String> {
        self.node_regions.read().ok()?.get(&node_id).cloned()
    }

    /// Get all registered regions.
    pub fn all_regions(&self) -> Vec<Region> {
        self.regions.read().map(|r| r.values().cloned().collect()).unwrap_or_default()
    }
}

impl Default for RegionTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Router that wraps routing decisions with region and locality preferences.
pub struct RegionAwareRouter {
    /// Region topology information.
    topology: Arc<RegionTopology>,
    /// The local region code for this node.
    local_region: String,
    /// Region routing policy.
    policy: RegionPolicy,
}

impl RegionAwareRouter {
    /// Create a new region-aware router.
    pub fn new(
        topology: Arc<RegionTopology>,
        local_region: impl Into<String>,
        policy: RegionPolicy,
    ) -> Self {
        Self { topology, local_region: local_region.into(), policy }
    }

    /// Route a sandbox respecting locality constraints.
    ///
    /// Selects a target node based on the region policy and the given constraint.
    pub fn route_with_locality(
        &self,
        sandbox_id: &str,
        constraint: &DataLocalityConstraint,
    ) -> Result<NodeId> {
        // If a specific region is required, only consider nodes in that region.
        if let Some(ref required) = constraint.required_region {
            let nodes = self.topology.nodes_in_region(required);
            return nodes.into_iter().next().ok_or_else(|| {
                Error::Engine(format!(
                    "No nodes available in required region '{}' for sandbox '{}'",
                    required, sandbox_id
                ))
            });
        }

        match self.policy {
            RegionPolicy::StrictLocal => {
                let nodes = self.topology.nodes_in_region(&self.local_region);
                nodes.into_iter().next().ok_or_else(|| {
                    Error::Engine(format!(
                        "No nodes available in local region '{}' for sandbox '{}'",
                        self.local_region, sandbox_id
                    ))
                })
            }
            RegionPolicy::PreferLocal => {
                // Try local region first.
                let local_nodes = self.topology.nodes_in_region(&self.local_region);
                if let Some(node) = local_nodes.into_iter().next() {
                    return Ok(node);
                }
                // Fall back to preferred regions.
                for region in &constraint.preferred_regions {
                    let nodes = self.topology.nodes_in_region(region);
                    if let Some(node) = nodes.into_iter().next() {
                        return Ok(node);
                    }
                }
                // Fall back to any region within latency constraint.
                self.find_any_node_within_latency(constraint)
            }
            RegionPolicy::LowestLatency => self.find_lowest_latency_node(constraint),
            RegionPolicy::Custom(_) => {
                // Custom policies fall back to PreferLocal behavior.
                let local_nodes = self.topology.nodes_in_region(&self.local_region);
                local_nodes.into_iter().next().ok_or_else(|| {
                    Error::Engine(format!(
                        "No nodes available for sandbox '{}' with custom policy",
                        sandbox_id
                    ))
                })
            }
        }
    }

    /// Find the node with the lowest latency from the local region.
    fn find_lowest_latency_node(&self, constraint: &DataLocalityConstraint) -> Result<NodeId> {
        let all_regions = self.topology.all_regions();
        let mut best_node: Option<NodeId> = None;
        let mut best_latency = u64::MAX;

        for region in &all_regions {
            let latency = self.topology.estimate_latency(&self.local_region, &region.code);
            let latency_ms = latency.as_millis() as u64;

            if constraint.max_cross_region_latency_ms > 0
                && latency_ms > constraint.max_cross_region_latency_ms
            {
                continue;
            }

            if latency_ms < best_latency {
                let nodes = self.topology.nodes_in_region(&region.code);
                if let Some(node) = nodes.into_iter().next() {
                    best_latency = latency_ms;
                    best_node = Some(node);
                }
            }
        }

        best_node.ok_or_else(|| {
            Error::Engine("No nodes available within latency constraints".to_string())
        })
    }

    /// Find any node within the latency constraint.
    fn find_any_node_within_latency(&self, constraint: &DataLocalityConstraint) -> Result<NodeId> {
        let all_regions = self.topology.all_regions();

        for region in &all_regions {
            let latency = self.topology.estimate_latency(&self.local_region, &region.code);
            let latency_ms = latency.as_millis() as u64;

            if constraint.max_cross_region_latency_ms > 0
                && latency_ms > constraint.max_cross_region_latency_ms
            {
                continue;
            }

            let nodes = self.topology.nodes_in_region(&region.code);
            if let Some(node) = nodes.into_iter().next() {
                return Ok(node);
            }
        }

        Err(Error::Engine("No nodes available within latency constraints".to_string()))
    }

    /// Get the local region code.
    pub fn local_region(&self) -> &str {
        &self.local_region
    }

    /// Get the routing policy.
    pub fn policy(&self) -> &RegionPolicy {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn setup_topology() -> Arc<RegionTopology> {
        let topo = Arc::new(RegionTopology::new());
        topo.add_region(Region::new("US East", "us-east-1", 39.0, -77.0));
        topo.add_region(Region::new("US West", "us-west-2", 46.0, -120.0));
        topo.add_region(Region::new("EU West", "eu-west-1", 53.0, -6.0));

        topo.assign_node(NodeId::new(1), "us-east-1");
        topo.assign_node(NodeId::new(2), "us-east-1");
        topo.assign_node(NodeId::new(3), "us-west-2");
        topo.assign_node(NodeId::new(4), "eu-west-1");

        topo.set_latency("us-east-1", "us-west-2", 60);
        topo.set_latency("us-east-1", "eu-west-1", 80);
        topo.set_latency("us-west-2", "eu-west-1", 140);

        topo
    }

    #[test]
    fn test_region_creation() {
        let region = Region::new("US East", "us-east-1", 39.0, -77.0);
        assert_eq!(region.code, "us-east-1");
        assert_eq!(region.name, "US East");
    }

    #[test]
    fn test_topology_nodes_in_region() {
        let topo = setup_topology();
        let east_nodes = topo.nodes_in_region("us-east-1");
        assert_eq!(east_nodes.len(), 2);

        let west_nodes = topo.nodes_in_region("us-west-2");
        assert_eq!(west_nodes.len(), 1);
        assert_eq!(west_nodes[0], NodeId::new(3));
    }

    #[test]
    fn test_topology_estimate_latency() {
        let topo = setup_topology();

        // Same region: 1ms.
        let same = topo.estimate_latency("us-east-1", "us-east-1");
        assert_eq!(same, Duration::from_millis(1));

        // Cross-region: configured value.
        let cross = topo.estimate_latency("us-east-1", "us-west-2");
        assert_eq!(cross, Duration::from_millis(60));

        // Unknown pair: default 100ms.
        let unknown = topo.estimate_latency("us-east-1", "ap-southeast-1");
        assert_eq!(unknown, Duration::from_millis(100));
    }

    #[test]
    fn test_prefer_local_routing() {
        let topo = setup_topology();
        let router = RegionAwareRouter::new(topo, "us-east-1", RegionPolicy::PreferLocal);

        let constraint = DataLocalityConstraint::default();
        let node = router.route_with_locality("sb-1", &constraint).unwrap();

        // Should pick a node from us-east-1.
        assert!(node == NodeId::new(1) || node == NodeId::new(2));
    }

    #[test]
    fn test_strict_local_routing() {
        let topo = setup_topology();
        let router = RegionAwareRouter::new(topo, "us-west-2", RegionPolicy::StrictLocal);

        let constraint = DataLocalityConstraint::default();
        let node = router.route_with_locality("sb-1", &constraint).unwrap();
        assert_eq!(node, NodeId::new(3));
    }

    #[test]
    fn test_strict_local_fails_when_no_nodes() {
        let topo = Arc::new(RegionTopology::new());
        topo.add_region(Region::new("AP Southeast", "ap-southeast-1", 1.3, 103.8));

        let router = RegionAwareRouter::new(topo, "ap-southeast-1", RegionPolicy::StrictLocal);

        let constraint = DataLocalityConstraint::default();
        let result = router.route_with_locality("sb-1", &constraint);
        assert!(result.is_err());
    }

    #[test]
    fn test_required_region_constraint() {
        let topo = setup_topology();
        let router = RegionAwareRouter::new(topo, "us-east-1", RegionPolicy::PreferLocal);

        let constraint = DataLocalityConstraint {
            required_region: Some("eu-west-1".to_string()),
            ..Default::default()
        };
        let node = router.route_with_locality("sb-1", &constraint).unwrap();
        assert_eq!(node, NodeId::new(4));
    }

    #[test]
    fn test_lowest_latency_routing() {
        let topo = setup_topology();
        let router = RegionAwareRouter::new(topo, "us-east-1", RegionPolicy::LowestLatency);

        let constraint = DataLocalityConstraint::default();
        let node = router.route_with_locality("sb-1", &constraint).unwrap();

        // From us-east-1: same region (1ms) < us-west-2 (60ms) < eu-west-1 (80ms).
        assert!(node == NodeId::new(1) || node == NodeId::new(2));
    }

    #[test]
    fn test_region_policy_default() {
        assert_eq!(RegionPolicy::default(), RegionPolicy::PreferLocal);
    }

    #[test]
    fn test_data_locality_constraint_default() {
        let c = DataLocalityConstraint::default();
        assert!(c.required_region.is_none());
        assert!(c.preferred_regions.is_empty());
        assert_eq!(c.max_cross_region_latency_ms, 0);
    }
}
