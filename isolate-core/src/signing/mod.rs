//! Module signing and verification.
//!
//! This module provides cryptographic signing and verification of WASM modules
//! to ensure only trusted code can be executed.
//!
//! # Features
//!
//! - **Ed25519 Signatures**: Uses Ed25519 for fast, secure signing
//! - **Key Management**: Generate, store, and manage signing keys
//! - **Signature Format**: Detached signatures with metadata
//! - **Policy Enforcement**: Configure trusted signers
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::signing::{SigningKey, ModuleSigner, SignaturePolicy};
//!
//! // Generate a signing key
//! let key = SigningKey::generate();
//!
//! // Sign a module
//! let signer = ModuleSigner::new(key);
//! let signature = signer.sign(&wasm_bytes)?;
//!
//! // Verify with policy
//! let policy = SignaturePolicy::require_signature()
//!     .add_trusted_key(key.public_key());
//! policy.verify(&wasm_bytes, &signature)?;
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.

mod keys;
mod policy;
mod signature;
mod signer;

pub use keys::{KeyId, SigningKey, VerifyingKey};
pub use policy::{PolicyVerificationError, SignaturePolicy};
pub use signature::{ModuleSignature, SignatureMetadata};
pub use signer::ModuleSigner;

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// Compute a SHA-256 hash of module bytes.
pub fn module_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// A trust store managing trusted signing keys and revocation.
///
/// Supports key rotation by allowing multiple active keys and
/// tracking revoked key IDs.
pub struct TrustStore {
    /// Active trusted verifying keys by key ID.
    trusted_keys: HashMap<KeyId, VerifyingKey>,
    /// Revoked key IDs (signatures from these keys are rejected).
    revoked_keys: HashSet<KeyId>,
}

impl TrustStore {
    /// Create an empty trust store.
    pub fn new() -> Self {
        Self { trusted_keys: HashMap::new(), revoked_keys: HashSet::new() }
    }

    /// Add a trusted verifying key.
    pub fn add_key(&mut self, key: VerifyingKey) {
        let id = key.key_id();
        self.trusted_keys.insert(id, key);
    }

    /// Revoke a key by ID. Future verifications will reject signatures
    /// from this key even if the key was previously trusted.
    pub fn revoke_key(&mut self, key_id: &KeyId) {
        self.trusted_keys.remove(key_id);
        self.revoked_keys.insert(key_id.clone());
    }

    /// Check if a key ID is revoked.
    pub fn is_revoked(&self, key_id: &KeyId) -> bool {
        self.revoked_keys.contains(key_id)
    }

    /// Get a trusted key by ID (returns None if revoked or not found).
    pub fn get_key(&self, key_id: &KeyId) -> Option<&VerifyingKey> {
        if self.revoked_keys.contains(key_id) {
            return None;
        }
        self.trusted_keys.get(key_id)
    }

    /// List all active (non-revoked) trusted key IDs.
    pub fn active_key_ids(&self) -> Vec<KeyId> {
        self.trusted_keys.keys().cloned().collect()
    }

    /// Number of active trusted keys.
    pub fn active_count(&self) -> usize {
        self.trusted_keys.len()
    }

    /// Number of revoked keys.
    pub fn revoked_count(&self) -> usize {
        self.revoked_keys.len()
    }
}

impl Default for TrustStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_hash() {
        let data = b"test wasm module";
        let hash1 = module_hash(data);
        let hash2 = module_hash(data);
        assert_eq!(hash1, hash2);

        let different = b"different data";
        let hash3 = module_hash(different);
        assert_ne!(hash1, hash3);
    }
}
