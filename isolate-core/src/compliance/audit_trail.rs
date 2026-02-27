//! Tamper-proof audit trail with cryptographic chaining.

use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A single entry in the audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub sequence: u64,
    pub event_type: String,
    pub description: String,
    pub actor: String,
    pub timestamp: u64,
    /// SHA-256 hash of this entry chained with previous.
    pub hash: String,
    pub prev_hash: String,
}

/// Verification result for the audit chain.
#[derive(Debug, Clone)]
pub struct ChainVerification {
    pub valid: bool,
    pub entries_checked: u64,
    pub first_invalid: Option<u64>,
}

/// Cryptographically chained audit trail.
#[derive(Clone)]
pub struct AuditTrail {
    inner: Arc<AuditTrailInner>,
}

struct AuditTrailInner {
    entries: RwLock<Vec<AuditEntry>>,
}

/// A view of the chain for inspection.
#[derive(Debug, Clone)]
pub struct AuditChain {
    pub entries: Vec<AuditEntry>,
    pub length: u64,
}

impl AuditTrail {
    pub fn new() -> Self {
        Self { inner: Arc::new(AuditTrailInner { entries: RwLock::new(Vec::new()) }) }
    }

    /// Record a new event in the audit trail.
    pub fn record(&self, event_type: &str, description: &str, actor: &str) -> u64 {
        let mut entries = self.inner.entries.write();
        let sequence = entries.len() as u64;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let prev_hash = entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| "0".repeat(64));

        let hash =
            Self::compute_hash(sequence, event_type, description, actor, timestamp, &prev_hash);

        entries.push(AuditEntry {
            sequence,
            event_type: event_type.to_string(),
            description: description.to_string(),
            actor: actor.to_string(),
            timestamp,
            hash,
            prev_hash,
        });

        sequence
    }

    /// Record with an explicit timestamp (for testing/replay).
    pub fn record_at(
        &self,
        event_type: &str,
        description: &str,
        actor: &str,
        timestamp: u64,
    ) -> u64 {
        let mut entries = self.inner.entries.write();
        let sequence = entries.len() as u64;

        let prev_hash = entries.last().map(|e| e.hash.clone()).unwrap_or_else(|| "0".repeat(64));

        let hash =
            Self::compute_hash(sequence, event_type, description, actor, timestamp, &prev_hash);

        entries.push(AuditEntry {
            sequence,
            event_type: event_type.to_string(),
            description: description.to_string(),
            actor: actor.to_string(),
            timestamp,
            hash,
            prev_hash,
        });

        sequence
    }

    fn compute_hash(
        seq: u64,
        event_type: &str,
        desc: &str,
        actor: &str,
        ts: u64,
        prev: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(seq.to_le_bytes());
        hasher.update(event_type.as_bytes());
        hasher.update(desc.as_bytes());
        hasher.update(actor.as_bytes());
        hasher.update(ts.to_le_bytes());
        hasher.update(prev.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Verify the integrity of the entire chain.
    pub fn verify(&self) -> ChainVerification {
        let entries = self.inner.entries.read();
        let mut checked = 0u64;

        for (i, entry) in entries.iter().enumerate() {
            let expected_prev = if i == 0 { "0".repeat(64) } else { entries[i - 1].hash.clone() };

            if entry.prev_hash != expected_prev {
                return ChainVerification {
                    valid: false,
                    entries_checked: checked,
                    first_invalid: Some(entry.sequence),
                };
            }

            let recomputed = Self::compute_hash(
                entry.sequence,
                &entry.event_type,
                &entry.description,
                &entry.actor,
                entry.timestamp,
                &entry.prev_hash,
            );

            if entry.hash != recomputed {
                return ChainVerification {
                    valid: false,
                    entries_checked: checked,
                    first_invalid: Some(entry.sequence),
                };
            }

            checked += 1;
        }

        ChainVerification { valid: true, entries_checked: checked, first_invalid: None }
    }

    /// Get all entries.
    pub fn chain(&self) -> AuditChain {
        let entries = self.inner.entries.read();
        AuditChain { length: entries.len() as u64, entries: entries.clone() }
    }

    /// Get entries filtered by event type.
    pub fn entries_by_type(&self, event_type: &str) -> Vec<AuditEntry> {
        self.inner.entries.read().iter().filter(|e| e.event_type == event_type).cloned().collect()
    }

    /// Get entries for a time range.
    pub fn entries_in_range(&self, start: u64, end: u64) -> Vec<AuditEntry> {
        self.inner
            .entries
            .read()
            .iter()
            .filter(|e| e.timestamp >= start && e.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Count total entries.
    pub fn len(&self) -> usize {
        self.inner.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.entries.read().is_empty()
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_verify() {
        let trail = AuditTrail::new();
        trail.record_at("login", "User logged in", "admin", 1000);
        trail.record_at("access", "Read secret key", "admin", 1001);
        trail.record_at("logout", "User logged out", "admin", 1002);

        let verification = trail.verify();
        assert!(verification.valid);
        assert_eq!(verification.entries_checked, 3);
    }

    #[test]
    fn test_chain_integrity() {
        let trail = AuditTrail::new();
        trail.record_at("a", "first", "sys", 100);
        trail.record_at("b", "second", "sys", 200);

        let chain = trail.chain();
        assert_eq!(chain.length, 2);
        assert_eq!(chain.entries[1].prev_hash, chain.entries[0].hash);
    }

    #[test]
    fn test_empty_trail() {
        let trail = AuditTrail::new();
        assert!(trail.is_empty());
        let v = trail.verify();
        assert!(v.valid);
        assert_eq!(v.entries_checked, 0);
    }

    #[test]
    fn test_filter_by_type() {
        let trail = AuditTrail::new();
        trail.record_at("login", "a", "u1", 100);
        trail.record_at("access", "b", "u1", 200);
        trail.record_at("login", "c", "u2", 300);

        let logins = trail.entries_by_type("login");
        assert_eq!(logins.len(), 2);
    }

    #[test]
    fn test_filter_by_range() {
        let trail = AuditTrail::new();
        trail.record_at("a", "x", "u", 100);
        trail.record_at("b", "y", "u", 200);
        trail.record_at("c", "z", "u", 300);

        let middle = trail.entries_in_range(150, 250);
        assert_eq!(middle.len(), 1);
        assert_eq!(middle[0].event_type, "b");
    }

    #[test]
    fn test_hash_determinism() {
        let h1 = AuditTrail::compute_hash(0, "test", "desc", "actor", 1000, "prev");
        let h2 = AuditTrail::compute_hash(0, "test", "desc", "actor", 1000, "prev");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_different_inputs_different_hashes() {
        let h1 = AuditTrail::compute_hash(0, "a", "d", "actor", 1000, "prev");
        let h2 = AuditTrail::compute_hash(0, "b", "d", "actor", 1000, "prev");
        assert_ne!(h1, h2);
    }
}
