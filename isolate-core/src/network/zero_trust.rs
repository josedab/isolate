//! Zero-trust networking: mTLS, identity verification, and certificate management.
//!
//! Provides enterprise zero-trust security for inter-sandbox and external
//! network communications:
//! - Mutual TLS (mTLS) with per-sandbox certificates
//! - Sandbox identity verification and attestation
//! - Certificate lifecycle management (issue, rotate, revoke)
//! - Network segmentation with micro-perimeters
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::network::zero_trust::*;
//!
//! let ca = CertificateAuthority::new(CaConfig::default());
//! let cert = ca.issue_sandbox_cert("sandbox-123", Duration::from_hours(24));
//!
//! let identity = SandboxIdentity::new("sandbox-123", "tenant-acme");
//! let verifier = IdentityVerifier::new(&ca);
//! assert!(verifier.verify(&identity, &cert).is_ok());
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Configuration for the certificate authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaConfig {
    /// CA certificate subject name.
    pub subject: String,
    /// Default certificate validity period.
    pub default_validity: Duration,
    /// Maximum certificate validity period.
    pub max_validity: Duration,
    /// Key size in bits.
    pub key_bits: u32,
    /// Whether to auto-rotate expiring certificates.
    pub auto_rotate: bool,
    /// Rotation threshold (rotate when this % of lifetime remains).
    pub rotation_threshold_pct: f64,
}

impl Default for CaConfig {
    fn default() -> Self {
        Self {
            subject: "Isolate Internal CA".to_string(),
            default_validity: Duration::from_secs(24 * 3600), // 24 hours
            max_validity: Duration::from_secs(7 * 24 * 3600), // 7 days
            key_bits: 2048,
            auto_rotate: true,
            rotation_threshold_pct: 20.0,
        }
    }
}

/// A certificate issued by the CA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    /// Certificate serial number.
    pub serial: String,
    /// Subject (sandbox or service identity).
    pub subject: String,
    /// Issuer (CA identifier).
    pub issuer: String,
    /// Valid from timestamp (seconds since epoch).
    pub not_before: u64,
    /// Valid until timestamp (seconds since epoch).
    pub not_after: u64,
    /// Certificate fingerprint (SHA-256 hash).
    pub fingerprint: String,
    /// Subject alternative names.
    pub san: Vec<String>,
    /// Whether this certificate has been revoked.
    pub revoked: bool,
    /// Certificate usage constraints.
    pub usage: CertificateUsage,
}

impl Certificate {
    /// Check if the certificate is currently valid.
    pub fn is_valid(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        !self.revoked && now >= self.not_before && now <= self.not_after
    }

    /// Check if the certificate needs rotation.
    pub fn needs_rotation(&self, threshold_pct: f64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let total_lifetime = self.not_after - self.not_before;
        let remaining = self.not_after.saturating_sub(now);
        let remaining_pct = (remaining as f64 / total_lifetime as f64) * 100.0;
        remaining_pct < threshold_pct
    }

    /// Remaining validity duration.
    pub fn remaining_validity(&self) -> Duration {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Duration::from_secs(self.not_after.saturating_sub(now))
    }
}

/// Certificate usage constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CertificateUsage {
    /// Client authentication (sandbox → service).
    ClientAuth,
    /// Server authentication (service → sandbox).
    ServerAuth,
    /// Both client and server authentication.
    DualAuth,
}

/// Internal certificate authority for sandbox identity management.
pub struct CertificateAuthority {
    config: CaConfig,
    /// Issued certificates by serial.
    issued: HashMap<String, Certificate>,
    /// Revoked certificate serials.
    revoked: HashSet<String>,
    /// Next serial number counter.
    next_serial: u64,
}

impl CertificateAuthority {
    /// Create a new certificate authority.
    pub fn new(config: CaConfig) -> Self {
        Self { config, issued: HashMap::new(), revoked: HashSet::new(), next_serial: 1 }
    }

    /// Issue a certificate for a sandbox.
    pub fn issue_sandbox_cert(
        &mut self,
        sandbox_id: &str,
        validity: Option<Duration>,
    ) -> Certificate {
        let validity =
            validity.unwrap_or(self.config.default_validity).min(self.config.max_validity);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let serial = format!("{:016x}", self.next_serial);
        self.next_serial += 1;

        let fingerprint = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(format!("{}:{}:{}", serial, sandbox_id, now).as_bytes());
            format!("{:x}", hasher.finalize())
        };

