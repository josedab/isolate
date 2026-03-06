//! Audit logging for capability usage.

use super::Capability;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

    /// Create a capability revoked event.
    pub fn revoked(sandbox_id: Uuid, capability: Capability) -> Self {
        Self::new(sandbox_id, AuditEventType::Revoked, capability, None)
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
    /// Total number of events dropped due to buffer overflow.
    overflow_count: AtomicU64,
    /// Whether we've already warned about overflow (to avoid log spam).
    overflow_warned: AtomicBool,
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
                overflow_count: AtomicU64::new(0),
                overflow_warned: AtomicBool::new(false),
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
            let count = self.inner.overflow_count.fetch_add(1, Ordering::Relaxed) + 1;

            // Warn on first overflow only
            if !self.inner.overflow_warned.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    sandbox_id = %self.inner.sandbox_id,
                    capacity = self.inner.max_events,
                    "Audit log overflow: oldest events are being dropped. \
                     Consider increasing capacity or attaching an AuditBackend for persistence."
                );
            }

            // Periodic reminders at powers of 10
            if count == 10 || count == 100 || count == 1000 || count % 10_000 == 0 {
                tracing::warn!(
                    sandbox_id = %self.inner.sandbox_id,
                    total_dropped = count,
                    "Audit log has dropped {} events", count
                );
            }
        }
        events.push(event);
    }

    /// Record a capability used event.
    pub fn record_used(&self, capability: Capability, context: Option<String>) {
        self.record(AuditEvent::used(self.inner.sandbox_id, capability, context));
    }

    /// Record a capability denied event.
    pub fn record_denied(&self, capability: Capability, context: Option<String>) {
        self.record(AuditEvent::denied(self.inner.sandbox_id, capability, context));
    }

    /// Record a capability granted event.
    pub fn record_granted(&self, capability: Capability) {
        self.record(AuditEvent::granted(self.inner.sandbox_id, capability));
    }

    /// Record a capability revoked event.
    pub fn record_revoked(&self, capability: Capability) {
        self.record(AuditEvent::revoked(self.inner.sandbox_id, capability));
    }

    /// Get all recorded events.
    pub fn events(&self) -> Vec<AuditEvent> {
        self.inner.events.read().clone()
    }

    /// Get events of a specific type.
    pub fn events_by_type(&self, event_type: AuditEventType) -> Vec<AuditEvent> {
        self.inner.events.read().iter().filter(|e| e.event_type == event_type).cloned().collect()
    }

    /// Get the number of denied events.
    pub fn denied_count(&self) -> usize {
        self.inner.events.read().iter().filter(|e| e.event_type == AuditEventType::Denied).count()
    }

    /// Clear all events.
    pub fn clear(&self) {
        self.inner.events.write().clear();
    }

    /// Export events as JSON.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.events())
    }

    /// Check whether the audit log has overflowed (lost events).
    pub fn has_overflowed(&self) -> bool {
        self.inner.overflow_count.load(Ordering::Relaxed) > 0
    }

    /// Get the total number of events dropped due to buffer overflow.
    pub fn overflow_count(&self) -> u64 {
        self.inner.overflow_count.load(Ordering::Relaxed)
    }

    /// Get the configured maximum capacity.
    pub fn capacity(&self) -> usize {
        self.inner.max_events
    }

    /// Get the current number of events in the log.
    pub fn len(&self) -> usize {
        self.inner.events.read().len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.events.read().is_empty()
    }
}

/// Trait for persistent audit event storage.
///
/// Implement this trait to store audit events in a durable backend
/// (filesystem, database, etc.) in addition to the in-memory log.
pub trait AuditBackend: Send + Sync {
    /// Write an audit event to the persistent store.
    fn write(&self, event: &AuditEvent);

    /// Query events matching a filter.
    fn query(&self, filter: &AuditFilter) -> Vec<AuditEvent>;

    /// Flush any buffered events to storage.
    fn flush(&self);
}

/// Filter criteria for audit event queries.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    /// Filter by sandbox ID.
    pub sandbox_id: Option<Uuid>,
    /// Filter by event type.
    pub event_type: Option<AuditEventType>,
    /// Only events after this time.
    pub since: Option<DateTime<Utc>>,
    /// Only events before this time.
    pub until: Option<DateTime<Utc>>,
    /// Maximum number of results.
    pub limit: Option<usize>,
}

/// Default maximum audit file size before rotation (10 MiB).
const DEFAULT_MAX_AUDIT_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum number of rotated audit files to keep.
const MAX_ROTATED_FILES: usize = 5;

