//! Error types for the Isolate runtime.

#![allow(missing_docs)]
use crate::capability::Capability;
use std::path::PathBuf;
use thiserror::Error;

/// Result type for Isolate operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for Isolate.
///
/// # Examples
///
/// ```
/// use isolate_core::Error;
/// use std::time::Duration;
///
/// // Check error categories
/// let timeout_err = Error::Timeout(Duration::from_secs(30));
/// assert!(timeout_err.is_timeout());
/// assert!(timeout_err.is_resource_limit());
///
/// let fuel_err = Error::FuelExhausted { limit: 1_000_000 };
/// assert!(fuel_err.is_resource_limit());
/// assert!(!fuel_err.is_timeout());
///
/// // Get fix suggestions
/// assert!(timeout_err.suggestion().is_some());
/// ```
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
    InvalidSignature { name: String, expected: String, actual: String },

    /// Pool exhausted.
    #[error("Warm pool exhausted, no available sandboxes")]
    PoolExhausted,

    /// HTTP client error.
    #[error("HTTP error: {0}")]
    Http(String),

    /// KV store error.
    #[error("KV store error: {0}")]
    KvStore(String),

    /// Policy evaluation error.
    #[error("Policy error: {0}")]
    Policy(String),

    /// Gateway error.
    #[error("Gateway error: {0}")]
    Gateway(String),

    /// Orchestrator error.
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    /// Marketplace error.
    #[error("Marketplace error: {0}")]
    Marketplace(String),
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

    /// Returns a suggestion for how to fix or investigate this error.
    ///
    /// Returns `None` if no specific suggestion is available.
    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Error::Create(_) => Some(
                "Check that the WASM module is valid and all required capabilities are granted.",
            ),
            Error::Compilation(_) => Some(
                "Verify the file is a valid WASM binary (starts with \\0asm magic bytes). \
                 Use 'isolate validate <file>' to diagnose module issues.",
            ),
            Error::Instantiation(_) => Some(
                "Ensure all required WASI imports are available and memory limits are sufficient. \
                 The module may require more initial memory than the configured limit.",
            ),
            Error::Execution(_) => Some(
                "Check that the entry point function exists and has the correct signature. \
                 Enable debug logging with -l debug for more details.",
            ),
            Error::Timeout(_) => Some(
                "Increase the timeout with --timeout <seconds> or optimize the WASM code. \
                 Consider adding fuel limits to catch infinite loops earlier.",
            ),
            Error::FuelExhausted { .. } => Some(
                "Increase the fuel limit with --fuel <amount> or optimize the WASM code. \
                 The default fuel limit may be too low for compute-intensive operations.",
            ),
            Error::MemoryLimitExceeded { .. } => Some(
                "Increase the memory limit with --memory-limit <size> (e.g., 256M, 1G). \
                 Consider if the module has a memory leak or requires more memory for its workload.",
            ),
            Error::CapabilityDenied(cap) => match cap {
                Capability::Stdio(_) => Some(
                    "Grant stdio capability with --cap-stdout, --cap-stderr, --cap-stdin, or --cap-stdio for all.",
                ),
                Capability::Filesystem(_) => Some(
                    "Grant filesystem capability with --cap-fs-read <path> or --cap-fs-write <path>.",
                ),
                Capability::Network(_) => Some(
                    "Grant network capability with --cap-http <host-pattern> and/or --cap-dns.",
                ),
                Capability::Time(_) => Some("Grant time capability with --cap-time."),
                Capability::Random(_) => Some("Grant random capability with --cap-random."),
                Capability::Environment(_) => Some(
                    "Pass environment variables explicitly with --env KEY=VALUE.",
                ),
                Capability::HostFunction(_) => Some(
                    "The required host function is not available. Check the sandbox configuration.",
                ),
            },
            Error::InvalidCapability(_) => Some(
                "Review the capability configuration syntax. Use 'isolate --help' for examples.",
            ),
            Error::InvalidConfig(_) => Some(
                "Review the configuration. Use 'isolate --help' for valid options and examples.",
            ),
            Error::InvalidState { .. } => Some(
                "This may indicate a bug or misuse of the API. Ensure operations are called in the correct order.",
            ),
            Error::Snapshot(_) => Some(
                "Snapshot operations may fail due to incompatible module state. \
                 Try creating a fresh sandbox instead.",
            ),
            Error::SnapshotNotFound(_) => Some(
                "List available snapshots with 'isolate snapshot list'. \
                 The snapshot may have been deleted or never created.",
            ),
            Error::Io { .. } => Some(
                "Check file permissions and that the path exists. \
                 Ensure the filesystem capability is granted for the required paths.",
            ),
            Error::FilesystemAccessDenied { .. } => Some(
                "Grant filesystem read/write capability for the required path with \
                 --cap-fs-read <path> or --cap-fs-write <path>.",
            ),
            Error::NetworkAccessDenied { .. } => Some(
                "Grant HTTP capability with --cap-http <host-pattern>. \
                 Use '*' to allow all hosts (not recommended for untrusted code).",
            ),
            Error::Engine(_) => Some(
                "This is an internal error. Please report this issue with the error details \
                 at https://github.com/josedab/isolate/issues",
            ),
            Error::ModuleValidation(_) => Some(
                "The WASM module failed validation. Ensure it was compiled correctly and \
                 is a valid WASM binary. Use 'isolate validate <file>' for details.",
            ),
            Error::FunctionNotFound(_) => Some(
                "Check that the entry point function exists in the module's exports. \
                 Use 'isolate info <file> --exports' to list available functions. \
                 The default entry point is '_start' for WASI modules.",
            ),
            Error::InvalidSignature { .. } => Some(
                "The function signature doesn't match what's expected. \
                 WASI entry points should have signature () -> () or (i32, i32) -> i32.",
            ),
            Error::PoolExhausted => Some(
                "All pre-warmed sandboxes are in use. Wait for running sandboxes to complete \
                 or increase the pool size in the server configuration.",
            ),
            Error::Http(_) => Some(
                "Check network connectivity and that the target URL is correct. \
                 Ensure --cap-http includes the required host pattern.",
            ),
            Error::KvStore(_) => Some(
                "Check KV store quota limits and key/value sizes. \
                 Use namespace stats to monitor usage.",
            ),
            Error::Policy(_) => Some(
                "Review the policy configuration. Ensure policy rules are valid \
                 and the evaluation context provides all required attributes.",
            ),
            Error::Gateway(_) => Some(
                "Check the gateway configuration and ensure the server is running. \
                 Review route definitions and middleware configuration.",
            ),
            Error::Orchestrator(_) => Some(
                "Check tenant quotas and orchestrator capacity. \
                 Review scheduler configuration and resource availability.",
            ),
            Error::Marketplace(_) => Some(
                "Check the module manifest and registry connectivity. \
                 Ensure module signatures are valid and trusted keys are configured.",
            ),
        }
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
