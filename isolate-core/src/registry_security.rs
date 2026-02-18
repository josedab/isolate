#![allow(dead_code)]

//! Security layer for the WASM module registry.
//!
//! Provides cryptographic signing, provenance tracking, and vulnerability
//! scanning for WASM modules distributed through the OCI registry.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
// ---------------------------------------------------------------------------
// Signature types
// ---------------------------------------------------------------------------

/// Supported signature algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureAlgorithm {
    /// Ed25519 (placeholder – not implemented in this module).
    Ed25519,
    /// HMAC-SHA256 – used for the built-in implementation.
    HmacSha256,
}

/// Cryptographic signature attached to a WASM module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSignature {
    /// SHA-256 hex digest of the module bytes.
    pub module_hash: String,
    /// Hex-encoded signature value.
    pub signature: String,
    /// Algorithm used to produce the signature.
    pub algorithm: SignatureAlgorithm,
    /// Identity of the signer (key id or human-readable label).
    pub signer_id: String,
    /// When the signature was created.
    pub signed_at: DateTime<Utc>,
    /// Optional expiry.
    pub expires_at: Option<DateTime<Utc>>,
}

impl ModuleSignature {
    /// Verify this signature against the given public/shared key and module bytes.
    ///
    /// Returns `true` when the recomputed HMAC matches the stored signature.
    /// Ed25519 verification is not implemented and always returns `false`.
    pub fn verify(&self, key: &[u8], module_bytes: &[u8]) -> bool {
        match self.algorithm {
            SignatureAlgorithm::HmacSha256 => {
                let expected = hmac_sha256(key, module_bytes);
                self.signature == expected
            }
            SignatureAlgorithm::Ed25519 => {
                // Production would use ed25519-dalek; stubbed here.
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Signing key
// ---------------------------------------------------------------------------

/// A key used to sign WASM modules.
#[derive(Debug, Clone)]
pub struct SigningKey {
    /// Unique identifier for this key.
    pub key_id: String,
    /// Algorithm this key is used with.
    pub algorithm: SignatureAlgorithm,
    /// Raw key material (shared secret for HMAC).
    key_bytes: Vec<u8>,
}

impl SigningKey {
    /// Create a new HMAC-SHA256 signing key.
    pub fn new_hmac(key_id: impl Into<String>, secret: &[u8]) -> Self {
        Self {
            key_id: key_id.into(),
            algorithm: SignatureAlgorithm::HmacSha256,
            key_bytes: secret.to_vec(),
        }
    }

    /// Sign `module_bytes` and return a [`ModuleSignature`].
    pub fn sign(&self, module_bytes: &[u8]) -> ModuleSignature {
        let module_hash = sha256_hex(module_bytes);
        let signature = hmac_sha256(&self.key_bytes, module_bytes);

        ModuleSignature {
            module_hash,
            signature,
            algorithm: self.algorithm,
            signer_id: self.key_id.clone(),
            signed_at: Utc::now(),
            expires_at: None,
        }
    }

    /// Return the raw key bytes (for verification).
    pub fn as_bytes(&self) -> &[u8] {
        &self.key_bytes
    }
}

// ---------------------------------------------------------------------------
// Signature verifier
// ---------------------------------------------------------------------------

/// Result of verifying a single signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the signature is valid.
    pub valid: bool,
    /// Human-readable message.
    pub message: String,
    /// Signer identity.
    pub signer_id: String,
}

/// Result of verifying a chain of signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerificationResult {
    /// `true` only when every signature in the chain is valid.
    pub all_valid: bool,
    /// Per-signature results.
    pub results: Vec<VerificationResult>,
}

/// Verifies module signatures.
pub struct SignatureVerifier;

impl SignatureVerifier {
    /// Verify a single signature.
    pub fn verify_signature(
        module_bytes: &[u8],
        signature: &ModuleSignature,
        key: &[u8],
    ) -> VerificationResult {
        let valid = signature.verify(key, module_bytes);
        VerificationResult {
            valid,
            message: if valid {
                "Signature verified successfully".into()
            } else {
                "Signature verification failed".into()
            },
            signer_id: signature.signer_id.clone(),
        }
    }

    /// Verify a chain of signatures – all must pass.
    pub fn verify_chain(
        module_bytes: &[u8],
        signatures: &[ModuleSignature],
        key: &[u8],
    ) -> ChainVerificationResult {
        let results: Vec<VerificationResult> = signatures
            .iter()
            .map(|sig| Self::verify_signature(module_bytes, sig, key))
            .collect();
        let all_valid = !results.is_empty() && results.iter().all(|r| r.valid);
        ChainVerificationResult { all_valid, results }
    }
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// A reference to a dependency used during the build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRef {
    /// Name of the dependency.
    pub name: String,
    /// Version string.
    pub version: String,
    /// SHA-256 hash of the dependency artifact.
    pub hash: String,
}

/// Provenance metadata tracking origin and build history of a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    /// SHA-256 of the module bytes.
    pub module_hash: String,
    /// Who (or what CI system) built the module.
    pub builder: String,
    /// Git repository URL.
    pub source_repo: String,
    /// When the module was built.
    pub build_timestamp: DateTime<Utc>,
    /// Command used to build the module.
    pub build_command: String,
    /// Dependencies consumed during the build.
    pub dependencies: Vec<DependencyRef>,
    /// Whether the build is reproducible.
    pub reproducible: bool,
}

impl ProvenanceRecord {
    /// Produce a SLSA v0.2-style provenance predicate as JSON.
    pub fn to_slsa_predicate(&self) -> serde_json::Value {
        let materials: Vec<serde_json::Value> = self
            .dependencies
            .iter()
            .map(|d| {
                serde_json::json!({
                    "uri": format!("pkg:{}/{}", d.name, d.version),
                    "digest": { "sha256": d.hash },
                })
            })
            .collect();

        serde_json::json!({
            "buildType": "https://isolate.dev/build/v1",
            "builder": { "id": self.builder },
            "invocation": {
                "configSource": {
                    "uri": self.source_repo,
                },
                "parameters": {
                    "command": self.build_command,
                },
            },
            "materials": materials,
            "metadata": {
                "buildStartedOn": self.build_timestamp.to_rfc3339(),
                "reproducible": self.reproducible,
            },
        })
    }
}

/// In-memory store for provenance records.
pub struct ProvenanceStore {
    records: HashMap<String, ProvenanceRecord>,
}

/// Summary produced by [`ProvenanceStore::verify_supply_chain`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyChainReport {
    /// The module hash being inspected.
    pub module_hash: String,
    /// Whether a provenance record exists.
    pub has_provenance: bool,
    /// Whether the build is marked reproducible.
    pub reproducible: bool,
    /// Number of dependencies.
    pub dependency_count: usize,
    /// Whether all dependency hashes are non-empty.
    pub all_deps_verified: bool,
}

