# Architecture

Internal architecture of Isolate.

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      isolate-core                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │   Sandbox   │───▶│   Engine    │───▶│    WASI     │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│         │                  │                   │            │
│         ▼                  ▼                   ▼            │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │ Capability  │    │  Resource   │    │   Capture   │     │
│  │  Enforcer   │    │   Meter     │    │   Streams   │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Components

### Sandbox (`sandbox.rs`)

The main API for creating and managing isolated WASM execution.

**Responsibilities:**
- Configuration validation
- Lifecycle management (create, run, terminate)
- State tracking
- Metrics collection

```rust
pub struct Sandbox {
    id: SandboxId,
    state: SandboxState,
    config: SandboxConfig,
    engine: Arc<WasmEngine>,
    compiled: CompiledModule,
    instance: Mutex<Option<WasmInstance>>,
    enforcer: CapabilityEnforcer,
    meter: ResourceMeter,
    metrics: SandboxMetrics,
    created_at: Instant,
}
```

### WasmEngine (`engine/wasm.rs`)

Manages the Wasmtime runtime and module compilation.

**Responsibilities:**
- Module compilation and caching
- Instance creation
- Epoch-based interruption

```rust
pub struct WasmEngine {
    engine: wasmtime::Engine,
    module_cache: DashMap<ModuleHash, CompiledModule>,
}
```

**Key Features:**
- Module compilation is cached by hash
- Shared engine reduces memory usage
- Epoch interruption enables timeouts

### CapabilityEnforcer (`capability/enforcer.rs`)

Checks permissions for all privileged operations.

**Responsibilities:**
- Permission checking
- Audit logging
- Default-deny enforcement

```rust
pub struct CapabilityEnforcer {
    capabilities: CapabilitySet,
    sandbox_id: Uuid,
}

impl CapabilityEnforcer {
    pub fn check_stdout(&self) -> Result<()>;
    pub fn check_fs_read(&self, path: &Path) -> Result<()>;
    pub fn check_http(&self, host: &str) -> Result<()>;
    // ...
}
```

### ResourceMeter (`resource/metering.rs`)

Tracks and enforces resource consumption.

**Responsibilities:**
- Fuel tracking
- I/O metering
- Memory monitoring

```rust
pub struct ResourceMeter {
    limits: ResourceLimits,
    fuel_consumed: AtomicU64,
    io_read: AtomicU64,
    io_write: AtomicU64,
    memory_peak: AtomicUsize,
}
```

### CaptureStream (`engine/capture.rs`)

Captures stdout/stderr output with metering.

**Responsibilities:**
- Output buffering
- I/O limit enforcement
- Thread-safe capture

## Execution Flow

### 1. Configuration Phase

```
SandboxConfig::builder()
    │
    ├── Validate WASM module
    │   └── Check magic number, size
    │
    ├── Build CapabilitySet
    │   └── Collect granted capabilities
    │
    ├── Build ResourceLimits
    │   └── Memory, CPU, I/O, time
    │
    └── build() → SandboxConfig
```

### 2. Creation Phase

```
Sandbox::create(config)
    │
    ├── Get or create WasmEngine
    │
    ├── Compile module (or use cache)
    │   └── wasmtime::Module::new()
    │
    ├── Create CapabilityEnforcer
    │
    ├── Create ResourceMeter
    │
    └── Return Sandbox (state: Ready)
```

### 3. Execution Phase

```
sandbox.run(input)
    │
    ├── Check state == Ready
    │
    ├── Create WasmInstance
    │   ├── Set up WASI context
    │   ├── Configure stdin with input
    │   ├── Configure stdout/stderr capture
    │   └── Set fuel and limits
    │
    ├── Start epoch ticker (for timeout)
    │
    ├── Execute _start function
    │   └── spawn_blocking for sync execution
    │
    ├── Stop epoch ticker
    │
    ├── Collect output and metrics
    │
    └── Return Output
```

## Thread Safety

### Safe Sharing

- `WasmEngine` is `Send + Sync` (uses `DashMap` for cache)
- `CapabilityEnforcer` is `Clone + Send + Sync`
- `ResourceMeter` uses atomics

### Execution Model

- WASM execution is synchronous (blocking)
- Wrapped in `spawn_blocking` for async compatibility
- Epoch ticker runs in separate task

## Memory Management

### Module Caching

Compiled modules are cached by content hash:

```rust
let hash = ModuleHash::from_bytes(&wasm_bytes);
if let Some(compiled) = cache.get(&hash) {
    return compiled.clone();
}
```

### Instance Lifecycle

- Instance created per `run()` call
- Dropped after execution
- Memory released immediately

## Error Handling

### Error Propagation

All errors are converted to `Error` enum:

```rust
// Wasmtime errors
wasmtime::Error → Error::Execution

// Capability checks
CapabilityDenied → Error::CapabilityDenied

// Resource limits
FuelExhausted → Error::FuelExhausted
```

### Panic Safety

- No panics in library code
- All `unwrap()`/`expect()` are in tests only
- `spawn_blocking` catches panics

## Extension Points

### Custom Host Functions

```rust
// In engine/host.rs
linker.func_wrap("custom", "log", |caller: Caller<'_, _>, ptr: i32, len: i32| {
    // Custom implementation
})?;
```

### Custom Capabilities

Extend `Capability` enum and `CapabilityEnforcer`:

```rust
pub enum Capability {
    // ... existing
    Custom(CustomCapability),
}
```

## See Also

- [WASM Engine](./wasm-engine.md)
- [Capability System](./capability-system.md)
