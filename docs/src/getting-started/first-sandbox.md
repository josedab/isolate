# Your First Sandbox

This tutorial walks through creating a complete sandboxed application with proper error handling, resource limits, and capabilities.

## The Scenario

We'll create a sandbox that:

1. Runs a WASM module that processes input data
2. Has read-only access to a configuration file
3. Can write to stdout
4. Has strict resource limits
5. Includes proper error handling

## Step 1: Project Setup

```bash
cargo new sandbox-demo
cd sandbox-demo
cargo add isolate-core tokio anyhow --features tokio/full
```

## Step 2: The Code

```rust
use anyhow::{Context, Result};
use isolate_core::{
    Sandbox, SandboxConfig, Output,
    capability::Capability,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging (optional but recommended)
    tracing_subscriber::fmt()
        .with_env_filter("isolate=info")
        .init();

    // Load your WASM module
    let wasm_bytes = std::fs::read("processor.wasm")
        .context("Failed to read WASM module")?;

    // Build the configuration
    let config = build_config(&wasm_bytes)?;

    // Create and run the sandbox
    let output = run_sandbox(config).await?;

    // Process the output
    handle_output(&output)?;

    Ok(())
}

fn build_config(wasm_bytes: &[u8]) -> Result<SandboxConfig> {
    SandboxConfig::builder()
        // Load the module
        .module(wasm_bytes)
        .context("Invalid WASM module")?

        // Memory limits
        .memory_limit(128 * 1024 * 1024)  // 128MB heap
        .stack_size(1024 * 1024)           // 1MB stack

        // CPU limits
        .fuel(10_000_000)                  // 10M instructions max
        .cpu_time_limit(Duration::from_secs(30))

        // Timeout (wall clock)
        .wall_time_limit(Duration::from_secs(60))

        // I/O limits
        .io_read_limit(10 * 1024 * 1024)   // 10MB read
        .io_write_limit(1024 * 1024)       // 1MB write

        // Capabilities (default deny - only grant what's needed)
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .capability(Capability::filesystem_read("/etc/processor"))
        .capability(Capability::clock())    // Allow time access

        // Environment variables (only specific ones)
        .capability(Capability::env_var("CONFIG_PATH"))
        .env("CONFIG_PATH", "/etc/processor/config.json")

        // Command-line arguments
        .arg("--verbose".to_string())
        .arg("--format=json".to_string())

        .build()
        .context("Failed to build sandbox config")
}

async fn run_sandbox(config: SandboxConfig) -> Result<Output> {
    // Create the sandbox
    let mut sandbox = Sandbox::create(config)
        .await
        .context("Failed to create sandbox")?;

    println!("Created sandbox: {}", sandbox.id());
    println!("Module hash: {}", sandbox.module_hash());

    // Prepare input data
    let input = b"process this data";

    // Run the sandbox
    let output = sandbox.run(input)
        .await
        .context("Sandbox execution failed")?;

    // Get metrics
    let metrics = sandbox.metrics();
    println!("Run count: {}", metrics.run_count());

    Ok(output)
}

fn handle_output(output: &Output) -> Result<()> {
    println!("\n=== Execution Results ===");
    println!("Exit code: {}", output.exit_code);
    println!("Duration: {:?}", output.duration);

    // Resource usage
    println!("\n=== Resource Usage ===");
    println!("Fuel consumed: {:?}", output.resource_usage.fuel_consumed);
    println!("Memory peak: {} bytes", output.resource_usage.memory_peak);
    println!("I/O read: {} bytes", output.resource_usage.io_read);
    println!("I/O write: {} bytes", output.resource_usage.io_write);

    // Output streams
    if !output.stdout.is_empty() {
        println!("\n=== Stdout ===");
        println!("{}", output.stdout_str());
    }

    if !output.stderr.is_empty() {
        println!("\n=== Stderr ===");
        eprintln!("{}", output.stderr_str());
    }

    // Check for errors
    if !output.success() {
        anyhow::bail!("Sandbox exited with code {}", output.exit_code);
    }

    Ok(())
}
```

## Step 3: Error Handling

Isolate uses a comprehensive error type. Here's how to handle specific errors:

```rust
use isolate_core::error::Error;

match sandbox.run(input).await {
    Ok(output) => { /* success */ }
    Err(Error::Timeout(duration)) => {
        eprintln!("Execution timed out after {:?}", duration);
    }
    Err(Error::FuelExhausted { limit }) => {
        eprintln!("CPU limit exceeded ({} fuel units)", limit);
    }
    Err(Error::MemoryLimitExceeded { limit, requested }) => {
        eprintln!("Memory limit exceeded: requested {} of {} bytes", requested, limit);
    }
    Err(Error::CapabilityDenied(cap)) => {
        eprintln!("Permission denied: {:?}", cap);
    }
    Err(e) => {
        eprintln!("Unexpected error: {}", e);
    }
}
```

## Step 4: Testing Your Sandbox

Create a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const TEST_WASM: &[u8] = include_bytes!("../test_fixtures/test.wasm");

    #[tokio::test]
    async fn test_sandbox_success() {
        let config = SandboxConfig::builder()
            .module(TEST_WASM).unwrap()
            .capability(Capability::stdout())
            .build().unwrap();

        let mut sandbox = Sandbox::create(config).await.unwrap();
        let output = sandbox.run(&[]).await.unwrap();

        assert!(output.success());
    }

    #[tokio::test]
    async fn test_memory_limit_enforced() {
        let config = SandboxConfig::builder()
            .module(TEST_WASM).unwrap()
            .memory_limit(1024)  // Very small
            .build().unwrap();

        let mut sandbox = Sandbox::create(config).await.unwrap();
        let result = sandbox.run(&[]).await;

        assert!(matches!(result, Err(Error::MemoryLimitExceeded { .. })));
    }
}
```

## Best Practices

1. **Always set resource limits** - Never run untrusted code without limits
2. **Use minimum capabilities** - Only grant what's absolutely necessary
3. **Validate WASM before loading** - Check file size, magic number
4. **Log sandbox events** - Use tracing for observability
5. **Handle all error cases** - Don't unwrap in production code

## Next Steps

- Learn about [Capabilities](../guide/capabilities.md) in depth
- Configure [Resource Limits](../guide/resource-limits.md)
- Set up [Monitoring](../guide/monitoring.md)
