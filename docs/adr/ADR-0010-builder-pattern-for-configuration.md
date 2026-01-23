# ADR-0010: Builder Pattern for Configuration

## Status

Accepted

## Context

Sandbox configuration involves many parameters: WASM module, resource limits (memory, CPU, I/O, time), capabilities, environment variables, arguments, snapshot settings, and entry point. Different approaches to configuration have trade-offs:

- **Struct with public fields**: Easy to create, but no validation, allows invalid states
- **Constructor with many arguments**: Hard to read, order-dependent
- **Multiple constructors**: Combinatorial explosion of options
- **Builder pattern**: Verbose but clear, enables validation, IDE-friendly

We needed a configuration approach that:

- Makes common cases easy (minimal config for simple sandboxes)
- Allows precise control when needed
- Validates configuration before use
- Provides good IDE autocomplete experience

## Decision

We implemented the **builder pattern** with a `#[must_use]` builder struct and method chaining.

### Builder Definition

```rust
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
```

### Method Chaining

```rust
impl SandboxConfigBuilder {
    pub fn new() -> Self {
        Self {
            entry_point: "_start".to_string(),
            ..Default::default()
        }
    }

    // Required: module (returns Result for validation)
    pub fn module(mut self, bytes: &[u8]) -> Result<Self> {
        self.module = Some(WasmModule::from_bytes(bytes.to_vec())?);
        Ok(self)
    }

    // Optional: capabilities
    pub fn capability(mut self, cap: Capability) -> Self {
        self.capabilities.grant(cap);
        self
    }

    // Optional: resource limits
    pub fn memory_limit(mut self, bytes: usize) -> Self {
        self.resources.memory.heap_max = bytes;
        self
    }

    pub fn fuel(mut self, fuel: u64) -> Self {
        self.resources.cpu.fuel = Some(fuel);
        self
    }

    // Optional: environment
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    // Terminal: build with validation
    pub fn build(self) -> Result<SandboxConfig> {
        let module = self.module
            .ok_or_else(|| Error::InvalidConfig("WASM module is required".into()))?;

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
```

### Usage Examples

```rust
// Minimal configuration
let config = SandboxConfig::builder()
    .module(wasm_bytes)?
    .build()?;

// Full configuration
let config = SandboxConfig::builder()
    .module(wasm_bytes)?
    .memory_limit(128 * 1024 * 1024)  // 128MB
    .fuel(1_000_000)
    .wall_time_limit(Duration::from_secs(30))
    .capability(Capability::stdout())
    .capability(Capability::filesystem_read("/data"))
    .env("API_URL", "https://api.example.com")
    .arg("--verbose".to_string())
    .entry_point("main")
    .build()?;
```

### Validation

The `build()` method validates:

1. Required fields are present (module)
2. Module is valid WASM (magic number check)
3. Configuration is internally consistent

## Consequences

### Positive

- **Readable**: Method names document parameters
- **IDE-friendly**: Autocomplete shows available options
- **Safe defaults**: Unspecified options get sensible defaults
- **Validation**: Invalid configs fail at `build()`, not runtime
- **Extensible**: New options can be added without breaking changes
- **Composable**: Builder can be passed around, partially filled

### Negative

- **Verbose**: More code than struct literals
- **Result chaining**: `module()` returns `Result`, requires `?` in chain
- **Allocation**: Builder allocates intermediate state

### Implications

- Entry point from `SandboxConfig::builder()`, not direct construction
- Always call `build()` at the end (enforced by `#[must_use]`)
- Error handling happens at both `module()` and `build()` calls
- Presets (restrictive, permissive) can be implemented as builder factories
