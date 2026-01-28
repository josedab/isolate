//! Cryptographic audit log chain.

use super::entry::{AuditAction, AuditEntry, AuditSeverity};
use super::verifier::{ChainVerificationError, ChainVerifier};
use super::AuditHash;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Configuration for the cryptographic audit log.
#[derive(Debug, Clone)]
pub struct CryptoAuditLogConfig {
    /// Maximum number of entries to keep.
    pub max_entries: usize,
    /// HMAC signing key (optional).
    pub signing_key: Option<Vec<u8>>,
    /// Whether to auto-verify on each append.
    pub verify_on_append: bool,
}

impl Default for CryptoAuditLogConfig {
    fn default() -> Self {
        Self { max_entries: 10_000, signing_key: None, verify_on_append: false }
    }
}

impl CryptoAuditLogConfig {
    /// Create a config with a signing key.
    pub fn with_signing_key(key: impl Into<Vec<u8>>) -> Self {
        Self { signing_key: Some(key.into()), ..Default::default() }
    }
}

/// Cryptographic audit log with hash chain.
#[derive(Debug)]
pub struct CryptoAuditLog {
    inner: Arc<CryptoAuditLogInner>,
}

#[derive(Debug)]
struct CryptoAuditLogInner {
    entries: RwLock<Vec<AuditEntry>>,
    config: CryptoAuditLogConfig,
    sandbox_id: Uuid,
}

impl CryptoAuditLog {
    /// Create a new cryptographic audit log.
    pub fn new(sandbox_id: Uuid) -> Self {
        Self::with_config(sandbox_id, CryptoAuditLogConfig::default())
    }

    /// Create a new audit log with configuration.
    pub fn with_config(sandbox_id: Uuid, config: CryptoAuditLogConfig) -> Self {
        let log = Self {
            inner: Arc::new(CryptoAuditLogInner {
                entries: RwLock::new(Vec::new()),
                config,
                sandbox_id,
            }),
        };

        // Add genesis entry
        let genesis = AuditEntry::genesis(sandbox_id);
        log.append_internal(genesis);

        log
    }

    /// Get the sandbox ID.
    pub fn sandbox_id(&self) -> Uuid {
        self.inner.sandbox_id
    }

    /// Record an audit entry.
    pub fn record(&self, action: AuditAction, severity: Option<AuditSeverity>) {
        let entry = AuditEntry::new(self.inner.sandbox_id, action, severity);
        self.append_internal(entry);
    }

    /// Append an entry to the log.
    fn append_internal(&self, mut entry: AuditEntry) {
        let mut entries = self.inner.entries.write();

        // Get previous hash and sequence
        let (sequence, previous_hash) = if entries.is_empty() {
            (0, [0u8; 32])
        } else {
            let last = entries.last().unwrap();
            (last.sequence + 1, last.hash)
        };

        // Finalize entry with chain info
        entry = entry.finalize_with_chain(sequence, previous_hash);

        // Sign if we have a key
        if let Some(ref key) = self.inner.config.signing_key {
            entry = entry.sign(key);
        }

        // Log the entry
        tracing::debug!(
            sandbox_id = %self.inner.sandbox_id,
            sequence = entry.sequence,
            action = %entry.action,
            hash = %hex::encode(&entry.hash[..8]),
            "audit entry recorded"
        );

        // Enforce capacity limit
        if entries.len() >= self.inner.config.max_entries {
            // Remove oldest entries but keep genesis
            let remove_count = entries.len() - self.inner.config.max_entries + 1;
            entries.drain(1..=remove_count);
        }

        entries.push(entry);
    }

    /// Record a sandbox started event.
    pub fn record_started(&self) {
        self.record(AuditAction::SandboxStarted, Some(AuditSeverity::Info));
    }

    /// Record sandbox completion.
    pub fn record_completed(&self, exit_code: i32) {
        self.record(AuditAction::SandboxCompleted { exit_code }, Some(AuditSeverity::Info));
    }

