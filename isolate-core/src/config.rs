//! Sandbox configuration.
//!
//! This module provides the configuration types for creating sandboxes.
//!
//! # Builder Pattern
//!
//! Configuration is built using the [`SandboxConfigBuilder`]:
//!
//! ```no_run
//! use isolate_core::{SandboxConfig, capability::Capability};
//! use std::time::Duration;
//!
//! # fn example() -> isolate_core::Result<()> {
//! let wasm = std::fs::read("module.wasm")?;
//!
//! let config = SandboxConfig::builder()
//!     // Required: WASM module
//!     .module(&wasm)?
//!
//!     // Resource limits
//!     .memory_limit(128 * 1024 * 1024)   // 128 MB heap
//!     .fuel(10_000_000)                   // CPU fuel units
//!     .wall_time_limit(Duration::from_secs(30))
//!     .io_write_limit(1024 * 1024)       // 1 MB output
//!
//!     // Capabilities
//!     .capability(Capability::stdout())
//!     .capability(Capability::stderr())
//!     .capability(Capability::filesystem_read("/data"))
//!
//!     // Environment
//!     .env("API_KEY", "secret")
//!     .arg("--verbose".to_string())
//!
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Available Options
//!
//! | Method | Description | Default |
//! |--------|-------------|---------|
//! | `module()` | WASM module bytes | Required |
//! | `memory_limit()` | Maximum heap memory | 64 MB |
//! | `fuel()` | CPU fuel units | Unlimited |
//! | `wall_time_limit()` | Maximum execution time | Unlimited |
//! | `io_read_limit()` | Maximum bytes read | Unlimited |
//! | `io_write_limit()` | Maximum bytes written | Unlimited |
//! | `capability()` | Grant a capability | None |
//! | `env()` | Set environment variable | Empty |
//! | `arg()` | Add command-line argument | Empty |
//! | `entry_point()` | Function to call | `_start` |

use crate::capability::{Capability, CapabilitySet};
use crate::error::{Error, Result};
use crate::profile::LanguageProfile;
use crate::ratelimit::RateLimitConfig;
use crate::resource::ResourceLimits;
use crate::sandbox_profile::SandboxProfile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Hash of a WASM module for caching and identification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleHash(pub String);

impl ModuleHash {
    /// Compute hash from WASM bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        Self(hex::encode(result))
    }
}

impl std::fmt::Display for ModuleHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0[..16])
    }
}

/// WASM module representation.
#[derive(Clone)]
pub struct WasmModule {
    /// Raw WASM bytes.
    bytes: Vec<u8>,
    /// Precomputed hash.
    hash: ModuleHash,
}

impl WasmModule {
    /// Create a new WASM module from bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 8 {
            return Err(Error::ModuleValidation("WASM module too small".to_string()));
        }

        // Check WASM magic number: \0asm
        if &bytes[0..4] != b"\0asm" {
            return Err(Error::ModuleValidation("Invalid WASM magic number".to_string()));
        }

        let hash = ModuleHash::from_bytes(&bytes);
        Ok(Self { bytes, hash })
    }

    /// Get the raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get the module hash.
    pub fn hash(&self) -> &ModuleHash {
        &self.hash
    }
}

impl std::fmt::Debug for WasmModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmModule")
            .field("size", &self.bytes.len())
            .field("hash", &self.hash)
            .finish()
    }
}

/// Snapshot configuration.
#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Enable snapshotting.
    pub enabled: bool,
    /// Snapshot storage path.
    pub storage_path: Option<PathBuf>,
    /// Maximum snapshots to keep per module.
    pub max_snapshots: usize,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self { enabled: false, storage_path: None, max_snapshots: 10 }
    }
}

/// Configuration for creating a sandbox.
///
/// Use the builder pattern via [`SandboxConfig::builder()`] to construct.
///
/// # Examples
///
/// ```
/// use isolate_core::{SandboxConfig, capability::Capability};
///
/// // Minimal valid WASM module (empty)
/// let wasm = &[0x00u8, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
///
/// let config = SandboxConfig::builder()
///     .module(wasm)
///     .unwrap()
///     .memory_limit(64 * 1024 * 1024)
///     .fuel(1_000_000)
///     .capability(Capability::stdout())
///     .env("MODE", "production")
///     .build()
///     .unwrap();
///
/// assert_eq!(config.resources.memory.heap_max, 64 * 1024 * 1024);
/// assert_eq!(config.resources.cpu.fuel, Some(1_000_000));
/// assert_eq!(config.entry_point, "_start");
/// ```
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// The WASM module to execute.
    pub module: WasmModule,
    /// Granted capabilities.
    pub capabilities: CapabilitySet,
    /// Resource limits.
    pub resources: ResourceLimits,
    /// Environment variables.
    pub env: HashMap<String, String>,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Snapshot configuration.
    pub snapshot: SnapshotConfig,
    /// Entry point function name (default: "_start").
    pub entry_point: String,
    /// Rate limiting configuration.
    pub rate_limit: RateLimitConfig,
}

