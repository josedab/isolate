//! Dependency resolution for WASM module registry.
//!
//! Resolves transitive dependencies with version constraint solving,
//! cycle detection, and conflict reporting.
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::marketplace::resolver::{DependencyResolver, ResolvedDependency};
//!
//! let mut resolver = DependencyResolver::new(&registry);
//! let resolved = resolver.resolve("my-module", &VersionConstraint::Any)?;
//! for dep in &resolved {
//!     println!("{} @ {}", dep.name, dep.version);
//! }
//! ```

use super::registry::{ModuleVersion, Registry, VersionConstraint};
use std::collections::{HashMap, HashSet};

/// A resolved dependency with its version and depth in the dependency tree.
#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    /// Module name.
    pub name: String,
    /// Resolved version.
    pub version: ModuleVersion,
    /// Module hash.
    pub module_hash: String,
    /// Depth in dependency tree (0 = direct dependency).
    pub depth: usize,
    /// Parent module that depends on this one.
    pub required_by: Option<String>,
}

/// Error during dependency resolution.
#[derive(Debug, Clone)]
pub enum ResolveError {
    /// Module not found in registry.
    ModuleNotFound { name: String },
    /// No version satisfies the constraint.
    NoMatchingVersion { name: String, constraint: String, available: Vec<String> },
    /// Circular dependency detected.
    CyclicDependency { cycle: Vec<String> },
    /// Conflicting version requirements.
    VersionConflict { name: String, required_by: Vec<(String, String)> },
    /// Resolution depth exceeded.
    MaxDepthExceeded { depth: usize },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotFound { name } => write!(f, "module '{}' not found", name),
            Self::NoMatchingVersion { name, constraint, available } => write!(
                f,
                "no version of '{}' satisfies constraint '{}' (available: {})",
                name,
                constraint,
                available.join(", ")
            ),
            Self::CyclicDependency { cycle } => {
                write!(f, "cyclic dependency: {}", cycle.join(" → "))
            }
            Self::VersionConflict { name, required_by } => {
                let reqs: Vec<String> = required_by
                    .iter()
                    .map(|(by, ver)| format!("{} requires {}", by, ver))
                    .collect();
                write!(f, "conflicting versions for '{}': {}", name, reqs.join(", "))
            }
            Self::MaxDepthExceeded { depth } => {
                write!(f, "dependency resolution exceeded max depth ({})", depth)
            }
        }
    }
}

impl std::error::Error for ResolveError {}

/// Dependency resolver for the module registry.
pub struct DependencyResolver<'a> {
    registry: &'a Registry,
    max_depth: usize,
}

impl<'a> DependencyResolver<'a> {
    /// Create a new resolver.
    pub fn new(registry: &'a Registry) -> Self {
        Self { registry, max_depth: 64 }
    }

    /// Set the maximum resolution depth.
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Resolve all dependencies for a module (including transitive).
    ///
    /// Returns dependencies in topological order (leaves first, root last).
    pub fn resolve(
        &self,
        name: &str,
        constraint: &VersionConstraint,
    ) -> Result<Vec<ResolvedDependency>, ResolveError> {
        let mut resolved: HashMap<String, ResolvedDependency> = HashMap::new();
        let mut visiting: HashSet<String> = HashSet::new();
        let mut visit_path: Vec<String> = Vec::new();

        self.resolve_recursive(
            name,
            constraint,
            None,
            0,
            &mut resolved,
            &mut visiting,
            &mut visit_path,
        )?;

        // Return in topological order (BFS from leaves)
        let mut result: Vec<ResolvedDependency> = resolved.into_values().collect();
        result.sort_by(|a, b| b.depth.cmp(&a.depth).then(a.name.cmp(&b.name)));
        Ok(result)
    }

    /// Build a dependency tree (for display purposes).
    pub fn dependency_tree(
        &self,
        name: &str,
        constraint: &VersionConstraint,
    ) -> Result<DependencyNode, ResolveError> {
        let resolved = self.resolve(name, constraint)?;
        let dep_map: HashMap<String, &ResolvedDependency> =
            resolved.iter().map(|d| (d.name.clone(), d)).collect();

        // Get the root module
        let root = dep_map
            .get(name)
            .ok_or_else(|| ResolveError::ModuleNotFound { name: name.to_string() })?;

        Ok(self.build_tree_node(name, &root.version, &dep_map))
    }

