# Serverless FaaS Platform

Build an AWS Lambda-compatible serverless platform with sub-5ms cold starts.

## Architecture Overview

```
┌──────────────┐     ┌─────────────┐     ┌──────────────┐
│   API        │────▶│  Scheduler   │────▶│  Isolate     │
│   Gateway    │     │  (tokio)     │     │  Sandbox     │
└──────────────┘     └─────────────┘     └──────────────┘
       │                    │                    │
       │                    ▼                    │
       │             ┌─────────────┐             │
       │             │  Snapshot   │             │
       │             │  Cache      │◀────────────┘
       │             └─────────────┘
       │
       ▼
┌──────────────┐
│  Metrics /   │
│  Dashboard   │
└──────────────┘
```

## Key Components

### 1. Request Router
Route incoming HTTP requests to the correct function handler:

```rust
use isolate_core::{Sandbox, SandboxConfig};
use isolate_core::capability::Capability;
use isolate_core::profile::LanguageProfile;

async fn handle_function(
    function_id: &str,
    wasm_bytes: &[u8],
    input: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let config = SandboxConfig::builder()
        .module(wasm_bytes)?
        .apply_profile(LanguageProfile::Rust)
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(input).await?;
    Ok(output.stdout)
}
```

### 2. Cold Start Optimization
Use the pre-compilation cache for frequently-invoked functions:

```rust
use isolate_core::coldstart::PrecompileCache;

let cache = PrecompileCache::new("/var/cache/isolate".into(), 1000)?;

// On deploy: pre-compile the module
let hash = cache.precompile(&engine, &wasm_bytes)?;

// On invoke: load from cache (sub-millisecond)
if let Some(module) = cache.load(&engine, &hash)? {
    // Fast path: module already compiled
}
```

### 3. Rate Limiting per Tenant
Prevent noisy neighbors with per-tenant rate limiting:

```rust
use isolate_core::ratelimit::{RateLimitConfig, QuotaConfig, SharedRateLimiter};

let limiter = SharedRateLimiter::new(
    RateLimitConfig::with_rate(100)   // 100 req/s sustained
        .burst(200)                     // 200 req/s burst
        .with_quota(QuotaConfig {
            max_executions_per_hour: Some(10_000),
            max_bandwidth_bytes_per_hour: Some(1024 * 1024 * 1024), // 1GB/hr
        }),
);

// Before each invocation
limiter.try_acquire()?;
```

## Scaling Considerations

| Metric | Target | How |
|--------|--------|-----|
| Cold start | <5ms p50 | PrecompileCache + LanguageProfile |
| Throughput | 10K req/s | tokio async + shared WasmEngine |
| Isolation | Per-request | Capability system + ResourceLimits |
| Multi-tenant | Fair share | RateLimiter per tenant |

## Production Checklist

- [ ] Enable Prometheus metrics export
- [ ] Configure snapshot auto-warming for hot functions
- [ ] Set resource limits appropriate to your SLA
- [ ] Enable audit logging for compliance
- [ ] Set up health checks (`/healthz`, `/readyz`)
