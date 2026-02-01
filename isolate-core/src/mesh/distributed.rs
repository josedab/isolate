//! Distributed Execution & Network Transport
//!
//! Provides the `NetworkTransport` trait for pluggable network backends,
//! and `ClusterManager` that integrates gossip, routing, scheduling, and
//! failure detection into a single coordinator for distributed sandbox execution.

use super::{
    member::GossipMessage, Gossip, HashRing, Member, MemberState, MeshConfig, MeshStats, NodeAddr,
    NodeId, SandboxRouter,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Network transport abstraction
// ---------------------------------------------------------------------------

/// Error type for network transport operations.
#[derive(Debug, Clone)]
pub enum TransportError {
    /// Connection failed.
    ConnectionFailed(String),
    /// Send failed.
    SendFailed(String),
    /// Receive timed out.
    Timeout,
    /// Node unreachable.
    Unreachable(NodeId),
    /// Serialization error.
    SerializationError(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(m) => write!(f, "Connection failed: {}", m),
            Self::SendFailed(m) => write!(f, "Send failed: {}", m),
            Self::Timeout => write!(f, "Transport timeout"),
            Self::Unreachable(id) => write!(f, "Node unreachable: {}", id),
            Self::SerializationError(m) => write!(f, "Serialization error: {}", m),
        }
    }
}

impl std::error::Error for TransportError {}

/// Abstraction over the network layer used for gossip and RPC.
///
/// Implementations handle actual TCP/UDP communication. The `InMemoryTransport`
/// is provided for testing and single-process scenarios.
pub trait NetworkTransport: Send + Sync + Debug {
    /// Returns the transport name (e.g., "tcp", "in-memory").
    fn name(&self) -> &str;

    /// Sends a gossip message to a specific node.
    fn send(&mut self, target: NodeId, msg: &GossipMessage) -> Result<(), TransportError>;

    /// Receives pending messages (non-blocking).
    fn receive(&mut self) -> Vec<(NodeId, GossipMessage)>;

    /// Checks connectivity to a node (returns RTT on success).
    fn ping(&mut self, target: NodeId) -> Result<Duration, TransportError>;

    /// Returns the set of directly connected peers.
    fn connected_peers(&self) -> Vec<NodeId>;
}

/// In-memory transport for testing and single-process clusters.
#[derive(Debug)]
pub struct InMemoryTransport {
    local_id: NodeId,
    inbox: Vec<(NodeId, GossipMessage)>,
    outbox: Vec<(NodeId, GossipMessage)>,
    peers: HashMap<NodeId, bool>, // connected?
}

impl InMemoryTransport {
    pub fn new(local_id: NodeId) -> Self {
        Self { local_id, inbox: Vec::new(), outbox: Vec::new(), peers: HashMap::new() }
    }

    /// Simulates connecting to a peer.
    pub fn connect_peer(&mut self, peer: NodeId) {
        self.peers.insert(peer, true);
    }

    /// Simulates a peer going offline.
    pub fn disconnect_peer(&mut self, peer: NodeId) {
        self.peers.insert(peer, false);
    }

    /// Delivers a message to this transport's inbox (simulates receiving).
    pub fn deliver(&mut self, from: NodeId, msg: GossipMessage) {
        self.inbox.push((from, msg));
    }

    /// Drains sent messages (for test assertions).
    pub fn drain_outbox(&mut self) -> Vec<(NodeId, GossipMessage)> {
        std::mem::take(&mut self.outbox)
    }
}

impl NetworkTransport for InMemoryTransport {
    fn name(&self) -> &str {
        "in-memory"
    }

    fn send(&mut self, target: NodeId, msg: &GossipMessage) -> Result<(), TransportError> {
        match self.peers.get(&target) {
            Some(true) => {
                self.outbox.push((target, msg.clone()));
                Ok(())
            }
            Some(false) => Err(TransportError::Unreachable(target)),
            None => Err(TransportError::Unreachable(target)),
        }
    }

