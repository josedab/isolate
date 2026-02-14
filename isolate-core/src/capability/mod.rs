//! Capability-based security system.
//!
//! Isolate uses a capability-based security model where all permissions must be
//! explicitly granted. By default, sandboxes have no capabilities (principle of
//! least privilege).
//!
//! # Available Capabilities
//!
//! | Category | Capability | Description |
//! |----------|------------|-------------|
//! | **I/O** | `Capability::stdout()` | Write to standard output |
//! | | `Capability::stderr()` | Write to standard error |
//! | | `Capability::stdin()` | Read from standard input |
//! | **Filesystem** | `Capability::filesystem_read(path)` | Read files under path |
//! | | `Capability::filesystem_write(path)` | Read/write files under path |
//! | | `Capability::temp_dir()` | Access temporary directory |
//! | **Network** | `Capability::http_client(hosts)` | HTTP requests to allowed hosts |
//! | | `Capability::tcp_connect(addrs)` | TCP connections to addresses |
//! | | `Capability::tcp_listen(port)` | Listen on TCP port |
//! | | `Capability::dns_resolve()` | DNS resolution |
//! | **Time** | `Capability::system_clock()` | Read system time |
//! | | `Capability::monotonic_clock()` | Read monotonic time |
//! | | `Capability::timers()` | Create timers/sleeps |
//! | **Random** | `Capability::secure_random()` | Cryptographic random |
//! | | `Capability::seeded_random(seed)` | Deterministic random |
//! | **Environment** | `Capability::env_var(name)` | Read specific env var |
//! | | `Capability::env_all()` | Read all env vars |
//! | | `Capability::args()` | Read command-line args |
//!
//! # Basic Example
//!
//! ```rust
//! use isolate_core::capability::{Capability, CapabilitySet};
//!
//! let mut caps = CapabilitySet::new();
//!
//! // Grant specific capabilities
//! caps.grant(Capability::stdout());
//! caps.grant(Capability::filesystem_read("/data"));
//! caps.grant(Capability::http_client(vec!["api.example.com"]));
//!
//! // Check if capability is granted
//! assert!(caps.has(&Capability::stdout()));
//! assert!(!caps.has(&Capability::stdin()));  // Not granted
//! ```
//!
//! # Using with SandboxConfig
//!
//! ```no_run
//! use isolate_core::{SandboxConfig, capability::Capability};
//!
//! # fn example() -> isolate_core::Result<()> {
//! let config = SandboxConfig::builder()
//!     .module(&[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])?
//!     // Grant output capabilities
//!     .capability(Capability::stdout())
//!     .capability(Capability::stderr())
//!     // Grant read access to data directory
//!     .capability(Capability::filesystem_read("/app/data"))
//!     // Allow HTTP to specific API
//!     .capability(Capability::http_client(vec!["api.example.com", "*.trusted.io"]))
//!     // Grant time access (needed for timeouts, rate limiting)
//!     .capability(Capability::system_clock())
//!     .build()?;
//! # Ok(())
//! # }
//! ```

mod audit;
pub mod designer;
mod enforcer;
pub mod policy_engine;
mod types;

pub use audit::{AuditEvent, AuditLog};
pub use enforcer::CapabilityEnforcer;
pub use types::*;

use std::collections::HashSet;

/// A set of granted capabilities.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    capabilities: HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set (default deny).
    pub fn new() -> Self {
        Self::default()
    }

    /// Grant a capability.
    pub fn grant(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    /// Revoke a capability.
    pub fn revoke(&mut self, cap: &Capability) {
        self.capabilities.remove(cap);
    }

    /// Check if a capability is granted.
    pub fn has(&self, cap: &Capability) -> bool {
        self.capabilities.contains(cap)
    }

    /// Check if any capability of the same category (variant) is granted.
    ///
    /// For example, `has_category(Capability::stdout())` returns true if any
    /// `Stdio` capability is granted (stdin, stdout, or stderr).
    pub fn has_category(&self, cap: &Capability) -> bool {
        self.capabilities.iter().any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap))
    }

    /// Check if any capability matching the predicate is granted.
    pub fn has_any<F>(&self, predicate: F) -> bool
    where
        F: Fn(&Capability) -> bool,
    {
        self.capabilities.iter().any(predicate)
    }

    /// Get all granted capabilities.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter()
    }

    /// Check if the set is empty (no capabilities granted).
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }

    /// Get the number of granted capabilities.
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Merge another capability set into this one.
    pub fn merge(&mut self, other: &CapabilitySet) {
        for cap in &other.capabilities {
            self.capabilities.insert(cap.clone());
        }
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self { capabilities: iter.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set_default_empty() {
        let caps = CapabilitySet::new();
        assert!(caps.is_empty());
        assert!(!caps.has(&Capability::stdout()));
    }

    #[test]
    fn test_capability_set_grant_revoke() {
        let mut caps = CapabilitySet::new();

        caps.grant(Capability::stdout());
        assert!(caps.has(&Capability::stdout()));

        caps.revoke(&Capability::stdout());
        assert!(!caps.has(&Capability::stdout()));
    }

    #[test]
    fn test_capability_set_from_iterator() {
        let caps: CapabilitySet =
            vec![Capability::stdout(), Capability::stderr()].into_iter().collect();

        assert_eq!(caps.len(), 2);
        assert!(caps.has(&Capability::stdout()));
        assert!(caps.has(&Capability::stderr()));
    }

    #[test]
    fn test_capability_set_merge() {
        let mut caps1 = CapabilitySet::new();
        caps1.grant(Capability::stdout());

        let mut caps2 = CapabilitySet::new();
        caps2.grant(Capability::stderr());

        caps1.merge(&caps2);

        assert!(caps1.has(&Capability::stdout()));
        assert!(caps1.has(&Capability::stderr()));
    }
}
