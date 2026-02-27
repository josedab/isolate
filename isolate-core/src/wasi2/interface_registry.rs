//! WASI Preview 2 interface registry and capability mapping.
//!
//! Maps Isolate capabilities to standardized WASI Preview 2 interfaces,
//! enabling type-safe interface negotiation between host and component.

use crate::capability::{Capability, CapabilitySet};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A WASI Preview 2 world definition describing required/optional interfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldDefinition {
    /// World name (e.g., "wasi:cli/command@0.2.0").
    pub name: String,
    /// Interfaces the host must provide (imports).
    pub imports: Vec<InterfaceBinding>,
    /// Interfaces the component exports.
    pub exports: Vec<InterfaceBinding>,
}

/// Binding between a WASI interface and an Isolate capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceBinding {
    /// Fully-qualified interface name.
    pub interface: String,
    /// Required Isolate capability (if any).
    pub required_capability: Option<CapabilityRef>,
    /// Whether this binding is optional.
    pub optional: bool,
}

/// Reference to an Isolate capability kind for policy checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityRef {
    Stdout,
    Stderr,
    Stdin,
    FilesystemRead,
    FilesystemWrite,
    HttpClient,
    DnsResolve,
    SystemClock,
    MonotonicClock,
    SecureRandom,
    EnvironmentVars,
}

/// Registry of known WASI P2 interfaces and their capability requirements.
pub struct InterfaceRegistry {
    worlds: HashMap<String, WorldDefinition>,
    interface_caps: HashMap<String, CapabilityRef>,
}

impl InterfaceRegistry {
    /// Create a registry pre-populated with standard WASI interfaces.
    pub fn new() -> Self {
        let mut registry = Self { worlds: HashMap::new(), interface_caps: HashMap::new() };
        registry.register_standard_interfaces();
        registry
    }

    fn register_standard_interfaces(&mut self) {
        // Map standard WASI P2 interfaces to capability requirements
        let mappings = [
            ("wasi:cli/stdout@0.2.0", CapabilityRef::Stdout),
            ("wasi:cli/stderr@0.2.0", CapabilityRef::Stderr),
            ("wasi:cli/stdin@0.2.0", CapabilityRef::Stdin),
            ("wasi:filesystem/types@0.2.0", CapabilityRef::FilesystemRead),
            ("wasi:filesystem/preopens@0.2.0", CapabilityRef::FilesystemRead),
            ("wasi:sockets/tcp@0.2.0", CapabilityRef::HttpClient),
            ("wasi:sockets/udp@0.2.0", CapabilityRef::HttpClient),
            ("wasi:sockets/ip-name-lookup@0.2.0", CapabilityRef::DnsResolve),
            ("wasi:http/outgoing-handler@0.2.0", CapabilityRef::HttpClient),
            ("wasi:clocks/wall-clock@0.2.0", CapabilityRef::SystemClock),
            ("wasi:clocks/monotonic-clock@0.2.0", CapabilityRef::MonotonicClock),
            ("wasi:random/random@0.2.0", CapabilityRef::SecureRandom),
            ("wasi:cli/environment@0.2.0", CapabilityRef::EnvironmentVars),
        ];
        for (iface, cap) in mappings {
            self.interface_caps.insert(iface.to_string(), cap);
        }

        // Register the standard CLI command world
        self.worlds.insert(
            "wasi:cli/command@0.2.0".to_string(),
            WorldDefinition {
                name: "wasi:cli/command@0.2.0".to_string(),
                imports: vec![
                    InterfaceBinding {
                        interface: "wasi:cli/stdout@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::Stdout),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:cli/stderr@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::Stderr),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:cli/stdin@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::Stdin),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:clocks/wall-clock@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::SystemClock),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:clocks/monotonic-clock@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::MonotonicClock),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:random/random@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::SecureRandom),
                        optional: true,
                    },
                    InterfaceBinding {
                        interface: "wasi:filesystem/types@0.2.0".to_string(),
                        required_capability: Some(CapabilityRef::FilesystemRead),
                        optional: true,
                    },
                ],
                exports: vec![InterfaceBinding {
                    interface: "wasi:cli/run@0.2.0".to_string(),
                    required_capability: None,
                    optional: false,
                }],
            },
        );

        // Register the HTTP proxy world
        self.worlds.insert(
            "wasi:http/proxy@0.2.0".to_string(),
            WorldDefinition {
                name: "wasi:http/proxy@0.2.0".to_string(),
                imports: vec![InterfaceBinding {
                    interface: "wasi:http/outgoing-handler@0.2.0".to_string(),
                    required_capability: Some(CapabilityRef::HttpClient),
                    optional: true,
                }],
                exports: vec![InterfaceBinding {
                    interface: "wasi:http/incoming-handler@0.2.0".to_string(),
                    required_capability: None,
                    optional: false,
                }],
            },
        );
    }

    /// Register a custom world definition.
    pub fn register_world(&mut self, world: WorldDefinition) {
        self.worlds.insert(world.name.clone(), world);
    }

    /// Register a custom interface-to-capability mapping.
    pub fn register_interface_capability(
        &mut self,
        interface: impl Into<String>,
        cap: CapabilityRef,
    ) {
        self.interface_caps.insert(interface.into(), cap);
    }

    /// Get the capability required for a given interface.
    pub fn required_capability(&self, interface: &str) -> Option<&CapabilityRef> {
        self.interface_caps.get(interface)
    }

    /// Get a world definition by name.
    pub fn get_world(&self, name: &str) -> Option<&WorldDefinition> {
        self.worlds.get(name)
    }

