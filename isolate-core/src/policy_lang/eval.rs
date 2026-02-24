use super::parser::{ParseError, SandboxPolicy};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A fully resolved policy ready for sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedPolicy {
    pub name: String,
    pub memory_limit_bytes: u64,
    pub fuel: Option<u64>,
    pub timeout: Option<Duration>,
    pub max_io_bytes: Option<u64>,
    pub allow_stdout: bool,
    pub allow_stderr: bool,
    pub allow_stdin: bool,
    pub allow_env: bool,
    pub allow_clock: bool,
    pub allow_random: bool,
    pub fs_read_paths: Vec<String>,
    pub fs_write_paths: Vec<String>,
    pub allow_dns: bool,
    pub allow_http_hosts: Vec<String>,
    pub allow_tcp_hosts: Vec<String>,
    pub network_deny_all: bool,
}

impl Default for ResolvedPolicy {
    fn default() -> Self {
        Self {
            name: String::new(),
            memory_limit_bytes: 64 * 1024 * 1024, // 64MB default
            fuel: None,
            timeout: None,
            max_io_bytes: None,
            allow_stdout: false,
            allow_stderr: false,
            allow_stdin: false,
            allow_env: false,
            allow_clock: false,
            allow_random: false,
            fs_read_paths: Vec::new(),
            fs_write_paths: Vec::new(),
            allow_dns: false,
            allow_http_hosts: Vec::new(),
            allow_tcp_hosts: Vec::new(),
            network_deny_all: true,
        }
    }
}

/// Evaluates parsed policies into resolved configurations.
pub struct PolicyEvaluator {
    defaults: ResolvedPolicy,
}

impl PolicyEvaluator {
    pub fn new() -> Self {
        Self {
            defaults: ResolvedPolicy::default(),
        }
    }

    pub fn with_defaults(mut self, defaults: ResolvedPolicy) -> Self {
        self.defaults = defaults;
        self
    }

    /// Resolve a parsed sandbox policy into concrete configuration values.
    pub fn resolve(&self, policy: &SandboxPolicy) -> Result<ResolvedPolicy, ParseError> {
        let mut resolved = self.defaults.clone();
        resolved.name = policy.name.clone();

        if let Some(ref res) = policy.resource {
            if let Some(ref mem) = res.memory_limit {
                resolved.memory_limit_bytes = parse_byte_size(mem).ok_or_else(|| ParseError {
                    message: format!("invalid memory size: '{mem}'"),
                    line: 0,
                    col: 0,
                })?;
            }
            if let Some(fuel) = res.fuel {
                resolved.fuel = Some(fuel);
            }
            if let Some(ref timeout) = res.timeout {
                resolved.timeout = Some(parse_duration(timeout).ok_or_else(|| ParseError {
                    message: format!("invalid duration: '{timeout}'"),
                    line: 0,
                    col: 0,
                })?);
            }
            resolved.max_io_bytes = res.max_io_bytes.or(resolved.max_io_bytes);
        }

        if let Some(ref cap) = policy.capability {
            resolved.allow_stdout = cap.allow_stdout.unwrap_or(resolved.allow_stdout);
            resolved.allow_stderr = cap.allow_stderr.unwrap_or(resolved.allow_stderr);
            resolved.allow_stdin = cap.allow_stdin.unwrap_or(resolved.allow_stdin);
            resolved.allow_env = cap.allow_env.unwrap_or(resolved.allow_env);
            resolved.allow_clock = cap.allow_clock.unwrap_or(resolved.allow_clock);
            resolved.allow_random = cap.allow_random.unwrap_or(resolved.allow_random);
            if !cap.fs_read.is_empty() {
                resolved.fs_read_paths = cap.fs_read.clone();
            }
            if !cap.fs_write.is_empty() {
                resolved.fs_write_paths = cap.fs_write.clone();
            }
        }

        if let Some(ref net) = policy.network {
            resolved.allow_dns = net.allow_dns.unwrap_or(resolved.allow_dns);
            resolved.network_deny_all = net.deny_all.unwrap_or(resolved.network_deny_all);
            if !net.allow_http.is_empty() {
                resolved.allow_http_hosts = net.allow_http.clone();
            }
            if !net.allow_tcp.is_empty() {
                resolved.allow_tcp_hosts = net.allow_tcp.clone();
            }
        }

        Ok(resolved)
    }
}

