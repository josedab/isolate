//! Migration utilities for transitioning from WASI Preview1 to Preview2.
//!
//! This module provides adapters that allow existing [`SandboxConfig`] configurations
//! to work transparently with the Preview2 component model, enabling a gradual
//! migration path without breaking existing code.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::wasi2::migration::{MigrationAdapter, MigrationReport};
//! use isolate_core::SandboxConfig;
//!
//! # fn example() -> isolate_core::Result<()> {
//! let wasm = std::fs::read("module.wasm")?;
//! let preview1_config = SandboxConfig::builder()
//!     .module(&wasm)?
//!     .memory_limit(128 * 1024 * 1024)
//!     .build()?;
//!
//! let adapter = MigrationAdapter::new();
//! let report = adapter.analyze(&preview1_config);
//! if report.is_compatible {
//!     let preview2_config = adapter.convert(&preview1_config)?;
//!     // Use preview2_config with ComponentSandbox
//! }
//! # Ok(())
//! # }
//! ```

use crate::capability::{Capability, CapabilitySet};
use crate::config::SandboxConfig;
use crate::error::{Error, Result};

use super::context::{ComponentConfig, ComponentConfigBuilder};

use std::time::Duration;

/// Adapter for migrating Preview1 sandbox configurations to Preview2 component configs.
#[derive(Debug, Clone)]
pub struct MigrationAdapter {
    /// Whether to automatically map stdio capabilities.
    pub auto_map_stdio: bool,
    /// Whether to map filesystem capabilities to preopened dirs.
    pub auto_map_filesystem: bool,
    /// Whether to map network capabilities.
    pub auto_map_network: bool,
    /// Default network timeout for migrated configs.
    pub default_network_timeout: Option<Duration>,
}

impl Default for MigrationAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationAdapter {
    /// Create a new migration adapter with default settings.
    pub fn new() -> Self {
        Self {
            auto_map_stdio: true,
            auto_map_filesystem: true,
            auto_map_network: true,
            default_network_timeout: Some(Duration::from_secs(30)),
        }
    }

    /// Analyze a Preview1 config and produce a migration compatibility report.
    pub fn analyze(&self, config: &SandboxConfig) -> MigrationReport {
        let mut warnings = Vec::new();
        let mut mapped_capabilities = Vec::new();
        let mut unmapped_capabilities = Vec::new();

        for cap in config.capabilities.iter() {
            match cap {
                Capability::Stdio(_) => {
                    if self.auto_map_stdio {
                        mapped_capabilities.push(CapabilityMapping {
                            preview1: cap.description(),
                            preview2: format!("wasi:cli/{}", cap.description()),
                            automatic: true,
                        });
                    }
                }
                Capability::Filesystem(fs_cap) => {
                    if self.auto_map_filesystem {
                        mapped_capabilities.push(CapabilityMapping {
                            preview1: cap.description(),
                            preview2: format!("wasi:filesystem/{}", fs_cap.description()),
                            automatic: true,
                        });
                    }
                }
                Capability::Network(net_cap) => {
                    if self.auto_map_network {
                        mapped_capabilities.push(CapabilityMapping {
                            preview1: cap.description(),
                            preview2: format!("wasi:sockets/{}", net_cap.description()),
                            automatic: true,
                        });
                    }
                }
                Capability::Time(_) => {
                    mapped_capabilities.push(CapabilityMapping {
                        preview1: cap.description(),
                        preview2: "wasi:clocks/*".to_string(),
                        automatic: true,
                    });
                }
                Capability::Random(_) => {
                    mapped_capabilities.push(CapabilityMapping {
                        preview1: cap.description(),
                        preview2: "wasi:random/*".to_string(),
                        automatic: true,
                    });
                }
                Capability::Environment(_) => {
                    mapped_capabilities.push(CapabilityMapping {
                        preview1: cap.description(),
                        preview2: "wasi:cli/environment".to_string(),
                        automatic: true,
                    });
                }
                Capability::HostFunction(hf) => {
                    unmapped_capabilities.push(cap.description());
                    warnings.push(format!(
                        "Host function capability '{}' has no Preview2 equivalent; \
                         use component imports instead",
                        hf.description()
                    ));
                }
            }
        }

        // Check for snapshot config incompatibilities
        if config.snapshot.enabled {
            warnings.push(
                "Snapshot/restore is not yet supported in Preview2 mode; \
                 snapshots will be disabled"
                    .to_string(),
            );
        }

        let is_compatible = unmapped_capabilities.is_empty();

        MigrationReport {
            is_compatible,
            warnings,
            mapped_capabilities,
            unmapped_capabilities,
            resource_limits_preserved: true,
            env_vars_preserved: true,
            args_preserved: true,
        }
    }

