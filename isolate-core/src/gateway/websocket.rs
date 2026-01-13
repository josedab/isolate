//! WebSocket gateway for real-time bidirectional sandbox I/O.
//!
//! Enables browser-based and streaming clients to interact with sandboxes
//! in real-time via WebSocket connections, with message framing and flow control.
//!
//! ```rust
//! use isolate_core::gateway::websocket::{
//!     WsMessage, WsSession, WsSessionConfig, WsGateway, WsGatewayConfig,
//! };
//!
//! let config = WsGatewayConfig::default();
//! let gateway = WsGateway::new(config);
//! assert_eq!(gateway.active_sessions(), 0);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// WebSocket message types for sandbox I/O.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsMessage {
    /// Write to sandbox stdin.
    Stdin(String),
    /// Sandbox stdout output.
    Stdout(String),
    /// Sandbox stderr output.
    Stderr(String),
    /// Sandbox has exited.
    Exit { code: i32, duration_ms: u64 },
    /// Error message.
    Error { message: String, code: String },
    /// Ping (keepalive).
    Ping { seq: u64 },
    /// Pong (keepalive response).
    Pong { seq: u64 },
    /// Resize terminal.
    Resize { cols: u16, rows: u16 },
    /// Signal to sandbox (e.g., interrupt).
    Signal { name: String },
    /// Resource usage update.
    ResourceUpdate { fuel_consumed: u64, memory_bytes: u64, elapsed_ms: u64 },
}

impl WsMessage {
    /// Serialize to JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Parse from JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// Check if this is a client-to-server message.
    pub fn is_client_message(&self) -> bool {
        matches!(
            self,
            WsMessage::Stdin(_)
                | WsMessage::Ping { .. }
                | WsMessage::Resize { .. }
                | WsMessage::Signal { .. }
        )
    }

    /// Check if this is a server-to-client message.
    pub fn is_server_message(&self) -> bool {
        matches!(
            self,
            WsMessage::Stdout(_)
                | WsMessage::Stderr(_)
                | WsMessage::Exit { .. }
                | WsMessage::Error { .. }
                | WsMessage::Pong { .. }
                | WsMessage::ResourceUpdate { .. }
        )
    }
}

/// Configuration for a WebSocket session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSessionConfig {
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Keepalive ping interval.
    pub ping_interval: Duration,
    /// Session timeout (idle).
    pub idle_timeout: Duration,
    /// Maximum output buffer size.
    pub max_output_buffer: usize,
    /// Enable resource usage updates.
    pub send_resource_updates: bool,
    /// Resource update interval.
    pub resource_update_interval: Duration,
}

impl Default for WsSessionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 64 * 1024, // 64 KB
            ping_interval: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(300),
            max_output_buffer: 1024 * 1024, // 1 MB
            send_resource_updates: true,
            resource_update_interval: Duration::from_secs(1),
        }
    }
}

