//! Error types for the Isolate runtime.

use crate::capability::Capability;
use serde::ser::{SerializeStruct, Serializer};
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
/// let fuel_err = Error::FuelExhausted { limit: 1_000_000, consumed: 1_000_001 };
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
    ///
    /// Returned by [`Sandbox::create()`](crate::Sandbox::create) when sandbox
    /// initialization fails (e.g., invalid config, engine setup failure).
    ///
    /// **Recovery:** Verify the [`SandboxConfig`](crate::SandboxConfig) is valid
    /// and all required capabilities are granted. Check the inner message for
    /// details.
    #[error("Failed to create sandbox: {0}")]
    Create(String),

    /// Error during WASM module compilation.
    ///
    /// Returned when the WASM binary cannot be compiled by the Wasmtime engine.
    /// Common causes: corrupt binary, unsupported WASM features, or invalid
    /// byte sequences.
    ///
    /// **Recovery:** Ensure the file is a valid `.wasm` binary (magic bytes
    /// `\0asm`). Use `isolate validate <file>` to diagnose.
    #[error("WASM compilation error: {0}")]
    Compilation(String),

    /// Error during WASM module instantiation.
    ///
    /// Returned when a compiled module cannot be instantiated—typically due to
    /// unsatisfied imports, insufficient memory for initial allocation, or
    /// missing WASI functions.
    ///
    /// **Recovery:** Ensure WASI imports are available and memory limits are
    /// sufficient for the module's initial memory requirements.
    #[error("WASM instantiation error: {0}")]
    Instantiation(String),

    /// Error during sandbox execution.
    ///
    /// Returned when the WASM module traps or encounters a runtime error
    /// during execution (e.g., unreachable instruction, division by zero,
    /// stack overflow).
    ///
    /// **Recovery:** Check the entry-point function exists and has the correct
    /// signature. Enable debug logging (`-l debug`) for more details.
    #[error("Execution error: {0}")]
    Execution(String),

    /// Sandbox execution timed out.
    ///
    /// Returned when wall-clock time exceeds the configured
    /// [`wall_time_limit`](crate::SandboxConfig). Enforced via epoch-based
    /// interruption.
    ///
    /// **Recovery:** Increase the timeout with `--timeout` or optimize the
    /// WASM code. Add fuel limits to catch infinite loops earlier.
    #[error("Execution timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// Sandbox ran out of fuel (CPU limit exceeded).
    ///
    /// Returned when the sandbox exhausts its fuel budget, indicating the
    /// instruction count limit was reached.
    ///
    /// **Recovery:** Increase the fuel limit with `--fuel <amount>` or
    /// optimize the WASM code. The default may be too low for compute-heavy
    /// workloads.
    #[error("CPU fuel exhausted (limit: {limit} units, consumed: {consumed} units)")]
    FuelExhausted {
        /// The fuel limit that was exceeded.
        limit: u64,
        /// The amount of fuel consumed before exhaustion.
        consumed: u64,
    },

    /// Memory limit exceeded.
    ///
    /// Returned when the sandbox attempts to allocate more memory than
    /// the configured [`memory_limit`](crate::SandboxConfigBuilder::memory_limit).
    ///
    /// **Recovery:** Increase `--memory-limit <size>` (e.g., `256M`, `1G`).
    /// Check for memory leaks in the WASM module.
    #[error("Memory limit exceeded (limit: {limit} bytes, requested: {requested} bytes, current usage: {current_usage} bytes)")]
    MemoryLimitExceeded {
        /// The configured memory limit in bytes.
        limit: usize,
        /// The number of bytes the sandbox attempted to allocate.
        requested: usize,
        /// The memory usage at the time of the failed allocation.
        current_usage: usize,
    },

    /// Capability not granted.
    ///
    /// Returned when the sandbox attempts an operation (I/O, network, etc.)
    /// for which no capability was granted via
    /// [`capability()`](crate::SandboxConfigBuilder::capability).
    ///
    /// **Recovery:** Grant the required capability in the sandbox config.
    /// Use `--cap-stdout`, `--cap-fs-read <path>`, etc.
    #[error("Capability not granted: {0}")]
    CapabilityDenied(Capability),

    /// Invalid capability configuration.
    ///
    /// Returned when a capability is syntactically or semantically invalid
    /// (e.g., malformed path pattern, conflicting grants).
    ///
    /// **Recovery:** Review the capability configuration syntax. Use
    /// `isolate --help` for valid patterns.
    #[error("Invalid capability configuration: {0}")]
    InvalidCapability(String),

    /// Invalid configuration.
    ///
    /// Returned by [`SandboxConfigBuilder::build()`](crate::SandboxConfigBuilder::build)
    /// when required fields are missing or values are out of range (e.g., no
    /// WASM module provided).
    ///
    /// **Recovery:** Ensure at least a WASM module is set. Check builder
    /// documentation for required vs. optional fields.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Sandbox is in an invalid state for the requested operation.
    ///
    /// Returned when an operation is called on a sandbox whose lifecycle
    /// state does not permit it (e.g., calling `run()` on a terminated sandbox).
    ///
    /// **Recovery:** Ensure operations are called in the correct order:
    /// create → run/call → terminate. Create a new sandbox if needed.
    #[error("Invalid sandbox state: expected {expected}, got {actual}")]
    InvalidState {
        /// The expected sandbox state for this operation.
        expected: String,
        /// The actual sandbox state at the time of the call.
        actual: String,
    },

    /// Snapshot error.
    ///
    /// Returned when a snapshot operation (save/restore) fails due to
    /// incompatible module state, I/O issues, or serialization errors.
    ///
    /// **Recovery:** Try creating a fresh sandbox instead. Ensure snapshot
    /// storage path is writable.
    #[error("Snapshot error: {0}")]
    Snapshot(String),

    /// Snapshot not found.
    ///
    /// Returned when attempting to restore from a snapshot ID that does not
    /// exist in the configured snapshot storage.
    ///
    /// **Recovery:** List available snapshots with `isolate snapshot list`.
    /// The snapshot may have been deleted or never created.
    #[error("Snapshot not found: {0}")]
    SnapshotNotFound(String),

    /// I/O error.
    ///
    /// Wraps a [`std::io::Error`] from file or stream operations. Commonly
    /// returned when reading WASM files or accessing preopened directories.
    ///
    /// **Recovery:** Check file permissions, path existence, and that
    /// filesystem capabilities are granted for the required paths.
    #[error("I/O error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },

    /// Filesystem access denied.
    ///
    /// Returned when the sandbox attempts to access a filesystem path that
    /// is not covered by any granted filesystem capability.
    ///
    /// **Recovery:** Grant `--cap-fs-read <path>` or `--cap-fs-write <path>`
    /// for the required path.
    #[error("Filesystem access denied: {path}")]
    FilesystemAccessDenied {
        /// The filesystem path that was denied.
        path: PathBuf,
    },

    /// Network access denied.
    ///
    /// Returned when the sandbox attempts to connect to a host not covered
    /// by any granted network capability.
    ///
    /// **Recovery:** Grant `--cap-http <host-pattern>`. Use `*` for all hosts
    /// (not recommended for untrusted code).
    #[error("Network access denied: {host}")]
    NetworkAccessDenied {
        /// The host that was denied access.
        host: String,
    },

    /// Internal engine error.
    ///
    /// Returned for unexpected failures inside the Wasmtime engine layer.
    /// This typically indicates a bug or an unsupported edge case.
    ///
    /// **Recovery:** Report the issue at
    /// <https://github.com/josedab/isolate/issues> with the full error message.
    #[error("Internal engine error: {0}")]
    Engine(String),

    /// Module validation failed.
    ///
    /// Returned when a WASM module fails structural validation (e.g.,
    /// invalid section layout, unsupported proposals).
    ///
    /// **Recovery:** Ensure the module was compiled correctly. Use
    /// `isolate validate <file>` for detailed diagnostics.
    #[error("Module validation failed: {0}")]
    ModuleValidation(String),

    /// Function not found in module.
    ///
    /// Returned when the requested entry point or function name does not
    /// exist in the module's exports.
    ///
    /// **Recovery:** Check exports with `isolate info <file> --exports`.
    /// The default entry point is `_start` for WASI modules.
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    /// Invalid function signature.
    ///
    /// Returned when an exported function's parameter or return types do
    /// not match the expected signature.
    ///
    /// **Recovery:** WASI entry points should have signature `() -> ()` or
    /// `(i32, i32) -> i32`. Verify the module's exported function types with
    /// `isolate info <file> --exports`.
    #[error("Invalid function signature for '{name}': expected {expected}, got {actual}")]
    InvalidSignature {
        /// The function name with the mismatched signature.
        name: String,
        /// The expected function signature (e.g., "(i32, i32) -> i32").
        expected: String,
        /// The actual function signature found in the module.
        actual: String,
        /// Number of parameters expected, if known.
        expected_params: Option<usize>,
        /// Number of parameters found, if known.
        actual_params: Option<usize>,
    },

    /// Pool exhausted.
    ///
    /// Returned when all pre-warmed sandboxes in the warm pool are in use
    /// and no idle instance is available.
    ///
    /// **Recovery:** Wait for running sandboxes to complete, or increase the
    /// pool size in the server configuration.
    #[error("Warm pool exhausted, no available sandboxes")]
    PoolExhausted,

    /// HTTP client error.
    ///
    /// Returned when an outbound HTTP request from the sandbox fails (e.g.,
    /// connection refused, DNS failure, response too large).
    ///
    /// **Recovery:** Check network connectivity, the target URL, and that
    /// `--cap-http` includes the required host pattern.
    #[error("HTTP error: {0}")]
    Http(String),

    /// KV store error.
    ///
    /// Returned when a key-value store operation fails (e.g., quota exceeded,
    /// key/value too large, version mismatch on CAS).
    ///
    /// **Recovery:** Check KV store quota limits and key/value sizes. Use
    /// namespace stats to monitor usage.
    #[error("KV store error: {0}")]
    KvStore(String),

    /// Policy evaluation error.
    ///
    /// Returned when a security policy rule fails to evaluate or denies the
    /// requested operation.
    ///
    /// **Recovery:** Review the policy configuration. Ensure policy rules are
    /// valid and the evaluation context provides all required attributes.
    #[error("Policy error: {0}")]
    Policy(String),

    /// Gateway error.
    ///
    /// Returned from gateway-layer operations (routing, middleware, request
    /// handling) in the `isolate-server`.
    ///
    /// **Recovery:** Check the gateway configuration, ensure the server is
    /// running, and review route definitions.
    #[error("Gateway error: {0}")]
    Gateway(String),

    /// Orchestrator error.
    ///
    /// Returned from multi-tenant orchestration operations (scheduling,
    /// admission control, tenant quota enforcement).
    ///
    /// **Recovery:** Check tenant quotas and orchestrator capacity. Review
    /// scheduler configuration and resource availability.
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    /// Marketplace error.
    ///
    /// Returned from module marketplace operations (registry lookups, module
    /// verification, manifest parsing).
    ///
    /// **Recovery:** Check the module manifest and registry connectivity.
    /// Ensure module signatures are valid and trusted keys are configured.
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

    /// Returns true if this is a security-related error.
    ///
    /// Security errors include capability denials, access control failures,
    /// and policy violations.
    pub fn is_security_error(&self) -> bool {
        matches!(self.category(), ErrorCategory::Security)
    }

    /// Returns true if this is a module-related error.
    ///
    /// Module errors include compilation failures, validation errors, and
    /// instantiation problems.
    pub fn is_module_error(&self) -> bool {
        matches!(self.category(), ErrorCategory::Module)
    }

    /// Returns true if this is a configuration error.
    pub fn is_config_error(&self) -> bool {
        matches!(self.category(), ErrorCategory::Config)
    }

    /// Returns true if this is an internal/service error.
    pub fn is_internal_error(&self) -> bool {
        matches!(self.category(), ErrorCategory::Internal)
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
                 Check the 'consumed' field to gauge how close to the limit the workload runs.",
            ),
            Error::MemoryLimitExceeded { .. } => Some(
                "Increase the memory limit with --memory-limit <size> (e.g., 256M, 1G). \
                 Compare 'current_usage' with 'requested' to distinguish memory leaks from large allocations.",
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
                 WASI entry points should have signature () -> (). \
                 Use 'isolate info <file> --exports' to check the module's exported function types.",
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

    /// Get a machine-readable error code string for this error variant.
    ///
    /// These codes are stable across releases and can be used by SDKs
    /// and tools for programmatic error handling.
    pub fn error_code(&self) -> &'static str {
        match self {
            Error::Create(_) => "SANDBOX_CREATE",
            Error::Compilation(_) => "WASM_COMPILATION",
            Error::Instantiation(_) => "WASM_INSTANTIATION",
            Error::Execution(_) => "WASM_EXECUTION",
            Error::Timeout(_) => "TIMEOUT",
            Error::FuelExhausted { .. } => "FUEL_EXHAUSTED",
            Error::MemoryLimitExceeded { .. } => "MEMORY_LIMIT",
            Error::CapabilityDenied(_) => "CAPABILITY_DENIED",
            Error::InvalidCapability(_) => "INVALID_CAPABILITY",
            Error::InvalidConfig(_) => "INVALID_CONFIG",
            Error::InvalidState { .. } => "INVALID_STATE",
            Error::Snapshot(_) => "SNAPSHOT",
            Error::SnapshotNotFound(_) => "SNAPSHOT_NOT_FOUND",
            Error::Io { .. } => "IO_ERROR",
            Error::FilesystemAccessDenied { .. } => "FS_ACCESS_DENIED",
            Error::NetworkAccessDenied { .. } => "NET_ACCESS_DENIED",
            Error::Engine(_) => "ENGINE_INTERNAL",
            Error::ModuleValidation(_) => "MODULE_VALIDATION",
            Error::FunctionNotFound(_) => "FUNCTION_NOT_FOUND",
            Error::InvalidSignature { .. } => "INVALID_SIGNATURE",
            Error::PoolExhausted => "POOL_EXHAUSTED",
            Error::Http(_) => "HTTP_ERROR",
            Error::KvStore(_) => "KV_STORE",
            Error::Policy(_) => "POLICY_VIOLATION",
            Error::Gateway(_) => "GATEWAY_ERROR",
            Error::Orchestrator(_) => "ORCHESTRATOR_ERROR",
            Error::Marketplace(_) => "MARKETPLACE_ERROR",
        }
    }

    /// Get the high-level error category.
    ///
    /// Categories group error variants for SDK-level retry/handling logic.
    pub fn category(&self) -> ErrorCategory {
        match self {
            Error::Create(_)
            | Error::Compilation(_)
            | Error::Instantiation(_)
            | Error::ModuleValidation(_) => ErrorCategory::Module,

            Error::Execution(_) | Error::FunctionNotFound(_) | Error::InvalidSignature { .. } => {
                ErrorCategory::Runtime
            }

            Error::Timeout(_)
            | Error::FuelExhausted { .. }
            | Error::MemoryLimitExceeded { .. }
            | Error::PoolExhausted => ErrorCategory::Resource,

            Error::CapabilityDenied(_)
            | Error::InvalidCapability(_)
            | Error::FilesystemAccessDenied { .. }
            | Error::NetworkAccessDenied { .. }
            | Error::Policy(_) => ErrorCategory::Security,

            Error::InvalidConfig(_) | Error::InvalidState { .. } => ErrorCategory::Config,

            Error::Io { .. } | Error::Http(_) => ErrorCategory::Io,

            Error::Snapshot(_) | Error::SnapshotNotFound(_) => ErrorCategory::Snapshot,

            Error::Engine(_)
            | Error::KvStore(_)
            | Error::Gateway(_)
            | Error::Orchestrator(_)
            | Error::Marketplace(_) => ErrorCategory::Internal,
        }
    }

    /// Whether this error is likely transient and the operation could succeed
    /// if retried.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Error::Timeout(_) | Error::PoolExhausted | Error::Http(_) | Error::Engine(_))
    }
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Error", 5)?;
        state.serialize_field("code", self.error_code())?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("category", &self.category())?;
        state.serialize_field("retryable", &self.is_retryable())?;
        state.serialize_field("suggestion", &self.suggestion())?;
        state.end()
    }
}

