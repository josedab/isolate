//! Capability bridge between Isolate's capability system and WASI Preview2 interfaces.
//!
//! Maps Isolate `Capability` types to WASI Preview2 `RequiredCapability` types,
//! enabling automatic validation that a sandbox configuration grants all capabilities
//! required by a WASM component's imports.
//!
//! ```rust,ignore
//! use isolate_core::wasi2::capability_bridge::CapabilityBridge;
//! use isolate_core::capability::Capability;
//!
//! let granted = vec![Capability::stdout(), Capability::stderr()];
//! let required_interfaces = vec!["wasi:cli/stdout", "wasi:cli/stderr"];
//!
//! let bridge = CapabilityBridge::new();
//! let result = bridge.validate(&granted, &required_interfaces);
//! assert!(result.is_satisfied());
//! ```

use super::runtime::RequiredCapability;
use crate::capability::{
    Capability, EnvironmentCapability, FilesystemCapability, NetworkCapability, RandomCapability,
    StdioCapability, TimeCapability,
};
use std::collections::HashSet;

/// Maps Isolate capabilities to WASI Preview2 required capabilities.
pub struct CapabilityBridge;

/// Result of capability validation.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Capabilities that are satisfied.
    pub satisfied: Vec<RequiredCapability>,
    /// Capabilities that are missing.
    pub missing: Vec<RequiredCapability>,
    /// Extra capabilities granted but not required.
    pub unused: Vec<String>,
}

impl ValidationResult {
    /// Returns true if all required capabilities are satisfied.
    pub fn is_satisfied(&self) -> bool {
        self.missing.is_empty()
    }

