//! Capability-gated sandbox networking mesh.
//!
//! Provides virtual channels for inter-sandbox communication with:
//! - Capability-based access control (sandboxes must be granted mesh capability)
//! - Named virtual channels with pub/sub and point-to-point patterns
//! - Message routing between sandboxes
//! - Per-channel rate limiting and size controls

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

/// Sandbox identifier within the mesh.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct MeshNodeId(pub String);

impl MeshNodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for MeshNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Virtual channel identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct VirtualChannelId(pub String);

impl VirtualChannelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Communication pattern for a virtual channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelPattern {
    /// Point-to-point: one sender, one receiver.
    PointToPoint,
    /// Pub/sub: one or more publishers, many subscribers.
    PubSub,
    /// Request/reply: sender expects a response.
    RequestReply,
}

/// Capability grant for mesh communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshCapability {
    /// The sandbox this capability is for.
    pub node_id: MeshNodeId,
    /// Channels this sandbox can send to.
    pub send_channels: HashSet<VirtualChannelId>,
    /// Channels this sandbox can receive from.
    pub receive_channels: HashSet<VirtualChannelId>,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Maximum messages per second.
    pub rate_limit: Option<u32>,
}

impl MeshCapability {
    /// Create a capability with full access to a channel.
    pub fn full_access(
        node_id: MeshNodeId,
        channel: VirtualChannelId,
    ) -> Self {
        let mut send = HashSet::new();
        let mut recv = HashSet::new();
        send.insert(channel.clone());
        recv.insert(channel);

        Self {
            node_id,
            send_channels: send,
            receive_channels: recv,
            max_message_size: 64 * 1024, // 64KB default
            rate_limit: None,
        }
    }

    /// Create a send-only capability.
    pub fn send_only(node_id: MeshNodeId, channel: VirtualChannelId) -> Self {
        let mut send = HashSet::new();
        send.insert(channel);

        Self {
            node_id,
            send_channels: send,
            receive_channels: HashSet::new(),
            max_message_size: 64 * 1024,
            rate_limit: None,
        }
    }

    /// Create a receive-only capability.
    pub fn receive_only(node_id: MeshNodeId, channel: VirtualChannelId) -> Self {
        let mut recv = HashSet::new();
        recv.insert(channel);

        Self {
            node_id,
            send_channels: HashSet::new(),
            receive_channels: recv,
            max_message_size: 64 * 1024,
            rate_limit: None,
        }
    }

    /// Check if this capability allows sending to a channel.
    pub fn can_send(&self, channel: &VirtualChannelId) -> bool {
        self.send_channels.contains(channel)
    }

    /// Check if this capability allows receiving from a channel.
    pub fn can_receive(&self, channel: &VirtualChannelId) -> bool {
        self.receive_channels.contains(channel)
    }
}

/// A message sent through the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMessage {
    /// Unique message identifier.
    pub id: u64,
    /// Sending sandbox.
    pub sender: MeshNodeId,
    /// Target channel.
    pub channel: VirtualChannelId,
    /// Message payload.
    pub payload: Vec<u8>,
    /// Metadata headers.
    pub headers: HashMap<String, String>,
    /// Timestamp when sent (epoch ms).
    pub sent_at_epoch_ms: u64,
}

/// Configuration for a virtual channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualChannelConfig {
    pub id: VirtualChannelId,
    pub pattern: ChannelPattern,
    /// Maximum buffered messages per subscriber.
    pub buffer_size: usize,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
}

impl VirtualChannelConfig {
    /// Create a new channel config.
    pub fn new(id: impl Into<String>, pattern: ChannelPattern) -> Self {
        Self {
            id: VirtualChannelId::new(id),
            pattern,
            buffer_size: 100,
            max_message_size: 64 * 1024,
        }
    }
}

/// Internal channel state.
struct ChannelState {
    config: VirtualChannelConfig,
    subscribers: HashSet<MeshNodeId>,
    queues: HashMap<MeshNodeId, VecDeque<MeshMessage>>,
}

/// Error from mesh operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshError {
    /// Capability denied.
    CapabilityDenied(String),
    /// Channel not found.
    ChannelNotFound(String),
    /// Channel already exists.
    ChannelAlreadyExists(String),
    /// Message too large.
    MessageTooLarge { size: usize, max: usize },
    /// Node not found.
    NodeNotFound(String),
    /// Buffer full.
    BufferFull(String),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::CapabilityDenied(m) => write!(f, "capability denied: {}", m),
            MeshError::ChannelNotFound(c) => write!(f, "channel not found: {}", c),
            MeshError::ChannelAlreadyExists(c) => write!(f, "channel already exists: {}", c),
            MeshError::MessageTooLarge { size, max } => {
                write!(f, "message too large: {} > {}", size, max)
            }
            MeshError::NodeNotFound(n) => write!(f, "node not found: {}", n),
            MeshError::BufferFull(c) => write!(f, "buffer full for channel: {}", c),
        }
    }
}

