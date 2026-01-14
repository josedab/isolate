//! Attestation audit trail with tamper-evident hash chain.
//!
//! Records attestation events in an append-only log where each entry is
//! chained to the previous via SHA-256, making retroactive tampering
//! detectable.

use super::TeeType;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::SystemTime;

/// An attestation audit event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationAuditEvent {
    /// Unique event identifier.
    pub event_id: String,
    /// Type of audit event.
    pub event_type: AuditEventType,
    /// When the event occurred.
    pub timestamp: SystemTime,
    /// Enclave identifier.
    pub enclave_id: String,
    /// TEE type.
    pub tee_type: TeeType,
    /// Arbitrary key-value details.
    pub details: HashMap<String, String>,
    /// Outcome of the event.
    pub outcome: AuditOutcome,
}

/// Audit event types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventType {
    /// A remote attestation was requested.
    AttestationRequested,
    /// An attestation was verified successfully.
    AttestationVerified,
    /// An attestation verification failed.
    AttestationFailed,
    /// An enclave was created.
    EnclaveCreated,
    /// An enclave was destroyed.
    EnclaveDestroyed,
    /// Data was sealed.
    DataSealed,
    /// Data was unsealed.
    DataUnsealed,
    /// A policy violation was detected.
    PolicyViolation,
    /// A cryptographic key was rotated.
    KeyRotation,
}

/// Outcome of an audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditOutcome {
    /// The operation succeeded.
    Success,
    /// The operation failed.
    Failure {
        /// Reason for failure.
        reason: String,
    },
    /// The operation was denied by policy.
    Denied {
        /// Policy that denied the operation.
        policy: String,
    },
}

/// Immutable, hash-chained audit trail for attestation events.
pub struct AttestationAuditLog {
    events: Vec<AttestationAuditEvent>,
    hash_chain: Vec<Vec<u8>>,
    next_seq: u64,
}

impl AttestationAuditLog {
    /// Create an empty audit log.
    pub fn new() -> Self {
        Self { events: Vec::new(), hash_chain: Vec::new(), next_seq: 1 }
    }

    /// Record an event and return the generated event ID.
    pub fn record(&mut self, mut event: AttestationAuditEvent) -> String {
        let event_id = format!("audit-{:06}", self.next_seq);
        self.next_seq += 1;
        event.event_id = event_id.clone();

        let prev_hash = self.hash_chain.last().cloned().unwrap_or_default();
        let event_json = serde_json::to_string(&event).unwrap_or_default();

        let mut hasher = Sha256::new();
        hasher.update(&prev_hash);
        hasher.update(event_json.as_bytes());
        self.hash_chain.push(hasher.finalize().to_vec());

        self.events.push(event);
        event_id
    }

    /// Verify the integrity of the entire hash chain.
    pub fn verify_integrity(&self) -> bool {
        if self.events.len() != self.hash_chain.len() {
            return false;
        }
        let mut prev_hash: Vec<u8> = Vec::new();
        for (event, stored_hash) in self.events.iter().zip(self.hash_chain.iter()) {
            let event_json = serde_json::to_string(event).unwrap_or_default();
            let mut hasher = Sha256::new();
            hasher.update(&prev_hash);
            hasher.update(event_json.as_bytes());
            let computed = hasher.finalize().to_vec();
            if computed != *stored_hash {
                return false;
            }
            prev_hash = computed;
        }
        true
    }

    /// Return events belonging to a specific enclave.
    pub fn events_for_enclave(&self, enclave_id: &str) -> Vec<&AttestationAuditEvent> {
        self.events.iter().filter(|e| e.enclave_id == enclave_id).collect()
    }

    /// Return events of a specific type.
    pub fn events_by_type(&self, event_type: &AuditEventType) -> Vec<&AttestationAuditEvent> {
        self.events.iter().filter(|e| e.event_type == *event_type).collect()
    }

    /// Return events that occurred at or after `since`.
    pub fn events_since(&self, since: SystemTime) -> Vec<&AttestationAuditEvent> {
        self.events.iter().filter(|e| e.timestamp >= since).collect()
    }

    /// Export the full log as JSON.
    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&self.events).unwrap_or_default()
    }

    /// Number of events in the log.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl Default for AttestationAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
