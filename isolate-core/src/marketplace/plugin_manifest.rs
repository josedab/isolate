//! Plugin manifest format and OCI-compatible registry federation.
//!
//! Defines the standard manifest format for WASM plugins, a local catalog
//! for installed plugins, and an OCI-compatible registry interface for
//! discovering and distributing sandbox modules.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Plugin manifest following a standardized format.
///
/// This is the `isolate-plugin.json` file that accompanies every published plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Plugin name (unique within registry).
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Author or organization.
    pub author: String,
    /// License identifier (SPDX).
    pub license: Option<String>,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// Repository URL.
    pub repository: Option<String>,
    /// Required capabilities for this plugin.
    pub capabilities: Vec<String>,
    /// Resource requirements.
    pub resources: PluginResources,
    /// Plugin entry points.
    pub entry_points: Vec<EntryPoint>,
    /// Dependencies on other plugins.
    pub dependencies: Vec<PluginDependency>,
    /// Plugin metadata/labels.
    pub metadata: HashMap<String, String>,
    /// Content hash of the WASM module (SHA-256).
    pub content_hash: String,
    /// Size of the WASM module in bytes.
    pub size_bytes: u64,
}

impl PluginManifest {
    /// Validate the manifest for required fields and consistency.
    pub fn validate(&self) -> Vec<ManifestValidationError> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push(ManifestValidationError::MissingField("name".into()));
        }
        if self.version.is_empty() {
            errors.push(ManifestValidationError::MissingField("version".into()));
        }
        if self.content_hash.is_empty() {
            errors.push(ManifestValidationError::MissingField("content_hash".into()));
        }
        if self.entry_points.is_empty() {
            errors.push(ManifestValidationError::MissingField("entry_points".into()));
        }

        // Check for valid semver (basic: major.minor.patch)
        let parts: Vec<&str> = self.version.split('.').collect();
        if parts.len() != 3 || !parts.iter().all(|p| p.parse::<u32>().is_ok()) {
            errors.push(ManifestValidationError::InvalidVersion(self.version.clone()));
        }

        // Check name format (lowercase alphanumeric with hyphens)
        if !self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            errors.push(ManifestValidationError::InvalidName(self.name.clone()));
        }

        // Check for circular dependencies
        if self.dependencies.iter().any(|d| d.name == self.name) {
            errors.push(ManifestValidationError::SelfDependency);
        }

        errors
    }

    /// Create an OCI-compatible reference string.
    pub fn oci_reference(&self, registry: &str) -> String {
        format!("{}/{}:{}", registry, self.name, self.version)
    }
}

/// Resource requirements declared by a plugin.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginResources {
    /// Minimum memory required in bytes.
    pub min_memory: Option<u64>,
    /// Recommended memory in bytes.
    pub recommended_memory: Option<u64>,
    /// Minimum fuel (CPU) required.
    pub min_fuel: Option<u64>,
    /// Recommended timeout.
    pub recommended_timeout_s: Option<u32>,
}

/// An entry point (exported function) in the plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    /// Function name.
    pub name: String,
    /// Description of what this entry point does.
    pub description: Option<String>,
    /// Expected input format.
    pub input_format: Option<String>,
    /// Expected output format.
    pub output_format: Option<String>,
}

/// A dependency on another plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    /// Plugin name.
    pub name: String,
    /// Version constraint (e.g., ">=1.0.0", "^2.0").
    pub version: String,
    /// Whether this dependency is optional.
    pub optional: bool,
}

/// Manifest validation error.
#[derive(Debug, Clone)]
pub enum ManifestValidationError {
    MissingField(String),
    InvalidVersion(String),
    InvalidName(String),
    SelfDependency,
}

impl std::fmt::Display for ManifestValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required field: {}", field),
            Self::InvalidVersion(v) => write!(f, "invalid semver: {}", v),
            Self::InvalidName(n) => write!(f, "invalid name (must be lowercase alphanumeric with hyphens): {}", n),
            Self::SelfDependency => write!(f, "plugin cannot depend on itself"),
        }
    }
}

/// Local catalog of installed plugins.
pub struct LocalCatalog {
    plugins: parking_lot::RwLock<HashMap<String, InstalledPlugin>>,
}

/// An installed plugin in the local catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    /// Local file path to the WASM module.
    pub local_path: String,
    /// Registry this plugin was installed from.
    pub source_registry: Option<String>,
    /// Whether this plugin is enabled.
    pub enabled: bool,
    /// Installation timestamp.
    pub installed_at_epoch_s: u64,
}

