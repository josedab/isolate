//! WASM Component Composition and Linking.
//!
//! Enables composing multiple WASM components via WIT interfaces,
//! with typed imports/exports and automatic dependency resolution.



use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A WIT (WebAssembly Interface Types) interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitInterface {
    /// Interface name (e.g., "wasi:filesystem/types@0.2.0").
    pub name: String,
    /// Package containing this interface.
    pub package: String,
    /// Version of the interface.
    pub version: String,
    /// Functions exported by this interface.
    pub functions: Vec<WitFunction>,
    /// Types defined in this interface.
    pub types: Vec<WitType>,
}

/// A function in a WIT interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitFunction {
    pub name: String,
    pub params: Vec<WitParam>,
    pub results: Vec<WitParam>,
    pub docs: Option<String>,
}

/// A parameter or result in a WIT function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitParam {
    pub name: String,
    pub ty: WitValueType,
}

/// WIT value types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WitValueType {
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
    List(Box<WitValueType>),
    Option(Box<WitValueType>),
    Result { ok: Option<Box<WitValueType>>, err: Option<Box<WitValueType>> },
    Record(String),
    Variant(String),
    Enum(String),
    Flags(String),
    Tuple(Vec<WitValueType>),
}

/// A type defined in a WIT interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitType {
    pub name: String,
    pub kind: WitTypeKind,
    pub docs: Option<String>,
}

/// Kinds of WIT type definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WitTypeKind {
    Record(Vec<WitParam>),
    Variant(Vec<WitVariantCase>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Alias(WitValueType),
    Resource(Vec<WitFunction>),
}

/// A case in a WIT variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitVariantCase {
    pub name: String,
    pub ty: Option<WitValueType>,
}

/// A composable WASM component with typed imports and exports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    /// Component name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Interfaces this component imports (requires).
    pub imports: Vec<InterfaceRef>,
    /// Interfaces this component exports (provides).
    pub exports: Vec<InterfaceRef>,
    /// Module hash of the compiled component.
    pub module_hash: String,
    /// Size in bytes.
    pub size: usize,
}

/// Reference to an interface with version constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceRef {
    /// Interface name.
    pub interface: String,
    /// Version constraint (semver range).
    pub version_constraint: String,
    /// Whether this import is optional.
    pub optional: bool,
}

/// Result of composing multiple components.
#[derive(Debug, Clone)]
pub struct CompositionResult {
    /// Components in dependency order (leaf-first).
    pub component_order: Vec<String>,
    /// Resolved interface bindings.
    pub bindings: Vec<InterfaceBinding>,
    /// Unresolved imports (if any).
    pub unresolved: Vec<InterfaceRef>,
    /// Warnings during composition.
    pub warnings: Vec<String>,
}

/// A binding between a component import and export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceBinding {
    /// Consuming component.
    pub consumer: String,
    /// Import interface name.
    pub import_interface: String,
    /// Providing component.
    pub provider: String,
    /// Export interface name.
    pub export_interface: String,
}

/// Error during composition.
#[derive(Debug, Clone)]
pub enum CompositionError {
    /// Circular dependency detected.
    CircularDependency(Vec<String>),
    /// Unresolved import.
    UnresolvedImport { component: String, interface: String },
    /// Type mismatch between import and export.
    TypeMismatch { interface: String, expected: String, actual: String },
    /// Version incompatibility.
    VersionIncompatible { interface: String, required: String, available: String },
    /// Duplicate export.
    DuplicateExport { interface: String, component1: String, component2: String },
}

impl std::fmt::Display for CompositionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CircularDependency(cycle) => {
                write!(f, "Circular dependency: {}", cycle.join(" -> "))
            }
            Self::UnresolvedImport { component, interface } => {
                write!(f, "Unresolved import: {} requires {}", component, interface)
            }
            Self::TypeMismatch { interface, expected, actual } => {
                write!(f, "Type mismatch in {}: expected {}, got {}", interface, expected, actual)
            }
            Self::VersionIncompatible { interface, required, available } => {
                write!(
                    f,
                    "Version incompatible for {}: requires {}, available {}",
                    interface, required, available
                )
            }
            Self::DuplicateExport { interface, component1, component2 } => {
                write!(
                    f,
                    "Duplicate export of {} by {} and {}",
                    interface, component1, component2
                )
            }
        }
    }
}

