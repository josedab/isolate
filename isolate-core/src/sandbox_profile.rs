//! Use-case-based sandbox profiles for workload-specific configuration.
//!
//! While [`LanguageProfile`](crate::profile::LanguageProfile) tunes resource limits
//! per compiled language, `SandboxProfile` configures security posture, capabilities,
//! and resource budgets based on the intended workload type.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::sandbox_profile::SandboxProfile;
//! use isolate_core::SandboxConfig;
//!
//! # fn example() -> isolate_core::Result<()> {
//! # let wasm_bytes = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
//! let config = SandboxConfig::builder()
//!     .module(wasm_bytes)?
//!     .use_profile(SandboxProfile::AiCodeExecution)
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use crate::capability::Capability;
use crate::resource::{CpuLimits, IoLimits, MemoryLimits, ResourceLimits, TimeLimits};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Security posture level for a sandbox profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityLevel {
    /// Tight restrictions, minimal capabilities. Suitable for untrusted code.
    Conservative,
    /// Balanced restrictions with selective capabilities.
    Moderate,
    /// Relaxed restrictions, broad capabilities. For trusted/development use.
    Permissive,
}

impl fmt::Display for SecurityLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conservative => write!(f, "Conservative"),
            Self::Moderate => write!(f, "Moderate"),
            Self::Permissive => write!(f, "Permissive"),
        }
    }
}

impl std::str::FromStr for SecurityLevel {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "conservative" => Ok(Self::Conservative),
            "moderate" => Ok(Self::Moderate),
            "permissive" => Ok(Self::Permissive),
            _ => Err(format!(
                "Unknown security level: '{}'. Available: conservative, moderate, permissive",
                s
            )),
        }
    }
}

/// A use-case-based sandbox profile with pre-configured security posture,
/// capabilities, and resource budgets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SandboxProfile {
    /// For running LLM-generated code: conservative limits, stdout+stderr only,
    /// no filesystem or network access.
    AiCodeExecution,
    /// For SaaS plugin execution: moderate limits, stdout+stderr+temp_dir,
    /// optional host functions.
    PluginRuntime,
    /// For CI/CD build/test steps: generous limits, full filesystem read,
    /// temp_dir, stdout+stderr, env access.
    CiRunner,
    /// For edge/serverless functions: tight limits, stdout+stderr, http_client,
    /// clock access.
    EdgeFunction,
    /// For interactive playgrounds: strict limits, stdout+stderr+stdin, clock,
    /// random.
    Playground,
    /// For development/testing: very generous limits, all stdio, filesystem,
    /// env, clock, random.
    Unrestricted,
}

