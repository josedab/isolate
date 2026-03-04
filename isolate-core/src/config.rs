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

        // Check WASM version: must be 1 (0x01 0x00 0x00 0x00)
        if bytes[4..8] != [0x01, 0x00, 0x00, 0x00] {
            return Err(Error::ModuleValidation(format!(
                "Unsupported WASM version: {:02x}{:02x}{:02x}{:02x} (only version 1 is supported)",
                bytes[4], bytes[5], bytes[6], bytes[7]
            )));
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
    /// User-defined metadata for tracking and correlation.
    pub metadata: HashMap<String, String>,
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

    /// Validate this config against a compiled module and return warnings.
    ///
    /// Checks that the config provides sufficient resources for the module's
    /// declared requirements (e.g., memory limits vs initial memory).
    pub fn validate_against_module(
        &self,
        module: &crate::engine::CompiledModule,
    ) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        // Check memory requirements
        if let Some(mem_req) = module.memory_requirements() {
            let configured_limit = self.resources.memory.heap_max as u64;
            if mem_req.initial_bytes > configured_limit {
                warnings.push(ConfigWarning {
                    kind: ConfigWarningKind::InsufficientMemory,
                    message: format!(
                        "Module requires at least {} bytes initial memory, but config limits to {} bytes",
                        mem_req.initial_bytes, configured_limit
                    ),
                    suggestion: format!(
                        "Increase memory_limit to at least {}",
                        mem_req.initial_bytes
                    ),
                });
            }
        }

        // Check that entry point exists
        if !module.has_export(&self.entry_point) {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::MissingEntryPoint,
                message: format!("Module does not export '{}' function", self.entry_point),
                suggestion:
                    "Check module exports with `isolate inspect` or set a different entry_point"
                        .to_string(),
            });
        }

        // Check WASI imports against granted capabilities
        let imports = module.required_imports();
        let wasi_imports: Vec<_> =
            imports.iter().filter(|i| i.module == "wasi_snapshot_preview1").collect();

        let has_stdio = self.capabilities.has_any(|c| matches!(c, Capability::Stdio(_)));
        let has_fs = self.capabilities.has_any(|c| matches!(c, Capability::Filesystem(_)));
        let has_env = self.capabilities.has_any(|c| matches!(c, Capability::Environment(_)));
        let has_time = self.capabilities.has_any(|c| matches!(c, Capability::Time(_)));
        let has_random = self.capabilities.has_any(|c| matches!(c, Capability::Random(_)));

        // Check for stdout/stderr writes without stdio capability
        let needs_stdio = wasi_imports.iter().any(|i| i.name == "fd_write");
        if needs_stdio && !has_stdio {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::InsufficientCapability,
                message: "Module imports 'fd_write' (WASI) but no Stdio capability is granted. \
                          Output will be silently discarded."
                    .to_string(),
                suggestion: "Add --cap-stdout or .capability(Capability::stdout())".to_string(),
            });
        }

        // Check for filesystem access without filesystem capability
        let needs_fs = wasi_imports.iter().any(|i| i.name == "path_open" || i.name == "fd_readdir");
        if needs_fs && !has_fs {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::InsufficientCapability,
                message: "Module imports filesystem functions (path_open/fd_readdir) but no \
                          Filesystem capability is granted."
                    .to_string(),
                suggestion:
                    "Add --cap-fs-read <path> or .capability(Capability::filesystem_read(path))"
                        .to_string(),
            });
        }

        // Check for environment access without environment capability
        let needs_env = wasi_imports.iter().any(|i| i.name == "environ_get");
        if needs_env && !has_env && self.env.is_empty() {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::InsufficientCapability,
                message: "Module imports 'environ_get' (WASI) but no Environment capability is \
                          granted and no env vars are set."
                    .to_string(),
                suggestion: "Add --env KEY=VALUE or .capability(Capability::env_read_all())"
                    .to_string(),
            });
        }

        // Check for clock access without time capability
        let needs_time = wasi_imports.iter().any(|i| i.name == "clock_time_get");
        if needs_time && !has_time {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::InsufficientCapability,
                message:
                    "Module imports 'clock_time_get' (WASI) but no Time capability is granted."
                        .to_string(),
                suggestion: "Add --cap-time or .capability(Capability::system_clock())".to_string(),
            });
        }

        // Check for random access without random capability
        let needs_random = wasi_imports.iter().any(|i| i.name == "random_get");
        if needs_random && !has_random {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::InsufficientCapability,
                message: "Module imports 'random_get' (WASI) but no Random capability is granted."
                    .to_string(),
                suggestion: "Add --cap-random or .capability(Capability::secure_random())"
                    .to_string(),
            });
        }

        warnings
    }
}

impl std::fmt::Display for SandboxConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SandboxConfig(module={}, entry={}, caps={}, {})",
            self.module.hash(),
            self.entry_point,
            self.capabilities.len(),
            self.resources,
        )
    }
}

/// A warning about potential misconfiguration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConfigWarning {
    /// Category of warning.
    pub kind: ConfigWarningKind,
    /// Human-readable description.
    pub message: String,
    /// Suggested fix.
    pub suggestion: String,
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}: {}", self.kind, self.message, self.suggestion)
    }
}

/// Category of configuration warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ConfigWarningKind {
    /// Memory limit is below module's initial requirements.
    InsufficientMemory,
    /// The configured entry point doesn't exist in the module.
    MissingEntryPoint,
    /// A capability may be insufficient for the module's imports.
    InsufficientCapability,
    /// Memory limit is suspiciously low (below 1 WASM page = 64KB).
    VeryLowMemory,
    /// Fuel limit is suspiciously low (may not complete basic initialization).
    VeryLowFuel,
    /// No capabilities granted — sandbox will have no I/O.
    NoCapabilities,
    /// No timeout configured — sandbox could run indefinitely.
    NoTimeout,
    /// Resource limits may conflict with each other.
    ResourceConflict,
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
    metadata: HashMap<String, String>,
}

