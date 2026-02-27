//! Replay session management and sharing.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::recording::Recording;
use super::timeline::Timeline;

/// Unique token for accessing a shared session.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionToken(String);

impl SessionToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn generate() -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        hasher.update(now.to_le_bytes());
        hasher.update(b"isolate-replay");
        // Use first 16 bytes for a shorter token
        let hash = hex::encode(hasher.finalize());
        Self(hash[..32].to_string())
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Settings for sharing a replay session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareSettings {
    pub is_public: bool,
    pub expires_at: Option<u64>,
    pub max_views: Option<u32>,
    pub allow_download: bool,
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self { is_public: false, expires_at: None, max_views: None, allow_download: true }
    }
}

/// A replay session containing a recording and its timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySession {
    pub token: SessionToken,
    pub recording: Recording,
    pub timeline: Timeline,
    pub settings: ShareSettings,
    pub created_at: u64,
    pub view_count: u32,
}

impl ReplaySession {
    /// Check if the session is still accessible.
    pub fn is_accessible(&self) -> bool {
        if let Some(expires) = self.settings.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if now > expires {
                return false;
            }
        }
        if let Some(max) = self.settings.max_views {
            if self.view_count >= max {
                return false;
            }
        }
        true
    }
}

/// Manages replay sessions.
#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionManagerInner>,
}

struct SessionManagerInner {
    sessions: RwLock<HashMap<SessionToken, ReplaySession>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self { inner: Arc::new(SessionManagerInner { sessions: RwLock::new(HashMap::new()) }) }
    }

    /// Create a new replay session from a recording.
    pub fn create_session(&self, recording: Recording, settings: ShareSettings) -> SessionToken {
        let token = SessionToken::generate();
        let timeline = Timeline::from_recording(&recording);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let session = ReplaySession {
            token: token.clone(),
            recording,
            timeline,
            settings,
            created_at: now,
            view_count: 0,
        };

        self.inner.sessions.write().insert(token.clone(), session);
        token
    }

    /// Get a session by token (increments view count).
    pub fn get_session(&self, token: &SessionToken) -> Option<ReplaySession> {
        let mut sessions = self.inner.sessions.write();
        let session = sessions.get_mut(token)?;

        if !session.is_accessible() {
            return None;
        }

        session.view_count += 1;
        Some(session.clone())
    }

    /// Delete a session.
    pub fn delete_session(&self, token: &SessionToken) -> bool {
        self.inner.sessions.write().remove(token).is_some()
    }

    /// List all session tokens.
    pub fn list_tokens(&self) -> Vec<SessionToken> {
        self.inner.sessions.read().keys().cloned().collect()
    }

    /// Count active sessions.
    pub fn count(&self) -> usize {
        self.inner.sessions.read().len()
    }

    /// Prune expired sessions.
    pub fn prune_expired(&self) -> usize {
        let mut sessions = self.inner.sessions.write();
        let before = sessions.len();
        sessions.retain(|_, s| s.is_accessible());
        before - sessions.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::recording::{EventKind, ExecutionRecorder};

    fn make_recording() -> Recording {
        let rec = ExecutionRecorder::new("test");
        rec.record_event(EventKind::Output(b"hello".to_vec()));
        rec.record_event(EventKind::Exit(0));
        rec.finish()
    }

    #[test]
    fn test_create_and_get_session() {
        let manager = SessionManager::new();
        let token = manager.create_session(make_recording(), ShareSettings::default());
        let session = manager.get_session(&token).unwrap();
        assert_eq!(session.recording.events.len(), 2);
        assert_eq!(session.view_count, 1);
    }

    #[test]
    fn test_view_count_increments() {
        let manager = SessionManager::new();
        let token = manager.create_session(make_recording(), ShareSettings::default());
        manager.get_session(&token);
        manager.get_session(&token);
        let session = manager.get_session(&token).unwrap();
        assert_eq!(session.view_count, 3);
    }

    #[test]
    fn test_max_views_limit() {
        let manager = SessionManager::new();
        let token = manager.create_session(
            make_recording(),
            ShareSettings { max_views: Some(2), ..Default::default() },
        );
        assert!(manager.get_session(&token).is_some()); // view 1
        assert!(manager.get_session(&token).is_some()); // view 2
        assert!(manager.get_session(&token).is_none()); // exceeded
    }

    #[test]
    fn test_delete_session() {
        let manager = SessionManager::new();
        let token = manager.create_session(make_recording(), ShareSettings::default());
        assert_eq!(manager.count(), 1);
        assert!(manager.delete_session(&token));
        assert_eq!(manager.count(), 0);
        assert!(manager.get_session(&token).is_none());
    }

    #[test]
    fn test_nonexistent_session() {
        let manager = SessionManager::new();
        assert!(manager.get_session(&SessionToken::new("nonexistent")).is_none());
    }

    #[test]
    fn test_list_tokens() {
        let manager = SessionManager::new();
        manager.create_session(make_recording(), ShareSettings::default());
        manager.create_session(make_recording(), ShareSettings::default());
        assert_eq!(manager.list_tokens().len(), 2);
    }

    #[test]
    fn test_session_token_display() {
        let token = SessionToken::new("abc123");
        assert_eq!(token.to_string(), "abc123");
        assert_eq!(token.as_str(), "abc123");
    }

    #[test]
    fn test_share_settings_default() {
        let s = ShareSettings::default();
        assert!(!s.is_public);
        assert!(s.expires_at.is_none());
        assert!(s.max_views.is_none());
        assert!(s.allow_download);
    }
}