impl SandboxProfile {
    /// Get the pre-configured resource limits for this profile.
    pub fn resource_limits(&self) -> ResourceLimits {
        match self {
            Self::AiCodeExecution => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 32 * 1024 * 1024,   // 32MB
                    stack_max: 512 * 1024,         // 512KB
                    total_max: 64 * 1024 * 1024,   // 64MB
                },
                cpu: CpuLimits {
                    fuel: Some(100_000),
                    cpu_time: Some(Duration::from_secs(10)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(1024 * 1024),       // 1MB
                    write_bytes: Some(1024 * 1024),      // 1MB
                    iops: Some(100),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(10)),
                    cpu_time: Some(Duration::from_secs(10)),
                },
            },
            Self::PluginRuntime => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 64 * 1024 * 1024,    // 64MB
                    stack_max: 1024 * 1024,         // 1MB
                    total_max: 128 * 1024 * 1024,   // 128MB
                },
                cpu: CpuLimits {
                    fuel: Some(1_000_000),
                    cpu_time: Some(Duration::from_secs(30)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(10 * 1024 * 1024),  // 10MB
                    write_bytes: Some(5 * 1024 * 1024),  // 5MB
                    iops: Some(500),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(30)),
                    cpu_time: Some(Duration::from_secs(30)),
                },
            },
            Self::CiRunner => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 256 * 1024 * 1024,   // 256MB
                    stack_max: 4 * 1024 * 1024,    // 4MB
                    total_max: 512 * 1024 * 1024,  // 512MB
                },
                cpu: CpuLimits {
                    fuel: Some(10_000_000),
                    cpu_time: Some(Duration::from_secs(300)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(100 * 1024 * 1024), // 100MB
                    write_bytes: Some(50 * 1024 * 1024), // 50MB
                    iops: Some(5000),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(300)),
                    cpu_time: Some(Duration::from_secs(300)),
                },
            },
            Self::EdgeFunction => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 16 * 1024 * 1024,    // 16MB
                    stack_max: 512 * 1024,          // 512KB
                    total_max: 32 * 1024 * 1024,   // 32MB
                },
                cpu: CpuLimits {
                    fuel: Some(500_000),
                    cpu_time: Some(Duration::from_secs(5)),
                    preemption_interval: Duration::from_millis(5),
                },
                io: IoLimits {
                    read_bytes: Some(5 * 1024 * 1024),   // 5MB
                    write_bytes: Some(1024 * 1024),      // 1MB
                    iops: Some(200),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(5)),
                    cpu_time: Some(Duration::from_secs(5)),
                },
            },
            Self::Playground => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 32 * 1024 * 1024,    // 32MB
                    stack_max: 512 * 1024,          // 512KB
                    total_max: 64 * 1024 * 1024,   // 64MB
                },
                cpu: CpuLimits {
                    fuel: Some(200_000),
                    cpu_time: Some(Duration::from_secs(10)),
                    preemption_interval: Duration::from_millis(10),
                },
                io: IoLimits {
                    read_bytes: Some(1024 * 1024),       // 1MB
                    write_bytes: Some(1024 * 1024),      // 1MB
                    iops: Some(100),
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(10)),
                    cpu_time: Some(Duration::from_secs(10)),
                },
            },
            Self::Unrestricted => ResourceLimits {
                memory: MemoryLimits {
                    heap_max: 1024 * 1024 * 1024,  // 1GB
                    stack_max: 8 * 1024 * 1024,    // 8MB
                    total_max: 2048 * 1024 * 1024, // 2GB
                },
                cpu: CpuLimits {
                    fuel: None, // Unlimited
                    cpu_time: None,
                    preemption_interval: Duration::from_millis(100),
                },
                io: IoLimits {
                    read_bytes: None,
                    write_bytes: None,
                    iops: None,
                },
                time: TimeLimits {
                    wall_time: Some(Duration::from_secs(3600)), // 1 hour
                    cpu_time: None,
                },
            },
        }
    }

    /// Get the granted capabilities for this profile.
    pub fn capabilities(&self) -> Vec<Capability> {
        match self {
            Self::AiCodeExecution => vec![
                Capability::stdout(),
                Capability::stderr(),
            ],
            Self::PluginRuntime => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::temp_dir(),
                Capability::host_function("*"),
            ],
            Self::CiRunner => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::filesystem_read("/"),
                Capability::temp_dir(),
                Capability::env_all(),
            ],
            Self::EdgeFunction => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::http_client(vec!["*".to_string()]),
                Capability::system_clock(),
                Capability::monotonic_clock(),
            ],
            Self::Playground => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::stdin(),
                Capability::system_clock(),
                Capability::monotonic_clock(),
                Capability::secure_random(),
            ],
            Self::Unrestricted => vec![
                Capability::stdout(),
                Capability::stderr(),
                Capability::stdin(),
                Capability::filesystem_read("/"),
                Capability::filesystem_write("/tmp"),
                Capability::temp_dir(),
                Capability::env_all(),
                Capability::system_clock(),
                Capability::monotonic_clock(),
                Capability::timers(),
                Capability::secure_random(),
            ],
        }
    }

    /// Get a human-readable description of this profile.
    pub fn description(&self) -> &'static str {
        match self {
            Self::AiCodeExecution => {
                "AI Code Execution: conservative limits (10s, 32MB, 100K fuel), stdout+stderr only"
            }
            Self::PluginRuntime => {
                "Plugin Runtime: moderate limits (30s, 64MB, 1M fuel), temp dir, host functions"
            }
            Self::CiRunner => {
                "CI Runner: generous limits (300s, 256MB, 10M fuel), filesystem read, env access"
            }
            Self::EdgeFunction => {
                "Edge Function: tight limits (5s, 16MB, 500K fuel), HTTP client, clock access"
            }
            Self::Playground => {
                "Playground: strict limits (10s, 32MB, 200K fuel), stdin/stdout, clock, random"
            }
            Self::Unrestricted => {
                "Unrestricted: generous limits (1h, 1GB, unlimited fuel), broad capabilities"
            }
        }
    }

    /// Get the security level of this profile.
    pub fn security_level(&self) -> SecurityLevel {
        match self {
            Self::AiCodeExecution => SecurityLevel::Conservative,
            Self::PluginRuntime => SecurityLevel::Moderate,
            Self::CiRunner => SecurityLevel::Moderate,
            Self::EdgeFunction => SecurityLevel::Conservative,
            Self::Playground => SecurityLevel::Conservative,
            Self::Unrestricted => SecurityLevel::Permissive,
        }
    }

    /// Apply this profile's settings to a
    /// [`SandboxConfigBuilder`](crate::config::SandboxConfigBuilder).
    ///
    /// Sets resource limits and capabilities. Settings applied by the profile
    /// can be overridden by subsequent builder calls.
    ///
    /// Prefer using [`SandboxConfigBuilder::use_profile`](crate::config::SandboxConfigBuilder::use_profile)
    /// for a more ergonomic builder chain.
    pub fn apply_to_builder(
        self,
        builder: crate::config::SandboxConfigBuilder,
    ) -> crate::config::SandboxConfigBuilder {
        builder.use_profile(self)
    }
}

