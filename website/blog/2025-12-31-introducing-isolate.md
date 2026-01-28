---
slug: introducing-isolate
title: Introducing Isolate - Secure Sandbox Runtime for WebAssembly
authors: [isolate-team]
tags: [announcement, wasm, security]
---

We're excited to introduce **Isolate**, a secure sandbox runtime for executing untrusted WebAssembly code with strong isolation guarantees.

<!-- truncate -->

## Why We Built Isolate

Running untrusted code is hard. Whether you're building a plugin system, a serverless platform, or a code playground, you need to balance functionality with security. Traditional approaches often fall short:

- **Containers** provide good isolation but have 100ms+ cold starts
- **microVMs** are secure but heavyweight (128MB+ memory overhead)
- **Raw WASM runtimes** are fast but require manual security implementation

Isolate bridges this gap by combining the speed of WebAssembly with built-in security controls.

## Key Features

### Sub-5ms Cold Start

Isolate creates new sandboxes in under 5 milliseconds (p99), making it suitable for latency-sensitive workloads like edge computing and serverless functions.

### Capability-Based Security

Unlike traditional sandboxes that block dangerous operations, Isolate uses a **default-deny** model. Code has no capabilities unless explicitly granted:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())
    .capability(Capability::filesystem_read("/data"))
    .build()?;
```

### Resource Limits

Prevent denial-of-service attacks with built-in controls:

- **CPU**: Fuel-based instruction metering
- **Memory**: Heap and stack limits
- **Time**: Wall-clock and CPU time limits
- **I/O**: Read and write byte quotas

### Multi-Language Support

Any language that compiles to WebAssembly works with Isolate: Rust, C/C++, Go, AssemblyScript, Python (via PyPy), and more.

## Getting Started

Add Isolate to your project:

```bash
cargo add isolate-core
```

Run your first sandbox:

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    let wasm_bytes = std::fs::read("module.wasm")?;

    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .memory_limit(64 * 1024 * 1024)
        .capability(Capability::stdout())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Output: {}", output.stdout_str());
    Ok(())
}
```

## What's Next

Isolate is currently at version 0.1.x. We're focused on:

1. **Stability**: Hardening the core API before 1.0
2. **Performance**: Further optimizing cold start times
3. **Features**: Snapshot/restore, distributed clustering, TEE integration

Check out the [documentation](/docs/) to get started, and join the discussion on [GitHub](https://github.com/josedab/isolate).

## Built on Wasmtime

Isolate is built on top of [Wasmtime](https://wasmtime.dev/), the industry-leading WebAssembly runtime by the Bytecode Alliance. We're grateful for their excellent work.

---

We'd love your feedback! Star us on [GitHub](https://github.com/josedab/isolate), try out the library, and let us know what you build.
