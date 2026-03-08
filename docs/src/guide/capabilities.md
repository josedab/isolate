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

```rust
// Read-only access to /data
.capability(Capability::filesystem_read("/data"))

// Read-write access to /tmp/sandbox
.capability(Capability::filesystem_read("/tmp/sandbox"))
.capability(Capability::filesystem_write("/tmp/sandbox"))
```

**Important:** Paths are strictly enforced. Access to `/data` does NOT grant access to `/data-backup`.

### Environment Variables

```rust
// Grant access to specific environment variables
.capability(Capability::env_var("API_KEY"))
.capability(Capability::env_var("CONFIG_PATH"))

// Set the values
.env("API_KEY", "secret123")
.env("CONFIG_PATH", "/etc/app/config.json")
```

### Network

| Capability | Description |
|------------|-------------|
| `http_client(hosts)` | HTTP client access to specific hosts |
| `tcp_connect(addrs)` | TCP connections to specific addresses |
| `dns_resolve()` | DNS resolution |

```rust
// HTTP access to specific APIs
.capability(Capability::http_client(vec![
    "api.example.com".to_string(),
    "cdn.example.com".to_string(),
]))

// DNS resolution
.capability(Capability::dns_resolve())
```

### System

| Capability | Description |
|------------|-------------|
| `system_clock()` | Access to system time |
| `monotonic_clock()` | Monotonic clock for durations |
| `secure_random()` | Cryptographic random numbers |

```rust
.capability(Capability::system_clock())   // For timestamps
.capability(Capability::secure_random())  // For crypto operations
```

## Capability Enforcement

Capabilities are enforced at runtime by the `CapabilityEnforcer`:

```rust
// Internal to Isolate - you don't call this directly
enforcer.check_stdout()?;           // Before stdout write
enforcer.check_fs_read(&path)?;     // Before file read
enforcer.check_http(&url)?;         // Before HTTP request
```

When a capability is denied, an error is returned:

```rust
match sandbox.run(&[]).await {
    Err(Error::CapabilityDenied(cap)) => {
        eprintln!("Blocked: {:?}", cap);
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
/// - `clock` - Timestamps
pub async fn run_processor(input: &[u8]) -> Result<Output> {
    // ...
}
```

## Custom Capabilities

For advanced use cases, you can extend the capability system:

```rust
// Define custom capability types
#[derive(Debug, Clone)]
pub enum CustomCapability {
    DatabaseQuery { tables: Vec<String> },
    MessageQueue { topics: Vec<String> },
}

// Implement enforcement in your host functions
fn check_database_access(cap: &CustomCapability, table: &str) -> bool {
    match cap {
        CustomCapability::DatabaseQuery { tables } => tables.contains(&table.to_string()),
        _ => false,
    }
}
```

## See Also

- [Security Model](./security-model.md) - How capabilities fit into defense-in-depth
- [Resource Limits](./resource-limits.md) - CPU and memory controls
- [Monitoring](./monitoring.md) - Tracking capability usage