impl LocalCatalog {
    /// Create a new empty local catalog.
    pub fn new() -> Self {
        Self {
            plugins: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Install a plugin into the local catalog.
    pub fn install(&self, manifest: PluginManifest, local_path: String, source_registry: Option<String>) -> Result<(), String> {
        let errors = manifest.validate();
        if !errors.is_empty() {
            return Err(format!("manifest validation failed: {}", errors[0]));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let plugin = InstalledPlugin {
            manifest: manifest.clone(),
            local_path,
            source_registry,
            enabled: true,
            installed_at_epoch_s: now,
        };

        self.plugins.write().insert(manifest.name.clone(), plugin);
        Ok(())
    }

    /// Uninstall a plugin.
    pub fn uninstall(&self, name: &str) -> bool {
        self.plugins.write().remove(name).is_some()
    }

    /// Get an installed plugin by name.
    pub fn get(&self, name: &str) -> Option<InstalledPlugin> {
        self.plugins.read().get(name).cloned()
    }

    /// List all installed plugins.
    pub fn list(&self) -> Vec<InstalledPlugin> {
        self.plugins.read().values().cloned().collect()
    }

    /// Enable or disable a plugin.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> bool {
        if let Some(p) = self.plugins.write().get_mut(name) {
            p.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get only enabled plugins.
    pub fn list_enabled(&self) -> Vec<InstalledPlugin> {
        self.plugins
            .read()
            .values()
            .filter(|p| p.enabled)
            .cloned()
            .collect()
    }

    /// Check if all dependencies of a plugin are satisfied.
    pub fn check_dependencies(&self, name: &str) -> Vec<String> {
        let plugins = self.plugins.read();
        let plugin = match plugins.get(name) {
            Some(p) => p,
            None => return vec![format!("plugin '{}' not installed", name)],
        };

        let mut missing = Vec::new();
        for dep in &plugin.manifest.dependencies {
            if dep.optional {
                continue;
            }
            if !plugins.contains_key(&dep.name) {
                missing.push(format!("missing dependency: {} ({})", dep.name, dep.version));
            }
        }
        missing
    }

    /// Number of installed plugins.
    pub fn count(&self) -> usize {
        self.plugins.read().len()
    }
}

impl Default for LocalCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// OCI-compatible registry reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciRegistryConfig {
    /// Registry URL (e.g., "registry.example.com").
    pub url: String,
    /// Whether this is a trusted registry.
    pub trusted: bool,
    /// Authentication token (if required).
    pub auth_token: Option<String>,
    /// Registry display name.
    pub name: String,
}

/// Registry federation - manages multiple OCI registries.
pub struct RegistryFederation {
    registries: parking_lot::RwLock<Vec<OciRegistryConfig>>,
}

impl RegistryFederation {
    /// Create a new federation.
    pub fn new() -> Self {
        Self {
            registries: parking_lot::RwLock::new(Vec::new()),
        }
    }

    /// Add a registry to the federation.
    pub fn add_registry(&self, config: OciRegistryConfig) {
        self.registries.write().push(config);
    }

    /// Remove a registry by URL.
    pub fn remove_registry(&self, url: &str) -> bool {
        let mut registries = self.registries.write();
        let before = registries.len();
        registries.retain(|r| r.url != url);
        registries.len() < before
    }

    /// List all configured registries.
    pub fn list_registries(&self) -> Vec<OciRegistryConfig> {
        self.registries.read().clone()
    }

    /// Find registries that might have a given plugin (by name).
    /// In a real implementation this would query each registry's API.
    /// Here we return all trusted registries.
    pub fn resolve_registries(&self, _plugin_name: &str) -> Vec<OciRegistryConfig> {
        self.registries
            .read()
            .iter()
            .filter(|r| r.trusted)
            .cloned()
            .collect()
    }

    /// Build an OCI pull reference for a plugin.
    pub fn pull_reference(registry: &OciRegistryConfig, name: &str, version: &str) -> String {
        format!("{}/{}:{}", registry.url, name, version)
    }

    /// Number of configured registries.
    pub fn count(&self) -> usize {
        self.registries.read().len()
    }
}

impl Default for RegistryFederation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(name: &str, version: &str) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            name: name.into(),
            version: version.into(),
            description: "Test plugin".into(),
            author: "tester".into(),
            license: Some("MIT".into()),
            homepage: None,
            repository: None,
            capabilities: vec!["stdout".into()],
            resources: PluginResources::default(),
            entry_points: vec![EntryPoint {
                name: "_start".into(),
                description: None,
                input_format: None,
                output_format: None,
            }],
            dependencies: vec![],
            metadata: HashMap::new(),
            content_hash: "abc123def456".into(),
            size_bytes: 1024,
        }
    }

    #[test]
    fn test_manifest_validation_valid() {
        let m = make_manifest("my-plugin", "1.0.0");
        assert!(m.validate().is_empty());
    }