impl std::error::Error for CompositionError {}

/// Composes multiple WASM components by resolving interface dependencies.
pub struct ComponentComposer {
    /// Registered components.
    components: HashMap<String, ComponentDescriptor>,
    /// Known WIT interfaces.
    interfaces: HashMap<String, WitInterface>,
}

impl ComponentComposer {
    /// Create a new composer.
    pub fn new() -> Self {
        Self { components: HashMap::new(), interfaces: HashMap::new() }
    }

    /// Register a WIT interface definition.
    pub fn register_interface(&mut self, interface: WitInterface) {
        self.interfaces.insert(interface.name.clone(), interface);
    }

    /// Register a component for composition.
    pub fn register_component(&mut self, component: ComponentDescriptor) {
        self.components.insert(component.name.clone(), component);
    }

    /// Compose all registered components.
    pub fn compose(&self) -> Result<CompositionResult, CompositionError> {
        // Build export index: interface -> component
        let mut export_index: HashMap<&str, &str> = HashMap::new();
        for (name, comp) in &self.components {
            for export in &comp.exports {
                if let Some(existing) = export_index.get(export.interface.as_str()) {
                    return Err(CompositionError::DuplicateExport {
                        interface: export.interface.clone(),
                        component1: existing.to_string(),
                        component2: name.clone(),
                    });
                }
                export_index.insert(&export.interface, name);
            }
        }

        // Resolve imports and build dependency graph
        let mut bindings = Vec::new();
        let mut unresolved = Vec::new();
        let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();

        for (name, comp) in &self.components {
            deps.entry(name).or_default();
            for import in &comp.imports {
                if let Some(&provider) = export_index.get(import.interface.as_str()) {
                    bindings.push(InterfaceBinding {
                        consumer: name.clone(),
                        import_interface: import.interface.clone(),
                        provider: provider.to_string(),
                        export_interface: import.interface.clone(),
                    });
                    deps.entry(name).or_default().push(provider);
                } else if !import.optional {
                    unresolved.push(import.clone());
                }
            }
        }

        // Topological sort for component order
        let order = self.topological_sort(&deps)?;

        let warnings = if unresolved.is_empty() {
            Vec::new()
        } else {
            unresolved
                .iter()
                .map(|u| format!("Unresolved optional import: {}", u.interface))
                .collect()
        };

        Ok(CompositionResult {
            component_order: order,
            bindings,
            unresolved,
            warnings,
        })
    }

    /// Topological sort of components.
    fn topological_sort(
        &self,
        deps: &HashMap<&str, Vec<&str>>,
    ) -> Result<Vec<String>, CompositionError> {
        let mut visited: HashMap<&str, bool> = HashMap::new(); // false=in-progress, true=done
        let mut order = Vec::new();

        for &node in deps.keys() {
            if !visited.contains_key(node) {
                self.visit(node, deps, &mut visited, &mut order)?;
            }
        }

        Ok(order)
    }

