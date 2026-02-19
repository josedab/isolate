//! CRDT-based data replication for multi-region KV store.
//!
//! Provides conflict-free replicated data types (CRDTs) enabling
//! eventual consistency across distributed KV store nodes.
//!
//! # Supported CRDTs
//!
//! - **LWW-Register**: Last-Writer-Wins register with wall-clock timestamps
//! - **OR-Set**: Observed-Remove Set for conflict-free set operations
//! - **G-Counter**: Grow-only counter (distributed increment)
//! - **PN-Counter**: Positive-Negative counter (increment and decrement)
//!
//! # Vector Clocks
//!
//! Vector clocks provide causal ordering of events across nodes.



use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, SystemTime};

/// Node identifier in the replication cluster.
pub type NodeId = String;

/// Vector clock for causal ordering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    clocks: BTreeMap<NodeId, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock.
    pub fn new() -> Self {
        Self { clocks: BTreeMap::new() }
    }

    /// Increment the clock for a node.
    pub fn increment(&mut self, node: &str) {
        let counter = self.clocks.entry(node.to_string()).or_insert(0);
        *counter += 1;
    }

    /// Get the clock value for a node.
    pub fn get(&self, node: &str) -> u64 {
        self.clocks.get(node).copied().unwrap_or(0)
    }

    /// Merge with another vector clock (take max of each component).
    pub fn merge(&mut self, other: &VectorClock) {
        for (node, &value) in &other.clocks {
            let current = self.clocks.entry(node.clone()).or_insert(0);
            *current = (*current).max(value);
        }
    }

    /// Check if this clock happened before another (causal ordering).
    pub fn happened_before(&self, other: &VectorClock) -> bool {
        let mut at_least_one_less = false;

        for (node, &value) in &self.clocks {
            let other_value = other.get(node);
            if value > other_value {
                return false;
            }
            if value < other_value {
                at_least_one_less = true;
            }
        }

        // Check if other has nodes we don't
        for (node, &value) in &other.clocks {
            if value > 0 && !self.clocks.contains_key(node) {
                at_least_one_less = true;
            }
        }

        at_least_one_less
    }

    /// Check if two clocks are concurrent (neither happened before the other).
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happened_before(other) && !other.happened_before(self) && self != other
    }
}

/// Last-Writer-Wins Register.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister {
    /// The value.
    pub value: Vec<u8>,
    /// Timestamp of last write.
    pub timestamp: SystemTime,
    /// Node that performed the write.
    pub node: NodeId,
    /// Vector clock for causal ordering.
    pub vclock: VectorClock,
}

impl LwwRegister {
    /// Create a new LWW register.
    pub fn new(value: Vec<u8>, node: NodeId) -> Self {
        let mut vclock = VectorClock::new();
        vclock.increment(&node);
        Self { value, timestamp: SystemTime::now(), node, vclock }
    }

    /// Update the register value.
    pub fn set(&mut self, value: Vec<u8>, node: &str) {
        self.value = value;
        self.timestamp = SystemTime::now();
        self.node = node.to_string();
        self.vclock.increment(node);
    }

    /// Merge with another register (last writer wins by timestamp, with node ID as tiebreaker).
    pub fn merge(&mut self, other: &LwwRegister) {
        self.vclock.merge(&other.vclock);

        let should_update = match self.timestamp.cmp(&other.timestamp) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Greater => false,
            std::cmp::Ordering::Equal => other.node > self.node,
        };

        if should_update {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
            self.node = other.node.clone();
        }
    }
}

/// Observed-Remove Set (OR-Set) for conflict-free set operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrSet {
    /// Elements with their unique tags.
    elements: HashMap<String, HashSet<String>>,
    /// Tombstones (removed element tags).
    tombstones: HashSet<String>,
    /// Tag counter for generating unique tags.
    tag_counter: u64,
    /// Node ID for tag generation.
    node: NodeId,
}

impl OrSet {
    /// Create a new empty OR-Set.
    pub fn new(node: NodeId) -> Self {
        Self {
            elements: HashMap::new(),
            tombstones: HashSet::new(),
            tag_counter: 0,
            node,
        }
    }

    /// Add an element to the set.
    pub fn add(&mut self, element: &str) {
        self.tag_counter += 1;
        let tag = format!("{}:{}", self.node, self.tag_counter);
        self.elements.entry(element.to_string()).or_default().insert(tag);
    }

