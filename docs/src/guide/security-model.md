# Security Model

Isolate provides defense-in-depth security through multiple layers of isolation and control.

## Security Layers

```
┌─────────────────────────────────────────────────────────┐
│                    Application                           │
├─────────────────────────────────────────────────────────┤
│  Layer 4: Capability System (Permission Control)        │
├─────────────────────────────────────────────────────────┤
│  Layer 3: Resource Limits (DoS Prevention)              │
├─────────────────────────────────────────────────────────┤
│  Layer 2: WASI Sandbox (System Call Filtering)          │
├─────────────────────────────────────────────────────────┤
│  Layer 1: WASM Sandbox (Memory Isolation)               │
├─────────────────────────────────────────────────────────┤
│  Layer 0: OS Isolation (Optional: seccomp, namespaces)  │
└─────────────────────────────────────────────────────────┘
```

## Layer 1: WASM Sandbox

WebAssembly provides strong isolation guarantees:

### Memory Safety

- **Linear memory**: WASM modules can only access their own linear memory
- **No pointers**: Can't access arbitrary memory addresses
- **Bounds checking**: All memory accesses are validated

### Control Flow Integrity

- **Type-safe calls**: Function calls are validated against signatures
- **No arbitrary jumps**: Can't jump to arbitrary code locations
- **Validated bytecode**: All WASM is validated before execution

### What WASM Prevents

- Buffer overflows (memory is bounds-checked)
- Use-after-free (no manual memory management in the sandbox)
- Return-oriented programming (no arbitrary code execution)
- Code injection (bytecode is immutable after validation)

## Layer 2: WASI Sandbox

WASI (WebAssembly System Interface) provides controlled system access:

### Preopened Directories

WASM modules can only access explicitly granted directories:

```rust
// Module can ONLY access /data and /tmp/output
.capability(Capability::filesystem_read("/data"))
.capability(Capability::filesystem_write("/tmp/output"))
```

### Filtered System Calls

WASI exposes only safe operations. Dangerous operations are not available:

| Available | Not Available |
|-----------|---------------|
| File read/write (to granted paths) | Raw system calls |
| Clock access (if granted) | Process spawning |
| Random numbers (if granted) | Network sockets (direct) |
| Environment variables (specific) | Shared memory |

## Layer 3: Resource Limits

Prevents denial-of-service attacks:

```rust
.memory_limit(128 * 1024 * 1024)     // Memory bombs
.fuel(1_000_000)                      // Infinite loops
.wall_time_limit(Duration::from_secs(30))  // Hanging
.io_write_limit(1024 * 1024)          // Disk filling
```

### Attack Prevention

| Attack | Mitigation |
|--------|------------|
| Memory bomb | Heap limit |
| Fork bomb | No process spawning |
| Infinite loop | Fuel metering |
| Slowloris | Wall clock timeout |
| Disk filling | I/O write limit |

## Layer 4: Capability System

Fine-grained permission control:

```rust
// Default: No capabilities (can't do anything useful)
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .build()?;

// Explicit grants only
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::stdout())
    .capability(Capability::filesystem_read("/data"))
    .build()?;
```

### Audit Logging

All capability checks are logged:

```
INFO capability_granted sandbox=abc123 cap=stdout
WARN capability_denied sandbox=abc123 cap=filesystem_read path="/etc/passwd"
```

## Layer 0: OS Isolation (Linux)

For additional security on Linux, Isolate supports:

### seccomp-bpf

Restricts available system calls at the kernel level:

```rust
// Experimental - enable OS-level isolation
.os_isolation(OsIsolation::Seccomp)
```

### Landlock LSM

Filesystem sandboxing at the kernel level:

```rust
.os_isolation(OsIsolation::Landlock)
```

### Namespaces

Process, network, and mount isolation:

```rust
.os_isolation(OsIsolation::Namespaces)
```

> **Note:** OS isolation features are experimental and Linux-only.

## Threat Model

### In Scope

Isolate protects against:

- **Malicious WASM modules** attempting to escape the sandbox
- **Resource exhaustion** attacks (DoS)
- **Unauthorized access** to filesystem, network, or environment
- **Information disclosure** through side channels (limited)

### Out of Scope

Isolate does NOT protect against:

- **Timing side channels**: WASM execution time may leak information
- **Spectre-class attacks**: Mitigated by Wasmtime but not eliminated
- **Host vulnerabilities**: Bugs in Isolate or Wasmtime itself
- **Physical attacks**: Physical access to the machine

## Security Best Practices

### 1. Defense in Depth

Never rely on a single security layer:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // Multiple layers
    .memory_limit(64 * 1024 * 1024)           // Resource limit
    .fuel(1_000_000)                           // CPU limit
    .capability(Capability::stdout())          // Capability
    .wall_time_limit(Duration::from_secs(10)) // Timeout
    .build()?;
```

### 2. Validate Input

Check WASM modules before loading:

```rust
// Validate size
if wasm_bytes.len() > 10 * 1024 * 1024 {
    return Err("Module too large");
}

// Validate magic number
if &wasm_bytes[0..4] != b"\0asm" {
    return Err("Invalid WASM module");
}
```

### 3. Least Privilege

Grant minimum necessary capabilities:

```rust
// Bad
.capability(Capability::filesystem_read("/"))

// Good
.capability(Capability::filesystem_read("/app/data/input.json"))
```

### 4. Monitor and Alert

Enable audit logging and monitor for anomalies:

```rust
tracing_subscriber::fmt()
    .with_env_filter("isolate::capability::audit=warn")
    .init();
```

### 5. Keep Updated

Regularly update Isolate and Wasmtime for security fixes:

```bash
cargo update
cargo audit
```

## Security Advisories

Security issues should be reported via [GitHub's private vulnerability reporting](https://github.com/josedab/isolate/security/advisories/new). **Do not open a public issue for security vulnerabilities.**

See [SECURITY.md](https://github.com/josedab/isolate/blob/main/SECURITY.md) for our full security policy.

## See Also

- [Capabilities](./capabilities.md) - Permission system details
- [Resource Limits](./resource-limits.md) - DoS prevention
- [Wasmtime Security](https://docs.wasmtime.dev/security.html) - Underlying runtime security