impl SandboxConfigBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self { entry_point: "_start".to_string(), ..Default::default() }
    }

    /// Create a builder pre-populated from an existing [`SandboxConfig`].
    ///
    /// All fields are copied from the config. You can then override individual
    /// fields before calling [`build()`](Self::build).
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::{SandboxConfig, SandboxConfigBuilder, capability::Capability};
    ///
    /// let wasm = &[0x00u8, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    /// let base = SandboxConfig::builder()
    ///     .module(wasm).unwrap()
    ///     .fuel(1_000_000)
    ///     .capability(Capability::stdout())
    ///     .build()
    ///     .unwrap();
    ///
    /// // Clone config and override just the fuel
    /// let modified = SandboxConfigBuilder::from_config(base)
    ///     .fuel(2_000_000)
    ///     .build()
    ///     .unwrap();
    /// assert_eq!(modified.resources.cpu.fuel, Some(2_000_000));
    /// ```
    pub fn from_config(config: SandboxConfig) -> Self {
        Self {
            module: Some(config.module),
            capabilities: config.capabilities,
            resources: config.resources,
            env: config.env,
            args: config.args,
            snapshot: config.snapshot,
            entry_point: config.entry_point,
            rate_limit: config.rate_limit,
            metadata: config.metadata,
        }
    }

    /// Merge settings from another config into this builder.
    ///
    /// This applies the overlay's fields on top of the builder's current state:
    /// - **Capabilities**: Overlay capabilities are added (union, not replaced)
    /// - **Resources**: Overlay resource limits fully replace current limits
    /// - **Environment**: Overlay env vars are merged (overlay wins on conflict)
    /// - **Args**: Overlay args replace current args if non-empty
    /// - **Metadata**: Overlay metadata is merged (overlay wins on conflict)
    /// - **Entry point**: Overlay entry point replaces if different from `_start`
    /// - **Module**: Not changed (keep the builder's module)
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::{SandboxConfig, SandboxConfigBuilder, capability::Capability};
    /// use std::time::Duration;
    ///
    /// let wasm = &[0x00u8, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    ///
    /// let base = SandboxConfig::builder()
    ///     .module(wasm).unwrap()
    ///     .fuel(1_000_000)
    ///     .capability(Capability::stdout())
    ///     .env("MODE", "base")
    ///     .build().unwrap();
    ///
    /// let overlay = SandboxConfig::builder()
    ///     .module(wasm).unwrap()
    ///     .fuel(2_000_000)
    ///     .capability(Capability::stderr())
    ///     .env("MODE", "override")
    ///     .env("EXTRA", "value")
    ///     .build().unwrap();
    ///
    /// let merged = SandboxConfigBuilder::from_config(base)
    ///     .merge_from(&overlay)
    ///     .build().unwrap();
    ///
    /// // Fuel overridden
    /// assert_eq!(merged.resources.cpu.fuel, Some(2_000_000));
    /// // Capabilities are unioned
    /// assert!(merged.capabilities.len() >= 2);
    /// // Env merged with overlay winning
    /// assert_eq!(merged.env.get("MODE").unwrap(), "override");
    /// assert_eq!(merged.env.get("EXTRA").unwrap(), "value");
    /// ```
    pub fn merge_from(mut self, overlay: &SandboxConfig) -> Self {
        // Module: replace if overlay has a different module
        if self.module.as_ref().map(|m| m.hash()) != Some(overlay.module.hash()) {
            self.module = Some(overlay.module.clone());
        }

        // Capabilities: union
        for cap in overlay.capabilities.iter() {
            self.capabilities.grant(cap.clone());
        }

        // Resources: full replace from overlay
        self.resources = overlay.resources.clone();

        // Environment: merge (overlay wins)
        for (k, v) in &overlay.env {
            self.env.insert(k.clone(), v.clone());
        }

        // Args: replace if non-empty
        if !overlay.args.is_empty() {
            self.args = overlay.args.clone();
        }

        // Entry point: replace if non-default
        if overlay.entry_point != "_start" {
            self.entry_point = overlay.entry_point.clone();
        }

        // Rate limit: replace
        self.rate_limit = overlay.rate_limit.clone();

        // Metadata: merge (overlay wins)
        for (k, v) in &overlay.metadata {
            self.metadata.insert(k.clone(), v.clone());
        }

        self
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
    /// The entry point is the exported function invoked by [`Sandbox::run()`](crate::Sandbox::run).
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

    /// Add a single metadata key-value pair.
    ///
    /// Metadata is user-defined and carried through the sandbox lifecycle.
    /// Use it for tracking, correlation, or labeling sandboxes.
    ///
    /// # Arguments
    ///
    /// * `key` - Metadata key.
    /// * `value` - Metadata value.
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set multiple metadata key-value pairs at once.
    ///
    /// # Arguments
    ///
    /// * `pairs` - Iterator of `(key, value)` pairs.
    pub fn metadata(mut self, pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        self.metadata.extend(pairs);
        self
    }

    /// Check the current builder state for potential misconfigurations.
    ///
    /// Unlike [`build()`](Self::build) which returns hard errors for invalid
    /// configs, this method returns warnings for configurations that are valid
    /// but likely to cause unexpected behavior.
    ///
    /// Call this before `build()` to surface issues early.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::SandboxConfig;
    /// use isolate_core::config::ConfigWarningKind;
    ///
    /// let wasm = &[0x00u8, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    ///
    /// let builder = SandboxConfig::builder()
    ///     .module(wasm).unwrap()
    ///     .memory_limit(1024)  // Very small
    ///     .fuel(10);           // Very low fuel
    ///
    /// let warnings = builder.validate_warnings();
    /// assert!(warnings.iter().any(|w| w.kind == ConfigWarningKind::VeryLowMemory));
    /// assert!(warnings.iter().any(|w| w.kind == ConfigWarningKind::VeryLowFuel));
    /// ```
    pub fn validate_warnings(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        // One WASM page = 64KB; anything below that can't even hold a single page
        const WASM_PAGE_SIZE: usize = 65_536;
        if self.resources.memory.heap_max > 0 && self.resources.memory.heap_max < WASM_PAGE_SIZE {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::VeryLowMemory,
                message: format!(
                    "Memory limit ({} bytes) is below one WASM page (64KB). Most modules need at least one page.",
                    self.resources.memory.heap_max,
                ),
                suggestion: "Set memory_limit to at least 65536 (64KB), or higher for real workloads".to_string(),
            });
        }

        // Fuel below 1000 likely can't even complete WASI initialization
        if let Some(fuel) = self.resources.cpu.fuel {
            if fuel < 1_000 {
                warnings.push(ConfigWarning {
                    kind: ConfigWarningKind::VeryLowFuel,
                    message: format!(
                        "Fuel limit ({}) is very low. Most modules consume >1000 fuel during WASI initialization alone.",
                        fuel,
                    ),
                    suggestion: "Increase fuel to at least 10_000 for basic modules, or 1_000_000+ for real workloads".to_string(),
                });
            }
        }

        // No capabilities means no I/O at all
        if self.capabilities.is_empty() {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::NoCapabilities,
                message: "No capabilities granted. The sandbox will have no I/O access (no stdout, no filesystem, no network).".to_string(),
                suggestion: "Add at least .capability(Capability::stdout()) for basic output".to_string(),
            });
        }

        // No timeout or fuel means the sandbox could run forever
        if self.resources.time.wall_time.is_none() && self.resources.cpu.fuel.is_none() {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::NoTimeout,
                message: "No wall_time_limit or fuel limit configured. A malicious or buggy module could run indefinitely.".to_string(),
                suggestion: "Set wall_time_limit or fuel to bound execution time".to_string(),
            });
        }

        // Check resource limit consistency
        let resource_errors = self.resources.validate();
        for err in resource_errors {
            warnings.push(ConfigWarning {
                kind: ConfigWarningKind::ResourceConflict,
                message: err,
                suggestion: "Review resource limits for internal consistency".to_string(),
            });
        }

        warnings
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
        // Emit warnings for likely misconfigurations (before we move fields)
        let warnings = self.validate_warnings();
        for warning in &warnings {
            tracing::warn!(
                kind = ?warning.kind,
                suggestion = %warning.suggestion,
                "Config warning: {}",
                warning.message
            );
        }

        let module = self
            .module
            .ok_or_else(|| Error::InvalidConfig("WASM module is required".to_string()))?;

        // Validate memory limit is non-zero
        if self.resources.memory.heap_max == 0 {
            return Err(Error::InvalidConfig(
                "memory_limit must be greater than 0 (use at least 65536 for one WASM page)"
                    .to_string(),
            ));
        }

        // Validate stack size is non-zero
        if self.resources.memory.stack_max == 0 {
            return Err(Error::InvalidConfig("stack_size must be greater than 0".to_string()));
        }

        // Validate fuel is non-zero when explicitly set
        if self.resources.cpu.fuel == Some(0) {
            return Err(Error::InvalidConfig(
                "fuel must be greater than 0 when set (omit to leave unlimited)".to_string(),
            ));
        }

        // Validate entry point is non-empty
        if self.entry_point.trim().is_empty() {
            return Err(Error::InvalidConfig(
                "entry_point must not be empty or whitespace-only (default is '_start')"
                    .to_string(),
            ));
        }

        Ok(SandboxConfig {
            module,
            capabilities: self.capabilities,
            resources: self.resources,
            env: self.env,
            args: self.args,
            snapshot: self.snapshot,
            entry_point: self.entry_point,
            rate_limit: self.rate_limit,
            metadata: self.metadata,
        })
    }

    /// Apply settings from a [`ConfigFile`] to this builder.
    ///
    /// Loads capabilities, resource limits, environment variables, args, and
    /// entry point from the parsed config. The WASM module must still be set
    /// separately via [`module()`](Self::module).
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::{SandboxConfig, config::ConfigFile};
    ///
    /// let json = r#"{
    ///     "capabilities": { "stdout": true, "stderr": true },
    ///     "resources": { "memory": { "heap_max": "64MB" }, "timeout": "30s" }
    /// }"#;
    ///
    /// let config_file = ConfigFile::from_json(json).unwrap();
    /// let builder = SandboxConfig::builder()
    ///     .apply_config_file(&config_file)
    ///     .unwrap();
    /// ```
    pub fn apply_config_file(
        mut self,
        cfg: &ConfigFile,
    ) -> std::result::Result<Self, ConfigFileError> {
        // Capabilities
        for cap in cfg.to_capabilities() {
            self = self.capability(cap);
        }

        // Resources
        if let Some(ref mem) = cfg.resources.memory {
            if let Some(ref s) = mem.heap_max {
                self = self.memory_limit(parse_size(s)?);
            }
            if let Some(ref s) = mem.stack_max {
                self = self.stack_size(parse_size(s)?);
            }
        }
        if let Some(ref cpu) = cfg.resources.cpu {
            if let Some(fuel) = cpu.fuel {
                self = self.fuel(fuel);
            }
            if let Some(ref s) = cpu.time_limit {
                self = self.cpu_time_limit(parse_duration(s)?);
            }
        }
        if let Some(ref io) = cfg.resources.io {
            if let Some(ref s) = io.read_limit {
                self = self.io_read_limit(parse_size(s)? as u64);
            }
            if let Some(ref s) = io.write_limit {
                self = self.io_write_limit(parse_size(s)? as u64);
            }
        }
        if let Some(ref t) = cfg.resources.timeout {
            self = self.wall_time_limit(parse_duration(t)?);
        }

        // Environment
        for (k, v) in &cfg.environment {
            self = self.env(k, v);
        }

        // Args
        for a in &cfg.args {
            self = self.arg(a);
        }

        // Entry point
        if let Some(ref ep) = cfg.entry_point {
            self = self.entry_point(ep);
        }

        Ok(self)
    }
}

