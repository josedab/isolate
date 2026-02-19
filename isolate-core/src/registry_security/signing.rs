//! Cryptographic signing for WASM modules.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::helpers::{hmac_sha256, sha256_hex};

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

    /// Sign the given module bytes, returning a [`ModuleSignature`].
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