    /// Convert a Preview1 [`SandboxConfig`] to a Preview2 [`ComponentConfig`].
    ///
    /// The WASM module bytes from the SandboxConfig are reused directly — the
    /// component runtime will handle module-vs-component detection at instantiation.
    pub fn convert(&self, config: &SandboxConfig) -> Result<ComponentConfig> {
        let mut builder = ComponentConfig::builder();

        // Transfer the module bytes as a component
        builder = builder.component(config.module.bytes()).map_err(|e| {
            Error::InvalidConfig(format!("Failed to convert module to component: {}", e))
        })?;

        // Transfer resource limits
        builder = builder.memory_limit(config.resources.memory.heap_max);

        if let Some(fuel) = config.resources.cpu.fuel {
            builder = builder.fuel(fuel);
        }

        if let Some(cpu_time) = config.resources.time.cpu_time {
            builder = builder.cpu_time_limit(cpu_time);
        }

        if let Some(wall_time) = config.resources.time.wall_time {
            builder = builder.wall_time_limit(wall_time);
        }

        // Transfer capabilities
        builder = self.map_capabilities(builder, &config.capabilities);

        // Transfer environment variables
        for (key, value) in &config.env {
            builder = builder.env(key, value);
        }

        // Transfer arguments
        for arg in &config.args {
            builder = builder.arg(arg);
        }

        builder.build()
    }

    /// Map Preview1 capabilities to Preview2 builder calls.
    fn map_capabilities(
        &self,
        mut builder: ComponentConfigBuilder,
        capabilities: &CapabilitySet,
    ) -> ComponentConfigBuilder {
        let mut http_hosts: Vec<String> = Vec::new();
        let mut has_tcp = false;
        let mut has_dns = false;

        for cap in capabilities.iter() {
            match cap {
                Capability::Stdio(stdio) => {
                    use crate::capability::StdioCapability;
                    match stdio {
                        StdioCapability::Stdout => builder = builder.allow_stdout(),
                        StdioCapability::Stderr => builder = builder.allow_stderr(),
                        StdioCapability::Stdin => builder = builder.allow_stdin(),
                    }
                }
                Capability::Filesystem(fs) => {
                    use crate::capability::FilesystemCapability;
                    match fs {
                        FilesystemCapability::ReadOnly(path) => {
                            builder = builder.filesystem_read(path);
                        }
                        FilesystemCapability::ReadWrite(path) => {
                            builder = builder.filesystem_write(path);
                        }
                        FilesystemCapability::TempDir => {
                            builder = builder.filesystem_write("/tmp");
                        }
                    }
                }
                Capability::Network(net) => {
                    use crate::capability::NetworkCapability;
                    match net {
                        NetworkCapability::HttpClient(hosts) => {
                            http_hosts.extend(hosts.clone());
                        }
                        NetworkCapability::TcpConnect(_) | NetworkCapability::TcpListen(_) => {
                            has_tcp = true;
                        }
                        NetworkCapability::DnsResolve => {
                            has_dns = true;
                        }
                    }
                }
                // Time, Random, Environment, HostFunction pass through as generic capabilities
                other => {
                    builder = builder.capability(other.clone());
                }
            }
        }

        // Apply aggregated network settings
        if !http_hosts.is_empty() {
            builder = builder.allow_http_hosts(http_hosts);
        }
        if has_tcp {
            builder = builder.allow_tcp();
        }
        if has_dns {
            builder = builder.allow_dns();
        }
        if let Some(timeout) = self.default_network_timeout {
            let has_network_cap = capabilities.iter().any(|c| matches!(c, Capability::Network(_)));
            if has_tcp || has_dns || has_network_cap {
                builder = builder.network_timeout(timeout);
            }
        }

        builder
    }
}

/// Report from analyzing a Preview1 config for Preview2 migration.
#[derive(Debug, Clone)]
pub struct MigrationReport {
    /// Whether the config can be fully migrated without data loss.
    pub is_compatible: bool,
    /// Warnings about migration issues.
    pub warnings: Vec<String>,
    /// Capabilities that were successfully mapped.
    pub mapped_capabilities: Vec<CapabilityMapping>,
    /// Capabilities that could not be mapped.
    pub unmapped_capabilities: Vec<String>,
    /// Whether resource limits are fully preserved.
    pub resource_limits_preserved: bool,
    /// Whether environment variables are preserved.
    pub env_vars_preserved: bool,
    /// Whether arguments are preserved.
    pub args_preserved: bool,
}

impl MigrationReport {
    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!(
            "Migration compatibility: {}",
            if self.is_compatible { "FULL" } else { "PARTIAL" }
        ));
        lines.push(format!(
            "Mapped capabilities: {}/{}",
            self.mapped_capabilities.len(),
            self.mapped_capabilities.len() + self.unmapped_capabilities.len()
        ));
        if !self.warnings.is_empty() {
            lines.push(format!("Warnings: {}", self.warnings.len()));
            for w in &self.warnings {
                lines.push(format!("  - {}", w));
            }
        }
        lines.join("\n")
    }
}

