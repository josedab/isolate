# Isolate Embed

Minimal embeddable WASM sandbox runtime — a single-crate library for running WebAssembly modules in a secure sandbox with a fully synchronous API.

## Features

- **Zero async requirement**: Fully synchronous API — no tokio or async runtime needed
- **Minimal dependencies**: Only Wasmtime + thiserror
- **Simple API**: Create → Run → Get output
- **C FFI layer**: Optional `cffi` feature for embedding in C/C++ applications

## Installation

```toml
[dependencies]
isolate-embed = "0.1"

# Enable C FFI bindings
# isolate-embed = { version = "0.1", features = ["cffi"] }
```

## Quick Start

```rust,no_run
use isolate_embed::{Sandbox, SandboxConfig};

let wasm_bytes = std::fs::read("module.wasm").unwrap();
let config = SandboxConfig::new(&wasm_bytes)
    .memory_limit(64 * 1024 * 1024)  // 64MB
    .fuel(1_000_000);

let mut sandbox = Sandbox::create(config).unwrap();
let output = sandbox.run(&[]).unwrap();

println!("Exit code: {}", output.exit_code);
println!("Stdout: {}", output.stdout_str());
```

## Configuration

```rust,no_run
use isolate_embed::SandboxConfig;

# let wasm_bytes = vec![];
let config = SandboxConfig::new(&wasm_bytes)
    .memory_limit(128 * 1024 * 1024)   // 128MB heap limit
    .fuel(10_000_000)                   // Instruction fuel budget
    .allow_stdout(true)                 // Capture stdout
    .allow_stderr(true)                 // Capture stderr
    .env("KEY", "value")               // Set environment variable
    .arg("--verbose")                   // Add CLI argument
    .entry_point("main");              // Custom entry point (default: _start)
```

## When to Use

| Crate | Use Case |
|-------|----------|
| **isolate-embed** | Sync embedding in Rust/C/C++, minimal dependencies, no async runtime |
| **isolate-core** | Full-featured async sandbox with capabilities, metrics, pooling |

## API Documentation

Generate and view the full API docs:

```bash
cargo doc --package isolate-embed --open
```

## License

MIT OR Apache-2.0
