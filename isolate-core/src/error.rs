//! Error types for the Isolate runtime.

use crate::capability::Capability;
use std::path::PathBuf;
use thiserror::Error;

/// Result type for Isolate operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for Isolate.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Error creating a sandbox.
    #[error("Failed to create sandbox: {0}")]
    Create(String),

    /// Error during WASM module compilation.
    #[error("WASM compilation error: {0}")]
    Compilation(String),

    /// Error during WASM module instantiation.
    #[error("WASM instantiation error: {0}")]
    Instantiation(String),

    /// Error during sandbox execution.
    #[error("Execution error: {0}")]
    Execution(String),

    /// Sandbox execution timed out.
    #[error("Execution timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// Sandbox ran out of fuel (CPU limit exceeded).
    #[error("CPU fuel exhausted (limit: {limit} units)")]
    FuelExhausted { limit: u64 },

    /// Memory limit exceeded.
    #[error("Memory limit exceeded (limit: {limit} bytes, requested: {requested} bytes)")]
    MemoryLimitExceeded { limit: usize, requested: usize },

    /// Capability not granted.
    #[error("Capability not granted: {0}")]
    CapabilityDenied(Capability),

    /// Invalid capability configuration.
    #[error("Invalid capability configuration: {0}")]
    InvalidCapability(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Sandbox is in an invalid state for the requested operation.
    #[error("Invalid sandbox state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },

    /// Snapshot error.
    #[error("Snapshot error: {0}")]
    Snapshot(String),

    /// Snapshot not found.
    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    /// I/O error.
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// Filesystem access denied.
    #[error("Filesystem access denied: {path}")]
    FilesystemAccessDenied { path: PathBuf },

    /// Network access denied.
    #[error("Network access denied: {host}")]
    NetworkAccessDenied { host: String },

    /// Internal engine error.
    #[error("Internal engine error: {0}")]
    Engine(String),

    /// Module validation failed.
    #[error("Module validation failed: {0}")]
    ModuleValidation(String),

    /// Function not found in module.
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    /// Invalid function signature.
    #[error("Invalid function signature for '{name}': expected {expected}, got {actual}")]
    InvalidSignature {
        name: String,
        expected: String,
        actual: String,
    },

    /// Pool exhausted.
    #[error("Warm pool exhausted, no available sandboxes")]
    PoolExhausted,

    /// HTTP client error.
    #[error("HTTP error: {0}")]
    Http(String),
}

impl Error {
    /// Returns true if this is a timeout error.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Error::Timeout(_))
    }

    /// Returns true if this is a resource limit error.
    pub fn is_resource_limit(&self) -> bool {
        matches!(
            self,
            Error::FuelExhausted { .. } | Error::MemoryLimitExceeded { .. } | Error::Timeout(_)
        )
    }

    /// Returns true if this is a capability error.
    pub fn is_capability_error(&self) -> bool {
        matches!(
            self,
            Error::CapabilityDenied(_)
                | Error::FilesystemAccessDenied { .. }
                | Error::NetworkAccessDenied { .. }
        )
    }

    /// Returns true if this is an HTTP error.
    pub fn is_http_error(&self) -> bool {
        matches!(self, Error::Http(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_error_categorization() {
        let timeout = Error::Timeout(Duration::from_secs(30));
        assert!(timeout.is_timeout());
        assert!(timeout.is_resource_limit());
        assert!(!timeout.is_capability_error());

        let fuel = Error::FuelExhausted { limit: 1000 };
        assert!(!fuel.is_timeout());
        assert!(fuel.is_resource_limit());

        let cap = Error::CapabilityDenied(Capability::stdout());
        assert!(cap.is_capability_error());
        assert!(!cap.is_resource_limit());
    }
}
