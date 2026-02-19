//! Signature verification for WASM modules.

use serde::{Deserialize, Serialize};

use super::signing::ModuleSignature;

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
