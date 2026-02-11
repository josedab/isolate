//! Production readiness assessment for WASI Preview2 features.
//!
//! Provides tooling to evaluate whether a Preview2 component meets production
//! deployment criteria including capability coverage, compatibility, and
//! performance characteristics.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Stability level for Preview2 features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StabilityLevel {
    /// Experimental: API may change, not for production.
    Experimental,
    /// Preview: API is stabilizing, suitable for non-critical workloads.
    Preview,
    /// Stable: API is frozen, suitable for production.
    Stable,
}

impl std::fmt::Display for StabilityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Experimental => write!(f, "experimental"),
            Self::Preview => write!(f, "preview"),
            Self::Stable => write!(f, "stable"),
        }
    }
}

/// Catalog of Preview2 interface stability levels.
pub struct InterfaceStability;

impl InterfaceStability {
    /// Get the stability level for a given WASI interface.
    pub fn level(interface: &str) -> StabilityLevel {
        match interface {
            "wasi:cli/run" | "wasi:cli/stdin" | "wasi:cli/stdout" | "wasi:cli/stderr" => {
                StabilityLevel::Stable
            }
            "wasi:cli/environment" | "wasi:cli/exit" => StabilityLevel::Stable,
            "wasi:filesystem/types" | "wasi:filesystem/preopens" => StabilityLevel::Stable,
            "wasi:clocks/wall-clock" | "wasi:clocks/monotonic-clock" => StabilityLevel::Stable,
            "wasi:random/random" | "wasi:random/insecure" => StabilityLevel::Stable,
            "wasi:io/streams" | "wasi:io/poll" => StabilityLevel::Stable,
            "wasi:sockets/tcp" | "wasi:sockets/udp" => StabilityLevel::Preview,
            "wasi:sockets/ip-name-lookup" => StabilityLevel::Preview,
            "wasi:http/types" | "wasi:http/outgoing-handler" => StabilityLevel::Preview,
            "wasi:http/incoming-handler" => StabilityLevel::Experimental,
            "wasi:keyvalue/store" | "wasi:keyvalue/batch" => StabilityLevel::Experimental,
            _ => StabilityLevel::Experimental,
        }
    }

    /// Get all known interfaces and their stability levels.
    pub fn all() -> HashMap<&'static str, StabilityLevel> {
        let interfaces = [
            "wasi:cli/run",
            "wasi:cli/stdin",
            "wasi:cli/stdout",
            "wasi:cli/stderr",
            "wasi:cli/environment",
            "wasi:cli/exit",
            "wasi:filesystem/types",
            "wasi:filesystem/preopens",
            "wasi:clocks/wall-clock",
            "wasi:clocks/monotonic-clock",
            "wasi:random/random",
            "wasi:random/insecure",
            "wasi:io/streams",
            "wasi:io/poll",
            "wasi:sockets/tcp",
            "wasi:sockets/udp",
            "wasi:sockets/ip-name-lookup",
            "wasi:http/types",
            "wasi:http/outgoing-handler",
            "wasi:http/incoming-handler",
            "wasi:keyvalue/store",
            "wasi:keyvalue/batch",
        ];
        interfaces.iter().map(|&i| (i, Self::level(i))).collect()
    }
}

/// Assessment result for a component's production readiness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessAssessment {
    /// Overall readiness: true if all used interfaces are Stable or Preview.
    pub is_production_ready: bool,
    /// Minimum stability level across all used interfaces.
    pub minimum_stability: StabilityLevel,
    /// Interfaces used by the component and their stability levels.
    pub interface_levels: HashMap<String, StabilityLevel>,
    /// Warnings about non-stable interfaces.
    pub warnings: Vec<String>,
    /// Recommendations for production deployment.
    pub recommendations: Vec<String>,
}

impl ReadinessAssessment {
    /// Assess production readiness from a list of required WASI interfaces.
    pub fn evaluate(required_interfaces: &[String]) -> Self {
        let mut interface_levels = HashMap::new();
        let mut warnings = Vec::new();
        let mut recommendations = Vec::new();
        let mut min_stability = StabilityLevel::Stable;

        for iface in required_interfaces {
            let level = InterfaceStability::level(iface);
            interface_levels.insert(iface.clone(), level);

            if level < min_stability {
                min_stability = level;
            }

            match level {
                StabilityLevel::Experimental => {
                    warnings.push(format!(
                        "Interface '{}' is experimental and may change without notice",
                        iface
                    ));
                }
                StabilityLevel::Preview => {
                    recommendations.push(format!(
                        "Interface '{}' is in preview - pin your Isolate version for stability",
                        iface
                    ));
                }
                StabilityLevel::Stable => {}
            }
        }

        if min_stability == StabilityLevel::Experimental {
            recommendations.push(
                "Consider using Preview1 compatibility mode for production workloads".to_string(),
            );
        }

        let is_production_ready = min_stability >= StabilityLevel::Preview;

        Self {
            is_production_ready,
            minimum_stability: min_stability,
            interface_levels,
            warnings,
            recommendations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_stability_levels() {
        assert_eq!(InterfaceStability::level("wasi:cli/stdout"), StabilityLevel::Stable);
        assert_eq!(InterfaceStability::level("wasi:sockets/tcp"), StabilityLevel::Preview);
        assert_eq!(InterfaceStability::level("wasi:http/incoming-handler"), StabilityLevel::Experimental);
        assert_eq!(InterfaceStability::level("unknown:interface"), StabilityLevel::Experimental);
    }

    #[test]
    fn test_readiness_assessment_stable() {
        let interfaces = vec![
            "wasi:cli/stdout".to_string(),
            "wasi:cli/stderr".to_string(),
            "wasi:filesystem/types".to_string(),
        ];
        let assessment = ReadinessAssessment::evaluate(&interfaces);
        assert!(assessment.is_production_ready);
        assert_eq!(assessment.minimum_stability, StabilityLevel::Stable);
        assert!(assessment.warnings.is_empty());
    }

    #[test]
    fn test_readiness_assessment_preview() {
        let interfaces = vec![
            "wasi:cli/stdout".to_string(),
            "wasi:sockets/tcp".to_string(),
        ];
        let assessment = ReadinessAssessment::evaluate(&interfaces);
        assert!(assessment.is_production_ready);
        assert_eq!(assessment.minimum_stability, StabilityLevel::Preview);
        assert!(!assessment.recommendations.is_empty());
    }

    #[test]
    fn test_readiness_assessment_experimental() {
        let interfaces = vec![
            "wasi:cli/stdout".to_string(),
            "wasi:http/incoming-handler".to_string(),
        ];
        let assessment = ReadinessAssessment::evaluate(&interfaces);
        assert!(!assessment.is_production_ready);
        assert_eq!(assessment.minimum_stability, StabilityLevel::Experimental);
        assert!(!assessment.warnings.is_empty());
    }

    #[test]
    fn test_stability_ordering() {
        assert!(StabilityLevel::Experimental < StabilityLevel::Preview);
        assert!(StabilityLevel::Preview < StabilityLevel::Stable);
    }

    #[test]
    fn test_all_interfaces() {
        let all = InterfaceStability::all();
        assert!(all.len() >= 20);
        assert_eq!(all.get("wasi:cli/run"), Some(&StabilityLevel::Stable));
    }
}
