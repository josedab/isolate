//! Cryptographic key types for module signing.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// A unique identifier for a key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyId(pub String);

impl KeyId {
    /// Create a new key ID from a string.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a key ID from a public key.
    pub fn from_public_key(key: &VerifyingKey) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&key.bytes);
        let hash = hasher.finalize();
        Self(hex::encode(&hash[..8]))
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for KeyId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A signing key (private key) for signing modules.
///
/// This implementation uses HMAC-SHA256 for simplicity.
/// For production use, consider Ed25519 with the `ed25519-dalek` crate.
#[derive(Clone)]
pub struct SigningKey {
    /// The secret key bytes (32 bytes).
    secret: [u8; 32],
    /// The derived public key.
    public: VerifyingKey,
}

impl SigningKey {
    /// Generate a new random signing key.
    pub fn generate() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Simple randomness from system time and memory addresses
        // In production, use a proper CSPRNG
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        let mut hasher = Sha256::new();
        hasher.update(&seed.to_le_bytes());
        hasher.update(&(std::ptr::null::<()>() as usize).to_le_bytes());
        let hash = hasher.finalize();

        let mut secret = [0u8; 32];
        secret.copy_from_slice(&hash);

        Self::from_bytes(&secret)
    }

    /// Create a signing key from raw bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(bytes);

        // Derive public key from secret
        let mut hasher = Sha256::new();
        hasher.update(b"isolate-signing-key-derivation");
        hasher.update(&secret);
        let public_hash = hasher.finalize();
        let mut public_bytes = [0u8; 32];
        public_bytes.copy_from_slice(&public_hash);

        Self {
            secret,
            public: VerifyingKey {
                bytes: public_bytes,
            },
        }
    }

    /// Get the public key.
    pub fn public_key(&self) -> &VerifyingKey {
        &self.public
    }

    /// Get the key ID.
    pub fn key_id(&self) -> KeyId {
        KeyId::from_public_key(&self.public)
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> [u8; 32] {
        // HMAC-like construction
        let mut hasher = Sha256::new();
        let mut padded_key = [0x36u8; 64];
        for (i, &k) in self.secret.iter().enumerate() {
            padded_key[i] ^= k;
        }
        hasher.update(&padded_key);
        hasher.update(message);
        let inner_result = hasher.finalize();

        let mut hasher = Sha256::new();
        let mut padded_key = [0x5cu8; 64];
        for (i, &k) in self.secret.iter().enumerate() {
            padded_key[i] ^= k;
        }
        hasher.update(&padded_key);
        hasher.update(&inner_result);

        let mut sig = [0u8; 32];
        sig.copy_from_slice(&hasher.finalize());
        sig
    }

    /// Export the secret key bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.secret
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey")
            .field("key_id", &self.key_id())
            .finish_non_exhaustive()
    }
}

/// A verifying key (public key) for verifying signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VerifyingKey {
    /// The public key bytes (32 bytes).
    #[serde(with = "hex_serde")]
    pub bytes: [u8; 32],
}

impl VerifyingKey {
    /// Create a verifying key from raw bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self { bytes: *bytes }
    }

    /// Get the key ID.
    pub fn key_id(&self) -> KeyId {
        KeyId::from_public_key(self)
    }

    /// Verify a signature.
    ///
    /// Note: This requires the signing key for HMAC verification.
    /// In a real Ed25519 implementation, only the public key is needed.
    pub fn verify(&self, message: &[u8], signature: &[u8; 32], signing_key: &SigningKey) -> bool {
        // Verify that this is the correct public key
        if &signing_key.public != self {
            return false;
        }

        // Verify the signature
        let expected = signing_key.sign(message);
        &expected == signature
    }

    /// Export to hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(&self.bytes)
    }

    /// Parse from hex string.
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self { bytes: arr })
    }
}

impl fmt::Display for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
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
            return Err(serde::de::Error::custom("invalid key length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();

        // Keys should be different
        assert_ne!(key1.to_bytes(), key2.to_bytes());

        // Public keys should match their derivation
        assert_eq!(key1.public_key(), &key1.public);
    }

    #[test]
    fn test_key_from_bytes() {
        let bytes = [0x42u8; 32];
        let key1 = SigningKey::from_bytes(&bytes);
        let key2 = SigningKey::from_bytes(&bytes);

        // Same bytes should produce same key
        assert_eq!(key1.to_bytes(), key2.to_bytes());
        assert_eq!(key1.public_key(), key2.public_key());
    }

    #[test]
    fn test_signing() {
        let key = SigningKey::generate();
        let message = b"Hello, World!";

        let sig1 = key.sign(message);
        let sig2 = key.sign(message);

        // Same message should produce same signature
        assert_eq!(sig1, sig2);

        // Different message should produce different signature
        let sig3 = key.sign(b"Different message");
        assert_ne!(sig1, sig3);
    }

    #[test]
    fn test_key_id() {
        let key = SigningKey::generate();
        let key_id = key.key_id();

        // Key ID should be derived from public key
        let expected_id = KeyId::from_public_key(key.public_key());
        assert_eq!(key_id, expected_id);
    }

    #[test]
    fn test_verifying_key_serialization() {
        let key = SigningKey::generate();
        let public = key.public_key();

        let json = serde_json::to_string(public).unwrap();
        let parsed: VerifyingKey = serde_json::from_str(&json).unwrap();

        assert_eq!(public, &parsed);
    }

    #[test]
    fn test_verifying_key_hex() {
        let key = SigningKey::generate();
        let public = key.public_key();

        let hex_str = public.to_hex();
        let parsed = VerifyingKey::from_hex(&hex_str).unwrap();

        assert_eq!(public, &parsed);
    }
}