/// High-level error category for SDK-friendly handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// Module compilation/validation errors.
    Module,
    /// Runtime execution errors.
    Runtime,
    /// Resource limit errors (fuel, memory, timeout, pool).
    Resource,
    /// Security/capability errors.
    Security,
    /// Configuration errors.
    Config,
    /// I/O and network errors.
    Io,
    /// Snapshot errors.
    Snapshot,
    /// Internal/service errors.
    Internal,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module => write!(f, "module"),
            Self::Runtime => write!(f, "runtime"),
            Self::Resource => write!(f, "resource"),
            Self::Security => write!(f, "security"),
            Self::Config => write!(f, "config"),
            Self::Io => write!(f, "io"),
            Self::Snapshot => write!(f, "snapshot"),
            Self::Internal => write!(f, "internal"),
        }
    }
}
///
/// Use this to enrich errors with sandbox_id, module_hash, or other
/// metadata without changing the underlying error variant.
#[derive(Debug)]
pub struct ErrorContext {
    /// The underlying error.
    pub error: Error,
    /// Contextual key-value pairs.
    pub context: Vec<(String, String)>,
}

impl ErrorContext {
    /// Wrap an error with context.
    pub fn new(error: Error) -> Self {
        Self { error, context: Vec::new() }
    }

    /// Add a context key-value pair.
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.push((key.into(), value.into()));
        self
    }

    /// Get a context value by key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.context.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)?;
        if !self.context.is_empty() {
            write!(f, " [")?;
            for (i, (k, v)) in self.context.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}={}", k, v)?;
            }
            write!(f, "]")?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorContext {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<Error> for ErrorContext {
    fn from(error: Error) -> Self {
        Self::new(error)
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
        assert!(!timeout.is_security_error());
        assert!(!timeout.is_module_error());

        let fuel = Error::FuelExhausted { limit: 1000, consumed: 1001 };
        assert!(!fuel.is_timeout());
        assert!(fuel.is_resource_limit());

        let cap = Error::CapabilityDenied(Capability::stdout());
        assert!(cap.is_capability_error());
        assert!(!cap.is_resource_limit());
    }

    #[test]
    fn test_engine_error() {
        let err = Error::Engine("wasmtime internal failure".into());
        assert_eq!(format!("{}", err), "Internal engine error: wasmtime internal failure");
        assert!(err.suggestion().is_some());
        assert!(!err.is_timeout());
        assert!(!err.is_resource_limit());
        assert!(!err.is_capability_error());
    }

    #[test]
    fn test_module_validation_error() {
        let err = Error::ModuleValidation("invalid section layout".into());
        assert_eq!(format!("{}", err), "Module validation failed: invalid section layout");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_function_not_found_error() {
        let err = Error::FunctionNotFound("_start".into());
        assert_eq!(format!("{}", err), "Function not found: _start");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_invalid_signature_error() {
        let err = Error::InvalidSignature {
            name: "main".into(),
            expected: "() -> ()".into(),
            actual: "(i32) -> i32".into(),
            expected_params: Some(0),
            actual_params: Some(1),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("main"));
        assert!(msg.contains("() -> ()"));
        assert!(msg.contains("(i32) -> i32"));
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_http_error() {
        let err = Error::Http("connection refused".into());
        assert_eq!(format!("{}", err), "HTTP error: connection refused");
        assert!(err.is_http_error());
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_kv_store_error() {
        let err = Error::KvStore("quota exceeded".into());
        assert_eq!(format!("{}", err), "KV store error: quota exceeded");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_gateway_error() {
        let err = Error::Gateway("route not found".into());
        assert_eq!(format!("{}", err), "Gateway error: route not found");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_marketplace_error() {
        let err = Error::Marketplace("module not found".into());
        assert_eq!(format!("{}", err), "Marketplace error: module not found");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_orchestrator_error() {
        let err = Error::Orchestrator("scheduler overloaded".into());
        assert_eq!(format!("{}", err), "Orchestrator error: scheduler overloaded");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_policy_error() {
        let err = Error::Policy("deny rule matched".into());
        assert_eq!(format!("{}", err), "Policy error: deny rule matched");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_pool_exhausted_error() {
        let err = Error::PoolExhausted;
        assert_eq!(format!("{}", err), "Warm pool exhausted, no available sandboxes");
        assert!(err.suggestion().is_some());
    }

    #[test]
    fn test_io_error_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io { .. }));
        let msg = format!("{}", err);
        assert!(msg.contains("file missing"));
    }

    #[test]
    fn test_all_errors_have_suggestions() {
        let errors: Vec<Error> = vec![
            Error::Create("test".into()),
            Error::Compilation("test".into()),
            Error::Instantiation("test".into()),
            Error::Execution("test".into()),
            Error::Timeout(Duration::from_secs(1)),
            Error::FuelExhausted { limit: 100, consumed: 101 },
            Error::MemoryLimitExceeded { limit: 1024, requested: 2048, current_usage: 512 },
            Error::CapabilityDenied(Capability::stdout()),
            Error::InvalidCapability("test".into()),
            Error::InvalidConfig("test".into()),
            Error::InvalidState { expected: "Running".into(), actual: "Terminated".into() },
            Error::Snapshot("test".into()),
            Error::SnapshotNotFound("snap-1".into()),
            Error::Io { source: std::io::Error::other("test") },
            Error::FilesystemAccessDenied { path: PathBuf::from("/etc") },
            Error::NetworkAccessDenied { host: "evil.com".into() },
            Error::Engine("test".into()),
            Error::ModuleValidation("test".into()),
            Error::FunctionNotFound("_start".into()),
            Error::InvalidSignature {
                name: "f".into(),
                expected: "a".into(),
                actual: "b".into(),
                expected_params: None,
                actual_params: None,
            },
            Error::PoolExhausted,
            Error::Http("test".into()),
            Error::KvStore("test".into()),
            Error::Policy("test".into()),
            Error::Gateway("test".into()),
            Error::Orchestrator("test".into()),
            Error::Marketplace("test".into()),
        ];

        for err in &errors {
            assert!(err.suggestion().is_some(), "Error variant {:?} should have a suggestion", err);
        }
    }

    #[test]
    fn test_is_retryable() {
        // Retryable errors
        assert!(Error::Timeout(Duration::from_secs(1)).is_retryable());
        assert!(Error::PoolExhausted.is_retryable());
        assert!(Error::Http("conn refused".into()).is_retryable());
        assert!(Error::Engine("internal".into()).is_retryable());

        // Non-retryable errors
        assert!(!Error::Compilation("invalid".into()).is_retryable());
        assert!(!Error::CapabilityDenied(Capability::stdout()).is_retryable());
        assert!(!Error::FuelExhausted { limit: 100, consumed: 101 }.is_retryable());
        assert!(!Error::InvalidConfig("bad".into()).is_retryable());
        assert!(!Error::MemoryLimitExceeded { limit: 100, requested: 200, current_usage: 50 }
            .is_retryable());
    }

    #[test]
    fn test_error_category() {
        assert_eq!(Error::Compilation("x".into()).category(), ErrorCategory::Module);
        assert_eq!(Error::Execution("x".into()).category(), ErrorCategory::Runtime);
        assert_eq!(
            Error::FuelExhausted { limit: 1, consumed: 2 }.category(),
            ErrorCategory::Resource
        );
        assert_eq!(
            Error::CapabilityDenied(Capability::stdout()).category(),
            ErrorCategory::Security
        );
        assert_eq!(Error::InvalidConfig("x".into()).category(), ErrorCategory::Config);
        assert_eq!(Error::Http("x".into()).category(), ErrorCategory::Io);
        assert_eq!(Error::Snapshot("x".into()).category(), ErrorCategory::Snapshot);
        assert_eq!(Error::Engine("x".into()).category(), ErrorCategory::Internal);
    }

    #[test]
    fn test_error_code_all_variants() {
        let errors = vec![
            Error::Create("x".into()),
            Error::Compilation("x".into()),
            Error::Execution("x".into()),
            Error::Timeout(Duration::from_secs(1)),
            Error::FuelExhausted { limit: 1, consumed: 2 },
            Error::PoolExhausted,
        ];
        for err in &errors {
            let code = err.error_code();
            assert!(!code.is_empty());
            // Codes should be UPPER_SNAKE_CASE
            assert_eq!(code, code.to_uppercase());
        }
    }

    #[test]
    fn test_error_context() {
        let ctx = ErrorContext::new(Error::Execution("failed".into()))
            .with("sandbox_id", "sb-123")
            .with("module_hash", "abc123");

        assert_eq!(ctx.get("sandbox_id"), Some("sb-123"));
        assert_eq!(ctx.get("module_hash"), Some("abc123"));
        assert_eq!(ctx.get("missing"), None);

        let display = format!("{}", ctx);
        assert!(display.contains("Execution error: failed"));
        assert!(display.contains("sandbox_id=sb-123"));
    }

    #[test]
    fn test_error_context_from_error() {
        let err = Error::Timeout(Duration::from_secs(10));
        let ctx: ErrorContext = err.into();
        assert!(format!("{}", ctx).contains("timed out"));
        assert!(ctx.context.is_empty());
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(format!("{}", ErrorCategory::Module), "module");
        assert_eq!(format!("{}", ErrorCategory::Resource), "resource");
        assert_eq!(format!("{}", ErrorCategory::Security), "security");
    }

    #[test]
    fn test_is_security_error() {
        assert!(Error::CapabilityDenied(Capability::stdout()).is_security_error());
        assert!(Error::FilesystemAccessDenied { path: PathBuf::from("/etc") }.is_security_error());
        assert!(Error::NetworkAccessDenied { host: "evil.com".into() }.is_security_error());
        assert!(Error::Policy("deny".into()).is_security_error());
        assert!(!Error::Timeout(Duration::from_secs(1)).is_security_error());
    }

    #[test]
    fn test_is_module_error() {
        assert!(Error::Compilation("bad".into()).is_module_error());
        assert!(Error::ModuleValidation("invalid".into()).is_module_error());
        assert!(Error::Instantiation("failed".into()).is_module_error());
        assert!(Error::Create("err".into()).is_module_error());
        assert!(!Error::Execution("trap".into()).is_module_error());
    }

    #[test]
    fn test_is_config_error() {
        assert!(Error::InvalidConfig("bad".into()).is_config_error());
        assert!(Error::InvalidState { expected: "Ready".into(), actual: "Terminated".into() }
            .is_config_error());
        assert!(!Error::Compilation("bad".into()).is_config_error());
    }

    #[test]
    fn test_is_internal_error() {
        assert!(Error::Engine("internal".into()).is_internal_error());
        assert!(Error::Gateway("err".into()).is_internal_error());
        assert!(Error::Orchestrator("err".into()).is_internal_error());
        assert!(!Error::Timeout(Duration::from_secs(1)).is_internal_error());
    }

    #[test]
    fn test_error_serialize_json() {
        let err = Error::Timeout(Duration::from_secs(30));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "TIMEOUT");
        assert_eq!(json["category"], "resource");
        assert_eq!(json["retryable"], true);
        assert!(json["message"].as_str().unwrap().contains("30"));
        assert!(json["suggestion"].as_str().is_some());
    }

    #[test]
    fn test_error_serialize_capability_denied() {
        let err = Error::CapabilityDenied(Capability::stdout());
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "CAPABILITY_DENIED");
        assert_eq!(json["category"], "security");
        assert_eq!(json["retryable"], false);
    }

    #[test]
    fn test_error_serialize_io_error() {
        let err = Error::Io { source: std::io::Error::other("disk full") };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "IO_ERROR");
        assert_eq!(json["category"], "io");
    }
}
