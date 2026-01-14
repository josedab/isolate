//! Cluster membership and gossip protocol.

use super::{NodeAddr, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// State of a cluster member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemberState {
    /// Member is joining the cluster.
    Joining,
    /// Member is alive and healthy.
    Alive,
    /// Member is suspected to be down.
    Suspect,
    /// Member has been confirmed dead.
    Dead,
    /// Member is leaving gracefully.
    Leaving,
    /// Member has left the cluster.
    Left,
}

impl MemberState {
    /// Check if member is active (can accept work).
    pub fn is_active(&self) -> bool {
        matches!(self, MemberState::Alive)
    }

    /// Check if member should be removed.
    pub fn should_remove(&self) -> bool {
        matches!(self, MemberState::Dead | MemberState::Left)
    }
}

/// Health status of a member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberHealth {
    /// Current health score (0.0-1.0).
    pub score: f64,
    /// Last successful ping.
    pub last_ping: Option<Duration>,
    /// Round-trip time in milliseconds.
    pub rtt_ms: u64,
    /// Number of failed pings.
    pub failed_pings: u32,
    /// CPU utilization percentage.
    pub cpu_usage: f64,
    /// Memory utilization percentage.
    pub memory_usage: f64,
    /// Number of active sandboxes.
    pub active_sandboxes: u32,
    /// Available capacity.
    pub available_capacity: u32,
}

impl Default for MemberHealth {
    fn default() -> Self {
        Self {
            score: 1.0,
            last_ping: None,
            rtt_ms: 0,
            failed_pings: 0,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            active_sandboxes: 0,
            available_capacity: 100,
        }
    }
}

impl MemberHealth {
    /// Check if member is healthy.
    pub fn is_healthy(&self) -> bool {
        self.score > 0.5 && self.failed_pings < 3
    }

    /// Update health based on ping result.
    pub fn record_ping(&mut self, success: bool, rtt_ms: u64) {
        if success {
            self.rtt_ms = rtt_ms;
            self.failed_pings = 0;
            self.score = (self.score * 0.9 + 1.0 * 0.1).min(1.0);
        } else {
            self.failed_pings += 1;
            self.score = (self.score * 0.9 + 0.0 * 0.1).max(0.0);
        }
    }

    /// Calculate load factor for routing decisions.
    pub fn load_factor(&self) -> f64 {
        let cpu_factor = self.cpu_usage / 100.0;
        let mem_factor = self.memory_usage / 100.0;
        let sandbox_factor = if self.available_capacity > 0 {
            self.active_sandboxes as f64 / (self.active_sandboxes + self.available_capacity) as f64
        } else {
            1.0
        };

        (cpu_factor * 0.4 + mem_factor * 0.3 + sandbox_factor * 0.3).min(1.0)
    }
}

/// A member of the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    /// Node address.
    pub addr: NodeAddr,
    /// Current state.
    pub state: MemberState,
    /// Health information.
    pub health: MemberHealth,
    /// Incarnation number for conflict resolution.
    pub incarnation: u64,
    /// Metadata tags.
    pub tags: HashMap<String, String>,
    /// Last state change.
    #[serde(skip)]
    pub last_state_change: Option<Instant>,
}

impl Member {
    /// Create a new member.
    pub fn new(addr: NodeAddr) -> Self {
        Self {
            addr,
            state: MemberState::Joining,
            health: MemberHealth::default(),
            incarnation: 0,
            tags: HashMap::new(),
            last_state_change: Some(Instant::now()),
        }
    }

    /// Get the node ID.
    pub fn id(&self) -> NodeId {
        self.addr.id
    }

    /// Transition to a new state.
    pub fn transition(&mut self, new_state: MemberState) {
        self.state = new_state;
        self.last_state_change = Some(Instant::now());
    }

    /// Increment incarnation for refuting suspicion.
    pub fn refute(&mut self) {
        self.incarnation += 1;
        if self.state == MemberState::Suspect {
            self.state = MemberState::Alive;
        }
    }

    /// Check if this member info supersedes another.
    pub fn supersedes(&self, other: &Member) -> bool {
        self.addr.id == other.addr.id && self.incarnation > other.incarnation
    }
}

