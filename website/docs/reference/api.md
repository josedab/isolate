---
sidebar_position: 2
---

# API Reference

This page provides an overview of the main Isolate API. For complete API documentation with all types and methods, see [docs.rs/isolate-core](https://docs.rs/isolate-core).

## Core Types

### Sandbox

The main type for creating and running isolated WASM code.

```rust
pub struct Sandbox {
    // ...
}

impl Sandbox {
    /// Create a new sandbox with the given configuration.
    pub async fn create(config: SandboxConfig) -> Result<Self>;

    /// Create a new sandbox with a shared engine (better performance).
    pub async fn create_with_engine(
        config: SandboxConfig,
        engine: Arc<WasmEngine>,
    ) -> Result<Self>;

    /// Get the sandbox ID.
    pub fn id(&self) -> SandboxId;

    /// Get the current state.
    pub fn state(&self) -> SandboxState;

    /// Get the module hash.
    pub fn module_hash(&self) -> &ModuleHash;

    /// Get the configuration.
    pub fn config(&self) -> &SandboxConfig;

    /// Run the sandbox with optional input.
    pub async fn run(&mut self, input: &[u8]) -> Result<Output>;

    /// Call a specific exported function.
    pub async fn call(
        &mut self,
        function: &str,
        args: &[wasmtime::Val],
    ) -> Result<Vec<wasmtime::Val>>;

    /// Terminate the sandbox.
    pub async fn terminate(&mut self) -> Result<SandboxMetrics>;
}
```

### SandboxConfig

Configuration for creating a sandbox.

```rust
impl SandboxConfig {
    /// Create a new builder.
    pub fn builder() -> SandboxConfigBuilder;
}

impl SandboxConfigBuilder {
    /// Set the WASM module.
    pub fn module(self, bytes: &[u8]) -> Result<Self>;

    /// Set memory limit.
    pub fn memory_limit(self, bytes: usize) -> Self;

    /// Set stack size.
    pub fn stack_size(self, bytes: usize) -> Self;

    /// Set fuel limit.
    pub fn fuel(self, fuel: u64) -> Self;

    /// Set CPU time limit.
    pub fn cpu_time_limit(self, duration: Duration) -> Self;

    /// Set wall clock timeout.
    pub fn wall_time_limit(self, duration: Duration) -> Self;

    /// Set I/O read limit.
    pub fn io_read_limit(self, bytes: u64) -> Self;

    /// Set I/O write limit.
    pub fn io_write_limit(self, bytes: u64) -> Self;

    /// Add a capability.
    pub fn capability(self, cap: Capability) -> Self;

    /// Add multiple capabilities.
    pub fn capabilities(self, caps: impl IntoIterator<Item = Capability>) -> Self;

    /// Set an environment variable.
    pub fn env(self, key: &str, value: &str) -> Self;

    /// Add a command-line argument.
    pub fn arg(self, arg: String) -> Self;

    /// Set the entry point.
    pub fn entry_point(self, name: &str) -> Self;

    /// Build the configuration.
    pub fn build(self) -> Result<SandboxConfig>;
}
```

### Output

Result of sandbox execution.

```rust
pub struct Output {
    /// Exit code (0 for success).
    pub exit_code: i32,

    /// Captured stdout.
    pub stdout: Vec<u8>,

    /// Captured stderr.
    pub stderr: Vec<u8>,

    /// Execution duration.
    pub duration: Duration,

    /// Resource usage.
    pub resource_usage: ResourceUsage,
}

impl Output {
    /// Check if execution was successful.
    pub fn success(&self) -> bool;

    /// Get stdout as a string.
    pub fn stdout_str(&self) -> String;

    /// Get stderr as a string.
    pub fn stderr_str(&self) -> String;
}
```

### ResourceUsage

Resource consumption during execution.

```rust
pub struct ResourceUsage {
    /// Fuel consumed (if fuel metering enabled).
    pub fuel_consumed: Option<u64>,

    /// Peak memory usage in bytes.
    pub memory_peak: usize,

    /// Total bytes read.
    pub io_read: u64,

    /// Total bytes written.
    pub io_write: u64,
}
```

### SandboxState

Current state of a sandbox.

```rust
pub enum SandboxState {
    /// Sandbox is being created.
    Creating,
    /// Sandbox is ready to run.
    Ready,
    /// Sandbox is currently running.
    Running,
    /// Sandbox is paused.
    Paused,
    /// Sandbox has terminated.
    Terminated,
}
```

## Capability Types

```rust
impl Capability {
    // Standard I/O
    pub fn stdout() -> Self;
    pub fn stderr() -> Self;
    pub fn stdin() -> Self;

    // Filesystem
    pub fn filesystem_read(path: impl Into<PathBuf>) -> Self;
    pub fn filesystem_write(path: impl Into<PathBuf>) -> Self;
    pub fn temp_dir() -> Self;

    // Network
    pub fn http_client(hosts: Vec<impl Into<String>>) -> Self;
    pub fn tcp_connect(addrs: Vec<SocketAddr>) -> Self;
    pub fn tcp_listen(port: u16) -> Self;
    pub fn dns_resolve() -> Self;

    // Time
    pub fn system_clock() -> Self;
    pub fn monotonic_clock() -> Self;
    pub fn timers() -> Self;

    // Random
    pub fn secure_random() -> Self;
    pub fn seeded_random(seed: u64) -> Self;

    // Environment
    pub fn env_var(name: impl Into<String>) -> Self;
    pub fn env_all() -> Self;
    pub fn args() -> Self;

    // Host functions
    pub fn host_function(name: impl Into<String>) -> Self;
}
```

## Error Types

```rust
pub enum Error {
    /// Invalid WASM module.
    InvalidModule(String),

    /// Module compilation failed.
    Compilation(String),

    /// Execution error.
    Execution(String),

    /// Fuel exhausted.
    FuelExhausted { limit: u64 },

    /// Memory limit exceeded.
    MemoryLimitExceeded { limit: usize, requested: usize },

    /// Timeout.
    Timeout(Duration),

    /// Capability denied.
    CapabilityDenied(String),

    /// Invalid state transition.
    InvalidState { expected: String, actual: String },

    /// I/O error.
    Io(std::io::Error),

    /// Configuration error.
    Configuration(String),
}

/// Result type alias.
pub type Result<T> = std::result::Result<T, Error>;
```

## WasmEngine

Shared WASM execution engine with module caching.

```rust
pub struct WasmEngine {
    // ...
}

impl WasmEngine {
    /// Create a new engine with default configuration.
    pub fn new() -> Result<Self>;

    /// Get the number of cached modules.
    pub fn cached_module_count(&self) -> usize;

    /// Clear the module cache.
    pub fn clear_cache(&self);

    /// Increment the epoch (for timeout enforcement).
    pub fn increment_epoch(&self);
}
```

## Metrics

```rust
pub struct SandboxMetrics {
    // ...
}

impl SandboxMetrics {
    /// Get the number of runs.
    pub fn run_count(&self) -> u64;

    /// Get total execution time.
    pub fn total_execution_time(&self) -> Duration;

    /// Get success count.
    pub fn success_count(&self) -> u64;

    /// Get failure count.
    pub fn failure_count(&self) -> u64;
}
```

## Feature Flags

Isolate supports optional features via Cargo feature flags:

| Feature | Description |
|---------|-------------|
| `snapshots` | Enable snapshot/restore functionality |
| `wasi-preview2` | WASI preview2 support (experimental) |
| `kubernetes` | Kubernetes integration |
| `otel-telemetry` | OpenTelemetry tracing |
| `debug-support` | Debugging support |

Enable features in `Cargo.toml`:

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["snapshots", "otel-telemetry"] }
```

## See Also

- [docs.rs/isolate-core](https://docs.rs/isolate-core) - Complete API documentation
- [Configuration](./configuration) - Configuration reference
- [Errors](./errors) - Error handling guide