impl Default for PolicyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a human-readable byte size string (e.g., "128MB", "1GB", "4096KB").
fn parse_byte_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num_str, unit) = if s.ends_with("GB") || s.ends_with("gb") {
        (&s[..s.len() - 2], 1024u64 * 1024 * 1024)
    } else if s.ends_with("MB") || s.ends_with("mb") {
        (&s[..s.len() - 2], 1024u64 * 1024)
    } else if s.ends_with("KB") || s.ends_with("kb") {
        (&s[..s.len() - 2], 1024u64)
    } else if s.ends_with('B') || s.ends_with('b') {
        (&s[..s.len() - 1], 1u64)
    } else {
        // Plain number = bytes
        return s.parse::<u64>().ok();
    };
    let num: u64 = num_str.trim().parse().ok()?;
    Some(num * unit)
}

/// Parse a human-readable duration string (e.g., "30s", "5m", "1h").
fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num_str, multiplier) = if let Some(stripped) = s.strip_suffix("ms") {
        (stripped, 1u64)
    } else if let Some(stripped) = s.strip_suffix('s') {
        (stripped, 1000u64)
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, 60_000u64)
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, 3_600_000u64)
    } else {
        // Default: seconds
        return s.parse::<u64>().ok().map(Duration::from_secs);
    };
    let num: u64 = num_str.trim().parse().ok()?;
    Some(Duration::from_millis(num * multiplier))
}

/// Versioned policy for tracking changes over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionedPolicy {
    /// Policy name.
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// The resolved policy.
    pub policy: ResolvedPolicy,
    /// Parent version (for inheritance).
    pub parent: Option<String>,
    /// Creation timestamp (ISO 8601).
    pub created_at: String,
    /// Description of changes.
    pub changelog: Option<String>,
}

/// Composes multiple policies with inheritance and conflict resolution.
pub struct PolicyComposer {
    /// Conflict resolution strategy.
    strategy: ConflictStrategy,
}

/// Strategy for resolving conflicts when composing policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStrategy {
    /// Use the most restrictive value.
    MostRestrictive,
    /// Use the least restrictive value.
    LeastRestrictive,
    /// Later policies override earlier ones.
    LastWins,
}

impl PolicyComposer {
    /// Create a new composer with the given conflict strategy.
    pub fn new(strategy: ConflictStrategy) -> Self {
        Self { strategy }
    }

    /// Compose multiple resolved policies into one.
    pub fn compose(&self, policies: &[ResolvedPolicy]) -> ResolvedPolicy {
        if policies.is_empty() {
            return ResolvedPolicy::default();
        }
        if policies.len() == 1 {
            return policies[0].clone();
        }

        let mut result = policies[0].clone();
        for policy in &policies[1..] {
            self.merge(&mut result, policy);
        }
        result
    }

    /// Apply a child policy on top of a parent (inheritance).
    pub fn inherit(parent: &ResolvedPolicy, child: &ResolvedPolicy) -> ResolvedPolicy {
        let mut result = parent.clone();
        result.name = child.name.clone();

        // Child overrides parent for all explicitly set fields
        if child.memory_limit_bytes != ResolvedPolicy::default().memory_limit_bytes {
            result.memory_limit_bytes = child.memory_limit_bytes;
        }
        if child.fuel.is_some() {
            result.fuel = child.fuel;
        }
        if child.timeout.is_some() {
            result.timeout = child.timeout;
        }
        if child.max_io_bytes.is_some() {
            result.max_io_bytes = child.max_io_bytes;
        }
        if child.allow_stdout {
            result.allow_stdout = true;
        }
        if child.allow_stderr {
            result.allow_stderr = true;
        }
        if child.allow_stdin {
            result.allow_stdin = true;
        }
        if !child.fs_read_paths.is_empty() {
            result.fs_read_paths = child.fs_read_paths.clone();
        }
        if !child.fs_write_paths.is_empty() {
            result.fs_write_paths = child.fs_write_paths.clone();
        }
        if !child.allow_http_hosts.is_empty() {
            result.allow_http_hosts = child.allow_http_hosts.clone();
        }
        if !child.network_deny_all {
            result.network_deny_all = false;
        }

        result
    }

