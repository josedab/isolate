//! Channel hub for managing multiple IPC channels.

use super::channel::{Channel, ChannelConfig, ChannelError, ChannelId, ChannelStats};
use super::message::Message;
use dashmap::DashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Central hub for managing IPC channels.
pub struct ChannelHub {
    channels: Arc<DashMap<ChannelId, Channel>>,
    config: HubConfig,
}

/// Hub configuration.
#[derive(Debug, Clone)]
pub struct HubConfig {
    /// Maximum number of channels.
    pub max_channels: usize,
    /// Default channel capacity.
    pub default_capacity: usize,
    /// Whether to allow dynamic channel creation.
    pub allow_dynamic_creation: bool,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            max_channels: 1000,
            default_capacity: super::DEFAULT_CHANNEL_CAPACITY,
            allow_dynamic_creation: true,
        }
    }
}

impl HubConfig {
    /// Create a new hub configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum number of channels.
    pub fn with_max_channels(mut self, max: usize) -> Self {
        self.max_channels = max;
        self
    }

    /// Set the default channel capacity.
    pub fn with_default_capacity(mut self, capacity: usize) -> Self {
        self.default_capacity = capacity;
        self
    }

    /// Set whether to allow dynamic channel creation.
    pub fn with_allow_dynamic_creation(mut self, allow: bool) -> Self {
        self.allow_dynamic_creation = allow;
        self
    }
}

impl ChannelHub {
    /// Create a new channel hub.
    pub fn new() -> Self {
        Self::with_config(HubConfig::default())
    }

    /// Create a new channel hub with the given configuration.
    pub fn with_config(config: HubConfig) -> Self {
        Self { channels: Arc::new(DashMap::new()), config }
    }

    /// Get the hub configuration.
    pub fn config(&self) -> &HubConfig {
        &self.config
    }

    /// Create a new channel.
    pub fn create_channel(&self, config: ChannelConfig) -> Result<(), IpcError> {
        if self.channels.len() >= self.config.max_channels {
            return Err(IpcError::TooManyChannels { max: self.config.max_channels });
        }

        if self.channels.contains_key(&config.id) {
            return Err(IpcError::Channel(ChannelError::AlreadyExists(config.id)));
        }

        let id = config.id.clone();
        let channel = Channel::new(config);
        self.channels.insert(id, channel);

        Ok(())
    }

    /// Get or create a channel.
    pub fn get_or_create(&self, id: impl Into<ChannelId>) -> Result<Channel, IpcError> {
        let id = id.into();

        if let Some(channel) = self.channels.get(&id) {
            return Ok(channel.clone());
        }

        if !self.config.allow_dynamic_creation {
            return Err(IpcError::Channel(ChannelError::NotFound(id)));
        }

        let config = ChannelConfig::new(id.clone()).with_capacity(self.config.default_capacity);
        self.create_channel(config)?;

        // Channel was just inserted above; use expect for clarity
        Ok(self.channels.get(&id).expect("channel was just inserted").clone())
    }

    /// Get a channel by ID.
    pub fn get(&self, id: impl Into<ChannelId>) -> Option<Channel> {
        let id = id.into();
        self.channels.get(&id).map(|c| c.clone())
    }

    /// Remove a channel.
    pub fn remove(&self, id: impl Into<ChannelId>) -> bool {
        let id = id.into();
        self.channels.remove(&id).is_some()
    }

    /// Check if a channel exists.
    pub fn has(&self, id: impl Into<ChannelId>) -> bool {
        let id = id.into();
        self.channels.contains_key(&id)
    }

    /// Get all channel IDs.
    pub fn channel_ids(&self) -> Vec<ChannelId> {
        self.channels.iter().map(|r| r.key().clone()).collect()
    }

    /// Get the number of channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Send a message to a channel.
    pub fn send(&self, channel_id: impl Into<ChannelId>, message: Message) -> Result<(), IpcError> {
        let id = channel_id.into();
        let channel = self.get(&id).ok_or_else(|| IpcError::Channel(ChannelError::NotFound(id)))?;
        channel.send(message).map_err(IpcError::Channel)
    }

    /// Receive a message from a channel.
    pub fn receive(
        &self,
        channel_id: impl Into<ChannelId>,
        receiver: Option<Uuid>,
    ) -> Result<Option<Message>, IpcError> {
        let id = channel_id.into();
        let channel = self.get(&id).ok_or_else(|| IpcError::Channel(ChannelError::NotFound(id)))?;
        channel.receive(receiver).map_err(IpcError::Channel)
    }

    /// Broadcast a message to multiple channels.
    pub fn broadcast(
        &self,
        channel_ids: &[ChannelId],
        message: Message,
    ) -> Result<usize, IpcError> {
        let mut sent = 0;
        for id in channel_ids {
            if let Some(channel) = self.get(id) {
                // Clone message for each channel
                let msg = Message {
                    id: super::message::MessageId::new(),
                    sender: message.sender,
                    recipient: message.recipient,
                    payload: message.payload.clone(),
                    timestamp: message.timestamp,
                    correlation_id: message.correlation_id,
                    headers: message.headers.clone(),
                    ttl: message.ttl,
                };
                if channel.send(msg).is_ok() {
                    sent += 1;
                }
            }
        }
        Ok(sent)
    }

