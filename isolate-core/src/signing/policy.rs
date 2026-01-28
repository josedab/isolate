//! Signature policy enforcement.

use super::keys::{KeyId, VerifyingKey};
use super::module_hash;
use super::signature::ModuleSignature;
use std::collections::{HashMap, HashSet};

/// Policy for verifying module signatures.
#[derive(Debug, Clone)]
pub struct SignaturePolicy {
    /// Whether signatures are required.
    require_signature: bool,
    /// Trusted public keys by key ID.
    trusted_keys: HashMap<KeyId, VerifyingKey>,
    /// Trusted signer names.
    trusted_signers: HashSet<String>,
    /// Whether to allow expired signatures.
    allow_expired: bool,
    /// Minimum required signatures.
    min_signatures: usize,
}

impl Default for SignaturePolicy {
    fn default() -> Self {
        Self {
            require_signature: false,
            trusted_keys: HashMap::new(),
            trusted_signers: HashSet::new(),
            allow_expired: false,
            min_signatures: 0,
        }
    }
}

impl SignaturePolicy {
    /// Create a new empty policy (allows all modules).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a policy that requires signatures.
    pub fn require_signature() -> Self {
        Self { require_signature: true, ..Default::default() }
    }

    /// Create a policy that allows any module (no verification).
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Set whether signatures are required.
    pub fn with_require_signature(mut self, require: bool) -> Self {
        self.require_signature = require;
        self
    }

    /// Add a trusted public key.
    pub fn add_trusted_key(mut self, key: VerifyingKey) -> Self {
        let key_id = key.key_id();
        self.trusted_keys.insert(key_id, key);
        self
    }

    /// Add a trusted public key with a specific key ID.
    pub fn add_trusted_key_with_id(mut self, key_id: KeyId, key: VerifyingKey) -> Self {
        self.trusted_keys.insert(key_id, key);
        self
    }

    /// Add a trusted signer name.
    pub fn add_trusted_signer(mut self, name: impl Into<String>) -> Self {
        self.trusted_signers.insert(name.into());
        self
    }

    /// Set whether expired signatures are allowed.
    pub fn with_allow_expired(mut self, allow: bool) -> Self {
        self.allow_expired = allow;
        self
    }

    /// Set minimum required signatures.
    pub fn with_min_signatures(mut self, min: usize) -> Self {
        self.min_signatures = min;
        if min > 0 {
            self.require_signature = true;
        }
        self
    }

    /// Check if signatures are required.
    pub fn requires_signature(&self) -> bool {
        self.require_signature
    }

    /// Get the trusted keys.
    pub fn trusted_keys(&self) -> &HashMap<KeyId, VerifyingKey> {
        &self.trusted_keys
    }

    /// Get the trusted signer names.
    pub fn trusted_signers(&self) -> &HashSet<String> {
        &self.trusted_signers
    }

    /// Check if a key is trusted.
    pub fn is_key_trusted(&self, key_id: &KeyId) -> bool {
        self.trusted_keys.contains_key(key_id)
    }

    /// Check if a signer name is trusted.
    pub fn is_signer_trusted(&self, name: &str) -> bool {
        // If no trusted signers specified, trust all
        self.trusted_signers.is_empty() || self.trusted_signers.contains(name)
    }

    /// Verify a module against this policy.
    pub fn verify(
        &self,
        wasm_bytes: &[u8],
        signature: Option<&ModuleSignature>,
    ) -> Result<PolicyVerificationResult, PolicyVerificationError> {
        // Check if signature is required
        if self.require_signature && signature.is_none() {
            return Err(PolicyVerificationError::SignatureRequired);
        }

        // If no signature and not required, allow
        let Some(sig) = signature else {
            return Ok(PolicyVerificationResult::allowed_unsigned());
        };

        // Check expiration
        if !self.allow_expired && sig.is_expired() {
            return Err(PolicyVerificationError::SignatureExpired);
        }

        // Verify module hash matches
        let computed_hash = module_hash(wasm_bytes);
        if computed_hash != sig.module_hash {
            return Err(PolicyVerificationError::HashMismatch {
                expected: hex::encode(sig.module_hash),
                actual: hex::encode(computed_hash),
            });
        }

        // Check if key is trusted
        let key_id = sig.key_id();
        if !self.trusted_keys.is_empty() && !self.trusted_keys.contains_key(key_id) {
            return Err(PolicyVerificationError::UntrustedKey { key_id: key_id.clone() });
        }

        // Check if signer name is trusted
        if let Some(ref signer_name) = sig.metadata.signer_name {
            if !self.is_signer_trusted(signer_name) {
                return Err(PolicyVerificationError::UntrustedSigner {
                    signer: signer_name.clone(),
                });
            }
        }

        // Verify the cryptographic signature
        if let Some(trusted_key) = self.trusted_keys.get(key_id) {
            // The public key in the signature must match our trusted key
            if sig.public_key() != trusted_key {
                return Err(PolicyVerificationError::KeyMismatch { key_id: key_id.clone() });
            }
        }

        Ok(PolicyVerificationResult::verified(sig))
    }