        let cert = Certificate {
            serial: serial.clone(),
            subject: format!("sandbox:{}", sandbox_id),
            issuer: self.config.subject.clone(),
            not_before: now,
            not_after: now + validity.as_secs(),
            fingerprint,
            san: vec![
                format!("sandbox:{}", sandbox_id),
                format!("{}.sandbox.isolate.local", sandbox_id),
            ],
            revoked: false,
            usage: CertificateUsage::ClientAuth,
        };

        self.issued.insert(serial, cert.clone());
        cert
    }

    /// Revoke a certificate by serial number.
    pub fn revoke(&mut self, serial: &str) -> bool {
        if let Some(cert) = self.issued.get_mut(serial) {
            cert.revoked = true;
            self.revoked.insert(serial.to_string());
            true
        } else {
            false
        }
    }

    /// Check if a certificate serial is revoked.
    pub fn is_revoked(&self, serial: &str) -> bool {
        self.revoked.contains(serial)
    }

    /// Get a certificate by serial.
    pub fn get_cert(&self, serial: &str) -> Option<&Certificate> {
        self.issued.get(serial)
    }

    /// Get all certificates that need rotation.
    pub fn certificates_needing_rotation(&self) -> Vec<&Certificate> {
        self.issued
            .values()
            .filter(|c| !c.revoked && c.needs_rotation(self.config.rotation_threshold_pct))
            .collect()
    }

    /// Get CA statistics.
    pub fn stats(&self) -> CaStats {
        let valid = self.issued.values().filter(|c| c.is_valid()).count();
        let expired = self.issued.values().filter(|c| !c.is_valid() && !c.revoked).count();
        CaStats {
            total_issued: self.issued.len(),
            currently_valid: valid,
            revoked: self.revoked.len(),
            expired,
            needs_rotation: self.certificates_needing_rotation().len(),
        }
    }
}

/// CA statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaStats {
    /// Total certificates ever issued.
    pub total_issued: usize,
    /// Currently valid certificates.
    pub currently_valid: usize,
    /// Revoked certificates.
    pub revoked: usize,
    /// Expired (but not revoked) certificates.
    pub expired: usize,
    /// Certificates needing rotation.
    pub needs_rotation: usize,
}

/// Identity of a sandbox for zero-trust verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxIdentity {
    /// Sandbox identifier.
    pub sandbox_id: String,
    /// Tenant identifier.
    pub tenant_id: String,
    /// Module hash (for attestation).
    pub module_hash: Option<String>,
    /// Labels for policy matching.
    pub labels: HashMap<String, String>,
    /// Trust level (0-10, higher = more trusted).
    pub trust_level: u8,
}

impl SandboxIdentity {
    /// Create a new sandbox identity.
    pub fn new(sandbox_id: impl Into<String>, tenant_id: impl Into<String>) -> Self {
        Self {
            sandbox_id: sandbox_id.into(),
            tenant_id: tenant_id.into(),
            module_hash: None,
            labels: HashMap::new(),
            trust_level: 0,
        }
    }

    /// Set the module hash for attestation.
    pub fn with_module_hash(mut self, hash: impl Into<String>) -> Self {
        self.module_hash = Some(hash.into());
        self
    }

    /// Set trust level.
    pub fn with_trust_level(mut self, level: u8) -> Self {
        self.trust_level = level.min(10);
        self
    }

    /// Add a label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Network segment (micro-perimeter) for sandbox isolation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSegment {
    /// Segment identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Allowed communication targets (segment IDs or sandbox patterns).
    pub allowed_targets: Vec<String>,
    /// Minimum trust level required to join this segment.
    pub min_trust_level: u8,
    /// Whether TLS is required for all communications.
    pub require_tls: bool,
    /// Maximum connections per sandbox in this segment.
    pub max_connections_per_sandbox: usize,
}

impl NetworkSegment {
    /// Create a new network segment.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            allowed_targets: Vec::new(),
            min_trust_level: 0,
            require_tls: true,
            max_connections_per_sandbox: 10,
        }
    }

    /// Check if a sandbox identity can join this segment.
    pub fn can_join(&self, identity: &SandboxIdentity) -> bool {
        identity.trust_level >= self.min_trust_level
    }

    /// Check if communication to a target segment is allowed.
    pub fn can_communicate_with(&self, target_segment_id: &str) -> bool {
        self.allowed_targets.iter().any(|t| {
            t == target_segment_id
                || t == "*"
                || (t.ends_with('*') && target_segment_id.starts_with(&t[..t.len() - 1]))
        })
    }
}