// ---------------------------------------------------------------------------
// Config file format (always available, no feature flag required)
// ---------------------------------------------------------------------------

/// Error from config file parsing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigFileError {
    /// JSON parse error.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    /// YAML parse error.
    #[error("YAML parse error: {0}")]
    Yaml(String),
    /// Invalid size string (e.g. "128XB").
    #[error("Invalid size value: {0}")]
    InvalidSize(String),
    /// Invalid duration string (e.g. "30x").
    #[error("Invalid duration value: {0}")]
    InvalidDuration(String),
    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),
}

/// A sandbox configuration file that can be loaded from JSON.
///
/// This provides a declarative way to configure sandboxes without code.
/// Unlike the `policy-engine` feature's `PolicyFile`, this struct is always
/// available and focuses on the core configuration surface.
///
/// # JSON Format
///
/// ```json
/// {
///   "capabilities": {
///     "stdout": true,
///     "stderr": true,
///     "filesystem": { "read": ["/data"], "write": ["/tmp"] },
///     "network": { "http_hosts": ["api.example.com"] }
///   },
///   "resources": {
///     "memory": { "heap_max": "128MB", "stack_max": "1MB" },
///     "cpu": { "fuel": 10000000, "time_limit": "30s" },
///     "io": { "read_limit": "10MB", "write_limit": "1MB" },
///     "timeout": "60s"
///   },
///   "environment": { "LOG_LEVEL": "info" },
///   "args": ["--verbose"],
///   "entry_point": "_start"
/// }
/// ```
///
/// # Examples
///
/// ```
/// use isolate_core::config::ConfigFile;
///
/// let cfg = ConfigFile::from_json(r#"{ "capabilities": { "stdout": true } }"#).unwrap();
/// assert_eq!(cfg.to_capabilities().len(), 1);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    /// Capability grants.
    #[serde(default)]
    pub capabilities: CfgCapabilities,
    /// Resource limits.
    #[serde(default)]
    pub resources: CfgResources,
    /// Environment variables to inject.
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// WASM entry point function name.
    #[serde(default)]
    pub entry_point: Option<String>,
}

