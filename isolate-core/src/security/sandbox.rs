//! Security context and sandbox configuration.

use super::policy::{LandlockPolicy, SeccompPolicy};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Namespace isolation options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceConfig {
    /// Use a new user namespace.
    pub user: bool,
    /// Use a new PID namespace.
    pub pid: bool,
    /// Use a new network namespace.
    pub network: bool,
    /// Use a new mount namespace.
    pub mount: bool,
    /// Use a new UTS namespace.
    pub uts: bool,
    /// Use a new IPC namespace.
    pub ipc: bool,
    /// Use a new cgroup namespace.
    pub cgroup: bool,
}

impl Default for NamespaceConfig {
    fn default() -> Self {
        Self {
            user: false,
            pid: false,
            network: false,
            mount: false,
            uts: false,
            ipc: false,
            cgroup: false,
        }
    }
}

impl NamespaceConfig {
    /// Create config with all namespaces enabled.
    pub fn all() -> Self {
        Self {
            user: true,
            pid: true,
            network: true,
            mount: true,
            uts: true,
            ipc: true,
            cgroup: true,
        }
    }

    /// Enable user namespace.
    pub fn with_user(mut self, enabled: bool) -> Self {
        self.user = enabled;
        self
    }

    /// Enable PID namespace.
    pub fn with_pid(mut self, enabled: bool) -> Self {
        self.pid = enabled;
        self
    }

    /// Enable network namespace.
    pub fn with_network(mut self, enabled: bool) -> Self {
        self.network = enabled;
        self
    }

    /// Enable mount namespace.
    pub fn with_mount(mut self, enabled: bool) -> Self {
        self.mount = enabled;
        self
    }

    /// Check if any namespace is enabled.
    pub fn any_enabled(&self) -> bool {
        self.user || self.pid || self.network || self.mount || self.uts || self.ipc || self.cgroup
    }

    /// Get enabled namespace flags.
    pub fn flags(&self) -> u32 {
        let mut flags = 0u32;
        // CLONE_NEWUSER = 0x10000000
        // CLONE_NEWPID = 0x20000000
        // CLONE_NEWNET = 0x40000000
        // CLONE_NEWNS = 0x00020000
        // CLONE_NEWUTS = 0x04000000
        // CLONE_NEWIPC = 0x08000000
        // CLONE_NEWCGROUP = 0x02000000
        if self.user {
            flags |= 0x10000000;
        }
        if self.pid {
            flags |= 0x20000000;
        }
        if self.network {
            flags |= 0x40000000;
        }
        if self.mount {
            flags |= 0x00020000;
        }
        if self.uts {
            flags |= 0x04000000;
        }
        if self.ipc {
            flags |= 0x08000000;
        }
        if self.cgroup {
            flags |= 0x02000000;
        }
        flags
    }
}

/// Resource limits via cgroups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgroupLimits {
    /// Memory limit in bytes.
    pub memory_limit: Option<u64>,
    /// CPU weight (1-10000).
    pub cpu_weight: Option<u32>,
    /// CPU quota in microseconds per period.
    pub cpu_quota_us: Option<u64>,
    /// CPU period in microseconds.
    pub cpu_period_us: Option<u64>,
    /// Maximum number of PIDs.
    pub pids_max: Option<u32>,
    /// I/O weight (1-10000).
    pub io_weight: Option<u32>,
}

impl Default for CgroupLimits {
    fn default() -> Self {
        Self {
            memory_limit: None,
            cpu_weight: None,
            cpu_quota_us: None,
            cpu_period_us: None,
            pids_max: None,
            io_weight: None,
        }
    }
}

impl CgroupLimits {
    /// Create new cgroup limits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set memory limit.
    pub fn with_memory(mut self, limit: u64) -> Self {
        self.memory_limit = Some(limit);
        self
    }

    /// Set CPU weight.
    pub fn with_cpu_weight(mut self, weight: u32) -> Self {
        self.cpu_weight = Some(weight.clamp(1, 10000));
        self
    }

    /// Set CPU quota.
    pub fn with_cpu_quota(mut self, quota_us: u64, period_us: u64) -> Self {
        self.cpu_quota_us = Some(quota_us);
        self.cpu_period_us = Some(period_us);
        self
    }

    /// Set max PIDs.
    pub fn with_pids_max(mut self, max: u32) -> Self {
        self.pids_max = Some(max);
        self
    }

    /// Set I/O weight.
    pub fn with_io_weight(mut self, weight: u32) -> Self {
        self.io_weight = Some(weight.clamp(1, 10000));
        self
    }