    fn build_tree_node(
        &self,
        name: &str,
        version: &ModuleVersion,
        dep_map: &HashMap<String, &ResolvedDependency>,
    ) -> DependencyNode {
        let children: Vec<DependencyNode> = dep_map
            .values()
            .filter(|d| d.required_by.as_deref() == Some(name))
            .map(|d| self.build_tree_node(&d.name, &d.version, dep_map))
            .collect();

        DependencyNode { name: name.to_string(), version: version.clone(), children }
    }

    fn resolve_recursive(
        &self,
        name: &str,
        constraint: &VersionConstraint,
        required_by: Option<&str>,
        depth: usize,
        resolved: &mut HashMap<String, ResolvedDependency>,
        visiting: &mut HashSet<String>,
        visit_path: &mut Vec<String>,
    ) -> Result<(), ResolveError> {
        if depth > self.max_depth {
            return Err(ResolveError::MaxDepthExceeded { depth });
        }

        // Check for cycles
        if visiting.contains(name) {
            let cycle_start = visit_path.iter().position(|n| n == name).unwrap_or(0);
            let mut cycle: Vec<String> = visit_path[cycle_start..].to_vec();
            cycle.push(name.to_string());
            return Err(ResolveError::CyclicDependency { cycle });
        }

        // If already resolved, check version compatibility
        if let Some(existing) = resolved.get(name) {
            if existing.version.satisfies(constraint) {
                return Ok(());
            } else {
                return Err(ResolveError::VersionConflict {
                    name: name.to_string(),
                    required_by: vec![
                        (
                            existing.required_by.clone().unwrap_or_default(),
                            existing.version.to_string(),
                        ),
                        (required_by.unwrap_or("root").to_string(), format!("{:?}", constraint)),
                    ],
                });
            }
        }

        // Find the best matching version in the registry
        let entry = self.find_best_version(name, constraint)?;

        // Mark as visiting
        visiting.insert(name.to_string());
        visit_path.push(name.to_string());

        // Resolve transitive dependencies
        for (dep_name, dep_constraint_str) in &entry.manifest.dependencies {
            let dep_constraint = parse_constraint(dep_constraint_str);
            self.resolve_recursive(
                dep_name,
                &dep_constraint,
                Some(name),
                depth + 1,
                resolved,
                visiting,
                visit_path,
            )?;
        }

        // Add to resolved
        resolved.insert(
            name.to_string(),
            ResolvedDependency {
                name: name.to_string(),
                version: entry.manifest.version.clone(),
                module_hash: entry.module_hash.clone(),
                depth,
                required_by: required_by.map(|s| s.to_string()),
            },
        );

        visit_path.pop();
        visiting.remove(name);

        Ok(())
    }

    fn find_best_version(
        &self,
        name: &str,
        constraint: &VersionConstraint,
    ) -> Result<super::registry::RegistryEntry, ResolveError> {
        let versions = self.registry.list_versions(name);
        if versions.is_empty() {
            return Err(ResolveError::ModuleNotFound { name: name.to_string() });
        }

        // Find the latest version satisfying the constraint
        let mut matching: Vec<_> =
            versions.into_iter().filter(|e| e.manifest.version.satisfies(constraint)).collect();

        matching.sort_by(|a, b| b.manifest.version.cmp(&a.manifest.version));

        matching.into_iter().next().ok_or_else(|| {
            let available = self
                .registry
                .list_versions(name)
                .iter()
                .map(|e| e.manifest.version.to_string())
                .collect();
            ResolveError::NoMatchingVersion {
                name: name.to_string(),
                constraint: format!("{:?}", constraint),
                available,
            }
        })
    }
}

/// A node in the dependency tree.
#[derive(Debug, Clone)]
pub struct DependencyNode {
    /// Module name.
    pub name: String,
    /// Resolved version.
    pub version: ModuleVersion,
    /// Child dependencies.
    pub children: Vec<DependencyNode>,
}

