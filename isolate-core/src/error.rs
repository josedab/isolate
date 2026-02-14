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
    #[error("CPU fuel exhausted (limit: {limit} units)")]
    FuelExhausted { limit: u64 },

    /// Memory limit exceeded.
    ///
    /// Returned when the sandbox attempts to allocate more memory than
    /// the configured [`memory_limit`](crate::SandboxConfigBuilder::memory_limit).
    ///
    /// **Recovery:** Increase `--memory-limit <size>` (e.g., `256M`, `1G`).
    /// Check for memory leaks in the WASM module.
    #[error("Memory limit exceeded (limit: {limit} bytes, requested: {requested} bytes)")]
    MemoryLimitExceeded { limit: usize, requested: usize },

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
    InvalidState { expected: String, actual: String },

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
    FilesystemAccessDenied { path: PathBuf },

    /// Network access denied.
    ///
    /// Returned when the sandbox attempts to connect to a host not covered
    /// by any granted network capability.
    ///
    /// **Recovery:** Grant `--cap-http <host-pattern>`. Use `*` for all hosts
    /// (not recommended for untrusted code).
    #[error("Network access denied: {host}")]
    NetworkAccessDenied { host: String },

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
    /// `(i32, i32) -> i32`. Verify the module's exported function types.
    #[error("Invalid function signature for '{name}': expected {expected}, got {actual}")]
    InvalidSignature { name: String, expected: String, actual: String },

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
            Error::FuelExhausted { limit: 100 },
            Error::MemoryLimitExceeded { limit: 1024, requested: 2048 },
            Error::CapabilityDenied(Capability::stdout()),
            Error::InvalidCapability("test".into()),
            Error::InvalidConfig("test".into()),
            Error::InvalidState { expected: "Running".into(), actual: "Terminated".into() },
            Error::Snapshot("test".into()),
            Error::SnapshotNotFound("snap-1".into()),
            Error::Io { source: std::io::Error::new(std::io::ErrorKind::Other, "test") },
            Error::FilesystemAccessDenied { path: PathBuf::from("/etc") },
            Error::NetworkAccessDenied { host: "evil.com".into() },
            Error::Engine("test".into()),
            Error::ModuleValidation("test".into()),
            Error::FunctionNotFound("_start".into()),
            Error::InvalidSignature { name: "f".into(), expected: "a".into(), actual: "b".into() },
            Error::PoolExhausted,
            Error::Http("test".into()),
            Error::KvStore("test".into()),
            Error::Policy("test".into()),
            Error::Gateway("test".into()),
            Error::Orchestrator("test".into()),
            Error::Marketplace("test".into()),
        ];

        for err in &errors {
            assert!(
                err.suggestion().is_some(),
                "Error variant {:?} should have a suggestion",
                err
            );
        }
    }
}
