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

/// Compute a SHA-256 hash of module bytes.
pub fn module_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
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
