//! Peer registry for the federated network.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Unique peer identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct PeerId(String);

impl PeerId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Peer health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerStatus {
    Active,
    Degraded,
    Unreachable,
    Banned,
}

/// Information about a peer node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub address: String,
    pub status: PeerStatus,
    pub last_seen: u64,
    pub modules_count: u32,
    pub bandwidth_score: f64,
}

impl PeerInfo {
    pub fn new(id: &str, address: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id: PeerId::new(id),
            address: address.to_string(),
            status: PeerStatus::Active,
            last_seen: now,
            modules_count: 0,
            bandwidth_score: 1.0,
        }
    }
}

/// Registry of known peers.
#[derive(Clone)]
pub struct PeerRegistry {
    inner: Arc<PeerRegistryInner>,
}

struct PeerRegistryInner {
    peers: RwLock<HashMap<PeerId, PeerInfo>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self { inner: Arc::new(PeerRegistryInner { peers: RwLock::new(HashMap::new()) }) }
    }

    /// Register a new peer.
    pub fn register(&self, info: PeerInfo) {
        self.inner.peers.write().insert(info.id.clone(), info);
    }

    /// Get peer info.
    pub fn get(&self, id: &PeerId) -> Option<PeerInfo> {
        self.inner.peers.read().get(id).cloned()
    }

    /// Update peer status.
    pub fn update_status(&self, id: &PeerId, status: PeerStatus) -> bool {
        let mut peers = self.inner.peers.write();
        if let Some(peer) = peers.get_mut(id) {
            peer.status = status;
            true
        } else {
            false
        }
    }

    /// Mark peer as seen (update last_seen timestamp).
    pub fn heartbeat(&self, id: &PeerId) -> bool {
        let mut peers = self.inner.peers.write();
        if let Some(peer) = peers.get_mut(id) {
            peer.last_seen = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            peer.status = PeerStatus::Active;
            true
        } else {
            false
        }
    }

    /// Get all active peers sorted by bandwidth score.
    pub fn active_peers(&self) -> Vec<PeerInfo> {
        let peers = self.inner.peers.read();
        let mut active: Vec<PeerInfo> =
            peers.values().filter(|p| p.status == PeerStatus::Active).cloned().collect();
        active.sort_by(|a, b| {
            b.bandwidth_score.partial_cmp(&a.bandwidth_score).unwrap_or(std::cmp::Ordering::Equal)
        });
        active
    }

    /// Remove a peer.
    pub fn remove(&self, id: &PeerId) -> bool {
        self.inner.peers.write().remove(id).is_some()
    }

    /// Count peers by status.
    pub fn count_by_status(&self, status: PeerStatus) -> usize {
        self.inner.peers.read().values().filter(|p| p.status == status).count()
    }

    /// Total peer count.
    pub fn count(&self) -> usize {
        self.inner.peers.read().len()
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        let peer = registry.get(&PeerId::new("p1")).unwrap();
        assert_eq!(peer.address, "10.0.0.1:9000");
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn test_update_status() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        registry.update_status(&PeerId::new("p1"), PeerStatus::Degraded);
        let peer = registry.get(&PeerId::new("p1")).unwrap();
        assert_eq!(peer.status, PeerStatus::Degraded);
    }

    #[test]
    fn test_active_peers() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        registry.register(PeerInfo::new("p2", "10.0.0.2:9000"));
        registry.update_status(&PeerId::new("p2"), PeerStatus::Unreachable);

        let active = registry.active_peers();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id.as_str(), "p1");
    }

    #[test]
    fn test_heartbeat() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        registry.update_status(&PeerId::new("p1"), PeerStatus::Degraded);
        registry.heartbeat(&PeerId::new("p1"));
        let peer = registry.get(&PeerId::new("p1")).unwrap();
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn test_remove_peer() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        assert_eq!(registry.count(), 1);
        assert!(registry.remove(&PeerId::new("p1")));
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_count_by_status() {
        let registry = PeerRegistry::new();
        registry.register(PeerInfo::new("p1", "10.0.0.1:9000"));
        registry.register(PeerInfo::new("p2", "10.0.0.2:9000"));
        registry.update_status(&PeerId::new("p2"), PeerStatus::Banned);

        assert_eq!(registry.count_by_status(PeerStatus::Active), 1);
        assert_eq!(registry.count_by_status(PeerStatus::Banned), 1);
    }
}