/// Gossip message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    /// Ping request.
    Ping {
        /// Sender node.
        from: NodeId,
        /// Sequence number.
        seq: u64,
    },
    /// Ping response.
    Ack {
        /// Responder node.
        from: NodeId,
        /// Sequence number.
        seq: u64,
    },
    /// Indirect ping request.
    PingReq {
        /// Sender node.
        from: NodeId,
        /// Target node.
        target: NodeId,
        /// Sequence number.
        seq: u64,
    },
    /// Member state update.
    Sync {
        /// Updated member info.
        member: Member,
    },
    /// Join request.
    Join {
        /// Joining member.
        member: Member,
    },
    /// Leave notification.
    Leave {
        /// Leaving node.
        node_id: NodeId,
        /// Incarnation.
        incarnation: u64,
    },
    /// Full membership list.
    MemberList {
        /// All known members.
        members: Vec<Member>,
    },
}

/// Gossip protocol implementation.
pub struct Gossip {
    /// This node's ID.
    local_id: NodeId,
    /// Known members.
    members: Arc<RwLock<HashMap<NodeId, Member>>>,
    /// Pending pings.
    pending_pings: Arc<RwLock<HashMap<u64, (NodeId, Instant)>>>,
    /// Next sequence number.
    next_seq: std::sync::atomic::AtomicU64,
    /// Gossip interval.
    interval: Duration,
    /// Suspicion timeout.
    suspicion_timeout: Duration,
}

impl Gossip {
    /// Create a new gossip protocol handler.
    pub fn new(local_id: NodeId, interval: Duration) -> Self {
        Self {
            local_id,
            members: Arc::new(RwLock::new(HashMap::new())),
            pending_pings: Arc::new(RwLock::new(HashMap::new())),
            next_seq: std::sync::atomic::AtomicU64::new(1),
            interval,
            suspicion_timeout: Duration::from_secs(5),
        }
    }

    /// Add a member.
    pub fn add_member(&self, member: Member) {
        if let Ok(mut members) = self.members.write() {
            let id = member.id();
            // Only update if newer incarnation or new member
            if let Some(existing) = members.get(&id) {
                if member.supersedes(existing) || existing.state.should_remove() {
                    members.insert(id, member);
                }
            } else {
                members.insert(id, member);
            }
        }
    }

    /// Get a member by ID.
    pub fn get_member(&self, id: NodeId) -> Option<Member> {
        self.members.read().ok()?.get(&id).cloned()
    }

    /// Get all alive members.
    pub fn alive_members(&self) -> Vec<Member> {
        self.members
            .read()
            .map(|m| m.values().filter(|m| m.state.is_active()).cloned().collect())
            .unwrap_or_default()
    }

    /// Get all members.
    pub fn all_members(&self) -> Vec<Member> {
        self.members.read().map(|m| m.values().cloned().collect()).unwrap_or_default()
    }

    /// Select random members for gossiping.
    pub fn select_gossip_targets(&self, count: usize) -> Vec<Member> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let members = self.alive_members();
        if members.len() <= count {
            return members;
        }

        // Simple random selection using hash
        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        let seed = hasher.finish() as usize;