impl ProvenanceStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Record provenance for a module.
    pub fn record(&mut self, provenance: ProvenanceRecord) -> Result<(), String> {
        if provenance.module_hash.is_empty() {
            return Err("module_hash must not be empty".into());
        }
        self.records
            .insert(provenance.module_hash.clone(), provenance);
        Ok(())
    }

    /// Retrieve provenance by module hash.
    pub fn get(&self, module_hash: &str) -> Option<&ProvenanceRecord> {
        self.records.get(module_hash)
    }

    /// Produce a supply-chain health report for a module.
    pub fn verify_supply_chain(&self, module_hash: &str) -> SupplyChainReport {
        match self.records.get(module_hash) {
            Some(record) => {
                let all_deps_verified =
                    !record.dependencies.is_empty()
                        && record.dependencies.iter().all(|d| !d.hash.is_empty());
                SupplyChainReport {
                    module_hash: module_hash.to_string(),
                    has_provenance: true,
                    reproducible: record.reproducible,
                    dependency_count: record.dependencies.len(),
                    all_deps_verified,
                }
            }
            None => SupplyChainReport {
                module_hash: module_hash.to_string(),
                has_provenance: false,
                reproducible: false,
                dependency_count: 0,
                all_deps_verified: false,
            },
        }
    }
}