    /// Record sandbox termination.
    pub fn record_terminated(&self, reason: impl Into<String>) {
        self.record(
            AuditAction::SandboxTerminated { reason: reason.into() },
            Some(AuditSeverity::Warning),
        );
    }

    /// Record a sandbox error.
    pub fn record_error(&self, error: impl Into<String>) {
        self.record(AuditAction::SandboxError { error: error.into() }, Some(AuditSeverity::Error));
    }

    /// Record capability granted.
    pub fn record_capability_granted(&self, capability: impl Into<String>) {
        self.record(
            AuditAction::CapabilityGranted { capability: capability.into() },
            Some(AuditSeverity::Info),
        );
    }

    /// Record capability usage.
    pub fn record_capability_used(&self, capability: impl Into<String>, context: Option<String>) {
        self.record(
            AuditAction::CapabilityUsed { capability: capability.into(), context },
            Some(AuditSeverity::Debug),
        );
    }

    /// Record capability denied.
    pub fn record_capability_denied(
        &self,
        capability: impl Into<String>,
        reason: impl Into<String>,
    ) {
        self.record(
            AuditAction::CapabilityDenied { capability: capability.into(), reason: reason.into() },
            Some(AuditSeverity::Warning),
        );
    }

    /// Record resource limit exceeded.
    pub fn record_resource_exceeded(
        &self,
        resource: impl Into<String>,
        limit: impl Into<String>,
        actual: impl Into<String>,
    ) {
        self.record(
            AuditAction::ResourceLimitExceeded {
                resource: resource.into(),
                limit: limit.into(),
                actual: actual.into(),
            },
            Some(AuditSeverity::Warning),
        );
    }

    /// Record filesystem access.
    pub fn record_filesystem_access(
        &self,
        path: impl Into<String>,
        operation: impl Into<String>,
        allowed: bool,
    ) {
        self.record(
            AuditAction::FilesystemAccess {
                path: path.into(),
                operation: operation.into(),
                allowed,
            },
            Some(if allowed { AuditSeverity::Debug } else { AuditSeverity::Warning }),
        );
    }

    /// Record network access.
    pub fn record_network_access(
        &self,
        host: impl Into<String>,
        operation: impl Into<String>,
        allowed: bool,
    ) {
        self.record(
            AuditAction::NetworkAccess { host: host.into(), operation: operation.into(), allowed },
            Some(if allowed { AuditSeverity::Debug } else { AuditSeverity::Warning }),
        );
    }

    /// Get all entries.
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.inner.entries.read().clone()
    }

    /// Get entry count.
    pub fn len(&self) -> usize {
        self.inner.entries.read().len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.entries.read().is_empty()
    }

    /// Get the latest entry.
    pub fn latest(&self) -> Option<AuditEntry> {
        self.inner.entries.read().last().cloned()
    }

    /// Get the head hash (hash of the latest entry).
    pub fn head_hash(&self) -> Option<AuditHash> {
        self.inner.entries.read().last().map(|e| e.hash)
    }

    /// Verify the integrity of the entire chain.
    pub fn verify_chain(&self) -> Result<(), ChainVerificationError> {
        let entries = self.inner.entries.read();
        ChainVerifier::verify_chain(&entries)
    }

    /// Verify entries with signatures.
    pub fn verify_signatures(&self, key: &[u8]) -> Result<(), ChainVerificationError> {
        let entries = self.inner.entries.read();
        ChainVerifier::verify_signatures(&entries, key)
    }

    /// Export the audit log as JSON.
    pub fn export_json(&self) -> Result<String, serde_json::Error> {
        let export = AuditLogExport {
            sandbox_id: self.inner.sandbox_id,
            entries: self.entries(),
            head_hash: self.head_hash(),
        };
        serde_json::to_string_pretty(&export)
    }

    /// Import an audit log from JSON.
    pub fn import_json(json: &str) -> Result<Self, serde_json::Error> {
        let export: AuditLogExport = serde_json::from_str(json)?;
        let log = Self {
            inner: Arc::new(CryptoAuditLogInner {
                entries: RwLock::new(export.entries),
                config: CryptoAuditLogConfig::default(),
                sandbox_id: export.sandbox_id,
            }),
        };
        Ok(log)
    }
}

