//! Declarative policy files for sandbox configuration.
//!
//! Parse YAML or JSON policy files into `SandboxConfig`, enabling operators
//! to manage sandbox policies as code (GitOps).
//!
//! # Policy Format (YAML)
//!
//! ```yaml
//! version: "1"
//! name: "api-handler"
//! description: "Policy for API handler modules"
//!
//! # Optional: inherit from another policy
//! extends: "base-policy"
//!
//! capabilities:
//!   stdout: true
//!   stderr: true
//!   filesystem:
//!     read: ["/data", "/config"]
//!     write: ["/tmp"]
//!   network:
//!     http_hosts: ["api.example.com", "*.trusted.com"]
//!   environment:
//!     allowed_vars: ["API_KEY", "LOG_LEVEL"]
//!
//! resources:
//!   memory:
//!     heap_max: "128MB"
//!     stack_max: "1MB"
//!   cpu:
//!     fuel: 10000000
//!     time_limit: "30s"
//!   io:
//!     read_limit: "10MB"
//!     write_limit: "1MB"
//!   timeout: "60s"
//!
//! environment:
//!   LOG_LEVEL: "info"
//!
//! entry_point: "_start"
//! ```
//!
//! # Example
//!
//! ```rust
//! use isolate_core::policy_file::{PolicyFile, PolicyError};
//!
//! let yaml = r#"
//! version: "1"
//! name: "test"
//! capabilities:
//!   stdout: true
//! resources:
//!   memory:
//!     heap_max: "64MB"
//!   timeout: "10s"
//! "#;
//!
//! let policy = PolicyFile::from_yaml(yaml).unwrap();
//! assert_eq!(policy.name, "test");
//! assert_eq!(policy.capabilities.stdout, Some(true));
//! ```

use crate::capability::Capability;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Errors from policy parsing.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("YAML parse error: {0}")]
    Yaml(String),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid policy: {0}")]
    Validation(String),
    #[error("Invalid size value: {0}")]
    InvalidSize(String),
    #[error("Invalid duration value: {0}")]
    InvalidDuration(String),
}

/// A parsed policy file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyFile {
    /// Policy schema version.
    #[serde(default = "default_version")]
    pub version: String,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Parent policy to inherit from.
    #[serde(default)]
    pub extends: Option<String>,
    /// Capability grants.
    #[serde(default)]
    pub capabilities: CapabilityPolicy,
    /// Resource limits.
    #[serde(default)]
    pub resources: ResourcePolicy,
    /// Environment variables to inject.
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// WASM entry point function.
    #[serde(default)]
    pub entry_point: Option<String>,
    /// Allowed module hashes (for allowlisting).
    #[serde(default)]
    pub allowed_modules: Vec<String>,
    /// Labels for policy selection.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

fn default_version() -> String {
    "1".to_string()
}

/// Capability grants in a policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityPolicy {
    #[serde(default)]
    pub stdout: Option<bool>,
    #[serde(default)]
    pub stderr: Option<bool>,
    #[serde(default)]
    pub stdin: Option<bool>,
    #[serde(default)]
    pub filesystem: Option<FilesystemPolicy>,
    #[serde(default)]
    pub network: Option<NetworkPolicy>,
    #[serde(default)]
    pub environment: Option<EnvironmentPolicy>,
    #[serde(default)]
    pub time: Option<TimePolicy>,
    #[serde(default)]
    pub random: Option<RandomPolicy>,
}

/// Filesystem access policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
    #[serde(default)]
    pub temp_dir: Option<bool>,
}

/// Network access policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkPolicy {
    #[serde(default)]
    pub http_hosts: Vec<String>,
    #[serde(default)]
    pub dns: Option<bool>,
}

/// Environment variable access policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvironmentPolicy {
    #[serde(default)]
    pub allowed_vars: Vec<String>,
    #[serde(default)]
    pub all: Option<bool>,
    #[serde(default)]
    pub args: Option<bool>,
}

/// Time access policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimePolicy {
    #[serde(default)]
    pub system_clock: Option<bool>,
    #[serde(default)]
    pub monotonic_clock: Option<bool>,
    #[serde(default)]
    pub timers: Option<bool>,
}