    /// Check if any limits are set.
    pub fn any_set(&self) -> bool {
        self.memory_limit.is_some()
            || self.cpu_weight.is_some()
            || self.cpu_quota_us.is_some()
            || self.pids_max.is_some()
            || self.io_weight.is_some()
    }
}

/// Comprehensive security context.
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Seccomp policy.
    pub seccomp: Option<SeccompPolicy>,
    /// Landlock policy.
    pub landlock: Option<LandlockPolicy>,
    /// Namespace configuration.
    pub namespaces: NamespaceConfig,
    /// Cgroup limits.
    pub cgroups: CgroupLimits,
    /// Drop all capabilities.
    pub no_capabilities: bool,
    /// Capabilities to keep (if no_capabilities is true).
    pub keep_capabilities: HashSet<String>,
    /// Set no_new_privs.
    pub no_new_privs: bool,
    /// Context name for identification.
    pub name: Option<String>,
}

impl Default for SecurityContext {
    fn default() -> Self {
        Self {
            seccomp: None,
            landlock: None,
            namespaces: NamespaceConfig::default(),
            cgroups: CgroupLimits::default(),
            no_capabilities: true,
            keep_capabilities: HashSet::new(),
            no_new_privs: true,
            name: None,
        }
    }
}

impl SecurityContext {
    /// Create a new security context builder.
    pub fn builder() -> SecurityContextBuilder {
        SecurityContextBuilder::new()
    }

    /// Create a minimal security context (for testing).
    pub fn minimal() -> Self {
        Self {
            seccomp: None,
            landlock: None,
            namespaces: NamespaceConfig::default(),
            cgroups: CgroupLimits::default(),
            no_capabilities: false,
            keep_capabilities: HashSet::new(),
            no_new_privs: false,
            name: Some("minimal".to_string()),
        }
    }

    /// Create a strict security context.
    pub fn strict() -> Self {
        Self::builder()
            .seccomp_policy(SeccompPolicy::strict())
            .enable_landlock(true)
            .enable_namespaces(true)
            .no_capabilities(true)
            .no_new_privs(true)
            .with_name("strict")
            .build()
            .expect("strict context should always build")
    }

    /// Create a sandbox security context.
    pub fn sandbox() -> Self {
        Self::builder()
            .seccomp_policy(SeccompPolicy::sandbox())
            .no_capabilities(true)
            .no_new_privs(true)
            .with_name("sandbox")
            .build()
            .expect("sandbox context should always build")
    }