impl fmt::Display for SandboxProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::AiCodeExecution => "ai-code-execution",
            Self::PluginRuntime => "plugin-runtime",
            Self::CiRunner => "ci-runner",
            Self::EdgeFunction => "edge-function",
            Self::Playground => "playground",
            Self::Unrestricted => "unrestricted",
        };
        write!(f, "{}", name)
    }
}

impl std::str::FromStr for SandboxProfile {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().replace('_', "-").as_str() {
            "ai-code-execution" | "ai" => Ok(Self::AiCodeExecution),
            "plugin-runtime" | "plugin" => Ok(Self::PluginRuntime),
            "ci-runner" | "ci" => Ok(Self::CiRunner),
            "edge-function" | "edge" => Ok(Self::EdgeFunction),
            "playground" => Ok(Self::Playground),
            "unrestricted" | "dev" => Ok(Self::Unrestricted),
            _ => Err(format!(
                "Unknown sandbox profile: '{}'. Available: ai-code-execution, plugin-runtime, ci-runner, edge-function, playground, unrestricted",
                s
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PROFILES: [SandboxProfile; 6] = [
        SandboxProfile::AiCodeExecution,
        SandboxProfile::PluginRuntime,
        SandboxProfile::CiRunner,
        SandboxProfile::EdgeFunction,
        SandboxProfile::Playground,
        SandboxProfile::Unrestricted,
    ];

    #[test]
    fn test_all_profiles_have_valid_resource_limits() {
        for profile in &ALL_PROFILES {
            let limits = profile.resource_limits();
            assert!(limits.memory.heap_max > 0, "{:?} heap_max must be > 0", profile);
            assert!(limits.memory.stack_max > 0, "{:?} stack_max must be > 0", profile);
            assert!(
                limits.memory.total_max >= limits.memory.heap_max,
                "{:?} total_max must be >= heap_max",
                profile
            );
            assert!(
                limits.time.wall_time.is_some(),
                "{:?} should have a wall time limit",
                profile
            );
        }
    }

    #[test]
    fn test_ai_code_execution_resource_limits() {
        let limits = SandboxProfile::AiCodeExecution.resource_limits();
        assert_eq!(limits.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(100_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_plugin_runtime_resource_limits() {
        let limits = SandboxProfile::PluginRuntime.resource_limits();
        assert_eq!(limits.memory.heap_max, 64 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(1_000_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_ci_runner_resource_limits() {
        let limits = SandboxProfile::CiRunner.resource_limits();
        assert_eq!(limits.memory.heap_max, 256 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(10_000_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_edge_function_resource_limits() {
        let limits = SandboxProfile::EdgeFunction.resource_limits();
        assert_eq!(limits.memory.heap_max, 16 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(500_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(5)));
    }

    #[test]
    fn test_playground_resource_limits() {
        let limits = SandboxProfile::Playground.resource_limits();
        assert_eq!(limits.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(200_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(10)));
    }

    #[test]
    fn test_unrestricted_resource_limits() {
        let limits = SandboxProfile::Unrestricted.resource_limits();
        assert_eq!(limits.memory.heap_max, 1024 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, None);
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(3600)));
    }

    #[test]
    fn test_ai_code_execution_capabilities() {
        let caps = SandboxProfile::AiCodeExecution.capabilities();
        assert_eq!(caps.len(), 2);
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
    }

    #[test]
    fn test_plugin_runtime_capabilities() {
        let caps = SandboxProfile::PluginRuntime.capabilities();
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::temp_dir()));
        assert!(caps.contains(&Capability::host_function("*")));
    }

    #[test]
    fn test_ci_runner_capabilities() {
        let caps = SandboxProfile::CiRunner.capabilities();
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::filesystem_read("/")));
        assert!(caps.contains(&Capability::temp_dir()));
        assert!(caps.contains(&Capability::env_all()));
    }

    #[test]
    fn test_edge_function_capabilities() {
        let caps = SandboxProfile::EdgeFunction.capabilities();
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::http_client(vec!["*".to_string()])));
        assert!(caps.contains(&Capability::system_clock()));
        assert!(caps.contains(&Capability::monotonic_clock()));
    }

    #[test]
    fn test_playground_capabilities() {
        let caps = SandboxProfile::Playground.capabilities();
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::stdin()));
        assert!(caps.contains(&Capability::system_clock()));
        assert!(caps.contains(&Capability::secure_random()));
    }

    #[test]
    fn test_unrestricted_capabilities() {
        let caps = SandboxProfile::Unrestricted.capabilities();
        assert!(caps.contains(&Capability::stdout()));
        assert!(caps.contains(&Capability::stderr()));
        assert!(caps.contains(&Capability::stdin()));
        assert!(caps.contains(&Capability::filesystem_read("/")));
        assert!(caps.contains(&Capability::temp_dir()));
        assert!(caps.contains(&Capability::env_all()));
        assert!(caps.contains(&Capability::system_clock()));
        assert!(caps.contains(&Capability::secure_random()));
    }

    #[test]
    fn test_all_profiles_have_stdout() {
        for profile in &ALL_PROFILES {
            let caps = profile.capabilities();
            assert!(
                caps.contains(&Capability::stdout()),
                "{:?} missing stdout capability",
                profile
            );
        }
    }

    #[test]
    fn test_security_levels() {
        assert_eq!(
            SandboxProfile::AiCodeExecution.security_level(),
            SecurityLevel::Conservative
        );
        assert_eq!(
            SandboxProfile::PluginRuntime.security_level(),
            SecurityLevel::Moderate
        );
        assert_eq!(
            SandboxProfile::CiRunner.security_level(),
            SecurityLevel::Moderate
        );
        assert_eq!(
            SandboxProfile::EdgeFunction.security_level(),
            SecurityLevel::Conservative
        );
        assert_eq!(
            SandboxProfile::Playground.security_level(),
            SecurityLevel::Conservative
        );
        assert_eq!(
            SandboxProfile::Unrestricted.security_level(),
            SecurityLevel::Permissive
        );
    }

    #[test]
    fn test_descriptions_non_empty() {
        for profile in &ALL_PROFILES {
            assert!(
                !profile.description().is_empty(),
                "{:?} has empty description",
                profile
            );
        }
    }

    #[test]
    fn test_security_level_display() {
        assert_eq!(SecurityLevel::Conservative.to_string(), "Conservative");
        assert_eq!(SecurityLevel::Moderate.to_string(), "Moderate");
        assert_eq!(SecurityLevel::Permissive.to_string(), "Permissive");
    }

    #[test]
    fn test_security_level_from_str() {
        assert_eq!(
            "conservative".parse::<SecurityLevel>().unwrap(),
            SecurityLevel::Conservative
        );
        assert_eq!(
            "Moderate".parse::<SecurityLevel>().unwrap(),
            SecurityLevel::Moderate
        );
        assert_eq!(
            "PERMISSIVE".parse::<SecurityLevel>().unwrap(),
            SecurityLevel::Permissive
        );
        assert!("unknown".parse::<SecurityLevel>().is_err());
    }

    #[test]
    fn test_sandbox_profile_display() {
        assert_eq!(SandboxProfile::AiCodeExecution.to_string(), "ai-code-execution");
        assert_eq!(SandboxProfile::PluginRuntime.to_string(), "plugin-runtime");
        assert_eq!(SandboxProfile::CiRunner.to_string(), "ci-runner");
        assert_eq!(SandboxProfile::EdgeFunction.to_string(), "edge-function");
        assert_eq!(SandboxProfile::Playground.to_string(), "playground");
        assert_eq!(SandboxProfile::Unrestricted.to_string(), "unrestricted");
    }

    #[test]
    fn test_sandbox_profile_from_str() {
        assert_eq!(
            "ai-code-execution".parse::<SandboxProfile>().unwrap(),
            SandboxProfile::AiCodeExecution
        );
        assert_eq!("ai".parse::<SandboxProfile>().unwrap(), SandboxProfile::AiCodeExecution);
        assert_eq!(
            "plugin-runtime".parse::<SandboxProfile>().unwrap(),
            SandboxProfile::PluginRuntime
        );
        assert_eq!("plugin".parse::<SandboxProfile>().unwrap(), SandboxProfile::PluginRuntime);
        assert_eq!("ci-runner".parse::<SandboxProfile>().unwrap(), SandboxProfile::CiRunner);
        assert_eq!("ci".parse::<SandboxProfile>().unwrap(), SandboxProfile::CiRunner);
        assert_eq!(
            "edge-function".parse::<SandboxProfile>().unwrap(),
            SandboxProfile::EdgeFunction
        );
        assert_eq!("edge".parse::<SandboxProfile>().unwrap(), SandboxProfile::EdgeFunction);
        assert_eq!(
            "playground".parse::<SandboxProfile>().unwrap(),
            SandboxProfile::Playground
        );
        assert_eq!(
            "unrestricted".parse::<SandboxProfile>().unwrap(),
            SandboxProfile::Unrestricted
        );
        assert_eq!("dev".parse::<SandboxProfile>().unwrap(), SandboxProfile::Unrestricted);
        assert!("unknown".parse::<SandboxProfile>().is_err());
    }

    #[test]
    fn test_apply_to_builder() {
        let wasm_bytes = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let builder = crate::SandboxConfig::builder()
            .module(wasm_bytes)
            .expect("valid module");
        let builder = SandboxProfile::AiCodeExecution.apply_to_builder(builder);
        let config = builder.build().expect("valid config");

        assert_eq!(config.resources.memory.heap_max, 32 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(100_000));
        assert!(config.capabilities.has(&Capability::stdout()));
        assert!(config.capabilities.has(&Capability::stderr()));
    }

    #[test]
    fn test_use_profile_on_builder() {
        let wasm_bytes = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let config = crate::SandboxConfig::builder()
            .module(wasm_bytes)
            .expect("valid module")
            .use_profile(SandboxProfile::EdgeFunction)
            .build()
            .expect("valid config");

        assert_eq!(config.resources.memory.heap_max, 16 * 1024 * 1024);
        assert_eq!(config.resources.cpu.fuel, Some(500_000));
        assert!(config.capabilities.has(&Capability::stdout()));
        assert!(config.capabilities.has(&Capability::system_clock()));
    }

    #[test]
    fn test_profile_serialization() {
        let profile = SandboxProfile::AiCodeExecution;
        let json = serde_json::to_string(&profile).expect("serialize");
        let deserialized: SandboxProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(profile, deserialized);
    }

    #[test]
    fn test_security_level_serialization() {
        let level = SecurityLevel::Conservative;
        let json = serde_json::to_string(&level).expect("serialize");
        let deserialized: SecurityLevel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(level, deserialized);
    }

    #[test]
    fn test_conservative_profiles_have_no_filesystem() {
        for profile in &ALL_PROFILES {
            if profile.security_level() == SecurityLevel::Conservative {
                let caps = profile.capabilities();
                let has_fs = caps.iter().any(|c| matches!(c, Capability::Filesystem(_)));
                assert!(
                    !has_fs,
                    "{:?} is Conservative but has filesystem capabilities",
                    profile
                );
            }
        }
    }

    #[test]
    fn test_profile_clone_and_eq() {
        let profile = SandboxProfile::PluginRuntime;
        let cloned = profile.clone();
        assert_eq!(profile, cloned);
    }
}
