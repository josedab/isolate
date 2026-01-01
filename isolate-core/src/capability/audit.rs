//! Audit logging for capability usage.

use super::Capability;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// An audit event recording capability usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID.
    pub id: Uuid,
    /// Timestamp of the event.
    pub timestamp: DateTime<Utc>,
    /// Sandbox ID.
    pub sandbox_id: Uuid,
    /// Type of event.
    pub event_type: AuditEventType,
    /// The capability involved.
    pub capability: Capability,
    /// Additional context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl AuditEvent {
    /// Create a new audit event.
    pub fn new(
        sandbox_id: Uuid,
        event_type: AuditEventType,
        capability: Capability,
        context: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            sandbox_id,
            event_type,
            capability,
            context,
        }
    }

    /// Create a capability used event.
    pub fn used(sandbox_id: Uuid, capability: Capability, context: Option<String>) -> Self {
        Self::new(sandbox_id, AuditEventType::Used, capability, context)
    }

    /// Create a capability denied event.
    pub fn denied(sandbox_id: Uuid, capability: Capability, context: Option<String>) -> Self {
        Self::new(sandbox_id, AuditEventType::Denied, capability, context)
    }

    /// Create a capability granted event.
    pub fn granted(sandbox_id: Uuid, capability: Capability) -> Self {
        Self::new(sandbox_id, AuditEventType::Granted, capability, None)
    }
}

/// Type of audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// Capability was granted to the sandbox.
    Granted,
    /// Capability was used successfully.
    Used,
    /// Capability usage was denied.
    Denied,
    /// Capability was revoked.
    Revoked,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Granted => write!(f, "granted"),
            Self::Used => write!(f, "used"),
            Self::Denied => write!(f, "denied"),
            Self::Revoked => write!(f, "revoked"),
        }
    }
}

/// Audit log for recording capability events.
#[derive(Debug, Clone)]
pub struct AuditLog {
    inner: Arc<AuditLogInner>,
}

#[derive(Debug)]
struct AuditLogInner {
    events: RwLock<Vec<AuditEvent>>,
    max_events: usize,
    sandbox_id: Uuid,
}

impl AuditLog {
    /// Create a new audit log for a sandbox.
    pub fn new(sandbox_id: Uuid) -> Self {
        Self::with_capacity(sandbox_id, 1000)
    }

    /// Create a new audit log with a specific capacity.
    pub fn with_capacity(sandbox_id: Uuid, max_events: usize) -> Self {
        Self {
            inner: Arc::new(AuditLogInner {
                events: RwLock::new(Vec::with_capacity(max_events.min(1000))),
                max_events,
                sandbox_id,
            }),
        }
    }

    /// Record an audit event.
    pub fn record(&self, event: AuditEvent) {
        let mut events = self.inner.events.write();

        // Emit structured log
        tracing::info!(
            sandbox_id = %self.inner.sandbox_id,
            event_id = %event.id,
            event_type = %event.event_type,
            capability = %event.capability,
            context = ?event.context,
            "capability audit event"
        );

        // Store event (with capacity limit)
        if events.len() >= self.inner.max_events {
            events.remove(0); // Remove oldest
        }
        events.push(event);
    }

    /// Record a capability used event.
    pub fn record_used(&self, capability: Capability, context: Option<String>) {
        self.record(AuditEvent::used(self.inner.sandbox_id, capability, context));
    }

    /// Record a capability denied event.
    pub fn record_denied(&self, capability: Capability, context: Option<String>) {
        self.record(AuditEvent::denied(
            self.inner.sandbox_id,
            capability,
            context,
        ));
    }

    /// Record a capability granted event.
    pub fn record_granted(&self, capability: Capability) {
        self.record(AuditEvent::granted(self.inner.sandbox_id, capability));
    }

    /// Get all recorded events.
    pub fn events(&self) -> Vec<AuditEvent> {
        self.inner.events.read().clone()
    }

    /// Get events of a specific type.
    pub fn events_by_type(&self, event_type: AuditEventType) -> Vec<AuditEvent> {
        self.inner
            .events
            .read()
            .iter()
            .filter(|e| e.event_type == event_type)
            .cloned()
            .collect()
    }

    /// Get the number of denied events.
    pub fn denied_count(&self) -> usize {
        self.inner
            .events
            .read()
            .iter()
            .filter(|e| e.event_type == AuditEventType::Denied)
            .count()
    }

    /// Clear all events.
    pub fn clear(&self) {
        self.inner.events.write().clear();
    }

    /// Export events as JSON.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.events())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_creation() {
        let sandbox_id = Uuid::new_v4();
        let event = AuditEvent::used(
            sandbox_id,
            Capability::stdout(),
            Some("writing output".to_string()),
        );

        assert_eq!(event.sandbox_id, sandbox_id);
        assert_eq!(event.event_type, AuditEventType::Used);
        assert_eq!(event.context, Some("writing output".to_string()));
    }

    #[test]
    fn test_audit_log_recording() {
        let sandbox_id = Uuid::new_v4();
        let log = AuditLog::new(sandbox_id);

        log.record_granted(Capability::stdout());
        log.record_used(Capability::stdout(), None);
        log.record_denied(Capability::filesystem_read("/secret"), None);

        let events = log.events();
        assert_eq!(events.len(), 3);

        let denied = log.events_by_type(AuditEventType::Denied);
        assert_eq!(denied.len(), 1);
        assert_eq!(log.denied_count(), 1);
    }

    #[test]
    fn test_audit_log_capacity() {
        let sandbox_id = Uuid::new_v4();
        let log = AuditLog::with_capacity(sandbox_id, 5);

        for i in 0..10 {
            log.record_used(Capability::stdout(), Some(format!("event {}", i)));
        }

        let events = log.events();
        assert_eq!(events.len(), 5);
        // Should have the last 5 events
        assert_eq!(events[0].context, Some("event 5".to_string()));
    }

    #[test]
    fn test_audit_log_export_json() {
        let sandbox_id = Uuid::new_v4();
        let log = AuditLog::new(sandbox_id);

        log.record_granted(Capability::stdout());

        let json = log.export_json().unwrap();
        // Capability is serialized as {"Stdio":"Stdout"} by serde
        assert!(json.contains("Stdio"));
        assert!(json.contains("Stdout"));
        assert!(json.contains("granted"));
    }
}