/// Capability section of a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgCapabilities {
    /// Grant stdout.
    #[serde(default)]
    pub stdout: Option<bool>,
    /// Grant stderr.
    #[serde(default)]
    pub stderr: Option<bool>,
    /// Grant stdin.
    #[serde(default)]
    pub stdin: Option<bool>,
    /// Filesystem access.
    #[serde(default)]
    pub filesystem: Option<CfgFilesystem>,
    /// Network access.
    #[serde(default)]
    pub network: Option<CfgNetwork>,
    /// Environment variable access.
    #[serde(default)]
    pub env_access: Option<CfgEnvAccess>,
    /// Time capabilities.
    #[serde(default)]
    pub time: Option<CfgTime>,
    /// Random number generation.
    #[serde(default)]
    pub random: Option<CfgRandom>,
}

/// Filesystem access in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgFilesystem {
    /// Read-only paths.
    #[serde(default)]
    pub read: Vec<String>,
    /// Read-write paths.
    #[serde(default)]
    pub write: Vec<String>,
    /// Grant temp directory access.
    #[serde(default)]
    pub temp_dir: Option<bool>,
}

/// Network access in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgNetwork {
    /// Allowed HTTP hosts.
    #[serde(default)]
    pub http_hosts: Vec<String>,
    /// Allow DNS resolution.
    #[serde(default)]
    pub dns: Option<bool>,
}

/// Environment variable access in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgEnvAccess {
    /// Specific allowed variable names.
    #[serde(default)]
    pub allowed_vars: Vec<String>,
    /// Allow all environment variables.
    #[serde(default)]
    pub all: Option<bool>,
    /// Allow command-line args access.
    #[serde(default)]
    pub args_access: Option<bool>,
}

/// Time capabilities in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgTime {
    /// Allow system clock.
    #[serde(default)]
    pub system_clock: Option<bool>,
    /// Allow monotonic clock.
    #[serde(default)]
    pub monotonic_clock: Option<bool>,
    /// Allow timers/sleeps.
    #[serde(default)]
    pub timers: Option<bool>,
}

/// Random number generation in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgRandom {
    /// Allow cryptographic random.
    #[serde(default)]
    pub secure: Option<bool>,
}

/// Resource limits section of a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgResources {
    /// Memory limits.
    #[serde(default)]
    pub memory: Option<CfgMemory>,
    /// CPU limits.
    #[serde(default)]
    pub cpu: Option<CfgCpu>,
    /// I/O limits.
    #[serde(default)]
    pub io: Option<CfgIo>,
    /// Wall-clock timeout (e.g. "60s", "5m").
    #[serde(default)]
    pub timeout: Option<String>,
}