    /// Apply the security context to the current process.
    ///
    /// This is a no-op on non-Linux systems.
    pub fn apply(&self) -> Result<(), SecurityError> {
        #[cfg(target_os = "linux")]
        {
            self.apply_linux()?;
        }

        #[cfg(not(target_os = "linux"))]
        {
            if self.seccomp.is_some() || self.landlock.is_some() {
                return Err(SecurityError::NotSupported(
                    "seccomp/landlock only available on Linux".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Apply security on Linux.
    #[cfg(target_os = "linux")]
    fn apply_linux(&self) -> Result<(), SecurityError> {
        // Apply no_new_privs first
        if self.no_new_privs {
            // Would use prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)
        }

        // Apply seccomp
        if let Some(ref _policy) = self.seccomp {
            // Would compile BPF filter and apply with seccomp()
        }

        // Apply landlock
        if let Some(ref _policy) = self.landlock {
            // Would create ruleset and restrict self
        }

        // Note: Actual implementation would use libc or nix crate
        // This is a skeleton for the API

        Ok(())
    }

    /// Validate the security context.
    pub fn validate(&self) -> Result<(), SecurityError> {
        // Check for conflicting settings
        if self.no_capabilities && !self.keep_capabilities.is_empty() {
            // This is fine - keep_capabilities specifies exceptions
        }

        // Validate seccomp policy if present
        if let Some(ref policy) = self.seccomp {
            if policy.rules.is_empty()
                && matches!(policy.default_action, super::policy::SeccompAction::Kill)
            {
                return Err(SecurityError::InvalidConfig(
                    "Seccomp policy with no rules and default kill would block everything"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Check if any security features are enabled.
    pub fn is_enabled(&self) -> bool {
        self.seccomp.is_some()
            || self.landlock.is_some()
            || self.namespaces.any_enabled()
            || self.cgroups.any_set()
            || self.no_capabilities
            || self.no_new_privs
    }

    /// Get a summary of enabled features.
    pub fn summary(&self) -> Vec<String> {
        let mut features = Vec::new();

        if let Some(ref policy) = self.seccomp {
            features.push(format!(
                "seccomp: {}",
                policy.name.as_deref().unwrap_or("custom")
            ));
        }

        if let Some(ref policy) = self.landlock {
            features.push(format!("landlock: {} rules", policy.rules.len()));
        }

        if self.namespaces.any_enabled() {
            let mut ns = Vec::new();
            if self.namespaces.user {
                ns.push("user");
            }
            if self.namespaces.pid {
                ns.push("pid");
            }
            if self.namespaces.network {
                ns.push("net");
            }
            if self.namespaces.mount {
                ns.push("mnt");
            }
            features.push(format!("namespaces: {}", ns.join(",")));
        }

        if self.cgroups.any_set() {
            features.push("cgroups: enabled".to_string());
        }

        if self.no_capabilities {
            features.push("no_capabilities".to_string());
        }

        if self.no_new_privs {
            features.push("no_new_privs".to_string());
        }

        features
    }
}

/// Builder for SecurityContext.
#[derive(Debug, Clone, Default)]
pub struct SecurityContextBuilder {
    seccomp: Option<SeccompPolicy>,
    landlock: Option<LandlockPolicy>,
    namespaces: NamespaceConfig,
    cgroups: CgroupLimits,
    no_capabilities: bool,
    keep_capabilities: HashSet<String>,
    no_new_privs: bool,
    name: Option<String>,
}

impl SecurityContextBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set seccomp policy.
    pub fn seccomp_policy(mut self, policy: SeccompPolicy) -> Self {
        self.seccomp = Some(policy);
        self
    }

    /// Set landlock policy.
    pub fn landlock_policy(mut self, policy: LandlockPolicy) -> Self {
        self.landlock = Some(policy);
        self
    }

    /// Enable landlock with default policy.
    pub fn enable_landlock(mut self, enable: bool) -> Self {
        if enable && self.landlock.is_none() {
            self.landlock = Some(LandlockPolicy::new());
        } else if !enable {
            self.landlock = None;
        }
        self
    }

    /// Set namespace configuration.
    pub fn namespaces(mut self, config: NamespaceConfig) -> Self {
        self.namespaces = config;
        self
    }

    /// Enable all namespaces.
    pub fn enable_namespaces(mut self, enable: bool) -> Self {
        if enable {
            self.namespaces = NamespaceConfig::all();
        } else {
            self.namespaces = NamespaceConfig::default();
        }
        self
    }

    /// Set cgroup limits.
    pub fn cgroups(mut self, limits: CgroupLimits) -> Self {
        self.cgroups = limits;
        self
    }

    /// Set memory limit via cgroups.
    pub fn memory_limit(mut self, bytes: u64) -> Self {
        self.cgroups.memory_limit = Some(bytes);
        self
    }

    /// Set CPU quota via cgroups.
    pub fn cpu_quota(mut self, quota_us: u64, period_us: u64) -> Self {
        self.cgroups.cpu_quota_us = Some(quota_us);
        self.cgroups.cpu_period_us = Some(period_us);
        self
    }

    /// Set max PIDs via cgroups.
    pub fn pids_max(mut self, max: u32) -> Self {
        self.cgroups.pids_max = Some(max);
        self
    }

    /// Drop all capabilities.
    pub fn no_capabilities(mut self, drop: bool) -> Self {
        self.no_capabilities = drop;
        self
    }

    /// Keep a specific capability.
    pub fn keep_capability(mut self, cap: impl Into<String>) -> Self {
        self.keep_capabilities.insert(cap.into());
        self
    }

    /// Set no_new_privs.
    pub fn no_new_privs(mut self, enable: bool) -> Self {
        self.no_new_privs = enable;
        self
    }

    /// Set context name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Build the security context.
    pub fn build(self) -> Result<SecurityContext, SecurityError> {
        let context = SecurityContext {
            seccomp: self.seccomp,
            landlock: self.landlock,
            namespaces: self.namespaces,
            cgroups: self.cgroups,
            no_capabilities: self.no_capabilities,
            keep_capabilities: self.keep_capabilities,
            no_new_privs: self.no_new_privs,
            name: self.name,
        };

        context.validate()?;
        Ok(context)
    }
}

/// Security-related errors.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// Feature not supported on this platform.
    #[error("Not supported: {0}")]
    NotSupported(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// System error.
    #[error("System error: {0}")]
    SystemError(String),

    /// Seccomp error.
    #[error("Seccomp error: {0}")]
    SeccompError(String),

    /// Landlock error.
    #[error("Landlock error: {0}")]
    LandlockError(String),

    /// Namespace error.
    #[error("Namespace error: {0}")]
    NamespaceError(String),

    /// Cgroup error.
    #[error("Cgroup error: {0}")]
    CgroupError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_config_default() {
        let config = NamespaceConfig::default();
        assert!(!config.any_enabled());
        assert_eq!(config.flags(), 0);
    }

    #[test]
    fn test_namespace_config_all() {
        let config = NamespaceConfig::all();
        assert!(config.any_enabled());
        assert!(config.user);
        assert!(config.pid);
        assert!(config.network);
    }

    #[test]
    fn test_namespace_config_flags() {
        let config = NamespaceConfig::default().with_user(true).with_pid(true);

        let flags = config.flags();
        assert!(flags & 0x10000000 != 0); // CLONE_NEWUSER
        assert!(flags & 0x20000000 != 0); // CLONE_NEWPID
    }

    #[test]
    fn test_cgroup_limits() {
        let limits = CgroupLimits::new()
            .with_memory(1024 * 1024 * 1024)
            .with_cpu_weight(100)
            .with_pids_max(100);

        assert!(limits.any_set());
        assert_eq!(limits.memory_limit, Some(1024 * 1024 * 1024));
        assert_eq!(limits.cpu_weight, Some(100));
        assert_eq!(limits.pids_max, Some(100));
    }

    #[test]
    fn test_cgroup_limits_clamp() {
        let limits = CgroupLimits::new()
            .with_cpu_weight(50000) // Should clamp to 10000
            .with_io_weight(0); // Should clamp to 1

        assert_eq!(limits.cpu_weight, Some(10000));
        assert_eq!(limits.io_weight, Some(1));
    }

    #[test]
    fn test_security_context_minimal() {
        let context = SecurityContext::minimal();
        assert!(!context.no_capabilities);
        assert!(!context.no_new_privs);
    }

    #[test]
    fn test_security_context_strict() {
        let context = SecurityContext::strict();
        assert!(context.seccomp.is_some());
        assert!(context.no_capabilities);
        assert!(context.no_new_privs);
        assert!(context.namespaces.any_enabled());
    }

    #[test]
    fn test_security_context_sandbox() {
        let context = SecurityContext::sandbox();
        assert!(context.seccomp.is_some());
        assert!(context.no_capabilities);
    }

    #[test]
    fn test_security_context_builder() {
        let context = SecurityContext::builder()
            .seccomp_policy(SeccompPolicy::sandbox())
            .memory_limit(512 * 1024 * 1024)
            .pids_max(50)
            .no_capabilities(true)
            .no_new_privs(true)
            .with_name("test")
            .build()
            .unwrap();

        assert!(context.seccomp.is_some());
        assert_eq!(context.cgroups.memory_limit, Some(512 * 1024 * 1024));
        assert_eq!(context.cgroups.pids_max, Some(50));
        assert!(context.no_capabilities);
        assert_eq!(context.name, Some("test".to_string()));
    }

    #[test]
    fn test_security_context_builder_namespaces() {
        let context = SecurityContext::builder()
            .enable_namespaces(true)
            .build()
            .unwrap();

        assert!(context.namespaces.any_enabled());
    }

    #[test]
    fn test_security_context_keep_capability() {
        let context = SecurityContext::builder()
            .no_capabilities(true)
            .keep_capability("CAP_NET_BIND_SERVICE")
            .build()
            .unwrap();

        assert!(context.keep_capabilities.contains("CAP_NET_BIND_SERVICE"));
    }

    #[test]
    fn test_security_context_validate() {
        let context = SecurityContext::builder()
            .seccomp_policy(SeccompPolicy::sandbox())
            .build()
            .unwrap();

        assert!(context.validate().is_ok());
    }

    #[test]
    fn test_security_context_is_enabled() {
        let empty = SecurityContext::minimal();
        assert!(!empty.is_enabled());

        let with_seccomp = SecurityContext::sandbox();
        assert!(with_seccomp.is_enabled());
    }

    #[test]
    fn test_security_context_summary() {
        let context = SecurityContext::strict();
        let summary = context.summary();

        assert!(!summary.is_empty());
        assert!(summary.iter().any(|s| s.contains("seccomp")));
    }

    #[test]
    fn test_security_context_apply_non_linux() {
        let context = SecurityContext::minimal();
        // Should succeed on any platform with minimal context
        assert!(context.apply().is_ok());
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn test_security_context_apply_seccomp_non_linux() {
        let context = SecurityContext::sandbox();
        // Should fail on non-Linux
        assert!(matches!(
            context.apply(),
            Err(SecurityError::NotSupported(_))
        ));
    }
}