impl SandboxConfig {
    /// Create a new configuration builder.
    pub fn builder() -> SandboxConfigBuilder {
        SandboxConfigBuilder::new()
    }

    /// Get the module hash.
    pub fn module_hash(&self) -> &ModuleHash {
        self.module.hash()
    }
}

/// Builder for SandboxConfig.
#[derive(Debug, Default)]
#[must_use = "builders do nothing unless you call .build()"]
pub struct SandboxConfigBuilder {
    module: Option<WasmModule>,
    capabilities: CapabilitySet,
    resources: ResourceLimits,
    env: HashMap<String, String>,
    args: Vec<String>,
    snapshot: SnapshotConfig,
    entry_point: String,
    rate_limit: RateLimitConfig,
}

impl SandboxConfigBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self { entry_point: "_start".to_string(), ..Default::default() }
    }

    /// Set the WASM module from raw bytes.
    ///
    /// Parses and validates the given bytes as a WebAssembly module. The bytes
    /// must start with the WASM magic number (`\0asm`).
    ///
    /// # Arguments
    ///
    /// * `bytes` - Raw WASM binary (`.wasm` file contents).
    ///
    /// # Errors
    ///
    /// Returns [`Error::ModuleValidation`] if the bytes are not a valid WASM module.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::SandboxConfig;
    /// # fn example() -> isolate_core::Result<()> {
    /// let wasm = std::fs::read("module.wasm")?;
    /// let config = SandboxConfig::builder().module(&wasm)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn module(mut self, bytes: &[u8]) -> Result<Self> {
        self.module = Some(WasmModule::from_bytes(bytes.to_vec())?);
        Ok(self)
    }

    /// Set the WASM module directly from a pre-constructed [`WasmModule`].
    ///
    /// Unlike [`module()`](Self::module), this skips parsing and validation—
    /// use it when the module has already been validated elsewhere.
    ///
    /// # Arguments
    ///
    /// * `module` - A validated [`WasmModule`] instance.
    pub fn wasm_module(mut self, module: WasmModule) -> Self {
        self.module = Some(module);
        self
    }

    /// Grant a single capability to the sandbox.
    ///
    /// Capabilities control what operations the sandbox may perform. By default
    /// no capabilities are granted, so the sandbox runs fully isolated.
    ///
    /// # Arguments
    ///
    /// * `cap` - The [`Capability`] to grant (e.g., `Capability::stdout()`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::{SandboxConfig, capability::Capability};
    /// # fn example() -> isolate_core::Result<()> {
    /// # let wasm = vec![];
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .capability(Capability::stdout())
    ///     .capability(Capability::filesystem_read("/data"))
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn capability(mut self, cap: Capability) -> Self {
        self.capabilities.grant(cap);
        self
    }

    /// Grant multiple capabilities at once.
    ///
    /// Convenience method equivalent to calling [`capability()`](Self::capability)
    /// for each item in the iterator.
    ///
    /// # Arguments
    ///
    /// * `caps` - An iterator of [`Capability`] values to grant.
    pub fn capabilities(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        for cap in caps {
            self.capabilities.grant(cap);
        }
        self
    }

    /// Set the maximum heap memory the sandbox may allocate.
    ///
    /// Enforced via Wasmtime's `StoreLimits`. The sandbox will receive an
    /// out-of-memory trap if it exceeds this limit.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Maximum heap memory in bytes. Default: 64 MB (`67_108_864`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::SandboxConfig;
    /// # fn example() -> isolate_core::Result<()> {
    /// # let wasm = vec![];
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .memory_limit(128 * 1024 * 1024) // 128 MB
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.resources.memory.heap_max = bytes;
        self
    }

    /// Set the maximum WASM stack size.
    ///
    /// Controls the Wasmtime stack size for the sandbox's execution thread.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Maximum stack size in bytes. Default: 1 MB (`1_048_576`).
    pub fn stack_size(mut self, bytes: usize) -> Self {
        self.resources.memory.stack_max = bytes;
        self
    }

    /// Set the fuel limit for CPU metering.
    ///
    /// Fuel limits the number of WASM instructions the sandbox can execute.
    /// When fuel runs out, execution traps with [`Error::FuelExhausted`].
    ///
    /// # Arguments
    ///
    /// * `fuel` - Maximum fuel units to consume. Default: unlimited (`None`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::SandboxConfig;
    /// # fn example() -> isolate_core::Result<()> {
    /// # let wasm = vec![];
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .fuel(10_000_000) // ~10M instructions
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn fuel(mut self, fuel: u64) -> Self {
        self.resources.cpu.fuel = Some(fuel);
        self
    }

    /// Set the maximum CPU time the sandbox may consume.
    ///
    /// Tracks actual CPU time (not wall clock). When exceeded, execution is
    /// interrupted.
    ///
    /// # Arguments
    ///
    /// * `duration` - Maximum CPU time. Default: unlimited (`None`).
    pub fn cpu_time_limit(mut self, duration: Duration) -> Self {
        self.resources.time.cpu_time = Some(duration);
        self
    }

    /// Set the maximum wall-clock time for sandbox execution.
    ///
    /// Enforced via epoch-based interruption (10 ms tick interval). When the
    /// wall-clock deadline is reached, the WASM execution is interrupted with
    /// [`Error::Timeout`].
    ///
    /// # Arguments
    ///
    /// * `duration` - Maximum wall-clock time. Default: unlimited (`None`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use isolate_core::SandboxConfig;
    /// # use std::time::Duration;
    /// # fn example() -> isolate_core::Result<()> {
    /// # let wasm = vec![];
    /// let config = SandboxConfig::builder()
    ///     .module(&wasm)?
    ///     .wall_time_limit(Duration::from_secs(30))
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn wall_time_limit(mut self, duration: Duration) -> Self {
        self.resources.time.wall_time = Some(duration);
        self
    }

    /// Set the epoch tick interval for cooperative scheduling.
    ///
    /// Controls how often the runtime checks for timeouts and preemption.
    /// Shorter intervals provide more responsive timeout enforcement but add
    /// overhead.
    ///
    /// # Arguments
    ///
    /// * `duration` - Tick interval. Default: 10 ms.
    pub fn preemption_interval(mut self, duration: Duration) -> Self {
        self.resources.cpu.preemption_interval = duration;
        self
    }

    /// Set the maximum number of bytes the sandbox may read.
    ///
    /// Enforced in the metered I/O stream layer. Exceeding this limit will
    /// cause subsequent reads to fail.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Maximum read bytes. Default: unlimited (`None`).
    pub fn io_read_limit(mut self, bytes: u64) -> Self {
        self.resources.io.read_bytes = Some(bytes);
        self
    }

    /// Set the maximum number of bytes the sandbox may write.
    ///
    /// Enforced in the metered I/O stream layer. Exceeding this limit will
    /// cause subsequent writes to fail.
    ///
    /// # Arguments
    ///
    /// * `bytes` - Maximum write bytes. Default: unlimited (`None`).
    pub fn io_write_limit(mut self, bytes: u64) -> Self {
        self.resources.io.write_bytes = Some(bytes);
        self
    }

    /// Set a single environment variable visible to the sandbox.
    ///
    /// Requires the environment capability to be granted; otherwise the
    /// variable is set but WASI will not expose it.
    ///
    /// # Arguments
    ///
    /// * `key` - Environment variable name.
    /// * `value` - Environment variable value.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set multiple environment variables at once.
    ///
    /// Convenience method equivalent to calling [`env()`](Self::env) for each
    /// key-value pair.
    ///
    /// # Arguments
    ///
    /// * `vars` - Iterator of `(key, value)` pairs.
    pub fn envs(mut self, vars: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env.extend(vars);
        self
    }

    /// Append a command-line argument passed to the WASM module.
    ///
    /// Arguments are available to the module via WASI's `args_get`.
    ///
    /// # Arguments
    ///
    /// * `arg` - The argument string to append.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Replace all command-line arguments with the given values.
    ///
    /// Any previously added arguments are discarded.
    ///
    /// # Arguments
    ///
    /// * `args` - Iterator of argument strings.
    pub fn args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.args = args.into_iter().collect();
        self
    }

    /// Enable snapshot/restore support for this sandbox.
    ///
    /// When enabled, sandbox state can be snapshotted and restored later.
    /// An optional storage path controls where snapshot data is persisted;
    /// if `None`, an in-memory default is used.
    ///
    /// # Arguments
    ///
    /// * `storage_path` - Optional directory for snapshot storage. Default: `None` (in-memory).
    pub fn enable_snapshots(mut self, storage_path: Option<PathBuf>) -> Self {
        self.snapshot.enabled = true;
        self.snapshot.storage_path = storage_path;
        self
    }

    /// Set the WASM entry-point function name.
    ///
    /// The entry point is the exported function invoked by [`Sandbox::run()`].
    ///
    /// # Arguments
    ///
    /// * `name` - Exported function name. Default: `"_start"` (WASI convention).
    pub fn entry_point(mut self, name: impl Into<String>) -> Self {
        self.entry_point = name.into();
        self
    }

    /// Apply a language-specific optimization profile.
    ///
    /// Sets resource limits and default capabilities based on the language.
    /// Settings applied by the profile can be overridden by subsequent builder calls.
    pub fn apply_profile(mut self, profile: LanguageProfile) -> Self {
        self.resources = profile.resource_limits();
        for cap in profile.default_capabilities() {
            self.capabilities.grant(cap);
        }
        self
    }

    /// Apply a use-case-based sandbox profile.
    ///
    /// Sets resource limits and capabilities based on the workload type.
    /// Settings applied by the profile can be overridden by subsequent builder calls.
    pub fn use_profile(mut self, profile: SandboxProfile) -> Self {
        self.resources = profile.resource_limits();
        for cap in profile.capabilities() {
            self.capabilities.grant(cap);
        }
        self
    }

    /// Set the full rate-limiting configuration for this sandbox.
    ///
    /// For simple rate limiting, prefer [`max_requests_per_second()`](Self::max_requests_per_second).
    ///
    /// # Arguments
    ///
    /// * `config` - A [`RateLimitConfig`] with the desired rate-limiting parameters.
    pub fn rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.rate_limit = config;
        self
    }

    /// Set a maximum requests-per-second rate limit.
    ///
    /// If no burst size has been configured, it defaults to `rps` (i.e., burst
    /// size equals the sustained rate).
    ///
    /// # Arguments
    ///
    /// * `rps` - Maximum sustained requests per second. Default: unlimited.
    pub fn max_requests_per_second(mut self, rps: u32) -> Self {
        self.rate_limit.requests_per_second = Some(rps);
        if self.rate_limit.burst_size.is_none() {
            self.rate_limit.burst_size = Some(rps);
        }
        self
    }

    /// Build the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if no WASM module was provided.
    ///
    /// ```
    /// use isolate_core::SandboxConfig;
    ///
    /// // Fails without a module
    /// let result = SandboxConfig::builder().build();
    /// assert!(result.is_err());
    /// ```
    pub fn build(self) -> Result<SandboxConfig> {
        let module = self
            .module
            .ok_or_else(|| Error::InvalidConfig("WASM module is required".to_string()))?;

        Ok(SandboxConfig {
            module,
            capabilities: self.capabilities,
            resources: self.resources,
            env: self.env,
            args: self.args,
            snapshot: self.snapshot,
            entry_point: self.entry_point,
            rate_limit: self.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module (empty module)
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    #[test]
    fn test_wasm_module_from_bytes() {
        let module = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();
        assert_eq!(module.bytes().len(), 8);
    }

    #[test]
    fn test_wasm_module_invalid_magic() {
        let invalid = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        let result = WasmModule::from_bytes(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .memory_limit(128 * 1024 * 1024)
            .fuel(1_000_000)
            .env("KEY", "value")
            .arg("arg1".to_string())
            .capability(Capability::stdout())
            .build()
            .unwrap();

        assert_eq!(config.resources.memory.heap_max, 128 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(1_000_000));
        assert_eq!(config.env.get("KEY"), Some(&"value".to_string()));
        assert_eq!(config.args, vec!["arg1".to_string()]);
        assert!(config.capabilities.has(&Capability::stdout()));
    }

    #[test]
    fn test_config_builder_missing_module() {
        let result = SandboxConfig::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_module_hash() {
        let module1 = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();
        let module2 = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();
        assert_eq!(module1.hash(), module2.hash());
    }
}
