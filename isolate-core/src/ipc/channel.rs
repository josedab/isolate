//! IPC channel implementation.

use super::message::Message;
use super::{DEFAULT_CHANNEL_CAPACITY, MAX_MESSAGE_SIZE};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

/// Unique channel identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChannelId(pub String);

impl ChannelId {
    /// Create a new channel ID from a string.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ChannelId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ChannelId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&ChannelId> for ChannelId {
    fn from(id: &ChannelId) -> Self {
        id.clone()
    }
}

/// Channel configuration.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Channel name/ID.
    pub id: ChannelId,
    /// Maximum number of messages in the queue.
    pub capacity: usize,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Whether the channel persists messages.
    pub persistent: bool,
    /// Whether to drop old messages when full (vs blocking).
    pub drop_oldest: bool,
    /// Allowed sender sandbox IDs (empty = all allowed).
    pub allowed_senders: Vec<Uuid>,
    /// Allowed receiver sandbox IDs (empty = all allowed).
    pub allowed_receivers: Vec<Uuid>,
    /// Channel description.
    pub description: Option<String>,
}

impl ChannelConfig {
    /// Create a new channel configuration.
    pub fn new(id: impl Into<ChannelId>) -> Self {
        Self {
            id: id.into(),
            capacity: DEFAULT_CHANNEL_CAPACITY,
            max_message_size: MAX_MESSAGE_SIZE,
            persistent: false,
            drop_oldest: true,
            allowed_senders: Vec::new(),
            allowed_receivers: Vec::new(),
            description: None,
        }
    }

    /// Set the channel capacity.
    pub fn with_capacity(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        self
    }

    /// Set the maximum message size.
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Set whether the channel is persistent.
    pub fn with_persistent(mut self, persistent: bool) -> Self {
        self.persistent = persistent;
        self
    }

    /// Set the drop policy when full.
    pub fn with_drop_oldest(mut self, drop: bool) -> Self {
        self.drop_oldest = drop;
        self
    }

    /// Add an allowed sender.
    pub fn allow_sender(mut self, sandbox_id: Uuid) -> Self {
        self.allowed_senders.push(sandbox_id);
        self
    }

    /// Add an allowed receiver.
    pub fn allow_receiver(mut self, sandbox_id: Uuid) -> Self {
        self.allowed_receivers.push(sandbox_id);
        self
    }

