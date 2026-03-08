//! Module stability tier classification.
//!
//! Classifies all isolate-core modules into stability tiers so users
//! know which APIs are safe for production use.
//!
//! # Tier Definitions
//!
//! - **Stable**: Production-ready, backwards-compatible, well-tested, real I/O
//! - **Beta**: Feature-complete but API may change, good test coverage
//! - **Preview**: API designed and compiles, but backed by in-memory simulation
//!   rather than real external integration. Safe to evaluate, not for production.
//! - **Experimental**: Working but unstable, may change significantly
//! - **Stub**: Scaffolding/simulated, not suitable for production

use std::collections::HashMap;

/// Stability tier for a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StabilityTier {
    /// Production-ready, backwards-compatible, well-tested.
    Stable,
    /// Feature-complete but API may change.
    Beta,
    /// API designed and compiles, but backed by in-memory simulation.
    /// Safe to evaluate for API feedback, not for production workloads.
    Preview,
    /// Working but unstable, may change significantly.
    Experimental,
    /// Scaffolding/simulated, not suitable for production.
    Stub,
}

impl StabilityTier {
    /// Whether this tier is safe for production use.
    pub fn is_production_safe(&self) -> bool {
        matches!(self, Self::Stable | Self::Beta)
    }

    /// Whether this tier is safe for evaluation and API feedback.
    pub fn is_evaluation_safe(&self) -> bool {
        matches!(self, Self::Stable | Self::Beta | Self::Preview)
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Preview => "Preview",
            Self::Experimental => "Experimental",
            Self::Stub => "Stub",
        }
    }

    /// Emoji indicator.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Stable => "✅",
            Self::Beta => "🔧",
            Self::Preview => "👁️",
            Self::Experimental => "⚠️",
            Self::Stub => "❌",
        }
    }
}

impl std::fmt::Display for StabilityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.emoji(), self.label())
    }
}

/// Metadata about a module's stability classification.
#[derive(Debug, Clone)]
pub struct ModuleClassification {
    /// Module name.
    pub name: &'static str,
    /// Stability tier.
    pub tier: StabilityTier,
    /// Brief description.
    pub description: &'static str,
    /// Whether the module requires a feature flag.
    pub feature_flag: Option<&'static str>,
    /// Number of tests (approximate).
    pub test_count: u32,
    /// Whether this module has real (non-simulated) implementation.
    pub has_real_implementation: bool,
}

