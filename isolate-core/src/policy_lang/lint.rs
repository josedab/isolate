//! Policy linter for validating and checking policy definitions.
//!
//! Detects common mistakes, security anti-patterns, and best practice
//! violations in policy definitions.

use super::parser::SandboxPolicy;
use serde::{Deserialize, Serialize};

/// Severity level for lint findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LintSeverity {
    /// Informational suggestion.
    Info,
    /// Warning that should be addressed.
    Warning,
    /// Error that must be fixed.
    Error,
}

impl std::fmt::Display for LintSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// A single lint finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintFinding {
    /// Lint rule ID.
    pub rule: String,
    /// Severity level.
    pub severity: LintSeverity,
    /// Human-readable message.
    pub message: String,
    /// Policy name this finding applies to.
    pub policy_name: String,
    /// Suggested fix.
    pub suggestion: Option<String>,
}

/// Result of linting a policy document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LintResult {
    /// All findings.
    pub findings: Vec<LintFinding>,
    /// Number of errors.
    pub error_count: usize,
    /// Number of warnings.
    pub warning_count: usize,
    /// Number of info findings.
    pub info_count: usize,
}

impl LintResult {
    /// Whether the policy passed linting (no errors).
    pub fn passed(&self) -> bool {
        self.error_count == 0
    }

    /// Get findings by severity.
    pub fn by_severity(&self, severity: LintSeverity) -> Vec<&LintFinding> {
        self.findings.iter().filter(|f| f.severity == severity).collect()
    }
}

/// Policy linter with configurable rules.
pub struct PolicyLinter {
    max_memory_bytes: usize,
    max_fuel: u64,
    require_timeout: bool,
    deny_wildcard_fs: bool,
    deny_wildcard_http: bool,
}

impl Default for PolicyLinter {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyLinter {
    /// Create a new linter with sensible defaults.
    pub fn new() -> Self {
        Self {
            max_memory_bytes: 4 * 1024 * 1024 * 1024, // 4GB
            max_fuel: 100_000_000_000,
            require_timeout: true,
            deny_wildcard_fs: true,
            deny_wildcard_http: true,
        }
    }

    /// Set maximum allowed memory.
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.max_memory_bytes = bytes;
        self
    }

    /// Set whether timeout is required.
    pub fn require_timeout(mut self, required: bool) -> Self {
        self.require_timeout = required;
        self
    }

    /// Lint a sandbox policy.
    pub fn lint(&self, policy: &SandboxPolicy) -> LintResult {
        let mut findings = Vec::new();
        let name = &policy.name;

        // Check resource block
        if let Some(ref res) = policy.resource {
            // Memory limit check
            if let Some(ref mem_str) = res.memory_limit {
                if let Some(bytes) = parse_size(mem_str) {
                    if bytes > self.max_memory_bytes {
                        findings.push(LintFinding {
                            rule: "resource/memory-too-high".to_string(),
                            severity: LintSeverity::Warning,
                            message: format!("Memory limit {} exceeds recommended maximum", mem_str),
                            policy_name: name.clone(),
                            suggestion: Some("Consider reducing memory limit".to_string()),
                        });
                    }
                    if bytes == 0 {
                        findings.push(LintFinding {
                            rule: "resource/zero-memory".to_string(),
                            severity: LintSeverity::Error,
                            message: "Memory limit is zero".to_string(),
                            policy_name: name.clone(),
                            suggestion: Some("Set a positive memory limit".to_string()),
                        });
                    }
                }
            } else {
                findings.push(LintFinding {
                    rule: "resource/no-memory-limit".to_string(),
                    severity: LintSeverity::Info,
                    message: "No explicit memory limit set; default will be used".to_string(),
                    policy_name: name.clone(),
                    suggestion: Some("Set memory_limit explicitly for clarity".to_string()),
                });
            }

            // Fuel check
            if let Some(fuel) = res.fuel {
                if fuel > self.max_fuel {
                    findings.push(LintFinding {
                        rule: "resource/fuel-too-high".to_string(),
                        severity: LintSeverity::Warning,
                        message: format!("Fuel limit {} is very high", fuel),
                        policy_name: name.clone(),
                        suggestion: None,
                    });
                }
            }

            // Timeout check
            if self.require_timeout && res.timeout.is_none() {
                findings.push(LintFinding {
                    rule: "resource/no-timeout".to_string(),
                    severity: LintSeverity::Warning,
                    message: "No timeout configured - sandbox may run indefinitely".to_string(),
                    policy_name: name.clone(),
                    suggestion: Some("Add timeout = \"30s\" to resource block".to_string()),
                });
            }
        } else {
            findings.push(LintFinding {
                rule: "resource/missing-block".to_string(),
                severity: LintSeverity::Info,
                message: "No resource block; all defaults will be used".to_string(),
                policy_name: name.clone(),
                suggestion: None,
            });
        }

        // Check capability block
        if let Some(ref cap) = policy.capability {
            // Wildcard filesystem check
            if self.deny_wildcard_fs {
                for path in &cap.fs_read {
                    if path == "/" || path == "/*" {
                        findings.push(LintFinding {
                            rule: "capability/wildcard-fs-read".to_string(),
                            severity: LintSeverity::Error,
                            message: format!("Wildcard filesystem read path '{}' grants access to entire filesystem", path),
                            policy_name: name.clone(),
                            suggestion: Some("Restrict to specific directories".to_string()),
                        });
                    }
                }
                for path in &cap.fs_write {
                    if path == "/" || path == "/*" {
                        findings.push(LintFinding {
                            rule: "capability/wildcard-fs-write".to_string(),
                            severity: LintSeverity::Error,
                            message: format!("Wildcard filesystem write path '{}' grants write access to entire filesystem", path),
                            policy_name: name.clone(),
                            suggestion: Some("Restrict to specific directories like /tmp".to_string()),
                        });
                    }
                }
            }
        }

        // Check network block
        if let Some(ref net) = policy.network {
            if self.deny_wildcard_http {
                for host in &net.allow_http {
                    if host == "*" {
                        findings.push(LintFinding {
                            rule: "network/wildcard-http".to_string(),
                            severity: LintSeverity::Warning,
                            message: "Wildcard HTTP access allows connecting to any host".to_string(),
                            policy_name: name.clone(),
                            suggestion: Some("Restrict to specific hostnames".to_string()),
                        });
                    }
                }
            }
        }

        // Count by severity
        let error_count = findings.iter().filter(|f| f.severity == LintSeverity::Error).count();
        let warning_count = findings.iter().filter(|f| f.severity == LintSeverity::Warning).count();
        let info_count = findings.iter().filter(|f| f.severity == LintSeverity::Info).count();

        LintResult { findings, error_count, warning_count, info_count }
    }
}

