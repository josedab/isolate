//! Gossip protocol for metadata propagation.

use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::content::ContentId;

/// A module announcement to propagate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleAnnouncement {
    pub module_name: String,
    pub cid: ContentId,
    pub size_bytes: usize,
    pub publisher: String,
    pub version: String,
    pub timestamp: u64,
}

/// Types of gossip messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    /// Announce a new module.
    Announce(ModuleAnnouncement),
    /// Request a specific module.
    Request { cid: ContentId },
    /// Peer discovery (I'm alive).
    Ping { peer_id: String },
    /// Response to ping.
    Pong { peer_id: String, modules_count: u32 },
    /// Withdraw a module (delete/unpublish).
    Withdraw { cid: ContentId, reason: String },
}

impl GossipMessage {
    pub fn message_type(&self) -> &'static str {
        match self {
            Self::Announce(_) => "announce",
            Self::Request { .. } => "request",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
            Self::Withdraw { .. } => "withdraw",
        }
    }
}

/// Gossip protocol engine.
#[derive(Clone)]
pub struct GossipProtocol {
    inner: Arc<GossipInner>,
}

struct GossipInner {
    fanout: u32,
    known_cids: RwLock<HashSet<String>>,
    outbox: RwLock<Vec<GossipMessage>>,
    received: RwLock<Vec<GossipMessage>>,
}

impl GossipProtocol {
    /// Create with given fanout (number of peers to propagate to).
    pub fn new(fanout: u32) -> Self {
        Self {
            inner: Arc::new(GossipInner {
                fanout,
                known_cids: RwLock::new(HashSet::new()),
                outbox: RwLock::new(Vec::new()),
                received: RwLock::new(Vec::new()),
            }),
        }
    }

    /// Create a module announcement.
    pub fn create_announcement(
        &self,
        name: &str,
        cid: &ContentId,
        size: usize,
    ) -> ModuleAnnouncement {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        ModuleAnnouncement {
            module_name: name.to_string(),
            cid: cid.clone(),
            size_bytes: size,
            publisher: String::new(),
            version: "1.0.0".to_string(),
            timestamp: ts,
        }
    }

    /// Queue a message for propagation.
    pub fn broadcast(&self, msg: GossipMessage) {
        // Track CIDs to avoid re-propagating
        match &msg {
            GossipMessage::Announce(ann) => {
                let cid_str = ann.cid.as_str().to_string();
                if !self.inner.known_cids.write().insert(cid_str) {
                    return; // Already known, don't re-broadcast
                }
            }
            _ => {}
        }
        self.inner.outbox.write().push(msg);
    }

    /// Receive a message from a peer.
    pub fn receive(&self, msg: GossipMessage) -> bool {
        // Check if we've already seen this announcement
        if let GossipMessage::Announce(ann) = &msg {
            if self.inner.known_cids.read().contains(ann.cid.as_str()) {
                return false; // Duplicate
            }
            self.inner.known_cids.write().insert(ann.cid.as_str().to_string());
        }

        self.inner.received.write().push(msg);
        true
    }

    /// Drain outbox (messages to send to peers).
    pub fn drain_outbox(&self) -> Vec<GossipMessage> {
        let mut outbox = self.inner.outbox.write();
        std::mem::take(&mut *outbox)
    }

    /// Get received messages.
    pub fn drain_received(&self) -> Vec<GossipMessage> {
        let mut received = self.inner.received.write();
        std::mem::take(&mut *received)
    }

    /// Number of known CIDs.
    pub fn known_count(&self) -> usize {
        self.inner.known_cids.read().len()
    }

    /// Outbox size.
    pub fn outbox_size(&self) -> usize {
        self.inner.outbox.read().len()
    }

    /// Fanout value.
    pub fn fanout(&self) -> u32 {
        self.inner.fanout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_announcement() {
        let gossip = GossipProtocol::new(3);
        let cid = ContentId::from_bytes(b"module data");
        let ann = gossip.create_announcement("test.wasm", &cid, 100);
        gossip.broadcast(GossipMessage::Announce(ann));

        assert_eq!(gossip.outbox_size(), 1);
        assert_eq!(gossip.known_count(), 1);
    }

    #[test]
    fn test_duplicate_suppression() {
        let gossip = GossipProtocol::new(3);
        let cid = ContentId::from_bytes(b"module");
        let ann1 = gossip.create_announcement("test.wasm", &cid, 100);
        let ann2 = gossip.create_announcement("test.wasm", &cid, 100);

        gossip.broadcast(GossipMessage::Announce(ann1));
        gossip.broadcast(GossipMessage::Announce(ann2)); // duplicate

        assert_eq!(gossip.outbox_size(), 1); // only one queued
    }

    #[test]
    fn test_receive_new_message() {
        let gossip = GossipProtocol::new(3);
        let cid = ContentId::from_bytes(b"new module");
        let ann = ModuleAnnouncement {
            module_name: "new.wasm".into(),
            cid,
            size_bytes: 50,
            publisher: "alice".into(),
            version: "1.0.0".into(),
            timestamp: 1000,
        };

        assert!(gossip.receive(GossipMessage::Announce(ann)));
        let received = gossip.drain_received();
        assert_eq!(received.len(), 1);
    }

    #[test]
    fn test_receive_duplicate() {
        let gossip = GossipProtocol::new(3);
        let cid = ContentId::from_bytes(b"module");
        let ann1 = gossip.create_announcement("a.wasm", &cid, 10);
        let ann2 = gossip.create_announcement("a.wasm", &cid, 10);

        assert!(gossip.receive(GossipMessage::Announce(ann1)));
        assert!(!gossip.receive(GossipMessage::Announce(ann2))); // dup
    }

    #[test]
    fn test_drain_outbox() {
        let gossip = GossipProtocol::new(3);
        gossip.broadcast(GossipMessage::Ping { peer_id: "p1".into() });
        gossip.broadcast(GossipMessage::Ping { peer_id: "p2".into() });

        let messages = gossip.drain_outbox();
        assert_eq!(messages.len(), 2);
        assert_eq!(gossip.outbox_size(), 0); // drained
    }

    #[test]
    fn test_message_types() {
        assert_eq!(GossipMessage::Ping { peer_id: "p".into() }.message_type(), "ping");
        assert_eq!(
            GossipMessage::Pong { peer_id: "p".into(), modules_count: 5 }.message_type(),
            "pong"
        );
        assert_eq!(GossipMessage::Request { cid: ContentId::new("cid") }.message_type(), "request");
        assert_eq!(
            GossipMessage::Withdraw { cid: ContentId::new("cid"), reason: "".into() }
                .message_type(),
            "withdraw"
        );
    }

    #[test]
    fn test_fanout() {
        let gossip = GossipProtocol::new(5);
        assert_eq!(gossip.fanout(), 5);
    }
}
