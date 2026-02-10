//! Chain verification utilities.

use super::entry::AuditEntry;
use super::AuditHash;

/// Error during chain verification.
#[derive(Debug, thiserror::Error)]
pub enum ChainVerificationError {
    /// Chain is empty.
    #[error("Audit chain is empty")]
    EmptyChain,

    /// Hash mismatch at a specific entry.
    #[error("Hash mismatch at entry {sequence}: expected {expected}, got {actual}")]
    HashMismatch { sequence: u64, expected: String, actual: String },

    /// Chain link broken (previous hash doesn't match).
    #[error("Chain broken at entry {sequence}: previous hash {expected} != {actual}")]
    ChainBroken { sequence: u64, expected: String, actual: String },

    /// Sequence number gap or mismatch.
    #[error("Sequence mismatch at entry: expected {expected}, got {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },

    /// Signature verification failed.
    #[error("Signature verification failed at entry {sequence}")]
    SignatureInvalid { sequence: u64 },

    /// Missing signature when expected.
    #[error("Missing signature at entry {sequence}")]
    MissingSignature { sequence: u64 },
}

/// Chain verifier utility.
pub struct ChainVerifier;

impl ChainVerifier {
    /// Verify the integrity of an audit chain.
    pub fn verify_chain(entries: &[AuditEntry]) -> Result<(), ChainVerificationError> {
        if entries.is_empty() {
            return Err(ChainVerificationError::EmptyChain);
        }

        let mut expected_previous_hash: AuditHash = [0u8; 32];

        for (expected_sequence, entry) in entries.iter().enumerate() {
            let expected_sequence = expected_sequence as u64;
            // Verify sequence
            if entry.sequence != expected_sequence {
                return Err(ChainVerificationError::SequenceMismatch {
                    expected: expected_sequence,
                    actual: entry.sequence,
                });
            }

            // Verify previous hash (genesis entry should have all zeros)
            if entry.previous_hash != expected_previous_hash {
                return Err(ChainVerificationError::ChainBroken {
                    sequence: entry.sequence,
                    expected: hex::encode(&expected_previous_hash[..8]),
                    actual: hex::encode(&entry.previous_hash[..8]),
                });
            }

            // Verify entry hash
            let computed_hash = entry.compute_hash();
            if entry.hash != computed_hash {
                return Err(ChainVerificationError::HashMismatch {
                    sequence: entry.sequence,
                    expected: hex::encode(&computed_hash[..8]),
                    actual: hex::encode(&entry.hash[..8]),
                });
            }

            // Update expectations for next entry
            expected_previous_hash = entry.hash;
        }

        Ok(())
    }

    /// Verify signatures on all entries.
    pub fn verify_signatures(
        entries: &[AuditEntry],
        key: &[u8],
    ) -> Result<(), ChainVerificationError> {
        for entry in entries {
            if entry.signature.is_none() {
                return Err(ChainVerificationError::MissingSignature { sequence: entry.sequence });
            }

            if !entry.verify_signature(key) {
                return Err(ChainVerificationError::SignatureInvalid { sequence: entry.sequence });
            }
        }

        Ok(())
    }

    /// Verify a single entry's hash and signature.
    pub fn verify_entry(
        entry: &AuditEntry,
        key: Option<&[u8]>,
    ) -> Result<(), ChainVerificationError> {
        // Verify hash
        let computed_hash = entry.compute_hash();
        if entry.hash != computed_hash {
            return Err(ChainVerificationError::HashMismatch {
                sequence: entry.sequence,
                expected: hex::encode(&computed_hash[..8]),
                actual: hex::encode(&entry.hash[..8]),
            });
        }

        // Verify signature if key provided
        if let Some(k) = key {
            if entry.signature.is_none() {
                return Err(ChainVerificationError::MissingSignature { sequence: entry.sequence });
            }

            if !entry.verify_signature(k) {
                return Err(ChainVerificationError::SignatureInvalid { sequence: entry.sequence });
            }
        }

        Ok(())
    }

    /// Find the first tampered entry in a chain.
    pub fn find_tampered(entries: &[AuditEntry]) -> Option<u64> {
        if entries.is_empty() {
            return None;
        }

        let mut expected_previous_hash: AuditHash = [0u8; 32];

        for (i, entry) in entries.iter().enumerate() {
            // Check sequence
            if entry.sequence != i as u64 {
                return Some(entry.sequence);
            }

            // Check previous hash
            if entry.previous_hash != expected_previous_hash {
                return Some(entry.sequence);
            }

            // Check hash
            let computed_hash = entry.compute_hash();
            if entry.hash != computed_hash {
                return Some(entry.sequence);
            }

            expected_previous_hash = entry.hash;
        }

        None
    }

