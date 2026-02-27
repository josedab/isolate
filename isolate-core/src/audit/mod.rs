//! Cryptographic audit logging with tamper-evident hash chains.
//!
//! This module provides secure audit logging with the following features:
//!
//! - **Hash Chaining**: Each audit entry includes a hash of the previous entry,
//!   creating a tamper-evident chain similar to blockchain.
//! - **Tamper Detection**: Verify the integrity of the audit log at any time.
//! - **Signed Entries**: Optionally sign entries with HMAC for authentication.
//! - **Export/Import**: Export audit logs as verifiable JSON.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::audit::{CryptoAuditLog, AuditEntry, AuditAction};
//!
//! let mut log = CryptoAuditLog::new();
//!
//! // Record entries
//! log.record(AuditEntry::new(
//!     sandbox_id,
//!     AuditAction::SandboxCreated,
//!     None,
//! ));
//!
//! // Verify chain integrity
//! assert!(log.verify_chain().is_ok());
//! ```

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.

mod chain;
mod entry;
pub mod sink;
mod verifier;

pub use chain::{CryptoAuditLog, CryptoAuditLogConfig};
pub use entry::{AuditAction, AuditEntry, AuditSeverity};
pub use verifier::{ChainVerificationError, ChainVerifier};

/// Hash used for audit chain.
pub type AuditHash = [u8; 32];

/// Convert a hash to hex string.
pub fn hash_to_hex(hash: &AuditHash) -> String {
    hex::encode(hash)
}

/// Parse a hex string to hash.
pub fn hex_to_hash(hex_str: &str) -> Result<AuditHash, hex::FromHexError> {
    let bytes = hex::decode(hex_str)?;
    if bytes.len() != 32 {
        return Err(hex::FromHexError::InvalidStringLength);
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_hex_conversion() {
        let hash: AuditHash = [0xab; 32];
        let hex_str = hash_to_hex(&hash);
        assert_eq!(hex_str.len(), 64);

        let parsed = hex_to_hash(&hex_str).unwrap();
        assert_eq!(parsed, hash);
    }

    #[test]
    fn test_invalid_hex_length() {
        let result = hex_to_hash("abcd");
        assert!(result.is_err());
    }
}