impl DependencyNode {
    /// Format as an indented tree string.
    pub fn to_tree_string(&self) -> String {
        let mut output = String::new();
        self.format_tree(&mut output, "", true);
        output
    }

    fn format_tree(&self, output: &mut String, prefix: &str, is_last: bool) {
        let connector = if prefix.is_empty() {
            ""
        } else if is_last {
            "└── "
        } else {
            "├── "
        };
        output.push_str(&format!("{}{}{} @ {}\n", prefix, connector, self.name, self.version));

        let child_prefix = if prefix.is_empty() {
            "".to_string()
        } else if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        for (i, child) in self.children.iter().enumerate() {
            child.format_tree(output, &child_prefix, i == self.children.len() - 1);
        }
    }
}

/// Parse a version constraint string like "^1.2.0", ">=1.0.0", "=1.2.3", "*".
fn parse_constraint(s: &str) -> VersionConstraint {
    let s = s.trim();

    if s == "*" || s.is_empty() {
        return VersionConstraint::Any;
    }

    if let Some(rest) = s.strip_prefix('^') {
        if let Ok(v) = ModuleVersion::parse(rest) {
            return VersionConstraint::Compatible(v);
        }
    }

    if let Some(rest) = s.strip_prefix(">=") {
        if let Ok(v) = ModuleVersion::parse(rest) {
            return VersionConstraint::Gte(v);
        }
    }

    if let Some(rest) = s.strip_prefix('<') {
        if let Ok(v) = ModuleVersion::parse(rest) {
            return VersionConstraint::Lt(v);
        }
    }

    if let Some(rest) = s.strip_prefix('=') {
        if let Ok(v) = ModuleVersion::parse(rest) {
            return VersionConstraint::Exact(v);
        }
    }

    // Try exact version
    if let Ok(v) = ModuleVersion::parse(s) {
        return VersionConstraint::Exact(v);
    }

    VersionConstraint::Any
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::registry::{ModuleManifest, RegistryConfig, RegistryEntry, TrustLevel};

    fn make_entry(
        manifest: super::super::registry::ModuleManifest,
        wasm: &[u8],
        trust: TrustLevel,
    ) -> RegistryEntry {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(wasm);
        let hash = hex::encode(hasher.finalize());
        RegistryEntry {
            manifest,
            trust_level: trust,
            module_hash: hash,
            size_bytes: wasm.len(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            downloads: 0,
            signature: None,
            signing_key_id: None,
        }
    }

    fn make_registry() -> Registry {
        let mut registry = Registry::new(RegistryConfig::default());

        let manifest_a = ModuleManifest::builder("module-a", ModuleVersion::new(1, 0, 0))
            .description("Module A")
            .build();
        registry.publish(make_entry(manifest_a, b"wasm-a-100", TrustLevel::Community)).unwrap();

        let manifest_a11 = ModuleManifest::builder("module-a", ModuleVersion::new(1, 1, 0))
            .description("Module A v1.1")
            .build();
        registry.publish(make_entry(manifest_a11, b"wasm-a-110", TrustLevel::Community)).unwrap();

        let manifest_b = ModuleManifest::builder("module-b", ModuleVersion::new(1, 0, 0))
            .description("Module B")
            .dependency("module-a", "^1.0.0")
            .build();
        registry.publish(make_entry(manifest_b, b"wasm-b-100", TrustLevel::Community)).unwrap();

        let manifest_c = ModuleManifest::builder("module-c", ModuleVersion::new(1, 0, 0))
            .description("Module C")
            .dependency("module-a", "^1.0.0")
            .dependency("module-b", "^1.0.0")
            .build();
        registry.publish(make_entry(manifest_c, b"wasm-c-100", TrustLevel::Community)).unwrap();

        registry
    }

    #[test]
    fn test_resolve_no_deps() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let resolved = resolver.resolve("module-a", &VersionConstraint::Any).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "module-a");
        assert_eq!(resolved[0].version, ModuleVersion::new(1, 1, 0)); // latest
    }

    #[test]
    fn test_resolve_exact_version() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let resolved = resolver
            .resolve("module-a", &VersionConstraint::Exact(ModuleVersion::new(1, 0, 0)))
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].version, ModuleVersion::new(1, 0, 0));
    }

    #[test]
    fn test_resolve_transitive() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let resolved = resolver.resolve("module-b", &VersionConstraint::Any).unwrap();
        assert_eq!(resolved.len(), 2);

        let names: Vec<&str> = resolved.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"module-a"));
        assert!(names.contains(&"module-b"));
    }

    #[test]
    fn test_resolve_diamond_dependency() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        // C depends on A and B; B also depends on A
        let resolved = resolver.resolve("module-c", &VersionConstraint::Any).unwrap();

        assert_eq!(resolved.len(), 3);
        let names: Vec<&str> = resolved.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"module-a"));
        assert!(names.contains(&"module-b"));
        assert!(names.contains(&"module-c"));

        // module-a should appear exactly once (deduplicated)
        assert_eq!(resolved.iter().filter(|d| d.name == "module-a").count(), 1);
    }

    #[test]
    fn test_resolve_not_found() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let result = resolver.resolve("nonexistent", &VersionConstraint::Any);
        assert!(matches!(result, Err(ResolveError::ModuleNotFound { .. })));
    }

    #[test]
    fn test_resolve_no_matching_version() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let result =
            resolver.resolve("module-a", &VersionConstraint::Exact(ModuleVersion::new(9, 9, 9)));
        assert!(matches!(result, Err(ResolveError::NoMatchingVersion { .. })));
    }

    #[test]
    fn test_resolve_cyclic_dependency() {
        let mut registry = Registry::new(RegistryConfig::default());

        let manifest_x = ModuleManifest::builder("module-x", ModuleVersion::new(1, 0, 0))
            .description("Module X")
            .dependency("module-y", "^1.0.0")
            .build();
        registry.publish(make_entry(manifest_x, b"wasm-x", TrustLevel::Community)).unwrap();

        let manifest_y = ModuleManifest::builder("module-y", ModuleVersion::new(1, 0, 0))
            .description("Module Y")
            .dependency("module-x", "^1.0.0")
            .build();
        registry.publish(make_entry(manifest_y, b"wasm-y", TrustLevel::Community)).unwrap();

        let resolver = DependencyResolver::new(&registry);
        let result = resolver.resolve("module-x", &VersionConstraint::Any);
        assert!(matches!(result, Err(ResolveError::CyclicDependency { .. })));
    }

    #[test]
    fn test_dependency_tree() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry);

        let tree = resolver.dependency_tree("module-c", &VersionConstraint::Any).unwrap();
        assert_eq!(tree.name, "module-c");
        assert!(!tree.children.is_empty());

        let tree_str = tree.to_tree_string();
        assert!(tree_str.contains("module-c"));
    }

    #[test]
    fn test_parse_constraint() {
        assert!(matches!(parse_constraint("*"), VersionConstraint::Any));
        assert!(matches!(parse_constraint(""), VersionConstraint::Any));
        assert!(matches!(parse_constraint("^1.0.0"), VersionConstraint::Compatible(_)));
        assert!(matches!(parse_constraint(">=1.2.0"), VersionConstraint::Gte(_)));
        assert!(matches!(parse_constraint("<2.0.0"), VersionConstraint::Lt(_)));
        assert!(matches!(parse_constraint("=1.0.0"), VersionConstraint::Exact(_)));
    }

    #[test]
    fn test_max_depth_exceeded() {
        let registry = make_registry();
        let resolver = DependencyResolver::new(&registry).with_max_depth(0);

        let result = resolver.resolve("module-b", &VersionConstraint::Any);
        // Should fail because module-b has deps but max_depth is 0
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_error_display() {
        let err = ResolveError::ModuleNotFound { name: "test".to_string() };
        assert_eq!(err.to_string(), "module 'test' not found");

        let err = ResolveError::CyclicDependency {
            cycle: vec!["a".to_string(), "b".to_string(), "a".to_string()],
        };
        assert!(err.to_string().contains("a → b → a"));
    }
}
