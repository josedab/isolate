# CI/CD Secure Runner

Execute untrusted build scripts and PR validation code safely.

## Architecture

```
┌──────────┐    ┌────────────┐    ┌──────────────┐
│ CI/CD    │───▶│ Policy     │───▶│ Isolate      │
│ Pipeline │    │ Validator  │    │ Sandbox      │
└──────────┘    └────────────┘    └──────────────┘
                                        │
                                        ▼
                                  ┌──────────────┐
                                  │ Artifact     │
                                  │ Storage      │
                                  └──────────────┘
```

## Implementation

### Safe Script Execution

```rust
use isolate_core::{Sandbox, SandboxConfig};
use isolate_core::capability::Capability;
use isolate_core::policy_gen::ModuleAnalyzer;

async fn run_build_script(
    script_wasm: &[u8],
    workspace_path: &str,
) -> Result<isolate_core::Output, Box<dyn std::error::Error>> {
    // Analyze first
    let analyzer = ModuleAnalyzer::new();
    let report = analyzer.analyze(script_wasm);
    if report.overall_risk >= isolate_core::policy_gen::RiskLevel::Critical {
        return Err("Build script requires unsafe capabilities".into());
    }

    let config = SandboxConfig::builder()
        .module(script_wasm)?
        .memory_limit(512 * 1024 * 1024)  // 512MB for builds
        .fuel(1_000_000_000)               // 1B instructions
        .wall_time_limit(std::time::Duration::from_secs(300))  // 5 min
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .capability(Capability::filesystem_read(workspace_path))
        .capability(Capability::temp_dir())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    Ok(sandbox.run(&[]).await?)
}
```

## Security Considerations

- **Filesystem**: Grant read-only access to workspace, temp dir for scratch
- **Network**: Deny by default, allow specific registries if needed
- **Time limits**: 5-minute max prevents resource abuse
- **Policy analysis**: Reject scripts requiring critical-risk capabilities
