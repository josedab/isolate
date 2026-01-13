//! Dual-mode WASI support (Preview 1 + Preview 2).
//!
//! Provides automatic detection and routing of WASM modules/components
//! to the appropriate WASI implementation, enabling seamless migration
//! from Preview 1 to Preview 2.

use crate::capability::{Capability, CapabilitySet};
use crate::error::{Error, Result};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WASI version detected from a WASM binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WasiVersion {
    /// WASI Preview 1 (classic module).
    Preview1,
    /// WASI Preview 2 (component model).
    Preview2,
    /// Unknown or no WASI.
    Unknown,
}

impl std::fmt::Display for WasiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preview1 => write!(f, "preview1"),
            Self::Preview2 => write!(f, "preview2"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detect the WASI version of a WASM binary.
pub fn detect_wasi_version(bytes: &[u8]) -> WasiVersion {
    if bytes.len() < 8 {
        return WasiVersion::Unknown;
    }

    // Check WASM magic number
    if &bytes[0..4] != b"\0asm" {
        return WasiVersion::Unknown;
    }

    // Check if it's a component (non-standard version field)
    let version = &bytes[4..8];
    if version == [0x01, 0x00, 0x00, 0x00] {
        // Standard WASM module → Preview 1
        WasiVersion::Preview1
    } else {
        // Component model binary → Preview 2
        WasiVersion::Preview2
    }
}

/// A WIT (WebAssembly Interface Type) interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitInterface {
    /// Interface name (e.g., "wasi:filesystem/types@0.2.0").
    pub name: String,
    /// Package name.
    pub package: String,
    /// Version.
    pub version: String,
    /// Functions defined in this interface.
    pub functions: Vec<WitFunction>,
    /// Types defined in this interface.
    pub types: Vec<WitType>,
}

/// A function defined in a WIT interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitFunction {
    /// Function name.
    pub name: String,
    /// Parameters.
    pub params: Vec<WitParam>,
    /// Return type.
    pub result: Option<WitTypeRef>,
    /// Whether this is a static or method function.
    pub kind: WitFunctionKind,
}

/// Kind of WIT function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitFunctionKind {
    /// Freestanding function.
    Freestanding,
    /// Method on a resource.
    Method,
    /// Static function on a resource.
    Static,
    /// Constructor for a resource.
    Constructor,
}

/// A parameter in a WIT function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitParam {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    pub ty: WitTypeRef,
}

/// A type defined in a WIT interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitType {
    /// Type name.
    pub name: String,
    /// Type kind.
    pub kind: WitTypeKind,
}

/// WIT type kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitTypeKind {
    /// Record (struct) type.
    Record(Vec<WitParam>),
    /// Enum type.
    Enum(Vec<String>),
    /// Variant (tagged union) type.
    Variant(Vec<WitVariantCase>),
    /// Flags (bitfield) type.
    Flags(Vec<String>),
    /// Resource type.
    Resource,
    /// Type alias.
    Alias(WitTypeRef),
}

/// A case in a WIT variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitVariantCase {
    /// Case name.
    pub name: String,
    /// Optional payload type.
    pub ty: Option<WitTypeRef>,
}

/// A reference to a WIT type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WitTypeRef {
    /// Primitive types.
    Bool,
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
    F32,
    F64,
    Char,
    String,
    /// List of a type.
    List(Box<WitTypeRef>),
    /// Option of a type.
    Option(Box<WitTypeRef>),
    /// Result type.
    Result {
        ok: Option<Box<WitTypeRef>>,
        err: Option<Box<WitTypeRef>>,
    },
    /// Tuple type.
    Tuple(Vec<WitTypeRef>),
    /// Named type reference.
    Named(String),
}

/// Capability-aware component linker.
///
/// Maps WASI Preview 2 interfaces to the Isolate capability system,
/// ensuring components can only access interfaces they have capabilities for.
#[derive(Debug)]
pub struct CapabilityLinker {
    /// Mapping from WASI interface names to required capabilities.
    interface_capabilities: HashMap<String, Vec<Capability>>,
    /// Granted capabilities for the current sandbox.
    granted: CapabilitySet,
}

impl CapabilityLinker {
    /// Create a new capability linker with default WASI interface mappings.
    pub fn new(capabilities: CapabilitySet) -> Self {
        let mut linker = Self { interface_capabilities: HashMap::new(), granted: capabilities };
        linker.register_default_mappings();
        linker
    }