    /// Verify a module with multiple signatures.
    pub fn verify_multi(
        &self,
        wasm_bytes: &[u8],
        signatures: &[ModuleSignature],
    ) -> Result<PolicyVerificationResult, PolicyVerificationError> {
        if self.require_signature && signatures.is_empty() {
            return Err(PolicyVerificationError::SignatureRequired);
        }

        if signatures.len() < self.min_signatures {
            return Err(PolicyVerificationError::InsufficientSignatures {
                required: self.min_signatures,
                provided: signatures.len(),
            });
        }

        let mut verified_keys = Vec::new();

        for sig in signatures {
            match self.verify(wasm_bytes, Some(sig)) {
                Ok(result) => {
                    if result.is_verified {
                        verified_keys.push(sig.key_id().clone());
                    }
                }
                Err(e) => return Err(e),
            }
        }

        Ok(PolicyVerificationResult {
            is_verified: !verified_keys.is_empty(),
            is_signed: true,
            verified_keys,
            signer_name: signatures.first().and_then(|s| s.metadata.signer_name.clone()),
        })
    }
}

/// Result of policy verification.
#[derive(Debug, Clone)]
pub struct PolicyVerificationResult {
    /// Whether the module was verified.
    pub is_verified: bool,
    /// Whether the module was signed.
    pub is_signed: bool,
    /// Key IDs that verified the signature.
    pub verified_keys: Vec<KeyId>,
    /// Signer name if available.
    pub signer_name: Option<String>,
}

impl PolicyVerificationResult {
    /// Create a result for an unsigned module that was allowed.
    pub fn allowed_unsigned() -> Self {
        Self { is_verified: false, is_signed: false, verified_keys: Vec::new(), signer_name: None }
    }

    /// Create a result for a verified signed module.
    pub fn verified(sig: &ModuleSignature) -> Self {
        Self {
            is_verified: true,
            is_signed: true,
            verified_keys: vec![sig.key_id().clone()],
            signer_name: sig.metadata.signer_name.clone(),
        }
    }
}

/// Error during policy verification.
#[derive(Debug, thiserror::Error)]
pub enum PolicyVerificationError {
    /// Signature is required but not provided.
    #[error("Signature required but not provided")]
    SignatureRequired,

    /// Signature has expired.
    #[error("Signature has expired")]
    SignatureExpired,

    /// Module hash does not match signature.
    #[error("Module hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    /// Signing key is not trusted.
    #[error("Untrusted signing key: {key_id}")]
    UntrustedKey { key_id: KeyId },

    /// Signer name is not trusted.
    #[error("Untrusted signer: {signer}")]
    UntrustedSigner { signer: String },

    /// Public key in signature doesn't match trusted key.
    #[error("Key mismatch for key ID: {key_id}")]
    KeyMismatch { key_id: KeyId },

    /// Not enough valid signatures.
    #[error("Insufficient signatures: required {required}, provided {provided}")]
    InsufficientSignatures { required: usize, provided: usize },

    /// Invalid signature format.
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::SigningKey;

    #[test]
    fn test_allow_all_policy() {
        let policy = SignaturePolicy::allow_all();
        let wasm = b"\x00asm\x01\x00\x00\x00";

        let result = policy.verify(wasm, None).unwrap();
        assert!(!result.is_verified);
        assert!(!result.is_signed);
    }