    fn merge(&self, base: &mut ResolvedPolicy, other: &ResolvedPolicy) {
        match self.strategy {
            ConflictStrategy::MostRestrictive => {
                base.memory_limit_bytes = base.memory_limit_bytes.min(other.memory_limit_bytes);
                base.fuel = match (base.fuel, other.fuel) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                base.timeout = match (base.timeout, other.timeout) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                // Most restrictive = deny wins
                base.allow_stdout = base.allow_stdout && other.allow_stdout;
                base.allow_stderr = base.allow_stderr && other.allow_stderr;
                base.allow_stdin = base.allow_stdin && other.allow_stdin;
                base.network_deny_all = base.network_deny_all || other.network_deny_all;
            }
            ConflictStrategy::LeastRestrictive => {
                base.memory_limit_bytes = base.memory_limit_bytes.max(other.memory_limit_bytes);
                base.fuel = match (base.fuel, other.fuel) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None, // No limit = least restrictive
                };
                base.allow_stdout = base.allow_stdout || other.allow_stdout;
                base.allow_stderr = base.allow_stderr || other.allow_stderr;
                base.allow_stdin = base.allow_stdin || other.allow_stdin;
                base.network_deny_all = base.network_deny_all && other.network_deny_all;
            }
            ConflictStrategy::LastWins => {
                *base = other.clone();
            }
        }
    }
}

/// Compliance report for a resolved policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    /// Policy name.
    pub policy_name: String,
    /// Compliance framework (e.g., "SOC2", "HIPAA").
    pub framework: String,
    /// Whether the policy is compliant.
    pub compliant: bool,
    /// Compliance findings.
    pub findings: Vec<ComplianceFinding>,
}

/// A single compliance finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceFinding {
    /// Finding severity.
    pub severity: FindingSeverity,
    /// Rule that was checked.
    pub rule: String,
    /// Description of the finding.
    pub message: String,
}

/// Severity of a compliance finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingSeverity {
    /// Informational.
    Info,
    /// Warning (should fix).
    Warning,
    /// Error (must fix for compliance).
    Error,
}

/// Check a resolved policy against SOC2 requirements.
pub fn check_soc2_compliance(policy: &ResolvedPolicy) -> ComplianceReport {
    let mut findings = Vec::new();

    // SOC2 requires memory limits
    if policy.memory_limit_bytes > 4 * 1024 * 1024 * 1024 {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Warning,
            rule: "CC6.1".to_string(),
            message: "Memory limit exceeds 4GB; consider restricting for multi-tenant isolation".to_string(),
        });
    }

    // SOC2 requires execution timeouts
    if policy.timeout.is_none() {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Error,
            rule: "CC7.2".to_string(),
            message: "No execution timeout set; required for denial-of-service protection".to_string(),
        });
    }

    // SOC2 requires network restrictions
    if !policy.network_deny_all && policy.allow_http_hosts.iter().any(|h| h == "*") {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Error,
            rule: "CC6.6".to_string(),
            message: "Unrestricted outbound HTTP access; must use allowlist".to_string(),
        });
    }

    // SOC2 audit logging requirement
    if policy.fuel.is_none() {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Warning,
            rule: "CC7.3".to_string(),
            message: "No fuel limit set; resource usage cannot be bounded".to_string(),
        });
    }

    let compliant = !findings.iter().any(|f| f.severity == FindingSeverity::Error);

    ComplianceReport {
        policy_name: policy.name.clone(),
        framework: "SOC2".to_string(),
        compliant,
        findings,
    }
}

