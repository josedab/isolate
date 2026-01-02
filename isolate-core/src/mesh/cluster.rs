//! Mesh cluster management.

use super::{
    hash::HashRing,
    member::{Gossip, Member, MemberState},
    migration::MigrationManager,
    router::SandboxRouter,
    MeshConfig, MeshStats, NodeAddr, NodeId,
};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Configuration for a mesh cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Mesh configuration.
    pub mesh: MeshConfig,
    /// Cluster name.
    pub name: String,
    /// Cluster version.
    pub version: String,
    /// Enable TLS.
    pub tls_enabled: bool,
    /// Authentication token.
    pub auth_token: Option<String>,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            mesh: MeshConfig::default(),
            name: "isolate-mesh".to_string(),
            version: "1.0.0".to_string(),
            tls_enabled: false,
            auth_token: None,
        }
    }
}

/// Events emitted by the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterEvent {
    /// This node joined the cluster.
    Joined {
        /// Number of existing members.
        members: usize,
    },
    /// A new member joined.
    MemberJoined {
        /// Member ID.
        member_id: NodeId,
    },
    /// A member left.
    MemberLeft {
        /// Member ID.
        member_id: NodeId,
    },
    /// A member failed.
    MemberFailed {
        /// Member ID.
        member_id: NodeId,
    },
    /// Cluster is rebalancing.
    Rebalancing {
        /// Number of sandboxes to move.
        sandboxes_moving: usize,
    },
    /// Rebalancing completed.
    RebalanceComplete {
        /// Number of sandboxes moved.
        sandboxes_moved: usize,
    },
    /// Leadership changed.
    LeaderChanged {
        /// New leader.
        new_leader: Option<NodeId>,
    },
}

/// A distributed mesh cluster.
pub struct MeshCluster {
    /// Configuration.
    config: ClusterConfig,
    /// This node's address.
    local_node: NodeAddr,
    /// Hash ring for sandbox distribution.
    hash_ring: Arc<RwLock<HashRing>>,
    /// Gossip protocol handler.
    gossip: Arc<Gossip>,
    /// Sandbox router.
    router: Arc<RwLock<SandboxRouter>>,
    /// Migration manager.
    migration_manager: Arc<RwLock<MigrationManager>>,
    /// Cluster state.
    state: Arc<RwLock<ClusterState>>,
    /// Event subscribers.
    event_handlers: Arc<RwLock<Vec<Box<dyn Fn(ClusterEvent) + Send + Sync>>>>,
}

/// Internal cluster state.
#[derive(Debug, Clone, Default)]
struct ClusterState {
    /// Is this node the leader.
    is_leader: bool,
    /// Current leader node.
    leader: Option<NodeId>,
    /// Is cluster healthy.
    is_healthy: bool,
    /// Statistics.
    stats: MeshStats,
}

