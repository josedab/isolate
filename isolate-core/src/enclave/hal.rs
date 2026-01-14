//! Hardware Abstraction Layer (HAL) for Trusted Execution Environment backends.
//!
//! Defines the `EnclaveBackend` trait that abstracts over different TEE hardware
//! (Intel SGX, AMD SEV-SNP, ARM TrustZone). The `SimulatedEnclaveBackend` is
//! the default fallback when no real TEE hardware is available.

use super::{AttestationReport, EnclaveConfig, EnclaveError, TeeType};
use std::collections::HashMap;
use std::fmt::Debug;

/// Trait abstracting TEE hardware operations.
///
/// Implementations provide the actual attestation, sealing, and enclave
/// lifecycle for a specific hardware platform.
pub trait EnclaveBackend: Send + Sync + Debug {
    /// Returns the backend name (e.g., "sgx", "sev-snp", "simulated").
    fn name(&self) -> &str;

    /// Returns the TEE type this backend supports.
    fn tee_type(&self) -> TeeType;

    /// Checks whether the backend's hardware is available.
    fn is_available(&self) -> bool;

    /// Initializes the TEE enclave.
    fn initialize(&mut self) -> Result<(), EnclaveError>;

    /// Returns whether the enclave is initialized.
    fn is_initialized(&self) -> bool;

    /// Generates a remote attestation report.
    fn attest(&mut self, user_data: &[u8]) -> Result<AttestationReport, EnclaveError>;

    /// Verifies an attestation report produced by this backend.
    fn verify_attestation(&self, report: &AttestationReport) -> Result<bool, EnclaveError>;

    /// Seals (encrypts) data for persistent storage.
    fn seal(&mut self, key: &str, data: &[u8]) -> Result<Vec<u8>, EnclaveError>;

    /// Unseals (decrypts) previously sealed data.
    fn unseal(&self, key: &str) -> Result<Vec<u8>, EnclaveError>;

    /// Lists all sealed data keys.
    fn sealed_keys(&self) -> Vec<String>;

    /// Destroys the enclave, clearing all secrets.
    fn destroy(&mut self);

    /// Returns the maximum enclave memory size in bytes.
    fn max_enclave_size(&self) -> usize;
}

// ---------------------------------------------------------------------------
// Simulated backend
// ---------------------------------------------------------------------------

/// Software-simulated TEE backend for development and testing.
///
/// Uses XOR-based "encryption" (obviously not secure) to mirror the sealing
/// API without requiring hardware.
#[derive(Debug)]
pub struct SimulatedEnclaveBackend {
    config: EnclaveConfig,
    initialized: bool,
    sealed_data: HashMap<String, Vec<u8>>,
    attestation_count: u64,
}

impl SimulatedEnclaveBackend {
    /// Creates a new simulated backend.
    pub fn new(config: EnclaveConfig) -> Self {
        Self { config, initialized: false, sealed_data: HashMap::new(), attestation_count: 0 }
    }

    fn generate_report_id(&mut self) -> String {
        self.attestation_count += 1;
        format!("sim-attest-{}", self.attestation_count)
    }
}

impl Default for SimulatedEnclaveBackend {
    fn default() -> Self {
        Self::new(EnclaveConfig::default())
    }
}

impl EnclaveBackend for SimulatedEnclaveBackend {
    fn name(&self) -> &str {
        "simulated"
    }

    fn tee_type(&self) -> TeeType {
        TeeType::Simulated
    }

    fn is_available(&self) -> bool {
        true
    }

    fn initialize(&mut self) -> Result<(), EnclaveError> {
        self.initialized = true;
        Ok(())
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn attest(&mut self, user_data: &[u8]) -> Result<AttestationReport, EnclaveError> {
        if !self.initialized {
            return Err(EnclaveError::NotInitialized);
        }

        // Simulated measurement: SHA-256-like hash of the config
        let measurement = {
            let mut m = vec![0u8; 32];
            for (i, byte) in self.config.tee_type.to_string().bytes().enumerate() {
                m[i % 32] ^= byte;
            }
            m
        };

        let report = AttestationReport {
            id: self.generate_report_id(),
            measurement,
            report_data: user_data.to_vec(),
            signature: vec![0xAA; 64],         // Simulated signature
            cert_chain: vec![vec![0xBB; 128]], // Simulated cert
            timestamp: std::time::SystemTime::now(),
        };

        Ok(report)
    }

    fn verify_attestation(&self, report: &AttestationReport) -> Result<bool, EnclaveError> {
        if !self.initialized {
            return Err(EnclaveError::NotInitialized);
        }
        // Simulated: accept any report with non-empty measurement
        Ok(!report.measurement.is_empty() && !report.signature.is_empty())
    }

    fn seal(&mut self, key: &str, data: &[u8]) -> Result<Vec<u8>, EnclaveError> {
        if !self.initialized {
            return Err(EnclaveError::NotInitialized);
        }
        // XOR "encryption" for simulation
        let sealed: Vec<u8> = data.iter().map(|b| b ^ 0xFF).collect();
        self.sealed_data.insert(key.to_string(), sealed.clone());
        Ok(sealed)
    }

    fn unseal(&self, key: &str) -> Result<Vec<u8>, EnclaveError> {
        if !self.initialized {
            return Err(EnclaveError::NotInitialized);
        }
        self.sealed_data
            .get(key)
            .map(|data| data.iter().map(|b| b ^ 0xFF).collect())
            .ok_or_else(|| EnclaveError::KeyNotFound(key.to_string()))
    }

    fn sealed_keys(&self) -> Vec<String> {
        self.sealed_data.keys().cloned().collect()
    }

    fn destroy(&mut self) {
        self.sealed_data.clear();
        self.initialized = false;
        self.attestation_count = 0;
    }

    fn max_enclave_size(&self) -> usize {
        self.config.size
    }
}

// Helper: Display for TeeType (needed above)
impl std::fmt::Display for TeeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeeType::IntelSgx => write!(f, "IntelSGX"),
            TeeType::AmdSev => write!(f, "AmdSEV"),
            TeeType::ArmTrustZone => write!(f, "ArmTrustZone"),
            TeeType::Simulated => write!(f, "Simulated"),
        }
    }
}

