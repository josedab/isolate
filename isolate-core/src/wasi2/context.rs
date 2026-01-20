//! WASI Preview2 context and configuration.

use crate::capability::{Capability, CapabilitySet};
use crate::error::{Error, Result};
use crate::resource::ResourceLimits;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiView};

/// Hash of a WASM component for caching and identification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentHash(pub String);

impl ComponentHash {
    /// Compute hash from component bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        Self(hex::encode(result))
    }
}

impl std::fmt::Display for ComponentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0[..16])
    }
}

/// WASM component representation.
#[derive(Clone)]
pub struct WasmComponent {
    /// Raw component bytes.
    bytes: Vec<u8>,
    /// Precomputed hash.
    hash: ComponentHash,
}

impl WasmComponent {
    /// Create a new WASM component from bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        super::validate_component(&bytes)?;
        let hash = ComponentHash::from_bytes(&bytes);
        Ok(Self { bytes, hash })
    }

    /// Get the raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get the component hash.
    pub fn hash(&self) -> &ComponentHash {
        &self.hash
    }
}

impl std::fmt::Debug for WasmComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmComponent")
            .field("size", &self.bytes.len())
            .field("hash", &self.hash)
            .finish()
    }
}

/// Configuration for creating a component sandbox.
#[derive(Debug, Clone)]
pub struct ComponentConfig {
    /// The WASM component to execute.
    pub component: WasmComponent,
    /// Granted capabilities.
    pub capabilities: CapabilitySet,
    /// Resource limits.
    pub resources: ResourceLimits,
    /// Environment variables.
    pub env: HashMap<String, String>,
    /// Command-line arguments.
    pub args: Vec<String>,
    /// Working directory for the component.
    pub working_dir: Option<PathBuf>,
    /// Network access configuration.
    pub network: NetworkConfig,
    /// Whether to inherit the host's environment.
    pub inherit_env: bool,
}

impl ComponentConfig {
    /// Create a new configuration builder.
    pub fn builder() -> ComponentConfigBuilder {
        ComponentConfigBuilder::new()
    }

    /// Get the component hash.
    pub fn component_hash(&self) -> &ComponentHash {
        self.component.hash()
    }
}

/// Network configuration for components.
#[derive(Debug, Clone, Default)]
pub struct NetworkConfig {
    /// Allowed HTTP hosts (supports wildcards like "*.example.com").
    pub allowed_hosts: Vec<String>,
    /// Whether to allow outbound TCP connections.
    pub allow_tcp: bool,
    /// Whether to allow DNS resolution.
    pub allow_dns: bool,
    /// Connection timeout.
    pub timeout: Option<Duration>,
}

/// Builder for ComponentConfig.
#[derive(Debug, Default)]
pub struct ComponentConfigBuilder {
    component: Option<WasmComponent>,
    capabilities: CapabilitySet,
    resources: ResourceLimits,
    env: HashMap<String, String>,
    args: Vec<String>,
    working_dir: Option<PathBuf>,
    network: NetworkConfig,
    inherit_env: bool,
}

impl ComponentConfigBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the WASM component from bytes.
    pub fn component(mut self, bytes: &[u8]) -> Result<Self> {
        self.component = Some(WasmComponent::from_bytes(bytes.to_vec())?);
        Ok(self)
    }

    /// Set the WASM component directly.
    pub fn wasm_component(mut self, component: WasmComponent) -> Self {
        self.component = Some(component);
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

    /// Allow stdout access.
    pub fn allow_stdout(self) -> Self {
        self.capability(Capability::stdout())
    }

    /// Allow stderr access.
    pub fn allow_stderr(self) -> Self {
        self.capability(Capability::stderr())
    }

    /// Allow stdin access.
    pub fn allow_stdin(self) -> Self {
        self.capability(Capability::stdin())
    }

    /// Set memory limit in bytes.
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.resources.memory.heap_max = bytes;
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

    /// Set the working directory.
    pub fn working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(path.into());
        self
    }

    /// Add allowed HTTP hosts.
    pub fn allow_http_hosts(mut self, hosts: Vec<impl Into<String>>) -> Self {
        self.network.allowed_hosts = hosts.into_iter().map(Into::into).collect();
        self
    }

    /// Allow outbound TCP connections.
    pub fn allow_tcp(mut self) -> Self {
        self.network.allow_tcp = true;
        self
    }

    /// Allow DNS resolution.
    pub fn allow_dns(mut self) -> Self {
        self.network.allow_dns = true;
        self
    }

    /// Set network timeout.
    pub fn network_timeout(mut self, timeout: Duration) -> Self {
        self.network.timeout = Some(timeout);
        self
    }

    /// Inherit environment variables from the host.
    pub fn inherit_env(mut self) -> Self {
        self.inherit_env = true;
        self
    }

    /// Grant filesystem read access.
    pub fn filesystem_read(mut self, path: impl Into<PathBuf>) -> Self {
        self.capabilities.grant(Capability::filesystem_read(path.into()));
        self
    }

    /// Grant filesystem write access.
    pub fn filesystem_write(mut self, path: impl Into<PathBuf>) -> Self {
        self.capabilities.grant(Capability::filesystem_write(path.into()));
        self
    }

    /// Set resource limits in bulk.
    pub fn resources(mut self, resources: crate::resource::ResourceLimits) -> Self {
        self.resources = resources;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> Result<ComponentConfig> {
        let component = self
            .component
            .ok_or_else(|| Error::InvalidConfig("WASM component is required".to_string()))?;

        Ok(ComponentConfig {
            component,
            capabilities: self.capabilities,
            resources: self.resources,
            env: self.env,
            args: self.args,
            working_dir: self.working_dir,
            network: self.network,
            inherit_env: self.inherit_env,
        })
    }
}