    fn receive(&mut self) -> Vec<(NodeId, GossipMessage)> {
        std::mem::take(&mut self.inbox)
    }

    fn ping(&mut self, target: NodeId) -> Result<Duration, TransportError> {
        match self.peers.get(&target) {
            Some(true) => Ok(Duration::from_micros(100)), // Simulated RTT
            _ => Err(TransportError::Unreachable(target)),
        }
    }

    fn connected_peers(&self) -> Vec<NodeId> {
        self.peers.iter().filter(|(_, connected)| **connected).map(|(id, _)| *id).collect()
    }
}

// ---------------------------------------------------------------------------
// Cluster manager
// ---------------------------------------------------------------------------

/// Event emitted by the cluster manager.
#[derive(Debug, Clone)]
pub enum ClusterManagerEvent {
    /// A new node joined the cluster.
    NodeJoined(NodeId),
    /// A node was detected as failed.
    NodeFailed(NodeId),
    /// A node left gracefully.
    NodeLeft(NodeId),
    /// A sandbox was placed on a node.
    SandboxPlaced { sandbox_id: String, node: NodeId },
    /// A sandbox was migrated from one node to another.
    SandboxMigrated { sandbox_id: String, from: NodeId, to: NodeId },
    /// Failure detection triggered for a suspect node.
    SuspicionRaised(NodeId),
}

/// Coordinates distributed sandbox execution.
///
/// Integrates:
/// - **Gossip** for membership and failure detection
/// - **HashRing** for deterministic sandbox placement
/// - **Router** for routing decisions
/// - **Transport** for network communication
pub struct ClusterManager {
    local_id: NodeId,
    config: MeshConfig,
    gossip: Gossip,
    hash_ring: HashRing,
    router: SandboxRouter,
    transport: Box<dyn NetworkTransport>,
    sandbox_placement: HashMap<String, NodeId>,
    events: Vec<ClusterManagerEvent>,
    stats: MeshStats,
    last_gossip_round: Instant,
    last_failure_check: Instant,
}

impl ClusterManager {
    /// Creates a new cluster manager.
    pub fn new(local_id: NodeId, config: MeshConfig, transport: Box<dyn NetworkTransport>) -> Self {
        let gossip = Gossip::new(local_id, config.gossip_interval);
        let hash_ring = HashRing::new(config.virtual_nodes);
        let router = SandboxRouter::new(config.replication_factor);

        Self {
            local_id,
            config,
            gossip,
            hash_ring,
            router,
            transport,
            sandbox_placement: HashMap::new(),
            events: Vec::new(),
            stats: MeshStats::default(),
            last_gossip_round: Instant::now(),
            last_failure_check: Instant::now(),
        }
    }

    /// Adds a node to the cluster.
    pub fn add_node(&mut self, addr: NodeAddr) {
        let node_id = addr.id;
        let mut member = Member::new(addr);
        member.transition(MemberState::Alive);

        self.gossip.add_member(member);
        self.hash_ring.add_node(node_id);
        self.stats.active_nodes += 1;
        self.events.push(ClusterManagerEvent::NodeJoined(node_id));
    }

    /// Removes a node from the cluster.
    pub fn remove_node(&mut self, node_id: NodeId) {
        self.hash_ring.remove_node(node_id);
        self.gossip.suspect_member(node_id);
        self.stats.active_nodes = self.stats.active_nodes.saturating_sub(1);
        self.events.push(ClusterManagerEvent::NodeLeft(node_id));
    }