/// Check a resolved policy against HIPAA security requirements.
pub fn check_hipaa_compliance(policy: &ResolvedPolicy) -> ComplianceReport {
    let mut findings = Vec::new();

    // HIPAA §164.312(a)(1) - Access control: enforce least privilege
    if !policy.network_deny_all && policy.allow_http_hosts.iter().any(|h| h == "*") {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Error,
            rule: "164.312(a)(1)".to_string(),
            message: "Unrestricted outbound network violates minimum necessary access".to_string(),
        });
    }

    // HIPAA §164.312(a)(2)(iv) - Encryption and decryption
    if policy.allow_stdout && !policy.network_deny_all {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Warning,
            rule: "164.312(a)(2)(iv)".to_string(),
            message: "Stdout with network access may leak PHI; consider output filtering".to_string(),
        });
    }

    // HIPAA §164.312(b) - Audit controls
    if policy.timeout.is_none() {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Error,
            rule: "164.312(b)".to_string(),
            message: "No execution timeout; required for audit trail completeness".to_string(),
        });
    }

    // HIPAA §164.312(c)(1) - Integrity: resource limits required
    if policy.fuel.is_none() && policy.max_io_bytes.is_none() {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Warning,
            rule: "164.312(c)(1)".to_string(),
            message: "No fuel or I/O limit; data integrity cannot be bounded".to_string(),
        });
    }

    // HIPAA §164.312(e)(1) - Transmission security
    if !policy.fs_write_paths.is_empty() && policy.fs_write_paths.iter().any(|p| p == "/" || p == "/tmp") {
        findings.push(ComplianceFinding {
            severity: FindingSeverity::Warning,
            rule: "164.312(e)(1)".to_string(),
            message: "Broad filesystem write access; restrict to dedicated data paths".to_string(),
        });
    }

    let compliant = !findings.iter().any(|f| f.severity == FindingSeverity::Error);

    ComplianceReport {
        policy_name: policy.name.clone(),
        framework: "HIPAA".to_string(),
        compliant,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_lang::PolicyParser;

    #[test]
    fn test_parse_byte_sizes() {
        assert_eq!(parse_byte_size("128MB"), Some(128 * 1024 * 1024));
        assert_eq!(parse_byte_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_byte_size("4096KB"), Some(4096 * 1024));
        assert_eq!(parse_byte_size("1024B"), Some(1024));
        assert_eq!(parse_byte_size("1024"), Some(1024));
        assert_eq!(parse_byte_size("invalid"), None);
    }

    #[test]
    fn test_parse_durations() {
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("60"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_resolve_defaults() {
        let input = r#"sandbox "empty" {}"#;
        let doc = PolicyParser::parse(input).unwrap();
        let evaluator = PolicyEvaluator::new();
        let resolved = evaluator.resolve(&doc.policies[0]).unwrap();

        assert_eq!(resolved.memory_limit_bytes, 64 * 1024 * 1024);
        assert!(!resolved.allow_stdout);
        assert!(resolved.network_deny_all);
    }

    #[test]
    fn test_resolve_with_custom_defaults() {
        let defaults = ResolvedPolicy {
            allow_stdout: true,
            allow_stderr: true,
            ..Default::default()
        };
        let evaluator = PolicyEvaluator::new().with_defaults(defaults);
        let input = r#"sandbox "x" {}"#;
        let doc = PolicyParser::parse(input).unwrap();
        let resolved = evaluator.resolve(&doc.policies[0]).unwrap();

        assert!(resolved.allow_stdout);
        assert!(resolved.allow_stderr);
    }

    #[test]
    fn test_resolve_full_policy() {
        let input = r#"
            sandbox "full" {
                resource {
                    memory_limit = "256MB"
                    fuel = 2000000
                    timeout = "60s"
                }
                capability {
                    allow_stdout = true
                    fs_read = ["/data"]
                }
                network {
                    allow_dns = true
                    deny_all = false
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let evaluator = PolicyEvaluator::new();
        let resolved = evaluator.resolve(&doc.policies[0]).unwrap();

        assert_eq!(resolved.memory_limit_bytes, 256 * 1024 * 1024);
        assert_eq!(resolved.fuel, Some(2_000_000));
        assert_eq!(resolved.timeout, Some(Duration::from_secs(60)));
        assert!(resolved.allow_stdout);
        assert!(!resolved.allow_stderr); // default
        assert_eq!(resolved.fs_read_paths, vec!["/data"]);
        assert!(resolved.allow_dns);
        assert!(!resolved.network_deny_all);
    }

    #[test]
    fn test_resolve_invalid_memory() {
        let input = r#"
            sandbox "bad" {
                resource {
                    memory_limit = "notanumber"
                }
            }
        "#;
        let doc = PolicyParser::parse(input).unwrap();
        let evaluator = PolicyEvaluator::new();
        let result = evaluator.resolve(&doc.policies[0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_policy_composer_most_restrictive() {
        let p1 = ResolvedPolicy {
            name: "a".to_string(),
            memory_limit_bytes: 256 * 1024 * 1024,
            allow_stdout: true,
            allow_stderr: true,
            ..Default::default()
        };
        let p2 = ResolvedPolicy {
            name: "b".to_string(),
            memory_limit_bytes: 128 * 1024 * 1024,
            allow_stdout: true,
            allow_stderr: false,
            ..Default::default()
        };

        let composer = PolicyComposer::new(ConflictStrategy::MostRestrictive);
        let result = composer.compose(&[p1, p2]);
        assert_eq!(result.memory_limit_bytes, 128 * 1024 * 1024);
        assert!(result.allow_stdout);
        assert!(!result.allow_stderr);
    }

    #[test]
    fn test_policy_composer_last_wins() {
        let p1 = ResolvedPolicy {
            name: "a".to_string(),
            memory_limit_bytes: 256 * 1024 * 1024,
            ..Default::default()
        };
        let p2 = ResolvedPolicy {
            name: "b".to_string(),
            memory_limit_bytes: 512 * 1024 * 1024,
            ..Default::default()
        };

        let composer = PolicyComposer::new(ConflictStrategy::LastWins);
        let result = composer.compose(&[p1, p2]);
        assert_eq!(result.name, "b");
        assert_eq!(result.memory_limit_bytes, 512 * 1024 * 1024);
    }

    #[test]
    fn test_policy_inheritance() {
        let parent = ResolvedPolicy {
            name: "parent".to_string(),
            memory_limit_bytes: 256 * 1024 * 1024,
            allow_stdout: true,
            allow_stderr: true,
            fuel: Some(1_000_000),
            ..Default::default()
        };
        let child = ResolvedPolicy {
            name: "child".to_string(),
            fuel: Some(500_000),
            allow_stdin: true,
            ..Default::default()
        };

        let result = PolicyComposer::inherit(&parent, &child);
        assert_eq!(result.name, "child");
        assert_eq!(result.memory_limit_bytes, 256 * 1024 * 1024); // from parent
        assert!(result.allow_stdout); // from parent
        assert_eq!(result.fuel, Some(500_000)); // overridden by child
        assert!(result.allow_stdin); // from child
    }

    #[test]
    fn test_soc2_compliance_pass() {
        let policy = ResolvedPolicy {
            name: "secure".to_string(),
            memory_limit_bytes: 128 * 1024 * 1024,
            fuel: Some(1_000_000),
            timeout: Some(Duration::from_secs(30)),
            network_deny_all: true,
            ..Default::default()
        };
        let report = check_soc2_compliance(&policy);
        assert!(report.compliant);
        assert!(report.findings.iter().all(|f| f.severity != FindingSeverity::Error));
    }

    #[test]
    fn test_soc2_compliance_fail() {
        let policy = ResolvedPolicy {
            name: "insecure".to_string(),
            timeout: None,
            network_deny_all: false,
            allow_http_hosts: vec!["*".to_string()],
            ..Default::default()
        };
        let report = check_soc2_compliance(&policy);
        assert!(!report.compliant);
        assert!(report.findings.iter().any(|f| f.severity == FindingSeverity::Error));
    }

    #[test]
    fn test_hipaa_compliance_pass() {
        let policy = ResolvedPolicy {
            name: "hipaa-safe".to_string(),
            memory_limit_bytes: 128 * 1024 * 1024,
            fuel: Some(1_000_000),
            timeout: Some(Duration::from_secs(30)),
            network_deny_all: true,
            ..Default::default()
        };
        let report = check_hipaa_compliance(&policy);
        assert!(report.compliant);
        assert_eq!(report.framework, "HIPAA");
    }

    #[test]
    fn test_hipaa_compliance_fail() {
        let policy = ResolvedPolicy {
            name: "hipaa-bad".to_string(),
            timeout: None,
            network_deny_all: false,
            allow_http_hosts: vec!["*".to_string()],
            ..Default::default()
        };
        let report = check_hipaa_compliance(&policy);
        assert!(!report.compliant);
        assert!(report.findings.iter().any(|f| f.rule.contains("164.312")));
    }
}
