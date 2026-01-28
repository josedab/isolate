//! Module signature types.

use super::keys::{KeyId, VerifyingKey};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata about a module signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureMetadata {
    /// Version of the signature format.
    pub version: u32,
    /// Timestamp when the signature was created.
    pub timestamp: DateTime<Utc>,
    /// ID of the signing key.
    pub key_id: KeyId,
    /// The public key used for verification.
    pub public_key: VerifyingKey,
    /// Optional human-readable name of the signer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_name: Option<String>,
    /// Optional URL or identifier for the signer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_url: Option<String>,
    /// Optional expiration time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Additional custom claims.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub claims: std::collections::HashMap<String, serde_json::Value>,
}

impl SignatureMetadata {
    /// Create new signature metadata.
    pub fn new(key_id: KeyId, public_key: VerifyingKey) -> Self {
        Self {
            version: 1,
            timestamp: Utc::now(),
            key_id,
            public_key,
            signer_name: None,
            signer_url: None,
            expires_at: None,
            claims: std::collections::HashMap::new(),
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

    /// Set expiration time.
    pub fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Add a custom claim.
    pub fn with_claim(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.claims.insert(key.into(), value);
        self
    }

    /// Check if the signature has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }
}

/// A complete module signature including metadata and the cryptographic signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSignature {
    /// Metadata about the signature.
    pub metadata: SignatureMetadata,
    /// SHA-256 hash of the module.
    #[serde(with = "hex_serde")]
    pub module_hash: [u8; 32],
    /// The cryptographic signature (HMAC-SHA256).
    #[serde(with = "hex_serde")]
    pub signature: [u8; 32],
}

impl ModuleSignature {
    /// Create a new module signature.
    pub fn new(metadata: SignatureMetadata, module_hash: [u8; 32], signature: [u8; 32]) -> Self {
        Self { metadata, module_hash, signature }
    }

    /// Get the key ID.
    pub fn key_id(&self) -> &KeyId {
        &self.metadata.key_id
    }

    /// Get the public key.
    pub fn public_key(&self) -> &VerifyingKey {
        &self.metadata.public_key
    }

    /// Check if the signature is expired.
    pub fn is_expired(&self) -> bool {
        self.metadata.is_expired()
    }

    /// Export to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Export to compact binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Simple format: version(4) + hash(32) + signature(32) + metadata_json
        let mut bytes = Vec::new();

        // Magic bytes
        bytes.extend_from_slice(b"ISIG");

        // Version
        bytes.extend_from_slice(&self.metadata.version.to_le_bytes());

        // Module hash
        bytes.extend_from_slice(&self.module_hash);

        // Signature
        bytes.extend_from_slice(&self.signature);

        // Metadata as JSON (with length prefix)
        let metadata_json = serde_json::to_string(&self.metadata).unwrap_or_default();
        let metadata_len = metadata_json.len() as u32;
        bytes.extend_from_slice(&metadata_len.to_le_bytes());
        bytes.extend_from_slice(metadata_json.as_bytes());

        bytes
    }

    /// Parse from binary format.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SignatureParseError> {
        if bytes.len() < 76 {
            return Err(SignatureParseError::InvalidLength);
        }

        // Check magic
        if &bytes[0..4] != b"ISIG" {
            return Err(SignatureParseError::InvalidMagic);
        }

        // Version
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 1 {
            return Err(SignatureParseError::UnsupportedVersion(version));
        }

        // Module hash
        let mut module_hash = [0u8; 32];
        module_hash.copy_from_slice(&bytes[8..40]);

        // Signature
        let mut signature = [0u8; 32];
        signature.copy_from_slice(&bytes[40..72]);

        // Metadata length
        let metadata_len =
            u32::from_le_bytes([bytes[72], bytes[73], bytes[74], bytes[75]]) as usize;

        if bytes.len() < 76 + metadata_len {
            return Err(SignatureParseError::InvalidLength);
        }

