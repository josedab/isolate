//! Inter-sandbox communication (IPC).
//!
//! This module provides mechanisms for sandboxes to communicate with each other
//! through message-passing channels.
//!
//! # Features
//!
//! - **Channels**: Named message channels between sandboxes
//! - **Message Types**: Typed messages with serialization
//! - **Permissions**: Fine-grained send/receive permissions
//! - **Buffering**: Configurable message queue capacity

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.

//! # Example
//!
//! ```rust,ignore
//! use isolate_core::ipc::{ChannelHub, ChannelConfig, Message};
//!
//! let hub = ChannelHub::new();
//!
//! // Create a channel
//! let config = ChannelConfig::new("events")
//!     .with_capacity(100);
//! hub.create_channel(config)?;
//!
//! // Send a message
//! let msg = Message::text("Hello from sandbox A");
//! hub.send("events", msg)?;
//!
//! // Receive a message
//! let received = hub.receive("events")?;
//! ```

mod channel;
mod hub;
mod message;
pub mod virtual_mesh;

pub use channel::{Channel, ChannelConfig, ChannelId, ChannelStats};
pub use hub::{ChannelHub, IpcError};
pub use message::{Message, MessageId, MessagePayload};
pub use virtual_mesh::{
    ChannelPattern, MeshCapability, MeshError, MeshMessage, MeshNodeId, MeshStatistics,
    SandboxMesh, VirtualChannelConfig, VirtualChannelId,
};

/// Default channel capacity.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1000;

/// Maximum message size in bytes.
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024; // 1MB

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_CHANNEL_CAPACITY, 1000);
        assert_eq!(MAX_MESSAGE_SIZE, 1024 * 1024);
    }
}
