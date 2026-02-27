//! Distributed Sandbox Mesh
//!
//! **WARNING: This module is experimental and not production-ready.**
//! Network operations are currently stubs and the API may change significantly.
//!
//! Provides distributed sandbox execution across multiple nodes with:
//! - Consistent hashing for deterministic placement
//! - Automatic failover and rebalancing
//! - Cross-node sandbox migration
//! - Gossip-based cluster membership
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │                  Mesh Cluster                    │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
//! │  │  Node A  │  │  Node B  │  │  Node C  │       │
//! │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │       │
//! │  │ │ SB 1 │ │  │ │ SB 3 │ │  │ │ SB 5 │ │       │
//! │  │ │ SB 2 │ │  │ │ SB 4 │ │  │ │ SB 6 │ │       │
//! │  │ └──────┘ │  │ └──────┘ │  │ └──────┘ │       │
//! │  └──────────┘  └──────────┘  └──────────┘       │
//! │        ↕             ↕             ↕            │
//! │  ════════════════════════════════════════       │
//! │              Gossip Protocol                    │
//! └──────────────────────────────────────────────────┘
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.

mod cluster;
pub mod consensus;
pub mod discovery;
pub mod distributed;
mod failover;
mod hash;
mod health;
mod member;
mod migration;
mod region;
mod router;
mod scheduler;
pub mod streaming;

pub use cluster::{ClusterConfig, ClusterEvent, MeshCluster};
pub use consensus::{
    PartitionAction, PartitionEvent, RaftCommand, RaftNode, RaftRole, RaftState,
    SplitBrainDetector, StealableTask, VoteRequest, VoteResponse, WorkStealingQueue,
};
pub use discovery::{
    DiscoveredNode, DiscoveryConfig, DiscoveryMethod, DiscoveryService, NodeCapabilities,
    PlacementDecision, PlacementRequirements,
};
pub use failover::{FailoverCoordinator, FailoverEvent, FailoverPolicy, FailoverState};
pub use hash::{ConsistentHash, HashRing, VirtualNode};
pub use health::{HealthChecker, HealthConfig, HealthStatus, NodeHealth};
pub use member::{Gossip, Member, MemberHealth, MemberState};
pub use migration::{MigrationManager, MigrationPlan, MigrationState};
pub use region::{DataLocalityConstraint, Region, RegionAwareRouter, RegionPolicy, RegionTopology};
pub use router::{RoutingDecision, RoutingPolicy, SandboxRouter};
pub use scheduler::{
    NodeCapacity, PlacementConstraint, PlacementStrategy, ResourceRequirements, ScheduledTask,
    SchedulerConfig, SchedulerStats, SchedulingResult, TaskPriority, TaskScheduler,
};

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

/// Unique identifier for a mesh node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u64);

impl NodeId {
    /// Generate a new random node ID.
    pub fn generate() -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Create a node ID from a value.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "node-{:016x}", self.0)
    }
}

/// Network address for a mesh node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeAddr {
    /// Node identifier.
    pub id: NodeId,
    /// Primary socket address.
    pub addr: SocketAddr,
    /// Optional gossip address.
    pub gossip_addr: Option<SocketAddr>,
}

impl NodeAddr {
    /// Create a new node address.
    pub fn new(id: NodeId, addr: SocketAddr) -> Self {
        Self { id, addr, gossip_addr: None }
    }

    /// Set the gossip address.
    pub fn with_gossip(mut self, addr: SocketAddr) -> Self {
        self.gossip_addr = Some(addr);
        self
    }
}

/// Mesh configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// This node's address.
    pub local_addr: SocketAddr,
    /// Seed nodes for joining the cluster.
    pub seed_nodes: Vec<SocketAddr>,
    /// Number of virtual nodes per physical node.
    pub virtual_nodes: usize,
    /// Replication factor for sandboxes.
    pub replication_factor: usize,
    /// Gossip interval.
    pub gossip_interval: Duration,
    /// Failure detection timeout.
    pub failure_timeout: Duration,
    /// Enable automatic migration.
    pub auto_migrate: bool,
    /// Maximum concurrent migrations.
    pub max_concurrent_migrations: usize,
    /// Region this node belongs to.
    pub region: Option<String>,
    /// Region routing policy.
    pub region_policy: Option<RegionPolicy>,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            local_addr: "127.0.0.1:9000".parse().unwrap(),
            seed_nodes: Vec::new(),
            virtual_nodes: 150,
            replication_factor: 2,
            gossip_interval: Duration::from_millis(500),
            failure_timeout: Duration::from_secs(5),
            auto_migrate: true,
            max_concurrent_migrations: 3,
            region: None,
            region_policy: None,
        }
    }
}

/// Statistics about mesh operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeshStats {
    /// Number of active nodes.
    pub active_nodes: usize,
    /// Number of sandboxes managed.
    pub total_sandboxes: usize,
    /// Number of local sandboxes.
    pub local_sandboxes: usize,
    /// Number of pending migrations.
    pub pending_migrations: usize,
    /// Number of completed migrations.
    pub completed_migrations: u64,
    /// Number of failed migrations.
    pub failed_migrations: u64,
    /// Gossip messages sent.
    pub gossip_sent: u64,
    /// Gossip messages received.
    pub gossip_received: u64,
    /// Current rebalance in progress.
    pub rebalancing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_generation() {
        let id1 = NodeId::generate();
        let id2 = NodeId::generate();
        // IDs should be different (high probability)
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_node_id_display() {
        let id = NodeId::new(0x1234567890abcdef);
        assert!(id.to_string().contains("1234567890abcdef"));
    }

    #[test]
    fn test_node_addr() {
        let id = NodeId::new(1);
        let addr: SocketAddr = "192.168.1.1:9000".parse().unwrap();
        let node_addr = NodeAddr::new(id, addr);

        assert_eq!(node_addr.id, id);
        assert_eq!(node_addr.addr, addr);
        assert!(node_addr.gossip_addr.is_none());
    }

    #[test]
    fn test_mesh_config_default() {
        let config = MeshConfig::default();
        assert_eq!(config.virtual_nodes, 150);
        assert_eq!(config.replication_factor, 2);
        assert!(config.auto_migrate);
    }
}