    /// Places a sandbox on the best available node.
    pub fn place_sandbox(&mut self, sandbox_id: &str) -> Option<NodeId> {
        let target = self.hash_ring.get_node(sandbox_id)?;

        // Verify the target node is healthy
        if let Some(member) = self.gossip.get_member(target) {
            if !member.state.is_active() {
                // Fallback: find next healthy node
                let alive = self.gossip.alive_members();
                if let Some(fallback) = alive.first() {
                    let fallback_id = fallback.id();
                    self.sandbox_placement.insert(sandbox_id.to_string(), fallback_id);
                    self.stats.total_sandboxes += 1;
                    self.events.push(ClusterManagerEvent::SandboxPlaced {
                        sandbox_id: sandbox_id.to_string(),
                        node: fallback_id,
                    });
                    return Some(fallback_id);
                }
                return None;
            }
        }

        self.sandbox_placement.insert(sandbox_id.to_string(), target);
        self.stats.total_sandboxes += 1;
        self.events.push(ClusterManagerEvent::SandboxPlaced {
            sandbox_id: sandbox_id.to_string(),
            node: target,
        });

        Some(target)
    }

    /// Returns which node owns a given sandbox.
    pub fn locate_sandbox(&self, sandbox_id: &str) -> Option<NodeId> {
        self.sandbox_placement.get(sandbox_id).copied()
    }

    /// Migrates a sandbox from its current node to a target node.
    pub fn migrate_sandbox(&mut self, sandbox_id: &str, target: NodeId) -> bool {
        if let Some(current) = self.sandbox_placement.get(sandbox_id).copied() {
            if current == target {
                return false;
            }

            // Verify target is alive
            if let Some(member) = self.gossip.get_member(target) {
                if !member.state.is_active() {
                    self.stats.failed_migrations += 1;
                    return false;
                }
            }

            self.sandbox_placement.insert(sandbox_id.to_string(), target);
            self.stats.completed_migrations += 1;
            self.events.push(ClusterManagerEvent::SandboxMigrated {
                sandbox_id: sandbox_id.to_string(),
                from: current,
                to: target,
            });
            true
        } else {
            false
        }
    }

    /// Runs one gossip round: send pings, process received messages, check failures.
    pub fn tick(&mut self) {
        // Process incoming messages
        let incoming = self.transport.receive();
        for (from, msg) in incoming {
            self.stats.gossip_received += 1;
            if let Some(reply) = self.gossip.handle_message(msg) {
                let _ = self.transport.send(from, &reply);
                self.stats.gossip_sent += 1;
            }
        }

        // Send gossip to random targets
        if self.last_gossip_round.elapsed() >= self.config.gossip_interval {
            let targets = self.gossip.select_gossip_targets(3);
            let ping = self.gossip.create_ping();

            for target in targets {
                let target_id = target.id();
                if let Err(_) = self.transport.send(target_id, &ping) {
                    self.gossip.suspect_member(target_id);
                    self.events.push(ClusterManagerEvent::SuspicionRaised(target_id));
                }
                self.stats.gossip_sent += 1;
            }
            self.last_gossip_round = Instant::now();
        }

        // Failure detection
        if self.last_failure_check.elapsed() >= self.config.failure_timeout {
            self.gossip.check_suspicion_timeout();

            // Check for newly-dead nodes and migrate their sandboxes
            if self.config.auto_migrate {
                self.handle_failures();
            }
            self.last_failure_check = Instant::now();
        }
    }

    /// Returns current cluster stats.
    pub fn stats(&self) -> &MeshStats {
        &self.stats
    }

    /// Returns all alive nodes.
    pub fn alive_nodes(&self) -> Vec<NodeId> {
        self.gossip.alive_members().iter().map(|m| m.id()).collect()
    }

    /// Returns pending events and clears the buffer.
    pub fn drain_events(&mut self) -> Vec<ClusterManagerEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns the local node ID.
    pub fn local_id(&self) -> NodeId {
        self.local_id
    }

    /// Returns the number of placed sandboxes.
    pub fn sandbox_count(&self) -> usize {
        self.sandbox_placement.len()
    }

    // -- private --