fn parse_size(s: &str) -> Option<usize> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("GB") {
        num.trim().parse::<usize>().ok().map(|n| n * 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("MB") {
        num.trim().parse::<usize>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("KB") {
        num.trim().parse::<usize>().ok().map(|n| n * 1024)
    } else {
        s.parse().ok()
    }
}

/// Simple policy test framework for verifying policies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyTest {
    /// Test name.
    pub name: String,
    /// Policy name to test.
    pub policy_name: String,
    /// Expected assertions.
    pub assertions: Vec<PolicyAssertion>,
}

/// An assertion about a policy's resolved state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyAssertion {
    /// Memory limit should be at most this many bytes.
    MemoryLimitAtMost(u64),
    /// Fuel should be set.
    FuelIsSet,
    /// Timeout should be set.
    TimeoutIsSet,
    /// Stdout should be allowed.
    StdoutAllowed,
    /// Filesystem read paths should include this path.
    FsReadIncludes(String),
    /// Network should be denied by default.
    NetworkDenied,
    /// Lint should pass (no errors).
    LintPasses,
}

/// Result of running a policy test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    /// Test name.
    pub name: String,
    /// Whether all assertions passed.
    pub passed: bool,
    /// Individual assertion results.
    pub assertion_results: Vec<(String, bool)>,
}

