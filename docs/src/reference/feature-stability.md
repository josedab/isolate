# Feature Stability Guide

Isolate uses a tiered stability system for its features. This guide helps you choose which features are safe for production use.

## Stability Tiers

### Tier 1: Stable ✅

These features are production-ready with backwards-compatibility guarantees within the same major version.

| Feature | Module | Description |
|---------|--------|-------------|
| Core Sandbox | `sandbox`, `config` | Sandbox creation, execution, output capture |
| Capabilities | `capability` | Default-deny permission system |
| Resource Limits | `resource` | Memory, CPU fuel, I/O, timeout enforcement |
| Error Handling | `error` | Typed errors with actionable suggestions |
| WASM Engine | `engine` | Wasmtime-based execution with module caching |
| Metrics | `metrics` | Prometheus metrics integration |

**No feature flag needed** — these are always available.

### Tier 2: Beta 🔶

These features are functional and tested but the API may change in minor versions. Safe for production with the understanding that upgrades may require code changes.

| Feature Flag | Modules | Description |
|-------------|---------|-------------|
| `pool` | `pool`, `predict` | Warm sandbox pool with autoscaling |
| `networking` | `http`, `network` | HTTP client and network policy |
| `agent` | `agent`, `llm` | AI agent SDK with LLM function calling |
| `policy-engine` | `policy`, `audit`, `compose` | Policy rules engine and audit logging |

**Usage:** `isolate-core = { version = "0.1", features = ["pool", "networking"] }`

### Tier 3: Alpha 🟡

These features are implemented and compile but are under active development. API will change. Use for evaluation and feedback.

| Feature Flag | Modules | Description |
|-------------|---------|-------------|
| `platform` | `admin`, `gateway`, `orchestrator`, `kv`, `secrets`, `ipc`, `marketplace`, `plugin`, `workflow`, `vfs`, `provenance`, `serverless`, `iac` | Full platform services |
| `snapshots` | `snapshot` | Copy-on-write snapshots for warm starts |
| `debug-support` | `debug` | Live debugging and time-travel replay |
| `module-signing` | `signing` | Cryptographic module signing |
| `kubernetes` | `k8s` | Kubernetes operator and CRDs |
| `otel-telemetry` | `telemetry` | OpenTelemetry tracing integration |
| `extras` | `ai_exec`, `carbon`, `enclave`, `jsrt`, `security`, `verify` | Additional integrations |

### Tier 4: Experimental 🔴

These features are proof-of-concept. They compile but contain stubs, may have incomplete implementations, and should not be used in production.

| Feature Flag | Modules | Description |
|-------------|---------|-------------|
| `hotpatch` | `hotpatch` | Hot code patching (simulated) |
| `distributed-mesh` | `mesh` | Distributed sandbox clustering |
| `gpu-compute` | `gpu` | GPU acceleration (simulated) |
| `chaos-testing` | `chaos` | Fault injection testing |

**Usage:** `isolate-core = { version = "0.1", features = ["experimental"] }`

## Meta-Features

| Feature | Includes |
|---------|----------|
| `full` | All Tier 2 + Tier 3 + Tier 4 features |
| `experimental` | Only Tier 4 features |

## Choosing Features

**For production applications:** Use only Tier 1 (default) and Tier 2 features.

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["pool", "networking"] }
```

**For evaluation and prototyping:** Add Tier 3 features as needed.

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["pool", "networking", "snapshots", "debug-support"] }
```

**For development/testing of Isolate itself:** Use `full` or `--all-features`.

```toml
[dependencies]
isolate-core = { version = "0.1", features = ["full"] }
```

## Migration Policy

- **Tier 1 → Tier 1:** No breaking changes within major version
- **Tier 2 → Tier 1:** Feature is promoted when API stabilizes
- **Tier 3 → Tier 2:** Feature is promoted after community feedback
- **Tier 4 → Tier 3:** Feature is promoted when core implementation is complete
- **Any tier → Removed:** Deprecated features get one minor version of warnings before removal