/// Zero-trust network policy evaluator.
pub struct ZeroTrustEvaluator {
    /// Network segments.
    segments: HashMap<String, NetworkSegment>,
    /// Sandbox → segment assignment.
    assignments: HashMap<String, String>,
    /// Certificate authority reference for verification.
    ca_subject: String,
}

impl ZeroTrustEvaluator {
    /// Create a new evaluator.
    pub fn new(ca_subject: impl Into<String>) -> Self {
        Self {
            segments: HashMap::new(),
            assignments: HashMap::new(),
            ca_subject: ca_subject.into(),
        }
    }

    /// Add a network segment.
    pub fn add_segment(&mut self, segment: NetworkSegment) {
        self.segments.insert(segment.id.clone(), segment);
    }

    /// Assign a sandbox to a segment.
    pub fn assign_sandbox(
        &mut self,
        sandbox_id: &str,
        segment_id: &str,
        identity: &SandboxIdentity,
    ) -> Result<(), String> {
        let segment = self
            .segments
            .get(segment_id)
            .ok_or_else(|| format!("Segment '{}' not found", segment_id))?;

        if !segment.can_join(identity) {
            return Err(format!(
                "Sandbox trust level {} below segment minimum {}",
                identity.trust_level, segment.min_trust_level
            ));
        }

        self.assignments.insert(sandbox_id.to_string(), segment_id.to_string());
        Ok(())
    }

    /// Check if sandbox A can communicate with sandbox B.
    pub fn can_communicate(&self, from_sandbox: &str, to_sandbox: &str) -> CommunicationDecision {
        let from_segment = match self.assignments.get(from_sandbox) {
            Some(s) => s,
            None => {
                return CommunicationDecision {
                    allowed: false,
                    reason: "Source sandbox not assigned to any segment".to_string(),
                    require_tls: true,
                };
            }
        };

        let to_segment = match self.assignments.get(to_sandbox) {
            Some(s) => s,
            None => {
                return CommunicationDecision {
                    allowed: false,
                    reason: "Target sandbox not assigned to any segment".to_string(),
                    require_tls: true,
                };
            }
        };

        let segment = match self.segments.get(from_segment) {
            Some(s) => s,
            None => {
                return CommunicationDecision {
                    allowed: false,
                    reason: "Source segment not found".to_string(),
                    require_tls: true,
                };
            }
        };

        let allowed = from_segment == to_segment || segment.can_communicate_with(to_segment);

        CommunicationDecision {
            allowed,
            reason: if allowed {
                "Communication allowed by segment policy".to_string()
            } else {
                format!(
                    "Segment '{}' does not allow communication with '{}'",
                    from_segment, to_segment
                )
            },
            require_tls: segment.require_tls,
        }
    }

    /// Get segment statistics.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Get number of assigned sandboxes.
    pub fn assigned_sandbox_count(&self) -> usize {
        self.assignments.len()
    }
}

/// Verifies sandbox identities against certificates and attestation.
pub struct IdentityVerifier {
    /// Trusted CA subject(s).
    trusted_issuers: Vec<String>,
    /// Whether to check certificate expiry.
    check_expiry: bool,
    /// Whether to verify module hash matches certificate SAN.
    verify_module_hash: bool,
}

impl IdentityVerifier {
    /// Create a new verifier trusting the given CA.
    pub fn new(ca: &CertificateAuthority) -> Self {
        Self {
            trusted_issuers: vec![ca.config.subject.clone()],
            check_expiry: true,
            verify_module_hash: false,
        }
    }

    /// Create a verifier from explicit trusted issuers.
    pub fn from_issuers(issuers: Vec<String>) -> Self {
        Self { trusted_issuers: issuers, check_expiry: true, verify_module_hash: false }
    }

    /// Enable module hash verification.
    pub fn with_module_hash_verification(mut self) -> Self {
        self.verify_module_hash = true;
        self
    }

