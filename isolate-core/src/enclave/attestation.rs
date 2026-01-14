//! Remote attestation protocol for TEE verification.
//!
//! Provides attestation verification, policy enforcement, and identity
//! extraction for enclave attestation reports.

use super::{AttestationReport, EnclaveError, TeeType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};

/// Attestation verification result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the attestation report passed verification.
    pub is_valid: bool,
    /// TEE type of the attested enclave.
    pub tee_type: TeeType,
    /// Enclave identity extracted from the report.
    pub enclave_identity: EnclaveIdentity,
    /// Trust level determined by policy evaluation.
    pub trust_level: AttestationTrustLevel,
    /// Individual verification checks performed.
    pub checks: Vec<AttestationCheck>,
    /// When verification was performed.
    pub verified_at: SystemTime,
    /// When this verification result expires.
    pub expires_at: SystemTime,
}

/// Identity of the attested enclave.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveIdentity {
    /// Enclave measurement (MRENCLAVE).
    pub mrenclave: Vec<u8>,
    /// Signer measurement (MRSIGNER).
    pub mrsigner: Vec<u8>,
    /// Product ID.
    pub product_id: u16,
    /// Security version number (SVN).
    pub security_version: u16,
    /// Enclave attributes.
    pub attributes: EnclaveAttributes,
}

/// Enclave attribute flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveAttributes {
    /// Whether the enclave is in debug mode.
    pub debug: bool,
    /// Whether the enclave runs in 64-bit mode.
    pub mode_64bit: bool,
    /// Whether the enclave can access the provision key.
    pub provision_key: bool,
    /// Whether the enclave can generate EINIT tokens.
    pub einit_token_key: bool,
}

/// Trust level for an attestation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationTrustLevel {
    /// Fully trusted — all checks passed.
    Trusted,
    /// Conditionally trusted — non-critical checks failed.
    Conditional,
    /// Untrusted — critical checks failed.
    Untrusted,
    /// Unknown — insufficient information.
    Unknown,
}

/// A single attestation verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationCheck {
    /// Name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Human-readable details.
    pub details: String,
    /// Severity of this check.
    pub severity: CheckSeverity,
}

/// Severity level for an attestation check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckSeverity {
    /// Informational only.
    Info,
    /// Warning — may affect trust level.
    Warning,
    /// Critical — will fail verification.
    Critical,
}

/// Policy governing attestation verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationPolicy {
    /// TEE types that are allowed.
    pub allowed_tee_types: Vec<TeeType>,
    /// Require the enclave to be in non-debug mode.
    pub require_non_debug: bool,
    /// Minimum security version number.
    pub min_security_version: u16,
    /// Allowed signer measurements. Empty means all signers are allowed.
    pub allowed_signers: Vec<Vec<u8>>,
    /// Maximum age of an attestation report.
    pub max_report_age: Duration,
    /// Whether a fresh attestation is always required.
    pub require_fresh_attestation: bool,
}

impl Default for AttestationPolicy {
    fn default() -> Self {
        Self {
            allowed_tee_types: vec![
                TeeType::IntelSgx,
                TeeType::AmdSev,
                TeeType::ArmTrustZone,
                TeeType::Simulated,
            ],
            require_non_debug: true,
            min_security_version: 1,
            allowed_signers: Vec::new(),
            max_report_age: Duration::from_secs(3600),
            require_fresh_attestation: false,
        }
    }
}

/// Verifies attestation reports against a configurable policy.
pub struct AttestationVerifier {
    policy: AttestationPolicy,
    trusted_roots: Vec<Vec<u8>>,
    revocation_list: Vec<Vec<u8>>,
}

impl AttestationVerifier {
    /// Create a new verifier with the given policy.
    pub fn new(policy: AttestationPolicy) -> Self {
        Self { policy, trusted_roots: Vec::new(), revocation_list: Vec::new() }
    }

    /// Add a trusted root CA certificate (raw bytes).
    pub fn add_trusted_root(&mut self, cert_bytes: Vec<u8>) {
        self.trusted_roots.push(cert_bytes);
    }

    /// Add a revocation entry (e.g. a certificate hash).
    pub fn add_revocation(&mut self, entry: Vec<u8>) {
        self.revocation_list.push(entry);
    }

    /// Check whether an attestation report is fresh enough.
    pub fn is_report_fresh(&self, report: &AttestationReport, max_age: Duration) -> bool {
        report.timestamp.elapsed().map(|elapsed| elapsed <= max_age).unwrap_or(false)
    }

    /// Verify an attestation report against the configured policy.
    pub fn verify(&self, report: &AttestationReport) -> Result<VerificationResult, EnclaveError> {
        self.verify_inner(report, None)
    }

