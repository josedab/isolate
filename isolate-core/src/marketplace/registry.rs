//! Module registry implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trust level for a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Unknown or unverified module.
    Unknown,
    /// Community-contributed module.
    Community,
    /// Module signed by a known key.
    Signed,
    /// Verified by the registry maintainers.
    Verified,
    /// First-party (official) module.
    Official,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::Community => write!(f, "community"),
            Self::Signed => write!(f, "signed"),
            Self::Verified => write!(f, "verified"),
            Self::Official => write!(f, "official"),
        }
    }
}

/// Semantic version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre_release: Option<String>,
}

impl ModuleVersion {
    /// Create a new version.
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch, pre_release: None }
    }

    /// Parse from a version string (e.g., "1.2.3").
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let (version_str, pre_release) = if let Some((v, pre)) = s.split_once('-') {
            (v, Some(pre.to_string()))
        } else {
            (s, None)
        };

        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("Invalid version format: {}", s));
        }

        Ok(Self {
            major: parts[0].parse().map_err(|_| format!("Invalid major: {}", parts[0]))?,
            minor: parts[1].parse().map_err(|_| format!("Invalid minor: {}", parts[1]))?,
            patch: parts[2].parse().map_err(|_| format!("Invalid patch: {}", parts[2]))?,
            pre_release,
        })
    }

    /// Check if this version satisfies a constraint.
    pub fn satisfies(&self, constraint: &VersionConstraint) -> bool {
        match constraint {
            VersionConstraint::Exact(v) => self == v,
            VersionConstraint::Gte(v) => self >= v,
            VersionConstraint::Lt(v) => self < v,
            VersionConstraint::Range { min, max } => self >= min && self < max,
            VersionConstraint::Compatible(v) => {
                self.major == v.major
                    && (self.minor > v.minor || (self.minor == v.minor && self.patch >= v.patch))
            }
            VersionConstraint::Any => true,
        }
    }
}

impl PartialOrd for ModuleVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModuleVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

impl std::fmt::Display for ModuleVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.pre_release {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

/// Version constraint for dependency resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionConstraint {
    /// Exact version match.
    Exact(ModuleVersion),
    /// Greater than or equal.
    Gte(ModuleVersion),
    /// Less than.
    Lt(ModuleVersion),
    /// Range [min, max).
    Range { min: ModuleVersion, max: ModuleVersion },
    /// Compatible (same major, >= minor.patch).
    Compatible(ModuleVersion),
    /// Any version.
    Any,
}