        members
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (seed + i) % 3 == 0) // ~33% selection
            .take(count)
            .map(|(_, m)| m)
            .collect()
    }

    /// Create a ping message.
    pub fn create_ping(&self) -> GossipMessage {
        let seq = self.next_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        GossipMessage::Ping { from: self.local_id, seq }
    }

    /// Create an ack message.
    pub fn create_ack(&self, seq: u64) -> GossipMessage {
        GossipMessage::Ack { from: self.local_id, seq }
    }

    /// Handle received gossip message.
    pub fn handle_message(&self, msg: GossipMessage) -> Option<GossipMessage> {
        match msg {
            GossipMessage::Ping { from: _, seq } => Some(self.create_ack(seq)),
            GossipMessage::Ack { from, seq } => {
                self.record_ack(from, seq);
                None
            }
            GossipMessage::Sync { member } => {
                self.merge_member(member);
                None
            }
            GossipMessage::Join { member } => {
                self.add_member(member);
                Some(GossipMessage::MemberList { members: self.all_members() })
            }
            GossipMessage::Leave { node_id, incarnation: _ } => {
                self.mark_leaving(node_id);
                None
            }
            GossipMessage::MemberList { members } => {
                for member in members {
                    self.merge_member(member);
                }
                None
            }
            GossipMessage::PingReq { from: _, target, seq } => {
                // Forward ping to target
                Some(GossipMessage::Ping { from: target, seq })
            }
        }
    }

    /// Record a ping being sent.
    pub fn record_ping_sent(&self, target: NodeId, seq: u64) {
        if let Ok(mut pending) = self.pending_pings.write() {
            pending.insert(seq, (target, Instant::now()));
        }
    }

    /// Record an ack received.
    fn record_ack(&self, from: NodeId, seq: u64) {
        let rtt = if let Ok(mut pending) = self.pending_pings.write() {
            pending.remove(&seq).map(|(_, sent)| sent.elapsed())
        } else {
            None
        };

        if let Ok(mut members) = self.members.write() {
            if let Some(member) = members.get_mut(&from) {
                member.health.record_ping(true, rtt.map(|d| d.as_millis() as u64).unwrap_or(0));
                if member.state == MemberState::Suspect {
                    member.transition(MemberState::Alive);
                }
            }
        }
    }

    /// Merge member information.
    fn merge_member(&self, new_member: Member) {
        if let Ok(mut members) = self.members.write() {
            let id = new_member.id();
            if let Some(existing) = members.get_mut(&id) {
                if new_member.supersedes(existing) {
                    *existing = new_member;
                }
            } else {
                members.insert(id, new_member);
            }
        }
    }

    /// Mark a member as leaving.
    fn mark_leaving(&self, node_id: NodeId) {
        if let Ok(mut members) = self.members.write() {
            if let Some(member) = members.get_mut(&node_id) {
                member.transition(MemberState::Leaving);
            }
        }
    }

    /// Suspect a member that hasn't responded.
    pub fn suspect_member(&self, node_id: NodeId) {
        if let Ok(mut members) = self.members.write() {
            if let Some(member) = members.get_mut(&node_id) {
                if member.state == MemberState::Alive {
                    member.transition(MemberState::Suspect);
                    member.health.record_ping(false, 0);
                }
            }
        }
    }

    /// Mark suspected members as dead after timeout.
    pub fn check_suspicion_timeout(&self) {
        if let Ok(mut members) = self.members.write() {
            let now = Instant::now();
            for member in members.values_mut() {
                if member.state == MemberState::Suspect {
                    if let Some(last_change) = member.last_state_change {
                        if now.duration_since(last_change) > self.suspicion_timeout {
                            member.transition(MemberState::Dead);
                        }
                    }
                }
            }
        }
    }

    /// Get gossip interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_addr(id: u64) -> NodeAddr {
        NodeAddr::new(NodeId::new(id), format!("127.0.0.1:{}", 9000 + id).parse().unwrap())
    }

    #[test]
    fn test_member_state() {
        assert!(MemberState::Alive.is_active());
        assert!(!MemberState::Dead.is_active());
        assert!(MemberState::Dead.should_remove());
    }

    #[test]
    fn test_member_health() {
        let mut health = MemberHealth::default();
        assert!(health.is_healthy());

        health.record_ping(true, 10);
        assert_eq!(health.rtt_ms, 10);
        assert_eq!(health.failed_pings, 0);

        health.record_ping(false, 0);
        health.record_ping(false, 0);
        health.record_ping(false, 0);
        assert!(!health.is_healthy());
    }

    #[test]
    fn test_member_transition() {
        let mut member = Member::new(test_addr(1));
        assert_eq!(member.state, MemberState::Joining);

        member.transition(MemberState::Alive);
        assert_eq!(member.state, MemberState::Alive);
    }

    #[test]
    fn test_member_refute() {
        let mut member = Member::new(test_addr(1));
        member.transition(MemberState::Suspect);
        member.refute();

        assert_eq!(member.state, MemberState::Alive);
        assert_eq!(member.incarnation, 1);
    }

    #[test]
    fn test_gossip_add_member() {
        let gossip = Gossip::new(NodeId::new(0), Duration::from_millis(100));
        let member = Member::new(test_addr(1));

        gossip.add_member(member.clone());
        let retrieved = gossip.get_member(NodeId::new(1));

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id(), NodeId::new(1));
    }

    #[test]
    fn test_gossip_ping_ack() {
        let gossip = Gossip::new(NodeId::new(0), Duration::from_millis(100));

        let ping = gossip.create_ping();
        if let GossipMessage::Ping { seq, .. } = ping {
            let ack = gossip.create_ack(seq);
            if let GossipMessage::Ack { seq: ack_seq, .. } = ack {
                assert_eq!(seq, ack_seq);
            } else {
                panic!("Expected Ack message");
            }
        } else {
            panic!("Expected Ping message");
        }
    }
}