/// State holder implementing WasiView for the component runtime.
pub struct ComponentHostState {
    /// WASI context.
    ctx: WasiCtx,
    /// Resource table for WASI resources.
    table: ResourceTable,
    /// Store limits for memory/fuel enforcement.
    limits: StoreLimits,
    /// I/O capture for stdout.
    stdout_buffer: Vec<u8>,
    /// I/O capture for stderr.
    stderr_buffer: Vec<u8>,
}

impl ComponentHostState {
    /// Create a new host state from configuration.
    pub fn new(config: &ComponentConfig) -> Result<Self> {
        let mut builder = WasiCtxBuilder::new();

        // Configure stdout/stderr
        if config.capabilities.has(&Capability::stdout()) {
            builder.inherit_stdout();
        }
        if config.capabilities.has(&Capability::stderr()) {
            builder.inherit_stderr();
        }
        if config.capabilities.has(&Capability::stdin()) {
            builder.inherit_stdin();
        }

        // Configure environment
        if config.inherit_env {
            builder.inherit_env();
        } else {
            for (key, value) in &config.env {
                builder.env(key, value);
            }
        }

        // Configure arguments
        builder.args(&config.args);

        // Configure network (if allowed)
        if config.network.allow_tcp || !config.network.allowed_hosts.is_empty() {
            builder.inherit_network();
        }

        let ctx = builder.build();
        let table = ResourceTable::new();

        // Create store limits from config
        let limits =
            StoreLimitsBuilder::new().memory_size(config.resources.memory.heap_max).build();

        Ok(Self { ctx, table, limits, stdout_buffer: Vec::new(), stderr_buffer: Vec::new() })
    }

    /// Get captured stdout.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout_buffer
    }

    /// Get captured stderr.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr_buffer
    }

    /// Get mutable reference to store limits.
    pub fn limits(&mut self) -> &mut StoreLimits {
        &mut self.limits
    }
}

impl WasiView for ComponentHostState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module (also valid as simple component for testing)
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    #[test]
    fn test_component_hash() {
        let hash1 = ComponentHash::from_bytes(MINIMAL_WASM);
        let hash2 = ComponentHash::from_bytes(MINIMAL_WASM);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_wasm_component_creation() {
        let component = WasmComponent::from_bytes(MINIMAL_WASM.to_vec());
        assert!(component.is_ok());
    }

    #[test]
    fn test_component_config_builder() {
        let config = ComponentConfig::builder()
            .component(MINIMAL_WASM)
            .unwrap()
            .memory_limit(128 * 1024 * 1024)
            .fuel(1_000_000)
            .allow_stdout()
            .allow_stderr()
            .env("KEY", "value")
            .arg("arg1".to_string())
            .build()
            .unwrap();

        assert_eq!(config.resources.memory.heap_max, 128 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(1_000_000));
        assert!(config.capabilities.has(&Capability::stdout()));
        assert!(config.capabilities.has(&Capability::stderr()));
    }

    #[test]
    fn test_component_config_builder_missing_component() {
        let result = ComponentConfig::builder().build();
        assert!(result.is_err());
    }

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert!(config.allowed_hosts.is_empty());
        assert!(!config.allow_tcp);
        assert!(!config.allow_dns);
    }

    #[test]
    fn test_component_config_with_network() {
        let config = ComponentConfig::builder()
            .component(MINIMAL_WASM)
            .unwrap()
            .allow_http_hosts(vec!["api.example.com", "*.trusted.com"])
            .allow_tcp()
            .allow_dns()
            .network_timeout(Duration::from_secs(30))
            .build()
            .unwrap();

        assert_eq!(config.network.allowed_hosts.len(), 2);
        assert!(config.network.allow_tcp);
        assert!(config.network.allow_dns);
        assert_eq!(config.network.timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_host_state_creation() {
        let config = ComponentConfig::builder()
            .component(MINIMAL_WASM)
            .unwrap()
            .allow_stdout()
            .build()
            .unwrap();

        let state = ComponentHostState::new(&config);
        assert!(state.is_ok());
    }
}