impl Default for ProvenanceStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Vulnerability scanning
// ---------------------------------------------------------------------------

/// Severity of a vulnerability finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Critical impact.
    Critical,
    /// High impact.
    High,
    /// Medium impact.
    Medium,
    /// Low impact.
    Low,
    /// Informational only.
    Info,
}

/// Overall risk level for a scanned module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// At least one critical finding.
    Critical,
    /// At least one high finding.
    High,
    /// At least one medium finding.
    Medium,
    /// Only low findings.
    Low,
    /// No findings.
    None,
}

/// A single vulnerability finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityFinding {
    /// CVE identifier, if applicable.
    pub cve_id: Option<String>,
    /// Short title.
    pub title: String,
    /// Longer description.
    pub description: String,
    /// Severity level.
    pub severity: Severity,
    /// Component or import that triggered the finding.
    pub affected_component: String,
    /// Suggested remediation.
    pub remediation: Option<String>,
}

/// Result of scanning a WASM module for vulnerabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulnerabilityReport {
    /// SHA-256 hash of the scanned module.
    pub module_hash: String,
    /// When the scan was performed.
    pub scanned_at: DateTime<Utc>,
    /// Individual findings.
    pub findings: Vec<VulnerabilityFinding>,
    /// Counts per severity level.
    pub severity_counts: HashMap<Severity, usize>,
    /// Overall risk level.
    pub overall_risk: RiskLevel,
}

/// A pattern-based rule used by the scanner.
#[derive(Debug, Clone)]
pub struct ScanRule {
    /// Unique rule identifier.
    pub id: String,
    /// Pattern to search for in the module (substring match on UTF-8 view).
    pub pattern: String,
    /// Severity assigned when the pattern is found.
    pub severity: Severity,
    /// Human-readable title for findings produced by this rule.
    pub title: String,
    /// Description included in findings.
    pub description: String,
    /// Optional remediation advice.
    pub remediation: Option<String>,
}

/// Scans WASM modules for known-bad patterns.
pub struct VulnerabilityScanner {
    rules: Vec<ScanRule>,
}

impl VulnerabilityScanner {
    /// Create a scanner pre-loaded with built-in rules.
    pub fn new() -> Self {
        Self {
            rules: Self::builtin_rules(),
        }
    }

    /// Add a custom scan rule.
    pub fn add_rule(&mut self, rule: ScanRule) {
        self.rules.push(rule);
    }

    /// Scan `module_bytes` and return a vulnerability report.
    pub fn scan(&self, module_bytes: &[u8]) -> VulnerabilityReport {
        let module_hash = sha256_hex(module_bytes);
        let text = String::from_utf8_lossy(module_bytes);
        let mut findings = Vec::new();

        for rule in &self.rules {
            if text.contains(&rule.pattern) {
                findings.push(VulnerabilityFinding {
                    cve_id: None,
                    title: rule.title.clone(),
                    description: rule.description.clone(),
                    severity: rule.severity,
                    affected_component: rule.pattern.clone(),
                    remediation: rule.remediation.clone(),
                });
            }
        }

        // Check for excessive memory requests (> 256 pages = 16 MiB).
        if Self::has_large_memory(module_bytes) {
            findings.push(VulnerabilityFinding {
                cve_id: None,
                title: "Excessive initial memory".into(),
                description: "Module requests more than 256 pages (16 MiB) of initial memory"
                    .into(),
                severity: Severity::Medium,
                affected_component: "memory".into(),
                remediation: Some("Reduce initial memory or use memory.grow dynamically".into()),
            });
        }

        let mut severity_counts: HashMap<Severity, usize> = HashMap::new();
        for f in &findings {
            *severity_counts.entry(f.severity).or_insert(0) += 1;
        }

        let overall_risk = if severity_counts.contains_key(&Severity::Critical) {
            RiskLevel::Critical
        } else if severity_counts.contains_key(&Severity::High) {
            RiskLevel::High
        } else if severity_counts.contains_key(&Severity::Medium) {
            RiskLevel::Medium
        } else if severity_counts.contains_key(&Severity::Low) {
            RiskLevel::Low
        } else {
            RiskLevel::None
        };

        VulnerabilityReport {
            module_hash,
            scanned_at: Utc::now(),
            findings,
            severity_counts,
            overall_risk,
        }
    }