/// Describes how a Preview1 capability maps to a Preview2 interface.
#[derive(Debug, Clone)]
pub struct CapabilityMapping {
    /// Preview1 capability description.
    pub preview1: String,
    /// Preview2 interface equivalent.
    pub preview2: String,
    /// Whether this mapping is automatic.
    pub automatic: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use std::time::Duration;

    const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    fn make_preview1_config() -> SandboxConfig {
        SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .memory_limit(128 * 1024 * 1024)
            .fuel(1_000_000)
            .wall_time_limit(Duration::from_secs(30))
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .capability(Capability::filesystem_read("/data"))
            .env("API_KEY", "secret")
            .arg("--verbose".to_string())
            .build()
            .unwrap()
    }

    #[test]
    fn test_migration_adapter_default() {
        let adapter = MigrationAdapter::new();
        assert!(adapter.auto_map_stdio);
        assert!(adapter.auto_map_filesystem);
        assert!(adapter.auto_map_network);
    }

    #[test]
    fn test_analyze_compatible_config() {
        let adapter = MigrationAdapter::new();
        let config = make_preview1_config();
        let report = adapter.analyze(&config);

        assert!(report.is_compatible);
        assert!(report.warnings.is_empty());
        assert_eq!(report.mapped_capabilities.len(), 3); // stdout, stderr, fs_read
        assert!(report.unmapped_capabilities.is_empty());
        assert!(report.resource_limits_preserved);
    }

    #[test]
    fn test_analyze_with_host_function() {
        let adapter = MigrationAdapter::new();
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::host_function("custom_fn"))
            .build()
            .unwrap();

        let report = adapter.analyze(&config);
        assert!(!report.is_compatible);
        assert_eq!(report.unmapped_capabilities.len(), 1);
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn test_analyze_with_snapshots_warning() {
        let adapter = MigrationAdapter::new();
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .enable_snapshots(None)
            .build()
            .unwrap();

        let report = adapter.analyze(&config);
        assert!(report.warnings.iter().any(|w| w.contains("Snapshot")));
    }

    #[test]
    fn test_convert_preserves_resources() {
        let adapter = MigrationAdapter::new();
        let p1_config = make_preview1_config();
        let p2_config = adapter.convert(&p1_config).unwrap();

        assert_eq!(p2_config.resources.memory.heap_max, 128 * 1024 * 1024);
        assert_eq!(p2_config.resources.cpu.fuel, Some(1_000_000));
        assert_eq!(p2_config.resources.time.wall_time, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_convert_preserves_capabilities() {
        let adapter = MigrationAdapter::new();
        let p1_config = make_preview1_config();
        let p2_config = adapter.convert(&p1_config).unwrap();

        assert!(p2_config.capabilities.has(&Capability::stdout()));
        assert!(p2_config.capabilities.has(&Capability::stderr()));
        assert!(p2_config.capabilities.has(&Capability::filesystem_read("/data")));
    }

    #[test]
    fn test_convert_preserves_env_and_args() {
        let adapter = MigrationAdapter::new();
        let p1_config = make_preview1_config();
        let p2_config = adapter.convert(&p1_config).unwrap();

        assert_eq!(p2_config.env.get("API_KEY"), Some(&"secret".to_string()));
        assert_eq!(p2_config.args, vec!["--verbose".to_string()]);
    }

    #[test]
    fn test_convert_maps_network_capabilities() {
        let adapter = MigrationAdapter::new();
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::http_client(vec!["api.example.com"]))
            .capability(Capability::dns_resolve())
            .build()
            .unwrap();

        let p2_config = adapter.convert(&config).unwrap();
        assert_eq!(p2_config.network.allowed_hosts, vec!["api.example.com"]);
        assert!(p2_config.network.allow_dns);
    }

    #[test]
    fn test_convert_maps_tcp_capabilities() {
        let adapter = MigrationAdapter::new();
        let addr = "127.0.0.1:8080".parse().unwrap();
        let config = SandboxConfig::builder()
            .module(MINIMAL_WASM)
            .unwrap()
            .capability(Capability::tcp_connect(vec![addr]))
            .build()
            .unwrap();

        let p2_config = adapter.convert(&config).unwrap();
        assert!(p2_config.network.allow_tcp);
    }

    #[test]
    fn test_migration_report_summary() {
        let adapter = MigrationAdapter::new();
        let config = make_preview1_config();
        let report = adapter.analyze(&config);
        let summary = report.summary();

        assert!(summary.contains("FULL"));
        assert!(summary.contains("3/3"));
    }
}
