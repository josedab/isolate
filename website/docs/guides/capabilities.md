---
sidebar_position: 1
---

# Capabilities

Isolate uses a **capability-based security model** with a default-deny policy. WASM modules have no access to system resources unless explicitly granted.

## Core Concept

Traditional sandboxes often work by blocking dangerous operations. Isolate inverts this: **everything is blocked by default**, and you explicitly grant capabilities.

```rust
// Without capabilities, the module can't do anything meaningful
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .build()?;

// Grant specific capabilities
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())           // Can write to stdout
    .capability(Capability::filesystem_read("/data"))  // Can read /data
    .build()?;
```

## Available Capabilities

### Standard I/O

| Capability | Description |
|------------|-------------|
| `Capability::stdout()` | Write to standard output |
| `Capability::stderr()` | Write to standard error |
| `Capability::stdin()` | Read from standard input |

```rust
.capability(Capability::stdout())
.capability(Capability::stderr())
.capability(Capability::stdin())
```

### Filesystem

| Capability | Description |
|------------|-------------|
| `filesystem_read(path)` | Read files under path |
| `filesystem_write(path)` | Write files under path |
| `temp_dir()` | Access to a temporary directory |

```rust
// Read-only access to /data
.capability(Capability::filesystem_read("/data"))

// Read-write access to /tmp/sandbox
.capability(Capability::filesystem_read("/tmp/sandbox"))
.capability(Capability::filesystem_write("/tmp/sandbox"))

// Temporary directory
.capability(Capability::temp_dir())
```

:::warning Path Enforcement
Paths are strictly enforced. Access to `/data` does NOT grant access to `/data-backup`.
:::

### Environment Variables

```rust
// Grant access to specific environment variables
.capability(Capability::env_var("API_KEY"))
.capability(Capability::env_var("CONFIG_PATH"))

// Set the values
.env("API_KEY", "secret123")
.env("CONFIG_PATH", "/etc/app/config.json")

// Read all environment variables (use with caution)
.capability(Capability::env_all())
```

### Network

| Capability | Description |
|------------|-------------|
| `http_client(hosts)` | HTTP client access to specific hosts |
| `tcp_connect(addrs)` | TCP connections to specific addresses |
| `tcp_listen(port)` | TCP listener on a specific port |
| `dns_resolve()` | DNS resolution |

```rust
// HTTP access to specific APIs
.capability(Capability::http_client(vec![
    "api.example.com",
    "cdn.example.com",
]))

// TCP to specific addresses
.capability(Capability::tcp_connect(vec![
    "192.168.1.100:5432".parse().unwrap(),
]))

// DNS resolution
.capability(Capability::dns_resolve())
```

### Time and Random

| Capability | Description |
|------------|-------------|
| `system_clock()` | Access to system time (wall clock) |
| `monotonic_clock()` | Monotonic clock for measuring durations |
| `timers()` | Timer creation (sleep, intervals) |
| `secure_random()` | Cryptographic random numbers |
| `seeded_random(seed)` | Deterministic random with a seed |

```rust
.capability(Capability::system_clock())   // For timestamps
.capability(Capability::monotonic_clock()) // For duration measurement
.capability(Capability::secure_random())  // For crypto operations

// Deterministic random for testing
.capability(Capability::seeded_random(42))
```

### Host Functions

```rust
// Grant access to specific host functions
.capability(Capability::host_function("log"))

// Or entire namespaces
// Allows db::query, db::insert, etc.
```

## Capability Enforcement

Capabilities are enforced at runtime by the `CapabilityEnforcer`. When a capability is denied, an error is returned:

```rust
match sandbox.run(&[]).await {
    Err(Error::CapabilityDenied(cap)) => {
        eprintln!("Blocked operation: {:?}", cap);
    }
    // ...
}
```

## Audit Logging

All capability checks are logged for security auditing:

```rust
// Enable audit logging
tracing_subscriber::fmt()
    .with_env_filter("isolate::capability::audit=info")
    .init();
```

Log output:

```
INFO isolate::capability::audit: capability_granted sandbox=550e8400 capability=stdout
WARN isolate::capability::audit: capability_denied sandbox=550e8400 capability=filesystem_read path="/etc/passwd"
```

## Best Practices

### 1. Principle of Least Privilege

Only grant the minimum capabilities required:

```rust
// Bad: Grants everything
.capability(Capability::filesystem_read("/"))
.capability(Capability::filesystem_write("/"))

// Good: Grants only what's needed
.capability(Capability::filesystem_read("/app/config"))
.capability(Capability::filesystem_write("/tmp/app"))
```

### 2. Use Specific Paths

```rust
// Bad: Too broad
.capability(Capability::filesystem_read("/home"))

// Good: Specific directory
.capability(Capability::filesystem_read("/home/app/data"))
```

### 3. Validate Before Granting

```rust
// Validate user input before creating capabilities
fn create_config(user_path: &str) -> Result<SandboxConfig> {
    // Validate the path
    if !user_path.starts_with("/allowed/") {
        return Err(Error::InvalidCapability("path not allowed".into()));
    }

    SandboxConfig::builder()
        .module(&wasm_bytes)?
        .capability(Capability::filesystem_read(user_path))
        .build()
}
```

### 4. Document Required Capabilities

```rust
/// Runs the data processor module.
///
/// # Required Capabilities
/// - `stdout` - Progress output
/// - `filesystem_read("/data")` - Input data
/// - `filesystem_write("/output")` - Results
/// - `system_clock` - Timestamps
pub async fn run_processor(input: &[u8]) -> Result<Output> {
    // ...
}
```

## See Also

- [Security Model](./security-model) - How capabilities fit into defense-in-depth
- [Resource Limits](./resource-limits) - CPU and memory controls
- [Monitoring](./monitoring) - Tracking capability usage