/// State of a WebSocket session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Connection established, waiting for sandbox.
    Connecting,
    /// Sandbox is running, I/O is active.
    Active,
    /// Sandbox has exited, draining output.
    Draining,
    /// Session is closed.
    Closed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connecting => write!(f, "connecting"),
            Self::Active => write!(f, "active"),
            Self::Draining => write!(f, "draining"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

/// A WebSocket session connected to a sandbox.
pub struct WsSession {
    /// Session ID.
    pub id: String,
    /// Associated sandbox ID.
    pub sandbox_id: Option<String>,
    /// Session configuration.
    config: WsSessionConfig,
    /// Current state.
    state: SessionState,
    /// Output buffer (messages pending send to client).
    output_buffer: Vec<WsMessage>,
    /// Input buffer (messages received from client).
    input_buffer: Vec<WsMessage>,
    /// Messages sent count.
    messages_sent: u64,
    /// Messages received count.
    messages_received: u64,
    /// Session creation time.
    created_at: Instant,
    /// Last activity time.
    last_activity: Instant,
    /// Ping sequence counter.
    ping_seq: u64,
}

impl WsSession {
    /// Create a new session.
    pub fn new(id: impl Into<String>, config: WsSessionConfig) -> Self {
        let now = Instant::now();
        Self {
            id: id.into(),
            sandbox_id: None,
            config,
            state: SessionState::Connecting,
            output_buffer: Vec::new(),
            input_buffer: Vec::new(),
            messages_sent: 0,
            messages_received: 0,
            created_at: now,
            last_activity: now,
            ping_seq: 0,
        }
    }

    /// Associate with a sandbox.
    pub fn attach_sandbox(&mut self, sandbox_id: String) {
        self.sandbox_id = Some(sandbox_id);
        self.state = SessionState::Active;
    }

    /// Get current state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Receive a message from the client.
    pub fn receive(&mut self, message: WsMessage) -> Result<(), String> {
        if self.state == SessionState::Closed {
            return Err("Session is closed".to_string());
        }

        self.last_activity = Instant::now();
        self.messages_received += 1;

        match &message {
            WsMessage::Ping { seq } => {
                // Auto-respond with pong
                self.output_buffer.push(WsMessage::Pong { seq: *seq });
                return Ok(());
            }
            WsMessage::Stdin(_) if self.state != SessionState::Active => {
                return Err("Sandbox not active".to_string());
            }
            _ => {}
        }

        self.input_buffer.push(message);
        Ok(())
    }

    /// Queue a message to send to the client.
    pub fn send(&mut self, message: WsMessage) {
        if self.state == SessionState::Closed {
            return;
        }

        // Check buffer limit
        let buffer_size: usize = self.output_buffer.iter().map(|m| m.to_json().len()).sum();
        if buffer_size < self.config.max_output_buffer {
            self.output_buffer.push(message);
            self.messages_sent += 1;
        }
    }

    /// Drain the output buffer.
    pub fn drain_output(&mut self) -> Vec<WsMessage> {
        std::mem::take(&mut self.output_buffer)
    }

    /// Drain the input buffer.
    pub fn drain_input(&mut self) -> Vec<WsMessage> {
        std::mem::take(&mut self.input_buffer)
    }

    /// Mark sandbox as exited.
    pub fn sandbox_exited(&mut self, exit_code: i32, duration: Duration) {
        self.send(WsMessage::Exit { code: exit_code, duration_ms: duration.as_millis() as u64 });
        self.state = SessionState::Draining;
    }

    /// Close the session.
    pub fn close(&mut self) {
        self.state = SessionState::Closed;
        self.output_buffer.clear();
        self.input_buffer.clear();
    }

    /// Check if the session has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.last_activity.elapsed() > self.config.idle_timeout
    }

    /// Generate a keepalive ping if needed.
    pub fn maybe_ping(&mut self) -> Option<WsMessage> {
        if self.last_activity.elapsed() > self.config.ping_interval
            && self.state == SessionState::Active
        {
            self.ping_seq += 1;
            let msg = WsMessage::Ping { seq: self.ping_seq };
            self.last_activity = Instant::now();
            Some(msg)
        } else {
            None
        }
    }

    /// Get session statistics.
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            session_id: self.id.clone(),
            state: self.state,
            messages_sent: self.messages_sent,
            messages_received: self.messages_received,
            output_buffer_len: self.output_buffer.len(),
            uptime: self.created_at.elapsed(),
        }
    }
}

/// Session statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub session_id: String,
    pub state: SessionState,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub output_buffer_len: usize,
    #[serde(with = "duration_serde")]
    pub uptime: Duration,
}

mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_millis().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// WebSocket gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsGatewayConfig {
    /// Maximum concurrent sessions.
    pub max_sessions: usize,
    /// Default session configuration.
    pub default_session_config: WsSessionConfig,
    /// Allowed origins for CORS.
    pub allowed_origins: Vec<String>,
    /// Require authentication.
    pub require_auth: bool,
}

