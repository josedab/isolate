# CLAUDE.md - Isolate Project Guide

This file provides guidance for AI assistants working with the Isolate codebase.

## Project Overview

**Isolate** is a secure sandbox runtime for executing untrusted WebAssembly (WASM) code with strong isolation guarantees. It combines WASM isolation with capability-based security and resource controls.

### Key Features
- Fast cold start (<5ms sandbox creation)
- Memory-safe Rust implementation
- Multi-language support (any WASM-compiled language)
- Capability-based fine-grained permissions
- Resource limits (CPU, memory, I/O) with enforcement
- Epoch-based timeout interruption

## Project Structure

```
isolate/
├── Cargo.toml              # Workspace root
├── isolate-core/           # Core library (main crate)
│   ├── src/
│   │   ├── lib.rs          # Crate root, re-exports
│   │   ├── sandbox.rs      # Sandbox lifecycle management
│   │   ├── config.rs       # Configuration and builders
│   │   ├── error.rs        # Error types
│   │   ├── metrics.rs      # Prometheus metrics
│   │   ├── capability/     # Capability-based security
│   │   │   ├── mod.rs
│   │   │   ├── types.rs    # Capability definitions
│   │   │   ├── enforcer.rs # Permission checking
│   │   │   └── audit.rs    # Audit logging
│   │   ├── engine/         # WASM execution engine
│   │   │   ├── mod.rs
│   │   │   ├── wasm.rs     # Wasmtime integration
│   │   │   ├── capture.rs  # I/O stream capture
│   │   │   └── host.rs     # Host function registry
│   │   ├── resource/       # Resource management
│   │   │   ├── mod.rs
│   │   │   ├── limits.rs   # Limit definitions
│   │   │   └── metering.rs # Usage tracking
│   │   └── snapshot/       # Snapshot/restore (WIP)
│   │       ├── mod.rs
│   │       └── pool.rs
│   └── tests/
│       ├── integration_test.rs
│       └── fixtures/       # WASM test fixtures
│           ├── minimal.wasm
│           ├── hello.wasm
│           └── exit_42.wasm
├── isolate-server/         # gRPC server
│   ├── src/
│   │   ├── main.rs
│   │   └── service.rs
│   └── proto/              # Protocol buffers
└── isolate-cli/            # CLI tool
    └── src/main.rs
```

## Build & Test Commands

```bash
# Build all crates
cargo build

# Build release
cargo build --release

# Run all tests
cargo test

# Run tests for core crate only
cargo test --package isolate-core

# Run specific test
cargo test test_sandbox_execution

# Check compilation without building
cargo check

# Run clippy lints
cargo clippy

# Format code
cargo fmt
```

## Architecture

### Core Components

1. **Sandbox** (`sandbox.rs`) - Main API for creating and running isolated WASM code
2. **WasmEngine** (`engine/wasm.rs`) - Wasmtime-based WASM execution with module caching
3. **CapabilityEnforcer** (`capability/enforcer.rs`) - Permission checking for all operations
4. **ResourceMeter** (`resource/metering.rs`) - Tracks and enforces resource limits

### Execution Flow

```
1. SandboxConfig::builder() → Configure sandbox
2. Sandbox::create(config) → Compile WASM, create instance
3. sandbox.run(&input) → Execute with timeout monitoring
4. Output { exit_code, stdout, stderr, resource_usage }
```

### Key Dependencies

- **wasmtime 27** - WASM runtime
- **wasmtime-wasi 27** - WASI preview1 support
- **tokio** - Async runtime
- **thiserror 2.0** - Error handling

## Code Conventions

### Error Handling
- Use `thiserror` for error definitions in `error.rs`
- All fallible functions return `Result<T, Error>` (aliased as `Result<T>`)
- Never panic in library code; use proper error propagation

### Resource Management
- `StoreLimits` for memory enforcement
- `ResourceMeter` for tracking I/O and fuel consumption
- Epoch-based interruption for timeouts (10ms tick interval)

### Capability System
Capabilities are granted explicitly and checked at runtime:
```rust
// Grant capabilities
.capability(Capability::stdout())
.capability(Capability::filesystem_read("/data"))

// Enforcer checks at runtime
enforcer.check_stdout()?;
enforcer.check_fs_read(path)?;
```

### WASI Configuration
- Stdin: `BufferedStdin` or `EmptyStdin`
- Stdout/Stderr: `CaptureStream` or `NullStream`
- Filesystem: Preopened directories based on capabilities
- I/O metering integrated into streams

## Testing

### Test Fixtures
Located in `isolate-core/tests/fixtures/`:
- `minimal.wasm` - Minimal WASM that exits with 0
- `hello.wasm` - Writes "Hello from WASM!\n" to stdout
- `exit_42.wasm` - Exits with code 42

### Writing Tests
```rust
#[tokio::test]
async fn test_example() {
    let config = SandboxConfig::builder()
        .module(WASM_BYTES)
        .expect("valid module")
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()
        .expect("valid config");

    let mut sandbox = Sandbox::create(config).await.expect("creation");
    let output = sandbox.run(&[]).await.expect("execution");

    assert_eq!(output.exit_code, 0);
}
```

## Common Tasks

### Adding a New Capability
1. Add variant to `FilesystemCapability`, `NetworkCapability`, etc. in `capability/types.rs`
2. Add check method to `CapabilityEnforcer` in `capability/enforcer.rs`
3. Wire into WASI context in `engine/wasm.rs`

### Modifying Resource Limits
1. Update structs in `resource/limits.rs`
2. Update metering in `resource/metering.rs`
3. Apply limits in `engine/wasm.rs` (StoreLimits, fuel, etc.)

### Adding WASM Test Fixtures
Create using Python with raw WASM binary format:
```python
# See existing fixtures for examples
# Key sections: Type, Import, Function, Memory, Export, Code
```

## Important Notes

- The project uses WASI **preview1** (not preview2)
- Epoch interruption requires a background task to increment epochs
- Module compilation is cached in `WasmEngine` for performance
- I/O limits are enforced in the stream implementations, not WASI layer

## Current Test Status

- 64 unit tests
- 14 integration tests
- 2 doc tests
- **Total: 80 tests passing**
