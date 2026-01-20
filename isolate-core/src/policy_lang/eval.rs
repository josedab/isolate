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
    let (num_str, multiplier) = if s.ends_with("ms") {
        (&s[..s.len() - 2], 1u64)
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1000u64)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60_000u64)
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3_600_000u64)
    } else {
        // Default: seconds
        return s.parse::<u64>().ok().map(Duration::from_secs);
    };
    let num: u64 = num_str.trim().parse().ok()?;
    Some(Duration::from_millis(num * multiplier))
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
}
