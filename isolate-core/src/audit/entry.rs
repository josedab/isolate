//! Audit entry types.

use super::AuditHash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Severity level of an audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSeverity {
    /// Debug-level event.
    Debug,
    /// Informational event.
    Info,
    /// Warning event.
    Warning,
    /// Error event.
    Error,
    /// Critical security event.
    Critical,
}

impl Default for AuditSeverity {
    fn default() -> Self {
        Self::Info
    }
}

impl std::fmt::Display for AuditSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Debug => write!(f, "DEBUG"),
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Error => write!(f, "ERROR"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Type of audit action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    /// Sandbox was created.
    SandboxCreated,
    /// Sandbox was started.
    SandboxStarted,
    /// Sandbox execution completed.
    SandboxCompleted { exit_code: i32 },
    /// Sandbox was terminated.
    SandboxTerminated { reason: String },
    /// Sandbox errored.
    SandboxError { error: String },

    /// Capability was granted.
    CapabilityGranted { capability: String },
    /// Capability was used.
    CapabilityUsed { capability: String, context: Option<String> },
    /// Capability was denied.
    CapabilityDenied { capability: String, reason: String },

    /// Resource limit was set.
    ResourceLimitSet { resource: String, limit: String },
    /// Resource limit was exceeded.
    ResourceLimitExceeded { resource: String, limit: String, actual: String },

    /// Filesystem access.
    FilesystemAccess { path: String, operation: String, allowed: bool },
    /// Network access.
    NetworkAccess { host: String, operation: String, allowed: bool },

    /// Snapshot created.
    SnapshotCreated { snapshot_id: String },
    /// Snapshot restored.
    SnapshotRestored { snapshot_id: String },

    /// Custom audit event.
    Custom { action: String, data: serde_json::Value },
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SandboxCreated => write!(f, "sandbox_created"),
            Self::SandboxStarted => write!(f, "sandbox_started"),
            Self::SandboxCompleted { exit_code } => write!(f, "sandbox_completed({})", exit_code),
            Self::SandboxTerminated { reason } => write!(f, "sandbox_terminated({})", reason),
            Self::SandboxError { error } => write!(f, "sandbox_error({})", error),
            Self::CapabilityGranted { capability } => {
                write!(f, "capability_granted({})", capability)
            }
            Self::CapabilityUsed { capability, .. } => write!(f, "capability_used({})", capability),
            Self::CapabilityDenied { capability, .. } => {
                write!(f, "capability_denied({})", capability)
            }
            Self::ResourceLimitSet { resource, limit } => {
                write!(f, "resource_limit_set({}={})", resource, limit)
            }
            Self::ResourceLimitExceeded { resource, .. } => {
                write!(f, "resource_limit_exceeded({})", resource)
            }
            Self::FilesystemAccess { path, operation, .. } => {
                write!(f, "filesystem_access({}, {})", operation, path)
            }
            Self::NetworkAccess { host, operation, .. } => {
                write!(f, "network_access({}, {})", operation, host)
            }
            Self::SnapshotCreated { snapshot_id } => {
                write!(f, "snapshot_created({})", snapshot_id)
            }
            Self::SnapshotRestored { snapshot_id } => {
                write!(f, "snapshot_restored({})", snapshot_id)
            }
            Self::Custom { action, .. } => write!(f, "custom({})", action),
        }
    }
}

/// A single audit entry in the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique entry ID.
    pub id: Uuid,
    /// Sequence number in the chain.
    pub sequence: u64,
    /// Timestamp of the entry.
    pub timestamp: DateTime<Utc>,
    /// Sandbox ID.
    pub sandbox_id: Uuid,
    /// The action being audited.
    pub action: AuditAction,
    /// Severity level.
    pub severity: AuditSeverity,
    /// Hash of the previous entry (genesis entry has all zeros).
    #[serde(with = "hex_serde")]
    pub previous_hash: AuditHash,
    /// Hash of this entry (computed over all fields except this one).
    #[serde(with = "hex_serde")]
    pub hash: AuditHash,
    /// Optional HMAC signature.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "option_hex_serde")]
    pub signature: Option<AuditHash>,
}

impl AuditEntry {
    /// Create a new audit entry.
    pub fn new(sandbox_id: Uuid, action: AuditAction, severity: Option<AuditSeverity>) -> Self {
        Self {
            id: Uuid::new_v4(),
            sequence: 0,
            timestamp: Utc::now(),
            sandbox_id,
            action,
            severity: severity.unwrap_or_default(),
            previous_hash: [0u8; 32],
            hash: [0u8; 32],
            signature: None,
        }
    }

    /// Create a genesis entry (first entry in a chain).
    pub fn genesis(sandbox_id: Uuid) -> Self {
        Self::new(sandbox_id, AuditAction::SandboxCreated, Some(AuditSeverity::Info))
    }

