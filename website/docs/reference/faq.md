---
sidebar_position: 7
---

# FAQ

Frequently asked questions about Isolate.

## General

### What is Isolate?

Isolate is a secure sandbox runtime for executing untrusted WebAssembly (WASM) code. It provides:

- **Capability-based security** with default-deny permissions
- **Resource limits** to control CPU, memory, and I/O usage
- **Sub-5ms cold start** for latency-sensitive applications
- **Production tooling** including metrics, tracing, and audit logging

### When should I use Isolate?

Isolate is ideal for scenarios where you need to run untrusted code safely:

- **Plugin systems** - Run third-party plugins without risking your application
- **Serverless functions** - Execute user-provided code in multi-tenant environments
- **Code sandboxing** - Safely run code snippets for testing, education, or CI/CD
- **Edge computing** - Deploy lightweight, isolated workloads close to users

### How does Isolate compare to containers?

| Aspect | Isolate | Containers |
|--------|---------|------------|
| Cold start | &lt;5ms | 100ms-1s+ |
| Memory overhead | &lt;5MB | 10-100MB |
| Isolation | WASM sandbox | Linux namespaces |
| Capabilities | Fine-grained | Coarse (CAP_*) |
| Languages | Any WASM-compiled | Any |

Containers provide stronger isolation (kernel-level) but with higher overhead. Isolate is better for high-density, latency-sensitive workloads.

### Is Isolate production-ready?

Isolate is currently at version 0.1.x and is suitable for experimentation and early adoption. The core API is stable, but some features are still evolving. We recommend:

- Pinning exact versions in production
- Testing thoroughly before deploying
- Monitoring resource usage closely

## Installation

### What are the system requirements?

- **Rust 1.70+** for building from source
- **Any OS** that Wasmtime supports (Linux, macOS, Windows)
- **tokio** runtime for async execution

### How do I install Isolate?

**As a library:**
```bash
cargo add isolate-core
```

**CLI tool:**
```bash
cargo install isolate-cli
```

**gRPC server:**
```bash
cargo install isolate-server
```

### Why is compilation slow?

Isolate depends on Wasmtime, which is a large crate. First-time compilation takes longer, but subsequent builds are cached. Use `cargo build --release` for optimized builds.

## WASM Modules

### What languages can I use?

Any language that compiles to WebAssembly:

- **Rust** (first-class support via `wasm32-wasi` target)
- **C/C++** (via Emscripten or WASI SDK)
- **Go** (via TinyGo)
- **AssemblyScript**
- **Python** (via MicroPython or Pyodide)
- **JavaScript** (via wasm-bindgen)

### How do I compile Rust to WASM?

```bash
# Install the target
rustup target add wasm32-wasi

# Build your project
cargo build --target wasm32-wasi --release

# Output is in target/wasm32-wasi/release/your_crate.wasm
```

### Why does my module fail with "Invalid WASM magic number"?

The file doesn't start with the WASM magic bytes (`\0asm`). Common causes:

1. **Loading source code instead of compiled WASM** - Make sure you're loading the `.wasm` file
2. **File path typo** - Verify the file exists at the specified path
3. **Corrupted download** - Re-download or re-compile the module

### Can I use WASI preview2?

Not yet. Isolate currently supports **WASI preview1** only. Preview2 support is planned for a future release. Enable it experimentally with the `wasi-preview2` feature flag.

## Capabilities

### Why is my module getting "Capability denied"?

WASM modules have **no capabilities by default**. You must explicitly grant each capability:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())  // Grant stdout
    .capability(Capability::filesystem_read("/data"))  // Grant fs read
    .build()?;
```

### What capabilities does my module need?

Analyze what your module does:

| Operation | Required Capability |
|-----------|---------------------|
| Print to stdout | `Capability::stdout()` |
| Print to stderr | `Capability::stderr()` |
| Read stdin | `Capability::stdin()` |
| Read files | `Capability::filesystem_read(path)` |
| Write files | `Capability::filesystem_write(path)` |
| Get current time | `Capability::system_clock()` |
| Generate random numbers | `Capability::secure_random()` |
| HTTP requests | `Capability::http_client(hosts)` |
| Read env vars | `Capability::env_var(name)` |

### Can I grant all capabilities?

There's no "grant all" capability by design. This is intentional for security. If you truly need broad access, grant each capability explicitly:

```rust
.capability(Capability::stdout())
.capability(Capability::stderr())
.capability(Capability::stdin())
.capability(Capability::filesystem_read("/"))
.capability(Capability::filesystem_write("/tmp"))
// etc.
```

:::warning
Granting broad filesystem access defeats the purpose of sandboxing. Consider what your module actually needs.
:::

## Resource Limits

### How do I prevent infinite loops?

Use **fuel metering**. Fuel is consumed as instructions execute:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .fuel(10_000_000)  // ~10M instructions
    .build()?;
```

When fuel is exhausted, execution stops with `Error::FuelExhausted`.

### How do I set a timeout?

Use **wall clock timeout** for real-time limits:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .wall_time_limit(Duration::from_secs(30))  // 30 second max
    .build()?;
