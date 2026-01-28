---
slug: security-best-practices
title: Security Best Practices for Running Untrusted Code
authors: [isolate-team]
tags: [security, tutorial]
---

Running untrusted code safely requires careful attention to security at every layer. This guide covers best practices for using Isolate in production environments.

<!-- truncate -->

## The Defense-in-Depth Approach

Isolate provides multiple layers of security. Each layer catches threats that might slip through others:

```
┌─────────────────────────────────────┐
│         Your Application            │
├─────────────────────────────────────┤
│     Isolate Capability System       │  ← Explicit permission grants
├─────────────────────────────────────┤
│       Resource Limits               │  ← DoS prevention
├─────────────────────────────────────┤
│    WASM Sandbox (Wasmtime)          │  ← Memory isolation
├─────────────────────────────────────┤
│   OS Security (optional)            │  ← seccomp, Landlock
└─────────────────────────────────────┘
```

## 1. Apply the Principle of Least Privilege

Grant only the capabilities that code actually needs:

```rust
// Bad: Grants too much access
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::filesystem_read("/"))  // Entire filesystem!
    .capability(Capability::env_all())             // All env vars!
    .build()?;

// Good: Minimal required capabilities
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    .capability(Capability::filesystem_read("/app/data/inputs"))
    .capability(Capability::env_var("CONFIG_PATH"))
    .build()?;
```

## 2. Set Appropriate Resource Limits

Always set resource limits to prevent denial-of-service:

```rust
let config = SandboxConfig::builder()
    .module(&wasm_bytes)?
    // Memory: Prevent OOM conditions
    .memory_limit(128 * 1024 * 1024)  // 128MB
    .stack_size(1024 * 1024)           // 1MB
    // CPU: Prevent infinite loops
    .fuel(10_000_000)                  // ~10M instructions
    .cpu_time_limit(Duration::from_secs(30))
    // Time: Prevent hung sandboxes
    .wall_time_limit(Duration::from_secs(60))
    // I/O: Prevent disk filling
    .io_write_limit(10 * 1024 * 1024)  // 10MB
    .build()?;
```

Choose limits based on your use case:

| Use Case | Memory | CPU Time | Wall Time |
|----------|--------|----------|-----------|
| Edge worker | 32MB | 50ms | 100ms |
| Serverless function | 128MB | 30s | 60s |
| Data processing | 512MB | 5min | 10min |
| CI test runner | 1GB | 10min | 30min |

## 3. Validate WASM Modules Before Execution

Don't blindly execute any WASM file:

```rust
use sha2::{Sha256, Digest};

fn validate_module(wasm_bytes: &[u8]) -> Result<(), String> {
    // Check size limits
    if wasm_bytes.len() > 50 * 1024 * 1024 {
        return Err("Module too large".into());
    }

    // Verify hash against allowlist (for known modules)
    let hash = hex::encode(Sha256::digest(wasm_bytes));
    if !is_approved_module(&hash) {
        return Err("Module not in approved list".into());
    }

    Ok(())
}
```

## 4. Use Separate Engines for Tenant Isolation

In multi-tenant environments, consider using separate engines:

```rust
use std::sync::Arc;
use dashmap::DashMap;

struct TenantRuntime {
    // Each tenant gets their own engine
    engines: DashMap<String, Arc<WasmEngine>>,
}

impl TenantRuntime {
    fn get_engine(&self, tenant_id: &str) -> Arc<WasmEngine> {
        self.engines
            .entry(tenant_id.to_string())
            .or_insert_with(|| Arc::new(WasmEngine::new().unwrap()))
            .clone()
    }
}
```

## 5. Enable Audit Logging

Track all capability checks for security monitoring:

```rust
// Enable audit logging
tracing_subscriber::fmt()
    .with_env_filter("isolate::capability::audit=info")
    .json()
    .init();
```

Log output:
```json
{"timestamp":"2024-01-20T10:30:00Z","level":"INFO","target":"isolate::capability::audit","message":"capability_granted","sandbox":"abc123","capability":"stdout"}
{"timestamp":"2024-01-20T10:30:01Z","level":"WARN","target":"isolate::capability::audit","message":"capability_denied","sandbox":"abc123","capability":"filesystem_read","path":"/etc/passwd"}
```

## 6. Handle Errors Gracefully

Don't leak internal details in error messages:

```rust
match sandbox.run(&input).await {
    Ok(output) => handle_success(output),
    Err(Error::CapabilityDenied(cap)) => {
        // Log internally
        tracing::warn!("Capability denied: {:?}", cap);
        // Return generic error to user
        Err("Operation not permitted".into())
    }
    Err(Error::Timeout(_)) => {
        Err("Execution timed out".into())
    }
    Err(e) => {
        tracing::error!("Sandbox error: {:?}", e);
        Err("Internal error".into())
    }
}
```

## 7. Clean Up After Execution

Ensure resources are released:

```rust
async fn run_with_cleanup(config: SandboxConfig, input: &[u8]) -> Result<Output> {
    let mut sandbox = Sandbox::create(config).await?;

    // Use a timeout wrapper for extra safety
    let result = tokio::time::timeout(
        Duration::from_secs(120),
        sandbox.run(input)
    ).await;

    // Always terminate the sandbox
    let _ = sandbox.terminate().await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(Error::Timeout(Duration::from_secs(120))),
    }
}
```

## Security Checklist

Before deploying to production:

- [ ] Resource limits set for memory, CPU, time, and I/O
- [ ] Capabilities follow least-privilege principle
- [ ] WASM modules validated before execution
- [ ] Audit logging enabled
- [ ] Error messages don't leak internal details
- [ ] Sandbox cleanup happens on all code paths
- [ ] Multi-tenant isolation verified
- [ ] Monitoring and alerting configured

## Further Reading

- [Capabilities Guide](/docs/guides/capabilities) - Detailed capability reference
- [Security Model](/docs/guides/security-model) - Defense-in-depth architecture
- [Monitoring Guide](/docs/guides/monitoring) - Metrics and observability

---

Have questions about securing your Isolate deployment? Open a [discussion on GitHub](https://github.com/josedab/isolate/discussions).