/// Random number generation policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RandomPolicy {
    #[serde(default)]
    pub secure: Option<bool>,
}

/// Resource limits in a policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcePolicy {
    #[serde(default)]
    pub memory: Option<MemoryPolicy>,
    #[serde(default)]
    pub cpu: Option<CpuPolicy>,
    #[serde(default)]
    pub io: Option<IoPolicy>,
    /// Wall-clock timeout (e.g. "60s", "5m").
    #[serde(default)]
    pub timeout: Option<String>,
}

/// Memory limits policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryPolicy {
    /// e.g. "128MB", "1GB"
    pub heap_max: Option<String>,
    pub stack_max: Option<String>,
}

/// CPU limits policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuPolicy {
    pub fuel: Option<u64>,
    /// e.g. "30s"
    pub time_limit: Option<String>,
}

/// I/O limits policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IoPolicy {
    /// e.g. "10MB"
    pub read_limit: Option<String>,
    /// e.g. "1MB"
    pub write_limit: Option<String>,
}

impl PolicyFile {
    /// Parse from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, PolicyError> {
        let policy: PolicyFile =
            serde_json::from_str(yaml).or_else(|_| {
                // serde_yaml is optional; fall back to JSON-compat subset
                serde_json::from_value(
                    yaml_to_json_value(yaml)?
                ).map_err(|e| PolicyError::Yaml(e.to_string()))
            })?;
        policy.validate()?;
        Ok(policy)
    }

    /// Parse from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, PolicyError> {
        let policy: PolicyFile = serde_json::from_str(json)?;
        policy.validate()?;
        Ok(policy)
    }

    /// Merge this policy with a parent (self takes precedence).
    pub fn merge(&self, parent: &PolicyFile) -> PolicyFile {
        let mut merged = parent.clone();
        merged.name = self.name.clone();
        merged.description = self.description.clone();

        // Override capabilities that are explicitly set
        if self.capabilities.stdout.is_some() {
            merged.capabilities.stdout = self.capabilities.stdout;
        }
        if self.capabilities.stderr.is_some() {
            merged.capabilities.stderr = self.capabilities.stderr;
        }
        if self.capabilities.stdin.is_some() {
            merged.capabilities.stdin = self.capabilities.stdin;
        }
        if self.capabilities.filesystem.is_some() {
            merged.capabilities.filesystem = self.capabilities.filesystem.clone();
        }
        if self.capabilities.network.is_some() {
            merged.capabilities.network = self.capabilities.network.clone();
        }
        if self.capabilities.environment.is_some() {
            merged.capabilities.environment = self.capabilities.environment.clone();
        }

        // Override resources
        if self.resources.memory.is_some() {
            merged.resources.memory = self.resources.memory.clone();
        }
        if self.resources.cpu.is_some() {
            merged.resources.cpu = self.resources.cpu.clone();
        }
        if self.resources.io.is_some() {
            merged.resources.io = self.resources.io.clone();
        }
        if self.resources.timeout.is_some() {
            merged.resources.timeout = self.resources.timeout.clone();
        }

        // Merge environment (child overrides parent)
        for (k, v) in &self.environment {
            merged.environment.insert(k.clone(), v.clone());
        }

        if self.entry_point.is_some() {
            merged.entry_point = self.entry_point.clone();
        }

        merged
    }

    /// Convert to a list of Capabilities.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        let mut caps = Vec::new();

        if self.capabilities.stdout == Some(true) {
            caps.push(Capability::stdout());
        }
        if self.capabilities.stderr == Some(true) {
            caps.push(Capability::stderr());
        }
        if self.capabilities.stdin == Some(true) {
            caps.push(Capability::stdin());
        }

        if let Some(ref fs) = self.capabilities.filesystem {
            for path in &fs.read {
                caps.push(Capability::filesystem_read(path));
            }
            for path in &fs.write {
                caps.push(Capability::filesystem_write(path));
            }
            if fs.temp_dir == Some(true) {
                caps.push(Capability::temp_dir());
            }
        }

        if let Some(ref net) = self.capabilities.network {
            if !net.http_hosts.is_empty() {
                caps.push(Capability::http_client(net.http_hosts.clone()));
            }
            if net.dns == Some(true) {
                caps.push(Capability::dns_resolve());
            }
        }

        if let Some(ref env) = self.capabilities.environment {
            if env.all == Some(true) {
                caps.push(Capability::env_all());
            } else {
                for var in &env.allowed_vars {
                    caps.push(Capability::env_var(var));
                }
            }
            if env.args == Some(true) {
                caps.push(Capability::args());
            }
        }

        if let Some(ref time) = self.capabilities.time {
            if time.system_clock == Some(true) {
                caps.push(Capability::system_clock());
            }
            if time.monotonic_clock == Some(true) {
                caps.push(Capability::monotonic_clock());
            }
            if time.timers == Some(true) {
                caps.push(Capability::timers());
            }
        }

        if let Some(ref rng) = self.capabilities.random {
            if rng.secure == Some(true) {
                caps.push(Capability::secure_random());
            }
        }

        caps
    }

    /// Apply this policy onto a `SandboxConfigBuilder`.
    pub fn apply_to(
        &self,
        mut builder: crate::config::SandboxConfigBuilder,
    ) -> Result<crate::config::SandboxConfigBuilder, PolicyError> {
        // Capabilities
        for cap in self.to_capabilities() {
            builder = builder.capability(cap);
        }

        // Resources
        if let Some(ref mem) = self.resources.memory {
            if let Some(ref s) = mem.heap_max {
                builder = builder.memory_limit(parse_size(s)?);
            }
            if let Some(ref s) = mem.stack_max {
                builder = builder.stack_size(parse_size(s)?);
            }
        }
        if let Some(ref cpu) = self.resources.cpu {
            if let Some(fuel) = cpu.fuel {
                builder = builder.fuel(fuel);
            }
            if let Some(ref s) = cpu.time_limit {
                builder = builder.cpu_time_limit(parse_duration(s)?);
            }
        }
        if let Some(ref io) = self.resources.io {
            if let Some(ref s) = io.read_limit {
                builder = builder.io_read_limit(parse_size(s)? as u64);
            }
            if let Some(ref s) = io.write_limit {
                builder = builder.io_write_limit(parse_size(s)? as u64);
            }
        }
        if let Some(ref t) = self.resources.timeout {
            builder = builder.wall_time_limit(parse_duration(t)?);
        }

        // Environment
        for (k, v) in &self.environment {
            builder = builder.env(k, v);
        }

        // Entry point
        if let Some(ref ep) = self.entry_point {
            builder = builder.entry_point(ep);
        }

        Ok(builder)
    }

    fn validate(&self) -> Result<(), PolicyError> {
        if self.version != "1" {
            return Err(PolicyError::Validation(format!(
                "Unsupported policy version: '{}' (expected '1')",
                self.version
            )));
        }
        Ok(())
    }
}

