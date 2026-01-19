# Error Handling

Comprehensive guide to handling errors in Isolate.

## Error Types

### Resource Limit Errors

```rust
// CPU fuel exhausted
Error::FuelExhausted { limit: 1_000_000 }

// Memory allocation failed
Error::MemoryLimitExceeded {
    limit: 134_217_728,     // 128MB
    requested: 268_435_456,  // 256MB
}

// Execution timeout
Error::Timeout(Duration::from_secs(30))
```

### Capability Errors

```rust
// Capability not granted
Error::CapabilityDenied(Capability::stdout())

// Filesystem access blocked
Error::FilesystemAccessDenied {
    path: PathBuf::from("/etc/passwd")
}

// Network access blocked
Error::NetworkAccessDenied {
    host: "evil.com".to_string()
}
```

### Module Errors

```rust
// Invalid WASM
Error::ModuleValidation("Invalid magic number".to_string())

// Compilation failed
Error::Compilation("Unknown import: foo".to_string())

// Instantiation failed
Error::Instantiation("Missing memory export".to_string())
```

### Runtime Errors

```rust
// Execution error (trap, panic, etc.)
Error::Execution("unreachable executed".to_string())

// Function not found
Error::FunctionNotFound("main".to_string())

// Wrong function signature
Error::InvalidSignature {
    name: "add".to_string(),
    expected: "(i32, i32) -> i32".to_string(),
    actual: "(i32) -> i32".to_string(),
}
```

### State Errors

```rust
// Wrong sandbox state
Error::InvalidState {
    expected: "Ready".to_string(),
    actual: "Terminated".to_string(),
}
```

## Error Categorization

Use helper methods to categorize errors:

```rust
let error: Error = /* ... */;

if error.is_timeout() {
    // Handle timeout specifically
}

if error.is_resource_limit() {
    // CPU, memory, or timeout
}

if error.is_capability_error() {
    // Permission denied
}
```

## Handling Patterns

### Match on Specific Errors

```rust
match sandbox.run(&input).await {
    Ok(output) => {
        if output.success() {
            println!("Success: {}", output.stdout_str());
        } else {
            println!("Failed with exit code: {}", output.exit_code);
        }
    }

    Err(Error::Timeout(duration)) => {
        eprintln!("Timed out after {:?}", duration);
        // Consider increasing timeout or optimizing module
    }

    Err(Error::FuelExhausted { limit }) => {
        eprintln!("CPU limit exceeded: {} fuel", limit);
        // Consider increasing fuel or optimizing module
    }

    Err(Error::MemoryLimitExceeded { limit, requested }) => {
        eprintln!("Memory limit: requested {} of {} bytes", requested, limit);
        // Consider increasing memory limit
    }

    Err(Error::CapabilityDenied(cap)) => {
        eprintln!("Permission denied: {:?}", cap);
        // Grant the required capability
    }

    Err(e) => {
        eprintln!("Unexpected error: {}", e);
    }
}
```

### Using anyhow for Context

```rust
use anyhow::{Context, Result};

async fn process(input: &[u8]) -> Result<String> {
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)
        .context("Invalid WASM module")?
        .build()
        .context("Invalid configuration")?;

    let mut sandbox = Sandbox::create(config)
        .await
        .context("Failed to create sandbox")?;

    let output = sandbox.run(input)
        .await
        .context("Execution failed")?;

    Ok(output.stdout_str())
}
```

### Custom Error Types

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Sandbox error: {0}")]
    Sandbox(#[from] isolate_core::Error),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Module not found: {0}")]
    ModuleNotFound(String),
}

fn run_module(name: &str, input: &[u8]) -> Result<Output, AppError> {
    let wasm = load_module(name)
        .ok_or_else(|| AppError::ModuleNotFound(name.to_string()))?;

    // ...
}
```

## Retry Strategies

### Transient Errors

```rust
async fn run_with_retry(
    sandbox: &mut Sandbox,
    input: &[u8],
    max_retries: u32,
) -> Result<Output> {
    let mut last_error = None;

    for attempt in 0..max_retries {
        match sandbox.run(input).await {
            Ok(output) => return Ok(output),

            Err(Error::Timeout(_)) if attempt < max_retries - 1 => {
                // Retry on timeout
                tracing::warn!("Attempt {} timed out, retrying...", attempt + 1);
                last_error = Some(Error::Timeout(Duration::default()));
                continue;
            }

            Err(e) => return Err(e),
        }
    }

    Err(last_error.unwrap())
}
```

### Exponential Backoff

```rust
async fn run_with_backoff(config: SandboxConfig, input: &[u8]) -> Result<Output> {
    let mut delay = Duration::from_millis(100);

    for attempt in 0..5 {
        let mut sandbox = Sandbox::create(config.clone()).await?;

        match sandbox.run(input).await {
            Ok(output) => return Ok(output),
            Err(e) if e.is_resource_limit() => {
                tracing::warn!("Attempt {} failed: {}, backing off", attempt + 1, e);
                tokio::time::sleep(delay).await;
                delay *= 2;  // Exponential backoff
            }
            Err(e) => return Err(e),
        }
    }

    Err(Error::Execution("Max retries exceeded".into()))
}
```

## Logging Errors

```rust
use tracing::{error, warn, info};

match sandbox.run(input).await {
    Ok(output) => {
        info!(
            sandbox_id = %sandbox.id(),
            exit_code = output.exit_code,
            duration_ms = output.duration.as_millis(),
            "Execution completed"
        );
    }
    Err(e) if e.is_resource_limit() => {
        warn!(
            sandbox_id = %sandbox.id(),
            error = %e,
            "Resource limit exceeded"
        );
    }
    Err(e) => {
        error!(
            sandbox_id = %sandbox.id(),
            error = %e,
            "Execution failed"
        );
    }
}
```

## See Also

- [API Reference](./api.md)
- [Troubleshooting](./troubleshooting.md)