    /// Set the channel description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Channel statistics.
#[derive(Debug, Clone, Default)]
pub struct ChannelStats {
    /// Total messages sent.
    pub messages_sent: u64,
    /// Total messages received.
    pub messages_received: u64,
    /// Total messages dropped.
    pub messages_dropped: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Current queue depth.
    pub queue_depth: usize,
    /// Peak queue depth.
    pub peak_queue_depth: usize,
    /// Channel creation time.
    pub created_at: Option<DateTime<Utc>>,
    /// Last message time.
    pub last_message_at: Option<DateTime<Utc>>,
}

/// Internal channel state.
struct ChannelState {
    messages: VecDeque<Message>,
    stats: ChannelStats,
}

/// A message channel for inter-sandbox communication.
pub struct Channel {
    config: ChannelConfig,
    state: Arc<RwLock<ChannelState>>,
}

impl Channel {
    /// Create a new channel with the given configuration.
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(ChannelState {
                messages: VecDeque::new(),
                stats: ChannelStats { created_at: Some(Utc::now()), ..Default::default() },
            })),
        }
    }

    /// Get the channel ID.
    pub fn id(&self) -> &ChannelId {
        &self.config.id
    }

    /// Get the channel configuration.
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    /// Check if a sender is allowed.
    pub fn is_sender_allowed(&self, sender: Option<Uuid>) -> bool {
        if self.config.allowed_senders.is_empty() {
            return true;
        }
        match sender {
            Some(id) => self.config.allowed_senders.contains(&id),
            None => false,
        }
    }

    /// Check if a receiver is allowed.
    pub fn is_receiver_allowed(&self, receiver: Option<Uuid>) -> bool {
        if self.config.allowed_receivers.is_empty() {
            return true;
        }
        match receiver {
            Some(id) => self.config.allowed_receivers.contains(&id),
            None => false,
        }
    }

    /// Send a message to the channel.
    pub fn send(&self, message: Message) -> Result<(), ChannelError> {
        // Check sender permission
        if !self.is_sender_allowed(message.sender) {
            return Err(ChannelError::PermissionDenied {
                channel: self.config.id.clone(),
                reason: "Sender not allowed".to_string(),
            });
        }

        // Check message size
        let size = message.size();
        if size > self.config.max_message_size {
            return Err(ChannelError::MessageTooLarge { size, max: self.config.max_message_size });
        }

        let mut state = self.state.write();

        // Handle capacity
        if state.messages.len() >= self.config.capacity {
            if self.config.drop_oldest {
                state.messages.pop_front();
                state.stats.messages_dropped += 1;
            } else {
                return Err(ChannelError::ChannelFull { channel: self.config.id.clone() });
            }
        }

        // Add message
        state.messages.push_back(message);
        state.stats.messages_sent += 1;
        state.stats.bytes_sent += size as u64;
        state.stats.queue_depth = state.messages.len();
        state.stats.peak_queue_depth = state.stats.peak_queue_depth.max(state.stats.queue_depth);
        state.stats.last_message_at = Some(Utc::now());

        Ok(())
    }

    /// Receive a message from the channel.
    pub fn receive(&self, receiver: Option<Uuid>) -> Result<Option<Message>, ChannelError> {
        // Check receiver permission
        if !self.is_receiver_allowed(receiver) {
            return Err(ChannelError::PermissionDenied {
                channel: self.config.id.clone(),
                reason: "Receiver not allowed".to_string(),
            });
        }

        let mut state = self.state.write();

        // Remove expired messages
        while let Some(front) = state.messages.front() {
            if front.is_expired() {
                state.messages.pop_front();
                state.stats.messages_dropped += 1;
            } else {
                break;
            }
        }

        // Get next message
        if let Some(message) = state.messages.pop_front() {
            state.stats.messages_received += 1;
            state.stats.bytes_received += message.size() as u64;
            state.stats.queue_depth = state.messages.len();
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    /// Peek at the next message without removing it.
    pub fn peek(&self, receiver: Option<Uuid>) -> Result<Option<Message>, ChannelError> {
        if !self.is_receiver_allowed(receiver) {
            return Err(ChannelError::PermissionDenied {
                channel: self.config.id.clone(),
                reason: "Receiver not allowed".to_string(),
            });
        }

        let state = self.state.read();
        Ok(state.messages.front().cloned())
    }

    /// Get the current queue depth.
    pub fn len(&self) -> usize {
        self.state.read().messages.len()
    }

    /// Check if the channel is empty.
    pub fn is_empty(&self) -> bool {
        self.state.read().messages.is_empty()
    }

    /// Get channel statistics.
    pub fn stats(&self) -> ChannelStats {
        self.state.read().stats.clone()
    }

    /// Clear all messages from the channel.
    pub fn clear(&self) {
        let mut state = self.state.write();
        let dropped = state.messages.len() as u64;
        state.messages.clear();
        state.stats.messages_dropped += dropped;
        state.stats.queue_depth = 0;
    }
}

impl Clone for Channel {
    fn clone(&self) -> Self {
        Self { config: self.config.clone(), state: Arc::clone(&self.state) }
    }
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("id", &self.config.id)
            .field("capacity", &self.config.capacity)
            .field("len", &self.len())
            .finish()
    }
}

