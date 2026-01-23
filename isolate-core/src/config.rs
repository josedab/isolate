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
use crate::resource::ResourceLimits;
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
            return Err(Error::ModuleValidation(
                "Invalid WASM magic number".to_string(),
            ));
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
        Self {
            enabled: false,
            storage_path: None,
            max_snapshots: 10,
        }
    }
}

/// Configuration for creating a sandbox.
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
}

impl SandboxConfigBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            entry_point: "_start".to_string(),
            ..Default::default()
        }
    }

    /// Set the WASM module from bytes.
    pub fn module(mut self, bytes: &[u8]) -> Result<Self> {
        self.module = Some(WasmModule::from_bytes(bytes.to_vec())?);
        Ok(self)
    }

    /// Set the WASM module directly.
    pub fn wasm_module(mut self, module: WasmModule) -> Self {
        self.module = Some(module);
        self
    }

    /// Add a capability.
    pub fn capability(mut self, cap: Capability) -> Self {
        self.capabilities.grant(cap);
        self
    }

    /// Add multiple capabilities.
    pub fn capabilities(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        for cap in caps {
            self.capabilities.grant(cap);
        }
        self
    }

    /// Set memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.resources.memory.heap_max = bytes;
        self
    }

    /// Set stack size in bytes.
    pub fn stack_size(mut self, bytes: usize) -> Self {
        self.resources.memory.stack_max = bytes;
        self
    }

    /// Set fuel limit for CPU metering.
    pub fn fuel(mut self, fuel: u64) -> Self {
        self.resources.cpu.fuel = Some(fuel);
        self
    }

    /// Set CPU time limit.
    pub fn cpu_time_limit(mut self, duration: Duration) -> Self {
        self.resources.time.cpu_time = Some(duration);
        self
    }

    /// Set wall clock time limit.
    pub fn wall_time_limit(mut self, duration: Duration) -> Self {
        self.resources.time.wall_time = Some(duration);
        self
    }

    /// Set the preemption interval for cooperative scheduling.
    pub fn preemption_interval(mut self, duration: Duration) -> Self {
        self.resources.cpu.preemption_interval = duration;
        self
    }

    /// Set I/O read limit in bytes.
    pub fn io_read_limit(mut self, bytes: u64) -> Self {
        self.resources.io.read_bytes = Some(bytes);
        self
    }

    /// Set I/O write limit in bytes.
    pub fn io_write_limit(mut self, bytes: u64) -> Self {
        self.resources.io.write_bytes = Some(bytes);
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set multiple environment variables.
    pub fn envs(mut self, vars: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env.extend(vars);
        self
    }

    /// Add a command-line argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set command-line arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.args = args.into_iter().collect();
        self
    }

    /// Enable snapshots.
    pub fn enable_snapshots(mut self, storage_path: Option<PathBuf>) -> Self {
        self.snapshot.enabled = true;
        self.snapshot.storage_path = storage_path;
        self
    }

    /// Set the entry point function name.
    pub fn entry_point(mut self, name: impl Into<String>) -> Self {
        self.entry_point = name.into();
        self
    }

    /// Build the configuration.
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