impl Clone for CryptoAuditLog {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

/// Serializable export format for audit logs.
#[derive(Debug, Serialize, Deserialize)]
struct AuditLogExport {
    sandbox_id: Uuid,
    entries: Vec<AuditEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_hash: Option<AuditHash>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_audit_log_creation() {
        let sandbox_id = Uuid::new_v4();
        let log = CryptoAuditLog::new(sandbox_id);

        assert_eq!(log.sandbox_id(), sandbox_id);
        assert_eq!(log.len(), 1); // Genesis entry
    }

    #[test]
    fn test_crypto_audit_log_record() {
        let sandbox_id = Uuid::new_v4();
        let log = CryptoAuditLog::new(sandbox_id);

        log.record_started();
        log.record_capability_granted("stdout");
        log.record_capability_used("stdout", Some("writing hello".to_string()));
        log.record_completed(0);

        assert_eq!(log.len(), 5); // Genesis + 4 events
    }

    #[test]
    fn test_crypto_audit_log_chain_integrity() {
        let sandbox_id = Uuid::new_v4();
        let log = CryptoAuditLog::new(sandbox_id);

        log.record_started();
        log.record_capability_granted("stdout");
        log.record_completed(0);

        // Verify chain is intact
        assert!(log.verify_chain().is_ok());

        // Check entries are properly linked
        let entries = log.entries();
        for i in 1..entries.len() {
            assert_eq!(entries[i].previous_hash, entries[i - 1].hash);
        }
    }

    #[test]
    fn test_crypto_audit_log_with_signing() {
        let sandbox_id = Uuid::new_v4();
        let config = CryptoAuditLogConfig::with_signing_key(b"test-key-12345");
        let log = CryptoAuditLog::with_config(sandbox_id, config);

        log.record_started();
        log.record_completed(0);

        // Verify signatures
        assert!(log.verify_signatures(b"test-key-12345").is_ok());
        assert!(log.verify_signatures(b"wrong-key").is_err());
    }

    #[test]
    fn test_crypto_audit_log_export_import() {
        let sandbox_id = Uuid::new_v4();
        let log = CryptoAuditLog::new(sandbox_id);

        log.record_started();
        log.record_capability_denied("fs:read:/etc/passwd", "capability not granted");
        log.record_completed(1);

        let json = log.export_json().unwrap();

        let imported = CryptoAuditLog::import_json(&json).unwrap();
        assert_eq!(imported.sandbox_id(), sandbox_id);
        assert_eq!(imported.len(), log.len());
        assert!(imported.verify_chain().is_ok());
    }

    #[test]
    fn test_crypto_audit_log_capacity() {
        let sandbox_id = Uuid::new_v4();
        let config = CryptoAuditLogConfig { max_entries: 5, ..Default::default() };
        let log = CryptoAuditLog::with_config(sandbox_id, config);

        // Add more entries than capacity
        for i in 0..10 {
            log.record(
                AuditAction::Custom {
                    action: format!("event_{}", i),
                    data: serde_json::Value::Null,
                },
                None,
            );
        }

        assert!(log.len() <= 5);
        // Genesis should still be there
        let entries = log.entries();
        assert!(matches!(entries[0].action, AuditAction::SandboxCreated));
    }

    #[test]
    fn test_crypto_audit_log_convenience_methods() {
        let sandbox_id = Uuid::new_v4();
        let log = CryptoAuditLog::new(sandbox_id);

        log.record_filesystem_access("/data/file.txt", "read", true);
        log.record_network_access("api.example.com", "http_get", true);
        log.record_resource_exceeded("memory", "100MB", "150MB");

        assert_eq!(log.len(), 4); // Genesis + 3 events
        assert!(log.verify_chain().is_ok());
    }
}
