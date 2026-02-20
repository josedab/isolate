//! Security layer for the WASM module registry.
//!
//! Provides cryptographic signing, provenance tracking, and vulnerability
//! scanning for WASM modules distributed through the OCI registry.

#![allow(missing_docs)]
pub mod signing;
pub mod supply_chain;
pub mod verification;

// Re-export all public types for backward compatibility.
pub use signing::*;
pub use supply_chain::*;
pub use verification::*;

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Orchestrator: RegistrySecurity
// ---------------------------------------------------------------------------

/// Combined security assessment for a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    /// Whether the signature was valid.
    pub signature_valid: bool,
    /// Provenance record, if available.
    pub provenance: Option<ProvenanceRecord>,
    /// Vulnerability scan results.
    pub vulnerability_report: VulnerabilityReport,
    /// Overall pass/fail verdict.
    pub overall_verdict: SecurityVerdict,
}

/// Final security verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityVerdict {
    /// Module passed all checks.
    Pass,
    /// Module passed but has warnings.
    Warn,
    /// Module failed one or more checks.
    Fail,
}

/// Combines signing, provenance tracking, and vulnerability scanning.
pub struct RegistrySecurity {
    scanner: VulnerabilityScanner,
    provenance_store: ProvenanceStore,
}

impl RegistrySecurity {
    /// Create a new `RegistrySecurity` instance.
    pub fn new() -> Self {
        Self {
            scanner: VulnerabilityScanner::new(),
            provenance_store: ProvenanceStore::new(),
        }
    }

    /// Sign a module, create a default provenance record, and store it.
    pub fn sign_and_store(
        &mut self,
        module_bytes: &[u8],
        signing_key: &SigningKey,
    ) -> Result<(ModuleSignature, ProvenanceRecord), String> {
        let signature = signing_key.sign(module_bytes);

        let provenance = ProvenanceRecord {
            module_hash: signature.module_hash.clone(),
            builder: signing_key.key_id.clone(),
            source_repo: String::new(),
            build_timestamp: Utc::now(),
            build_command: String::new(),
            dependencies: Vec::new(),
            reproducible: false,
        };

        self.provenance_store.record(provenance.clone())?;

        Ok((signature, provenance))
    }

    /// Verify a module's signature, look up provenance, and run a scan.
    pub fn verify_and_scan(
        &self,
        module_bytes: &[u8],
        signature: &ModuleSignature,
        key: &[u8],
    ) -> SecurityReport {
        let sig_valid = signature.verify(key, module_bytes);
        let module_hash = helpers::sha256_hex(module_bytes);
        let provenance = self.provenance_store.get(&module_hash).cloned();
        let vulnerability_report = self.scanner.scan(module_bytes);

        let overall_verdict = if !sig_valid {
            SecurityVerdict::Fail
        } else if vulnerability_report.overall_risk == RiskLevel::Critical
            || vulnerability_report.overall_risk == RiskLevel::High
        {
            SecurityVerdict::Fail
        } else if vulnerability_report.overall_risk == RiskLevel::Medium {
            SecurityVerdict::Warn
        } else {
            SecurityVerdict::Pass
        };

        SecurityReport {
            signature_valid: sig_valid,
            provenance,
            vulnerability_report,
            overall_verdict,
        }
    }

    /// Access the inner provenance store.
    pub fn provenance_store(&self) -> &ProvenanceStore {
        &self.provenance_store
    }

    /// Access the inner provenance store mutably.
    pub fn provenance_store_mut(&mut self) -> &mut ProvenanceStore {
        &mut self.provenance_store
    }

    /// Access the inner vulnerability scanner mutably (e.g. to add rules).
    pub fn scanner_mut(&mut self) -> &mut VulnerabilityScanner {
        &mut self.scanner
    }
}

impl Default for RegistrySecurity {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers (crate-internal)
// ---------------------------------------------------------------------------

pub(crate) mod helpers {
    use sha2::{Digest, Sha256};

    /// Compute the SHA-256 hex digest of `data`.
    pub fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }

    /// HMAC-SHA256 implemented via the standard construction (RFC 2104).
    ///
    /// Uses the `sha2` crate directly so we don't need an extra `hmac` dependency.
    pub fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
        const BLOCK_SIZE: usize = 64;

        // Step 1: normalise key to block size
        let normalised = if key.len() > BLOCK_SIZE {
            let mut h = Sha256::new();
            h.update(key);
            h.finalize().to_vec()
        } else {
            key.to_vec()
        };