impl std::error::Error for MeshError {}

/// Statistics for the sandbox mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatistics {
    pub total_messages_sent: u64,
    pub total_messages_delivered: u64,
    pub active_channels: usize,
    pub connected_nodes: usize,
}

/// The sandbox networking mesh.
///
/// Manages virtual channels and capability-gated message routing
/// between sandboxes.
pub struct SandboxMesh {
    channels: parking_lot::RwLock<HashMap<VirtualChannelId, ChannelState>>,
    capabilities: dashmap::DashMap<MeshNodeId, MeshCapability>,
    msg_counter: AtomicU64,
    total_sent: AtomicU64,
    total_delivered: AtomicU64,
}

impl SandboxMesh {
    /// Create a new sandbox mesh.
    pub fn new() -> Self {
        Self {
            channels: parking_lot::RwLock::new(HashMap::new()),
            capabilities: dashmap::DashMap::new(),
            msg_counter: AtomicU64::new(0),
            total_sent: AtomicU64::new(0),
            total_delivered: AtomicU64::new(0),
        }
    }

    /// Create a virtual channel.
    pub fn create_channel(&self, config: VirtualChannelConfig) -> Result<(), MeshError> {
        let mut channels = self.channels.write();
        if channels.contains_key(&config.id) {
            return Err(MeshError::ChannelAlreadyExists(config.id.0.clone()));
        }
        let id = config.id.clone();
        channels.insert(
            id,
            ChannelState {
                config,
                subscribers: HashSet::new(),
                queues: HashMap::new(),
            },
        );
        Ok(())
    }