    /// Check which interfaces a capability set satisfies.
    pub fn satisfied_interfaces(&self, caps: &CapabilitySet) -> Vec<String> {
        self.interface_caps
            .iter()
            .filter(|(_, cap_ref)| Self::capability_set_satisfies(caps, cap_ref))
            .map(|(iface, _)| iface.clone())
            .collect()
    }

    /// Check which interfaces are missing for a given world.
    pub fn missing_capabilities(
        &self,
        world_name: &str,
        caps: &CapabilitySet,
    ) -> Vec<InterfaceBinding> {
        let Some(world) = self.worlds.get(world_name) else {
            return Vec::new();
        };
        world
            .imports
            .iter()
            .filter(|binding| {
                if binding.optional {
                    return false;
                }
                match &binding.required_capability {
                    Some(cap_ref) => !Self::capability_set_satisfies(caps, cap_ref),
                    None => false,
                }
            })
            .cloned()
            .collect()
    }

    fn capability_set_satisfies(caps: &CapabilitySet, cap_ref: &CapabilityRef) -> bool {
        caps.iter().any(|cap| match (cap, cap_ref) {
            (
                Capability::Stdio(crate::capability::StdioCapability::Stdout),
                CapabilityRef::Stdout,
            ) => true,
            (
                Capability::Stdio(crate::capability::StdioCapability::Stderr),
                CapabilityRef::Stderr,
            ) => true,
            (
                Capability::Stdio(crate::capability::StdioCapability::Stdin),
                CapabilityRef::Stdin,
            ) => true,
            (
                Capability::Filesystem(crate::capability::FilesystemCapability::ReadOnly(_)),
                CapabilityRef::FilesystemRead,
            ) => true,
            (
                Capability::Filesystem(crate::capability::FilesystemCapability::ReadWrite(_)),
                CapabilityRef::FilesystemRead | CapabilityRef::FilesystemWrite,
            ) => true,
            (
                Capability::Network(crate::capability::NetworkCapability::HttpClient(_)),
                CapabilityRef::HttpClient,
            ) => true,
            (
                Capability::Network(crate::capability::NetworkCapability::DnsResolve),
                CapabilityRef::DnsResolve,
            ) => true,
            (
                Capability::Time(crate::capability::TimeCapability::SystemClock),
                CapabilityRef::SystemClock,
            ) => true,
            (
                Capability::Time(crate::capability::TimeCapability::MonotonicClock),
                CapabilityRef::MonotonicClock,
            ) => true,
            (
                Capability::Random(crate::capability::RandomCapability::Secure),
                CapabilityRef::SecureRandom,
            ) => true,
            (Capability::Environment(_), CapabilityRef::EnvironmentVars) => true,
            _ => false,
        })
    }

    /// List all registered worlds.
    pub fn list_worlds(&self) -> Vec<&str> {
        self.worlds.keys().map(|s| s.as_str()).collect()
    }

    /// List all known interfaces.
    pub fn list_interfaces(&self) -> Vec<&str> {
        self.interface_caps.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for InterfaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::*;

    #[test]
    fn test_registry_default_interfaces() {
        let registry = InterfaceRegistry::new();
        assert!(registry.list_interfaces().len() >= 10);
        assert!(registry.list_worlds().contains(&"wasi:cli/command@0.2.0"));
        assert!(registry.list_worlds().contains(&"wasi:http/proxy@0.2.0"));
    }

    #[test]
    fn test_satisfied_interfaces() {
        let registry = InterfaceRegistry::new();
        let mut caps = CapabilitySet::new();
        caps.grant(Capability::stdout());
        caps.grant(Capability::stderr());
        caps.grant(Capability::system_clock());

        let satisfied = registry.satisfied_interfaces(&caps);
        assert!(satisfied.contains(&"wasi:cli/stdout@0.2.0".to_string()));
        assert!(satisfied.contains(&"wasi:cli/stderr@0.2.0".to_string()));
        assert!(satisfied.contains(&"wasi:clocks/wall-clock@0.2.0".to_string()));
        assert!(!satisfied.contains(&"wasi:filesystem/types@0.2.0".to_string()));
    }

    #[test]
    fn test_missing_capabilities() {
        let registry = InterfaceRegistry::new();
        let caps = CapabilitySet::new(); // empty

        // All imports in cli/command are optional, so nothing should be missing
        let missing = registry.missing_capabilities("wasi:cli/command@0.2.0", &caps);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_required_capability_lookup() {
        let registry = InterfaceRegistry::new();
        assert_eq!(
            registry.required_capability("wasi:cli/stdout@0.2.0"),
            Some(&CapabilityRef::Stdout)
        );
        assert_eq!(
            registry.required_capability("wasi:http/outgoing-handler@0.2.0"),
            Some(&CapabilityRef::HttpClient)
        );
        assert_eq!(registry.required_capability("unknown:interface"), None);
    }

    #[test]
    fn test_custom_world_registration() {
        let mut registry = InterfaceRegistry::new();
        registry.register_world(WorldDefinition {
            name: "custom:world@1.0.0".to_string(),
            imports: vec![InterfaceBinding {
                interface: "wasi:cli/stdout@0.2.0".to_string(),
                required_capability: Some(CapabilityRef::Stdout),
                optional: false,
            }],
            exports: vec![],
        });
        assert!(registry.get_world("custom:world@1.0.0").is_some());

        let caps = CapabilitySet::new();
        let missing = registry.missing_capabilities("custom:world@1.0.0", &caps);
        assert_eq!(missing.len(), 1);
    }
}
