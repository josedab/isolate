# Multi-Tenant Analytics UDFs

Run user-defined functions (UDFs) for custom analytics transformations.

## Architecture

```
┌──────────┐    ┌────────────┐    ┌──────────────┐
│ Analytics│───▶│ UDF        │───▶│ Isolate      │
│ Engine   │    │ Registry   │    │ Sandbox      │
└──────────┘    └────────────┘    └──────────────┘
      │                                  │
      ▼                                  ▼
┌──────────┐                      ┌──────────────┐
│ Data     │                      │ Pipeline     │
│ Source   │                      │ Orchestrator │
└──────────┘                      └──────────────┘
```

## Implementation

### UDF Execution with Pipelines

```rust
use isolate_core::{Sandbox, SandboxConfig};
use isolate_core::capability::Capability;
use isolate_core::pipeline::{PipelineDefinition, Stage};
use isolate_core::profile::LanguageProfile;

fn build_etl_pipeline(
    extract_wasm: &[u8],
    transform_wasm: &[u8],
    load_wasm: &[u8],
) -> isolate_core::Result<PipelineDefinition> {
    let extract = SandboxConfig::builder()
        .module(extract_wasm)?
        .apply_profile(LanguageProfile::Python)
        .capability(Capability::stdout())
        .build()?;

    let transform = SandboxConfig::builder()
        .module(transform_wasm)?
        .apply_profile(LanguageProfile::Rust)
        .capability(Capability::stdout())
        .capability(Capability::stdin())
        .build()?;

    let load = SandboxConfig::builder()
        .module(load_wasm)?
        .apply_profile(LanguageProfile::Go)
        .capability(Capability::stdout())
        .capability(Capability::stdin())
        .build()?;

    PipelineDefinition::builder()
        .stage(Stage::new("extract", extract))
        .stage(Stage::new("transform", transform))
        .stage(Stage::new("load", load))
        .chain("extract", "transform")
        .chain("transform", "load")
        .build()
}
```

## Data Flow Patterns

| Pattern | Description | Use Case |
|---------|------------|----------|
| Linear | A → B → C | ETL pipelines |
| Fan-out | A → (B, C) | Parallel processing |
| Fan-in | (B, C) → D | Aggregation |
| Conditional | A → B or C | Data routing |

## Resource Planning

Per-UDF recommended limits:
- **Memory**: 64MB (Python UDFs may need 128MB)
- **CPU**: 10M fuel per row batch
- **I/O**: 10MB read, 1MB write per invocation
- **Timeout**: 30s per batch
