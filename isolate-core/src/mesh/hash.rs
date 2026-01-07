//! Consistent hashing for sandbox distribution.

use super::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

/// A virtual node on the hash ring.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VirtualNode {
    /// The physical node this virtual node represents.
    pub node_id: NodeId,
    /// Virtual node index.
    pub index: usize,
    /// Position on the ring.
    pub position: u64,
}

impl VirtualNode {
    /// Create a new virtual node.
    pub fn new(node_id: NodeId, index: usize) -> Self {
        let position = Self::hash_position(node_id, index);
        Self {
            node_id,
            index,
            position,
        }
    }

    fn hash_position(node_id: NodeId, index: usize) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        node_id.0.hash(&mut hasher);
        index.hash(&mut hasher);
        hasher.finish()
    }
}

/// Consistent hash ring for sandbox distribution.
#[derive(Debug, Clone)]
pub struct HashRing {
    /// Virtual nodes sorted by position.
    ring: BTreeMap<u64, VirtualNode>,
    /// Mapping from physical node to virtual nodes.
    node_vnodes: HashMap<NodeId, Vec<u64>>,
    /// Number of virtual nodes per physical node.
    virtual_node_count: usize,
}

impl Default for HashRing {
    fn default() -> Self {
        Self::new(150)
    }
}

impl HashRing {
    /// Create a new hash ring.
    pub fn new(virtual_node_count: usize) -> Self {
        Self {
            ring: BTreeMap::new(),
            node_vnodes: HashMap::new(),
            virtual_node_count,
        }
    }

    /// Add a node to the ring.
    pub fn add_node(&mut self, node_id: NodeId) {
        let mut positions = Vec::with_capacity(self.virtual_node_count);

        for i in 0..self.virtual_node_count {
            let vnode = VirtualNode::new(node_id, i);
            positions.push(vnode.position);
            self.ring.insert(vnode.position, vnode);
        }

        self.node_vnodes.insert(node_id, positions);
    }

    /// Remove a node from the ring.
    pub fn remove_node(&mut self, node_id: NodeId) {
        if let Some(positions) = self.node_vnodes.remove(&node_id) {
            for pos in positions {
                self.ring.remove(&pos);
            }
        }
    }

    /// Check if a node is in the ring.
    pub fn contains_node(&self, node_id: NodeId) -> bool {
        self.node_vnodes.contains_key(&node_id)
    }

    /// Get the primary node for a key.
    pub fn get_node(&self, key: &str) -> Option<NodeId> {
        self.get_node_by_hash(Self::hash_key(key))
    }

    /// Get the primary node for a hash value.
    pub fn get_node_by_hash(&self, hash: u64) -> Option<NodeId> {
        if self.ring.is_empty() {
            return None;
        }

        // Find the first node with position >= hash
        let vnode = self
            .ring
            .range(hash..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, vnode)| vnode)?;

        Some(vnode.node_id)
    }

    /// Get N nodes for a key (for replication).
    pub fn get_nodes(&self, key: &str, count: usize) -> Vec<NodeId> {
        self.get_nodes_by_hash(Self::hash_key(key), count)
    }

    /// Get N nodes for a hash value (for replication).
    pub fn get_nodes_by_hash(&self, hash: u64, count: usize) -> Vec<NodeId> {
        if self.ring.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(count.min(self.node_vnodes.len()));
        let mut seen = std::collections::HashSet::new();

        // Start from the hash position and walk the ring
        let iter = self.ring.range(hash..).chain(self.ring.iter());

        for (_, vnode) in iter {
            if seen.insert(vnode.node_id) {
                result.push(vnode.node_id);
                if result.len() >= count {
                    break;
                }
            }
        }

        result
    }

    /// Get all nodes in the ring.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_vnodes.keys().copied()
    }

    /// Get the number of physical nodes.
    pub fn node_count(&self) -> usize {
        self.node_vnodes.len()
    }

    /// Get the number of virtual nodes.
    pub fn virtual_node_count(&self) -> usize {
        self.ring.len()
    }

    /// Hash a key to a position.
    pub fn hash_key(key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Calculate load distribution across nodes.
    pub fn load_distribution(&self) -> HashMap<NodeId, f64> {
        let total_vnodes = self.ring.len() as f64;
        if total_vnodes == 0.0 {
            return HashMap::new();
        }

        self.node_vnodes
            .iter()
            .map(|(&node_id, vnodes)| {
                let load = vnodes.len() as f64 / total_vnodes;
                (node_id, load)
            })
            .collect()
    }

    /// Get keys that would move if a node is added.
    pub fn keys_to_move_on_add(
        &self,
        new_node: NodeId,
        sample_keys: &[&str],
    ) -> Vec<(String, NodeId, NodeId)> {
        let mut moves = Vec::new();

        // Create a temporary ring with the new node
        let mut temp_ring = self.clone();
        temp_ring.add_node(new_node);

        for &key in sample_keys {
            let old_node = self.get_node(key);
            let new_owner = temp_ring.get_node(key);

            if old_node != new_owner {
                if let (Some(from), Some(to)) = (old_node, new_owner) {
                    moves.push((key.to_string(), from, to));
                }
            }
        }

        moves
    }
}