    /// Get statistics for all channels.
    pub fn all_stats(&self) -> Vec<(ChannelId, ChannelStats)> {
        self.channels.iter().map(|r| (r.key().clone(), r.value().stats())).collect()
    }

    /// Clear all messages from all channels.
    pub fn clear_all(&self) {
        for channel in self.channels.iter() {
            channel.clear();
        }
    }

    /// Remove all channels.
    pub fn remove_all(&self) {
        self.channels.clear();
    }
}

impl Default for ChannelHub {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ChannelHub {
    fn clone(&self) -> Self {
        Self { channels: Arc::clone(&self.channels), config: self.config.clone() }
    }
}

impl std::fmt::Debug for ChannelHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelHub")
            .field("channel_count", &self.channels.len())
            .field("config", &self.config)
            .finish()
    }
}

/// IPC-related errors.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Channel error.
    #[error(transparent)]
    Channel(#[from] ChannelError),

    /// Too many channels.
    #[error("Too many channels (max: {max})")]
    TooManyChannels { max: usize },

    /// Message serialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hub_creation() {
        let hub = ChannelHub::new();
        assert_eq!(hub.channel_count(), 0);
    }

    #[test]
    fn test_hub_create_channel() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("test")).unwrap();

        assert!(hub.has("test"));
        assert_eq!(hub.channel_count(), 1);
    }

    #[test]
    fn test_hub_get_or_create() {
        let hub = ChannelHub::new();

        // Creates channel
        let channel1 = hub.get_or_create("test").unwrap();
        assert_eq!(hub.channel_count(), 1);

        // Gets existing channel
        let channel2 = hub.get_or_create("test").unwrap();
        assert_eq!(hub.channel_count(), 1);

        // Same channel (shared state)
        channel1.send(Message::text("Hello")).unwrap();
        assert_eq!(channel2.len(), 1);
    }

    #[test]
    fn test_hub_get_or_create_disabled() {
        let hub = ChannelHub::with_config(HubConfig::new().with_allow_dynamic_creation(false));

        let result = hub.get_or_create("test");
        assert!(result.is_err());
    }

    #[test]
    fn test_hub_send_receive() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("test")).unwrap();

        hub.send("test", Message::text("Hello")).unwrap();
        let msg = hub.receive("test", None).unwrap().unwrap();

        assert_eq!(msg.payload.as_text(), Some("Hello"));
    }

    #[test]
    fn test_hub_send_not_found() {
        let hub = ChannelHub::new();

        let result = hub.send("missing", Message::text("Hello"));
        assert!(matches!(result, Err(IpcError::Channel(ChannelError::NotFound(_)))));
    }

    #[test]
    fn test_hub_remove_channel() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("test")).unwrap();

        assert!(hub.remove("test"));
        assert!(!hub.has("test"));
        assert!(!hub.remove("test")); // Already removed
    }

    #[test]
    fn test_hub_channel_ids() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();
        hub.create_channel(ChannelConfig::new("c")).unwrap();

        let ids = hub.channel_ids();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn test_hub_broadcast() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();
        hub.create_channel(ChannelConfig::new("c")).unwrap();

        let channels = vec![ChannelId::new("a"), ChannelId::new("b"), ChannelId::new("missing")];

        let sent = hub.broadcast(&channels, Message::text("Hello")).unwrap();
        assert_eq!(sent, 2); // a and b, not missing

        assert_eq!(hub.get("a").unwrap().len(), 1);
        assert_eq!(hub.get("b").unwrap().len(), 1);
    }

    #[test]
    fn test_hub_max_channels() {
        let hub = ChannelHub::with_config(HubConfig::new().with_max_channels(2));

        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();

        let result = hub.create_channel(ChannelConfig::new("c"));
        assert!(matches!(result, Err(IpcError::TooManyChannels { max: 2 })));
    }

    #[test]
    fn test_hub_duplicate_channel() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("test")).unwrap();

        let result = hub.create_channel(ChannelConfig::new("test"));
        assert!(matches!(result, Err(IpcError::Channel(ChannelError::AlreadyExists(_)))));
    }

    #[test]
    fn test_hub_all_stats() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();

        hub.send("a", Message::text("Hello")).unwrap();

        let stats = hub.all_stats();
        assert_eq!(stats.len(), 2);
    }

    #[test]
    fn test_hub_clear_all() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();

        hub.send("a", Message::text("1")).unwrap();
        hub.send("b", Message::text("2")).unwrap();

        hub.clear_all();

        assert!(hub.get("a").unwrap().is_empty());
        assert!(hub.get("b").unwrap().is_empty());
    }

    #[test]
    fn test_hub_remove_all() {
        let hub = ChannelHub::new();
        hub.create_channel(ChannelConfig::new("a")).unwrap();
        hub.create_channel(ChannelConfig::new("b")).unwrap();

        hub.remove_all();
        assert_eq!(hub.channel_count(), 0);
    }

    #[test]
    fn test_hub_clone_shares_state() {
        let hub1 = ChannelHub::new();
        let hub2 = hub1.clone();

        hub1.create_channel(ChannelConfig::new("test")).unwrap();
        assert!(hub2.has("test"));

        hub2.send("test", Message::text("Hello")).unwrap();
        let msg = hub1.receive("test", None).unwrap().unwrap();
        assert_eq!(msg.payload.as_text(), Some("Hello"));
    }
}
