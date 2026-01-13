//! Capability enforcement.

use super::{
    AuditLog, Capability, CapabilitySet, EnvironmentCapability, FilesystemCapability,
    NetworkCapability, StdioCapability,
};
use crate::error::{Error, Result};
use std::net::SocketAddr;
use std::path::Path;
use uuid::Uuid;

/// Enforces capability checks for sandbox operations.
#[derive(Debug, Clone)]
pub struct CapabilityEnforcer {
    /// Granted capabilities.
    granted: CapabilitySet,
    /// Audit log for recording capability usage.
    audit_log: AuditLog,
}

impl CapabilityEnforcer {
    /// Create a new capability enforcer.
    pub fn new(granted: CapabilitySet, sandbox_id: Uuid) -> Self {
        let audit_log = AuditLog::new(sandbox_id);

        // Log all granted capabilities
        for cap in granted.iter() {
            audit_log.record_granted(cap.clone());
        }

        Self { granted, audit_log }
    }

    /// Check if a capability is granted.
    pub fn check(&self, required: &Capability) -> Result<()> {
        if self.granted.has(required) {
            self.audit_log.record_used(required.clone(), None);
            Ok(())
        } else {
            self.audit_log.record_denied(required.clone(), None);
            Err(Error::CapabilityDenied(required.clone()))
        }
    }

    /// Check if stdout is allowed.
    pub fn check_stdout(&self) -> Result<()> {
        self.check(&Capability::Stdio(StdioCapability::Stdout))
    }

    /// Check if stderr is allowed.
    pub fn check_stderr(&self) -> Result<()> {
        self.check(&Capability::Stdio(StdioCapability::Stderr))
    }

    /// Check if stdin is allowed.
    pub fn check_stdin(&self) -> Result<()> {
        self.check(&Capability::Stdio(StdioCapability::Stdin))
    }