    /// Compute the hash of this entry.
    pub fn compute_hash(&self) -> AuditHash {
        let mut hasher = Sha256::new();

        // Hash all fields except the hash itself
        hasher.update(self.id.as_bytes());
        hasher.update(self.sequence.to_le_bytes());
        hasher.update(self.timestamp.timestamp_nanos_opt().unwrap_or(0).to_le_bytes());
        hasher.update(self.sandbox_id.as_bytes());

        // Hash the action as JSON for deterministic serialization
        let action_json = serde_json::to_string(&self.action).unwrap_or_default();
        hasher.update(action_json.as_bytes());

        hasher.update(&[self.severity as u8]);
        hasher.update(&self.previous_hash);

        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Finalize the entry by computing its hash.
    pub fn finalize(mut self) -> Self {
        self.hash = self.compute_hash();
        self
    }

    /// Finalize with a sequence number and previous hash.
    pub fn finalize_with_chain(mut self, sequence: u64, previous_hash: AuditHash) -> Self {
        self.sequence = sequence;
        self.previous_hash = previous_hash;
        self.hash = self.compute_hash();
        self
    }

    /// Sign the entry with HMAC.
    pub fn sign(mut self, key: &[u8]) -> Self {
        use sha2::Sha256;
        use std::io::Write;

        // Simple HMAC-like construction
        let mut hasher = Sha256::new();
        let mut padded_key = [0x36u8; 64];
        for (i, &k) in key.iter().take(64).enumerate() {
            padded_key[i] ^= k;
        }
        hasher.update(&padded_key);
        hasher.update(&self.hash);

        let inner_result = hasher.finalize();

        let mut hasher = Sha256::new();
        let mut padded_key = [0x5cu8; 64];
        for (i, &k) in key.iter().take(64).enumerate() {
            padded_key[i] ^= k;
        }
        hasher.update(&padded_key);
        hasher.update(&inner_result);

        let mut sig = [0u8; 32];
        let _ = (&mut sig[..]).write(&hasher.finalize());
        self.signature = Some(sig);
        self
    }

    /// Verify the HMAC signature.
    pub fn verify_signature(&self, key: &[u8]) -> bool {
        if let Some(existing_sig) = &self.signature {
            let mut entry_copy = self.clone();
            entry_copy.signature = None;
            let signed = entry_copy.sign(key);
            if let Some(computed_sig) = signed.signature {
                return &computed_sig == existing_sig;
            }
        }
        false
    }

    /// Verify that the hash is correct.
    pub fn verify_hash(&self) -> bool {
        self.hash == self.compute_hash()
    }
}

mod hex_serde {
    use super::AuditHash;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(hash: &AuditHash, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(hash))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AuditHash, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid hash length"));
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        Ok(hash)
    }
}

mod option_hex_serde {
    use super::AuditHash;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(hash: &Option<AuditHash>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match hash {
            Some(h) => serializer.serialize_some(&hex::encode(h)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AuditHash>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
                if bytes.len() != 32 {
                    return Err(serde::de::Error::custom("invalid hash length"));
                }
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&bytes);
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_entry_creation() {
        let sandbox_id = Uuid::new_v4();
        let entry = AuditEntry::new(sandbox_id, AuditAction::SandboxCreated, None);

        assert_eq!(entry.sandbox_id, sandbox_id);
        assert_eq!(entry.sequence, 0);
        assert_eq!(entry.severity, AuditSeverity::Info);
    }

    #[test]
    fn test_audit_entry_finalize() {
        let sandbox_id = Uuid::new_v4();
        let entry = AuditEntry::new(sandbox_id, AuditAction::SandboxStarted, None).finalize();

        assert_ne!(entry.hash, [0u8; 32]);
        assert!(entry.verify_hash());
    }

    #[test]
    fn test_audit_entry_chain() {
        let sandbox_id = Uuid::new_v4();
        let entry1 = AuditEntry::new(sandbox_id, AuditAction::SandboxCreated, None)
            .finalize_with_chain(0, [0u8; 32]);

        let entry2 = AuditEntry::new(sandbox_id, AuditAction::SandboxStarted, None)
            .finalize_with_chain(1, entry1.hash);

        assert_eq!(entry2.previous_hash, entry1.hash);
        assert!(entry1.verify_hash());
        assert!(entry2.verify_hash());
    }

    #[test]
    fn test_audit_entry_signature() {
        let sandbox_id = Uuid::new_v4();
        let key = b"secret-key-12345";

        let entry =
            AuditEntry::new(sandbox_id, AuditAction::SandboxCreated, None).finalize().sign(key);

        assert!(entry.signature.is_some());
        assert!(entry.verify_signature(key));
        assert!(!entry.verify_signature(b"wrong-key"));
    }

    #[test]
    fn test_audit_entry_serialization() {
        let sandbox_id = Uuid::new_v4();
        let entry = AuditEntry::new(sandbox_id, AuditAction::SandboxCreated, None).finalize();

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, entry.id);
        assert_eq!(parsed.hash, entry.hash);
    }

    #[test]
    fn test_audit_action_display() {
        let action = AuditAction::CapabilityDenied {
            capability: "stdout".to_string(),
            reason: "not granted".to_string(),
        };
        let s = action.to_string();
        assert!(s.contains("capability_denied"));
        assert!(s.contains("stdout"));
    }

    #[test]
    fn test_audit_severity_ordering() {
        assert!(AuditSeverity::Debug < AuditSeverity::Info);
        assert!(AuditSeverity::Info < AuditSeverity::Warning);
        assert!(AuditSeverity::Warning < AuditSeverity::Error);
        assert!(AuditSeverity::Error < AuditSeverity::Critical);
    }
}