/// Module manifest declaring capabilities and requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleManifest {
    /// Module name (unique identifier).
    pub name: String,
    /// Module version.
    pub version: ModuleVersion,
    /// Human-readable description.
    pub description: String,
    /// Author name.
    pub author: Option<String>,
    /// License (SPDX identifier).
    pub license: Option<String>,
    /// Repository URL.
    pub repository: Option<String>,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// Keywords for search.
    pub keywords: Vec<String>,
    /// Required capabilities (what the module needs).
    pub required_capabilities: Vec<String>,
    /// Provided capabilities (what the module offers as host functions).
    pub provided_capabilities: Vec<String>,
    /// Minimum Isolate version required.
    pub min_isolate_version: Option<String>,
    /// Dependencies on other modules.
    pub dependencies: HashMap<String, String>,
    /// Module entry point.
    pub entry_point: Option<String>,
    /// Custom metadata.
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ModuleManifest {
    /// Create a new manifest builder.
    pub fn builder(name: impl Into<String>, version: ModuleVersion) -> ManifestBuilder {
        ManifestBuilder::new(name, version)
    }

    /// Validate the manifest.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.name.is_empty() {
            errors.push("Module name cannot be empty".to_string());
        }

        if self.name.contains(' ') {
            errors.push("Module name cannot contain spaces".to_string());
        }

        if self.description.is_empty() {
            errors.push("Description cannot be empty".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Builder for ModuleManifest.
pub struct ManifestBuilder {
    name: String,
    version: ModuleVersion,
    description: String,
    author: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    keywords: Vec<String>,
    required_capabilities: Vec<String>,
    provided_capabilities: Vec<String>,
    min_isolate_version: Option<String>,
    dependencies: HashMap<String, String>,
    entry_point: Option<String>,
    metadata: HashMap<String, serde_json::Value>,
}

impl ManifestBuilder {
    fn new(name: impl Into<String>, version: ModuleVersion) -> Self {
        Self {
            name: name.into(),
            version,
            description: String::new(),
            author: None,
            license: None,
            repository: None,
            homepage: None,
            keywords: Vec::new(),
            required_capabilities: Vec::new(),
            provided_capabilities: Vec::new(),
            min_isolate_version: None,
            dependencies: HashMap::new(),
            entry_point: None,
            metadata: HashMap::new(),
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.push(keyword.into());
        self
    }

    pub fn require_capability(mut self, cap: impl Into<String>) -> Self {
        self.required_capabilities.push(cap.into());
        self
    }

    pub fn provide_capability(mut self, cap: impl Into<String>) -> Self {
        self.provided_capabilities.push(cap.into());
        self
    }

    pub fn dependency(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.dependencies.insert(name.into(), version.into());
        self
    }

    pub fn build(self) -> ModuleManifest {
        ModuleManifest {
            name: self.name,
            version: self.version,
            description: self.description,
            author: self.author,
            license: self.license,
            repository: self.repository,
            homepage: self.homepage,
            keywords: self.keywords,
            required_capabilities: self.required_capabilities,
            provided_capabilities: self.provided_capabilities,
            min_isolate_version: self.min_isolate_version,
            dependencies: self.dependencies,
            entry_point: self.entry_point,
            metadata: self.metadata,
        }
    }
}

/// A registry entry (published module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Module manifest.
    pub manifest: ModuleManifest,
    /// Trust level.
    pub trust_level: TrustLevel,
    /// Module hash (SHA-256 of WASM bytes).
    pub module_hash: String,
    /// Module size in bytes.
    pub size_bytes: usize,
    /// Publication timestamp.
    pub published_at: String,
    /// Download count.
    pub downloads: u64,
    /// Signature (if signed).
    pub signature: Option<String>,
    /// Signing key ID (if signed).
    pub signing_key_id: Option<String>,
}

/// Search query for the registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Search text (matches name, description, keywords).
    pub text: Option<String>,
    /// Filter by keyword.
    pub keyword: Option<String>,
    /// Filter by minimum trust level.
    pub min_trust: Option<TrustLevel>,
    /// Filter by author.
    pub author: Option<String>,
    /// Maximum results to return.
    pub limit: Option<usize>,
    /// Offset for pagination.
    pub offset: Option<usize>,
}

/// Search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Matching entries.
    pub entries: Vec<RegistryEntry>,
    /// Total matches (for pagination).
    pub total: usize,
}

/// Registry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Maximum modules in the registry.
    pub max_modules: usize,
    /// Maximum versions per module.
    pub max_versions_per_module: usize,
    /// Require all modules to be signed.
    pub require_signatures: bool,
    /// Minimum trust level for installation.
    pub min_install_trust: TrustLevel,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_modules: 10_000,
            max_versions_per_module: 100,
            require_signatures: false,
            min_install_trust: TrustLevel::Unknown,
        }
    }
}

/// The module registry.
pub struct Registry {
    config: RegistryConfig,
    /// name -> version -> entry
    modules: HashMap<String, HashMap<String, RegistryEntry>>,
}

impl Registry {
    /// Create a new registry.
    pub fn new(config: RegistryConfig) -> Self {
        Self { config, modules: HashMap::new() }
    }

    /// Publish a module to the registry.
    pub fn publish(&mut self, entry: RegistryEntry) -> Result<(), String> {
        // Validate manifest
        if let Err(errors) = entry.manifest.validate() {
            return Err(format!("Invalid manifest: {}", errors.join(", ")));
        }

        // Check trust level for signature requirement
        if self.config.require_signatures && entry.signature.is_none() {
            return Err("Module must be signed".to_string());
        }

        // Check module count limit
        if !self.modules.contains_key(&entry.manifest.name)
            && self.modules.len() >= self.config.max_modules
        {
            return Err("Registry is full".to_string());
        }

        // Check version count limit
        let versions = self.modules.entry(entry.manifest.name.clone()).or_default();

        if versions.len() >= self.config.max_versions_per_module {
            return Err("Too many versions for this module".to_string());
        }

        let version_str = entry.manifest.version.to_string();
        if versions.contains_key(&version_str) {
            return Err(format!(
                "Version {} already published for {}",
                version_str, entry.manifest.name
            ));
        }

        versions.insert(version_str, entry);
        Ok(())
    }

