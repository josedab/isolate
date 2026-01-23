//! WIT (WebAssembly Interface Types) definitions for Isolate capabilities.
//!
//! This module provides interface definitions that map Isolate's capability
//! system to WIT interfaces, enabling components to declare their requirements
//! in a standard way.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WIT interface version.
pub const WIT_VERSION: &str = "0.1.0";

/// WIT package namespace for Isolate.
pub const WIT_NAMESPACE: &str = "isolate";

/// A WIT interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitInterface {
    /// Interface name.
    pub name: String,
    /// Package path (e.g., "isolate:capability/filesystem").
    pub package: String,
    /// Interface version.
    pub version: String,
    /// Type definitions.
    pub types: Vec<WitType>,
    /// Function definitions.
    pub functions: Vec<WitFunction>,
    /// Documentation.
    pub docs: Option<String>,
}

impl WitInterface {
    /// Create a new WIT interface.
    pub fn new(name: impl Into<String>, package: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            package: package.into(),
            version: WIT_VERSION.to_string(),
            types: Vec::new(),
            functions: Vec::new(),
            docs: None,
        }
    }

    /// Add documentation.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Add a type definition.
    pub fn with_type(mut self, ty: WitType) -> Self {
        self.types.push(ty);
        self
    }

    /// Add a function definition.
    pub fn with_function(mut self, func: WitFunction) -> Self {
        self.functions.push(func);
        self
    }

    /// Generate WIT text representation.
    pub fn to_wit(&self) -> String {
        let mut wit = String::new();

        // Package declaration
        wit.push_str(&format!("package {};\n\n", self.package));

        // Interface
        wit.push_str(&format!("interface {} {{\n", self.name));

        // Documentation
        if let Some(docs) = &self.docs {
            for line in docs.lines() {
                wit.push_str(&format!("    /// {}\n", line));
            }
        }

        // Types
        for ty in &self.types {
            wit.push_str(&format!("    {}\n", ty.to_wit()));
        }

        // Functions
        for func in &self.functions {
            wit.push_str(&format!("    {}\n", func.to_wit()));
        }

        wit.push_str("}\n");
        wit
    }
}

/// A WIT type definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitType {
    /// Type name.
    pub name: String,
    /// Type kind.
    pub kind: WitTypeKind,
    /// Documentation.
    pub docs: Option<String>,
}

impl WitType {
    /// Create a new type definition.
    pub fn new(name: impl Into<String>, kind: WitTypeKind) -> Self {
        Self {
            name: name.into(),
            kind,
            docs: None,
        }
    }

    /// Add documentation.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Generate WIT text representation.
    pub fn to_wit(&self) -> String {
        let mut wit = String::new();
        if let Some(docs) = &self.docs {
            for line in docs.lines() {
                wit.push_str(&format!("/// {}\n    ", line));
            }
        }

        match &self.kind {
            WitTypeKind::Record(fields) => {
                wit.push_str(&format!("record {} {{\n", self.name));
                for (name, ty) in fields {
                    wit.push_str(&format!("        {}: {},\n", name, ty));
                }
                wit.push_str("    }");
            }
            WitTypeKind::Variant(cases) => {
                wit.push_str(&format!("variant {} {{\n", self.name));
                for (name, payload) in cases {
                    if let Some(ty) = payload {
                        wit.push_str(&format!("        {}({}),\n", name, ty));
                    } else {
                        wit.push_str(&format!("        {},\n", name));
                    }
                }
                wit.push_str("    }");
            }
            WitTypeKind::Enum(values) => {
                wit.push_str(&format!("enum {} {{\n", self.name));
                for val in values {
                    wit.push_str(&format!("        {},\n", val));
                }
                wit.push_str("    }");
            }
            WitTypeKind::Alias(target) => {
                wit.push_str(&format!("type {} = {}", self.name, target));
            }
            WitTypeKind::Flags(flags) => {
                wit.push_str(&format!("flags {} {{\n", self.name));
                for flag in flags {
                    wit.push_str(&format!("        {},\n", flag));
                }
                wit.push_str("    }");
            }
            WitTypeKind::Resource(methods) => {
                wit.push_str(&format!("resource {} {{\n", self.name));
                for method in methods {
                    wit.push_str(&format!("        {}\n", method.to_wit()));
                }
                wit.push_str("    }");
            }
        }

        wit
    }
}

/// WIT type kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WitTypeKind {
    /// Record type (struct).
    Record(Vec<(String, String)>),
    /// Variant type (enum with payloads).
    Variant(Vec<(String, Option<String>)>),
    /// Simple enum.
    Enum(Vec<String>),
    /// Type alias.
    Alias(String),
    /// Flags type (bitflags).
    Flags(Vec<String>),
    /// Resource type with methods.
    Resource(Vec<WitFunction>),
}

/// A WIT function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitFunction {
    /// Function name.
    pub name: String,
    /// Parameters.
    pub params: Vec<(String, String)>,
    /// Return type.
    pub results: Option<String>,
    /// Is this a method (first param is self)?
    pub is_method: bool,
    /// Is this a static method?
    pub is_static: bool,
    /// Is this a constructor?
    pub is_constructor: bool,
    /// Documentation.
    pub docs: Option<String>,
}