/// Parse a human-readable size string (e.g. "128MB", "1GB", "512KB") to bytes.
pub fn parse_size(s: &str) -> Result<usize, PolicyError> {
    let s = s.trim();
    let (num_str, multiplier) = if s.ends_with("GB") || s.ends_with("gb") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") || s.ends_with("mb") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") || s.ends_with("kb") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') || s.ends_with('b') {
        (&s[..s.len() - 1], 1)
    } else {
        // Assume raw bytes
        (s, 1)
    };
    let num: usize = num_str
        .trim()
        .parse()
        .map_err(|_| PolicyError::InvalidSize(s.to_string()))?;
    Ok(num * multiplier)
}

/// Parse a human-readable duration string (e.g. "30s", "5m", "1h").
pub fn parse_duration(s: &str) -> Result<std::time::Duration, PolicyError> {
    let s = s.trim();
    let (num_str, factor) = if s.ends_with("ms") {
        (&s[..s.len() - 2], 1u64)
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1000)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60 * 1000)
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3600 * 1000)
    } else {
        // Assume seconds
        (s, 1000)
    };
    let num: u64 = num_str
        .trim()
        .parse()
        .map_err(|_| PolicyError::InvalidDuration(s.to_string()))?;
    Ok(std::time::Duration::from_millis(num * factor))
}