// ---------------------------------------------------------------------------
// Backend detection
// ---------------------------------------------------------------------------

/// Detects the best available TEE backend on the current system.
///
/// Priority: Intel SGX > AMD SEV-SNP > ARM TrustZone > Simulated.
/// Currently only returns `Simulated` since real backends require
/// feature-gated hardware crates.
pub fn detect_enclave_backend(config: EnclaveConfig) -> Box<dyn EnclaveBackend> {
    // Future: probe /dev/sgx_enclave, /dev/sev, etc.
    Box::new(SimulatedEnclaveBackend::new(config))
}

/// Lists all TEE backends known to this build.
pub fn available_enclave_backends() -> Vec<&'static str> {
    let backends = vec!["simulated"];
    // #[cfg(feature = "tee-sgx")]   backends.push("sgx");
    // #[cfg(feature = "tee-sev")]   backends.push("sev-snp");
    backends
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> SimulatedEnclaveBackend {
        SimulatedEnclaveBackend::default()
    }

    #[test]
    fn test_backend_name() {
        assert_eq!(backend().name(), "simulated");
    }

    #[test]
    fn test_tee_type() {
        assert_eq!(backend().tee_type(), TeeType::Simulated);
    }

    #[test]
    fn test_always_available() {
        assert!(backend().is_available());
    }

    #[test]
    fn test_initialize() {
        let mut b = backend();
        assert!(!b.is_initialized());
        b.initialize().unwrap();
        assert!(b.is_initialized());
    }

    #[test]
    fn test_attest_requires_init() {
        let mut b = backend();
        assert!(matches!(b.attest(b"data"), Err(EnclaveError::NotInitialized)));
    }

    #[test]
    fn test_attest_after_init() {
        let mut b = backend();
        b.initialize().unwrap();
        let report = b.attest(b"user-data").unwrap();
        assert!(!report.id.is_empty());
        assert_eq!(report.report_data, b"user-data");
        assert_eq!(report.measurement.len(), 32);
    }

    #[test]
    fn test_verify_attestation() {
        let mut b = backend();
        b.initialize().unwrap();
        let report = b.attest(b"data").unwrap();
        assert!(b.verify_attestation(&report).unwrap());
    }

    #[test]
    fn test_seal_unseal_roundtrip() {
        let mut b = backend();
        b.initialize().unwrap();

        let original = b"secret payload";
        let sealed = b.seal("key1", original).unwrap();
        assert_ne!(sealed, original.to_vec(), "sealed data should differ");

        let unsealed = b.unseal("key1").unwrap();
        assert_eq!(unsealed, original.to_vec());
    }

    #[test]
    fn test_seal_requires_init() {
        let mut b = backend();
        assert!(matches!(b.seal("k", b"v"), Err(EnclaveError::NotInitialized)));
    }

    #[test]
    fn test_unseal_unknown_key() {
        let mut b = backend();
        b.initialize().unwrap();
        assert!(matches!(b.unseal("nope"), Err(EnclaveError::KeyNotFound(_))));
    }

    #[test]
    fn test_sealed_keys() {
        let mut b = backend();
        b.initialize().unwrap();
        b.seal("a", b"1").unwrap();
        b.seal("b", b"2").unwrap();

        let mut keys = b.sealed_keys();
        keys.sort();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn test_destroy_clears_state() {
        let mut b = backend();
        b.initialize().unwrap();
        b.seal("key", b"data").unwrap();
        assert!(b.is_initialized());

        b.destroy();
        assert!(!b.is_initialized());
        assert!(b.sealed_keys().is_empty());
    }

    #[test]
    fn test_max_enclave_size() {
        let b = backend();
        assert_eq!(b.max_enclave_size(), 64 * 1024 * 1024);
    }

    #[test]
    fn test_detect_returns_simulated() {
        let b = detect_enclave_backend(EnclaveConfig::default());
        assert_eq!(b.name(), "simulated");
    }

    #[test]
    fn test_available_backends() {
        let backends = available_enclave_backends();
        assert!(backends.contains(&"simulated"));
    }

    #[test]
    fn test_multiple_attestations() {
        let mut b = backend();
        b.initialize().unwrap();

        let r1 = b.attest(b"a").unwrap();
        let r2 = b.attest(b"b").unwrap();
        assert_ne!(r1.id, r2.id, "each attestation gets a unique ID");
    }

    #[test]
    fn test_tee_type_display() {
        assert_eq!(TeeType::IntelSgx.to_string(), "IntelSGX");
        assert_eq!(TeeType::AmdSev.to_string(), "AmdSEV");
        assert_eq!(TeeType::ArmTrustZone.to_string(), "ArmTrustZone");
        assert_eq!(TeeType::Simulated.to_string(), "Simulated");
    }
}