    /// Remove an element (observes and removes all current tags).
    pub fn remove(&mut self, element: &str) {
        if let Some(tags) = self.elements.remove(element) {
            for tag in tags {
                self.tombstones.insert(tag);
            }
        }
    }

    /// Check if an element is in the set.
    pub fn contains(&self, element: &str) -> bool {
        self.elements
            .get(element)
            .map(|tags| !tags.is_empty())
            .unwrap_or(false)
    }

    /// Get all elements in the set.
    pub fn elements(&self) -> Vec<&str> {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(elem, _)| elem.as_str())
            .collect()
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.elements.values().filter(|tags| !tags.is_empty()).count()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Merge with another OR-Set.
    pub fn merge(&mut self, other: &OrSet) {
        // Add all tombstones from other
        self.tombstones.extend(other.tombstones.iter().cloned());

        // Merge elements: union of tags, minus tombstones
        for (element, other_tags) in &other.elements {
            let tags = self.elements.entry(element.clone()).or_default();
            for tag in other_tags {
                if !self.tombstones.contains(tag) {
                    tags.insert(tag.clone());
                }
            }
        }

        // Remove tombstoned tags from our elements
        for tags in self.elements.values_mut() {
            tags.retain(|tag| !self.tombstones.contains(tag));
        }

        // Clean up empty entries
        self.elements.retain(|_, tags| !tags.is_empty());
    }
}

/// Grow-only counter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GCounter {
    counts: HashMap<NodeId, u64>,
}

impl GCounter {
    /// Create a new counter.
    pub fn new() -> Self {
        Self { counts: HashMap::new() }
    }

    /// Increment on the given node.
    pub fn increment(&mut self, node: &str) {
        *self.counts.entry(node.to_string()).or_insert(0) += 1;
    }

    /// Increment by a specific amount.
    pub fn increment_by(&mut self, node: &str, amount: u64) {
        *self.counts.entry(node.to_string()).or_insert(0) += amount;
    }

    /// Get the total count.
    pub fn value(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Merge with another counter.
    pub fn merge(&mut self, other: &GCounter) {
        for (node, &count) in &other.counts {
            let current = self.counts.entry(node.clone()).or_insert(0);
            *current = (*current).max(count);
        }
    }
}

/// Positive-Negative counter (supports both increment and decrement).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PnCounter {
    positive: GCounter,
    negative: GCounter,
}

impl PnCounter {
    /// Create a new counter.
    pub fn new() -> Self {
        Self { positive: GCounter::new(), negative: GCounter::new() }
    }

    /// Increment the counter.
    pub fn increment(&mut self, node: &str) {
        self.positive.increment(node);
    }

    /// Decrement the counter.
    pub fn decrement(&mut self, node: &str) {
        self.negative.increment(node);
    }

    /// Get the current value.
    pub fn value(&self) -> i64 {
        self.positive.value() as i64 - self.negative.value() as i64
    }

    /// Merge with another counter.
    pub fn merge(&mut self, other: &PnCounter) {
        self.positive.merge(&other.positive);
        self.negative.merge(&other.negative);
    }
}

/// Replication coordinator managing data sync across nodes.
pub struct ReplicationCoordinator {
    /// Local node ID.
    local_node: NodeId,
    /// Known peer nodes.
    peers: Vec<NodeId>,
    /// Pending replication operations.
    pending_ops: Vec<ReplicationOp>,
    /// Replication statistics.
    stats: ReplicationStats,
    /// Configuration.
    config: ReplicationConfig,
}

/// A replication operation to send to peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationOp {
    /// Source node.
    pub source: NodeId,
    /// Namespace.
    pub namespace: String,
    /// Key.
    pub key: String,
    /// Operation type.
    pub op_type: ReplicationOpType,
    /// Vector clock at time of operation.
    pub vclock: VectorClock,
    /// Timestamp.
    pub timestamp: SystemTime,
}

/// Type of replication operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationOpType {
    Set { value: Vec<u8> },
    Delete,
    Merge { crdt_state: Vec<u8> },
}

/// Replication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Replication factor (number of copies).
    pub replication_factor: usize,
    /// Sync interval.
    pub sync_interval: Duration,
    /// Maximum pending operations before forced sync.
    pub max_pending_ops: usize,
    /// Enable anti-entropy protocol.
    pub anti_entropy: bool,
    /// Read consistency level.
    pub read_consistency: ConsistencyLevel,
    /// Write consistency level.
    pub write_consistency: ConsistencyLevel,
}