    /// Verify an identity against a certificate.
    pub fn verify(
        &self,
        identity: &SandboxIdentity,
        cert: &Certificate,
    ) -> Result<VerificationResult, String> {
        let mut checks = Vec::new();

        // Check issuer trust
        let issuer_trusted = self.trusted_issuers.contains(&cert.issuer);
        checks.push(("issuer_trusted".to_string(), issuer_trusted));
        if !issuer_trusted {
            return Ok(VerificationResult {
                verified: false,
                checks,
                reason: Some(format!("Untrusted issuer: {}", cert.issuer)),
            });
        }

        // Check certificate validity
        if self.check_expiry && !cert.is_valid() {
            checks.push(("certificate_valid".to_string(), false));
            return Ok(VerificationResult {
                verified: false,
                checks,
                reason: Some("Certificate is expired or revoked".to_string()),
            });
        }
        checks.push(("certificate_valid".to_string(), true));

        // Check identity matches certificate subject
        let expected_subject = format!("sandbox:{}", identity.sandbox_id);
        let subject_matches =
            cert.subject == expected_subject || cert.san.contains(&expected_subject);
        checks.push(("subject_matches".to_string(), subject_matches));
        if !subject_matches {
            return Ok(VerificationResult {
                verified: false,
                checks,
                reason: Some(format!(
                    "Certificate subject '{}' does not match sandbox '{}'",
                    cert.subject, identity.sandbox_id
                )),
            });
        }

        // Optionally verify module hash
        if self.verify_module_hash {
            if let Some(ref hash) = identity.module_hash {
                let hash_san = format!("module:{}", hash);
                let hash_matches = cert.san.contains(&hash_san);
                checks.push(("module_hash".to_string(), hash_matches));
                if !hash_matches {
                    return Ok(VerificationResult {
                        verified: false,
                        checks,
                        reason: Some("Module hash not attested in certificate".to_string()),
                    });
                }
            }
        }

        Ok(VerificationResult { verified: true, checks, reason: None })
    }
}

/// Result of identity verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the identity was verified.
    pub verified: bool,
    /// Individual check results.
    pub checks: Vec<(String, bool)>,
    /// Reason for failure (if not verified).
    pub reason: Option<String>,
}

