---
sidebar_position: 3
---

# Error Handling

Isolate uses a comprehensive error type that provides detailed information about failures. This guide covers error types, handling patterns, and best practices.

## Error Type

All fallible operations return `Result<T, Error>`:

```rust
use isolate_core::{Result, Error};

fn example() -> Result<()> {
    // Operations that can fail...
    Ok(())
}
```

## Error Variants

### InvalidModule

The WASM module is invalid or malformed.

```rust
Error::InvalidModule(String)
```

**Common causes:**
- File doesn't start with WASM magic number (`\0asm`)
- Module is corrupted or truncated
- Module uses unsupported WASM features

**Handling:**

```rust
match result {
    Err(Error::InvalidModule(msg)) => {
        eprintln!("Invalid WASM module: {}", msg);
        // Validate module before retrying
    }
    // ...
}
```

### Compilation

Module compilation failed.

```rust
Error::Compilation(String)
```

**Common causes:**
- Invalid WASM bytecode
- Unsupported instructions
- Type validation errors

### Execution

Runtime execution error.

```rust
Error::Execution(String)
```

**Common causes:**
- WASM trap (unreachable, division by zero, etc.)
- Stack overflow
- Out-of-bounds memory access

### FuelExhausted

CPU limit reached.

```rust
Error::FuelExhausted { limit: u64 }
```

**Handling:**

```rust
match result {
    Err(Error::FuelExhausted { limit }) => {
        eprintln!("CPU limit of {} fuel units exceeded", limit);
        // Consider increasing fuel limit or optimizing module
    }
    // ...
}
```

### MemoryLimitExceeded

Memory allocation failed due to limit.

```rust
Error::MemoryLimitExceeded { limit: usize, requested: usize }
```

**Handling:**

```rust
match result {
    Err(Error::MemoryLimitExceeded { limit, requested }) => {
        eprintln!(
            "Memory limit exceeded: requested {} bytes, limit is {} bytes",
            requested, limit
        );
        // Consider increasing memory limit
    }
    // ...
}
```

### Timeout

Wall clock timeout reached.

```rust
Error::Timeout(Duration)
```

**Handling:**

```rust
match result {
    Err(Error::Timeout(duration)) => {
        eprintln!("Execution timed out after {:?}", duration);
        // Consider increasing timeout or investigating slow code
    }
    // ...
}
```

### CapabilityDenied

Required capability was not granted.

```rust
Error::CapabilityDenied(String)
```

**Handling:**

```rust
match result {
    Err(Error::CapabilityDenied(cap)) => {
        eprintln!("Permission denied: {}", cap);
        // Grant the required capability if appropriate
    }
    // ...
}
```

### InvalidState

Operation invalid for current sandbox state.

```rust
Error::InvalidState { expected: String, actual: String }
```

**Handling:**

```rust
match result {
    Err(Error::InvalidState { expected, actual }) => {
        eprintln!("Invalid state: expected {}, got {}", expected, actual);
        // Check sandbox state before operations
    }
    // ...
}
```

### Io

I/O error occurred.

```rust
Error::Io(std::io::Error)
```

### Configuration

Configuration error.

```rust
Error::Configuration(String)
```

## Comprehensive Error Handling

### Pattern Matching

```rust
use isolate_core::Error;

match sandbox.run(&input).await {
    Ok(output) => {
        if output.success() {
            println!("Success: {}", output.stdout_str());
        } else {
            eprintln!("Module exited with code: {}", output.exit_code);
        }
    }

    // Resource limits
    Err(Error::FuelExhausted { limit }) => {
        log::warn!("CPU limit exceeded ({} fuel)", limit);
    }
    Err(Error::MemoryLimitExceeded { limit, requested }) => {
        log::warn!("Memory limit exceeded ({}/{} bytes)", requested, limit);
    }
    Err(Error::Timeout(duration)) => {
        log::warn!("Timeout after {:?}", duration);
    }

    // Security
    Err(Error::CapabilityDenied(cap)) => {
        log::warn!("Capability denied: {}", cap);
    }

    // Module issues
    Err(Error::InvalidModule(msg)) => {
        log::error!("Invalid module: {}", msg);
    }
    Err(Error::Compilation(msg)) => {
        log::error!("Compilation failed: {}", msg);
    }
    Err(Error::Execution(msg)) => {
        log::error!("Execution failed: {}", msg);
    }

    // Other
    Err(e) => {
        log::error!("Unexpected error: {}", e);
    }
}
```

### Using anyhow

```rust
use anyhow::{Context, Result};

async fn run_module(wasm_bytes: &[u8]) -> Result<String> {
    let config = SandboxConfig::builder()
        .module(wasm_bytes)
        .context("Invalid WASM module")?
        .capability(Capability::stdout())
        .build()
        .context("Failed to build configuration")?;

    let mut sandbox = Sandbox::create(config)
        .await
        .context("Failed to create sandbox")?;

    let output = sandbox.run(&[])
        .await
        .context("Sandbox execution failed")?;

    if output.success() {
        Ok(output.stdout_str())
    } else {
        anyhow::bail!("Module exited with code {}", output.exit_code)
    }
}
```

### Custom Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Sandbox error: {0}")]
    Sandbox(#[from] isolate_core::Error),

    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

fn run_module(name: &str) -> Result<(), AppError> {
    let wasm_bytes = std::fs::read(format!("{}.wasm", name))
        .map_err(|_| AppError::ModuleNotFound(name.to_string()))?;

    // Isolate errors are automatically converted
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .build()?;

    Ok(())
}
```

## Error Recovery

### Retry Logic

```rust
async fn run_with_retry(
    config: SandboxConfig,
    max_retries: u32,
) -> Result<Output> {
    let mut attempts = 0;

    loop {
        let config_clone = config.clone();
        let mut sandbox = Sandbox::create(config_clone).await?;

        match sandbox.run(&[]).await {
            Ok(output) => return Ok(output),
            Err(Error::Timeout(_)) if attempts < max_retries => {
                attempts += 1;
                log::warn!("Timeout, retrying ({}/{})", attempts, max_retries);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}
```

### Fallback

```rust
async fn run_with_fallback(primary: &[u8], fallback: &[u8]) -> Result<Output> {
    let primary_config = SandboxConfig::builder()
        .module(primary)?
        .capability(Capability::stdout())
        .build()?;

    match Sandbox::create(primary_config).await?.run(&[]).await {
        Ok(output) if output.success() => Ok(output),
        _ => {
            log::info!("Primary module failed, using fallback");
            let fallback_config = SandboxConfig::builder()
                .module(fallback)?
                .capability(Capability::stdout())
                .build()?;
            Sandbox::create(fallback_config).await?.run(&[]).await
        }
    }
}
```

## Best Practices

1. **Always handle errors explicitly** - Don't use `unwrap()` in production
2. **Log errors with context** - Include sandbox ID, module hash, etc.
3. **Distinguish recoverable from fatal errors** - Timeouts may be retried
4. **Monitor error rates** - Track errors in metrics for alerting
5. **Provide user-friendly messages** - Map internal errors to meaningful responses

## See Also

- [Troubleshooting](./troubleshooting) - Common issues and solutions
- [Monitoring](../guides/monitoring) - Error tracking and alerting
- [Resource Limits](../guides/resource-limits) - Preventing resource errors