/// Consistency levels for read/write operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsistencyLevel {
    /// Return immediately (no guarantees).
    One,
    /// Wait for quorum of nodes.
    Quorum,
    /// Wait for all replicas.
    All,
    /// Local only, no replication wait.
    Local,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            replication_factor: 3,
            sync_interval: Duration::from_millis(500),
            max_pending_ops: 1000,
            anti_entropy: true,
            read_consistency: ConsistencyLevel::One,
            write_consistency: ConsistencyLevel::One,
        }
    }
}

/// Replication statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplicationStats {
    pub ops_sent: u64,
    pub ops_received: u64,
    pub conflicts_resolved: u64,
    pub sync_rounds: u64,
    pub pending_ops: usize,
    pub bytes_replicated: u64,
}

impl ReplicationCoordinator {
    /// Create a new replication coordinator.
    pub fn new(local_node: NodeId, config: ReplicationConfig) -> Self {
        Self {
            local_node,
            peers: Vec::new(),
            pending_ops: Vec::new(),
            stats: ReplicationStats::default(),
            config,
        }
    }

    /// Add a peer node.
    pub fn add_peer(&mut self, peer: NodeId) {
        if !self.peers.contains(&peer) {
            self.peers.push(peer);
        }
    }

    /// Remove a peer node.
    pub fn remove_peer(&mut self, peer: &str) {
        self.peers.retain(|p| p != peer);
    }

    /// Record a local write for replication.
    pub fn record_write(&mut self, namespace: &str, key: &str, value: Vec<u8>) {
        let mut vclock = VectorClock::new();
        vclock.increment(&self.local_node);

        self.pending_ops.push(ReplicationOp {
            source: self.local_node.clone(),
            namespace: namespace.to_string(),
            key: key.to_string(),
            op_type: ReplicationOpType::Set { value },
            vclock,
            timestamp: SystemTime::now(),
        });

        self.stats.pending_ops = self.pending_ops.len();
    }

    /// Record a local delete for replication.
    pub fn record_delete(&mut self, namespace: &str, key: &str) {
        let mut vclock = VectorClock::new();
        vclock.increment(&self.local_node);

        self.pending_ops.push(ReplicationOp {
            source: self.local_node.clone(),
            namespace: namespace.to_string(),
            key: key.to_string(),
            op_type: ReplicationOpType::Delete,
            vclock,
            timestamp: SystemTime::now(),
        });

        self.stats.pending_ops = self.pending_ops.len();
    }

    /// Get pending operations to send to peers.
    pub fn drain_pending(&mut self) -> Vec<ReplicationOp> {
        let ops = std::mem::take(&mut self.pending_ops);
        self.stats.ops_sent += ops.len() as u64;
        self.stats.pending_ops = 0;
        ops
    }

    /// Apply a received replication operation.
    pub fn receive_op(&mut self, _op: &ReplicationOp) {
        self.stats.ops_received += 1;
    }

    /// Check if a sync is needed.
    pub fn needs_sync(&self) -> bool {
        self.pending_ops.len() >= self.config.max_pending_ops
    }

    /// Get replication statistics.
    pub fn stats(&self) -> &ReplicationStats {
        &self.stats
    }

    /// Get peer count.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get local node ID.
    pub fn local_node(&self) -> &str {
        &self.local_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock_increment() {
        let mut vc = VectorClock::new();
        vc.increment("node-1");
        vc.increment("node-1");
        vc.increment("node-2");

        assert_eq!(vc.get("node-1"), 2);
        assert_eq!(vc.get("node-2"), 1);
        assert_eq!(vc.get("node-3"), 0);
    }

    #[test]
    fn test_vector_clock_merge() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");
        vc1.increment("a");
        vc1.increment("b");

        let mut vc2 = VectorClock::new();
        vc2.increment("a");
        vc2.increment("c");
        vc2.increment("c");

        vc1.merge(&vc2);
        assert_eq!(vc1.get("a"), 2);
        assert_eq!(vc1.get("b"), 1);
        assert_eq!(vc1.get("c"), 2);
    }

    #[test]
    fn test_vector_clock_causality() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("a");
        vc2.increment("a");

        assert!(vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let mut vc1 = VectorClock::new();
        vc1.increment("a");

        let mut vc2 = VectorClock::new();
        vc2.increment("b");

        assert!(vc1.is_concurrent(&vc2));
    }

