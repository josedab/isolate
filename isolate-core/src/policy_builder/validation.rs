//! Policy validation engine.

use serde::{Deserialize, Serialize};

use super::ir::{BlockKind, PolicyIR, ResourceBlock};

/// Severity of a validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

/// A validation issue found in a policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub block_id: Option<String>,
    pub severity: IssueSeverity,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Validates policy IR for correctness and best practices.
pub struct PolicyValidator {
    max_memory: u64,
    max_fuel: u64,
    max_timeout_ms: u64,
}

impl PolicyValidator {
    pub fn new() -> Self {
        Self {
            max_memory: 512 * 1024 * 1024, // 512MB
            max_fuel: 100_000_000,
            max_timeout_ms: 300_000, // 5 minutes
        }
    }

    /// Validate a policy IR and return all issues found.
    pub fn validate(&self, ir: &PolicyIR) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        if ir.name.is_empty() {
            issues.push(ValidationIssue {
                block_id: None,
                severity: IssueSeverity::Error,
                message: "Policy name cannot be empty".into(),
                suggestion: Some("Provide a descriptive name for the policy".into()),
            });
        }

        if ir.blocks.is_empty() {
            issues.push(ValidationIssue {
                block_id: None,
                severity: IssueSeverity::Warning,
                message: "Policy has no blocks defined".into(),
                suggestion: Some("Add at least a resource block".into()),
            });
        }

        let mut has_resource = false;
        for block in &ir.blocks {
            if !block.enabled {
                continue;
            }
            match &block.kind {
                BlockKind::Resource(r) => {
                    has_resource = true;
                    self.validate_resource(r, &block.id, &mut issues);
                }
                BlockKind::Capability(c) => {
                    if !c.filesystem_write.is_empty() && c.filesystem_read.is_empty() {
                        issues.push(ValidationIssue {
                            block_id: Some(block.id.clone()),
                            severity: IssueSeverity::Warning,
                            message: "Write access granted without explicit read paths".into(),
                            suggestion: Some(
                                "Consider adding read paths for write directories".into(),
                            ),
                        });
                    }
                }
                BlockKind::Network(n) => {
                    if n.allow_outbound && n.allowed_hosts.is_empty() {
                        issues.push(ValidationIssue {
                            block_id: Some(block.id.clone()),
                            severity: IssueSeverity::Warning,
                            message: "Outbound network enabled with no host restrictions".into(),
                            suggestion: Some(
                                "Restrict to specific hosts for better security".into(),
                            ),
                        });
                    }
                }
                BlockKind::Environment(e) => {
                    if e.inherit {
                        issues.push(ValidationIssue {
                            block_id: Some(block.id.clone()),
                            severity: IssueSeverity::Warning,
                            message: "Inheriting host environment variables".into(),
                            suggestion: Some(
                                "Use explicit variables or passthrough for security".into(),
                            ),
                        });
                    }
                }
            }
        }

        if !has_resource {
            issues.push(ValidationIssue {
                block_id: None,
                severity: IssueSeverity::Warning,
                message: "No resource limits defined; sandbox will use defaults".into(),
                suggestion: Some("Add a resource block to control memory and CPU limits".into()),
            });
        }

        issues
    }

    fn validate_resource(
        &self,
        r: &ResourceBlock,
        block_id: &str,
        issues: &mut Vec<ValidationIssue>,
    ) {
        if let Some(mem) = r.max_memory_bytes {
            if mem > self.max_memory {
                issues.push(ValidationIssue {
                    block_id: Some(block_id.to_string()),
                    severity: IssueSeverity::Error,
                    message: format!(
                        "Memory limit {}MB exceeds maximum {}MB",
                        mem / (1024 * 1024),
                        self.max_memory / (1024 * 1024)
                    ),
                    suggestion: Some(format!(
                        "Reduce to at most {}MB",
                        self.max_memory / (1024 * 1024)
                    )),
                });
            }
            if mem < 1024 * 1024 {
                issues.push(ValidationIssue {
                    block_id: Some(block_id.to_string()),
                    severity: IssueSeverity::Warning,
                    message: "Memory limit below 1MB may be too restrictive".into(),
                    suggestion: Some("Most WASM modules need at least 1MB".into()),
                });
            }
        }

        if let Some(fuel) = r.max_fuel {
            if fuel > self.max_fuel {
                issues.push(ValidationIssue {
                    block_id: Some(block_id.to_string()),
                    severity: IssueSeverity::Error,
                    message: format!("Fuel limit {} exceeds maximum {}", fuel, self.max_fuel),
                    suggestion: None,
                });
            }
        }

        if let Some(timeout) = r.timeout_ms {
            if timeout > self.max_timeout_ms {
                issues.push(ValidationIssue {
                    block_id: Some(block_id.to_string()),
                    severity: IssueSeverity::Error,
                    message: format!(
                        "Timeout {}ms exceeds maximum {}ms",
                        timeout, self.max_timeout_ms
                    ),
                    suggestion: None,
                });
            }
        }
    }
}

impl Default for PolicyValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_builder::ir::*;

    #[test]
    fn test_valid_policy() {
        let ir = PolicyIR::new("valid")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())))
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock::default())));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().all(|i| i.severity != IssueSeverity::Error));
    }

    #[test]
    fn test_empty_name() {
        let ir = PolicyIR::new("");
        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("name")));
    }

    #[test]
    fn test_excessive_memory() {
        let ir = PolicyIR::new("big").add_block(PolicyBlock::new(
            "res",
            BlockKind::Resource(ResourceBlock {
                max_memory_bytes: Some(1024 * 1024 * 1024), // 1GB
                ..Default::default()
            }),
        ));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues
            .iter()
            .any(|i| i.severity == IssueSeverity::Error && i.message.contains("Memory")));
    }

    #[test]
    fn test_open_network_warning() {
        let ir = PolicyIR::new("open-net")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())))
            .add_block(PolicyBlock::new(
                "net",
                BlockKind::Network(NetworkBlock { allow_outbound: true, ..Default::default() }),
            ));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().any(|i| i.message.contains("host restrictions")));
    }

    #[test]
    fn test_env_inherit_warning() {
        let ir = PolicyIR::new("env")
            .add_block(PolicyBlock::new("res", BlockKind::Resource(ResourceBlock::default())))
            .add_block(PolicyBlock::new(
                "env",
                BlockKind::Environment(EnvironmentBlock { inherit: true, ..Default::default() }),
            ));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().any(|i| i.message.contains("Inheriting")));
    }

    #[test]
    fn test_no_resource_block_warning() {
        let ir = PolicyIR::new("no-res")
            .add_block(PolicyBlock::new("cap", BlockKind::Capability(CapabilityBlock::default())));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().any(|i| i.message.contains("No resource limits")));
    }

    #[test]
    fn test_tiny_memory_warning() {
        let ir = PolicyIR::new("tiny").add_block(PolicyBlock::new(
            "res",
            BlockKind::Resource(ResourceBlock {
                max_memory_bytes: Some(512), // 512 bytes
                ..Default::default()
            }),
        ));

        let validator = PolicyValidator::new();
        let issues = validator.validate(&ir);
        assert!(issues.iter().any(|i| i.message.contains("below 1MB")));
    }
}