        // Metadata JSON
        let metadata_json = std::str::from_utf8(&bytes[76..76 + metadata_len])
            .map_err(|_| SignatureParseError::InvalidMetadata)?;
        let metadata: SignatureMetadata = serde_json::from_str(metadata_json)
            .map_err(|_| SignatureParseError::InvalidMetadata)?;

        Ok(Self { metadata, module_hash, signature })
    }
}

/// Error parsing a module signature.
#[derive(Debug, thiserror::Error)]
pub enum SignatureParseError {
    /// Invalid signature length.
    #[error("Invalid signature length")]
    InvalidLength,

    /// Invalid magic bytes.
    #[error("Invalid magic bytes (not a valid signature file)")]
    InvalidMagic,

    /// Unsupported signature version.
    #[error("Unsupported signature version: {0}")]
    UnsupportedVersion(u32),

    /// Invalid metadata.
    #[error("Invalid signature metadata")]
    InvalidMetadata,
}

mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("invalid hash length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::SigningKey;

    #[test]
    fn test_signature_metadata_creation() {
        let key = SigningKey::generate();
        let metadata = SignatureMetadata::new(key.key_id(), key.public_key().clone());

        assert_eq!(metadata.version, 1);
        assert!(!metadata.is_expired());
    }

    #[test]
    fn test_signature_metadata_with_options() {
        let key = SigningKey::generate();
        let metadata = SignatureMetadata::new(key.key_id(), key.public_key().clone())
            .with_signer_name("Test Signer")
            .with_signer_url("https://example.com")
            .with_claim("purpose", serde_json::json!("testing"));

        assert_eq!(metadata.signer_name, Some("Test Signer".to_string()));
        assert_eq!(metadata.signer_url, Some("https://example.com".to_string()));
        assert!(metadata.claims.contains_key("purpose"));
    }

    #[test]
    fn test_signature_expiration() {
        let key = SigningKey::generate();

        // Not expired
        let future = Utc::now() + chrono::Duration::hours(1);
        let metadata =
            SignatureMetadata::new(key.key_id(), key.public_key().clone()).with_expiration(future);
        assert!(!metadata.is_expired());

        // Expired
        let past = Utc::now() - chrono::Duration::hours(1);
        let metadata =
            SignatureMetadata::new(key.key_id(), key.public_key().clone()).with_expiration(past);
        assert!(metadata.is_expired());
    }

    #[test]
    fn test_module_signature_json_roundtrip() {
        let key = SigningKey::generate();
        let metadata = SignatureMetadata::new(key.key_id(), key.public_key().clone());
        let sig = ModuleSignature::new(metadata, [0x42; 32], [0xAB; 32]);

        let json = sig.to_json().unwrap();
        let parsed = ModuleSignature::from_json(&json).unwrap();

        assert_eq!(sig.module_hash, parsed.module_hash);
        assert_eq!(sig.signature, parsed.signature);
    }

    #[test]
    fn test_module_signature_binary_roundtrip() {
        let key = SigningKey::generate();
        let metadata =
            SignatureMetadata::new(key.key_id(), key.public_key().clone()).with_signer_name("Test");
        let sig = ModuleSignature::new(metadata, [0x42; 32], [0xAB; 32]);

        let bytes = sig.to_bytes();
        let parsed = ModuleSignature::from_bytes(&bytes).unwrap();

        assert_eq!(sig.module_hash, parsed.module_hash);
        assert_eq!(sig.signature, parsed.signature);
        assert_eq!(sig.metadata.signer_name, parsed.metadata.signer_name);
    }

    #[test]
    fn test_signature_parse_errors() {
        // Too short
        assert!(matches!(
            ModuleSignature::from_bytes(&[0; 10]),
            Err(SignatureParseError::InvalidLength)
        ));

        // Invalid magic
        let mut bad_magic = vec![0; 100];
        bad_magic[0..4].copy_from_slice(b"BAAD");
        assert!(matches!(
            ModuleSignature::from_bytes(&bad_magic),
            Err(SignatureParseError::InvalidMagic)
        ));
    }
}
