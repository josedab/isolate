# ADR-0001: Wasmtime as WASM Runtime

## Status

Accepted

## Context

Isolate requires a WebAssembly runtime capable of executing untrusted code with strong isolation guarantees. The runtime must support:

- WASI (WebAssembly System Interface) for standardized system access
- Deterministic execution limits (timeout and CPU metering)
- Memory isolation and bounds checking
- Efficient compilation and caching
- Production-ready stability and security track record

Several WASM runtimes were considered: Wasmtime, Wasmer, WasmEdge, and WAMR. The choice would fundamentally shape Isolate's security model, performance characteristics, and maintenance burden.

## Decision

We selected **Wasmtime 27** as the WASM runtime with WASI preview1 support.

Key factors in this decision:

1. **Epoch-based interruption**: Wasmtime provides epoch counters that can be incremented from a background thread, enabling cooperative timeout enforcement without polling. We use a 10ms tick interval for responsive timeout detection.

2. **Fuel-based CPU metering**: Wasmtime's fuel mechanism charges approximately 1 fuel unit per WASM instruction, providing deterministic CPU limiting that cannot be bypassed by infinite loops or CPU-intensive operations.

3. **Cranelift compiler**: Wasmtime uses Cranelift for JIT compilation, offering good performance with compile-time bounds checking that eliminates runtime overhead for memory safety.

4. **WASI preview1 maturity**: While WASI preview2 (Component Model) offers more features, preview1 is stable, widely supported, and sufficient for Isolate's current requirements.

5. **Bytecode Alliance backing**: Wasmtime is developed by the Bytecode Alliance with strong security focus, regular audits, and long-term support commitment.

Configuration approach:
```rust
let mut config = Config::new();
config.epoch_interruption(true);
config.consume_fuel(true);
config.wasm_memory64(false);  // 32-bit memory for safety
```

## Consequences

### Positive

- **Reliable timeout enforcement**: Epoch-based interruption handles infinite loops, unlike polling-based approaches that can miss tight loops
- **Deterministic resource accounting**: Fuel consumption is predictable and reproducible across runs
- **Memory safety without runtime overhead**: Cranelift's compile-time bounds checking is both safe and fast
- **Strong ecosystem**: Extensive documentation, active community, and corporate backing
- **Future-proof**: Path to WASI preview2/Component Model when needed

### Negative

- **Version coupling**: Pinned to Wasmtime 27; upgrades require careful testing due to potential breaking changes in WASI behavior
- **Preview1 limitations**: Cannot leverage Component Model features like interface types without migration effort
- **Compilation overhead**: JIT compilation adds latency to cold starts (mitigated by module caching in ADR-0004)
- **Binary size**: Wasmtime adds ~15MB to the final binary

### Implications

- All sandbox timeout and CPU limiting relies on Wasmtime's epoch and fuel mechanisms
- WASI configuration (stdin, stdout, filesystem, environment) flows through wasmtime-wasi
- Future WASI preview2 migration will require architectural changes to the engine layer