        let mut padded = vec![0u8; BLOCK_SIZE];
        padded[..normalised.len()].copy_from_slice(&normalised);

        // Step 2: inner hash – H((K ⊕ ipad) || data)
        let mut i_key_pad = vec![0x36u8; BLOCK_SIZE];
        for (i, b) in padded.iter().enumerate() {
            i_key_pad[i] ^= b;
        }

        let mut inner = Sha256::new();
        inner.update(&i_key_pad);
        inner.update(data);
        let inner_hash = inner.finalize();

        // Step 3: outer hash – H((K ⊕ opad) || inner_hash)
        let mut o_key_pad = vec![0x5cu8; BLOCK_SIZE];
        for (i, b) in padded.iter().enumerate() {
            o_key_pad[i] ^= b;
        }

        let mut outer = Sha256::new();
        outer.update(&o_key_pad);
        outer.update(inner_hash);

        hex::encode(outer.finalize())
    }

    /// Decode an unsigned LEB128 value. Returns `(value, bytes_consumed)`.
    pub fn read_leb128_u32(bytes: &[u8], start: usize) -> (u32, usize) {
        let mut result: u32 = 0;
        let mut shift = 0u32;
        let mut pos = start;
        loop {
            if pos >= bytes.len() {
                break;
            }
            let byte = bytes[pos];
            pos += 1;
            result |= ((byte & 0x7f) as u32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 35 {
                break;
            }
        }
        (result, pos - start)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use helpers::*;

    fn sample_bytes() -> Vec<u8> {
        b"hello wasm module".to_vec()
    }

    fn make_key() -> SigningKey {
        SigningKey::new_hmac("test-key-1", b"supersecret")
    }

    // -- Signing -----------------------------------------------------------

    #[test]
    fn test_signing_key_sign_produces_valid_signature() {
        let key = make_key();
        let data = sample_bytes();
        let sig = key.sign(&data);
        assert!(sig.verify(key.as_bytes(), &data));
    }

    #[test]
    fn test_signature_rejects_wrong_key() {
        let key = make_key();
        let sig = key.sign(&sample_bytes());
        assert!(!sig.verify(b"wrong-key", &sample_bytes()));
    }

    #[test]
    fn test_signature_rejects_tampered_data() {
        let key = make_key();
        let sig = key.sign(&sample_bytes());
        assert!(!sig.verify(key.as_bytes(), b"tampered"));
    }

    #[test]
    fn test_signature_module_hash_is_sha256() {
        let key = make_key();
        let data = sample_bytes();
        let sig = key.sign(&data);
        assert_eq!(sig.module_hash, sha256_hex(&data));
    }

    #[test]
    fn test_ed25519_verify_returns_false() {
        let sig = ModuleSignature {
            module_hash: "abc".into(),
            signature: "def".into(),
            algorithm: SignatureAlgorithm::Ed25519,
            signer_id: "s".into(),
            signed_at: Utc::now(),
            expires_at: None,
        };
        assert!(!sig.verify(b"any", b"any"));
    }

    // -- Verifier ----------------------------------------------------------

    #[test]
    fn test_verifier_single_valid() {
        let key = make_key();
        let data = sample_bytes();
        let sig = key.sign(&data);
        let result = SignatureVerifier::verify_signature(&data, &sig, key.as_bytes());
        assert!(result.valid);
    }

    #[test]
    fn test_verifier_single_invalid() {
        let key = make_key();
        let data = sample_bytes();
        let sig = key.sign(&data);
        let result = SignatureVerifier::verify_signature(b"bad", &sig, key.as_bytes());
        assert!(!result.valid);
    }

    #[test]
    fn test_verifier_chain_all_valid() {
        let key = make_key();
        let data = sample_bytes();
        let s1 = key.sign(&data);
        let s2 = key.sign(&data);
        let chain = SignatureVerifier::verify_chain(&data, &[s1, s2], key.as_bytes());
        assert!(chain.all_valid);
        assert_eq!(chain.results.len(), 2);
    }

    #[test]
    fn test_verifier_chain_empty_is_invalid() {
        let chain = SignatureVerifier::verify_chain(&sample_bytes(), &[], b"k");
        assert!(!chain.all_valid);
    }

    #[test]
    fn test_verifier_chain_one_bad() {
        let key = make_key();
        let data = sample_bytes();
        let good = key.sign(&data);
        let mut bad = key.sign(&data);
        bad.signature = "0000".into();
        let chain = SignatureVerifier::verify_chain(&data, &[good, bad], key.as_bytes());
        assert!(!chain.all_valid);
    }

    // -- Provenance --------------------------------------------------------

    #[test]
    fn test_provenance_store_record_and_get() {
        let mut store = ProvenanceStore::new();
        let rec = ProvenanceRecord {
            module_hash: "abc123".into(),
            builder: "ci".into(),
            source_repo: "https://github.com/example/repo".into(),
            build_timestamp: Utc::now(),
            build_command: "cargo build".into(),
            dependencies: vec![],
            reproducible: true,
        };
        store.record(rec).unwrap();
        assert!(store.get("abc123").is_some());
    }

    #[test]
    fn test_provenance_store_empty_hash_rejected() {
        let mut store = ProvenanceStore::new();
        let rec = ProvenanceRecord {
            module_hash: "".into(),
            builder: "ci".into(),
            source_repo: "".into(),
            build_timestamp: Utc::now(),
            build_command: "".into(),
            dependencies: vec![],
            reproducible: false,
        };
        assert!(store.record(rec).is_err());
    }

    #[test]
    fn test_provenance_store_missing_hash() {
        let store = ProvenanceStore::new();
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn test_supply_chain_report_no_provenance() {
        let store = ProvenanceStore::new();
        let report = store.verify_supply_chain("missing");
        assert!(!report.has_provenance);
    }

    #[test]
    fn test_supply_chain_report_with_deps() {
        let mut store = ProvenanceStore::new();
        let rec = ProvenanceRecord {
            module_hash: "h1".into(),
            builder: "ci".into(),
            source_repo: "".into(),
            build_timestamp: Utc::now(),
            build_command: "make".into(),
            dependencies: vec![DependencyRef {
                name: "libc".into(),
                version: "0.2".into(),
                hash: "deadbeef".into(),
            }],
            reproducible: true,
        };
        store.record(rec).unwrap();
        let report = store.verify_supply_chain("h1");
        assert!(report.has_provenance);
        assert!(report.reproducible);
        assert_eq!(report.dependency_count, 1);
        assert!(report.all_deps_verified);
    }

    #[test]
    fn test_slsa_predicate_structure() {
        let rec = ProvenanceRecord {
            module_hash: "aaa".into(),
            builder: "github-actions".into(),
            source_repo: "https://github.com/org/repo".into(),
            build_timestamp: Utc::now(),
            build_command: "cargo build --release".into(),
            dependencies: vec![DependencyRef {
                name: "dep".into(),
                version: "1.0".into(),
                hash: "ff".into(),
            }],
            reproducible: true,
        };
        let pred = rec.to_slsa_predicate();
        assert_eq!(pred["builder"]["id"], "github-actions");
        assert!(pred["materials"].as_array().unwrap().len() == 1);
        assert_eq!(pred["metadata"]["reproducible"], true);
    }

    // -- Vulnerability scanning --------------------------------------------

    #[test]
    fn test_scanner_clean_module() {
        let scanner = VulnerabilityScanner::new();
        let report = scanner.scan(b"\x00asm\x01\x00\x00\x00");
        assert_eq!(report.overall_risk, RiskLevel::None);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn test_scanner_detects_sock_accept() {
        let scanner = VulnerabilityScanner::new();
        let report = scanner.scan(b"sock_accept");
        assert_eq!(report.overall_risk, RiskLevel::High);
    }

    #[test]
    fn test_scanner_detects_eval() {
        let scanner = VulnerabilityScanner::new();
        let report = scanner.scan(b"eval something");
        assert_eq!(report.overall_risk, RiskLevel::Medium);
    }

    #[test]
    fn test_scanner_custom_rule() {
        let mut scanner = VulnerabilityScanner::new();
        scanner.add_rule(ScanRule {
            id: "CUSTOM-001".into(),
            pattern: "backdoor".into(),
            severity: Severity::Critical,
            title: "Backdoor detected".into(),
            description: "Contains backdoor string".into(),
            remediation: Some("Remove backdoor".into()),
        });
        let report = scanner.scan(b"this has a backdoor");
        assert_eq!(report.overall_risk, RiskLevel::Critical);
    }

    #[test]
    fn test_scanner_severity_counts() {
        let scanner = VulnerabilityScanner::new();
        // Contains both proc_exit (Low) and fd_write (Info)
        let report = scanner.scan(b"proc_exit fd_write");
        assert_eq!(*report.severity_counts.get(&Severity::Low).unwrap_or(&0), 1);
        assert_eq!(
            *report.severity_counts.get(&Severity::Info).unwrap_or(&0),
            1
        );
    }

    // -- RegistrySecurity --------------------------------------------------

    #[test]
    fn test_registry_security_sign_and_store() {
        let mut sec = RegistrySecurity::new();
        let key = make_key();
        let data = sample_bytes();
        let (sig, prov) = sec.sign_and_store(&data, &key).unwrap();
        assert_eq!(sig.module_hash, prov.module_hash);
        assert!(sec.provenance_store().get(&sig.module_hash).is_some());
    }

    #[test]
    fn test_registry_security_verify_and_scan_pass() {
        let mut sec = RegistrySecurity::new();
        let key = make_key();
        let data = b"\x00asm\x01\x00\x00\x00".to_vec();
        let (sig, _) = sec.sign_and_store(&data, &key).unwrap();
        let report = sec.verify_and_scan(&data, &sig, key.as_bytes());
        assert!(report.signature_valid);
        assert_eq!(report.overall_verdict, SecurityVerdict::Pass);
    }

    #[test]
    fn test_registry_security_verify_and_scan_fail_sig() {
        let sec = RegistrySecurity::new();
        let key = make_key();
        let data = b"\x00asm\x01\x00\x00\x00".to_vec();
        let sig = key.sign(&data);
        let report = sec.verify_and_scan(&data, &sig, b"wrong");
        assert!(!report.signature_valid);
        assert_eq!(report.overall_verdict, SecurityVerdict::Fail);
    }

    #[test]
    fn test_registry_security_verify_and_scan_warn() {
        let mut sec = RegistrySecurity::new();
        let key = make_key();
        // Module contains "eval" → Medium severity → Warn verdict
        let data = b"eval code here";
        let (sig, _) = sec.sign_and_store(data, &key).unwrap();
        let report = sec.verify_and_scan(data, &sig, key.as_bytes());
        assert!(report.signature_valid);
        assert_eq!(report.overall_verdict, SecurityVerdict::Warn);
    }

    // -- HMAC edge cases ---------------------------------------------------

    #[test]
    fn test_hmac_deterministic() {
        let a = hmac_sha256(b"key", b"data");
        let b = hmac_sha256(b"key", b"data");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hmac_different_keys_differ() {
        let a = hmac_sha256(b"key1", b"data");
        let b = hmac_sha256(b"key2", b"data");
        assert_ne!(a, b);
    }

    #[test]
    fn test_hmac_long_key() {
        // Key longer than block size (64 bytes) is hashed first.
        let long_key = vec![0xABu8; 128];
        let result = hmac_sha256(&long_key, b"msg");
        assert_eq!(result.len(), 64); // 32 bytes hex-encoded
    }

    #[test]
    fn test_hmac_key_exactly_block_size() {
        let key = vec![0x42u8; 64]; // exactly block size
        let result = hmac_sha256(&key, b"data");
        assert_eq!(result.len(), 64);
        // Should be deterministic
        assert_eq!(result, hmac_sha256(&key, b"data"));
    }

    #[test]
    fn test_hmac_empty_data() {
        let result = hmac_sha256(b"key", b"");
        assert_eq!(result.len(), 64);
        assert_ne!(result, hmac_sha256(b"key", b"notempty"));
    }

    #[test]
    fn test_hmac_empty_key() {
        let result = hmac_sha256(b"", b"data");
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_ed25519_verify_always_returns_false() {
        // Ed25519 is stubbed and should always return false regardless of inputs
        let sig = ModuleSignature {
            module_hash: "hash".into(),
            signature: "sig".into(),
            algorithm: SignatureAlgorithm::Ed25519,
            signer_id: "signer".into(),
            signed_at: Utc::now(),
            expires_at: None,
        };
        assert!(!sig.verify(b"key", b"data"));
        assert!(!sig.verify(b"", b""));
        assert!(!sig.verify(&[0u8; 256], &[0u8; 1024]));
    }

    #[test]
    fn test_leb128_single_byte() {
        let bytes = [0x05]; // 5
        let (val, consumed) = read_leb128_u32(&bytes, 0);
        assert_eq!(val, 5);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_leb128_multi_byte() {
        let bytes = [0x80, 0x01]; // 128
        let (val, consumed) = read_leb128_u32(&bytes, 0);
        assert_eq!(val, 128);
        assert_eq!(consumed, 2);
    }

    #[test]
    fn test_leb128_truncated_input() {
        // High bit set but no continuation byte
        let bytes = [0x80];
        let (val, consumed) = read_leb128_u32(&bytes, 0);
        assert_eq!(consumed, 1);
        assert_eq!(val, 0); // partial decode
    }

    #[test]
    fn test_leb128_empty_input() {
        let bytes: [u8; 0] = [];
        let (val, consumed) = read_leb128_u32(&bytes, 0);
        assert_eq!(val, 0);
        assert_eq!(consumed, 0);
    }

    #[test]
    fn test_leb128_max_u32() {
        // u32::MAX = 4294967295 = 0xFF_FF_FF_FF
        // LEB128: [0xFF, 0xFF, 0xFF, 0xFF, 0x0F]
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        let (val, consumed) = read_leb128_u32(&bytes, 0);
        assert_eq!(val, u32::MAX);
        assert_eq!(consumed, 5);
    }

    #[test]
    fn test_leb128_overflow_protection() {
        // Too many continuation bytes (shift >= 35 stops)
        let bytes = [0x80, 0x80, 0x80, 0x80, 0x80, 0x01];
        let (_, consumed) = read_leb128_u32(&bytes, 0);
        // Should stop at shift=35 (5 bytes consumed)
        assert!(consumed <= 6);
    }

    #[test]
    fn test_leb128_with_offset() {
        let bytes = [0x00, 0x00, 0x2A]; // 42 at offset 2
        let (val, consumed) = read_leb128_u32(&bytes, 2);
        assert_eq!(val, 42);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn test_chain_with_mixed_valid_invalid() {
        let key = make_key();
        let data = sample_bytes();

        let valid1 = key.sign(&data);
        let valid2 = key.sign(&data);
        let mut invalid = key.sign(&data);
        invalid.signature = "tampered".into();

        // All valid chain
        let chain = SignatureVerifier::verify_chain(&data, &[valid1.clone(), valid2.clone()], key.as_bytes());
        assert!(chain.all_valid);

        // Mixed chain: one valid + one invalid
        let chain = SignatureVerifier::verify_chain(&data, &[valid1, invalid], key.as_bytes());
        assert!(!chain.all_valid);
        assert!(chain.results[0].valid);
        assert!(!chain.results[1].valid);
    }

    #[test]
    fn test_large_memory_detection_clean_module() {
        // Valid WASM header with no memory section
        let wasm = b"\x00asm\x01\x00\x00\x00";
        assert!(!VulnerabilityScanner::has_large_memory(wasm));
    }

    #[test]
    fn test_large_memory_detection_small_module() {
        // Too small to parse
        assert!(!VulnerabilityScanner::has_large_memory(b"\x00asm"));
        assert!(!VulnerabilityScanner::has_large_memory(b""));
    }

    #[test]
    fn test_signature_expiry_field() {
        let key = make_key();
        let sig = key.sign(&sample_bytes());
        assert!(sig.expires_at.is_none());
        assert_eq!(sig.algorithm, SignatureAlgorithm::HmacSha256);
    }

    #[test]
    fn test_signing_key_as_bytes() {
        let key = SigningKey::new_hmac("id", b"secret");
        assert_eq!(key.as_bytes(), b"secret");
        assert_eq!(key.key_id, "id");
        assert_eq!(key.algorithm, SignatureAlgorithm::HmacSha256);
    }

    #[test]
    fn test_supply_chain_unverified_deps() {
        let mut store = ProvenanceStore::new();
        let rec = ProvenanceRecord {
            module_hash: "h2".into(),
            builder: "ci".into(),
            source_repo: "".into(),
            build_timestamp: Utc::now(),
            build_command: "make".into(),
            dependencies: vec![DependencyRef {
                name: "lib".into(),
                version: "1.0".into(),
                hash: "".into(), // empty hash = unverified
            }],
            reproducible: false,
        };
        store.record(rec).unwrap();
        let report = store.verify_supply_chain("h2");
        assert!(!report.all_deps_verified);
        assert!(!report.reproducible);
    }
}