impl WitFunction {
    /// Create a new function definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            results: None,
            is_method: false,
            is_static: false,
            is_constructor: false,
            docs: None,
        }
    }

    /// Add a parameter.
    pub fn with_param(mut self, name: impl Into<String>, ty: impl Into<String>) -> Self {
        self.params.push((name.into(), ty.into()));
        self
    }

    /// Set return type.
    pub fn with_result(mut self, ty: impl Into<String>) -> Self {
        self.results = Some(ty.into());
        self
    }

    /// Mark as method.
    pub fn as_method(mut self) -> Self {
        self.is_method = true;
        self
    }

    /// Mark as static method.
    pub fn as_static(mut self) -> Self {
        self.is_static = true;
        self
    }

    /// Mark as constructor.
    pub fn as_constructor(mut self) -> Self {
        self.is_constructor = true;
        self
    }

    /// Add documentation.
    pub fn with_docs(mut self, docs: impl Into<String>) -> Self {
        self.docs = Some(docs.into());
        self
    }

    /// Generate WIT text representation.
    pub fn to_wit(&self) -> String {
        let mut wit = String::new();

        if let Some(docs) = &self.docs {
            for line in docs.lines() {
                wit.push_str(&format!("/// {}\n        ", line));
            }
        }

        let prefix = if self.is_constructor {
            "constructor"
        } else if self.is_static {
            "[static]"
        } else {
            ""
        };

        if !prefix.is_empty() {
            wit.push_str(prefix);
            wit.push(' ');
        }

        wit.push_str(&self.name);
        wit.push_str(": func(");

        let params: Vec<_> = self
            .params
            .iter()
            .map(|(name, ty)| format!("{}: {}", name, ty))
            .collect();
        wit.push_str(&params.join(", "));

        wit.push(')');

        if let Some(result) = &self.results {
            wit.push_str(&format!(" -> {}", result));
        }

        wit.push(';');
        wit
    }
}

/// Collection of WIT interfaces for Isolate.
pub struct IsolateWitInterfaces {
    interfaces: HashMap<String, WitInterface>,
}

impl IsolateWitInterfaces {
    /// Create the standard Isolate WIT interfaces.
    pub fn new() -> Self {
        let mut interfaces = HashMap::new();

        // Filesystem capability interface
        interfaces.insert(
            "filesystem".to_string(),
            Self::filesystem_interface(),
        );

        // Network capability interface
        interfaces.insert(
            "network".to_string(),
            Self::network_interface(),
        );

        // Environment capability interface
        interfaces.insert(
            "environment".to_string(),
            Self::environment_interface(),
        );

        // Resource limits interface
        interfaces.insert(
            "resources".to_string(),
            Self::resources_interface(),
        );

        Self { interfaces }
    }

    /// Get an interface by name.
    pub fn get(&self, name: &str) -> Option<&WitInterface> {
        self.interfaces.get(name)
    }

    /// Get all interfaces.
    pub fn all(&self) -> impl Iterator<Item = &WitInterface> {
        self.interfaces.values()
    }