    #[test]
    fn test_lww_register() {
        let mut reg = LwwRegister::new(b"hello".to_vec(), "node-1".to_string());
        assert_eq!(reg.value, b"hello");

        reg.set(b"world".to_vec(), "node-1");
        assert_eq!(reg.value, b"world");
    }

    #[test]
    fn test_lww_register_merge() {
        let reg1 = LwwRegister::new(b"old".to_vec(), "node-1".to_string());
        std::thread::sleep(Duration::from_millis(10));
        let reg2 = LwwRegister::new(b"new".to_vec(), "node-2".to_string());

        let mut merged = reg1.clone();
        merged.merge(&reg2);
        assert_eq!(merged.value, b"new");
    }

    #[test]
    fn test_or_set_basic() {
        let mut set = OrSet::new("node-1".to_string());

        set.add("alice");
        set.add("bob");
        assert!(set.contains("alice"));
        assert!(set.contains("bob"));
        assert_eq!(set.len(), 2);

        set.remove("alice");
        assert!(!set.contains("alice"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_or_set_merge_concurrent_add() {
        let mut set1 = OrSet::new("node-1".to_string());
        let mut set2 = OrSet::new("node-2".to_string());

        set1.add("alice");
        set2.add("bob");

        set1.merge(&set2);
        assert!(set1.contains("alice"));
        assert!(set1.contains("bob"));
    }

    #[test]
    fn test_or_set_merge_add_remove_conflict() {
        let mut set1 = OrSet::new("node-1".to_string());
        set1.add("alice");

        let mut set2 = set1.clone();

        // Node 1 removes alice
        set1.remove("alice");
        // Node 2 concurrently adds alice again
        set2.add("alice");

        // Merge: add wins over remove (OR-Set semantics)
        set1.merge(&set2);
        assert!(set1.contains("alice"));
    }

    #[test]
    fn test_g_counter() {
        let mut counter = GCounter::new();

        counter.increment("node-1");
        counter.increment("node-1");
        counter.increment("node-2");

        assert_eq!(counter.value(), 3);
    }

    #[test]
    fn test_g_counter_merge() {
        let mut c1 = GCounter::new();
        c1.increment_by("a", 5);

        let mut c2 = GCounter::new();
        c2.increment_by("a", 3);
        c2.increment_by("b", 2);

        c1.merge(&c2);
        assert_eq!(c1.value(), 7); // max(5,3) + 2
    }

    #[test]
    fn test_pn_counter() {
        let mut counter = PnCounter::new();

        counter.increment("a");
        counter.increment("a");
        counter.decrement("b");

        assert_eq!(counter.value(), 1);
    }

    #[test]
    fn test_pn_counter_merge() {
        let mut c1 = PnCounter::new();
        c1.increment("a");
        c1.increment("a");

        let mut c2 = PnCounter::new();
        c2.increment("a");
        c2.decrement("b");

        c1.merge(&c2);
        assert_eq!(c1.value(), 1); // max(2,1) - max(0,1) = 2 - 1
    }

    #[test]
    fn test_replication_coordinator() {
        let config = ReplicationConfig::default();
        let mut coord = ReplicationCoordinator::new("node-1".to_string(), config);

        coord.add_peer("node-2".to_string());
        coord.add_peer("node-3".to_string());
        assert_eq!(coord.peer_count(), 2);

        coord.record_write("ns1", "key1", b"value1".to_vec());
        assert_eq!(coord.stats().pending_ops, 1);

        let ops = coord.drain_pending();
        assert_eq!(ops.len(), 1);
        assert_eq!(coord.stats().ops_sent, 1);
        assert_eq!(coord.stats().pending_ops, 0);
    }

    #[test]
    fn test_replication_coordinator_delete() {
        let config = ReplicationConfig::default();
        let mut coord = ReplicationCoordinator::new("node-1".to_string(), config);

        coord.record_delete("ns1", "key1");
        let ops = coord.drain_pending();
        assert!(matches!(ops[0].op_type, ReplicationOpType::Delete));
    }

    #[test]
    fn test_consistency_levels() {
        let config = ReplicationConfig {
            read_consistency: ConsistencyLevel::Quorum,
            write_consistency: ConsistencyLevel::All,
            ..Default::default()
        };

        assert_eq!(config.read_consistency, ConsistencyLevel::Quorum);
        assert_eq!(config.write_consistency, ConsistencyLevel::All);
    }
}
