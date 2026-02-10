//! Plugin API for grouping host functions into reusable packages.
//!
//! Plugins bundle related host functions with metadata, versioning,
//! and capability requirements into a single registrable unit.

use super::host::HostFunctions;
use super::host_sdk::{HostFnDescriptor, HostFnRegistry};
use crate::error::Result;

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Type alias for a boxed plugin function closure.
type BoxedPluginFn = Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>;

/// A plugin that bundles related host functions.
pub struct HostPlugin {
    /// Plugin metadata.
    pub metadata: PluginMetadata,
    /// Functions provided by this plugin.
    functions: Vec<PluginFunction>,
}

/// Metadata describing a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name (e.g., "database", "http-client").
    pub name: String,
    /// Semantic version (e.g., "1.0.0").
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Author or organization.
    pub author: String,
    /// Capabilities required by this plugin.
    pub required_capabilities: Vec<String>,
}

/// A function within a plugin.
struct PluginFunction {
    name: String,
    description: String,
    func: BoxedPluginFn,
}

impl HostPlugin {
    /// Create a new plugin with the given name and version.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            metadata: PluginMetadata {
                name: name.into(),
                version: version.into(),
                description: String::new(),
                author: String::new(),
                required_capabilities: Vec::new(),
            },
            functions: Vec::new(),
        }
    }

    /// Set the plugin description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.metadata.description = desc.into();
        self
    }

    /// Set the plugin author.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.metadata.author = author.into();
        self
    }

    /// Declare a required capability.
    pub fn requires_capability(mut self, cap_desc: impl Into<String>) -> Self {
        self.metadata.required_capabilities.push(cap_desc.into());
        self
    }

    /// Add a host function to this plugin.
    pub fn function<F>(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        func: F,
    ) -> Self
    where
        F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        self.functions.push(PluginFunction {
            name: name.into(),
            description: description.into(),
            func: Box::new(func),
        });
        self
    }

    /// Get the number of functions in this plugin.
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Get the list of function names in this plugin.
    pub fn function_names(&self) -> Vec<&str> {
        self.functions.iter().map(|f| f.name.as_str()).collect()
    }
}

/// Registry for managing multiple plugins.
pub struct PluginRegistry {
    plugins: Vec<PluginMetadata>,
    host_functions: HostFnRegistry,
}

impl PluginRegistry {
    /// Create a new plugin registry.
    pub fn new() -> Self {
        Self { plugins: Vec::new(), host_functions: HostFnRegistry::new() }
    }

    /// Install a plugin, registering all its functions.
    ///
    /// Function names are prefixed with the plugin name (e.g., "db.query").
    pub fn install(&mut self, plugin: HostPlugin) {
        let prefix = plugin.metadata.name.clone();
        self.plugins.push(plugin.metadata);

        for func in plugin.functions {
            let qualified_name = format!("{}.{}", prefix, func.name);
            self.host_functions
                .register_fn(qualified_name, func.func)
                .with_description(func.description);
        }
    }

    /// Install a plugin without name prefixing.
    pub fn install_unprefixed(&mut self, plugin: HostPlugin) {
        self.plugins.push(plugin.metadata);

        for func in plugin.functions {
            self.host_functions
                .register_fn(func.name, func.func)
                .with_description(func.description);
        }
    }

    /// List installed plugins.
    pub fn installed_plugins(&self) -> &[PluginMetadata] {
        &self.plugins
    }

    /// Get the number of installed plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get all function descriptors.
    pub fn function_descriptors(&self) -> &[HostFnDescriptor] {
        self.host_functions.descriptors()
    }

    /// Build the final host functions from all installed plugins.
    pub fn build(self) -> Arc<HostFunctions> {
        self.host_functions.build()
    }

    /// Generate a JSON catalog of all installed plugins and their functions.
    pub fn catalog_json(&self) -> String {
        let catalog: Vec<serde_json::Value> = self
            .plugins
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "version": p.version,
                    "description": p.description,
                    "author": p.author,
                    "required_capabilities": p.required_capabilities,
                })
            })
            .collect();
        serde_json::to_string_pretty(&catalog).unwrap_or_default()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_creation() {
        let plugin = HostPlugin::new("math", "1.0.0")
            .description("Mathematical operations")
            .author("isolate-team")
            .function("add", "Add two numbers", |args| Ok(args.to_vec()))
            .function("multiply", "Multiply two numbers", |args| Ok(args.to_vec()));

        assert_eq!(plugin.metadata.name, "math");
        assert_eq!(plugin.function_count(), 2);
        assert_eq!(plugin.function_names(), vec!["add", "multiply"]);
    }

    #[test]
    fn test_plugin_registry_install() {
        let mut registry = PluginRegistry::new();

        let plugin = HostPlugin::new("db", "1.0.0")
            .function("query", "Run a query", |_| Ok(b"result".to_vec()))
            .function("insert", "Insert a row", |_| Ok(vec![]));

        registry.install(plugin);

        assert_eq!(registry.plugin_count(), 1);
        // Functions should be prefixed
        let descs = registry.function_descriptors();
        assert_eq!(descs.len(), 2);
        assert!(descs.iter().any(|d| d.name == "db.query"));
        assert!(descs.iter().any(|d| d.name == "db.insert"));
    }

    #[test]
    fn test_plugin_registry_unprefixed() {
        let mut registry = PluginRegistry::new();

        let plugin = HostPlugin::new("math", "1.0.0")
            .function("add", "Add numbers", |_| Ok(vec![]));

        registry.install_unprefixed(plugin);

        let descs = registry.function_descriptors();
        assert!(descs.iter().any(|d| d.name == "add"));
    }

    #[test]
    fn test_plugin_registry_build_and_call() {
        let mut registry = PluginRegistry::new();

        let plugin = HostPlugin::new("echo", "1.0.0")
            .function("back", "Echo input", |args| Ok(args.to_vec()));

        registry.install(plugin);

        let host_fns = registry.build();
        let result = host_fns.call("echo.back", b"hello").unwrap();
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_multiple_plugins() {
        let mut registry = PluginRegistry::new();

        registry.install(
            HostPlugin::new("math", "1.0.0")
                .function("add", "Add", |_| Ok(vec![1]))
        );
        registry.install(
            HostPlugin::new("string", "2.0.0")
                .function("upper", "Uppercase", |_| Ok(vec![2]))
        );

        assert_eq!(registry.plugin_count(), 2);
        let host_fns = registry.build();
        assert!(host_fns.has("math.add"));
        assert!(host_fns.has("string.upper"));
    }

    #[test]
    fn test_plugin_catalog_json() {
        let mut registry = PluginRegistry::new();
        registry.install(
            HostPlugin::new("db", "1.0.0")
                .description("Database access")
                .author("team")
                .requires_capability("filesystem:read")
        );

        let catalog = registry.catalog_json();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&catalog).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "db");
        assert_eq!(parsed[0]["version"], "1.0.0");
    }

    #[test]
    fn test_plugin_with_required_capabilities() {
        let plugin = HostPlugin::new("net", "1.0.0")
            .requires_capability("network:http")
            .requires_capability("network:dns");

        assert_eq!(plugin.metadata.required_capabilities.len(), 2);
    }
}