    /// Returns a human-readable summary.
    pub fn summary(&self) -> String {
        if self.is_satisfied() {
            format!(
                "All {} required capabilities satisfied ({} unused grants)",
                self.satisfied.len(),
                self.unused.len()
            )
        } else {
            format!(
                "{} missing capabilities: {}",
                self.missing.len(),
                self.missing.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

impl CapabilityBridge {
    pub fn new() -> Self {
        Self
    }

    /// Convert an Isolate `Capability` to a set of WASI Preview2 required capabilities.
    pub fn to_required(&self, cap: &Capability) -> Vec<RequiredCapability> {
        match cap {
            Capability::Stdio(StdioCapability::Stdout) => vec![RequiredCapability::Stdout],
            Capability::Stdio(StdioCapability::Stderr) => vec![RequiredCapability::Stderr],
            Capability::Stdio(StdioCapability::Stdin) => vec![RequiredCapability::Stdin],
            Capability::Filesystem(FilesystemCapability::ReadOnly(_)) => {
                vec![RequiredCapability::FilesystemRead]
            }
            Capability::Filesystem(FilesystemCapability::ReadWrite(_)) => {
                vec![RequiredCapability::FilesystemRead, RequiredCapability::FilesystemWrite]
            }
            Capability::Filesystem(FilesystemCapability::TempDir) => {
                vec![RequiredCapability::FilesystemRead, RequiredCapability::FilesystemWrite]
            }
            Capability::Network(NetworkCapability::HttpClient(_)) => {
                vec![RequiredCapability::HttpClient]
            }
            Capability::Network(NetworkCapability::TcpConnect(_)) => {
                vec![RequiredCapability::NetworkOutbound]
            }
            Capability::Network(NetworkCapability::TcpListen(_)) => {
                vec![RequiredCapability::NetworkInbound]
            }
            Capability::Network(NetworkCapability::DnsResolve) => {
                vec![RequiredCapability::NetworkOutbound]
            }
            Capability::Time(TimeCapability::SystemClock)
            | Capability::Time(TimeCapability::MonotonicClock)
            | Capability::Time(TimeCapability::Timers) => vec![RequiredCapability::Clock],
            Capability::Random(RandomCapability::Secure)
            | Capability::Random(RandomCapability::Seeded(_)) => vec![RequiredCapability::Random],
            Capability::Environment(EnvironmentCapability::ReadVar(_))
            | Capability::Environment(EnvironmentCapability::ReadAll)
            | Capability::Environment(EnvironmentCapability::Args) => {
                vec![RequiredCapability::EnvironmentVars]
            }
            Capability::HostFunction(_) => vec![],
        }
    }

    /// Convert a set of Isolate capabilities to the full set of required capabilities they cover.
    pub fn granted_set(&self, capabilities: &[Capability]) -> HashSet<RequiredCapability> {
        capabilities.iter().flat_map(|c| self.to_required(c)).collect()
    }

    /// Validate that granted capabilities satisfy all WASI interface requirements.
    pub fn validate(&self, granted: &[Capability], interface_names: &[&str]) -> ValidationResult {
        let granted_set = self.granted_set(granted);
        let required = super::runtime::infer_capabilities(interface_names);
        let required_set: HashSet<RequiredCapability> = required.iter().cloned().collect();

        let satisfied: Vec<RequiredCapability> =
            required_set.intersection(&granted_set).cloned().collect();

        let missing: Vec<RequiredCapability> =
            required_set.difference(&granted_set).cloned().collect();

        let required_strings: HashSet<String> =
            required_set.iter().map(|c| c.to_string()).collect();
        let unused: Vec<String> = granted_set
            .iter()
            .filter(|c| !required_strings.contains(&c.to_string()))
            .map(|c| c.to_string())
            .collect();

        ValidationResult { satisfied, missing, unused }
    }

    /// Suggest minimal capabilities needed for a set of WASI interfaces.
    pub fn suggest_capabilities(&self, interface_names: &[&str]) -> Vec<Capability> {
        let required = super::runtime::infer_capabilities(interface_names);
        let mut suggestions = Vec::new();

        for req in &required {
            let cap = match req {
                RequiredCapability::Stdout => Capability::stdout(),
                RequiredCapability::Stderr => Capability::stderr(),
                RequiredCapability::Stdin => Capability::stdin(),
                RequiredCapability::FilesystemRead => Capability::filesystem_read("/"),
                RequiredCapability::FilesystemWrite => Capability::filesystem_write("/tmp"),
                RequiredCapability::NetworkOutbound | RequiredCapability::HttpClient => {
                    Capability::http_client(vec!["*".to_string()])
                }
                RequiredCapability::NetworkInbound => continue,
                RequiredCapability::EnvironmentVars => Capability::env_all(),
                RequiredCapability::Clock => Capability::system_clock(),
                RequiredCapability::Random => Capability::secure_random(),
            };
            suggestions.push(cap);
        }

        suggestions
    }
}

impl Default for CapabilityBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdio_mapping() {
        let bridge = CapabilityBridge::new();
        let caps = bridge.to_required(&Capability::stdout());
        assert_eq!(caps, vec![RequiredCapability::Stdout]);

        let caps = bridge.to_required(&Capability::stderr());
        assert_eq!(caps, vec![RequiredCapability::Stderr]);
    }

    #[test]
    fn test_filesystem_mapping() {
        let bridge = CapabilityBridge::new();
        let caps = bridge.to_required(&Capability::filesystem_read("/data"));
        assert_eq!(caps, vec![RequiredCapability::FilesystemRead]);

        let caps = bridge.to_required(&Capability::filesystem_write("/data"));
        assert!(caps.contains(&RequiredCapability::FilesystemRead));
        assert!(caps.contains(&RequiredCapability::FilesystemWrite));
    }

    #[test]
    fn test_network_mapping() {
        let bridge = CapabilityBridge::new();
        let caps = bridge.to_required(&Capability::http_client(vec!["example.com".to_string()]));
        assert_eq!(caps, vec![RequiredCapability::HttpClient]);
    }

    #[test]
    fn test_validate_satisfied() {
        let bridge = CapabilityBridge::new();
        let granted = vec![Capability::stdout(), Capability::stderr()];
        let interfaces = vec!["wasi:cli/stdout", "wasi:cli/stderr"];

        let result = bridge.validate(&granted, &interfaces);
        assert!(result.is_satisfied());
        assert!(result.missing.is_empty());
    }

    #[test]
    fn test_validate_missing() {
        let bridge = CapabilityBridge::new();
        let granted = vec![Capability::stdout()];
        let interfaces = vec!["wasi:cli/stdout", "wasi:filesystem/read"];

        let result = bridge.validate(&granted, &interfaces);
        assert!(!result.is_satisfied());
        assert!(result.missing.contains(&RequiredCapability::FilesystemRead));
    }

    #[test]
    fn test_validate_unused() {
        let bridge = CapabilityBridge::new();
        let granted = vec![Capability::stdout(), Capability::stderr(), Capability::secure_random()];
        let interfaces = vec!["wasi:cli/stdout"];

        let result = bridge.validate(&granted, &interfaces);
        assert!(result.is_satisfied());
        assert!(!result.unused.is_empty());
    }

    #[test]
    fn test_suggest_capabilities() {
        let bridge = CapabilityBridge::new();
        let suggestions =
            bridge.suggest_capabilities(&["wasi:cli/stdout", "wasi:clocks/wall-clock"]);
        assert!(!suggestions.is_empty());
    }

    #[test]
    fn test_granted_set() {
        let bridge = CapabilityBridge::new();
        let set = bridge.granted_set(&[
            Capability::stdout(),
            Capability::stderr(),
            Capability::system_clock(),
        ]);
        assert!(set.contains(&RequiredCapability::Stdout));
        assert!(set.contains(&RequiredCapability::Stderr));
        assert!(set.contains(&RequiredCapability::Clock));
        assert!(!set.contains(&RequiredCapability::Random));
    }

    #[test]
    fn test_summary_satisfied() {
        let result = ValidationResult {
            satisfied: vec![RequiredCapability::Stdout],
            missing: vec![],
            unused: vec![],
        };
        assert!(result.summary().contains("satisfied"));
    }

    #[test]
    fn test_summary_missing() {
        let result = ValidationResult {
            satisfied: vec![],
            missing: vec![RequiredCapability::FilesystemRead],
            unused: vec![],
        };
        assert!(result.summary().contains("missing"));
    }

    #[test]
    fn test_default_bridge() {
        let bridge = CapabilityBridge::default();
        let caps = bridge.to_required(&Capability::stdout());
        assert_eq!(caps, vec![RequiredCapability::Stdout]);
    }
}