/// Memory limits in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgMemory {
    /// e.g. "128MB", "1GB".
    pub heap_max: Option<String>,
    /// e.g. "1MB".
    pub stack_max: Option<String>,
}

/// CPU limits in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgCpu {
    /// Fuel units.
    pub fuel: Option<u64>,
    /// CPU time limit (e.g. "30s").
    pub time_limit: Option<String>,
}

/// I/O limits in a config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CfgIo {
    /// Read limit (e.g. "10MB").
    pub read_limit: Option<String>,
    /// Write limit (e.g. "1MB").
    pub write_limit: Option<String>,
}

impl ConfigFile {
    /// Parse from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::Json`] if the string is not valid JSON.
    pub fn from_json(json: &str) -> std::result::Result<Self, ConfigFileError> {
        Ok(serde_json::from_str(json)?)
    }

    /// Parse from a YAML string.
    ///
    /// When compiled with a `platform-*` or similar feature that enables
    /// `serde_yaml`, this uses full YAML parsing. Otherwise it falls back
    /// to attempting JSON parse (since JSON is valid YAML).
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::Yaml`] if the string is not valid YAML.
    pub fn from_yaml(yaml: &str) -> std::result::Result<Self, ConfigFileError> {
        #[cfg(feature = "platform-admin")]
        {
            serde_yaml::from_str(yaml).map_err(|e| ConfigFileError::Yaml(e.to_string()))
        }
        #[cfg(not(feature = "platform-admin"))]
        {
            // Fall back to JSON parsing (JSON is a subset of YAML)
            serde_json::from_str(yaml).map_err(|e| ConfigFileError::Yaml(e.to_string()))
        }
    }

    /// Load from a file path, auto-detecting format from the extension.
    ///
    /// Files ending in `.yaml` or `.yml` are parsed as YAML; all others as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigFileError::Io`] if the file cannot be read, or a
    /// parse error if the contents are invalid.
    pub fn from_file(
        path: impl AsRef<std::path::Path>,
    ) -> std::result::Result<Self, ConfigFileError> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)?;

        match path.extension().and_then(|e| e.to_str()) {
            Some("yaml" | "yml") => Self::from_yaml(&contents),
            _ => Self::from_json(&contents),
        }
    }

    /// Validate that all resource values are parseable.
    ///
    /// Call this after loading to catch invalid size/duration strings early.
    pub fn validate(&self) -> std::result::Result<(), ConfigFileError> {
        if let Some(ref mem) = self.resources.memory {
            if let Some(ref s) = mem.heap_max {
                parse_size(s)?;
            }
            if let Some(ref s) = mem.stack_max {
                parse_size(s)?;
            }
        }
        if let Some(ref cpu) = self.resources.cpu {
            if let Some(ref s) = cpu.time_limit {
                parse_duration(s)?;
            }
        }
        if let Some(ref io) = self.resources.io {
            if let Some(ref s) = io.read_limit {
                parse_size(s)?;
            }
            if let Some(ref s) = io.write_limit {
                parse_size(s)?;
            }
        }
        if let Some(ref t) = self.resources.timeout {
            parse_duration(t)?;
        }
        Ok(())
    }

    /// Serialize this config to a pretty-printed JSON string.
    pub fn to_json(&self) -> std::result::Result<String, ConfigFileError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Convert the capabilities section into a list of [`Capability`] values.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();

        if self.capabilities.stdout == Some(true) {
            caps.push(Capability::stdout());
        }
        if self.capabilities.stderr == Some(true) {
            caps.push(Capability::stderr());
        }
        if self.capabilities.stdin == Some(true) {
            caps.push(Capability::stdin());
        }

        if let Some(ref fs) = self.capabilities.filesystem {
            for path in &fs.read {
                caps.push(Capability::filesystem_read(path));
            }
            for path in &fs.write {
                caps.push(Capability::filesystem_write(path));
            }
            if fs.temp_dir == Some(true) {
                caps.push(Capability::temp_dir());
            }
        }

        if let Some(ref net) = self.capabilities.network {
            if !net.http_hosts.is_empty() {
                caps.push(Capability::http_client(net.http_hosts.clone()));
            }
            if net.dns == Some(true) {
                caps.push(Capability::dns_resolve());
            }
        }

        if let Some(ref env) = self.capabilities.env_access {
            if env.all == Some(true) {
                caps.push(Capability::env_all());
            } else {
                for var in &env.allowed_vars {
                    caps.push(Capability::env_var(var));
                }
            }
            if env.args_access == Some(true) {
                caps.push(Capability::args());
            }
        }

        if let Some(ref time) = self.capabilities.time {
            if time.system_clock == Some(true) {
                caps.push(Capability::system_clock());
            }
            if time.monotonic_clock == Some(true) {
                caps.push(Capability::monotonic_clock());
            }
            if time.timers == Some(true) {
                caps.push(Capability::timers());
            }
        }

        if let Some(ref rng) = self.capabilities.random {
            if rng.secure == Some(true) {
                caps.push(Capability::secure_random());
            }
        }

        caps
    }
}

/// Parse a human-readable size string to bytes.
///
/// Supports `KB`, `MB`, `GB` suffixes (case-insensitive) and raw byte counts.
///
/// # Examples
///
/// ```
/// use isolate_core::config::parse_size;
///
/// assert_eq!(parse_size("128MB").unwrap(), 128 * 1024 * 1024);
/// assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
/// assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
/// assert_eq!(parse_size("1024").unwrap(), 1024);
/// ```
pub fn parse_size(s: &str) -> std::result::Result<usize, ConfigFileError> {
    let s = s.trim();
    let (num_str, multiplier) = if s.ends_with("GB") || s.ends_with("gb") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") || s.ends_with("mb") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") || s.ends_with("kb") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') || s.ends_with('b') {
        (&s[..s.len() - 1], 1)
    } else {
        (s, 1)
    };
    let num: usize =
        num_str.trim().parse().map_err(|_| ConfigFileError::InvalidSize(s.to_string()))?;
    num.checked_mul(multiplier).ok_or_else(|| {
        ConfigFileError::InvalidSize(format!("{s} exceeds maximum representable size"))
    })
}

