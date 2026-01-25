//! Module stability tier classification.
//!
//! Classifies all isolate-core modules into stability tiers so users
//! know which APIs are safe for production use.
//!
//! # Tier Definitions
//!
//! - **Stable**: Production-ready, backwards-compatible, well-tested
//! - **Beta**: Feature-complete but API may change, good test coverage
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

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Beta => "Beta",
            Self::Experimental => "Experimental",
            Self::Stub => "Stub",
        }
    }

    /// Emoji indicator.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Stable => "✅",
            Self::Beta => "🔧",
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
        // === Stable ===
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
        // === Beta ===
        ModuleClassification {
            name: "vfs",
            tier: StabilityTier::Beta,
            description: "Virtual filesystem with quotas",
            feature_flag: None,
            test_count: 44,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "security",
            tier: StabilityTier::Beta,
            description: "Security policies and syscall filtering",
            feature_flag: None,
            test_count: 48,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "pool",
            tier: StabilityTier::Beta,
            description: "Multi-tenant resource pooling with warm pool",
            feature_flag: None,
            test_count: 43,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "policy",
            tier: StabilityTier::Beta,
            description: "Declarative policy engine with bundles",
            feature_flag: None,
            test_count: 35,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "network",
            tier: StabilityTier::Beta,
            description: "Network policies, TCP, DNS",
            feature_flag: None,
            test_count: 15,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "ipc",
            tier: StabilityTier::Beta,
            description: "Inter-sandbox communication",
            feature_flag: None,
            test_count: 37,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "kv",
            tier: StabilityTier::Beta,
            description: "Key-value store for sandboxes",
            feature_flag: None,
            test_count: 17,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "audit",
            tier: StabilityTier::Beta,
            description: "Audit logging and verification",
            feature_flag: None,
            test_count: 10,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "workflow",
            tier: StabilityTier::Beta,
            description: "Workflow orchestration and pipeline DSL",
            feature_flag: None,
            test_count: 23,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "orchestrator",
            tier: StabilityTier::Beta,
            description: "Scheduler and admission control",
            feature_flag: None,
            test_count: 28,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "marketplace",
            tier: StabilityTier::Beta,
            description: "Module registry and dependency resolution",
            feature_flag: None,
            test_count: 23,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "compose",
            tier: StabilityTier::Beta,
            description: "Module composition and linking",
            feature_flag: None,
            test_count: 25,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "secrets",
            tier: StabilityTier::Beta,
            description: "Secrets management",
            feature_flag: None,
            test_count: 13,
            has_real_implementation: true,
        },
        // === Experimental ===
        ModuleClassification {
            name: "wasi2",
            tier: StabilityTier::Experimental,
            description: "WASI Preview2 component model",
            feature_flag: Some("wasi-preview2"),
            test_count: 63,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "snapshot",
            tier: StabilityTier::Experimental,
            description: "Snapshot/restore with manager",
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
            description: "Cryptographic module signing",
            feature_flag: Some("module-signing"),
            test_count: 10,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "k8s",
            tier: StabilityTier::Experimental,
            description: "Kubernetes operator and CRDs",
            feature_flag: Some("kubernetes"),
            test_count: 30,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "telemetry",
            tier: StabilityTier::Experimental,
            description: "OpenTelemetry distributed tracing",
            feature_flag: Some("otel-telemetry"),
            test_count: 5,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "nlp",
            tier: StabilityTier::Experimental,
            description: "Natural language policy parsing",
            feature_flag: Some("nlp-policies"),
            test_count: 16,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "carbon",
            tier: StabilityTier::Experimental,
            description: "Carbon-aware scheduling",
            feature_flag: None,
            test_count: 13,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "chaos",
            tier: StabilityTier::Experimental,
            description: "Chaos engineering and fault injection",
            feature_flag: Some("chaos-testing"),
            test_count: 11,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "predict",
            tier: StabilityTier::Experimental,
            description: "Predictive resource scaling",
            feature_flag: None,
            test_count: 4,
            has_real_implementation: true,
        },
        ModuleClassification {
            name: "mesh",
            tier: StabilityTier::Experimental,
            description: "Distributed sandbox mesh",
            feature_flag: Some("distributed-mesh"),
            test_count: 83,
            has_real_implementation: true,
        },
        // === Stubs ===
        ModuleClassification {
            name: "enclave",
            tier: StabilityTier::Stub,
            description: "TEE/SGX enclave (simulated)",
            feature_flag: None,
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
            name: "ai",
            tier: StabilityTier::Stub,
            description: "AI anomaly detection (framework only)",
            feature_flag: Some("ai-detection"),
            test_count: 29,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "agent",
            tier: StabilityTier::Stub,
            description: "AI agent SDK (skeleton)",
            feature_flag: None,
            test_count: 12,
            has_real_implementation: false,
        },
        ModuleClassification {
            name: "gateway",
            tier: StabilityTier::Stub,
            description: "API gateway (partial)",
            feature_flag: None,
            test_count: 13,
            has_real_implementation: false,
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
    }

    #[test]
    fn test_experimental_has_feature_flags() {
        let classes = module_classifications();
        for c in classes.iter().filter(|c| c.tier == StabilityTier::Experimental) {
            // Most experimental modules have feature flags
            // (carbon and predict are exceptions as they're always compiled)
            if c.name != "carbon" && c.name != "predict" {
                assert!(c.feature_flag.is_some(), "{} should have a feature flag", c.name);
            }
        }
    }

    #[test]
    fn test_stubs_not_production_safe() {
        assert!(!StabilityTier::Stub.is_production_safe());
        assert!(!StabilityTier::Experimental.is_production_safe());
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
        assert!(summary.len() == 4); // All 4 tiers
        assert!(*summary.get(&StabilityTier::Stable).unwrap() >= 5);
    }

    #[test]
    fn test_maturity_matrix_output() {
        let matrix = maturity_matrix();
        assert!(matrix.contains("# Module Maturity Matrix"));
        assert!(matrix.contains("Stable"));
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
        assert!(StabilityTier::Beta < StabilityTier::Experimental);
        assert!(StabilityTier::Experimental < StabilityTier::Stub);
    }
}
