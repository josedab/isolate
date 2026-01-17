# SaaS Plugin System

Enable users to write and run custom scripts safely within your SaaS product.

## Architecture Overview

```
┌──────────────┐     ┌─────────────┐     ┌──────────────┐
│  User Code   │────▶│  Policy     │────▶│  Isolate     │
│  Editor UI   │     │  Generator  │     │  Sandbox     │
└──────────────┘     └─────────────┘     └──────────────┘
                            │                    │
                            ▼                    ▼
                     ┌─────────────┐     ┌──────────────┐
                     │  Capability │     │  Rate        │
                     │  Enforcer   │     │  Limiter     │
                     └─────────────┘     └──────────────┘
```

## Implementation

### 1. User Script Execution
Accept user-provided WASM modules with strict sandboxing:

```rust
use isolate_core::{Sandbox, SandboxConfig};
use isolate_core::capability::Capability;
use isolate_core::ratelimit::RateLimitConfig;

async fn run_user_script(
    wasm_bytes: &[u8],
    input: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let config = SandboxConfig::builder()
        .module(wasm_bytes)?
        .memory_limit(32 * 1024 * 1024)          // 32MB max
        .fuel(5_000_000)                           // 5M instructions
        .wall_time_limit(std::time::Duration::from_secs(5))
        .capability(Capability::stdout())
        .max_requests_per_second(10)               // Rate limit
        .build()?;

    let input_bytes = serde_json::to_vec(&input)?;
    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&input_bytes).await?;

    let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(result)
}
```

### 2. Automatic Policy Generation
Use the policy generator to suggest capabilities:

```rust
use isolate_core::policy_gen::ModuleAnalyzer;

let analyzer = ModuleAnalyzer::new();
let report = analyzer.analyze(&user_wasm);

// Show users what their module needs
for suggestion in &report.suggested_capabilities {
    println!("Needs: {} ({})", suggestion.capability, suggestion.reason);
}

// Block high-risk modules
if report.overall_risk >= isolate_core::policy_gen::RiskLevel::High {
    return Err("Module requires elevated permissions - contact support");
}
```

## Pricing Tiers

| Tier | Executions/hr | Memory | CPU Fuel | Capabilities |
|------|--------------|--------|----------|-------------|
| Free | 100 | 16MB | 1M | stdout only |
| Pro | 10,000 | 64MB | 50M | stdout + HTTP |
| Enterprise | Unlimited | 256MB | Unlimited | All |

## Security Best Practices

1. **Never grant filesystem access** to user-provided modules
2. **Always set fuel limits** to prevent infinite loops
3. **Use the policy generator** to validate modules before deployment
4. **Enable audit logging** for all capability checks
5. **Rate limit per user**, not per request