    /// Check if reading a path is allowed.
    pub fn check_fs_read(&self, path: &Path) -> Result<()> {
        // Check if any filesystem capability allows reading this path
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::Filesystem(fs) => fs.allows_read(path),
            _ => false,
        });

        if allowed {
            self.audit_log.record_used(
                Capability::filesystem_read(path),
                Some(format!("read: {}", path.display())),
            );
            Ok(())
        } else {
            let cap = Capability::filesystem_read(path);
            self.audit_log
                .record_denied(cap.clone(), Some(format!("read denied: {}", path.display())));
            Err(Error::FilesystemAccessDenied { path: path.to_path_buf() })
        }
    }

    /// Check if writing to a path is allowed.
    pub fn check_fs_write(&self, path: &Path) -> Result<()> {
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::Filesystem(fs) => fs.allows_write(path),
            _ => false,
        });

        if allowed {
            self.audit_log.record_used(
                Capability::filesystem_write(path),
                Some(format!("write: {}", path.display())),
            );
            Ok(())
        } else {
            let cap = Capability::filesystem_write(path);
            self.audit_log
                .record_denied(cap.clone(), Some(format!("write denied: {}", path.display())));
            Err(Error::FilesystemAccessDenied { path: path.to_path_buf() })
        }
    }

    /// Check if temp dir access is allowed.
    pub fn check_temp_dir(&self) -> Result<()> {
        self.check(&Capability::Filesystem(FilesystemCapability::TempDir))
    }

    /// Check if HTTP access to a host is allowed.
    pub fn check_http(&self, host: &str) -> Result<()> {
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::Network(NetworkCapability::HttpClient(hosts)) => {
                hosts.iter().any(|pattern| {
                    if pattern.starts_with("*.") {
                        let suffix = &pattern[1..];
                        host.ends_with(suffix) || host == &pattern[2..]
                    } else {
                        host == pattern
                    }
                })
            }
            _ => false,
        });

        if allowed {
            self.audit_log.record_used(
                Capability::http_client(vec![host.to_string()]),
                Some(format!("http: {}", host)),
            );
            Ok(())
        } else {
            let cap = Capability::http_client(vec![host.to_string()]);
            self.audit_log.record_denied(cap, Some(format!("http denied: {}", host)));
            Err(Error::NetworkAccessDenied { host: host.to_string() })
        }
    }

    /// Check if TCP connect to an address is allowed.
    pub fn check_tcp_connect(&self, addr: &SocketAddr) -> Result<()> {
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::Network(net) => net.allows_tcp_connect(addr),
            _ => false,
        });

        if allowed {
            self.audit_log.record_used(
                Capability::tcp_connect(vec![*addr]),
                Some(format!("tcp connect: {}", addr)),
            );
            Ok(())
        } else {
            let cap = Capability::tcp_connect(vec![*addr]);
            self.audit_log.record_denied(cap, Some(format!("tcp denied: {}", addr)));
            Err(Error::NetworkAccessDenied { host: addr.to_string() })
        }
    }

    /// Check if DNS resolution is allowed.
    pub fn check_dns(&self) -> Result<()> {
        self.check(&Capability::Network(NetworkCapability::DnsResolve))
    }

    /// Check if reading an environment variable is allowed.
    pub fn check_env_var(&self, name: &str) -> Result<()> {
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::Environment(env) => env.allows_var(name),
            _ => false,
        });

        if allowed {
            self.audit_log.record_used(Capability::env_var(name), Some(format!("env: {}", name)));
            Ok(())
        } else {
            let cap = Capability::env_var(name);
            self.audit_log.record_denied(cap.clone(), Some(format!("env denied: {}", name)));
            Err(Error::CapabilityDenied(cap))
        }
    }

    /// Check if reading command-line arguments is allowed.
    pub fn check_args(&self) -> Result<()> {
        self.check(&Capability::Environment(EnvironmentCapability::Args))
    }

    /// Check if system clock access is allowed.
    pub fn check_system_clock(&self) -> Result<()> {
        self.check(&Capability::system_clock())
    }

    /// Check if monotonic clock access is allowed.
    pub fn check_monotonic_clock(&self) -> Result<()> {
        self.check(&Capability::monotonic_clock())
    }

    /// Check if timer creation is allowed.
    pub fn check_timers(&self) -> Result<()> {
        self.check(&Capability::timers())
    }

    /// Check if secure random is allowed.
    pub fn check_secure_random(&self) -> Result<()> {
        self.check(&Capability::secure_random())
    }

    /// Check if a host function can be called.
    pub fn check_host_function(&self, name: &str) -> Result<()> {
        let allowed = self.granted.has_any(|cap| match cap {
            Capability::HostFunction(hf) => hf.allows_function(name),
            _ => false,
        });

        if allowed {
            self.audit_log
                .record_used(Capability::host_function(name), Some(format!("hostfn: {}", name)));
            Ok(())
        } else {
            let cap = Capability::host_function(name);
            self.audit_log.record_denied(cap.clone(), Some(format!("hostfn denied: {}", name)));
            Err(Error::CapabilityDenied(cap))
        }
    }

    /// Get the audit log.
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    /// Get the granted capabilities.
    pub fn granted(&self) -> &CapabilitySet {
        &self.granted
    }

    /// Get the list of directories that should be preopened for filesystem access.
    /// Returns pairs of (host_path, guest_path) where guest_path is the path
    /// visible inside the sandbox.
    pub fn filesystem_preopens(&self) -> Vec<(std::path::PathBuf, String)> {
        let mut preopens = Vec::new();

        for cap in self.granted.iter() {
            match cap {
                Capability::Filesystem(FilesystemCapability::ReadOnly(path))
                | Capability::Filesystem(FilesystemCapability::ReadWrite(path)) => {
                    // Use the same path for both host and guest
                    let guest_path = path.to_string_lossy().to_string();
                    preopens.push((path.clone(), guest_path));
                }
                Capability::Filesystem(FilesystemCapability::TempDir) => {
                    // Add /tmp as a preopened directory
                    preopens.push((std::path::PathBuf::from("/tmp"), "/tmp".to_string()));
                }
                _ => {}
            }
        }

        preopens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_enforcer_check_granted() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_stdout().is_ok());
        assert!(enforcer.check_stderr().is_err());
    }

    #[test]
    fn test_enforcer_check_fs_read() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::filesystem_read("/data"));

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_fs_read(Path::new("/data/file.txt")).is_ok());
        assert!(enforcer.check_fs_read(Path::new("/secret/file.txt")).is_err());
    }

    #[test]
    fn test_enforcer_check_fs_write() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::Filesystem(FilesystemCapability::ReadWrite(PathBuf::from("/data"))));

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_fs_write(Path::new("/data/file.txt")).is_ok());
        assert!(enforcer.check_fs_read(Path::new("/data/file.txt")).is_ok());
    }

    #[test]
    fn test_enforcer_check_http() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::http_client(vec!["api.example.com", "*.trusted.com"]));

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_http("api.example.com").is_ok());
        assert!(enforcer.check_http("sub.trusted.com").is_ok());
        assert!(enforcer.check_http("evil.com").is_err());
    }

    #[test]
    fn test_enforcer_check_env() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::env_var("API_KEY"));
        caps.grant(Capability::env_all());

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_env_var("API_KEY").is_ok());
        assert!(enforcer.check_env_var("OTHER").is_ok()); // env_all allows all
    }

    #[test]
    fn test_enforcer_audit_logging() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        // One grant event from initialization
        assert_eq!(enforcer.audit_log().events().len(), 1);

        enforcer.check_stdout().unwrap();
        let _ = enforcer.check_stderr(); // This will fail

        let events = enforcer.audit_log().events();
        assert_eq!(events.len(), 3); // 1 grant + 1 used + 1 denied
        assert_eq!(enforcer.audit_log().denied_count(), 1);
    }
}