    /// Get a summary of chain statistics.
    pub fn chain_stats(entries: &[AuditEntry]) -> ChainStats {
        let mut stats = ChainStats { total_entries: entries.len(), ..Default::default() };

        if let Some(first) = entries.first() {
            stats.first_timestamp = Some(first.timestamp);
        }

        if let Some(last) = entries.last() {
            stats.last_timestamp = Some(last.timestamp);
            stats.head_hash = Some(last.hash);
        }

        for entry in entries {
            if entry.signature.is_some() {
                stats.signed_entries += 1;
            }
        }

        stats.is_valid = Self::verify_chain(entries).is_ok();

        stats
    }
}

/// Statistics about an audit chain.
#[derive(Debug, Default)]
pub struct ChainStats {
    /// Total number of entries.
    pub total_entries: usize,
    /// Number of signed entries.
    pub signed_entries: usize,
    /// First entry timestamp.
    pub first_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Last entry timestamp.
    pub last_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    /// Head hash.
    pub head_hash: Option<AuditHash>,
    /// Whether the chain is valid.
    pub is_valid: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::entry::AuditAction;
    use uuid::Uuid;

    fn create_test_chain(count: usize) -> Vec<AuditEntry> {
        let sandbox_id = Uuid::new_v4();
        let mut entries = Vec::new();
        let mut previous_hash: AuditHash = [0u8; 32];

        for i in 0..count {
            let entry = AuditEntry::new(
                sandbox_id,
                AuditAction::Custom {
                    action: format!("event_{}", i),
                    data: serde_json::Value::Null,
                },
                None,
            )
            .finalize_with_chain(i as u64, previous_hash);

            previous_hash = entry.hash;
            entries.push(entry);
        }

        entries
    }

    #[test]
    fn test_verify_valid_chain() {
        let entries = create_test_chain(5);
        assert!(ChainVerifier::verify_chain(&entries).is_ok());
    }

    #[test]
    fn test_verify_empty_chain() {
        let entries: Vec<AuditEntry> = vec![];
        assert!(matches!(
            ChainVerifier::verify_chain(&entries),
            Err(ChainVerificationError::EmptyChain)
        ));
    }

    #[test]
    fn test_verify_tampered_hash() {
        let mut entries = create_test_chain(3);

        // Tamper with the hash
        entries[1].hash[0] ^= 0xFF;

        let result = ChainVerifier::verify_chain(&entries);
        assert!(matches!(result, Err(ChainVerificationError::HashMismatch { .. })));
    }

    #[test]
    fn test_verify_broken_chain() {
        let mut entries = create_test_chain(3);

        // Break the chain by modifying previous_hash
        entries[2].previous_hash[0] ^= 0xFF;
        // Also need to recompute hash to pass hash check
        entries[2].hash = entries[2].compute_hash();

        let result = ChainVerifier::verify_chain(&entries);
        assert!(matches!(result, Err(ChainVerificationError::ChainBroken { .. })));
    }

    #[test]
    fn test_verify_sequence_mismatch() {
        let mut entries = create_test_chain(3);

        // Skip a sequence number
        entries[2].sequence = 5;

        let result = ChainVerifier::verify_chain(&entries);
        assert!(matches!(result, Err(ChainVerificationError::SequenceMismatch { .. })));
    }

    #[test]
    fn test_find_tampered() {
        let mut entries = create_test_chain(5);

        // No tampering
        assert!(ChainVerifier::find_tampered(&entries).is_none());

        // Tamper with entry 3
        entries[3].hash[0] ^= 0xFF;
        assert_eq!(ChainVerifier::find_tampered(&entries), Some(3));
    }

    #[test]
    fn test_chain_stats() {
        let entries = create_test_chain(5);
        let stats = ChainVerifier::chain_stats(&entries);

        assert_eq!(stats.total_entries, 5);
        assert!(stats.is_valid);
        assert!(stats.first_timestamp.is_some());
        assert!(stats.last_timestamp.is_some());
        assert!(stats.head_hash.is_some());
    }

    #[test]
    fn test_verify_signatures() {
        let sandbox_id = Uuid::new_v4();
        let key = b"test-key-12345";
        let mut entries = Vec::new();
        let mut previous_hash: AuditHash = [0u8; 32];

        for i in 0..3 {
            let entry = AuditEntry::new(
                sandbox_id,
                AuditAction::Custom {
                    action: format!("event_{}", i),
                    data: serde_json::Value::Null,
                },
                None,
            )
            .finalize_with_chain(i as u64, previous_hash)
            .sign(key);

            previous_hash = entry.hash;
            entries.push(entry);
        }

        assert!(ChainVerifier::verify_signatures(&entries, key).is_ok());
        assert!(ChainVerifier::verify_signatures(&entries, b"wrong-key").is_err());
    }
}