    /// Get a specific module version.
    pub fn get(&self, name: &str, version: &str) -> Option<&RegistryEntry> {
        self.modules.get(name).and_then(|v| v.get(version))
    }

    /// Get the latest version of a module.
    pub fn get_latest(&self, name: &str) -> Option<&RegistryEntry> {
        self.modules.get(name).and_then(|versions| {
            versions.values().max_by(|a, b| a.manifest.version.cmp(&b.manifest.version))
        })
    }

    /// Search the registry.
    pub fn search(&self, query: &SearchQuery) -> SearchResult {
        let mut results: Vec<&RegistryEntry> = Vec::new();

        for versions in self.modules.values() {
            // Get latest version for each module
            if let Some(latest) =
                versions.values().max_by(|a, b| a.manifest.version.cmp(&b.manifest.version))
            {
                let mut matches = true;

                // Text search
                if let Some(ref text) = query.text {
                    let lower = text.to_lowercase();
                    let name_match = latest.manifest.name.to_lowercase().contains(&lower);
                    let desc_match = latest.manifest.description.to_lowercase().contains(&lower);
                    let keyword_match =
                        latest.manifest.keywords.iter().any(|k| k.to_lowercase().contains(&lower));
                    matches = name_match || desc_match || keyword_match;
                }

                // Keyword filter
                if let Some(ref keyword) = query.keyword {
                    matches = matches && latest.manifest.keywords.contains(keyword);
                }

                // Trust level filter
                if let Some(ref min_trust) = query.min_trust {
                    matches = matches && latest.trust_level >= *min_trust;
                }

                // Author filter
                if let Some(ref author) = query.author {
                    matches = matches
                        && latest.manifest.author.as_ref().map(|a| a == author).unwrap_or(false);
                }

                if matches {
                    results.push(latest);
                }
            }
        }

        let total = results.len();
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(20);

        let entries: Vec<RegistryEntry> =
            results.into_iter().skip(offset).take(limit).cloned().collect();

        SearchResult { entries, total }
    }

    /// Get total module count.
    pub fn count(&self) -> usize {
        self.modules.len()
    }

    /// List all module names.
    pub fn list_modules(&self) -> Vec<String> {
        self.modules.keys().cloned().collect()
    }

    /// Remove a module and all its versions.
    pub fn remove(&mut self, name: &str) -> bool {
        self.modules.remove(name).is_some()
    }

