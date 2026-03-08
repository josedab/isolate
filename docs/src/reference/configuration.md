# Configuration Reference

Complete reference for `SandboxConfig` and related configuration options.

## SandboxConfigBuilder

### Module Configuration

```rust
SandboxConfig::builder()
    // From bytes
    .module(&wasm_bytes)?

    // Or from a pre-validated WasmModule
    .wasm_module(wasm_module)
```

### Memory Limits

```rust
    // Maximum heap memory (default: 256MB)
    .memory_limit(128 * 1024 * 1024)

    // Maximum stack size (default: 1MB)
    .stack_size(1024 * 1024)
```

### CPU Limits

```rust
    // Fuel limit (default: unlimited)
    .fuel(1_000_000)

    // CPU time limit (default: unlimited)
    .cpu_time_limit(Duration::from_secs(30))

    // Preemption interval for cooperative scheduling
    .preemption_interval(Duration::from_millis(10))
```

### Time Limits

```rust
    // Wall clock timeout (default: unlimited)
    .wall_time_limit(Duration::from_secs(60))
```

### I/O Limits

```rust
    // Maximum bytes that can be read (default: unlimited)
    .io_read_limit(10 * 1024 * 1024)

    // Maximum bytes that can be written (default: unlimited)
    .io_write_limit(1024 * 1024)
```

### Capabilities

```rust
    // Standard I/O
    .capability(Capability::stdout())
    .capability(Capability::stderr())
    .capability(Capability::stdin())

    // Filesystem
    .capability(Capability::filesystem_read("/data"))
    .capability(Capability::filesystem_write("/tmp"))

    // Network
    .capability(Capability::http_client(vec!["api.example.com".into()]))
    .capability(Capability::dns_resolve())

    // System
    .capability(Capability::system_clock())
    .capability(Capability::secure_random())

    // Environment
    .capability(Capability::env_var("API_KEY"))

    // Multiple at once
    .capabilities([
        Capability::stdout(),
        Capability::stderr(),
    ])
```

### Environment Variables

```rust
    // Single variable
    .env("KEY", "value")

    // Multiple variables
    .envs([
        ("KEY1".to_string(), "value1".to_string()),
        ("KEY2".to_string(), "value2".to_string()),
    ])
```

### Arguments

```rust
    // Single argument
    .arg("--verbose".to_string())

    // Multiple arguments
    .args(["--config".to_string(), "config.json".to_string()])
```

### Entry Point

```rust
    // Custom entry point (default: "_start")
    .entry_point("main")
```

### Snapshots

```rust
    // Enable snapshots with optional storage path
    .enable_snapshots(Some(PathBuf::from("/var/isolate/snapshots")))
```

### Building

```rust
    // Build the configuration
    .build()?
```

## Complete Example

```rust
use isolate_core::{SandboxConfig, capability::Capability};
use std::time::Duration;

let config = SandboxConfig::builder()
    // Module
    .module(&wasm_bytes)?

    // Memory
    .memory_limit(128 * 1024 * 1024)
    .stack_size(1024 * 1024)

    // CPU
    .fuel(10_000_000)
    .cpu_time_limit(Duration::from_secs(30))

    // Time
    .wall_time_limit(Duration::from_secs(60))

    // I/O
    .io_read_limit(10 * 1024 * 1024)
    .io_write_limit(1024 * 1024)

    // Capabilities
    .capability(Capability::stdout())
    .capability(Capability::stderr())
    .capability(Capability::filesystem_read("/data"))
    .capability(Capability::system_clock())

    // Environment
    .env("CONFIG_PATH", "/data/config.json")

    // Arguments
    .arg("--verbose".to_string())

    // Entry point
    .entry_point("_start")

    // Build
    .build()?;
```

## ResourceLimits

The `ResourceLimits` struct contains all resource limit settings:

```rust
pub struct ResourceLimits {
    pub memory: MemoryLimits,
    pub cpu: CpuLimits,
    pub time: TimeLimits,
    pub io: IoLimits,
}

pub struct MemoryLimits {
    pub heap_max: usize,      // Maximum heap size
    pub stack_max: usize,     // Maximum stack size
}

pub struct CpuLimits {
    pub fuel: Option<u64>,              // Instruction fuel
    pub preemption_interval: Duration,  // Scheduling interval
}

pub struct TimeLimits {
    pub cpu_time: Option<Duration>,    // CPU time limit
    pub wall_time: Option<Duration>,   // Wall clock limit
}

pub struct IoLimits {
    pub read_bytes: Option<u64>,   // Read limit
    pub write_bytes: Option<u64>,  // Write limit
}
```

## Capability Types

```rust
pub enum Capability {
    // Standard I/O
    Stdout,
    Stderr,
    Stdin,

    // Filesystem
    FilesystemRead(PathBuf),
    FilesystemWrite(PathBuf),

    // Network
    HttpClient(Vec<String>),
    TcpConnect(Vec<String>),
    DnsLookup,

    // System
    Clock,
    Random,

    // Environment
    EnvVar(String),
}
```

## Defaults

| Setting | Default Value |
|---------|---------------|
| `memory_limit` | 256 MB |
| `stack_size` | 1 MB |
| `fuel` | Unlimited |
| `cpu_time_limit` | Unlimited |
| `wall_time_limit` | Unlimited |
| `io_read_limit` | Unlimited |
| `io_write_limit` | Unlimited |
| `entry_point` | `"_start"` |
| `capabilities` | Empty (none) |