    fn handle_failures(&mut self) {
        let all_members = self.gossip.all_members();
        let dead_nodes: Vec<NodeId> =
            all_members.iter().filter(|m| m.state == MemberState::Dead).map(|m| m.id()).collect();

        for dead_id in dead_nodes {
            // Find sandboxes on the dead node
            let affected: Vec<String> = self
                .sandbox_placement
                .iter()
                .filter(|(_, node)| **node == dead_id)
                .map(|(id, _)| id.clone())
                .collect();

            // Migrate each to the next available node
            let alive = self.gossip.alive_members();
            if let Some(target) = alive.first() {
                let target_id = target.id();
                for sandbox_id in affected {
                    self.sandbox_placement.insert(sandbox_id.clone(), target_id);
                    self.stats.completed_migrations += 1;
                    self.events.push(ClusterManagerEvent::SandboxMigrated {
                        sandbox_id,
                        from: dead_id,
                        to: target_id,
                    });
                }
            }

            self.hash_ring.remove_node(dead_id);
            self.stats.active_nodes = self.stats.active_nodes.saturating_sub(1);
            self.events.push(ClusterManagerEvent::NodeFailed(dead_id));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn addr(id: u64) -> NodeAddr {
        NodeAddr::new(
            NodeId::new(id),
            format!("127.0.0.1:{}", 9000 + id).parse::<SocketAddr>().unwrap(),
        )
    }

    fn manager() -> ClusterManager {
        let local_id = NodeId::new(0);
        let config = MeshConfig::default();
        let mut transport = InMemoryTransport::new(local_id);
        // Connect peers
        for i in 1..=5 {
            transport.connect_peer(NodeId::new(i));
        }
        ClusterManager::new(local_id, config, Box::new(transport))
    }

    // -- Transport tests --

    #[test]
    fn test_in_memory_transport_name() {
        let t = InMemoryTransport::new(NodeId::new(0));
        assert_eq!(t.name(), "in-memory");
    }

    #[test]
    fn test_in_memory_send_to_connected_peer() {
        let mut t = InMemoryTransport::new(NodeId::new(0));
        t.connect_peer(NodeId::new(1));

        let msg = GossipMessage::Ping { from: NodeId::new(0), seq: 1 };
        assert!(t.send(NodeId::new(1), &msg).is_ok());
        assert_eq!(t.drain_outbox().len(), 1);
    }

    #[test]
    fn test_in_memory_send_to_disconnected() {
        let mut t = InMemoryTransport::new(NodeId::new(0));
        let msg = GossipMessage::Ping { from: NodeId::new(0), seq: 1 };
        assert!(matches!(t.send(NodeId::new(99), &msg), Err(TransportError::Unreachable(_))));
    }

    #[test]
    fn test_in_memory_ping() {
        let mut t = InMemoryTransport::new(NodeId::new(0));
        t.connect_peer(NodeId::new(1));
        assert!(t.ping(NodeId::new(1)).is_ok());
        assert!(t.ping(NodeId::new(99)).is_err());
    }

    #[test]
    fn test_in_memory_deliver_and_receive() {
        let mut t = InMemoryTransport::new(NodeId::new(0));
        t.deliver(NodeId::new(1), GossipMessage::Ping { from: NodeId::new(1), seq: 1 });

        let msgs = t.receive();
        assert_eq!(msgs.len(), 1);
        assert!(t.receive().is_empty()); // drained
    }

    #[test]
    fn test_in_memory_connected_peers() {
        let mut t = InMemoryTransport::new(NodeId::new(0));
        t.connect_peer(NodeId::new(1));
        t.connect_peer(NodeId::new(2));
        t.disconnect_peer(NodeId::new(1));

        let peers = t.connected_peers();
        assert_eq!(peers.len(), 1);
        assert!(peers.contains(&NodeId::new(2)));
    }

    // -- ClusterManager tests --

    #[test]
    fn test_cluster_manager_creation() {
        let m = manager();
        assert_eq!(m.local_id(), NodeId::new(0));
        assert_eq!(m.sandbox_count(), 0);
    }

    #[test]
    fn test_add_and_remove_node() {
        let mut m = manager();
        m.add_node(addr(1));
        m.add_node(addr(2));
        assert_eq!(m.stats().active_nodes, 2);
        assert_eq!(m.alive_nodes().len(), 2);

        m.remove_node(NodeId::new(1));
        assert_eq!(m.stats().active_nodes, 1);
    }

    #[test]
    fn test_place_sandbox() {
        let mut m = manager();
        m.add_node(addr(1));
        m.add_node(addr(2));

        let node = m.place_sandbox("sandbox-alpha");
        assert!(node.is_some());
        assert_eq!(m.sandbox_count(), 1);
        assert_eq!(m.locate_sandbox("sandbox-alpha"), node);
    }

    #[test]
    fn test_place_multiple_sandboxes() {
        let mut m = manager();
        for i in 1..=3 {
            m.add_node(addr(i));
        }

        for i in 0..10 {
            m.place_sandbox(&format!("sb-{}", i));
        }

        assert_eq!(m.sandbox_count(), 10);
    }

    #[test]
    fn test_migrate_sandbox() {
        let mut m = manager();
        m.add_node(addr(1));
        m.add_node(addr(2));

        m.place_sandbox("sandbox-1");
        let original = m.locate_sandbox("sandbox-1").unwrap();

        let target = if original == NodeId::new(1) { NodeId::new(2) } else { NodeId::new(1) };

        assert!(m.migrate_sandbox("sandbox-1", target));
        assert_eq!(m.locate_sandbox("sandbox-1"), Some(target));
        assert_eq!(m.stats().completed_migrations, 1);
    }

    #[test]
    fn test_migrate_nonexistent_sandbox() {
        let mut m = manager();
        m.add_node(addr(1));
        assert!(!m.migrate_sandbox("ghost", NodeId::new(1)));
    }

    #[test]
    fn test_migrate_same_node_noop() {
        let mut m = manager();
        m.add_node(addr(1));
        m.place_sandbox("sb-1");

        let current = m.locate_sandbox("sb-1").unwrap();
        assert!(!m.migrate_sandbox("sb-1", current));
    }

    #[test]
    fn test_drain_events() {
        let mut m = manager();
        m.add_node(addr(1));
        m.place_sandbox("sb-1");

        let events = m.drain_events();
        assert!(events.len() >= 2); // NodeJoined + SandboxPlaced
        assert!(m.drain_events().is_empty()); // drained
    }

    #[test]
    fn test_tick_processes_messages() {
        let mut m = manager();
        m.add_node(addr(1));

        // Simulate receiving a ping
        let local_id = m.local_id();
        let _config = MeshConfig::default();
        let mut transport = InMemoryTransport::new(local_id);
        transport.connect_peer(NodeId::new(1));
        transport.deliver(NodeId::new(1), GossipMessage::Ping { from: NodeId::new(1), seq: 42 });

        // Replace transport
        m.transport = Box::new(transport);
        m.tick();

        assert!(m.stats().gossip_received >= 1);
    }

    #[test]
    fn test_cluster_stats() {
        let mut m = manager();
        m.add_node(addr(1));
        m.add_node(addr(2));
        m.place_sandbox("sb-1");

        let stats = m.stats();
        assert_eq!(stats.active_nodes, 2);
        assert_eq!(stats.total_sandboxes, 1);
    }

    #[test]
    fn test_transport_error_display() {
        assert!(TransportError::Timeout.to_string().contains("timeout"));
        assert!(TransportError::Unreachable(NodeId::new(1)).to_string().contains("unreachable"));
        assert!(TransportError::ConnectionFailed("test".into()).to_string().contains("Connection"));
    }

    #[test]
    fn test_cluster_manager_event_types() {
        let mut m = manager();
        m.add_node(addr(1));
        m.place_sandbox("sb-1");

        let events = m.drain_events();
        let has_join = events.iter().any(|e| matches!(e, ClusterManagerEvent::NodeJoined(_)));
        let has_place =
            events.iter().any(|e| matches!(e, ClusterManagerEvent::SandboxPlaced { .. }));

        assert!(has_join);
        assert!(has_place);
    }
}