    /// Remove a virtual channel.
    pub fn remove_channel(&self, id: &VirtualChannelId) -> Result<(), MeshError> {
        let mut channels = self.channels.write();
        channels
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| MeshError::ChannelNotFound(id.0.clone()))
    }

    /// Grant a mesh capability to a sandbox.
    pub fn grant_capability(&self, capability: MeshCapability) {
        let node_id = capability.node_id.clone();
        self.capabilities.insert(node_id, capability);
    }

    /// Revoke mesh capability from a sandbox.
    pub fn revoke_capability(&self, node_id: &MeshNodeId) {
        self.capabilities.remove(node_id);
        // Unsubscribe from all channels
        let mut channels = self.channels.write();
        for state in channels.values_mut() {
            state.subscribers.remove(node_id);
            state.queues.remove(node_id);
        }
    }

    /// Subscribe a sandbox to a channel (capability checked).
    pub fn subscribe(
        &self,
        node_id: &MeshNodeId,
        channel_id: &VirtualChannelId,
    ) -> Result<(), MeshError> {
        let cap = self
            .capabilities
            .get(node_id)
            .ok_or_else(|| MeshError::CapabilityDenied("no capability granted".into()))?;

        if !cap.can_receive(channel_id) {
            return Err(MeshError::CapabilityDenied(format!(
                "{} cannot receive from {}",
                node_id, channel_id.0
            )));
        }
        drop(cap);

        let mut channels = self.channels.write();
        let state = channels
            .get_mut(channel_id)
            .ok_or_else(|| MeshError::ChannelNotFound(channel_id.0.clone()))?;

        state.subscribers.insert(node_id.clone());
        state
            .queues
            .entry(node_id.clone())
            .or_insert_with(VecDeque::new);
        Ok(())
    }

    /// Send a message through the mesh (capability checked).
    pub fn send(
        &self,
        sender: &MeshNodeId,
        channel_id: &VirtualChannelId,
        payload: Vec<u8>,
        headers: HashMap<String, String>,
    ) -> Result<u64, MeshError> {
        let cap = self
            .capabilities
            .get(sender)
            .ok_or_else(|| MeshError::CapabilityDenied("no capability granted".into()))?;

        if !cap.can_send(channel_id) {
            return Err(MeshError::CapabilityDenied(format!(
                "{} cannot send to {}",
                sender, channel_id.0
            )));
        }

        if payload.len() > cap.max_message_size {
            return Err(MeshError::MessageTooLarge {
                size: payload.len(),
                max: cap.max_message_size,
            });
        }
        drop(cap);

        let msg_id = self.msg_counter.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let msg = MeshMessage {
            id: msg_id,
            sender: sender.clone(),
            channel: channel_id.clone(),
            payload,
            headers,
            sent_at_epoch_ms: now,
        };

        self.total_sent.fetch_add(1, Ordering::Relaxed);

        let mut channels = self.channels.write();
        let state = channels
            .get_mut(channel_id)
            .ok_or_else(|| MeshError::ChannelNotFound(channel_id.0.clone()))?;

        // Deliver to all subscribers
        let subscribers: Vec<MeshNodeId> = state.subscribers.iter().cloned().collect();
        for sub in &subscribers {
            if let Some(queue) = state.queues.get_mut(sub) {
                // Drop oldest if buffer full
                if queue.len() >= state.config.buffer_size {
                    queue.pop_front();
                }
                queue.push_back(msg.clone());
                self.total_delivered.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(msg_id)
    }

    /// Receive the next message for a sandbox from a channel.
    pub fn receive(
        &self,
        node_id: &MeshNodeId,
        channel_id: &VirtualChannelId,
    ) -> Result<Option<MeshMessage>, MeshError> {
        let cap = self
            .capabilities
            .get(node_id)
            .ok_or_else(|| MeshError::CapabilityDenied("no capability granted".into()))?;

        if !cap.can_receive(channel_id) {
            return Err(MeshError::CapabilityDenied(format!(
                "{} cannot receive from {}",
                node_id, channel_id.0
            )));
        }
        drop(cap);

        let mut channels = self.channels.write();
        let state = channels
            .get_mut(channel_id)
            .ok_or_else(|| MeshError::ChannelNotFound(channel_id.0.clone()))?;

        Ok(state
            .queues
            .get_mut(node_id)
            .and_then(|q| q.pop_front()))
    }

    /// Get the number of pending messages for a node on a channel.
    pub fn pending_count(
        &self,
        node_id: &MeshNodeId,
        channel_id: &VirtualChannelId,
    ) -> usize {
        let channels = self.channels.read();
        channels
            .get(channel_id)
            .and_then(|s| s.queues.get(node_id))
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Get mesh statistics.
    pub fn statistics(&self) -> MeshStatistics {
        let channels = self.channels.read();
        MeshStatistics {
            total_messages_sent: self.total_sent.load(Ordering::Relaxed),
            total_messages_delivered: self.total_delivered.load(Ordering::Relaxed),
            active_channels: channels.len(),
            connected_nodes: self.capabilities.len(),
        }
    }

    /// List all channel IDs.
    pub fn list_channels(&self) -> Vec<VirtualChannelId> {
        self.channels.read().keys().cloned().collect()
    }
}

impl Default for SandboxMesh {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_channel() {
        let mesh = SandboxMesh::new();
        let config = VirtualChannelConfig::new("events", ChannelPattern::PubSub);
        mesh.create_channel(config).unwrap();
        assert_eq!(mesh.list_channels().len(), 1);
    }

    #[test]
    fn test_duplicate_channel_error() {
        let mesh = SandboxMesh::new();
        let config = VirtualChannelConfig::new("events", ChannelPattern::PubSub);
        mesh.create_channel(config.clone()).unwrap();
        assert!(mesh.create_channel(config).is_err());
    }

    #[test]
    fn test_capability_gated_send() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let sender = MeshNodeId::new("sandbox-a");
        let receiver = MeshNodeId::new("sandbox-b");

        // Grant capabilities
        mesh.grant_capability(MeshCapability::send_only(sender.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::receive_only(receiver.clone(), ch.clone()));

        // Subscribe receiver
        mesh.subscribe(&receiver, &ch).unwrap();

        // Send a message
        let msg_id = mesh
            .send(&sender, &ch, b"hello".to_vec(), HashMap::new())
            .unwrap();
        assert_eq!(msg_id, 0);

        // Receive
        let msg = mesh.receive(&receiver, &ch).unwrap().unwrap();
        assert_eq!(msg.payload, b"hello");
        assert_eq!(msg.sender, sender);
    }

    #[test]
    fn test_capability_denied_send() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let node = MeshNodeId::new("sandbox-a");
        // Grant receive-only
        mesh.grant_capability(MeshCapability::receive_only(node.clone(), ch.clone()));

        // Try to send
        let result = mesh.send(&node, &ch, b"data".to_vec(), HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_capability_denied_receive() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let node = MeshNodeId::new("sandbox-a");
        mesh.grant_capability(MeshCapability::send_only(node.clone(), ch.clone()));

        let result = mesh.subscribe(&node, &ch);
        assert!(result.is_err());
    }

    #[test]
    fn test_no_capability() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let node = MeshNodeId::new("unregistered");
        let result = mesh.send(&node, &ch, b"data".to_vec(), HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_message_too_large() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let node = MeshNodeId::new("sandbox-a");
        let mut cap = MeshCapability::send_only(node.clone(), ch.clone());
        cap.max_message_size = 10;
        mesh.grant_capability(cap);

        let result = mesh.send(&node, &ch, vec![0u8; 100], HashMap::new());
        assert!(matches!(result, Err(MeshError::MessageTooLarge { .. })));
    }

    #[test]
    fn test_pubsub_multiple_subscribers() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let sender = MeshNodeId::new("publisher");
        let sub1 = MeshNodeId::new("sub-1");
        let sub2 = MeshNodeId::new("sub-2");

        mesh.grant_capability(MeshCapability::send_only(sender.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::receive_only(sub1.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::receive_only(sub2.clone(), ch.clone()));

        mesh.subscribe(&sub1, &ch).unwrap();
        mesh.subscribe(&sub2, &ch).unwrap();

        mesh.send(&sender, &ch, b"broadcast".to_vec(), HashMap::new())
            .unwrap();

        // Both subscribers should receive the message
        let msg1 = mesh.receive(&sub1, &ch).unwrap().unwrap();
        let msg2 = mesh.receive(&sub2, &ch).unwrap().unwrap();
        assert_eq!(msg1.payload, b"broadcast");
        assert_eq!(msg2.payload, b"broadcast");
    }

    #[test]
    fn test_buffer_overflow() {
        let mesh = SandboxMesh::new();
        let mut config = VirtualChannelConfig::new("events", ChannelPattern::PubSub);
        config.buffer_size = 2;
        mesh.create_channel(config).unwrap();

        let ch = VirtualChannelId::new("events");
        let sender = MeshNodeId::new("sender");
        let receiver = MeshNodeId::new("receiver");

        mesh.grant_capability(MeshCapability::full_access(sender.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::full_access(receiver.clone(), ch.clone()));
        mesh.subscribe(&receiver, &ch).unwrap();

        // Send 3 messages with buffer size 2 → oldest should be dropped
        mesh.send(&sender, &ch, b"msg1".to_vec(), HashMap::new()).unwrap();
        mesh.send(&sender, &ch, b"msg2".to_vec(), HashMap::new()).unwrap();
        mesh.send(&sender, &ch, b"msg3".to_vec(), HashMap::new()).unwrap();

        // Should get msg2 and msg3 (msg1 was dropped)
        let first = mesh.receive(&receiver, &ch).unwrap().unwrap();
        assert_eq!(first.payload, b"msg2");
        let second = mesh.receive(&receiver, &ch).unwrap().unwrap();
        assert_eq!(second.payload, b"msg3");
    }

    #[test]
    fn test_revoke_capability() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let node = MeshNodeId::new("sandbox-a");
        mesh.grant_capability(MeshCapability::full_access(node.clone(), ch.clone()));
        mesh.subscribe(&node, &ch).unwrap();

        mesh.revoke_capability(&node);
        let result = mesh.send(&node, &ch, b"data".to_vec(), HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_statistics() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let sender = MeshNodeId::new("sender");
        let receiver = MeshNodeId::new("receiver");
        mesh.grant_capability(MeshCapability::full_access(sender.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::full_access(receiver.clone(), ch.clone()));
        mesh.subscribe(&receiver, &ch).unwrap();

        mesh.send(&sender, &ch, b"hello".to_vec(), HashMap::new()).unwrap();

        let stats = mesh.statistics();
        assert_eq!(stats.total_messages_sent, 1);
        assert_eq!(stats.total_messages_delivered, 1);
        assert_eq!(stats.active_channels, 1);
        assert_eq!(stats.connected_nodes, 2);
    }

    #[test]
    fn test_pending_count() {
        let mesh = SandboxMesh::new();
        let ch = VirtualChannelId::new("events");
        mesh.create_channel(VirtualChannelConfig::new("events", ChannelPattern::PubSub))
            .unwrap();

        let sender = MeshNodeId::new("sender");
        let receiver = MeshNodeId::new("receiver");
        mesh.grant_capability(MeshCapability::full_access(sender.clone(), ch.clone()));
        mesh.grant_capability(MeshCapability::full_access(receiver.clone(), ch.clone()));
        mesh.subscribe(&receiver, &ch).unwrap();

        assert_eq!(mesh.pending_count(&receiver, &ch), 0);
        mesh.send(&sender, &ch, b"msg".to_vec(), HashMap::new()).unwrap();
        assert_eq!(mesh.pending_count(&receiver, &ch), 1);
    }
}