    /// Register default WASI Preview 2 interface → capability mappings.
    fn register_default_mappings(&mut self) {
        use std::path::PathBuf;

        // wasi:filesystem/* requires filesystem capabilities
        self.interface_capabilities.insert(
            "wasi:filesystem/types".to_string(),
            vec![Capability::Filesystem(crate::capability::FilesystemCapability::ReadOnly(
                PathBuf::from("/"),
            ))],
        );
        self.interface_capabilities.insert(
            "wasi:filesystem/preopens".to_string(),
            vec![Capability::Filesystem(crate::capability::FilesystemCapability::ReadOnly(
                PathBuf::from("/"),
            ))],
        );

        // wasi:sockets/* requires network capabilities
        self.interface_capabilities.insert(
            "wasi:sockets/tcp".to_string(),
            vec![Capability::Network(crate::capability::NetworkCapability::DnsResolve)],
        );
        self.interface_capabilities.insert(
            "wasi:sockets/udp".to_string(),
            vec![Capability::Network(crate::capability::NetworkCapability::DnsResolve)],
        );

        // wasi:http/* requires HTTP capabilities
        self.interface_capabilities.insert(
            "wasi:http/outgoing-handler".to_string(),
            vec![Capability::Network(crate::capability::NetworkCapability::HttpClient(vec![
                "*".to_string()
            ]))],
        );

        // wasi:clocks/* requires time capabilities
        self.interface_capabilities.insert(
            "wasi:clocks/wall-clock".to_string(),
            vec![Capability::Time(crate::capability::TimeCapability::SystemClock)],
        );
        self.interface_capabilities.insert(
            "wasi:clocks/monotonic-clock".to_string(),
            vec![Capability::Time(crate::capability::TimeCapability::MonotonicClock)],
        );

        // wasi:random/* requires random capabilities
        self.interface_capabilities.insert(
            "wasi:random/random".to_string(),
            vec![Capability::Random(crate::capability::RandomCapability::Secure)],
        );

        // wasi:cli/std* requires stdio capabilities
        self.interface_capabilities.insert("wasi:cli/stdin".to_string(), vec![Capability::stdin()]);
        self.interface_capabilities
            .insert("wasi:cli/stdout".to_string(), vec![Capability::stdout()]);
        self.interface_capabilities
            .insert("wasi:cli/stderr".to_string(), vec![Capability::stderr()]);
        self.interface_capabilities.insert(
            "wasi:cli/environment".to_string(),
            vec![Capability::Environment(crate::capability::EnvironmentCapability::ReadAll)],
        );
    }

    /// Register a custom interface → capability mapping.
    pub fn register_mapping(
        &mut self,
        interface: impl Into<String>,
        capabilities: Vec<Capability>,
    ) {
        self.interface_capabilities.insert(interface.into(), capabilities);
    }

    /// Check if a WASI interface is allowed by the granted capabilities.
    ///
    /// For stdio capabilities, exact matching is used (stdout != stdin).
    /// For filesystem/network/time/random, category matching is used
    /// (any filesystem cap satisfies the filesystem interface requirement).
    pub fn is_interface_allowed(&self, interface: &str) -> bool {
        match self.interface_capabilities.get(interface) {
            Some(required) => required.iter().any(|cap| {
                match cap {
                    // Stdio needs exact matching (stdout != stdin)
                    Capability::Stdio(_) => self.granted.has(cap),
                    // Other categories: any capability of the same type suffices
                    _ => self.granted.has_category(cap),
                }
            }),
            None => {
                // Unknown interface: default-deny for safety
                false
            }
        }
    }

    /// Get the list of allowed interfaces.
    pub fn allowed_interfaces(&self) -> Vec<String> {
        self.interface_capabilities
            .keys()
            .filter(|iface| self.is_interface_allowed(iface))
            .cloned()
            .collect()
    }

    /// Get the list of denied interfaces with reasons.
    pub fn denied_interfaces(&self) -> Vec<(String, String)> {
        self.interface_capabilities
            .iter()
            .filter(|(iface, _)| !self.is_interface_allowed(iface))
            .map(|(iface, required)| {
                let caps: Vec<String> = required.iter().map(|c| c.description()).collect();
                (iface.clone(), format!("Requires: {}", caps.join(", ")))
            })
            .collect()
    }

    /// Validate that all required interfaces of a component are allowed.
    pub fn validate_component_imports(&self, required_interfaces: &[String]) -> Result<()> {
        let mut denied = Vec::new();

        for iface in required_interfaces {
            if !self.is_interface_allowed(iface) {
                denied.push(iface.clone());
            }
        }

        if denied.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidCapability(format!(
                "Component requires denied interfaces: {}",
                denied.join(", ")
            )))
        }
    }
}