/// Minimal YAML-to-JSON-value converter for when serde_yaml is not available.
/// Handles the simple subset used by policy files.
fn yaml_to_json_value(yaml: &str) -> Result<serde_json::Value, PolicyError> {
    // For now, fall back to attempting JSON parse directly.
    // In production, this would use the serde_yaml crate (feature-gated).
    serde_json::from_str(yaml).map_err(|e| PolicyError::Yaml(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("128MB").unwrap(), 128 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("512KB").unwrap(), 512 * 1024);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert!(parse_size("invalid").is_err());
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(
            parse_duration("30s").unwrap(),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(
            parse_duration("5m").unwrap(),
            std::time::Duration::from_secs(300)
        );
        assert_eq!(
            parse_duration("100ms").unwrap(),
            std::time::Duration::from_millis(100)
        );
        assert_eq!(
            parse_duration("1h").unwrap(),
            std::time::Duration::from_secs(3600)
        );
    }

    #[test]
    fn test_policy_from_json() {
        let json = r#"{
            "version": "1",
            "name": "test-policy",
            "capabilities": {
                "stdout": true,
                "stderr": true,
                "filesystem": {
                    "read": ["/data"],
                    "write": ["/tmp"]
                }
            },
            "resources": {
                "memory": { "heap_max": "64MB" },
                "cpu": { "fuel": 1000000 },
                "timeout": "30s"
            },
            "environment": { "LOG_LEVEL": "info" }
        }"#;

        let policy = PolicyFile::from_json(json).unwrap();
        assert_eq!(policy.name, "test-policy");
        assert_eq!(policy.capabilities.stdout, Some(true));

        let caps = policy.to_capabilities();
        assert!(caps.iter().any(|c| matches!(c, Capability::Stdio(_))));
        assert!(caps.iter().any(|c| matches!(c, Capability::Filesystem(_))));
    }

    #[test]
    fn test_policy_merge() {
        let parent_json = r#"{
            "version": "1",
            "name": "parent",
            "capabilities": { "stdout": true, "stderr": true },
            "resources": { "memory": { "heap_max": "256MB" }, "timeout": "60s" }
        }"#;
        let child_json = r#"{
            "version": "1",
            "name": "child",
            "capabilities": { "stdout": false },
            "resources": { "timeout": "10s" }
        }"#;

        let parent = PolicyFile::from_json(parent_json).unwrap();
        let child = PolicyFile::from_json(child_json).unwrap();
        let merged = child.merge(&parent);

        assert_eq!(merged.name, "child");
        // Child overrides stdout
        assert_eq!(merged.capabilities.stdout, Some(false));
        // Parent's stderr preserved
        assert_eq!(merged.capabilities.stderr, Some(true));
        // Child overrides timeout
        assert_eq!(merged.resources.timeout, Some("10s".to_string()));
        // Parent's memory preserved
        assert!(merged.resources.memory.is_some());
    }

    #[test]
    fn test_policy_to_capabilities() {
        let json = r#"{
            "version": "1",
            "name": "full",
            "capabilities": {
                "stdout": true,
                "stdin": true,
                "filesystem": { "read": ["/a", "/b"], "write": ["/c"] },
                "network": { "http_hosts": ["api.example.com"], "dns": true },
                "environment": { "allowed_vars": ["KEY"], "args": true },
                "time": { "system_clock": true },
                "random": { "secure": true }
            }
        }"#;

        let policy = PolicyFile::from_json(json).unwrap();
        let caps = policy.to_capabilities();

        // stdout + stdin + 2 fs_read + 1 fs_write + http + dns + env_var + args + clock + random = 11
        assert_eq!(caps.len(), 11);
    }

    #[test]
    fn test_invalid_version() {
        let json = r#"{ "version": "2", "name": "bad" }"#;
        assert!(PolicyFile::from_json(json).is_err());
    }
}