fn make_event(
    event_type: AuditEventType,
    enclave_id: &str,
    outcome: AuditOutcome,
) -> AttestationAuditEvent {
    AttestationAuditEvent {
        event_id: String::new(), // filled by record()
        event_type,
        timestamp: SystemTime::now(),
        enclave_id: enclave_id.to_string(),
        tee_type: TeeType::Simulated,
        details: HashMap::new(),
        outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn simple_event(event_type: AuditEventType, enclave_id: &str) -> AttestationAuditEvent {
        make_event(event_type, enclave_id, AuditOutcome::Success)
    }

    #[test]
    fn test_record_and_len() {
        let mut log = AttestationAuditLog::new();
        assert!(log.is_empty());
        let id = log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        assert_eq!(log.len(), 1);
        assert!(id.starts_with("audit-"));
    }

    #[test]
    fn test_verify_integrity_empty() {
        let log = AttestationAuditLog::new();
        assert!(log.verify_integrity());
    }

    #[test]
    fn test_verify_integrity_with_events() {
        let mut log = AttestationAuditLog::new();
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        log.record(simple_event(AuditEventType::AttestationRequested, "enc-1"));
        log.record(simple_event(AuditEventType::DataSealed, "enc-1"));
        assert!(log.verify_integrity());
    }

    #[test]
    fn test_tampered_hash_chain_detected() {
        let mut log = AttestationAuditLog::new();
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        log.record(simple_event(AuditEventType::AttestationVerified, "enc-1"));
        // Tamper with the first hash.
        log.hash_chain[0] = vec![0xFF; 32];
        assert!(!log.verify_integrity());
    }

    #[test]
    fn test_events_for_enclave() {
        let mut log = AttestationAuditLog::new();
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-2"));
        log.record(simple_event(AuditEventType::DataSealed, "enc-1"));
        assert_eq!(log.events_for_enclave("enc-1").len(), 2);
        assert_eq!(log.events_for_enclave("enc-2").len(), 1);
        assert_eq!(log.events_for_enclave("enc-3").len(), 0);
    }

    #[test]
    fn test_events_by_type() {
        let mut log = AttestationAuditLog::new();
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        log.record(simple_event(AuditEventType::AttestationVerified, "enc-1"));
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-2"));
        let created = log.events_by_type(&AuditEventType::EnclaveCreated);
        assert_eq!(created.len(), 2);
    }

    #[test]
    fn test_events_since() {
        let mut log = AttestationAuditLog::new();
        let before = SystemTime::now();
        std::thread::sleep(Duration::from_millis(10));
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        let events = log.events_since(before);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_export_json() {
        let mut log = AttestationAuditLog::new();
        log.record(simple_event(AuditEventType::EnclaveCreated, "enc-1"));
        let json = log.export_json();
        assert!(json.contains("enc-1"));
        assert!(json.contains("EnclaveCreated"));
    }

    #[test]
    fn test_event_ids_are_sequential() {
        let mut log = AttestationAuditLog::new();
        let id1 = log.record(simple_event(AuditEventType::EnclaveCreated, "a"));
        let id2 = log.record(simple_event(AuditEventType::EnclaveDestroyed, "a"));
        assert_eq!(id1, "audit-000001");
        assert_eq!(id2, "audit-000002");
    }

    #[test]
    fn test_failure_outcome() {
        let mut log = AttestationAuditLog::new();
        let event = make_event(
            AuditEventType::AttestationFailed,
            "enc-1",
            AuditOutcome::Failure { reason: "expired cert".into() },
        );
        log.record(event);
        assert!(log.verify_integrity());
        let events = log.events_by_type(&AuditEventType::AttestationFailed);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].outcome,
            AuditOutcome::Failure { reason } if reason == "expired cert"
        ));
    }

    #[test]
    fn test_denied_outcome() {
        let mut log = AttestationAuditLog::new();
        let event = make_event(
            AuditEventType::PolicyViolation,
            "enc-1",
            AuditOutcome::Denied { policy: "no-debug".into() },
        );
        log.record(event);
        let events = log.events_by_type(&AuditEventType::PolicyViolation);
        assert!(matches!(
            &events[0].outcome,
            AuditOutcome::Denied { policy } if policy == "no-debug"
        ));
    }
}