impl Default for WsGatewayConfig {
    fn default() -> Self {
        Self {
            max_sessions: 100,
            default_session_config: WsSessionConfig::default(),
            allowed_origins: vec!["*".to_string()],
            require_auth: false,
        }
    }
}

/// WebSocket gateway managing multiple sessions.
pub struct WsGateway {
    config: WsGatewayConfig,
    sessions: HashMap<String, WsSession>,
    next_id: u64,
    total_sessions_created: u64,
}

impl WsGateway {
    /// Create a new gateway.
    pub fn new(config: WsGatewayConfig) -> Self {
        Self { config, sessions: HashMap::new(), next_id: 1, total_sessions_created: 0 }
    }

    /// Create a new session, returning its ID.
    pub fn create_session(&mut self) -> Result<String, String> {
        if self.sessions.len() >= self.config.max_sessions {
            return Err(format!("Maximum sessions ({}) reached", self.config.max_sessions));
        }

        let id = format!("ws-{}", self.next_id);
        self.next_id += 1;

        let session = WsSession::new(id.clone(), self.config.default_session_config.clone());
        self.sessions.insert(id.clone(), session);
        self.total_sessions_created += 1;

        Ok(id)
    }

    /// Get a session by ID.
    pub fn session(&self, id: &str) -> Option<&WsSession> {
        self.sessions.get(id)
    }

    /// Get a mutable session by ID.
    pub fn session_mut(&mut self, id: &str) -> Option<&mut WsSession> {
        self.sessions.get_mut(id)
    }

    /// Remove a session.
    pub fn remove_session(&mut self, id: &str) -> bool {
        if let Some(mut session) = self.sessions.remove(id) {
            session.close();
            true
        } else {
            false
        }
    }

    /// Clean up timed-out sessions.
    pub fn cleanup_stale(&mut self) -> Vec<String> {
        let stale: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_timed_out() || s.state() == SessionState::Closed)
            .map(|(id, _)| id.clone())
            .collect();

        for id in &stale {
            self.sessions.remove(id);
        }

        stale
    }

    /// Number of active sessions.
    pub fn active_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Get gateway statistics.
    pub fn stats(&self) -> GatewayStats {
        let active = self.sessions.values().filter(|s| s.state() == SessionState::Active).count();

        GatewayStats {
            total_sessions: self.sessions.len(),
            active_sessions: active,
            total_sessions_created: self.total_sessions_created,
            max_sessions: self.config.max_sessions,
        }
    }
}