/// Consistent hash implementation using jump hash.
#[derive(Debug, Clone)]
pub struct ConsistentHash {
    /// Number of buckets.
    num_buckets: usize,
    /// Bucket to node mapping.
    buckets: Vec<NodeId>,
}

impl ConsistentHash {
    /// Create a new consistent hash with given nodes.
    pub fn new(nodes: &[NodeId]) -> Self {
        Self {
            num_buckets: nodes.len(),
            buckets: nodes.to_vec(),
        }
    }

    /// Get the node for a key using jump hash.
    pub fn get_node(&self, key: &str) -> Option<NodeId> {
        if self.buckets.is_empty() {
            return None;
        }

        let hash = HashRing::hash_key(key);
        let bucket = self.jump_hash(hash, self.num_buckets as u32) as usize;
        self.buckets.get(bucket).copied()
    }

    /// Jump consistent hash algorithm.
    fn jump_hash(&self, mut key: u64, num_buckets: u32) -> u32 {
        let mut b: i64 = -1;
        let mut j: i64 = 0;

        while j < num_buckets as i64 {
            b = j;
            key = key.wrapping_mul(2862933555777941757).wrapping_add(1);
            j = ((b.wrapping_add(1) as f64)
                * (((1u64 << 31) as f64) / (((key >> 33).wrapping_add(1)) as f64)))
                as i64;
        }

        b as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_node() {
        let vnode = VirtualNode::new(NodeId::new(1), 0);
        assert_eq!(vnode.node_id, NodeId::new(1));
        assert_eq!(vnode.index, 0);
    }

    #[test]
    fn test_hash_ring_add_remove() {
        let mut ring = HashRing::new(10);

        ring.add_node(NodeId::new(1));
        assert_eq!(ring.node_count(), 1);
        assert_eq!(ring.virtual_node_count(), 10);

        ring.add_node(NodeId::new(2));
        assert_eq!(ring.node_count(), 2);
        assert_eq!(ring.virtual_node_count(), 20);

        ring.remove_node(NodeId::new(1));
        assert_eq!(ring.node_count(), 1);
        assert_eq!(ring.virtual_node_count(), 10);
    }

    #[test]
    fn test_hash_ring_get_node() {
        let mut ring = HashRing::new(10);
        ring.add_node(NodeId::new(1));
        ring.add_node(NodeId::new(2));
        ring.add_node(NodeId::new(3));

        // Should return some node for any key
        let node = ring.get_node("test-key");
        assert!(node.is_some());

        // Same key should always return same node
        let node1 = ring.get_node("sandbox-123");
        let node2 = ring.get_node("sandbox-123");
        assert_eq!(node1, node2);
    }

    #[test]
    fn test_hash_ring_replication() {
        let mut ring = HashRing::new(10);
        ring.add_node(NodeId::new(1));
        ring.add_node(NodeId::new(2));
        ring.add_node(NodeId::new(3));

        let nodes = ring.get_nodes("test-key", 2);
        assert_eq!(nodes.len(), 2);

        // Nodes should be different
        assert_ne!(nodes[0], nodes[1]);
    }

    #[test]
    fn test_hash_ring_empty() {
        let ring = HashRing::new(10);
        assert!(ring.get_node("test").is_none());
        assert!(ring.get_nodes("test", 2).is_empty());
    }

    #[test]
    fn test_consistent_hash() {
        let nodes = vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)];
        let hash = ConsistentHash::new(&nodes);

        // Should return some node
        let node = hash.get_node("test-key");
        assert!(node.is_some());

        // Consistent results
        let node1 = hash.get_node("sandbox-123");
        let node2 = hash.get_node("sandbox-123");
        assert_eq!(node1, node2);
    }

    #[test]
    fn test_load_distribution() {
        let mut ring = HashRing::new(100);
        ring.add_node(NodeId::new(1));
        ring.add_node(NodeId::new(2));

        let dist = ring.load_distribution();
        assert_eq!(dist.len(), 2);

        // Each node should have roughly 50%
        for (_, load) in dist {
            assert!((load - 0.5).abs() < 0.01);
        }
    }
}
