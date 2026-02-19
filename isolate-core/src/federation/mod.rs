//! Federated Module Registry.
//!
//! Decentralized module distribution using content-addressed storage
//! with peer discovery, gossip-based metadata sync, and local caching.
//!
//! # Features
//!
//! - **Content-Addressed Storage**: SHA-256 based CID for integrity
//! - **Peer Registry**: Distributed peer tracking and health
//! - **Gossip Protocol**: Metadata propagation across peers
//! - **Local Cache**: LRU eviction with configurable limits



pub mod cache;
pub mod content;
pub mod gossip;
pub mod peers;

pub use cache::{ModuleCache, CacheConfig, CacheEntry, CacheStats};
pub use content::{ContentId, ContentStore, StoredModule};
pub use gossip::{GossipMessage, GossipProtocol, ModuleAnnouncement};
pub use peers::{PeerRegistry, PeerId, PeerInfo, PeerStatus};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federation_flow() {
        // Store a module
        let store = ContentStore::new();
        let cid = store.store(b"(module)", "test.wasm");
        assert_eq!(cid.as_str().len(), 64); // SHA-256 hex

        // Register peers
        let peers = PeerRegistry::new();
        peers.register(PeerInfo::new("peer-1", "192.168.1.1:9000"));
        peers.register(PeerInfo::new("peer-2", "192.168.1.2:9000"));

        // Announce via gossip
        let gossip = GossipProtocol::new(3);
        let msg = gossip.create_announcement("test.wasm", &cid, 8);
        assert_eq!(msg.module_name, "test.wasm");

        // Cache locally
        let cache = ModuleCache::new(CacheConfig::default());
        cache.put(&cid, b"(module)", "test.wasm");
        assert!(cache.get(&cid).is_some());
    }
}