/// Dual-mode execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualModeConfig {
    /// Preferred WASI version (auto-detect if None).
    pub preferred_version: Option<WasiVersion>,
    /// Whether to allow fallback from Preview 2 to Preview 1.
    pub allow_fallback: bool,
    /// Component model specific settings.
    pub component_settings: ComponentSettings,
}

impl Default for DualModeConfig {
    fn default() -> Self {
        Self {
            preferred_version: None,
            allow_fallback: true,
            component_settings: ComponentSettings::default(),
        }
    }
}

/// Component model specific settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSettings {
    /// Maximum component composition depth.
    pub max_composition_depth: usize,
    /// Enable component caching.
    pub enable_caching: bool,
    /// Maximum component size in bytes.
    pub max_component_size: usize,
}

impl Default for ComponentSettings {
    fn default() -> Self {
        Self {
            max_composition_depth: 10,
            enable_caching: true,
            max_component_size: 100 * 1024 * 1024, // 100 MB
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;

    #[test]
    fn test_detect_wasi_version() {
        // Standard module → Preview 1
        let module = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(detect_wasi_version(&module), WasiVersion::Preview1);

        // Too small → Unknown
        assert_eq!(detect_wasi_version(&[0x00, 0x61]), WasiVersion::Unknown);

        // Invalid magic → Unknown
        let invalid = [0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(detect_wasi_version(&invalid), WasiVersion::Unknown);

        // Component (non-v1 version) → Preview 2
        let component = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        assert_eq!(detect_wasi_version(&component), WasiVersion::Preview2);
    }

    #[test]
    fn test_capability_linker_stdio() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::stdout());
        caps.grant(Capability::stderr());

        let linker = CapabilityLinker::new(caps);

        assert!(linker.is_interface_allowed("wasi:cli/stdout"));
        assert!(linker.is_interface_allowed("wasi:cli/stderr"));
        assert!(!linker.is_interface_allowed("wasi:cli/stdin"));
    }

    #[test]
    fn test_capability_linker_filesystem() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::filesystem_read("/data"));

        let linker = CapabilityLinker::new(caps);

        assert!(linker.is_interface_allowed("wasi:filesystem/types"));
        assert!(!linker.is_interface_allowed("wasi:sockets/tcp"));
    }

    #[test]
    fn test_capability_linker_network() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::dns_resolve());

        let linker = CapabilityLinker::new(caps);

        assert!(linker.is_interface_allowed("wasi:sockets/tcp"));
        assert!(!linker.is_interface_allowed("wasi:filesystem/types"));
    }

    #[test]
    fn test_capability_linker_allowed_interfaces() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::stdout());
        caps.grant(Capability::monotonic_clock());

        let linker = CapabilityLinker::new(caps);

        let allowed = linker.allowed_interfaces();
        assert!(allowed.contains(&"wasi:cli/stdout".to_string()));
        assert!(allowed.contains(&"wasi:clocks/monotonic-clock".to_string()));
        assert!(!allowed.contains(&"wasi:filesystem/types".to_string()));
    }

    #[test]
    fn test_capability_linker_validate_imports() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::stdout());

        let linker = CapabilityLinker::new(caps);

        // Allowed
        let result = linker.validate_component_imports(&["wasi:cli/stdout".to_string()]);
        assert!(result.is_ok());

        // Denied
        let result = linker.validate_component_imports(&[
            "wasi:cli/stdout".to_string(),
            "wasi:sockets/tcp".to_string(),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_capability_linker_unknown_interface_denied() {
        let caps = CapabilitySet::default();
        let linker = CapabilityLinker::new(caps);

        // Unknown interfaces are denied by default
        assert!(!linker.is_interface_allowed("custom:unknown/thing"));
    }

    #[test]
    fn test_dual_mode_config_default() {
        let config = DualModeConfig::default();
        assert!(config.preferred_version.is_none());
        assert!(config.allow_fallback);
        assert!(config.component_settings.enable_caching);
    }

    #[test]
    fn test_wasi_version_display() {
        assert_eq!(WasiVersion::Preview1.to_string(), "preview1");
        assert_eq!(WasiVersion::Preview2.to_string(), "preview2");
        assert_eq!(WasiVersion::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_custom_interface_mapping() {
        let mut caps = CapabilitySet::default();
        caps.grant(Capability::host_function("my-func"));

        let mut linker = CapabilityLinker::new(caps);
        linker.register_mapping(
            "custom:my-package/my-interface",
            vec![Capability::host_function("my-func")],
        );

        assert!(linker.is_interface_allowed("custom:my-package/my-interface"));
    }
}