/// Gateway statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_sessions_created: u64,
    pub max_sessions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_message_json_roundtrip() {
        let msg = WsMessage::Stdout("Hello, world!".to_string());
        let json = msg.to_json();
        let parsed = WsMessage::from_json(&json).unwrap();
        match parsed {
            WsMessage::Stdout(s) => assert_eq!(s, "Hello, world!"),
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_ws_message_types() {
        assert!(WsMessage::Stdin("test".to_string()).is_client_message());
        assert!(!WsMessage::Stdin("test".to_string()).is_server_message());

        assert!(WsMessage::Stdout("test".to_string()).is_server_message());
        assert!(!WsMessage::Stdout("test".to_string()).is_client_message());
    }

    #[test]
    fn test_ws_message_exit() {
        let msg = WsMessage::Exit { code: 0, duration_ms: 1500 };
        let json = msg.to_json();
        assert!(json.contains("Exit"));
    }

    #[test]
    fn test_session_lifecycle() {
        let mut session = WsSession::new("test-1", WsSessionConfig::default());
        assert_eq!(session.state(), SessionState::Connecting);

        session.attach_sandbox("sandbox-123".to_string());
        assert_eq!(session.state(), SessionState::Active);
        assert_eq!(session.sandbox_id, Some("sandbox-123".to_string()));

        session.sandbox_exited(0, Duration::from_secs(5));
        assert_eq!(session.state(), SessionState::Draining);

        let output = session.drain_output();
        assert!(!output.is_empty());

        session.close();
        assert_eq!(session.state(), SessionState::Closed);
    }

    #[test]
    fn test_session_receive_stdin() {
        let mut session = WsSession::new("test-1", WsSessionConfig::default());
        session.attach_sandbox("sandbox-1".to_string());

        session.receive(WsMessage::Stdin("input data".to_string())).unwrap();

        let input = session.drain_input();
        assert_eq!(input.len(), 1);
    }

    #[test]
    fn test_session_receive_when_closed() {
        let mut session = WsSession::new("test-1", WsSessionConfig::default());
        session.close();

        let result = session.receive(WsMessage::Stdin("data".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_session_auto_pong() {
        let mut session = WsSession::new("test-1", WsSessionConfig::default());
        session.attach_sandbox("sb".to_string());

        session.receive(WsMessage::Ping { seq: 42 }).unwrap();

        let output = session.drain_output();
        assert_eq!(output.len(), 1);
        match &output[0] {
            WsMessage::Pong { seq } => assert_eq!(*seq, 42),
            _ => panic!("Expected Pong"),
        }
    }

    #[test]
    fn test_session_stats() {
        let mut session = WsSession::new("test-1", WsSessionConfig::default());
        session.attach_sandbox("sb".to_string());
        session.send(WsMessage::Stdout("hello".to_string()));

        let stats = session.stats();
        assert_eq!(stats.session_id, "test-1");
        assert_eq!(stats.state, SessionState::Active);
        assert_eq!(stats.messages_sent, 1);
    }

    #[test]
    fn test_gateway_create_session() {
        let mut gateway = WsGateway::new(WsGatewayConfig::default());
        let id = gateway.create_session().unwrap();
        assert!(id.starts_with("ws-"));
        assert_eq!(gateway.active_sessions(), 1);
    }

    #[test]
    fn test_gateway_max_sessions() {
        let config = WsGatewayConfig { max_sessions: 2, ..Default::default() };
        let mut gateway = WsGateway::new(config);

        gateway.create_session().unwrap();
        gateway.create_session().unwrap();
        assert!(gateway.create_session().is_err());
    }

    #[test]
    fn test_gateway_remove_session() {
        let mut gateway = WsGateway::new(WsGatewayConfig::default());
        let id = gateway.create_session().unwrap();

        assert!(gateway.remove_session(&id));
        assert_eq!(gateway.active_sessions(), 0);
        assert!(!gateway.remove_session(&id));
    }

    #[test]
    fn test_gateway_cleanup_closed() {
        let mut gateway = WsGateway::new(WsGatewayConfig::default());
        let id = gateway.create_session().unwrap();
        gateway.session_mut(&id).unwrap().close();

        let stale = gateway.cleanup_stale();
        assert_eq!(stale.len(), 1);
        assert_eq!(gateway.active_sessions(), 0);
    }

    #[test]
    fn test_gateway_stats() {
        let mut gateway = WsGateway::new(WsGatewayConfig::default());
        let id = gateway.create_session().unwrap();
        gateway.session_mut(&id).unwrap().attach_sandbox("sb".to_string());

        let stats = gateway.stats();
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.active_sessions, 1);
        assert_eq!(stats.total_sessions_created, 1);
    }

    #[test]
    fn test_session_state_display() {
        assert_eq!(SessionState::Connecting.to_string(), "connecting");
        assert_eq!(SessionState::Active.to_string(), "active");
        assert_eq!(SessionState::Draining.to_string(), "draining");
        assert_eq!(SessionState::Closed.to_string(), "closed");
    }

    #[test]
    fn test_ws_gateway_config_default() {
        let config = WsGatewayConfig::default();
        assert_eq!(config.max_sessions, 100);
        assert!(!config.require_auth);
    }
}