/// File-backed audit backend that appends JSON lines to a file.
///
/// Each event is written as a single JSON line (newline-delimited JSON).
/// This format is easy to parse, grep, and ingest into log aggregators.
///
/// Files are rotated when they exceed `max_file_size` (default 10 MiB).
/// Up to 5 rotated files are kept (`.1` through `.5`).
pub struct FileAuditBackend {
    path: std::path::PathBuf,
    writer: std::sync::Mutex<Option<std::io::BufWriter<std::fs::File>>>,
    max_file_size: u64,
    bytes_written: std::sync::atomic::AtomicU64,
}

impl FileAuditBackend {
    /// Create a new file audit backend at the given path.
    ///
    /// Creates or appends to the file. Returns `None` if the file
    /// cannot be opened.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Option<Self> {
        let path = path.into();
        let existing_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok()?;
        Some(Self {
            path,
            writer: std::sync::Mutex::new(Some(std::io::BufWriter::new(file))),
            max_file_size: DEFAULT_MAX_AUDIT_FILE_SIZE,
            bytes_written: std::sync::atomic::AtomicU64::new(existing_size),
        })
    }

    /// Set a custom maximum file size before rotation.
    #[allow(dead_code)]
    pub fn with_max_file_size(mut self, max_bytes: u64) -> Self {
        self.max_file_size = max_bytes;
        self
    }

    fn rotate_if_needed(&self) {
        let size = self.bytes_written.load(Ordering::Relaxed);
        if size < self.max_file_size {
            return;
        }

        if let Ok(mut guard) = self.writer.lock() {
            // Flush and drop current writer
            if let Some(ref mut w) = *guard {
                use std::io::Write;
                let _ = w.flush();
            }
            *guard = None;

            // Rotate: .5 → delete, .4 → .5, .3 → .4, ..., current → .1
            for i in (1..MAX_ROTATED_FILES).rev() {
                let from = self.path.with_extension(format!("jsonl.{}", i));
                let to = self.path.with_extension(format!("jsonl.{}", i + 1));
                let _ = std::fs::rename(&from, &to);
            }
            let rotated = self.path.with_extension("jsonl.1");
            let _ = std::fs::rename(&self.path, &rotated);

            // Open new file
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)
            {
                *guard = Some(std::io::BufWriter::new(file));
                self.bytes_written.store(0, Ordering::Relaxed);
            }
        }
    }
}