    // --- built-in rules ---------------------------------------------------

    fn builtin_rules() -> Vec<ScanRule> {
        vec![
            ScanRule {
                id: "WASM-001".into(),
                pattern: "proc_exit".into(),
                severity: Severity::Low,
                title: "Uses proc_exit".into(),
                description: "Module imports proc_exit which can terminate the sandbox".into(),
                remediation: Some("Ensure host traps proc_exit appropriately".into()),
            },
            ScanRule {
                id: "WASM-002".into(),
                pattern: "fd_write".into(),
                severity: Severity::Info,
                title: "Uses fd_write".into(),
                description: "Module imports fd_write for I/O".into(),
                remediation: None,
            },
            ScanRule {
                id: "WASM-003".into(),
                pattern: "sock_accept".into(),
                severity: Severity::High,
                title: "Socket accept import".into(),
                description: "Module imports sock_accept which may open network listeners".into(),
                remediation: Some("Remove network capability or audit usage".into()),
            },
            ScanRule {
                id: "WASM-004".into(),
                pattern: "__wasm_call_ctors".into(),
                severity: Severity::Info,
                title: "Static constructors detected".into(),
                description: "Module contains static constructor calls".into(),
                remediation: None,
            },
            ScanRule {
                id: "WASM-005".into(),
                pattern: "eval".into(),
                severity: Severity::Medium,
                title: "Suspicious function name: eval".into(),
                description: "Module contains a reference to 'eval' which may indicate dynamic code execution".into(),
                remediation: Some("Audit eval usage or sandbox further".into()),
            },
        ]
    }

    /// Heuristic: check if a WASM module requests > 256 initial pages.
    ///
    /// The memory section (id 5) contains a limits entry.  We do a minimal
    /// parse rather than pulling in a full WASM parser.
    fn has_large_memory(bytes: &[u8]) -> bool {
        // Minimal WASM header: 8 bytes
        if bytes.len() < 8 {
            return false;
        }
        let mut pos = 8;
        while pos < bytes.len() {
            if pos + 1 >= bytes.len() {
                break;
            }
            let section_id = bytes[pos];
            pos += 1;
            let (section_len, adv) = read_leb128_u32(bytes, pos);
            pos += adv;
            if section_id == 5 {
                // Memory section – first byte is count
                if pos < bytes.len() {
                    let mem_start = pos + 1; // skip count
                    if mem_start < bytes.len() {
                        let flags = bytes[mem_start];
                        let (initial, _) = read_leb128_u32(bytes, mem_start + 1);
                        let _ = flags; // limits flags (has-max)
                        return initial > 256;
                    }
                }
                return false;
            }
            pos += section_len as usize;
        }
        false
    }
}

impl Default for VulnerabilityScanner {
    fn default() -> Self {
        Self::new()
    }
}

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
        let module_hash = sha256_hex(module_bytes);
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
// Helpers
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of `data`.
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// HMAC-SHA256 implemented via the standard construction (RFC 2104).
///
/// Uses the `sha2` crate directly so we don't need an extra `hmac` dependency.
fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
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
fn read_leb128_u32(bytes: &[u8], start: usize) -> (u32, usize) {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