    /// Generate all WIT definitions.
    pub fn to_wit(&self) -> String {
        self.interfaces
            .values()
            .map(|i| i.to_wit())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn filesystem_interface() -> WitInterface {
        WitInterface::new("filesystem", "isolate:capability/filesystem")
            .with_docs("Filesystem capability interface for sandboxed WASM components.")
            .with_type(
                WitType::new(
                    "access-mode",
                    WitTypeKind::Enum(vec![
                        "read-only".to_string(),
                        "read-write".to_string(),
                    ]),
                )
                .with_docs("File access mode"),
            )
            .with_type(
                WitType::new(
                    "path-permission",
                    WitTypeKind::Record(vec![
                        ("path".to_string(), "string".to_string()),
                        ("mode".to_string(), "access-mode".to_string()),
                    ]),
                )
                .with_docs("Permission for a specific path"),
            )
            .with_function(
                WitFunction::new("check-read")
                    .with_param("path", "string")
                    .with_result("result<bool, string>")
                    .with_docs("Check if reading from the given path is allowed"),
            )
            .with_function(
                WitFunction::new("check-write")
                    .with_param("path", "string")
                    .with_result("result<bool, string>")
                    .with_docs("Check if writing to the given path is allowed"),
            )
    }

    fn network_interface() -> WitInterface {
        WitInterface::new("network", "isolate:capability/network")
            .with_docs("Network capability interface for sandboxed WASM components.")
            .with_type(
                WitType::new(
                    "network-capability",
                    WitTypeKind::Variant(vec![
                        ("http-client".to_string(), Some("list<string>".to_string())),
                        ("tcp-connect".to_string(), Some("list<string>".to_string())),
                        ("tcp-listen".to_string(), Some("list<u16>".to_string())),
                        ("dns-resolve".to_string(), None),
                    ]),
                )
                .with_docs("Network capability types"),
            )
            .with_function(
                WitFunction::new("check-http")
                    .with_param("host", "string")
                    .with_result("result<bool, string>")
                    .with_docs("Check if HTTP access to the given host is allowed"),
            )
            .with_function(
                WitFunction::new("check-tcp-connect")
                    .with_param("host", "string")
                    .with_param("port", "u16")
                    .with_result("result<bool, string>")
                    .with_docs("Check if TCP connection to the given host:port is allowed"),
            )
    }

    fn environment_interface() -> WitInterface {
        WitInterface::new("environment", "isolate:capability/environment")
            .with_docs("Environment capability interface for sandboxed WASM components.")
            .with_type(
                WitType::new(
                    "env-permission",
                    WitTypeKind::Variant(vec![
                        ("read-var".to_string(), Some("string".to_string())),
                        ("read-all".to_string(), None),
                    ]),
                )
                .with_docs("Environment variable access permission"),
            )
            .with_function(
                WitFunction::new("check-env-var")
                    .with_param("name", "string")
                    .with_result("result<bool, string>")
                    .with_docs("Check if reading the given environment variable is allowed"),
            )
            .with_function(
                WitFunction::new("get-allowed-vars")
                    .with_result("list<string>")
                    .with_docs("Get list of allowed environment variable names"),
            )
    }

    fn resources_interface() -> WitInterface {
        WitInterface::new("resources", "isolate:capability/resources")
            .with_docs("Resource limits interface for sandboxed WASM components.")
            .with_type(
                WitType::new(
                    "resource-limits",
                    WitTypeKind::Record(vec![
                        ("memory-bytes".to_string(), "option<u64>".to_string()),
                        ("fuel".to_string(), "option<u64>".to_string()),
                        ("wall-time-ms".to_string(), "option<u64>".to_string()),
                        ("read-bytes".to_string(), "option<u64>".to_string()),
                        ("write-bytes".to_string(), "option<u64>".to_string()),
                    ]),
                )
                .with_docs("Resource limit configuration"),
            )
            .with_type(
                WitType::new(
                    "resource-usage",
                    WitTypeKind::Record(vec![
                        ("memory-bytes".to_string(), "u64".to_string()),
                        ("fuel-consumed".to_string(), "u64".to_string()),
                        ("wall-time-ms".to_string(), "u64".to_string()),
                        ("bytes-read".to_string(), "u64".to_string()),
                        ("bytes-written".to_string(), "u64".to_string()),
                    ]),
                )
                .with_docs("Current resource usage"),
            )
            .with_function(
                WitFunction::new("get-limits")
                    .with_result("resource-limits")
                    .with_docs("Get the configured resource limits"),
            )
            .with_function(
                WitFunction::new("get-usage")
                    .with_result("resource-usage")
                    .with_docs("Get current resource usage"),
            )
            .with_function(
                WitFunction::new("remaining-fuel")
                    .with_result("option<u64>")
                    .with_docs("Get remaining fuel (if fuel metering is enabled)"),
            )
    }
}

impl Default for IsolateWitInterfaces {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wit_interface_creation() {
        let interface = WitInterface::new("test", "isolate:test/interface")
            .with_docs("Test interface");

        assert_eq!(interface.name, "test");
        assert_eq!(interface.package, "isolate:test/interface");
        assert!(interface.docs.is_some());
    }

    #[test]
    fn test_wit_type_record() {
        let ty = WitType::new(
            "my-record",
            WitTypeKind::Record(vec![
                ("field1".to_string(), "u32".to_string()),
                ("field2".to_string(), "string".to_string()),
            ]),
        );

        let wit = ty.to_wit();
        assert!(wit.contains("record my-record"));
        assert!(wit.contains("field1: u32"));
        assert!(wit.contains("field2: string"));
    }

    #[test]
    fn test_wit_type_enum() {
        let ty = WitType::new(
            "my-enum",
            WitTypeKind::Enum(vec![
                "value1".to_string(),
                "value2".to_string(),
            ]),
        );

        let wit = ty.to_wit();
        assert!(wit.contains("enum my-enum"));
        assert!(wit.contains("value1"));
        assert!(wit.contains("value2"));
    }

    #[test]
    fn test_wit_function() {
        let func = WitFunction::new("my-func")
            .with_param("input", "string")
            .with_result("u32")
            .with_docs("A test function");

        let wit = func.to_wit();
        assert!(wit.contains("my-func"));
        assert!(wit.contains("input: string"));
        assert!(wit.contains("-> u32"));
    }

    #[test]
    fn test_isolate_wit_interfaces() {
        let interfaces = IsolateWitInterfaces::new();

        assert!(interfaces.get("filesystem").is_some());
        assert!(interfaces.get("network").is_some());
        assert!(interfaces.get("environment").is_some());
        assert!(interfaces.get("resources").is_some());
    }

    #[test]
    fn test_wit_generation() {
        let interfaces = IsolateWitInterfaces::new();
        let wit = interfaces.to_wit();

        assert!(wit.contains("package isolate:capability/filesystem"));
        assert!(wit.contains("interface filesystem"));
        assert!(wit.contains("check-read"));
    }
}
