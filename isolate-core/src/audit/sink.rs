//! Audit log export sinks.
//!
//! Provides trait-based sinks for exporting audit events to external systems.

use super::entry::AuditEntry;

/// Trait for exporting audit events to external systems.
pub trait AuditSink: Send + Sync {
    /// Export a single audit entry.
    fn export(&self, entry: &AuditEntry);

    /// Flush any buffered entries.
    fn flush(&self) {}
}

/// Writes audit entries as JSON lines to stdout (container-friendly).
pub struct StdoutSink;

impl AuditSink for StdoutSink {
    fn export(&self, entry: &AuditEntry) {
        if let Ok(json) = serde_json::to_string(entry) {
            println!("{json}");
        }
    }
}

/// Writes audit entries as JSON lines to a file.
pub struct FileSink {
    path: std::path::PathBuf,
}

impl FileSink {
    /// Create a new file sink that appends to the given path.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl AuditSink for FileSink {
    fn export(&self, entry: &AuditEntry) {
        use std::io::Write;
        if let Ok(json) = serde_json::to_string(entry) {
            if let Ok(mut file) =
                std::fs::OpenOptions::new().create(true).append(true).open(&self.path)
            {
                let _ = writeln!(file, "{json}");
            }
        }
    }
}

/// No-op sink that discards all entries (for testing or when audit is disabled).
pub struct NullSink;

impl AuditSink for NullSink {
    fn export(&self, _entry: &AuditEntry) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::entry::{AuditAction, AuditEntry};
    use uuid::Uuid;

    #[test]
    fn test_null_sink() {
        let sink = NullSink;
        let entry = AuditEntry::new(Uuid::new_v4(), AuditAction::SandboxCreated, None);
        sink.export(&entry); // Should not panic
    }

    #[test]
    fn test_file_sink_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        let sink = FileSink::new(&path);

        let entry = AuditEntry::new(Uuid::new_v4(), AuditAction::SandboxCreated, None);
        sink.export(&entry);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "audit file should not be empty");
    }

    #[test]
    fn test_stdout_sink_does_not_panic() {
        let sink = StdoutSink;
        let entry = AuditEntry::new(Uuid::new_v4(), AuditAction::SandboxStarted, None);
        sink.export(&entry);
    }
}
