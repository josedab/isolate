//! Real-time execution event streaming.
//!
//! Provides an event broadcasting system for monitoring sandbox execution

#![allow(missing_docs)]//! in real time. Events are delivered via a broadcast channel, allowing
//! multiple subscribers to independently observe execution progress.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::engine::event_stream::{EventBroadcaster, ExecutionEvent};
//! use chrono::Utc;
//! use uuid::Uuid;
//!
//! let broadcaster = EventBroadcaster::new(64);
//! let mut sub = broadcaster.subscribe();
//!
//! broadcaster.emit(ExecutionEvent::Started {
//!     sandbox_id: Uuid::new_v4(),
//!     timestamp: Utc::now(),
//! });
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

/// An event emitted during sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionEvent {
    /// The sandbox has started executing.
    Started {
        sandbox_id: Uuid,
        timestamp: DateTime<Utc>,
    },
    /// A chunk of stdout data was produced.
    StdoutChunk {
        data: Vec<u8>,
        timestamp: DateTime<Utc>,
    },
    /// A chunk of stderr data was produced.
    StderrChunk {
        data: Vec<u8>,
        timestamp: DateTime<Utc>,
    },
    /// A resource usage update.
    ResourceUpdate {
        fuel_remaining: Option<u64>,
        memory_used: u64,
        timestamp: DateTime<Utc>,
    },
    /// Execution completed successfully.
    Completed {
        exit_code: i32,
        duration: Duration,
        timestamp: DateTime<Utc>,
    },
    /// An error occurred during execution.
    Error {
        message: String,
        timestamp: DateTime<Utc>,
    },
}

/// Broadcasts execution events to multiple subscribers.
pub struct EventBroadcaster {
    sender: broadcast::Sender<ExecutionEvent>,
}

impl EventBroadcaster {
    /// Create a new broadcaster with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Create a new subscription to this broadcaster.
    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription {
            receiver: self.sender.subscribe(),
        }
    }

    /// Emit an event to all active subscribers.
    pub fn emit(&self, event: ExecutionEvent) {
        // Ignore send errors (no active receivers).
        let _ = self.sender.send(event);
    }

    /// Return the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A subscription that receives execution events from an [`EventBroadcaster`].
pub struct EventSubscription {
    receiver: broadcast::Receiver<ExecutionEvent>,
}

impl EventSubscription {
    /// Wait for the next event. Returns `None` if the channel is closed or
    /// if this subscriber has lagged behind and lost messages.
    pub async fn next(&mut self) -> Option<ExecutionEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => return None,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Try to receive the next event without blocking.
    /// Returns `None` if no event is available or the channel is closed/lagged.
    pub fn try_next(&mut self) -> Option<ExecutionEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(_) => None,
        }
    }

    /// Collect all events until a `Completed` or `Error` event is received.
    pub async fn collect_until_complete(&mut self) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    let is_terminal = matches!(
                        event,
                        ExecutionEvent::Completed { .. } | ExecutionEvent::Error { .. }
                    );
                    events.push(event);
                    if is_terminal {
                        return events;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => return events,
                Err(broadcast::error::RecvError::Closed) => return events,
            }
        }
    }
}

/// Filters execution events by type.
pub struct EventFilter {
    stdout: bool,
    stderr: bool,
    lifecycle: bool,
    resource: bool,
}

impl EventFilter {
    /// Create a filter that matches all events.
    pub fn new() -> Self {
        Self {
            stdout: true,
            stderr: true,
            lifecycle: true,
            resource: true,
        }
    }

    /// Create a filter that matches only stdout events.
    pub fn stdout_only() -> Self {
        Self {
            stdout: true,
            stderr: false,
            lifecycle: false,
            resource: false,
        }
    }

    /// Create a filter that matches only stderr events.
    pub fn stderr_only() -> Self {
        Self {
            stdout: false,
            stderr: true,
            lifecycle: false,
            resource: false,
        }
    }

    /// Create a filter that matches only lifecycle events (Started, Completed, Error).
    pub fn lifecycle_only() -> Self {
        Self {
            stdout: false,
            stderr: false,
            lifecycle: true,
            resource: false,
        }
    }

    /// Check whether a given event matches this filter.
    pub fn matches(&self, event: &ExecutionEvent) -> bool {
        match event {
            ExecutionEvent::StdoutChunk { .. } => self.stdout,
            ExecutionEvent::StderrChunk { .. } => self.stderr,
            ExecutionEvent::Started { .. }
            | ExecutionEvent::Completed { .. }
            | ExecutionEvent::Error { .. } => self.lifecycle,
            ExecutionEvent::ResourceUpdate { .. } => self.resource,
        }
    }
}

