# Quick Start

This guide will have you running your first sandboxed WASM module in under 2 minutes.

## Step 1: Create a New Project

```bash
cargo new hello-isolate
cd hello-isolate
```

## Step 2: Add Dependencies

```bash
cargo add isolate-core tokio --features tokio/full
```

## Step 3: Write the Code

Replace `src/main.rs` with:

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

// A minimal "Hello World" WASM module (pre-compiled)
// In practice, you'd load a .wasm file from disk
const HELLO_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,  // WASM magic + version
    // ... (see full example in docs)
];

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Configure the sandbox
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)?
        .memory_limit(64 * 1024 * 1024)  // 64MB
        .capability(Capability::stdout())
        .build()?;

    // Create the sandbox
    let mut sandbox = Sandbox::create(config).await?;
    println!("Sandbox created: {}", sandbox.id());

    // Run the WASM module
    let output = sandbox.run(&[]).await?;

    // Print results
    println!("Exit code: {}", output.exit_code);
    println!("Duration: {:?}", output.duration);
    println!("Stdout: {}", output.stdout_str());

    Ok(())
}
```

## Step 4: Run It

```bash
cargo run
```

Expected output:

```
Sandbox created: 550e8400-e29b-41d4-a716-446655440000
Exit code: 0
Duration: 1.234ms
Stdout: Hello from WASM!
```

## Using the CLI

If you've installed the CLI, you can run WASM files directly:

```bash
# Run a WASM module
isolate run hello.wasm

# Run with capabilities
isolate run hello.wasm --cap-stdout --cap-stderr

# Run with resource limits
isolate run compute.wasm --memory-limit 128M --timeout 30s
```

## What's Next?

- [Your First Sandbox](./first-sandbox.md) - A more detailed walkthrough
- [Capabilities](../guide/capabilities.md) - Learn about the security model
- [Resource Limits](../guide/resource-limits.md) - Control CPU and memory usage

## Common Issues

### "WASM module too small"

The module must be at least 8 bytes (magic number + version). Ensure you're loading a valid `.wasm` file.

### "Invalid WASM magic number"

The file doesn't start with `\0asm`. Verify it's a compiled WebAssembly module, not source code.

### "Capability not granted"

WASM modules have no capabilities by default. Grant the required capabilities:

```rust
.capability(Capability::stdout())  // For printing
.capability(Capability::filesystem_read("/data"))  // For file access
```
