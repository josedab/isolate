//! Module signing implementation.

use super::keys::SigningKey;
use super::module_hash;
use super::signature::{ModuleSignature, SignatureMetadata};

/// Signs WASM modules with a cryptographic key.
pub struct ModuleSigner {
    key: SigningKey,
    signer_name: Option<String>,
    signer_url: Option<String>,
}

impl ModuleSigner {
    /// Create a new module signer.
    pub fn new(key: SigningKey) -> Self {
        Self {
            key,
            signer_name: None,
            signer_url: None,
        }
    }

    /// Set the signer name.
    pub fn with_signer_name(mut self, name: impl Into<String>) -> Self {
        self.signer_name = Some(name.into());
        self
    }

    /// Set the signer URL.
    pub fn with_signer_url(mut self, url: impl Into<String>) -> Self {
        self.signer_url = Some(url.into());
        self
    }

    /// Get the signing key.
    pub fn key(&self) -> &SigningKey {
        &self.key
    }

    /// Sign a WASM module.
    pub fn sign(&self, wasm_bytes: &[u8]) -> ModuleSignature {
        // Compute module hash
        let hash = module_hash(wasm_bytes);

        // Create signature over the hash
        let signature = self.key.sign(&hash);

        // Build metadata
        let mut metadata = SignatureMetadata::new(self.key.key_id(), self.key.public_key().clone());

        if let Some(ref name) = self.signer_name {
            metadata = metadata.with_signer_name(name);
        }

        if let Some(ref url) = self.signer_url {
            metadata = metadata.with_signer_url(url);
        }

        ModuleSignature::new(metadata, hash, signature)
    }

    /// Sign a module with custom metadata.
    pub fn sign_with_metadata(
        &self,
        wasm_bytes: &[u8],
        mut metadata: SignatureMetadata,
    ) -> ModuleSignature {
        let hash = module_hash(wasm_bytes);
        let signature = self.key.sign(&hash);

        // Ensure key info is correct
        metadata.key_id = self.key.key_id();
        metadata.public_key = self.key.public_key().clone();

        ModuleSignature::new(metadata, hash, signature)
    }

    /// Verify a signature was created by this signer.
    pub fn verify(&self, wasm_bytes: &[u8], signature: &ModuleSignature) -> bool {
        // Check key matches
        if signature.public_key() != self.key.public_key() {
            return false;
        }

        // Verify module hash
        let hash = module_hash(wasm_bytes);
        if hash != signature.module_hash {
            return false;
        }

        // Verify signature
        let expected = self.key.sign(&hash);
        expected == signature.signature
    }
}

impl std::fmt::Debug for ModuleSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleSigner")
            .field("key_id", &self.key.key_id())
            .field("signer_name", &self.signer_name)
            .field("signer_url", &self.signer_url)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signer_creation() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key);

        assert!(signer.signer_name.is_none());
    }

    #[test]
    fn test_signer_with_options() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key)
            .with_signer_name("Test Org")
            .with_signer_url("https://test.org");

        assert_eq!(signer.signer_name, Some("Test Org".to_string()));
        assert_eq!(signer.signer_url, Some("https://test.org".to_string()));
    }

    #[test]
    fn test_sign_module() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key).with_signer_name("Test");

        let wasm = b"\x00asm\x01\x00\x00\x00"; // Minimal WASM header
        let sig = signer.sign(wasm);

        assert_eq!(sig.metadata.signer_name, Some("Test".to_string()));
        assert_eq!(sig.module_hash, module_hash(wasm));
    }

    #[test]
    fn test_verify_signature() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key);

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        assert!(signer.verify(wasm, &sig));
    }

    #[test]
    fn test_verify_fails_wrong_module() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key);

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign(wasm);

        let different_wasm = b"\x00asm\x01\x00\x00\x01"; // Different bytes
        assert!(!signer.verify(different_wasm, &sig));
    }

    #[test]
    fn test_verify_fails_wrong_signer() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();

        let signer1 = ModuleSigner::new(key1);
        let signer2 = ModuleSigner::new(key2);

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer1.sign(wasm);

        // signer2 should not verify signer1's signature
        assert!(!signer2.verify(wasm, &sig));
    }

    #[test]
    fn test_sign_with_custom_metadata() {
        let key = SigningKey::generate();
        let signer = ModuleSigner::new(key);

        let metadata =
            SignatureMetadata::new(signer.key().key_id(), signer.key().public_key().clone())
                .with_claim("build_id", serde_json::json!("12345"));

        let wasm = b"\x00asm\x01\x00\x00\x00";
        let sig = signer.sign_with_metadata(wasm, metadata);

        assert!(sig.metadata.claims.contains_key("build_id"));
        assert!(signer.verify(wasm, &sig));
    }
}