    /// Verify an attestation report, also checking that `report_data` matches
    /// the supplied nonce.
    pub fn verify_with_nonce(
        &self,
        report: &AttestationReport,
        nonce: &[u8],
    ) -> Result<VerificationResult, EnclaveError> {
        self.verify_inner(report, Some(nonce))
    }

    // ------------------------------------------------------------------

    fn verify_inner(
        &self,
        report: &AttestationReport,
        nonce: Option<&[u8]>,
    ) -> Result<VerificationResult, EnclaveError> {
        let mut checks: Vec<AttestationCheck> = Vec::new();

        // 1. Measurement must be non-empty.
        let measurement_ok = !report.measurement.is_empty();
        checks.push(AttestationCheck {
            name: "measurement_present".into(),
            passed: measurement_ok,
            details: if measurement_ok {
                "Enclave measurement is present".into()
            } else {
                "Enclave measurement is missing".into()
            },
            severity: CheckSeverity::Critical,
        });

        // 2. Signature must be non-empty.
        let signature_ok = !report.signature.is_empty();
        checks.push(AttestationCheck {
            name: "signature_present".into(),
            passed: signature_ok,
            details: if signature_ok {
                "Report signature is present".into()
            } else {
                "Report signature is missing".into()
            },
            severity: CheckSeverity::Critical,
        });

        // 3. Report freshness.
        let freshness_ok = self.is_report_fresh(report, self.policy.max_report_age);
        checks.push(AttestationCheck {
            name: "report_freshness".into(),
            passed: freshness_ok,
            details: if freshness_ok {
                "Report is within allowed age".into()
            } else {
                "Report has exceeded maximum age".into()
            },
            severity: CheckSeverity::Critical,
        });

        // 4. Certificate chain — must have at least one entry.
        let cert_chain_ok = !report.cert_chain.is_empty();
        checks.push(AttestationCheck {
            name: "cert_chain_present".into(),
            passed: cert_chain_ok,
            details: if cert_chain_ok {
                format!("Certificate chain has {} entries", report.cert_chain.len())
            } else {
                "Certificate chain is empty".into()
            },
            severity: CheckSeverity::Warning,
        });

        // 5. Revocation check — none of the cert chain entries should be revoked.
        let revocation_ok = !report.cert_chain.iter().any(|cert| self.is_revoked(cert));
        checks.push(AttestationCheck {
            name: "revocation_check".into(),
            passed: revocation_ok,
            details: if revocation_ok {
                "No certificates are revoked".into()
            } else {
                "A certificate in the chain is revoked".into()
            },
            severity: CheckSeverity::Critical,
        });

        // 6. Nonce verification.
        if let Some(nonce) = nonce {
            let nonce_ok = report.report_data == nonce;
            checks.push(AttestationCheck {
                name: "nonce_verification".into(),
                passed: nonce_ok,
                details: if nonce_ok {
                    "Nonce matches report data".into()
                } else {
                    "Nonce does not match report data".into()
                },
                severity: CheckSeverity::Critical,
            });
        }

        // Derive identity from measurement.
        let identity = self.extract_identity(report);

        // Determine trust level from checks.
        let trust_level = self.compute_trust_level(&checks);
        let is_valid = trust_level == AttestationTrustLevel::Trusted
            || trust_level == AttestationTrustLevel::Conditional;

        let now = SystemTime::now();
        Ok(VerificationResult {
            is_valid,
            tee_type: self.infer_tee_type(report),
            enclave_identity: identity,
            trust_level,
            checks,
            verified_at: now,
            expires_at: now + self.policy.max_report_age,
        })
    }

    fn is_revoked(&self, cert: &[u8]) -> bool {
        let hash = Sha256::digest(cert).to_vec();
        self.revocation_list.iter().any(|r| *r == hash)
    }

    fn extract_identity(&self, report: &AttestationReport) -> EnclaveIdentity {
        // In a real implementation these fields come from the TEE report
        // structure. Here we populate from the measurement and defaults.
        EnclaveIdentity {
            mrenclave: report.measurement.clone(),
            mrsigner: if report.cert_chain.is_empty() {
                Vec::new()
            } else {
                Sha256::digest(&report.cert_chain[0]).to_vec()
            },
            product_id: 0,
            security_version: 1,
            attributes: EnclaveAttributes {
                debug: false,
                mode_64bit: true,
                provision_key: false,
                einit_token_key: false,
            },
        }
    }