    /// List all versions of a module as registry entries.
    pub fn list_versions(&self, name: &str) -> Vec<RegistryEntry> {
        self.modules
            .get(name)
            .map(|versions| versions.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest(name: &str, version: &str) -> ModuleManifest {
        ModuleManifest::builder(name, ModuleVersion::parse(version).unwrap())
            .description(format!("Test module: {}", name))
            .author("tester")
            .keyword("test")
            .build()
    }

    fn test_entry(name: &str, version: &str) -> RegistryEntry {
        RegistryEntry {
            manifest: test_manifest(name, version),
            trust_level: TrustLevel::Community,
            module_hash: "abc123".to_string(),
            size_bytes: 1024,
            published_at: "2025-01-01T00:00:00Z".to_string(),
            downloads: 0,
            signature: None,
            signing_key_id: None,
        }
    }

    #[test]
    fn test_version_parse() {
        let v = ModuleVersion::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.to_string(), "1.2.3");

        let v = ModuleVersion::parse("v0.1.0-beta").unwrap();
        assert_eq!(v.pre_release, Some("beta".to_string()));
    }

    #[test]
    fn test_version_ordering() {
        let v1 = ModuleVersion::new(1, 0, 0);
        let v2 = ModuleVersion::new(1, 1, 0);
        let v3 = ModuleVersion::new(2, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn test_version_constraint() {
        let v = ModuleVersion::new(1, 5, 0);

        assert!(v.satisfies(&VersionConstraint::Any));
        assert!(v.satisfies(&VersionConstraint::Exact(ModuleVersion::new(1, 5, 0))));
        assert!(!v.satisfies(&VersionConstraint::Exact(ModuleVersion::new(1, 6, 0))));
        assert!(v.satisfies(&VersionConstraint::Gte(ModuleVersion::new(1, 0, 0))));
        assert!(!v.satisfies(&VersionConstraint::Gte(ModuleVersion::new(2, 0, 0))));
        assert!(v.satisfies(&VersionConstraint::Compatible(ModuleVersion::new(1, 3, 0))));
        assert!(!v.satisfies(&VersionConstraint::Compatible(ModuleVersion::new(2, 0, 0))));
    }

    #[test]
    fn test_manifest_validation() {
        let valid = test_manifest("my-module", "1.0.0");
        assert!(valid.validate().is_ok());

        let invalid =
            ModuleManifest::builder("", ModuleVersion::new(1, 0, 0)).description("").build();
        assert!(invalid.validate().is_err());

        let spaces = ModuleManifest::builder("has spaces", ModuleVersion::new(1, 0, 0))
            .description("desc")
            .build();
        assert!(spaces.validate().is_err());
    }

    #[test]
    fn test_publish_and_get() {
        let mut registry = Registry::new(RegistryConfig::default());

        registry.publish(test_entry("my-module", "1.0.0")).unwrap();
        registry.publish(test_entry("my-module", "1.1.0")).unwrap();

        assert_eq!(registry.count(), 1);

        let entry = registry.get("my-module", "1.0.0").unwrap();
        assert_eq!(entry.manifest.version, ModuleVersion::new(1, 0, 0));

        let latest = registry.get_latest("my-module").unwrap();
        assert_eq!(latest.manifest.version, ModuleVersion::new(1, 1, 0));
    }

    #[test]
    fn test_duplicate_version() {
        let mut registry = Registry::new(RegistryConfig::default());
        registry.publish(test_entry("mod", "1.0.0")).unwrap();
        assert!(registry.publish(test_entry("mod", "1.0.0")).is_err());
    }

    #[test]
    fn test_search_by_text() {
        let mut registry = Registry::new(RegistryConfig::default());
        registry.publish(test_entry("json-parser", "1.0.0")).unwrap();
        registry.publish(test_entry("csv-reader", "1.0.0")).unwrap();

        let results =
            registry.search(&SearchQuery { text: Some("json".to_string()), ..Default::default() });
        assert_eq!(results.total, 1);
        assert_eq!(results.entries[0].manifest.name, "json-parser");
    }

    #[test]
    fn test_search_by_trust() {
        let mut registry = Registry::new(RegistryConfig::default());

        let mut entry = test_entry("trusted", "1.0.0");
        entry.trust_level = TrustLevel::Verified;
        registry.publish(entry).unwrap();

        let mut entry = test_entry("untrusted", "1.0.0");
        entry.trust_level = TrustLevel::Unknown;
        registry.publish(entry).unwrap();

        let results = registry
            .search(&SearchQuery { min_trust: Some(TrustLevel::Verified), ..Default::default() });
        assert_eq!(results.total, 1);
    }

    #[test]
    fn test_require_signatures() {
        let mut registry =
            Registry::new(RegistryConfig { require_signatures: true, ..Default::default() });

        // Unsigned module should fail
        assert!(registry.publish(test_entry("mod", "1.0.0")).is_err());

        // Signed module should succeed
        let mut entry = test_entry("mod", "1.0.0");
        entry.signature = Some("sig123".to_string());
        assert!(registry.publish(entry).is_ok());
    }

    #[test]
    fn test_remove_module() {
        let mut registry = Registry::new(RegistryConfig::default());
        registry.publish(test_entry("mod", "1.0.0")).unwrap();

        assert!(registry.remove("mod"));
        assert_eq!(registry.count(), 0);
        assert!(!registry.remove("mod"));
    }

    #[test]
    fn test_trust_level_ordering() {
        assert!(TrustLevel::Unknown < TrustLevel::Community);
        assert!(TrustLevel::Community < TrustLevel::Signed);
        assert!(TrustLevel::Signed < TrustLevel::Verified);
        assert!(TrustLevel::Verified < TrustLevel::Official);
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = ModuleManifest::builder("my-plugin", ModuleVersion::new(1, 0, 0))
            .description("A test plugin")
            .author("dev")
            .license("MIT")
            .keyword("utility")
            .require_capability("stdio:stdout")
            .provide_capability("hostfn:format")
            .dependency("other-module", "^1.0")
            .build();

        assert_eq!(manifest.name, "my-plugin");
        assert_eq!(manifest.required_capabilities, vec!["stdio:stdout"]);
        assert_eq!(manifest.provided_capabilities, vec!["hostfn:format"]);
    }
}