```

Or use **CPU time limit** to limit actual CPU consumption:

```rust
.cpu_time_limit(Duration::from_secs(10))  // 10 seconds CPU time
```

### What happens when a limit is exceeded?

The execution is interrupted and an error is returned:

- **Fuel exhausted**: `Error::FuelExhausted { limit }`
- **Memory exceeded**: `Error::MemoryLimitExceeded { limit, requested }`
- **Timeout**: `Error::Timeout(duration)`

### How much fuel should I allocate?

It depends on your workload. Start with these guidelines:

| Workload Type | Suggested Fuel |
|---------------|----------------|
| Simple computation | 1,000,000 |
| Data processing | 10,000,000 |
| Complex algorithms | 100,000,000 |
| Long-running tasks | 1,000,000,000+ |

Monitor actual usage with metrics and adjust accordingly.

## Performance

### Why is the first execution slow?

The first execution compiles the WASM module to native code. Use a shared `WasmEngine` to cache compiled modules:

```rust
let engine = Arc::new(WasmEngine::new()?);

// First execution compiles the module
let sandbox1 = Sandbox::create_with_engine(config1, engine.clone()).await?;

// Subsequent executions use cached compilation
let sandbox2 = Sandbox::create_with_engine(config2, engine.clone()).await?;
```

### How do I improve cold start time?

1. **Share the engine** across sandboxes (see above)
2. **Pre-warm** sandboxes before they're needed
3. **Use smaller modules** with fewer imports
4. **Enable compilation caching** at the Wasmtime level

### How do I measure performance?

Use the built-in metrics:

```rust
// Resource usage in output
let output = sandbox.run(&[]).await?;
println!("Duration: {:?}", output.duration);
println!("Fuel consumed: {:?}", output.resource_usage.fuel_consumed);
println!("Memory peak: {} bytes", output.resource_usage.memory_peak);
```

Or enable Prometheus metrics:

```rust
// Metrics are automatically exposed
// sandbox_execution_duration_seconds
// sandbox_fuel_consumed
// sandbox_memory_bytes
```

## Debugging

### How do I see what my module is doing?

1. **Grant stdout/stderr capabilities** to see output:
   ```rust
   .capability(Capability::stdout())
   .capability(Capability::stderr())
   ```

2. **Enable audit logging** to see capability checks:
   ```rust
   tracing_subscriber::fmt()
       .with_env_filter("isolate::capability::audit=debug")
       .init();
   ```

3. **Check resource usage** in the output:
   ```rust
   println!("Fuel: {:?}", output.resource_usage.fuel_consumed);
   println!("I/O read: {} bytes", output.resource_usage.io_read);
   ```

### Why does my module exit with code 1?

Exit code 1 typically means the module encountered an error. Check stderr:

```rust
let output = sandbox.run(&[]).await?;
if output.exit_code != 0 {
    eprintln!("Module error: {}", output.stderr_str());
}
```

Common causes:
- Missing capability (check audit logs)
- File not found (verify paths)
- Invalid input data
- Panic in the module (check stderr for panic message)

### How do I trace execution?

Enable OpenTelemetry tracing with the `otel-telemetry` feature:

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["otel-telemetry"] }
```

Then configure a tracing exporter (Jaeger, Zipkin, OTLP).

## gRPC Server

### How do I run the gRPC server?

```bash
# Default (localhost:50051)
isolate-server

# Custom address
isolate-server --addr 0.0.0.0:50051

# With TLS
isolate-server --tls-cert server.crt --tls-key server.key
```

### What clients are available?

- **Go SDK**: `go get github.com/josedab/isolate/sdk/go`
- **TypeScript SDK**: `npm install @isolate/client`
- **Any gRPC client**: Use the proto file at `isolate-server/proto/isolate.proto`

### How do I monitor the server?

The server exposes Prometheus metrics at the `/metrics` endpoint:

```bash
curl http://localhost:9090/metrics
```

Or via gRPC:

```bash
grpcurl -plaintext localhost:50051 isolate.v1.IsolateService/GetMetrics
```

## Troubleshooting

### "module has no default export"

The WASM module needs a `_start` function (WASI entry point). For Rust:

```rust
fn main() {
    // Your code
}
```

Make sure you're compiling with `--target wasm32-wasi`.

### "memory limit exceeded during instantiation"

The module requires more memory than allowed. Increase the memory limit:

```rust
.memory_limit(256 * 1024 * 1024)  // 256MB
```

### "connection refused" with gRPC client

1. Verify the server is running: `ps aux | grep isolate-server`
2. Check the port is correct: `netstat -an | grep 50051`
3. Verify firewall rules allow the connection
4. Check if TLS is required but not configured

### "unknown capability type"

You're using a capability that doesn't exist or is misspelled. Check the [Capabilities guide](../guides/capabilities) for valid capability types.

## Getting Help

### Where can I ask questions?

- **GitHub Discussions**: [github.com/josedab/isolate/discussions](https://github.com/josedab/isolate/discussions)
- **GitHub Issues**: For bugs and feature requests
- **Stack Overflow**: Tag with `isolate-wasm`

### How do I report a bug?

Open an issue at [github.com/josedab/isolate/issues](https://github.com/josedab/isolate/issues) with:

1. Isolate version (`cargo pkgid isolate-core`)
2. Rust version (`rustc --version`)
3. Operating system
4. Minimal reproduction case
5. Expected vs actual behavior

### How do I contribute?

See the [Contributing guide](../contributing) for:

- Code contribution guidelines
- Development setup
- Testing requirements
- Pull request process
