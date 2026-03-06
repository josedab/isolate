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
    ///
    /// Rejects paths containing `..` components to prevent directory traversal.
    pub fn check_fs_read(&self, path: &Path) -> Result<()> {
        Self::validate_path(path)?;

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
    ///
    /// Rejects paths containing `..` components to prevent directory traversal.
    pub fn check_fs_write(&self, path: &Path) -> Result<()> {
        Self::validate_path(path)?;

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
                        // *.example.com matches foo.example.com but NOT example.com
                        // and NOT evil-example.com (must be a subdomain boundary)
                        let suffix = &pattern[1..]; // ".example.com"
                        if let Some(prefix) = host.strip_suffix(suffix) {
                            // prefix must be non-empty and not contain dots
                            // for single-level wildcard, or just be non-empty
                            !prefix.is_empty() && !prefix.ends_with('.')
                        } else {
                            false
                        }
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
    ///
    /// Returns the [`AuditLog`] recording all capability checks (granted, used,
    /// and denied) for this enforcer's lifetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::capability::{CapabilityEnforcer, CapabilitySet, Capability};
    /// use uuid::Uuid;
    ///
    /// let mut caps = CapabilitySet::new();
    /// caps.grant(Capability::stdout());
    /// let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
    ///
    /// enforcer.check_stdout().unwrap();
    /// let _ = enforcer.check_stderr(); // denied
    ///
    /// let log = enforcer.audit_log();
    /// assert_eq!(log.denied_count(), 1);
    /// ```
    pub fn audit_log(&self) -> &AuditLog {
        &self.audit_log
    }

    /// Get the granted capabilities.
    ///
    /// Returns the full [`CapabilitySet`] that was granted when this enforcer
    /// was created.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::capability::{CapabilityEnforcer, CapabilitySet, Capability};
    /// use uuid::Uuid;
    ///
    /// let mut caps = CapabilitySet::new();
    /// caps.grant(Capability::stdout());
    /// caps.grant(Capability::stderr());
    /// let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
    ///
    /// assert!(enforcer.granted().has(&Capability::stdout()));
    /// assert!(!enforcer.granted().has(&Capability::secure_random()));
    /// ```
    pub fn granted(&self) -> &CapabilitySet {
        &self.granted
    }

    /// Revoke a previously granted capability at runtime.
    ///
    /// After revocation, any check for this capability will be denied.
    /// The revocation is recorded in the audit log.
    pub fn revoke_capability(&mut self, cap: &Capability) {
        self.granted.revoke(cap);
        self.audit_log.record_revoked(cap.clone());
    }

    /// Check a capability using hierarchical subsumption.
    ///
    /// Unlike [`check()`](Self::check), this method uses the subsumes
    /// relationship. For example, a granted `filesystem_read("/data")`
    /// will satisfy a check for `filesystem_read("/data/subdir")`.
    pub fn check_hierarchical(&self, required: &Capability) -> Result<()> {
        if self.granted.satisfies(required) {
            self.audit_log.record_used(required.clone(), None);
            Ok(())
        } else {
            self.audit_log.record_denied(required.clone(), None);
            Err(Error::CapabilityDenied(required.clone()))
        }
    }

    /// Check that all given capabilities are granted.
    ///
    /// Returns `Ok(())` if every capability passes, or the first denial error.
    /// All capabilities are checked and audited regardless of early failures.
    pub fn check_all(&self, capabilities: &[Capability]) -> Result<()> {
        let mut first_error = None;
        for cap in capabilities {
            if let Err(e) = self.check(cap) {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Check that at least one of the given capabilities is granted.
    ///
    /// Returns `Ok(())` if any capability passes, or the last denial error.
    pub fn check_any(&self, capabilities: &[Capability]) -> Result<()> {
        if capabilities.is_empty() {
            return Ok(());
        }
        let mut last_error = None;
        for cap in capabilities {
            match self.check(cap) {
                Ok(()) => return Ok(()),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.unwrap())
    }

    /// Get the list of directories that should be preopened for filesystem access.
    /// Returns pairs of (host_path, guest_path) where guest_path is the path
    /// visible inside the sandbox.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::capability::{CapabilityEnforcer, CapabilitySet, Capability};
    /// use uuid::Uuid;
    /// use std::path::PathBuf;
    ///
    /// let mut caps = CapabilitySet::new();
    /// caps.grant(Capability::filesystem_read("/data"));
    /// let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
    ///
    /// let preopens = enforcer.filesystem_preopens();
    /// assert_eq!(preopens.len(), 1);
    /// assert_eq!(preopens[0].0, PathBuf::from("/data"));
    /// assert_eq!(preopens[0].1, "/data");
    /// ```
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

    /// Reject paths containing traversal components (`..`) and symlinks
    /// that escape their parent directory.
    fn validate_path(path: &Path) -> Result<()> {
        for component in path.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(Error::FilesystemAccessDenied { path: path.to_path_buf() });
            }
        }

        // If the path exists on the host, verify that symlink resolution
        // doesn't escape the path's own parent directory. This catches:
        //   /data/link -> /etc/passwd  (canonical /etc/passwd not under /data)
        if path.exists() {
            if let Ok(canonical) = path.canonicalize() {
                if let Some(parent) = path.parent() {
                    if let Ok(canonical_parent) = parent.canonicalize() {
                        if !canonical.starts_with(&canonical_parent) {
                            tracing::warn!(
                                path = %path.display(),
                                resolved = %canonical.display(),
                                parent = %canonical_parent.display(),
                                "Symlink escape detected: resolved path outside parent directory"
                            );
                            return Err(Error::FilesystemAccessDenied { path: path.to_path_buf() });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a path contains any symlink components.
    ///
    /// Returns `true` if any component in the path is a symbolic link.
    /// Useful for security auditing.
    pub fn path_contains_symlink(path: &Path) -> bool {
        let mut current = std::path::PathBuf::new();
        for component in path.components() {
            current.push(component);
            if current.is_symlink() {
                return true;
            }
        }
        false
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

    #[test]
    fn test_path_traversal_rejected_read() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::filesystem_read("/data"));
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_fs_read(Path::new("/data/../etc/passwd")).is_err());
        assert!(enforcer.check_fs_read(Path::new("/data/subdir/../../etc")).is_err());
        assert!(enforcer.check_fs_read(Path::new("/data/safe/file.txt")).is_ok());
    }

    #[test]
    fn test_path_traversal_rejected_write() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::filesystem_write("/tmp"));
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_fs_write(Path::new("/tmp/../root/.ssh")).is_err());
        assert!(enforcer.check_fs_write(Path::new("/tmp/ok.txt")).is_ok());
    }

    #[test]
    fn test_http_wildcard_subdomain_security() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::http_client(vec!["*.example.com"]));
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        // Valid subdomains should be allowed
        assert!(enforcer.check_http("api.example.com").is_ok());
        assert!(enforcer.check_http("foo.example.com").is_ok());
        assert!(enforcer.check_http("deep.sub.example.com").is_ok());

        // Base domain should be denied (wildcard requires a subdomain)
        assert!(enforcer.check_http("example.com").is_err());

        // Suffix spoofing must be rejected
        assert!(enforcer.check_http("evil-example.com").is_err());
        assert!(enforcer.check_http("notexample.com").is_err());

        // Unrelated domains denied
        assert!(enforcer.check_http("other.com").is_err());
    }

    #[test]
    fn test_http_exact_match() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::http_client(vec!["api.example.com"]));
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        assert!(enforcer.check_http("api.example.com").is_ok());
        assert!(enforcer.check_http("other.example.com").is_err());
        assert!(enforcer.check_http("api.example.com.evil.com").is_err());
    }

    #[test]
    fn test_validate_path_rejects_traversal() {
        let result = CapabilityEnforcer::validate_path(Path::new("/data/../etc/passwd"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_accepts_normal() {
        let result = CapabilityEnforcer::validate_path(Path::new("/data/subdir/file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_accepts_relative() {
        let result = CapabilityEnforcer::validate_path(Path::new("data/file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_contains_symlink_no_symlink() {
        // A path that doesn't exist has no symlinks
        let result =
            CapabilityEnforcer::path_contains_symlink(Path::new("/nonexistent/path/abc123"));
        assert!(!result);
    }

    #[test]
    fn test_path_contains_symlink_with_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::write(&target, "content").unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();

        // The link itself should be detected as containing a symlink
        assert!(CapabilityEnforcer::path_contains_symlink(&link));
    }

    #[test]
    fn test_fs_read_with_symlink_inside_allowed() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("data");
        std::fs::create_dir(&subdir).unwrap();
        let target = subdir.join("real_file.txt");
        std::fs::write(&target, "content").unwrap();
        let link = subdir.join("link_file.txt");
        symlink(&target, &link).unwrap();

        let mut caps = CapabilitySet::new();
        caps.grant(Capability::filesystem_read(dir.path()));

        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        // Symlink within allowed dir should be OK
        assert!(enforcer.check_fs_read(&link).is_ok());
    }

    #[test]
    fn test_check_all_succeeds() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());
        caps.grant(Capability::stderr());
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        let result = enforcer.check_all(&[Capability::stdout(), Capability::stderr()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_all_fails_on_missing() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        let result = enforcer.check_all(&[Capability::stdout(), Capability::stderr()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_any_succeeds() {
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stderr());
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        let result = enforcer.check_any(&[Capability::stdout(), Capability::stderr()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_any_fails_when_none_granted() {
        let caps = CapabilitySet::new();
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());

        let result = enforcer.check_any(&[Capability::stdout(), Capability::stderr()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_any_empty_succeeds() {
        let caps = CapabilitySet::new();
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
        assert!(enforcer.check_any(&[]).is_ok());
    }
}