/// Get the complete module classification matrix.
pub fn module_classifications() -> Vec<ModuleClassification> {
    vec![
        // === Stable: Production-ready, real I/O, well-tested ===
        ModuleClassification {
            name: "sandbox",
            tier: StabilityTier::Stable,
            description: "Core sandbox creation and execution",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "config",
            tier: StabilityTier::Stable,
            description: "Sandbox configuration and builders",
            feature_flag: None,
            test_count: 5,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "error",
            tier: StabilityTier::Stable,
            description: "Error types and handling",
            feature_flag: None,
            test_count: 1,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "capability",
            tier: StabilityTier::Stable,
            description: "Capability-based security model",
            feature_flag: None,
            test_count: 8,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "engine",
            tier: StabilityTier::Stable,
            description: "WASM execution engine (Wasmtime)",
            feature_flag: None,
            test_count: 15,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "resource",
            tier: StabilityTier::Stable,
            description: "Resource metering and limits",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "metrics",
            tier: StabilityTier::Stable,
            description: "Prometheus metrics integration",
            feature_flag: None,
            test_count: 3,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "pool",
            tier: StabilityTier::Stable,
            description: "Multi-tenant resource pooling with warm pool",
            feature_flag: Some("pool"),
            test_count: 43,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "http",
            tier: StabilityTier::Stable,
            description: "Secure HTTP client with capability enforcement",
            feature_flag: Some("networking"),
            test_count: 15,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "network",
            tier: StabilityTier::Stable,
            description: "Network policies and access control",
            feature_flag: Some("networking"),
            test_count: 15,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "policy",
            tier: StabilityTier::Stable,
            description: "Declarative policy engine with bundles",
            feature_flag: Some("policy-engine"),
            test_count: 35,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "coldstart",
            tier: StabilityTier::Stable,
            description: "Pre-compilation cache for fast startup",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "profile",
            tier: StabilityTier::Stable,
            description: "Language-specific optimization profiles",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "sandbox_profile",
            tier: StabilityTier::Stable,
            description: "Use-case-based sandbox profiles",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        // === Beta: Feature-complete, API may change ===
        ModuleClassification {
            name: "vfs",
            tier: StabilityTier::Beta,
            description: "Virtual filesystem with quotas",
            feature_flag: Some("platform-storage"),
            test_count: 44,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "ipc",
            tier: StabilityTier::Beta,
            description: "Inter-sandbox communication",
            feature_flag: Some("platform-comm"),
            test_count: 37,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "kv",
            tier: StabilityTier::Beta,
            description: "Key-value store for sandboxes",
            feature_flag: Some("platform-storage"),
            test_count: 17,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "compose",
            tier: StabilityTier::Beta,
            description: "Module composition and linking",
            feature_flag: Some("policy-engine"),
            test_count: 25,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "audit",
            tier: StabilityTier::Beta,
            description: "Cryptographic audit logging and verification",
            feature_flag: Some("policy-engine"),
            test_count: 10,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "policy_lang",
            tier: StabilityTier::Beta,
            description: "HCL-inspired policy-as-code DSL",
            feature_flag: Some("policy-engine"),
            test_count: 16,
            has_real_implementation: true,
        },
        // === Preview: API designed, in-memory simulation, not production ===
        ModuleClassification {
            name: "billing",
            tier: StabilityTier::Preview,
            description:
                "Multi-tenant billing with memory-seconds tracking (in-memory, no payment provider)",
            feature_flag: Some("billing"),
            test_count: 24,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "cloud_cost",
            tier: StabilityTier::Preview,
            description: "Cloud cost tracking (simulated pricing data)",
            feature_flag: Some("billing"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "federation",
            tier: StabilityTier::Preview,
            description: "Federated registry (in-memory gossip, no real network)",
            feature_flag: Some("federation"),
            test_count: 20,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "georep",
            tier: StabilityTier::Preview,
            description: "Geo-replication (simulated regions)",
            feature_flag: Some("federation"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "saas",
            tier: StabilityTier::Preview,
            description: "SaaS multi-tenancy (in-memory, no database)",
            feature_flag: Some("platform-hosting"),
            test_count: 18,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "carbon",
            tier: StabilityTier::Preview,
            description: "Carbon tracking (static data, no grid API)",
            feature_flag: Some("extras"),
            test_count: 13,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "ai_sandbox",
            tier: StabilityTier::Preview,
            description: "AI output sanitization (in-memory verdicts)",
            feature_flag: Some("extras"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "ai_exec",
            tier: StabilityTier::Preview,
            description: "AI code execution SDK (framework only)",
            feature_flag: Some("extras"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "agent",
            tier: StabilityTier::Preview,
            description: "AI agent SDK (in-memory session, no provider)",
            feature_flag: Some("agent"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "llm",
            tier: StabilityTier::Preview,
            description: "LLM function calling (no real provider calls)",
            feature_flag: Some("agent"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "admin",
            tier: StabilityTier::Preview,
            description: "Admin dashboard API (in-memory state)",
            feature_flag: Some("platform-admin"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "gateway",
            tier: StabilityTier::Preview,
            description: "HTTP/REST gateway (partial implementation)",
            feature_flag: Some("platform-admin"),
            test_count: 13,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "orchestrator",
            tier: StabilityTier::Preview,
            description: "Multi-tenant scheduler (in-memory quotas)",
            feature_flag: Some("platform-admin"),
            test_count: 28,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "secrets",
            tier: StabilityTier::Preview,
            description: "Secrets management (in-memory, no vault/cloud)",
            feature_flag: Some("platform-storage"),
            test_count: 13,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "provenance",
            tier: StabilityTier::Preview,
            description: "Supply-chain provenance (in-memory tracking)",
            feature_flag: Some("platform-provenance"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "serverless",
            tier: StabilityTier::Preview,
            description: "Serverless adapters (manifest generation only)",
            feature_flag: Some("platform-hosting"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "playground",
            tier: StabilityTier::Preview,
            description: "Web playground (API only, no frontend)",
            feature_flag: Some("platform-hosting"),
            test_count: 6,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "iac",
            tier: StabilityTier::Preview,
            description: "Infrastructure as code (template generation only)",
            feature_flag: Some("platform-infra"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "workflow",
            tier: StabilityTier::Preview,
            description: "Workflow orchestration (in-memory executor)",
            feature_flag: Some("platform-workflow"),
            test_count: 23,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "workflow_engine",
            tier: StabilityTier::Preview,
            description: "DAG execution engine (in-memory, no persistence)",
            feature_flag: Some("platform-workflow"),
            test_count: 15,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "pipeline",
            tier: StabilityTier::Preview,
            description: "Multi-sandbox pipelines (in-memory)",
            feature_flag: Some("platform-workflow"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "rpc",
            tier: StabilityTier::Preview,
            description: "Inter-sandbox RPC (in-memory registry)",
            feature_flag: Some("platform-comm"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "graphql_schema",
            tier: StabilityTier::Preview,
            description: "Auto-generated GraphQL schemas",
            feature_flag: Some("platform-comm"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "autoscale",
            tier: StabilityTier::Preview,
            description: "Predictive auto-scaling (in-memory PID loops)",
            feature_flag: Some("deployment"),
            test_count: 15,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "deploy",
            tier: StabilityTier::Preview,
            description: "Multi-cloud deployment (config generation only)",
            feature_flag: Some("deployment"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "hot_reload",
            tier: StabilityTier::Preview,
            description: "Zero-downtime updates (in-memory canary)",
            feature_flag: Some("deployment"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "module_registry",
            tier: StabilityTier::Preview,
            description: "Module registry (in-memory cache only)",
            feature_flag: Some("deployment"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "oci_registry",
            tier: StabilityTier::Preview,
            description: "OCI registry (in-memory manifests, no real push/pull)",
            feature_flag: Some("deployment"),
            test_count: 14,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "registry_security",
            tier: StabilityTier::Preview,
            description: "Registry signing and scanning (framework only)",
            feature_flag: Some("deployment"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "dashboard",
            tier: StabilityTier::Preview,
            description: "Metrics dashboard (in-memory state)",
            feature_flag: Some("observability"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "dashboard_api",
            tier: StabilityTier::Preview,
            description: "Dashboard REST API with routing, query params, and handlers",
            feature_flag: Some("observability"),
            test_count: 19,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "observability",
            tier: StabilityTier::Preview,
            description: "Grafana templates (config generation only)",
            feature_flag: Some("observability"),
            test_count: 6,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "tracing_ctx",
            tier: StabilityTier::Preview,
            description: "Trace context propagation (in-memory spans)",
            feature_flag: Some("observability"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "wasm_analytics",
            tier: StabilityTier::Preview,
            description: "WASM profiling analytics (in-memory metrics)",
            feature_flag: Some("observability"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "benchmark",
            tier: StabilityTier::Preview,
            description: "Benchmarking framework (in-memory analysis)",
            feature_flag: Some("extras"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "bench_compare",
            tier: StabilityTier::Preview,
            description: "Comparative benchmarking (framework only)",
            feature_flag: Some("extras"),
            test_count: 6,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "jsrt",
            tier: StabilityTier::Preview,
            description: "JavaScript runtime (QuickJS WASM wrapper)",
            feature_flag: Some("extras"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "replay",
            tier: StabilityTier::Preview,
            description: "Execution recording/replay (in-memory)",
            feature_flag: Some("extras"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "transpiler",
            tier: StabilityTier::Preview,
            description: "WASM AOT compilation (framework only)",
            feature_flag: Some("extras"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "compliance",
            tier: StabilityTier::Preview,
            description: "Compliance templates (no real evidence collection)",
            feature_flag: Some("policy-engine"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "policy_builder",
            tier: StabilityTier::Preview,
            description: "Visual policy builder (in-memory IR)",
            feature_flag: Some("policy-engine"),
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "policy_file",
            tier: StabilityTier::Preview,
            description: "Policy file I/O (framework only)",
            feature_flag: Some("policy-engine"),
            test_count: 6,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "policy_gen",
            tier: StabilityTier::Preview,
            description: "Policy code generation (template output)",
            feature_flag: Some("policy-engine"),
            test_count: 8,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "sandbox_kv",
            tier: StabilityTier::Preview,
            description: "Per-sandbox KV bindings (in-memory)",
            feature_flag: Some("platform-storage"),
            test_count: 8,
            has_real_implementation: false,
        },
        // === Experimental: Working but unstable ===
        ModuleClassification {
            name: "wasi2",
            tier: StabilityTier::Experimental,
            description: "WASI Preview2 component model (detection works, execution partial)",
            feature_flag: Some("wasi-preview2"),
            test_count: 63,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "snapshot",
            tier: StabilityTier::Experimental,
            description: "Snapshot/restore with CoW",
            feature_flag: Some("snapshots"),
            test_count: 8,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "debug",
            tier: StabilityTier::Experimental,
            description: "Debugging and flame graphs",
            feature_flag: Some("debug-support"),
            test_count: 10,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "signing",
            tier: StabilityTier::Experimental,
            description: "Cryptographic module signing (HMAC-SHA256; Ed25519 stubbed)",
            feature_flag: Some("module-signing"),
            test_count: 10,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "k8s",
            tier: StabilityTier::Experimental,
            description: "Kubernetes CRD definitions and operator types (no K8s API client)",
            feature_flag: Some("kubernetes"),
            test_count: 30,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "telemetry",
            tier: StabilityTier::Experimental,
            description: "OpenTelemetry tracing (event model only, no eBPF probes)",
            feature_flag: Some("otel-telemetry"),
            test_count: 5,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "chaos",
            tier: StabilityTier::Experimental,
            description: "Chaos engineering DSL and fault injection (simulated execution)",
            feature_flag: Some("chaos-testing"),
            test_count: 11,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "predict",
            tier: StabilityTier::Experimental,
            description: "Predictive resource scaling",
            feature_flag: Some("pool"),
            test_count: 4,
            has_real_implementation: true,
        },
        // === Stub: Simulated, not production ready ===
        ModuleClassification {
            name: "enclave",
            tier: StabilityTier::Stub,
            description: "TEE/SGX enclave (simulated)",
            feature_flag: Some("extras"),
            test_count: 3,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "gpu",
            tier: StabilityTier::Stub,
            description: "GPU compute (simulated)",
            feature_flag: Some("gpu-compute"),
            test_count: 16,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "hotpatch",
            tier: StabilityTier::Stub,
            description: "Hot code patching (simulated)",
            feature_flag: Some("hotpatch"),
            test_count: 21,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "mesh",
            tier: StabilityTier::Stub,
            description: "Distributed sandbox mesh (network stubs)",
            feature_flag: Some("distributed-mesh"),
            test_count: 83,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "verify",
            tier: StabilityTier::Stub,
            description: "Formal verification (simplified methods)",
            feature_flag: Some("extras"),
            test_count: 10,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "security",
            tier: StabilityTier::Stub,
            description: "OS security seccomp/Landlock (skeleton)",
            feature_flag: Some("extras"),
            test_count: 48,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "ai",
            tier: StabilityTier::Stub,
            description: "AI anomaly detection (framework only)",
            feature_flag: Some("ai-detection"),
            test_count: 29,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "nlp",
            tier: StabilityTier::Experimental,
            description: "Natural language policy parsing with keyword extraction",
            feature_flag: Some("nlp-policies"),
            test_count: 18,
            has_real_implementation: true,
        },
    ]
}

/// Get classification for a specific module.
pub fn get_tier(module_name: &str) -> Option<StabilityTier> {
    module_classifications().iter().find(|c| c.name == module_name).map(|c| c.tier)
}

/// Get all modules of a specific tier.
pub fn modules_by_tier(tier: StabilityTier) -> Vec<&'static str> {
    module_classifications().iter().filter(|c| c.tier == tier).map(|c| c.name).collect()
}

/// Generate a maturity matrix as a formatted string.
pub fn maturity_matrix() -> String {
    let classifications = module_classifications();
    let mut output = String::new();

    output.push_str("# Module Maturity Matrix\n\n");

    for tier in &[
        StabilityTier::Stable,
        StabilityTier::Beta,
        StabilityTier::Preview,
        StabilityTier::Experimental,
        StabilityTier::Stub,
    ] {
        output.push_str(&format!("## {} {}\n\n", tier.emoji(), tier.label()));
        output.push_str("| Module | Description | Feature Flag | Tests |\n");
        output.push_str("|--------|-------------|-------------|-------|\n");

        for c in classifications.iter().filter(|c| c.tier == *tier) {
            output.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                c.name,
                c.description,
                c.feature_flag.unwrap_or("—"),
                c.test_count,
            ));
        }

        output.push('\n');
    }

    output
}

/// Summary counts by tier.
pub fn tier_summary() -> HashMap<StabilityTier, usize> {
    let mut counts = HashMap::new();
    for c in module_classifications() {
        *counts.entry(c.tier).or_insert(0) += 1;
    }
    counts
}

/// Check which enabled features correspond to non-production-safe modules.
///
/// Returns a list of `(module_name, tier)` pairs for any enabled feature-gated
/// module that is below the `Beta` tier (i.e., Preview, Experimental, or Stub).
///
/// Use this at startup to log warnings about non-production modules in use.
///
/// # Examples
///
/// ```
/// let warnings = isolate_core::stability::stability_check();
/// for (name, tier) in &warnings {
///     eprintln!("Warning: module '{}' is {} — not production-safe", name, tier);
/// }
/// ```
pub fn stability_check() -> Vec<(&'static str, StabilityTier)> {
    let mut warnings = Vec::new();

    for classification in module_classifications() {
        if classification.tier.is_production_safe() {
            continue;
        }

        // Check if the feature is enabled at compile time
        let enabled = match classification.feature_flag {
            None => true, // Always-on module
            Some(flag) => is_feature_enabled(flag),
        };

        if enabled {
            warnings.push((classification.name, classification.tier));
        }
    }

    warnings
}

/// Log stability warnings for all enabled non-production modules.
///
/// Call this once at application startup to surface which experimental
/// or stub modules are compiled in.
pub fn log_stability_warnings() {
    let warnings = stability_check();
    if warnings.is_empty() {
        return;
    }

    for (name, tier) in &warnings {
        tracing::warn!(
            module = name,
            tier = %tier,
            "Non-production module enabled: '{}' is {}",
            name,
            tier.label()
        );
    }
}

fn is_feature_enabled(flag: &str) -> bool {
    match flag {
        "pool" => cfg!(feature = "pool"),
        "networking" => cfg!(feature = "networking"),
        "agent" => cfg!(feature = "agent"),
        "policy-engine" => cfg!(feature = "policy-engine"),
        "platform" => cfg!(feature = "platform"),
        "platform-admin" => cfg!(feature = "platform-admin"),
        "platform-storage" => cfg!(feature = "platform-storage"),
        "platform-workflow" => cfg!(feature = "platform-workflow"),
        "platform-provenance" => cfg!(feature = "platform-provenance"),
        "platform-comm" => cfg!(feature = "platform-comm"),
        "platform-hosting" => cfg!(feature = "platform-hosting"),
        "platform-infra" => cfg!(feature = "platform-infra"),
        "extras" => cfg!(feature = "extras"),
        "observability" => cfg!(feature = "observability"),
        "billing" => cfg!(feature = "billing"),
        "deployment" => cfg!(feature = "deployment"),
        "federation" => cfg!(feature = "federation"),
        "serverless" => cfg!(feature = "serverless"),
        "snapshots" => cfg!(feature = "snapshots"),
        "wasi-preview2" => cfg!(feature = "wasi-preview2"),
        "debug-support" => cfg!(feature = "debug-support"),
        "module-signing" => cfg!(feature = "module-signing"),
        "kubernetes" => cfg!(feature = "kubernetes"),
        "otel-telemetry" => cfg!(feature = "otel-telemetry"),
        "ai-detection" => cfg!(feature = "ai-detection"),
        "nlp-policies" => cfg!(feature = "nlp-policies"),
        "hotpatch" => cfg!(feature = "hotpatch"),
        "distributed-mesh" => cfg!(feature = "distributed-mesh"),
        "gpu-compute" => cfg!(feature = "gpu-compute"),
        "chaos-testing" => cfg!(feature = "chaos-testing"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classifications_not_empty() {
        assert!(!module_classifications().is_empty());
    }

    #[test]
    fn test_stable_modules_exist() {
        let stable = modules_by_tier(StabilityTier::Stable);
        assert!(!stable.is_empty());
        assert!(stable.contains(&"sandbox"));
        assert!(stable.contains(&"config"));
        assert!(stable.contains(&"capability"));
    }

    #[test]
    fn test_core_modules_are_stable() {
        assert_eq!(get_tier("sandbox"), Some(StabilityTier::Stable));
        assert_eq!(get_tier("config"), Some(StabilityTier::Stable));
        assert_eq!(get_tier("error"), Some(StabilityTier::Stable));
        assert_eq!(get_tier("engine"), Some(StabilityTier::Stable));
    }

    #[test]
    fn test_stubs_marked_correctly() {
        assert_eq!(get_tier("enclave"), Some(StabilityTier::Stub));
        assert_eq!(get_tier("gpu"), Some(StabilityTier::Stub));
        assert_eq!(get_tier("hotpatch"), Some(StabilityTier::Stub));
        assert_eq!(get_tier("mesh"), Some(StabilityTier::Stub));
    }

    #[test]
    fn test_preview_modules_no_real_impl() {
        for c in module_classifications().iter().filter(|c| c.tier == StabilityTier::Preview) {
            assert!(
                !c.has_real_implementation,
                "Preview '{}' should not claim real implementation",
                c.name
            );
        }
    }

    #[test]
    fn test_preview_tier_not_production_safe() {
        assert!(!StabilityTier::Preview.is_production_safe());
        assert!(StabilityTier::Preview.is_evaluation_safe());
    }

    #[test]
    fn test_experimental_has_feature_flags() {
        let classes = module_classifications();
        for c in classes.iter().filter(|c| c.tier == StabilityTier::Experimental) {
            assert!(c.feature_flag.is_some(), "{} should have a feature flag", c.name);
        }
    }

    #[test]
    fn test_stubs_not_production_safe() {
        assert!(!StabilityTier::Stub.is_production_safe());
        assert!(!StabilityTier::Experimental.is_production_safe());
        assert!(!StabilityTier::Preview.is_production_safe());
        assert!(StabilityTier::Stable.is_production_safe());
        assert!(StabilityTier::Beta.is_production_safe());
    }

    #[test]
    fn test_tier_display() {
        assert!(StabilityTier::Stable.to_string().contains("Stable"));
        assert!(StabilityTier::Stub.to_string().contains("Stub"));
    }

    #[test]
    fn test_get_tier_unknown() {
        assert_eq!(get_tier("nonexistent"), None);
    }

    #[test]
    fn test_tier_summary() {
        let summary = tier_summary();
        assert!(summary.len() == 5); // All 5 tiers
        assert!(*summary.get(&StabilityTier::Stable).unwrap() >= 5);
        assert!(*summary.get(&StabilityTier::Preview).unwrap() >= 10);
    }

    #[test]
    fn test_maturity_matrix_output() {
        let matrix = maturity_matrix();
        assert!(matrix.contains("# Module Maturity Matrix"));
        assert!(matrix.contains("Stable"));
        assert!(matrix.contains("Preview"));
        assert!(matrix.contains("Stub"));
        assert!(matrix.contains("sandbox"));
    }

    #[test]
    fn test_all_stubs_have_no_real_impl() {
        for c in module_classifications().iter().filter(|c| c.tier == StabilityTier::Stub) {
            assert!(
                !c.has_real_implementation,
                "Stub '{}' should not have real implementation",
                c.name
            );
        }
    }

    #[test]
    fn test_all_stable_have_real_impl() {
        for c in module_classifications().iter().filter(|c| c.tier == StabilityTier::Stable) {
            assert!(
                c.has_real_implementation,
                "Stable '{}' should have real implementation",
                c.name
            );
        }
    }

    #[test]
    fn test_tier_ordering() {
        assert!(StabilityTier::Stable < StabilityTier::Beta);
        assert!(StabilityTier::Beta < StabilityTier::Preview);
        assert!(StabilityTier::Preview < StabilityTier::Experimental);
        assert!(StabilityTier::Experimental < StabilityTier::Stub);
    }

    #[test]
    fn test_stability_check_returns_vec() {
        let warnings = stability_check();
        // With default features (no optional features enabled),
        // only always-on modules that are non-production should appear
        for (name, tier) in &warnings {
            assert!(
                !tier.is_production_safe(),
                "stability_check should only return non-production modules, got: {} ({})",
                name,
                tier.label()
            );
        }
    }

    #[test]
    fn test_is_feature_enabled_returns_false_for_unknown() {
        assert!(!is_feature_enabled("nonexistent-feature"));
    }

    #[test]
    fn test_log_stability_warnings_does_not_panic() {
        // Just verify it doesn't panic
        log_stability_warnings();
    }
}