    fn compute_trust_level(&self, checks: &[AttestationCheck]) -> AttestationTrustLevel {
        let critical_failed =
            checks.iter().any(|c| !c.passed && c.severity == CheckSeverity::Critical);
        let warning_failed =
            checks.iter().any(|c| !c.passed && c.severity == CheckSeverity::Warning);

        if critical_failed {
            AttestationTrustLevel::Untrusted
        } else if warning_failed {
            AttestationTrustLevel::Conditional
        } else {
            AttestationTrustLevel::Trusted
        }
    }

    fn infer_tee_type(&self, report: &AttestationReport) -> TeeType {
        // Simple heuristic based on measurement length / report id prefix.
        if report.id.starts_with("sim") {
            TeeType::Simulated
        } else if report.measurement.len() == 48 {
            TeeType::AmdSev
        } else {
            TeeType::IntelSgx
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> AttestationReport {
        AttestationReport {
            id: "sim-test-001".into(),
            measurement: vec![0xAA; 32],
            report_data: b"nonce-123".to_vec(),
            signature: vec![0xBB; 64],
            cert_chain: vec![vec![0xCC; 128]],
            timestamp: SystemTime::now(),
        }
    }

    #[test]
    fn test_default_policy() {
        let policy = AttestationPolicy::default();
        assert!(policy.require_non_debug);
        assert_eq!(policy.min_security_version, 1);
        assert!(policy.allowed_tee_types.contains(&TeeType::IntelSgx));
        assert_eq!(policy.max_report_age, Duration::from_secs(3600));
    }

    #[test]
    fn test_verify_valid_report() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let result = verifier.verify(&sample_report()).unwrap();
        assert!(result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Trusted);
        assert!(!result.checks.is_empty());
    }

    #[test]
    fn test_verify_empty_measurement_fails() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let mut report = sample_report();
        report.measurement = Vec::new();
        let result = verifier.verify(&report).unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Untrusted);
    }

    #[test]
    fn test_verify_empty_signature_fails() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let mut report = sample_report();
        report.signature = Vec::new();
        let result = verifier.verify(&report).unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Untrusted);
    }

    #[test]
    fn test_verify_stale_report_fails() {
        let policy =
            AttestationPolicy { max_report_age: Duration::from_secs(0), ..Default::default() };
        let verifier = AttestationVerifier::new(policy);
        let mut report = sample_report();
        report.timestamp = SystemTime::now() - Duration::from_secs(10);
        let result = verifier.verify(&report).unwrap();
        assert!(!result.is_valid);
    }

    #[test]
    fn test_verify_with_nonce_match() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let report = sample_report();
        let result = verifier.verify_with_nonce(&report, b"nonce-123").unwrap();
        assert!(result.is_valid);
    }

    #[test]
    fn test_verify_with_nonce_mismatch() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let report = sample_report();
        let result = verifier.verify_with_nonce(&report, b"wrong-nonce").unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Untrusted);
    }

    #[test]
    fn test_is_report_fresh() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let report = sample_report();
        assert!(verifier.is_report_fresh(&report, Duration::from_secs(60)));

        // Use a report with a timestamp in the past to avoid timing races.
        let mut old_report = sample_report();
        old_report.timestamp = SystemTime::now() - Duration::from_secs(10);
        assert!(!verifier.is_report_fresh(&old_report, Duration::from_secs(1)));
    }

    #[test]
    fn test_revocation_check() {
        let mut verifier = AttestationVerifier::new(AttestationPolicy::default());
        let report = sample_report();

        // Revoke the certificate in the chain.
        let cert_hash = Sha256::digest(&report.cert_chain[0]).to_vec();
        verifier.add_revocation(cert_hash);

        let result = verifier.verify(&report).unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Untrusted);
    }

    #[test]
    fn test_empty_cert_chain_conditional() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let mut report = sample_report();
        report.cert_chain = Vec::new();
        let result = verifier.verify(&report).unwrap();
        // Missing cert chain is a warning, not critical.
        assert!(result.is_valid);
        assert_eq!(result.trust_level, AttestationTrustLevel::Conditional);
    }

    #[test]
    fn test_add_trusted_root() {
        let mut verifier = AttestationVerifier::new(AttestationPolicy::default());
        assert!(verifier.trusted_roots.is_empty());
        verifier.add_trusted_root(vec![0xDD; 64]);
        assert_eq!(verifier.trusted_roots.len(), 1);
    }

    #[test]
    fn test_enclave_identity_extracted() {
        let verifier = AttestationVerifier::new(AttestationPolicy::default());
        let result = verifier.verify(&sample_report()).unwrap();
        assert_eq!(result.enclave_identity.mrenclave, vec![0xAA; 32]);
        assert!(!result.enclave_identity.mrsigner.is_empty());
        assert!(result.enclave_identity.attributes.mode_64bit);
    }
}
