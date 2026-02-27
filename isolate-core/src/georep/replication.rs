use super::region::RegionId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Configuration for replication behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationConfig {
    /// Minimum number of regions that must have the data.
    pub min_replicas: usize,
    /// Maximum age of a replica before it's considered stale (ms).
    pub max_staleness_ms: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self { min_replicas: 2, max_staleness_ms: 30_000 }
    }
}

/// Type of replicated item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicatedItemKind {
    Config,
    CompiledModule,
    Snapshot,
}

/// An item to be replicated across regions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicatedItem {
    pub id: String,
    pub kind: ReplicatedItemKind,
    pub content_hash: String,
    pub size_bytes: usize,
    pub created_epoch_ms: u64,
}

impl ReplicatedItem {
    pub fn config(id: impl Into<String>, content: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(content));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            id: id.into(),
            kind: ReplicatedItemKind::Config,
            content_hash: hash,
            size_bytes: content.len(),
            created_epoch_ms: now,
        }
    }

    pub fn module(id: impl Into<String>, content: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(content));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            id: id.into(),
            kind: ReplicatedItemKind::CompiledModule,
            content_hash: hash,
            size_bytes: content.len(),
            created_epoch_ms: now,
        }
    }
}

/// Replication state for a single item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationState {
    pub item_id: String,
    pub replicated_regions: HashSet<String>,
    pub pending_regions: HashSet<String>,
    pub last_replicated_epoch_ms: u64,
}

/// Tracks replication state across regions.
pub struct ReplicationTracker {
    config: ReplicationConfig,
    items: dashmap::DashMap<String, ReplicatedItem>,
    states: dashmap::DashMap<String, ReplicationState>,
}

impl ReplicationTracker {
    pub fn new(config: ReplicationConfig) -> Self {
        Self { config, items: dashmap::DashMap::new(), states: dashmap::DashMap::new() }
    }

    /// Add an item to be replicated.
    pub fn add_item(&self, item: ReplicatedItem) {
        let id = item.id.clone();
        self.items.insert(id.clone(), item);
        self.states.insert(
            id.clone(),
            ReplicationState {
                item_id: id,
                replicated_regions: HashSet::new(),
                pending_regions: HashSet::new(),
                last_replicated_epoch_ms: 0,
            },
        );
    }

    /// Mark an item as replicated to a region.
    pub fn mark_replicated(&self, item_id: &str, region: &RegionId) {
        if let Some(mut state) = self.states.get_mut(item_id) {
            state.replicated_regions.insert(region.as_str().to_string());
            state.pending_regions.remove(region.as_str());
            state.last_replicated_epoch_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
        }
    }

    /// Mark an item as pending replication to a region.
    pub fn mark_pending(&self, item_id: &str, region: &RegionId) {
        if let Some(mut state) = self.states.get_mut(item_id) {
            state.pending_regions.insert(region.as_str().to_string());
        }
    }

    /// Get the replication state for an item.
    pub fn get_state(&self, item_id: &str) -> Option<ReplicationState> {
        self.states.get(item_id).map(|s| s.value().clone())
    }

    /// Check if an item meets the minimum replica requirement.
    pub fn is_fully_replicated(&self, item_id: &str) -> bool {
        self.states
            .get(item_id)
            .map(|s| s.replicated_regions.len() >= self.config.min_replicas)
            .unwrap_or(false)
    }

    /// Get items that need more replicas.
    pub fn under_replicated(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|e| e.value().replicated_regions.len() < self.config.min_replicas)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Total number of tracked items.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
}

impl Default for ReplicationTracker {
    fn default() -> Self {
        Self::new(ReplicationConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_track() {
        let tracker = ReplicationTracker::new(ReplicationConfig::default());
        tracker.add_item(ReplicatedItem::config("cfg-1", b"data"));
        assert_eq!(tracker.item_count(), 1);
        assert!(!tracker.is_fully_replicated("cfg-1"));
    }

    #[test]
    fn test_replication_tracking() {
        let tracker = ReplicationTracker::new(ReplicationConfig {
            min_replicas: 2,
            max_staleness_ms: 30_000,
        });
        tracker.add_item(ReplicatedItem::config("cfg-1", b"data"));

        tracker.mark_replicated("cfg-1", &RegionId::new("us-east-1"));
        assert!(!tracker.is_fully_replicated("cfg-1"));

        tracker.mark_replicated("cfg-1", &RegionId::new("eu-west-1"));
        assert!(tracker.is_fully_replicated("cfg-1"));
    }

    #[test]
    fn test_under_replicated() {
        let tracker = ReplicationTracker::new(ReplicationConfig {
            min_replicas: 2,
            max_staleness_ms: 30_000,
        });
        tracker.add_item(ReplicatedItem::config("cfg-1", b"data1"));
        tracker.add_item(ReplicatedItem::config("cfg-2", b"data2"));

        tracker.mark_replicated("cfg-1", &RegionId::new("us-east-1"));
        tracker.mark_replicated("cfg-1", &RegionId::new("eu-west-1"));

        let under = tracker.under_replicated();
        assert_eq!(under.len(), 1);
        assert_eq!(under[0], "cfg-2");
    }

    #[test]
    fn test_pending_tracking() {
        let tracker = ReplicationTracker::new(ReplicationConfig::default());
        tracker.add_item(ReplicatedItem::module("mod-1", b"wasm"));

        tracker.mark_pending("mod-1", &RegionId::new("ap-southeast-1"));
        let state = tracker.get_state("mod-1").unwrap();
        assert!(state.pending_regions.contains("ap-southeast-1"));

        tracker.mark_replicated("mod-1", &RegionId::new("ap-southeast-1"));
        let state = tracker.get_state("mod-1").unwrap();
        assert!(!state.pending_regions.contains("ap-southeast-1"));
        assert!(state.replicated_regions.contains("ap-southeast-1"));
    }

    #[test]
    fn test_content_hash_deterministic() {
        let item1 = ReplicatedItem::config("a", b"same content");
        let item2 = ReplicatedItem::config("b", b"same content");
        assert_eq!(item1.content_hash, item2.content_hash);
    }
}