impl Default for EventFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::time::Duration;
    use uuid::Uuid;

    fn started_event() -> ExecutionEvent {
        ExecutionEvent::Started {
            sandbox_id: Uuid::new_v4(),
            timestamp: Utc::now(),
        }
    }

    fn completed_event() -> ExecutionEvent {
        ExecutionEvent::Completed {
            exit_code: 0,
            duration: Duration::from_millis(100),
            timestamp: Utc::now(),
        }
    }

    fn stdout_event(data: &[u8]) -> ExecutionEvent {
        ExecutionEvent::StdoutChunk {
            data: data.to_vec(),
            timestamp: Utc::now(),
        }
    }

    fn stderr_event(data: &[u8]) -> ExecutionEvent {
        ExecutionEvent::StderrChunk {
            data: data.to_vec(),
            timestamp: Utc::now(),
        }
    }

    fn error_event(msg: &str) -> ExecutionEvent {
        ExecutionEvent::Error {
            message: msg.to_string(),
            timestamp: Utc::now(),
        }
    }

    fn resource_event() -> ExecutionEvent {
        ExecutionEvent::ResourceUpdate {
            fuel_remaining: Some(500),
            memory_used: 1024,
            timestamp: Utc::now(),
        }
    }

    // ---- broadcasting tests ----

    #[tokio::test]
    async fn test_broadcast_single_subscriber() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        broadcaster.emit(started_event());
        let event = sub.next().await.unwrap();
        assert!(matches!(event, ExecutionEvent::Started { .. }));
    }

    #[tokio::test]
    async fn test_broadcast_multiple_subscribers() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub1 = broadcaster.subscribe();
        let mut sub2 = broadcaster.subscribe();

        broadcaster.emit(stdout_event(b"hello"));

        let e1 = sub1.next().await.unwrap();
        let e2 = sub2.next().await.unwrap();

        assert!(matches!(e1, ExecutionEvent::StdoutChunk { .. }));
        assert!(matches!(e2, ExecutionEvent::StdoutChunk { .. }));
    }

    #[tokio::test]
    async fn test_subscriber_count() {
        let broadcaster = EventBroadcaster::new(16);
        assert_eq!(broadcaster.subscriber_count(), 0);

        let _sub1 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 1);

        let _sub2 = broadcaster.subscribe();
        assert_eq!(broadcaster.subscriber_count(), 2);

        drop(_sub1);
        assert_eq!(broadcaster.subscriber_count(), 1);
    }

    // ---- event filtering tests ----

    #[test]
    fn test_filter_all() {
        let filter = EventFilter::new();
        assert!(filter.matches(&started_event()));
        assert!(filter.matches(&stdout_event(b"x")));
        assert!(filter.matches(&stderr_event(b"x")));
        assert!(filter.matches(&completed_event()));
        assert!(filter.matches(&error_event("boom")));
        assert!(filter.matches(&resource_event()));
    }

    #[test]
    fn test_filter_stdout_only() {
        let filter = EventFilter::stdout_only();
        assert!(filter.matches(&stdout_event(b"x")));
        assert!(!filter.matches(&stderr_event(b"x")));
        assert!(!filter.matches(&started_event()));
        assert!(!filter.matches(&completed_event()));
        assert!(!filter.matches(&resource_event()));
    }

    #[test]
    fn test_filter_stderr_only() {
        let filter = EventFilter::stderr_only();
        assert!(filter.matches(&stderr_event(b"x")));
        assert!(!filter.matches(&stdout_event(b"x")));
        assert!(!filter.matches(&started_event()));
    }

    #[test]
    fn test_filter_lifecycle_only() {
        let filter = EventFilter::lifecycle_only();
        assert!(filter.matches(&started_event()));
        assert!(filter.matches(&completed_event()));
        assert!(filter.matches(&error_event("e")));
        assert!(!filter.matches(&stdout_event(b"x")));
        assert!(!filter.matches(&stderr_event(b"x")));
        assert!(!filter.matches(&resource_event()));
    }

    #[test]
    fn test_filter_default() {
        let filter = EventFilter::default();
        assert!(filter.matches(&started_event()));
        assert!(filter.matches(&stdout_event(b"x")));
    }

    // ---- subscription lifecycle tests ----

    #[tokio::test]
    async fn test_subscription_closed_channel() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        drop(broadcaster);

        let result = sub.next().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_try_next_empty() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        assert!(sub.try_next().is_none());
    }

    #[tokio::test]
    async fn test_try_next_with_event() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        broadcaster.emit(started_event());

        let event = sub.try_next();
        assert!(event.is_some());
        assert!(matches!(event.unwrap(), ExecutionEvent::Started { .. }));
    }

    // ---- backpressure (lagged subscriber) ----

    #[tokio::test]
    async fn test_lagged_subscriber() {
        let broadcaster = EventBroadcaster::new(2);
        let mut sub = broadcaster.subscribe();

        // Overflow the channel: capacity is 2, send 3 events.
        broadcaster.emit(stdout_event(b"1"));
        broadcaster.emit(stdout_event(b"2"));
        broadcaster.emit(stdout_event(b"3"));

        // The subscriber has lagged; next() should return None.
        let result = sub.next().await;
        assert!(result.is_none());
    }

    // ---- collect_until_complete ----

    #[tokio::test]
    async fn test_collect_until_completed() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        let handle = tokio::spawn(async move { sub.collect_until_complete().await });

        broadcaster.emit(started_event());
        broadcaster.emit(stdout_event(b"out"));
        broadcaster.emit(completed_event());

        let events = handle.await.unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], ExecutionEvent::Started { .. }));
        assert!(matches!(events[1], ExecutionEvent::StdoutChunk { .. }));
        assert!(matches!(events[2], ExecutionEvent::Completed { .. }));
    }

    #[tokio::test]
    async fn test_collect_until_error() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        let handle = tokio::spawn(async move { sub.collect_until_complete().await });

        broadcaster.emit(started_event());
        broadcaster.emit(error_event("crashed"));

        let events = handle.await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[1], ExecutionEvent::Error { .. }));
    }

    #[tokio::test]
    async fn test_collect_until_channel_closed() {
        let broadcaster = EventBroadcaster::new(16);
        let mut sub = broadcaster.subscribe();

        let handle = tokio::spawn(async move { sub.collect_until_complete().await });

        broadcaster.emit(started_event());
        drop(broadcaster);

        let events = handle.await.unwrap();
        // Should have collected the Started event before the channel closed.
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ExecutionEvent::Started { .. }));
    }

    // ---- serialization ----

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = completed_event();
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ExecutionEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ExecutionEvent::Completed { .. }));
    }
}
