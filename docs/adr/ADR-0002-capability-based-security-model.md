# ADR-0002: Capability-Based Security Model

## Status

Accepted

## Context

Sandboxes executing untrusted WASM code must be prevented from accessing system resources without explicit authorization. Traditional permission models (role-based, ACL) are often coarse-grained and difficult to audit. We needed a security model that:

- Follows the principle of least privilege by default
- Provides fine-grained control over specific operations
- Enables auditing of all permission checks
- Composes well with the builder-pattern configuration
- Maps cleanly to WASI's capability-based design

The security model is critical because Isolate's primary value proposition is safe execution of untrusted code.

## Decision

We implemented a **capability-based security model** with default-deny semantics. Every operation that accesses external resources requires an explicit capability grant.

### Capability Types

```rust
pub enum Capability {
    Filesystem(FilesystemCapability),  // ReadOnly, ReadWrite, TempDir
    Network(NetworkCapability),         // HttpClient, TcpConnect, TcpListen, DnsResolve
    Time(TimeCapability),               // SystemClock, MonotonicClock, Timers
    Random(RandomCapability),           // Secure, Seeded
    Environment(EnvironmentCapability), // ReadVar, ReadAll, Args
    Stdio(StdioCapability),             // Stdin, Stdout, Stderr
    HostFunction(HostFunctionCapability), // Named, Namespaced
}
```

### Enforcement Architecture

1. **CapabilityEnforcer**: Central component that checks all capability requests
   - `check_fs_read(path)`, `check_fs_write(path)`
   - `check_http_request(url)`, `check_tcp_connect(addr)`
   - `check_env_var(name)`, `check_stdout()`, etc.

2. **Path matching**: Filesystem capabilities use exact-prefix matching
   - `/data` grants access to `/data/file.txt` but not `/data2/file.txt`

3. **Glob patterns**: Network capabilities support wildcards
   - `*.example.com` matches `api.example.com`, `cdn.example.com`

4. **Audit logging**: All checks are logged with grant/deny status
   ```rust
   pub struct AuditEntry {
       capability: Capability,
       action: AuditAction,  // Granted, Used, Denied
       timestamp: DateTime<Utc>,
       details: Option<String>,
   }
   ```

### Configuration API

```rust
SandboxConfig::builder()
    .capability(Capability::stdout())
    .capability(Capability::filesystem_read("/data"))
    .capability(Capability::http_client("*.api.example.com"))
    .build()
```

## Consequences

### Positive

- **Defense in depth**: Even if WASM code exploits a vulnerability, it cannot access resources without capabilities
- **Auditable**: Complete audit trail of all capability checks enables security forensics
- **Composable**: Capabilities can be combined freely via the builder pattern
- **WASI-aligned**: Maps naturally to WASI's preopened directories and allowed hosts
- **Testable**: Capability checks can be unit tested in isolation

### Negative

- **Configuration complexity**: Users must explicitly grant each capability, which can be verbose
- **Runtime overhead**: Every I/O operation requires a capability check (mitigated by using HashSet lookups)
- **Error messages**: Capability denials require clear error messages to help users understand what's missing

### Implications

- WASI context setup reads from CapabilityEnforcer to configure preopened directories and allowed hosts
- New system features require corresponding capability types and enforcement points
- Preset configurations (restrictive, permissive) help users get started without enumerating capabilities