/// Parse a human-readable duration string.
///
/// Supports `ms`, `s`, `m`, `h` suffixes. Bare numbers are treated as seconds.
///
/// # Examples
///
/// ```
/// use isolate_core::config::parse_duration;
/// use std::time::Duration;
///
/// assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
/// assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
/// assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
/// ```
pub fn parse_duration(s: &str) -> std::result::Result<Duration, ConfigFileError> {
    let s = s.trim();
    let (num_str, factor) = if let Some(stripped) = s.strip_suffix("ms") {
        (stripped, 1u64)
    } else if let Some(stripped) = s.strip_suffix('s') {
        (stripped, 1000)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 60 * 1000)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3600 * 1000)
    } else {
        (s, 1000) // Assume seconds
    };
    let num: u64 =
        num_str.trim().parse().map_err(|_| ConfigFileError::InvalidDuration(s.to_string()))?;
    let millis = num.checked_mul(factor).ok_or_else(|| {
        ConfigFileError::InvalidDuration(format!("{s} exceeds maximum representable duration"))
    })?;
    Ok(Duration::from_millis(millis))
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
    fn test_wasm_module_invalid_version() {
        // Valid magic but version 2 (doesn't exist yet)
        let invalid = vec![0x00, 0x61, 0x73, 0x6d, 0x02, 0x00, 0x00, 0x00];
        let result = WasmModule::from_bytes(invalid);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unsupported WASM version"), "got: {}", msg);
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

    #[test]
    fn test_config_builder_all_resource_limits() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .memory_limit(32 * 1024 * 1024)
            .fuel(500_000)
            .wall_time_limit(Duration::from_secs(5))
            .io_read_limit(1024)
            .io_write_limit(2048)
            .preemption_interval(Duration::from_millis(5))
            .build()
            .unwrap();

        assert_eq!(config.resources.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(500_000));
        assert_eq!(config.resources.time.wall_time, Some(Duration::from_secs(5)));
        assert_eq!(config.resources.io.read_bytes, Some(1024));
        assert_eq!(config.resources.io.write_bytes, Some(2048));
    }

    #[test]
    fn test_config_builder_multiple_envs() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .env("A", "1")
            .env("B", "2")
            .envs(vec![("C".to_string(), "3".to_string())])
            .build()
            .unwrap();

        assert_eq!(config.env.get("A"), Some(&"1".to_string()));
        assert_eq!(config.env.get("B"), Some(&"2".to_string()));
        assert_eq!(config.env.get("C"), Some(&"3".to_string()));
    }

    #[test]
    fn test_config_builder_multiple_args() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .arg("arg1".to_string())
            .arg("arg2".to_string())
            .build()
            .unwrap();

        assert_eq!(config.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_config_builder_args_replace() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .arg("old".to_string())
            .args(vec!["new1".to_string(), "new2".to_string()])
            .build()
            .unwrap();

        assert_eq!(config.args, vec!["new1", "new2"]);
    }

    #[test]
    fn test_config_builder_duplicate_capabilities() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::stdout())
            .capability(Capability::stdout()) // duplicate
            .capability(Capability::stderr())
            .build()
            .unwrap();

        assert!(config.capabilities.has(&Capability::stdout()));
        assert!(config.capabilities.has(&Capability::stderr()));
    }

    #[test]
    fn test_config_builder_entry_point() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .entry_point("custom_main")
            .build()
            .unwrap();

        assert_eq!(config.entry_point, "custom_main");
    }

    #[test]
    fn test_wasm_module_too_short() {
        let short = vec![0x00, 0x61, 0x73];
        let result = WasmModule::from_bytes(short);
        assert!(result.is_err());
    }

    #[test]
    fn test_wasm_module_empty() {
        let result = WasmModule::from_bytes(vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_file_from_json() {
        let json = r#"{
            "capabilities": {
                "stdout": true,
                "stderr": true,
                "filesystem": { "read": ["/data"], "write": ["/tmp"], "temp_dir": true },
                "network": { "http_hosts": ["api.example.com"], "dns": true }
            },
            "resources": {
                "memory": { "heap_max": "128MB", "stack_max": "1MB" },
                "cpu": { "fuel": 5000000, "time_limit": "10s" },
                "io": { "read_limit": "10MB", "write_limit": "1MB" },
                "timeout": "30s"
            },
            "environment": { "LOG_LEVEL": "info" },
            "args": ["--verbose"],
            "entry_point": "main"
        }"#;

        let cfg = ConfigFile::from_json(json).unwrap();
        let caps = cfg.to_capabilities();

        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::temp_dir()));
        assert!(caps.contains(&Capability::dns_resolve()));
        assert_eq!(cfg.environment.get("LOG_LEVEL").unwrap(), "info");
        assert_eq!(cfg.args, vec!["--verbose"]);
        assert_eq!(cfg.entry_point, Some("main".to_string()));
    }

    #[test]
    fn test_config_file_apply_to_builder() {
        let json = r#"{
            "capabilities": { "stdout": true },
            "resources": {
                "memory": { "heap_max": "64MB" },
                "cpu": { "fuel": 1000000 },
                "timeout": "10s"
            },
            "environment": { "KEY": "value" }
        }"#;

        let cfg = ConfigFile::from_json(json).unwrap();
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .apply_config_file(&cfg)
            .unwrap()
            .build()
            .unwrap();

        assert!(config.capabilities.has(&Capability::stdout()));
        assert_eq!(config.resources.memory.heap_max, 64 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(1_000_000));
        assert_eq!(config.resources.time.wall_time, Some(Duration::from_secs(10)));
        assert_eq!(config.env.get("KEY").unwrap(), "value");
    }

    #[test]
    fn test_config_file_empty() {
        let cfg = ConfigFile::from_json("{}").unwrap();
        assert!(cfg.to_capabilities().is_empty());
        assert!(cfg.environment.is_empty());
        assert!(cfg.entry_point.is_none());
    }

    #[test]
    fn test_parse_size_variants() {
        assert_eq!(parse_size("128MB").unwrap(), 128 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("2048").unwrap(), 2048);
        assert!(parse_size("invalid").is_err());
    }

    #[test]
    fn test_parse_size_overflow() {
        // On 64-bit systems usize is 64 bits, so we need truly huge values
        let huge = format!("{}GB", usize::MAX);
        assert!(parse_size(&huge).is_err(), "should reject overflow");
        // Zero is fine
        assert_eq!(parse_size("0GB").unwrap(), 0);
    }

    #[test]
    fn test_parse_duration_variants() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("100ms").unwrap(), Duration::from_millis(100));
        assert_eq!(parse_duration("10").unwrap(), Duration::from_secs(10));
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_parse_duration_overflow() {
        let huge = format!("{}h", u64::MAX);
        assert!(parse_duration(&huge).is_err(), "should reject overflow");
    }

    #[test]
    fn test_config_file_from_yaml_json_fallback() {
        // JSON is valid YAML, so from_yaml should work on JSON strings
        let json = r#"{ "capabilities": { "stdout": true } }"#;
        let cfg = ConfigFile::from_yaml(json).unwrap();
        assert_eq!(cfg.to_capabilities().len(), 1);
    }

    #[test]
    fn test_config_file_validate_valid() {
        let cfg = ConfigFile::from_json(
            r#"{ "resources": { "memory": { "heap_max": "128MB" }, "timeout": "30s" } }"#,
        )
        .unwrap();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_config_file_validate_invalid_size() {
        let cfg =
            ConfigFile::from_json(r#"{ "resources": { "memory": { "heap_max": "notasize" } } }"#)
                .unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_file_validate_invalid_duration() {
        let cfg = ConfigFile::from_json(r#"{ "resources": { "timeout": "xyz" } }"#).unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_file_to_json_roundtrip() {
        let json = r#"{ "capabilities": { "stdout": true }, "args": ["--verbose"] }"#;
        let cfg = ConfigFile::from_json(json).unwrap();
        let serialized = cfg.to_json().unwrap();
        let cfg2 = ConfigFile::from_json(&serialized).unwrap();
        assert_eq!(cfg.to_capabilities().len(), cfg2.to_capabilities().len());
        assert_eq!(cfg.args, cfg2.args);
    }

    #[test]
    fn test_config_file_from_file_auto_detect() {
        // Create a temp JSON file
        let dir = tempfile::tempdir().unwrap();
        let json_path = dir.path().join("config.json");
        std::fs::write(&json_path, r#"{ "capabilities": { "stdout": true } }"#).unwrap();

        let cfg = ConfigFile::from_file(&json_path).unwrap();
        assert_eq!(cfg.to_capabilities().len(), 1);

        // Create a YAML file with JSON content (valid YAML subset)
        let yaml_path = dir.path().join("config.yaml");
        std::fs::write(&yaml_path, r#"{ "capabilities": { "stderr": true } }"#).unwrap();

        let cfg2 = ConfigFile::from_file(&yaml_path).unwrap();
        assert_eq!(cfg2.to_capabilities().len(), 1);
    }

    #[test]
    fn test_config_builder_rejects_zero_memory_limit() {
        let result = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().memory_limit(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
        assert!(err.to_string().contains("memory_limit"));
    }

    #[test]
    fn test_config_builder_rejects_zero_stack_size() {
        let result = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().stack_size(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
        assert!(err.to_string().contains("stack_size"));
    }

    #[test]
    fn test_config_builder_rejects_zero_fuel() {
        let result = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().fuel(0).build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
        assert!(err.to_string().contains("fuel"));
    }

    #[test]
    fn test_config_builder_rejects_empty_entry_point() {
        let result = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().entry_point("").build();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::InvalidConfig(_)));
        assert!(err.to_string().contains("entry_point"));
    }

    #[test]
    fn test_config_builder_accepts_valid_limits() {
        // Ensure valid non-zero limits still work fine
        let result = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .memory_limit(65536) // One WASM page
            .stack_size(4096)
            .fuel(1)
            .entry_point("_start")
            .build();
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_builder_default_fuel_is_nonzero() {
        // Not setting fuel explicitly uses the default (10M instructions)
        let config = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();
        assert!(config.resources.cpu.fuel.is_some());
        assert!(config.resources.cpu.fuel.unwrap() > 0);
    }

    #[test]
    fn test_validate_warnings_very_low_memory() {
        let builder = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().memory_limit(1024); // Way below 64KB page size

        let warnings = builder.validate_warnings();
        assert!(
            warnings.iter().any(|w| w.kind == ConfigWarningKind::VeryLowMemory),
            "Expected VeryLowMemory warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_validate_warnings_very_low_fuel() {
        let builder = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().fuel(10);

        let warnings = builder.validate_warnings();
        assert!(
            warnings.iter().any(|w| w.kind == ConfigWarningKind::VeryLowFuel),
            "Expected VeryLowFuel warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_validate_warnings_no_capabilities() {
        let builder = SandboxConfig::builder().module(MINIMAL_WASM).unwrap();

        let warnings = builder.validate_warnings();
        assert!(
            warnings.iter().any(|w| w.kind == ConfigWarningKind::NoCapabilities),
            "Expected NoCapabilities warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_validate_warnings_no_timeout() {
        use crate::resource::{CpuLimits, TimeLimits};
        // Explicitly set unlimited time and no fuel to trigger warning
        let mut builder =
            SandboxConfig::builder().module(MINIMAL_WASM).unwrap().capability(Capability::stdout());
        builder.resources.time = TimeLimits::unlimited();
        builder.resources.cpu = CpuLimits { fuel: None, cpu_time: None, ..Default::default() };

        let warnings = builder.validate_warnings();
        assert!(
            warnings.iter().any(|w| w.kind == ConfigWarningKind::NoTimeout),
            "Expected NoTimeout warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_validate_warnings_clean_config() {
        use std::time::Duration;
        let builder = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .memory_limit(64 * 1024 * 1024) // 64MB
            .fuel(1_000_000)
            .wall_time_limit(Duration::from_secs(30))
            .capability(Capability::stdout());

        let warnings = builder.validate_warnings();
        assert!(
            warnings.is_empty(),
            "Expected no warnings for a well-configured builder, got: {:?}",
            warnings
        );
    }

    #[test]
    fn test_config_warning_display() {
        let w = ConfigWarning {
            kind: ConfigWarningKind::VeryLowMemory,
            message: "Memory too low".to_string(),
            suggestion: "Increase it".to_string(),
        };
        let display = format!("{}", w);
        assert!(display.contains("Memory too low"));
        assert!(display.contains("Increase it"));
    }

    #[test]
    fn test_metadata_label() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .label("team", "platform")
            .label("env", "staging")
            .build()
            .unwrap();

        assert_eq!(config.metadata.get("team").unwrap(), "platform");
        assert_eq!(config.metadata.get("env").unwrap(), "staging");
    }

    #[test]
    fn test_metadata_bulk() {
        let labels = vec![
            ("region".to_string(), "us-east-1".to_string()),
            ("version".to_string(), "1.2.3".to_string()),
        ];

        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .metadata(labels)
            .build()
            .unwrap();

        assert_eq!(config.metadata.len(), 2);
        assert_eq!(config.metadata.get("region").unwrap(), "us-east-1");
    }

    #[test]
    fn test_metadata_empty_default() {
        let config = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();

        assert!(config.metadata.is_empty());
    }

    #[test]
    fn test_from_config() {
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .fuel(500_000)
            .memory_limit(32 * 1024 * 1024)
            .capability(Capability::stdout())
            .env("KEY", "val")
            .label("team", "core")
            .build()
            .unwrap();

        let rebuilt = SandboxConfigBuilder::from_config(config.clone())
            .fuel(1_000_000) // override fuel
            .build()
            .unwrap();

        assert_eq!(rebuilt.resources.cpu.fuel, Some(1_000_000));
        assert_eq!(rebuilt.resources.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(rebuilt.env.get("KEY").unwrap(), "val");
        assert_eq!(rebuilt.metadata.get("team").unwrap(), "core");
    }

    #[test]
    fn test_merge_from_capabilities_union() {
        let base = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::stdout())
            .build()
            .unwrap();

        let overlay = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::stderr())
            .build()
            .unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        // Both capabilities should be present
        assert!(merged.capabilities.has(&Capability::stdout()));
        assert!(merged.capabilities.has(&Capability::stderr()));
    }

    #[test]
    fn test_merge_from_env_overlay_wins() {
        let base = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .env("SHARED", "base_value")
            .env("BASE_ONLY", "keep")
            .build()
            .unwrap();

        let overlay = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .env("SHARED", "overlay_value")
            .env("NEW", "added")
            .build()
            .unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        assert_eq!(merged.env.get("SHARED").unwrap(), "overlay_value");
        assert_eq!(merged.env.get("BASE_ONLY").unwrap(), "keep");
        assert_eq!(merged.env.get("NEW").unwrap(), "added");
    }

    #[test]
    fn test_merge_from_resources_replaced() {
        let base = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .fuel(1_000_000)
            .memory_limit(64 * 1024 * 1024)
            .build()
            .unwrap();

        let overlay = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .fuel(2_000_000)
            .memory_limit(128 * 1024 * 1024)
            .build()
            .unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        assert_eq!(merged.resources.cpu.fuel, Some(2_000_000));
        assert_eq!(merged.resources.memory.heap_max, 128 * 1024 * 1024);
    }

    #[test]
    fn test_merge_from_metadata() {
        let base = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .label("team", "base")
            .label("env", "staging")
            .build()
            .unwrap();

        let overlay = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .label("team", "overlay")
            .label("region", "us-east")
            .build()
            .unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        assert_eq!(merged.metadata.get("team").unwrap(), "overlay");
        assert_eq!(merged.metadata.get("env").unwrap(), "staging");
        assert_eq!(merged.metadata.get("region").unwrap(), "us-east");
    }

    #[test]
    fn test_merge_from_entry_point() {
        let base = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();

        let overlay = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .entry_point("main")
            .build()
            .unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        assert_eq!(merged.entry_point, "main");
    }

    #[test]
    fn test_merge_from_keeps_default_entry_point() {
        let base = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .entry_point("custom_main")
            .build()
            .unwrap();

        let overlay = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();

        let merged = SandboxConfigBuilder::from_config(base).merge_from(&overlay).build().unwrap();

        // Overlay has default _start, so base's custom_main is preserved
        assert_eq!(merged.entry_point, "custom_main");
    }
}