impl MeshCluster {
    /// Create a new mesh cluster.
    pub fn new(config: ClusterConfig) -> Result<Self> {
        let local_id = NodeId::generate();
        let local_node = NodeAddr::new(local_id, config.mesh.local_addr);

        let hash_ring = HashRing::new(config.mesh.virtual_nodes);
        let gossip = Gossip::new(local_id, config.mesh.gossip_interval);
        let router = SandboxRouter::new(config.mesh.replication_factor);
        let migration_manager = MigrationManager::new(config.mesh.max_concurrent_migrations);

        Ok(Self {
            config,
            local_node,
            hash_ring: Arc::new(RwLock::new(hash_ring)),
            gossip: Arc::new(gossip),
            router: Arc::new(RwLock::new(router)),
            migration_manager: Arc::new(RwLock::new(migration_manager)),
            state: Arc::new(RwLock::new(ClusterState::default())),
            event_handlers: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Get this node's ID.
    pub fn local_id(&self) -> NodeId {
        self.local_node.id
    }

    /// Get this node's address.
    pub fn local_addr(&self) -> &NodeAddr {
        &self.local_node
    }

    /// Join the cluster.
    pub async fn join(&self) -> Result<()> {
        // Add self to hash ring
        {
            let mut ring = self
                .hash_ring
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            ring.add_node(self.local_node.id);
        }

        // Create local member
        let mut local_member = Member::new(self.local_node.clone());
        local_member.transition(MemberState::Alive);
        self.gossip.add_member(local_member);

        // Contact seed nodes
        let seed_count = self.config.mesh.seed_nodes.len();
        if seed_count > 0 {
            // In production, this would actually connect to seeds
            // For now, just emit the joined event
        }

        self.emit_event(ClusterEvent::Joined {
            members: seed_count,
        });
        Ok(())
    }

    /// Leave the cluster gracefully.
    pub async fn leave(&self) -> Result<()> {
        // Remove from hash ring
        {
            let mut ring = self
                .hash_ring
                .write()
                .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
            ring.remove_node(self.local_node.id);
        }

        // Trigger migration of local sandboxes
        if self.config.mesh.auto_migrate {
            self.rebalance().await?;
        }

        Ok(())
    }

    /// Get the node responsible for a sandbox.
    pub fn get_owner(&self, sandbox_id: &str) -> Result<Option<NodeId>> {
        let ring = self
            .hash_ring
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(ring.get_node(sandbox_id))
    }

    /// Get the nodes responsible for a sandbox (with replicas).
    pub fn get_owners(&self, sandbox_id: &str) -> Result<Vec<NodeId>> {
        let ring = self
            .hash_ring
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        Ok(ring.get_nodes(sandbox_id, self.config.mesh.replication_factor))
    }

    /// Check if this node owns a sandbox.
    pub fn is_owner(&self, sandbox_id: &str) -> Result<bool> {
        let owners = self.get_owners(sandbox_id)?;
        Ok(owners.contains(&self.local_node.id))
    }

    /// Get all active members.
    pub fn members(&self) -> Vec<Member> {
        self.gossip.alive_members()
    }

    /// Get member by ID.
    pub fn get_member(&self, id: NodeId) -> Option<Member> {
        self.gossip.get_member(id)
    }

    /// Rebalance sandboxes across the cluster.
    pub async fn rebalance(&self) -> Result<()> {
        let _migration_manager = self
            .migration_manager
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        // Calculate which sandboxes need to move
        // In production, this would coordinate migrations

        self.emit_event(ClusterEvent::Rebalancing {
            sandboxes_moving: 0,
        });
        self.emit_event(ClusterEvent::RebalanceComplete { sandboxes_moved: 0 });

        Ok(())
    }

    /// Get cluster statistics.
    pub fn stats(&self) -> Result<MeshStats> {
        let state = self
            .state
            .read()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        let mut stats = state.stats.clone();
        stats.active_nodes = self.gossip.alive_members().len();

        Ok(stats)
    }

    /// Subscribe to cluster events.
    pub fn on_event<F>(&self, handler: F) -> Result<()>
    where
        F: Fn(ClusterEvent) + Send + Sync + 'static,
    {
        let mut handlers = self
            .event_handlers
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;
        handlers.push(Box::new(handler));
        Ok(())
    }

    /// Check if cluster is healthy.
    pub fn is_healthy(&self) -> bool {
        self.state.read().map(|s| s.is_healthy).unwrap_or(false)
    }

    /// Check if this node is the leader.
    pub fn is_leader(&self) -> bool {
        self.state.read().map(|s| s.is_leader).unwrap_or(false)
    }

    /// Get the current leader.
    pub fn leader(&self) -> Option<NodeId> {
        self.state.read().ok().and_then(|s| s.leader)
    }

    /// Run the cluster background tasks.
    pub async fn run(&self) -> Result<()> {
        let gossip_interval = self.gossip.interval();

        loop {
            // Gossip with random members
            self.gossip_round().await?;

            // Check for suspected members
            self.gossip.check_suspicion_timeout();

            // Update cluster state
            self.update_state()?;

            tokio::time::sleep(gossip_interval).await;
        }
    }

    /// Perform one gossip round.
    async fn gossip_round(&self) -> Result<()> {
        let targets = self.gossip.select_gossip_targets(3);

        for target in targets {
            let ping = self.gossip.create_ping();
            if let super::member::GossipMessage::Ping { seq, .. } = &ping {
                self.gossip.record_ping_sent(target.id(), *seq);
            }
            // In production, would send ping over network
        }

        Ok(())
    }

    /// Update cluster state.
    fn update_state(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|e| Error::Engine(format!("Lock error: {}", e)))?;

        let members = self.gossip.alive_members();
        state.is_healthy = !members.is_empty();
        state.stats.active_nodes = members.len();

        // Simple leader election: lowest node ID
        state.leader = members.iter().map(|m| m.id()).min();

        state.is_leader = state.leader == Some(self.local_node.id);

        Ok(())
    }

    /// Emit an event to all handlers.
    fn emit_event(&self, event: ClusterEvent) {
        if let Ok(handlers) = self.event_handlers.read() {
            for handler in handlers.iter() {
                handler(event.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_config_default() {
        let config = ClusterConfig::default();
        assert_eq!(config.name, "isolate-mesh");
        assert!(!config.tls_enabled);
    }

    #[test]
    fn test_cluster_creation() {
        let config = ClusterConfig::default();
        let cluster = MeshCluster::new(config).unwrap();

        assert!(!cluster.is_leader());
        assert!(!cluster.is_healthy());
    }

    #[tokio::test]
    async fn test_cluster_join() {
        let config = ClusterConfig::default();
        let cluster = MeshCluster::new(config).unwrap();

        cluster.join().await.unwrap();

        let members = cluster.members();
        assert_eq!(members.len(), 1);
    }

    #[tokio::test]
    async fn test_cluster_ownership() {
        let config = ClusterConfig::default();
        let cluster = MeshCluster::new(config).unwrap();
        cluster.join().await.unwrap();

        // With only one node, it should own everything
        let is_owner = cluster.is_owner("sandbox-123").unwrap();
        assert!(is_owner);
    }
}
