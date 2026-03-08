# API Reference

Core types and functions in `isolate-core`.

## Sandbox

The main entry point for creating and running isolated WASM code.

### Creating a Sandbox

```rust
use isolate_core::{Sandbox, SandboxConfig};

let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .build()?;

let sandbox = Sandbox::create(config).await?;
```

### With a Shared Engine

For better performance when creating many sandboxes:

```rust
use isolate_core::engine::WasmEngine;
use std::sync::Arc;

let engine = Arc::new(WasmEngine::new()?);

let sandbox1 = Sandbox::create_with_engine(config1, engine.clone()).await?;
let sandbox2 = Sandbox::create_with_engine(config2, engine.clone()).await?;
```

### Running

```rust
// Run with empty input
let output = sandbox.run(&[]).await?;

// Run with input
let output = sandbox.run(b"input data").await?;
```

### Calling Functions

```rust
use wasmtime::Val;

let result = sandbox.call("add", &[Val::I32(1), Val::I32(2)]).await?;
```

### Sandbox Properties

```rust
// Unique identifier
let id: SandboxId = sandbox.id();

// Current state
let state: SandboxState = sandbox.state();

// Module hash
let hash: &ModuleHash = sandbox.module_hash();

// Configuration
let config: &SandboxConfig = sandbox.config();

// Age since creation
let age: Duration = sandbox.age();
```

### Termination

```rust
let metrics = sandbox.terminate().await?;
```

## SandboxId

A unique identifier for a sandbox.

```rust
let id = SandboxId::new();
println!("Sandbox ID: {}", id);  // UUID format
```

## SandboxState

Current state of the sandbox.

```rust
pub enum SandboxState {
    Creating,    // Being initialized
    Ready,       // Ready to run
    Running,     // Currently executing
    Paused,      // Execution paused
    Terminated,  // Finished
}
```

## Output

Result of sandbox execution.

```rust
pub struct Output {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration: Duration,
    pub resource_usage: ResourceUsage,
}

impl Output {
    // Check if execution succeeded (exit code 0)
    pub fn success(&self) -> bool;

    // Get stdout as string
    pub fn stdout_str(&self) -> String;

    // Get stderr as string
    pub fn stderr_str(&self) -> String;
}
```

## ResourceUsage

Resource consumption during execution.

```rust
pub struct ResourceUsage {
    pub fuel_consumed: Option<u64>,
    pub memory_peak: usize,
    pub io_read: u64,
    pub io_write: u64,
}
```

## Error

All possible error types.

```rust
pub enum Error {
    Create(String),
    Compilation(String),
    Instantiation(String),
    Execution(String),
    Timeout(Duration),
    FuelExhausted { limit: u64 },
    MemoryLimitExceeded { limit: usize, requested: usize },
    CapabilityDenied(Capability),
    InvalidCapability(String),
    InvalidConfig(String),
    InvalidState { expected: String, actual: String },
    Snapshot(String),
    SnapshotNotFound(String),
    Io { source: std::io::Error },
    FilesystemAccessDenied { path: PathBuf },
    NetworkAccessDenied { host: String },
    Engine(String),
    ModuleValidation(String),
    FunctionNotFound(String),
    InvalidSignature { name: String, expected: String, actual: String },
    PoolExhausted,
    Http(String),
}

impl Error {
    pub fn is_timeout(&self) -> bool;
    pub fn is_resource_limit(&self) -> bool;
    pub fn is_capability_error(&self) -> bool;
}
```

## Result

Type alias for `Result<T, Error>`.

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## WasmModule

Validated WASM module.

```rust
let module = WasmModule::from_bytes(wasm_bytes)?;
let hash = module.hash();
let bytes = module.bytes();
```

## ModuleHash

SHA-256 hash of a WASM module.

```rust
let hash = ModuleHash::from_bytes(&wasm_bytes);
println!("Hash: {}", hash);  // First 16 chars
```

## Capability

Permission grants for sandbox operations.

```rust
// Constructors
Capability::stdout()
Capability::stderr()
Capability::stdin()
Capability::system_clock()
Capability::monotonic_clock()
Capability::secure_random()
Capability::filesystem_read(path)
Capability::filesystem_write(path)
Capability::http_client(hosts)
Capability::dns_resolve()
Capability::env_var(name)
Capability::env_all()
```

## CapabilitySet

Collection of granted capabilities.

```rust
let mut caps = CapabilitySet::default();
caps.grant(Capability::stdout());
caps.has(&Capability::stdout());  // true
```

## SandboxMetrics

Runtime metrics for a sandbox.

```rust
let metrics = sandbox.metrics();
let run_count = metrics.run_count();
```

## See Also

- [Configuration Reference](./configuration.md)
- [Error Handling](./errors.md)
- [API Documentation](https://docs.rs/isolate-core)