/// Channel-related errors.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// Channel is full.
    #[error("Channel '{channel}' is full")]
    ChannelFull { channel: ChannelId },

    /// Message too large.
    #[error("Message size {size} exceeds maximum {max}")]
    MessageTooLarge { size: usize, max: usize },

    /// Permission denied.
    #[error("Permission denied on channel '{channel}': {reason}")]
    PermissionDenied { channel: ChannelId, reason: String },

    /// Channel not found.
    #[error("Channel '{0}' not found")]
    NotFound(ChannelId),

    /// Channel already exists.
    #[error("Channel '{0}' already exists")]
    AlreadyExists(ChannelId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_id() {
        let id = ChannelId::new("test");
        assert_eq!(id.0, "test");
        assert_eq!(id.to_string(), "test");
    }

    #[test]
    fn test_channel_config() {
        let config = ChannelConfig::new("events")
            .with_capacity(500)
            .with_max_message_size(1024)
            .with_description("Event channel");

        assert_eq!(config.id.0, "events");
        assert_eq!(config.capacity, 500);
        assert_eq!(config.max_message_size, 1024);
        assert_eq!(config.description, Some("Event channel".to_string()));
    }

    #[test]
    fn test_channel_send_receive() {
        let channel = Channel::new(ChannelConfig::new("test"));

        let msg = Message::text("Hello");
        channel.send(msg).unwrap();

        assert_eq!(channel.len(), 1);

        let received = channel.receive(None).unwrap().unwrap();
        assert_eq!(received.payload.as_text(), Some("Hello"));
        assert!(channel.is_empty());
    }

    #[test]
    fn test_channel_peek() {
        let channel = Channel::new(ChannelConfig::new("test"));

        channel.send(Message::text("Hello")).unwrap();

        let peeked = channel.peek(None).unwrap().unwrap();
        assert_eq!(peeked.payload.as_text(), Some("Hello"));
        assert_eq!(channel.len(), 1); // Still there

        let received = channel.receive(None).unwrap().unwrap();
        assert_eq!(received.payload.as_text(), Some("Hello"));
        assert!(channel.is_empty());
    }

    #[test]
    fn test_channel_capacity() {
        let channel = Channel::new(ChannelConfig::new("test").with_capacity(2));

        channel.send(Message::text("1")).unwrap();
        channel.send(Message::text("2")).unwrap();
        channel.send(Message::text("3")).unwrap(); // Should drop "1"

        assert_eq!(channel.len(), 2);

        let msg = channel.receive(None).unwrap().unwrap();
        assert_eq!(msg.payload.as_text(), Some("2"));
    }

    #[test]
    fn test_channel_capacity_no_drop() {
        let channel =
            Channel::new(ChannelConfig::new("test").with_capacity(2).with_drop_oldest(false));

        channel.send(Message::text("1")).unwrap();
        channel.send(Message::text("2")).unwrap();

        let result = channel.send(Message::text("3"));
        assert!(matches!(result, Err(ChannelError::ChannelFull { .. })));
    }

    #[test]
    fn test_channel_message_too_large() {
        let channel = Channel::new(ChannelConfig::new("test").with_max_message_size(10));

        let result = channel.send(Message::text("This is a very long message"));
        assert!(matches!(result, Err(ChannelError::MessageTooLarge { .. })));
    }

    #[test]
    fn test_channel_sender_permission() {
        let allowed = Uuid::new_v4();
        let denied = Uuid::new_v4();

        let channel = Channel::new(ChannelConfig::new("test").allow_sender(allowed));

        // Allowed sender
        let msg = Message::text("Hello").with_sender(allowed);
        assert!(channel.send(msg).is_ok());

        // Denied sender
        let msg = Message::text("Hello").with_sender(denied);
        assert!(matches!(channel.send(msg), Err(ChannelError::PermissionDenied { .. })));
    }

    #[test]
    fn test_channel_receiver_permission() {
        let allowed = Uuid::new_v4();
        let denied = Uuid::new_v4();

        let channel = Channel::new(ChannelConfig::new("test").allow_receiver(allowed));
        channel.send(Message::text("Hello")).unwrap();

        // Allowed receiver
        assert!(channel.receive(Some(allowed)).is_ok());

        // Denied receiver
        channel.send(Message::text("Hello")).unwrap();
        assert!(matches!(
            channel.receive(Some(denied)),
            Err(ChannelError::PermissionDenied { .. })
        ));
    }

    #[test]
    fn test_channel_stats() {
        let channel = Channel::new(ChannelConfig::new("test"));

        channel.send(Message::text("Hello")).unwrap();
        channel.send(Message::text("World")).unwrap();
        channel.receive(None).unwrap();

        let stats = channel.stats();
        assert_eq!(stats.messages_sent, 2);
        assert_eq!(stats.messages_received, 1);
        assert_eq!(stats.queue_depth, 1);
        assert!(stats.created_at.is_some());
    }

    #[test]
    fn test_channel_clear() {
        let channel = Channel::new(ChannelConfig::new("test"));

        channel.send(Message::text("1")).unwrap();
        channel.send(Message::text("2")).unwrap();
        assert_eq!(channel.len(), 2);

        channel.clear();
        assert!(channel.is_empty());

        let stats = channel.stats();
        assert_eq!(stats.messages_dropped, 2);
    }

    #[test]
    fn test_channel_clone_shares_state() {
        let channel1 = Channel::new(ChannelConfig::new("test"));
        let channel2 = channel1.clone();

        channel1.send(Message::text("Hello")).unwrap();
        assert_eq!(channel2.len(), 1);

        let msg = channel2.receive(None).unwrap().unwrap();
        assert_eq!(msg.payload.as_text(), Some("Hello"));
        assert!(channel1.is_empty());
    }
}