    #[test]
    fn test_manifest_validation_empty_name() {
        let m = make_manifest("", "1.0.0");
        let errors = m.validate();
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_manifest_validation_bad_version() {
        let m = make_manifest("plugin", "not-semver");
        let errors = m.validate();
        assert!(errors.iter().any(|e| matches!(e, ManifestValidationError::InvalidVersion(_))));
    }

    #[test]
    fn test_manifest_validation_bad_name() {
        let m = make_manifest("My_Plugin", "1.0.0");
        let errors = m.validate();
        assert!(errors.iter().any(|e| matches!(e, ManifestValidationError::InvalidName(_))));
    }

    #[test]
    fn test_manifest_self_dependency() {
        let mut m = make_manifest("my-plugin", "1.0.0");
        m.dependencies.push(PluginDependency {
            name: "my-plugin".into(),
            version: ">=1.0.0".into(),
            optional: false,
        });
        let errors = m.validate();
        assert!(errors.iter().any(|e| matches!(e, ManifestValidationError::SelfDependency)));
    }

    #[test]
    fn test_oci_reference() {
        let m = make_manifest("my-plugin", "1.0.0");
        assert_eq!(
            m.oci_reference("ghcr.io/isolate"),
            "ghcr.io/isolate/my-plugin:1.0.0"
        );
    }

    #[test]
    fn test_local_catalog_install() {
        let catalog = LocalCatalog::new();
        let m = make_manifest("test-plugin", "1.0.0");
        catalog
            .install(m, "/tmp/test.wasm".into(), Some("ghcr.io".into()))
            .unwrap();
        assert_eq!(catalog.count(), 1);

        let plugin = catalog.get("test-plugin").unwrap();
        assert_eq!(plugin.manifest.version, "1.0.0");
        assert!(plugin.enabled);
    }

    #[test]
    fn test_local_catalog_uninstall() {
        let catalog = LocalCatalog::new();
        catalog
            .install(make_manifest("p1", "1.0.0"), "/tmp/p1.wasm".into(), None)
            .unwrap();
        assert!(catalog.uninstall("p1"));
        assert_eq!(catalog.count(), 0);
    }

    #[test]
    fn test_local_catalog_enable_disable() {
        let catalog = LocalCatalog::new();
        catalog
            .install(make_manifest("p1", "1.0.0"), "/tmp/p1.wasm".into(), None)
            .unwrap();

        catalog.set_enabled("p1", false);
        assert!(!catalog.get("p1").unwrap().enabled);
        assert!(catalog.list_enabled().is_empty());

        catalog.set_enabled("p1", true);
        assert_eq!(catalog.list_enabled().len(), 1);
    }

    #[test]
    fn test_dependency_check_satisfied() {
        let catalog = LocalCatalog::new();
        let mut m = make_manifest("app", "1.0.0");
        m.dependencies.push(PluginDependency {
            name: "lib".into(),
            version: ">=1.0.0".into(),
            optional: false,
        });
        catalog.install(make_manifest("lib", "1.0.0"), "/tmp/lib.wasm".into(), None).unwrap();
        catalog.install(m, "/tmp/app.wasm".into(), None).unwrap();

        let missing = catalog.check_dependencies("app");
        assert!(missing.is_empty());
    }

    #[test]
    fn test_dependency_check_missing() {
        let catalog = LocalCatalog::new();
        let mut m = make_manifest("app", "1.0.0");
        m.dependencies.push(PluginDependency {
            name: "missing-lib".into(),
            version: ">=1.0.0".into(),
            optional: false,
        });
        catalog.install(m, "/tmp/app.wasm".into(), None).unwrap();

        let missing = catalog.check_dependencies("app");
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("missing-lib"));
    }

    #[test]
    fn test_optional_dependency_not_required() {
        let catalog = LocalCatalog::new();
        let mut m = make_manifest("app", "1.0.0");
        m.dependencies.push(PluginDependency {
            name: "optional-lib".into(),
            version: ">=1.0.0".into(),
            optional: true,
        });
        catalog.install(m, "/tmp/app.wasm".into(), None).unwrap();

        let missing = catalog.check_dependencies("app");
        assert!(missing.is_empty());
    }

    #[test]
    fn test_registry_federation() {
        let fed = RegistryFederation::new();
        fed.add_registry(OciRegistryConfig {
            url: "ghcr.io/isolate".into(),
            trusted: true,
            auth_token: None,
            name: "GitHub".into(),
        });
        fed.add_registry(OciRegistryConfig {
            url: "registry.example.com".into(),
            trusted: false,
            auth_token: Some("token".into()),
            name: "Private".into(),
        });

        assert_eq!(fed.count(), 2);

        let trusted = fed.resolve_registries("any-plugin");
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].url, "ghcr.io/isolate");
    }

    #[test]
    fn test_registry_remove() {
        let fed = RegistryFederation::new();
        fed.add_registry(OciRegistryConfig {
            url: "ghcr.io".into(),
            trusted: true,
            auth_token: None,
            name: "GH".into(),
        });
        assert!(fed.remove_registry("ghcr.io"));
        assert_eq!(fed.count(), 0);
    }

    #[test]
    fn test_pull_reference() {
        let reg = OciRegistryConfig {
            url: "ghcr.io/isolate".into(),
            trusted: true,
            auth_token: None,
            name: "GH".into(),
        };
        assert_eq!(
            RegistryFederation::pull_reference(&reg, "my-plugin", "2.0.0"),
            "ghcr.io/isolate/my-plugin:2.0.0"
        );
    }

    #[test]
    fn test_install_invalid_manifest_rejected() {
        let catalog = LocalCatalog::new();
        let m = make_manifest("", "1.0.0"); // Empty name
        let result = catalog.install(m, "/tmp/bad.wasm".into(), None);
        assert!(result.is_err());
    }
}