/// Run policy tests against resolved policies.
pub fn run_policy_tests(
    tests: &[PolicyTest],
    policies: &[SandboxPolicy],
) -> Vec<TestResult> {
    let evaluator = super::eval::PolicyEvaluator::new();
    let linter = PolicyLinter::new();

    tests.iter().map(|test| {
        let policy = policies.iter().find(|p| p.name == test.policy_name);
        let mut assertion_results = Vec::new();

        if let Some(policy) = policy {
            let resolved = evaluator.resolve(policy);
            let lint = linter.lint(policy);

            for assertion in &test.assertions {
                let (desc, passed) = match assertion {
                    PolicyAssertion::MemoryLimitAtMost(max) => {
                        let mem = resolved.as_ref().map(|r| r.memory_limit_bytes).unwrap_or(0);
                        (format!("memory <= {}", max), mem <= *max)
                    }
                    PolicyAssertion::FuelIsSet => {
                        let has = resolved.as_ref().map(|r| r.fuel.is_some()).unwrap_or(false);
                        ("fuel is set".to_string(), has)
                    }
                    PolicyAssertion::TimeoutIsSet => {
                        let has = resolved.as_ref().map(|r| r.timeout.is_some()).unwrap_or(false);
                        ("timeout is set".to_string(), has)
                    }
                    PolicyAssertion::StdoutAllowed => {
                        let allowed = resolved.as_ref().map(|r| r.allow_stdout).unwrap_or(false);
                        ("stdout allowed".to_string(), allowed)
                    }
                    PolicyAssertion::FsReadIncludes(path) => {
                        let has = resolved.as_ref()
                            .map(|r| r.fs_read_paths.contains(path))
                            .unwrap_or(false);
                        (format!("fs_read includes {}", path), has)
                    }
                    PolicyAssertion::NetworkDenied => {
                        let denied = resolved.as_ref().map(|r| r.network_deny_all).unwrap_or(true);
                        ("network denied".to_string(), denied)
                    }
                    PolicyAssertion::LintPasses => {
                        ("lint passes".to_string(), lint.passed())
                    }
                };
                assertion_results.push((desc, passed));
            }
        } else {
            assertion_results.push(("policy found".to_string(), false));
        }

        let passed = assertion_results.iter().all(|(_, p)| *p);
        TestResult {
            name: test.name.clone(),
            passed,
            assertion_results,
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_lang::PolicyParser;

    #[test]
    fn test_lint_clean_policy() {
        let input = r#"
            sandbox "clean" {
                resource {
                    memory_limit = "128MB"
                    fuel = 1000000
                    timeout = "30s"
                }
                capability {
                    allow_stdout = true
                    fs_read = ["/data"]
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();
        let linter = PolicyLinter::new();
        let result = linter.lint(&doc.policies[0]);

        assert!(result.passed());
        assert_eq!(result.error_count, 0);
    }

    #[test]
    fn test_lint_wildcard_fs() {
        let input = r#"
            sandbox "risky" {
                resource {
                    memory_limit = "128MB"
                    timeout = "30s"
                }
                capability {
                    fs_read = ["/"]
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();
        let linter = PolicyLinter::new();
        let result = linter.lint(&doc.policies[0]);

        assert!(!result.passed());
        assert!(result.findings.iter().any(|f| f.rule == "capability/wildcard-fs-read"));
    }

    #[test]
    fn test_lint_no_timeout() {
        let input = r#"
            sandbox "no-timeout" {
                resource {
                    memory_limit = "128MB"
                    fuel = 1000000
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();
        let linter = PolicyLinter::new();
        let result = linter.lint(&doc.policies[0]);

        assert!(result.findings.iter().any(|f| f.rule == "resource/no-timeout"));
    }

    #[test]
    fn test_lint_wildcard_http() {
        let input = r#"
            sandbox "open-net" {
                resource {
                    memory_limit = "128MB"
                    timeout = "30s"
                }
                network {
                    allow_http = ["*"]
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();
        let linter = PolicyLinter::new();
        let result = linter.lint(&doc.policies[0]);

        assert!(result.findings.iter().any(|f| f.rule == "network/wildcard-http"));
    }

    #[test]
    fn test_policy_test_framework() {
        let input = r#"
            sandbox "test-policy" {
                resource {
                    memory_limit = "256MB"
                    fuel = 2000000
                    timeout = "60s"
                }
                capability {
                    allow_stdout = true
                    fs_read = ["/data"]
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();

        let tests = vec![PolicyTest {
            name: "verify-test-policy".to_string(),
            policy_name: "test-policy".to_string(),
            assertions: vec![
                PolicyAssertion::MemoryLimitAtMost(512 * 1024 * 1024),
                PolicyAssertion::FuelIsSet,
                PolicyAssertion::TimeoutIsSet,
                PolicyAssertion::StdoutAllowed,
                PolicyAssertion::FsReadIncludes("/data".to_string()),
                PolicyAssertion::LintPasses,
            ],
        }];

        let results = run_policy_tests(&tests, &doc.policies);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn test_policy_test_failure() {
        let input = r#"
            sandbox "limited" {
                resource {
                    memory_limit = "128MB"
                }
            }
        "#;

        let doc = PolicyParser::parse(input).unwrap();

        let tests = vec![PolicyTest {
            name: "check-fuel".to_string(),
            policy_name: "limited".to_string(),
            assertions: vec![PolicyAssertion::FuelIsSet],
        }];

        let results = run_policy_tests(&tests, &doc.policies);
        assert!(!results[0].passed);
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("128MB"), Some(128 * 1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("512KB"), Some(512 * 1024));
        assert_eq!(parse_size("1024"), Some(1024));
    }

    #[test]
    fn test_lint_severity_ordering() {
        assert!(LintSeverity::Info < LintSeverity::Warning);
        assert!(LintSeverity::Warning < LintSeverity::Error);
    }
}