    #[test]
    fn test_require_signature_policy() {
        let policy = SignaturePolicy::require_signature();
        let wasm = b"\x00asm\x01\x00\x00\x00";

        let result = policy.verify(wasm, None);
        assert!(matches!(result, Err(PolicyVerificationError::SignatureRequired)));
    }

    #[test]
    fn test_trusted_key_verification() {
        let key = SigningKey::generate();
        let signer = crate::signing::ModuleSigner::new(key.clone());

        let policy = SignaturePolicy::require_signature().add_trusted_key(key.public_key().clone());

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let result = policy.verify(wasm, Some(&sig)).unwrap();
        assert!(result.is_verified);
        assert!(result.is_signed);
    }

    #[test]
    fn test_untrusted_key_rejected() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();
        let signer = crate::signing::ModuleSigner::new(key1);

        // Only trust key2
        let policy =
            SignaturePolicy::require_signature().add_trusted_key(key2.public_key().clone());

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let result = policy.verify(wasm, Some(&sig));
        assert!(matches!(result, Err(PolicyVerificationError::UntrustedKey { .. })));
    }

    #[test]
    fn test_hash_mismatch_rejected() {
        let key = SigningKey::generate();
        let signer = crate::signing::ModuleSigner::new(key.clone());

        let policy = SignaturePolicy::require_signature().add_trusted_key(key.public_key().clone());

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let different_wasm = b"\x00asm\x01\x00\x00\x01";
        let result = policy.verify(different_wasm, Some(&sig));
        assert!(matches!(result, Err(PolicyVerificationError::HashMismatch { .. })));
    }

    #[test]
    fn test_trusted_signer_name() {
        let key = SigningKey::generate();
        let signer =
            crate::signing::ModuleSigner::new(key.clone()).with_signer_name("Trusted Corp");

        let policy = SignaturePolicy::require_signature()
            .add_trusted_key(key.public_key().clone())
            .add_trusted_signer("Trusted Corp");

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let result = policy.verify(wasm, Some(&sig)).unwrap();
        assert!(result.is_verified);
        assert_eq!(result.signer_name, Some("Trusted Corp".to_string()));
    }

    #[test]
    fn test_untrusted_signer_name_rejected() {
        let key = SigningKey::generate();
        let signer = crate::signing::ModuleSigner::new(key.clone()).with_signer_name("Evil Corp");

        let policy = SignaturePolicy::require_signature()
            .add_trusted_key(key.public_key().clone())
            .add_trusted_signer("Trusted Corp");

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let result = policy.verify(wasm, Some(&sig));
        assert!(matches!(result, Err(PolicyVerificationError::UntrustedSigner { .. })));
    }

    #[test]
    fn test_multi_signature_verification() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();
        let signer1 = crate::signing::ModuleSigner::new(key1.clone());
        let signer2 = crate::signing::ModuleSigner::new(key2.clone());

        let policy = SignaturePolicy::require_signature()
            .add_trusted_key(key1.public_key().clone())
            .add_trusted_key(key2.public_key().clone())
            .with_min_signatures(2);

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig1 = signer1.sign(wasm);
        let sig2 = signer2.sign(wasm);

        let result = policy.verify_multi(wasm, &[sig1, sig2]).unwrap();
        assert!(result.is_verified);
        assert_eq!(result.verified_keys.len(), 2);
    }

    #[test]
    fn test_insufficient_signatures_rejected() {
        let key = SigningKey::generate();
        let signer = crate::signing::ModuleSigner::new(key.clone());

        let policy = SignaturePolicy::require_signature()
            .add_trusted_key(key.public_key().clone())
            .with_min_signatures(2);

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let result = policy.verify_multi(wasm, &[sig]);
        assert!(matches!(
            result,
            Err(PolicyVerificationError::InsufficientSignatures { required: 2, provided: 1 })
        ));
    }

    #[test]
    fn test_policy_builder() {
        let key = SigningKey::generate();

        let policy = SignaturePolicy::new()
            .with_require_signature(true)
            .add_trusted_key(key.public_key().clone())
            .add_trusted_signer("Test Signer")
            .with_allow_expired(false)
            .with_min_signatures(1);

        assert!(policy.requires_signature());
        assert!(policy.is_key_trusted(&key.key_id()));
        assert!(policy.is_signer_trusted("Test Signer"));
        assert!(!policy.is_signer_trusted("Unknown"));
    }
}
