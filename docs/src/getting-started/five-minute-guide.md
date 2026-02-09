# Five-Minute Getting Started Guide

Everything you need to go from clone to running your first sandbox.

## Prerequisites

- **Rust 1.75.0+** — Install via [rustup](https://rustup.rs/)
- **Git** — For cloning the repository

No Python, no Docker, no other tools required.

## Step 1: Clone and Build (2 minutes)

```bash
git clone https://github.com/josedab/isolate.git
cd isolate
cargo build
```

> ⏱️ First build takes ~2 minutes. Subsequent builds take ~5 seconds.

## Step 2: Run the Built-In Example (instant)

```bash
cargo run --package isolate-core --example basic_sandbox
```

Expected output:

```
Exit code: 0
Stdout: Hello from WASM!
```

That's it — you just ran a WASM module in a secure, capability-controlled sandbox.

## Step 3: Run the Tests (30 seconds)

```bash
cargo test --package isolate-core
```

This runs ~100 core tests. To run all 1,700+ tests across all features:

```bash
cargo test --all-features
```

## Step 4: Write Your Own Sandbox

Create `examples/my_sandbox.rs` in `isolate-core/`:

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};
use std::time::Duration;

// Built-in test fixture: writes "Hello from WASM!\n" to stdout
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    let config = SandboxConfig::builder()
        .module(HELLO_WASM)?
        .memory_limit(64 * 1024 * 1024)  // 64MB
        .fuel(1_000_000)                  // CPU limit
        .wall_time_limit(Duration::from_secs(5))
        .capability(Capability::stdout()) // Grant stdout access
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit: {}, Output: {}", output.exit_code, output.stdout_str());
    Ok(())
}
```

Run it: `cargo run --package isolate-core --example my_sandbox`

## Step 5: Understand the Security Model

Sandboxes have **zero capabilities by default**. You must explicitly grant each permission:

| Capability | What It Allows | How to Grant |
|-----------|----------------|--------------|
| `Capability::stdout()` | Write to stdout | `.capability(Capability::stdout())` |
| `Capability::stderr()` | Write to stderr | `.capability(Capability::stderr())` |
| `Capability::filesystem_read(path)` | Read files at path | `.capability(Capability::filesystem_read("/data"))` |
| `Capability::filesystem_write(path)` | Write files at path | `.capability(Capability::filesystem_write("/tmp"))` |
| `Capability::http_client(hosts)` | HTTP requests to hosts | `.capability(Capability::http_client(vec!["api.example.com"]))` |
| `Capability::system_clock()` | Read system time | `.capability(Capability::system_clock())` |
| `Capability::secure_random()` | Cryptographic randomness | `.capability(Capability::secure_random())` |

If a module tries to use a capability it wasn't granted, you get a clear error:

```
Error: Capability not granted: Stdio(Stdout)
Suggestion: Grant stdio capability with --cap-stdout, --cap-stderr, or --cap-stdio for all.
```

## Common Errors and Fixes

| Error | Cause | Fix |
|-------|-------|-----|
| `Capability not granted: Stdio(Stdout)` | Module writes to stdout without permission | Add `.capability(Capability::stdout())` |
| `CPU fuel exhausted` | Module exceeded CPU limit | Increase `.fuel(10_000_000)` |
| `Execution timed out` | Module ran too long | Increase `.wall_time_limit(Duration::from_secs(60))` |
| `Memory limit exceeded` | Module allocated too much memory | Increase `.memory_limit(256 * 1024 * 1024)` |
| `WASM compilation error` | Invalid WASM binary | Verify file starts with `\0asm` magic bytes |

## What's Next?

- **[Capabilities Guide](../guide/capabilities.md)** — Deep dive into the permission system
- **[Resource Limits](../guide/resource-limits.md)** — CPU, memory, and I/O controls
- **[Error Handling](../reference/errors.md)** — Complete error reference with suggestions
- **[CLI Usage](../guide/cli.md)** — Run WASM modules from the command line

## Development Workflow

```bash
# Check everything before committing
just pre-commit

# Or manually:
cargo fmt --check          # Formatting
cargo clippy --all-features -- -D warnings  # Lints  
cargo test --all-features  # Tests

# Verify your environment
just doctor
```
