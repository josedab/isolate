//! IPC message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique message identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

impl MessageId {
    /// Generate a new random message ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Message payload types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MessagePayload {
    /// Text message.
    Text(String),
    /// Binary data.
    Binary(Vec<u8>),
    /// JSON value.
    Json(serde_json::Value),
    /// Empty message (signal).
    Empty,
}

impl MessagePayload {
    /// Create a text payload.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create a binary payload.
    pub fn binary(data: Vec<u8>) -> Self {
        Self::Binary(data)
    }

    /// Create a JSON payload.
    pub fn json(value: serde_json::Value) -> Self {
        Self::Json(value)
    }

    /// Create an empty payload.
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Get the payload as text if it's a text payload.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get the payload as binary if it's a binary payload.
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Self::Binary(b) => Some(b),
            _ => None,
        }
    }

    /// Get the payload as JSON if it's a JSON payload.
    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Json(v) => Some(v),
            _ => None,
        }
    }

    /// Check if the payload is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Get the size of the payload in bytes.
    pub fn size(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
            Self::Binary(b) => b.len(),
            Self::Json(v) => serde_json::to_string(v).map(|s| s.len()).unwrap_or(0),
            Self::Empty => 0,
        }
    }
}

/// An IPC message between sandboxes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message ID.
    pub id: MessageId,
    /// Sender sandbox ID (if known).
    pub sender: Option<Uuid>,
    /// Recipient sandbox ID (if targeted).
    pub recipient: Option<Uuid>,
    /// Message payload.
    pub payload: MessagePayload,
    /// Timestamp when the message was created.
    pub timestamp: DateTime<Utc>,
    /// Optional correlation ID for request/response patterns.
    pub correlation_id: Option<MessageId>,
    /// Optional message headers.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub headers: std::collections::HashMap<String, String>,
    /// Time-to-live in seconds (0 = no expiration).
    #[serde(default)]
    pub ttl: u32,
}

impl Message {
    /// Create a new message with the given payload.
    pub fn new(payload: MessagePayload) -> Self {
        Self {
            id: MessageId::new(),
            sender: None,
            recipient: None,
            payload,
            timestamp: Utc::now(),
            correlation_id: None,
            headers: std::collections::HashMap::new(),
            ttl: 0,
        }
    }

    /// Create a text message.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new(MessagePayload::text(text))
    }

    /// Create a binary message.
    pub fn binary(data: Vec<u8>) -> Self {
        Self::new(MessagePayload::binary(data))
    }

    /// Create a JSON message.
    pub fn json(value: serde_json::Value) -> Self {
        Self::new(MessagePayload::json(value))
    }

    /// Create an empty signal message.
    pub fn signal() -> Self {
        Self::new(MessagePayload::empty())
    }

    /// Set the sender sandbox ID.
    pub fn with_sender(mut self, sender: Uuid) -> Self {
        self.sender = Some(sender);
        self
    }

    /// Set the recipient sandbox ID.
    pub fn with_recipient(mut self, recipient: Uuid) -> Self {
        self.recipient = Some(recipient);
        self
    }

    /// Set a correlation ID.
    pub fn with_correlation_id(mut self, id: MessageId) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Set a header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Set the time-to-live in seconds.
    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }

    /// Get a header value.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key).map(|s| s.as_str())
    }

    /// Check if the message has expired.
    pub fn is_expired(&self) -> bool {
        if self.ttl == 0 {
            return false;
        }
        let age = Utc::now().signed_duration_since(self.timestamp);
        age.num_seconds() > self.ttl as i64
    }

    /// Get the message size in bytes.
    pub fn size(&self) -> usize {
        // Approximate size including overhead
        self.payload.size()
            + 16 // UUID
            + 8  // timestamp
            + self.headers.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
    }

    /// Create a reply message.
    pub fn reply(&self, payload: MessagePayload) -> Self {
        let mut reply = Self::new(payload);
        reply.correlation_id = Some(self.id);
        reply.recipient = self.sender;
        reply
    }
}

impl Default for Message {
    fn default() -> Self {
        Self::signal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id() {
        let id1 = MessageId::new();
        let id2 = MessageId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_message_payload_text() {
        let payload = MessagePayload::text("Hello");
        assert_eq!(payload.as_text(), Some("Hello"));
        assert!(payload.as_binary().is_none());
        assert_eq!(payload.size(), 5);
    }

    #[test]
    fn test_message_payload_binary() {
        let payload = MessagePayload::binary(vec![1, 2, 3, 4]);
        assert!(payload.as_text().is_none());
        assert_eq!(payload.as_binary(), Some(&[1, 2, 3, 4][..]));
        assert_eq!(payload.size(), 4);
    }

    #[test]
    fn test_message_payload_json() {
        let value = serde_json::json!({"key": "value"});
        let payload = MessagePayload::json(value.clone());
        assert_eq!(payload.as_json(), Some(&value));
    }

    #[test]
    fn test_message_payload_empty() {
        let payload = MessagePayload::empty();
        assert!(payload.is_empty());
        assert_eq!(payload.size(), 0);
    }

    #[test]
    fn test_message_creation() {
        let msg = Message::text("Hello, World!");

        assert!(!msg.id.0.is_nil());
        assert!(msg.sender.is_none());
        assert!(msg.recipient.is_none());
        assert_eq!(msg.payload.as_text(), Some("Hello, World!"));
        assert!(msg.correlation_id.is_none());
        assert!(!msg.is_expired());
    }

    #[test]
    fn test_message_builder() {
        let sender = Uuid::new_v4();
        let recipient = Uuid::new_v4();

        let msg = Message::text("Test")
            .with_sender(sender)
            .with_recipient(recipient)
            .with_header("content-type", "text/plain")
            .with_ttl(60);

        assert_eq!(msg.sender, Some(sender));
        assert_eq!(msg.recipient, Some(recipient));
        assert_eq!(msg.header("content-type"), Some("text/plain"));
        assert_eq!(msg.ttl, 60);
    }

    #[test]
    fn test_message_reply() {
        let sender = Uuid::new_v4();
        let original = Message::text("Request").with_sender(sender);
        let reply = original.reply(MessagePayload::text("Response"));

        assert_eq!(reply.correlation_id, Some(original.id));
        assert_eq!(reply.recipient, Some(sender));
    }

    #[test]
    fn test_message_expiration() {
        // Non-expiring message
        let msg = Message::text("Test");
        assert!(!msg.is_expired());

        // Message with TTL 0 never expires
        let msg = Message::text("Test").with_ttl(0);
        assert!(!msg.is_expired());
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::text("Hello").with_header("key", "value");

        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();

        assert_eq!(msg.id, parsed.id);
        assert_eq!(msg.payload.as_text(), parsed.payload.as_text());
        assert_eq!(msg.header("key"), parsed.header("key"));
    }
}