impl AuditBackend for FileAuditBackend {
    fn write(&self, event: &AuditEvent) {
        self.rotate_if_needed();

        use std::io::Write;
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                if let Ok(json) = serde_json::to_string(event) {
                    let line_len = json.len() as u64 + 1; // +1 for newline
                    let _ = writeln!(writer, "{}", json);
                    self.bytes_written.fetch_add(line_len, Ordering::Relaxed);
                }
            }
        }
    }

    fn query(&self, filter: &AuditFilter) -> Vec<AuditEvent> {
        use std::io::BufRead;
        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = std::io::BufReader::new(file);
        let limit = filter.limit.unwrap_or(usize::MAX);
        let mut results = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            let event: AuditEvent = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            if let Some(sid) = filter.sandbox_id {
                if event.sandbox_id != sid {
                    continue;
                }
            }
            if let Some(et) = filter.event_type {
                if event.event_type != et {
                    continue;
                }
            }
            if let Some(since) = filter.since {
                if event.timestamp < since {
                    continue;
                }
            }
            if let Some(until) = filter.until {
                if event.timestamp > until {
                    continue;
                }
            }

            results.push(event);
            if results.len() >= limit {
                break;
            }
        }

        results
    }

    fn flush(&self) {
        use std::io::Write;
        if let Ok(mut guard) = self.writer.lock() {
            if let Some(ref mut writer) = *guard {
                let _ = writer.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_creation() {
        let sandbox_id = Uuid::new_v4();
        let event =
            AuditEvent::used(sandbox_id, Capability::stdout(), Some("writing output".to_string()));

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

    #[test]
    fn test_audit_log_revoked_event() {
        let sandbox_id = Uuid::new_v4();
        let log = AuditLog::new(sandbox_id);

        log.record_granted(Capability::stdout());
        log.record_revoked(Capability::stdout());

        let events = log.events_by_type(AuditEventType::Revoked);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_file_audit_backend_write_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let backend = FileAuditBackend::new(&path).unwrap();
        let sandbox_id = Uuid::new_v4();

        // Write events
        let event1 = AuditEvent::granted(sandbox_id, Capability::stdout());
        let event2 = AuditEvent::denied(sandbox_id, Capability::stderr(), None);
        backend.write(&event1);
        backend.write(&event2);
        backend.flush();

        // Query all
        let filter = AuditFilter::default();
        let results = backend.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_file_audit_backend_filter_by_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let backend = FileAuditBackend::new(&path).unwrap();
        let sid = Uuid::new_v4();

        backend.write(&AuditEvent::granted(sid, Capability::stdout()));
        backend.write(&AuditEvent::denied(sid, Capability::stderr(), None));
        backend.write(&AuditEvent::denied(sid, Capability::stdin(), None));
        backend.flush();

        let filter = AuditFilter { event_type: Some(AuditEventType::Denied), ..Default::default() };
        let results = backend.query(&filter);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_file_audit_backend_filter_by_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let backend = FileAuditBackend::new(&path).unwrap();
        let sid1 = Uuid::new_v4();
        let sid2 = Uuid::new_v4();

        backend.write(&AuditEvent::granted(sid1, Capability::stdout()));
        backend.write(&AuditEvent::granted(sid2, Capability::stderr()));
        backend.flush();

        let filter = AuditFilter { sandbox_id: Some(sid1), ..Default::default() };
        let results = backend.query(&filter);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_file_audit_backend_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let backend = FileAuditBackend::new(&path).unwrap();
        let sid = Uuid::new_v4();

        for _ in 0..10 {
            backend.write(&AuditEvent::granted(sid, Capability::stdout()));
        }
        backend.flush();

        let filter = AuditFilter { limit: Some(3), ..Default::default() };
        let results = backend.query(&filter);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_overflow_detection() {
        // Create a tiny audit log that can hold 3 events
        let log = AuditLog::with_capacity(Uuid::new_v4(), 3);

        assert!(!log.has_overflowed());
        assert_eq!(log.overflow_count(), 0);
        assert_eq!(log.capacity(), 3);

        // Fill it up
        log.record_granted(Capability::stdout());
        log.record_granted(Capability::stderr());
        log.record_granted(Capability::stdin());

        assert!(!log.has_overflowed());
        assert_eq!(log.len(), 3);

        // This should trigger overflow
        log.record_used(Capability::stdout(), None);

        assert!(log.has_overflowed());
        assert_eq!(log.overflow_count(), 1);
        assert_eq!(log.len(), 3); // Still capped

        // More overflows
        log.record_used(Capability::stderr(), None);
        assert_eq!(log.overflow_count(), 2);
    }

    #[test]
    fn test_no_overflow_within_capacity() {
        let log = AuditLog::with_capacity(Uuid::new_v4(), 100);

        for _ in 0..100 {
            log.record_granted(Capability::stdout());
        }

        assert!(!log.has_overflowed());
        assert_eq!(log.overflow_count(), 0);
        assert_eq!(log.len(), 100);
    }

    #[test]
    fn test_overflow_preserves_newest_events() {
        let log = AuditLog::with_capacity(Uuid::new_v4(), 2);

        log.record_granted(Capability::stdout());
        log.record_granted(Capability::stderr());
        log.record_granted(Capability::stdin()); // Pushes out stdout

        let events = log.events();
        assert_eq!(events.len(), 2);
        // The newest events should be kept
        assert_eq!(events[0].capability, Capability::stderr());
        assert_eq!(events[1].capability, Capability::stdin());
    }

    #[test]
    fn test_len_and_is_empty() {
        let log = AuditLog::with_capacity(Uuid::new_v4(), 10);

        assert!(log.is_empty());
        assert_eq!(log.len(), 0);

        log.record_granted(Capability::stdout());
        assert!(!log.is_empty());
        assert_eq!(log.len(), 1);

        log.clear();
        assert!(log.is_empty());
    }

    #[test]
    fn test_file_audit_backend_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Use a very small max size to trigger rotation quickly
        let backend = FileAuditBackend::new(&path).unwrap().with_max_file_size(200);
        let sandbox_id = Uuid::new_v4();

        // Write enough events to trigger rotation
        for i in 0..20 {
            let event =
                AuditEvent::used(sandbox_id, Capability::stdout(), Some(format!("event-{i}")));
            backend.write(&event);
        }
        backend.flush();

        // The main file should exist and be small (post-rotation)
        assert!(path.exists());
        let main_size = std::fs::metadata(&path).unwrap().len();
        assert!(main_size < 5000, "main file should be small after rotation, got {main_size}");

        // At least one rotated file should exist
        let rotated = path.with_extension("jsonl.1");
        assert!(rotated.exists(), "rotated file .1 should exist");
    }

    #[test]
    fn test_file_audit_backend_write_query_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_audit.jsonl");

        let backend = FileAuditBackend::new(&path).unwrap();
        let sandbox_id = Uuid::new_v4();

        backend.write(&AuditEvent::used(sandbox_id, Capability::stdout(), None));
        backend.write(&AuditEvent::denied(sandbox_id, Capability::stdin(), None));
        backend.flush();

        let filter = AuditFilter { sandbox_id: Some(sandbox_id), ..AuditFilter::default() };
        let results = backend.query(&filter);
        assert_eq!(results.len(), 2);
    }
}