    fn visit<'a>(
        &self,
        node: &'a str,
        deps: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashMap<&'a str, bool>,
        order: &mut Vec<String>,
    ) -> Result<(), CompositionError> {
        if let Some(&done) = visited.get(node) {
            if !done {
                return Err(CompositionError::CircularDependency(vec![node.to_string()]));
            }
            return Ok(());
        }

        visited.insert(node, false);

        if let Some(node_deps) = deps.get(node) {
            for &dep in node_deps {
                self.visit(dep, deps, visited, order)?;
            }
        }

        visited.insert(node, true);
        order.push(node.to_string());
        Ok(())
    }

    /// Get a registered component.
    pub fn get_component(&self, name: &str) -> Option<&ComponentDescriptor> {
        self.components.get(name)
    }

    /// Get a registered interface.
    pub fn get_interface(&self, name: &str) -> Option<&WitInterface> {
        self.interfaces.get(name)
    }

    /// List all registered components.
    pub fn list_components(&self) -> Vec<&str> {
        self.components.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ComponentComposer {
    fn default() -> Self {
        Self::new()
    }
}

/// Semantic version range matching for interface compatibility.
pub struct VersionMatcher;

impl VersionMatcher {
    /// Check if an available version satisfies a constraint.
    ///
    /// Supports exact match ("1.0.0"), wildcard ("*"), caret ("^1.2"),
    /// and tilde ("~1.2") constraints.
    pub fn satisfies(available: &str, constraint: &str) -> bool {
        if constraint == "*" || constraint.is_empty() {
            return true;
        }

        let (prefix, constraint_version) = if let Some(stripped) = constraint.strip_prefix('^') {
            ("^", stripped)
        } else if let Some(stripped) = constraint.strip_prefix('~') {
            ("~", stripped)
        } else {
            ("=", constraint)
        };

        let avail_parts = Self::parse_semver(available);
        let constraint_parts = Self::parse_semver(constraint_version);

        match prefix {
            "=" => avail_parts == constraint_parts,
            "^" => {
                // Compatible with: same major, >= minor.patch
                avail_parts.0 == constraint_parts.0
                    && (avail_parts.1 > constraint_parts.1
                        || (avail_parts.1 == constraint_parts.1
                            && avail_parts.2 >= constraint_parts.2))
            }
            "~" => {
                // Approximately: same major.minor, >= patch
                avail_parts.0 == constraint_parts.0
                    && avail_parts.1 == constraint_parts.1
                    && avail_parts.2 >= constraint_parts.2
            }
            _ => avail_parts == constraint_parts,
        }
    }

    fn parse_semver(version: &str) -> (u32, u32, u32) {
        let parts: Vec<u32> =
            version.split('.').filter_map(|s| s.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    }
}

/// Result of validating a composition graph before execution.
#[derive(Debug, Clone)]
pub struct CompositionValidation {
    /// Whether the composition is valid and can execute.
    pub is_valid: bool,
    /// Critical errors that prevent execution.
    pub errors: Vec<String>,
    /// Non-critical warnings.
    pub warnings: Vec<String>,
    /// Total number of components.
    pub component_count: usize,
    /// Total number of resolved bindings.
    pub binding_count: usize,
    /// Maximum depth of the dependency chain.
    pub max_depth: usize,
}

impl ComponentComposer {
    /// Validate the full composition graph with version compatibility checks.
    pub fn validate(&self) -> CompositionValidation {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Check for version compatibility on all bindings
        let mut export_versions: HashMap<&str, (&str, &str)> = HashMap::new();
        for (name, comp) in &self.components {
            for export in &comp.exports {
                export_versions.insert(&export.interface, (name, &export.version_constraint));
            }
        }

        for (name, comp) in &self.components {
            for import in &comp.imports {
                if let Some(&(provider, export_version)) =
                    export_versions.get(import.interface.as_str())
                {
                    if !VersionMatcher::satisfies(export_version, &import.version_constraint) {
                        errors.push(format!(
                            "Version mismatch: {} requires {} {} but {} provides {}",
                            name,
                            import.interface,
                            import.version_constraint,
                            provider,
                            export_version
                        ));
                    }
                } else if !import.optional {
                    errors.push(format!(
                        "Unresolved import: {} requires {}",
                        name, import.interface
                    ));
                }
            }
        }

        // Calculate max dependency depth
        let max_depth = self.calculate_max_depth();

        let is_valid = errors.is_empty();
        if self.components.len() > 20 {
            warnings.push(format!(
                "Large composition graph ({} components) may impact startup time",
                self.components.len()
            ));
        }

        CompositionValidation {
            is_valid,
            errors,
            warnings,
            component_count: self.components.len(),
            binding_count: self.components.values().map(|c| c.imports.len()).sum(),
            max_depth,
        }
    }

    fn calculate_max_depth(&self) -> usize {
        let mut export_index: HashMap<&str, &str> = HashMap::new();
        for (name, comp) in &self.components {
            for export in &comp.exports {
                export_index.insert(&export.interface, name);
            }
        }

        let mut max_depth = 0;
        for name in self.components.keys() {
            let depth = self.depth_of(name, &export_index, &mut HashMap::new());
            if depth > max_depth {
                max_depth = depth;
            }
        }
        max_depth
    }

    fn depth_of<'a>(
        &self,
        name: &'a str,
        export_index: &HashMap<&str, &'a str>,
        cache: &mut HashMap<&'a str, usize>,
    ) -> usize {
        if let Some(&cached) = cache.get(name) {
            return cached;
        }

        let Some(comp) = self.components.get(name) else {
            return 0;
        };

        let mut max_dep = 0;
        for import in &comp.imports {
            if let Some(&provider) = export_index.get(import.interface.as_str()) {
                if provider != name {
                    let dep_depth = self.depth_of(provider, export_index, cache);
                    if dep_depth + 1 > max_dep {
                        max_dep = dep_depth + 1;
                    }
                }
            }
        }

        cache.insert(name, max_dep);
        max_dep
    }
}

/// Registry for discovering and sharing WIT interfaces.
pub struct WitRegistry {
    interfaces: HashMap<String, Vec<WitInterface>>,
}

impl WitRegistry {
    /// Create a new WIT registry.
    pub fn new() -> Self {
        Self { interfaces: HashMap::new() }
    }

    /// Publish an interface version.
    pub fn publish(&mut self, interface: WitInterface) {
        self.interfaces.entry(interface.name.clone()).or_default().push(interface);
    }

    /// Look up an interface by name and optional version.
    pub fn lookup(&self, name: &str, version: Option<&str>) -> Option<&WitInterface> {
        let versions = self.interfaces.get(name)?;
        match version {
            Some(v) => versions.iter().find(|iface| iface.version == v),
            None => versions.last(),
        }
    }

    /// Search interfaces by name prefix.
    pub fn search(&self, prefix: &str) -> Vec<&WitInterface> {
        self.interfaces
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .flat_map(|(_, versions)| versions.last())
            .collect()
    }

    /// Count total interfaces.
    pub fn count(&self) -> usize {
        self.interfaces.len()
    }
}

impl Default for WitRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_interface(name: &str) -> WitInterface {
        WitInterface {
            name: name.to_string(),
            package: "test".to_string(),
            version: "0.1.0".to_string(),
            functions: vec![WitFunction {
                name: "do-something".to_string(),
                params: vec![WitParam { name: "input".to_string(), ty: WitValueType::String }],
                results: vec![WitParam { name: "output".to_string(), ty: WitValueType::U32 }],
                docs: None,
            }],
            types: Vec::new(),
        }
    }

    fn test_component(
        name: &str,
        imports: Vec<&str>,
        exports: Vec<&str>,
    ) -> ComponentDescriptor {
        ComponentDescriptor {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            imports: imports
                .into_iter()
                .map(|i| InterfaceRef {
                    interface: i.to_string(),
                    version_constraint: "*".to_string(),
                    optional: false,
                })
                .collect(),
            exports: exports
                .into_iter()
                .map(|e| InterfaceRef {
                    interface: e.to_string(),
                    version_constraint: "0.1.0".to_string(),
                    optional: false,
                })
                .collect(),
            module_hash: "abc123".to_string(),
            size: 1024,
        }
    }

    #[test]
    fn test_compose_simple() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("auth", vec![], vec!["auth-api"]));
        composer.register_component(test_component("app", vec!["auth-api"], vec![]));

        let result = composer.compose().unwrap();
        assert_eq!(result.component_order.len(), 2);
        assert_eq!(result.bindings.len(), 1);
        assert!(result.unresolved.is_empty());

        // Auth should come before app in order
        let auth_idx = result.component_order.iter().position(|n| n == "auth").unwrap();
        let app_idx = result.component_order.iter().position(|n| n == "app").unwrap();
        assert!(auth_idx < app_idx);
    }

    #[test]
    fn test_compose_three_components() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("db", vec![], vec!["storage"]));
        composer.register_component(test_component("auth", vec!["storage"], vec!["auth-api"]));
        composer.register_component(test_component("app", vec!["auth-api", "storage"], vec![]));

        let result = composer.compose().unwrap();
        assert_eq!(result.component_order.len(), 3);
        assert_eq!(result.bindings.len(), 3);
    }

    #[test]
    fn test_compose_circular_dependency() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("a", vec!["b-api"], vec!["a-api"]));
        composer.register_component(test_component("b", vec!["a-api"], vec!["b-api"]));

        let result = composer.compose();
        assert!(matches!(result, Err(CompositionError::CircularDependency(_))));
    }

    #[test]
    fn test_compose_duplicate_export() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("a", vec![], vec!["shared"]));
        composer.register_component(test_component("b", vec![], vec!["shared"]));

        let result = composer.compose();
        assert!(matches!(result, Err(CompositionError::DuplicateExport { .. })));
    }

    #[test]
    fn test_wit_registry() {
        let mut registry = WitRegistry::new();
        registry.publish(test_interface("wasi:filesystem/types"));
        registry.publish(test_interface("wasi:http/types"));

        assert_eq!(registry.count(), 2);
        assert!(registry.lookup("wasi:filesystem/types", None).is_some());
        assert!(registry.lookup("nonexistent", None).is_none());

        let wasi = registry.search("wasi:");
        assert_eq!(wasi.len(), 2);
    }

    #[test]
    fn test_wit_value_types() {
        let ty = WitValueType::Result {
            ok: Some(Box::new(WitValueType::String)),
            err: Some(Box::new(WitValueType::U32)),
        };
        assert_eq!(
            ty,
            WitValueType::Result {
                ok: Some(Box::new(WitValueType::String)),
                err: Some(Box::new(WitValueType::U32)),
            }
        );
    }

    #[test]
    fn test_component_descriptor() {
        let comp = test_component("myapp", vec!["auth"], vec!["api"]);
        assert_eq!(comp.name, "myapp");
        assert_eq!(comp.imports.len(), 1);
        assert_eq!(comp.exports.len(), 1);
    }

    #[test]
    fn test_version_matcher_exact() {
        assert!(VersionMatcher::satisfies("1.0.0", "1.0.0"));
        assert!(!VersionMatcher::satisfies("1.0.1", "1.0.0"));
    }

    #[test]
    fn test_version_matcher_wildcard() {
        assert!(VersionMatcher::satisfies("1.0.0", "*"));
        assert!(VersionMatcher::satisfies("99.99.99", "*"));
        assert!(VersionMatcher::satisfies("0.0.1", ""));
    }

    #[test]
    fn test_version_matcher_caret() {
        assert!(VersionMatcher::satisfies("1.2.0", "^1.0"));
        assert!(VersionMatcher::satisfies("1.5.3", "^1.2"));
        assert!(!VersionMatcher::satisfies("2.0.0", "^1.2"));
        assert!(!VersionMatcher::satisfies("0.9.0", "^1.0"));
    }

    #[test]
    fn test_version_matcher_tilde() {
        assert!(VersionMatcher::satisfies("1.2.3", "~1.2.0"));
        assert!(VersionMatcher::satisfies("1.2.9", "~1.2.0"));
        assert!(!VersionMatcher::satisfies("1.3.0", "~1.2.0"));
    }

    #[test]
    fn test_composition_validation_valid() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("auth", vec![], vec!["auth-api"]));
        composer.register_component(test_component("app", vec!["auth-api"], vec![]));

        let validation = composer.validate();
        assert!(validation.is_valid);
        assert_eq!(validation.component_count, 2);
        assert!(validation.errors.is_empty());
        assert_eq!(validation.max_depth, 1);
    }

    #[test]
    fn test_composition_validation_unresolved() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("app", vec!["missing-api"], vec![]));

        let validation = composer.validate();
        assert!(!validation.is_valid);
        assert!(validation.errors.iter().any(|e| e.contains("Unresolved")));
    }

    #[test]
    fn test_composition_max_depth_chain() {
        let mut composer = ComponentComposer::new();
        composer.register_component(test_component("db", vec![], vec!["storage"]));
        composer.register_component(test_component("auth", vec!["storage"], vec!["auth-api"]));
        composer.register_component(test_component("app", vec!["auth-api"], vec![]));

        let validation = composer.validate();
        assert!(validation.is_valid);
        assert_eq!(validation.max_depth, 2);
    }
}