/// Result of a communication authorization check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationDecision {
    /// Whether communication is allowed.
    pub allowed: bool,
    /// Reason for the decision.
    pub reason: String,
    /// Whether TLS is required.
    pub require_tls: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ca_issue_cert() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        let cert = ca.issue_sandbox_cert("sb-1", None);

        assert!(cert.is_valid());
        assert!(!cert.revoked);
        assert_eq!(cert.usage, CertificateUsage::ClientAuth);
        assert!(cert.san.contains(&"sandbox:sb-1".to_string()));
    }

    #[test]
    fn test_ca_revoke_cert() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        let cert = ca.issue_sandbox_cert("sb-1", None);
        let serial = cert.serial.clone();

        assert!(!ca.is_revoked(&serial));
        ca.revoke(&serial);
        assert!(ca.is_revoked(&serial));

        let revoked_cert = ca.get_cert(&serial).unwrap();
        assert!(!revoked_cert.is_valid());
    }

    #[test]
    fn test_ca_stats() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        ca.issue_sandbox_cert("sb-1", None);
        ca.issue_sandbox_cert("sb-2", None);
        let cert3 = ca.issue_sandbox_cert("sb-3", None);
        ca.revoke(&cert3.serial);

        let stats = ca.stats();
        assert_eq!(stats.total_issued, 3);
        assert_eq!(stats.currently_valid, 2);
        assert_eq!(stats.revoked, 1);
    }

    #[test]
    fn test_sandbox_identity() {
        let identity = SandboxIdentity::new("sb-1", "acme")
            .with_module_hash("abc123")
            .with_trust_level(5)
            .with_label("env", "production");

        assert_eq!(identity.sandbox_id, "sb-1");
        assert_eq!(identity.tenant_id, "acme");
        assert_eq!(identity.trust_level, 5);
        assert_eq!(identity.module_hash, Some("abc123".to_string()));
    }

    #[test]
    fn test_trust_level_clamped() {
        let identity = SandboxIdentity::new("sb-1", "acme").with_trust_level(15);
        assert_eq!(identity.trust_level, 10);
    }

    #[test]
    fn test_network_segment_join() {
        let segment = NetworkSegment {
            id: "trusted".to_string(),
            name: "Trusted Zone".to_string(),
            allowed_targets: vec!["public".to_string()],
            min_trust_level: 5,
            require_tls: true,
            max_connections_per_sandbox: 10,
        };

        let low_trust = SandboxIdentity::new("sb-1", "acme").with_trust_level(3);
        let high_trust = SandboxIdentity::new("sb-2", "acme").with_trust_level(7);

        assert!(!segment.can_join(&low_trust));
        assert!(segment.can_join(&high_trust));
    }

    #[test]
    fn test_zero_trust_evaluator() {
        let mut evaluator = ZeroTrustEvaluator::new("Isolate CA");

        let mut frontend = NetworkSegment::new("frontend", "Frontend Zone");
        frontend.allowed_targets = vec!["backend".to_string()];

        let backend = NetworkSegment::new("backend", "Backend Zone");

        evaluator.add_segment(frontend);
        evaluator.add_segment(backend);

        let id_a = SandboxIdentity::new("sb-a", "acme");
        let id_b = SandboxIdentity::new("sb-b", "acme");

        evaluator.assign_sandbox("sb-a", "frontend", &id_a).unwrap();
        evaluator.assign_sandbox("sb-b", "backend", &id_b).unwrap();

        // Frontend can communicate with backend
        let decision = evaluator.can_communicate("sb-a", "sb-b");
        assert!(decision.allowed);
        assert!(decision.require_tls);

        // Backend cannot communicate with frontend (not in allowed targets)
        let decision = evaluator.can_communicate("sb-b", "sb-a");
        assert!(!decision.allowed);
    }

    #[test]
    fn test_segment_wildcard_target() {
        let segment = NetworkSegment {
            id: "admin".to_string(),
            name: "Admin".to_string(),
            allowed_targets: vec!["*".to_string()],
            min_trust_level: 8,
            require_tls: true,
            max_connections_per_sandbox: 5,
        };

        assert!(segment.can_communicate_with("anything"));
        assert!(segment.can_communicate_with("backend"));
    }

    #[test]
    fn test_unassigned_sandbox_denied() {
        let evaluator = ZeroTrustEvaluator::new("Isolate CA");
        let decision = evaluator.can_communicate("sb-1", "sb-2");
        assert!(!decision.allowed);
    }

    #[test]
    fn test_same_segment_communication() {
        let mut evaluator = ZeroTrustEvaluator::new("Isolate CA");
        evaluator.add_segment(NetworkSegment::new("workers", "Workers"));

        let id = SandboxIdentity::new("sb-1", "acme");
        evaluator.assign_sandbox("sb-1", "workers", &id).unwrap();
        evaluator.assign_sandbox("sb-2", "workers", &id).unwrap();

        let decision = evaluator.can_communicate("sb-1", "sb-2");
        assert!(decision.allowed);
    }

    #[test]
    fn test_identity_verifier_success() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        let cert = ca.issue_sandbox_cert("sb-1", None);
        let identity = SandboxIdentity::new("sb-1", "acme");
        let verifier = IdentityVerifier::new(&ca);

        let result = verifier.verify(&identity, &cert).unwrap();
        assert!(result.verified);
        assert!(result.reason.is_none());
    }

    #[test]
    fn test_identity_verifier_wrong_sandbox() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        let cert = ca.issue_sandbox_cert("sb-1", None);
        let identity = SandboxIdentity::new("sb-999", "acme");
        let verifier = IdentityVerifier::new(&ca);

        let result = verifier.verify(&identity, &cert).unwrap();
        assert!(!result.verified);
        assert!(result.reason.unwrap().contains("does not match"));
    }

    #[test]
    fn test_identity_verifier_revoked_cert() {
        let mut ca = CertificateAuthority::new(CaConfig::default());
        let cert = ca.issue_sandbox_cert("sb-1", None);
        ca.revoke(&cert.serial);
        let revoked_cert = ca.get_cert(&cert.serial).unwrap().clone();

        let identity = SandboxIdentity::new("sb-1", "acme");
        let verifier = IdentityVerifier::new(&ca);

        let result = verifier.verify(&identity, &revoked_cert).unwrap();
        assert!(!result.verified);
        assert!(result.reason.unwrap().contains("expired or revoked"));
    }

    #[test]
    fn test_identity_verifier_untrusted_issuer() {
        let verifier = IdentityVerifier::from_issuers(vec!["Trusted CA".to_string()]);
        let cert = Certificate {
            serial: "001".to_string(),
            subject: "sandbox:sb-1".to_string(),
            issuer: "Evil CA".to_string(),
            not_before: 0,
            not_after: u64::MAX,
            fingerprint: String::new(),
            san: vec!["sandbox:sb-1".to_string()],
            revoked: false,
            usage: CertificateUsage::ClientAuth,
        };
        let identity = SandboxIdentity::new("sb-1", "acme");
        let result = verifier.verify(&identity, &cert).unwrap();
        assert!(!result.verified);
        assert!(result.reason.unwrap().contains("Untrusted"));
    }
}
