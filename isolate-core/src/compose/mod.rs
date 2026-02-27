//! WebAssembly Component Composition
//!
//! Compose multiple WASM modules using the Component Model standard.
//! Link modules together, define interfaces, and create modular sandboxes.
//!
//! ## Sub-modules
//!
//! - [`linker`] -- Module linking and composition: dependency graphs,
//!   topological sorting, import/export resolution, and type checking.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod linker;

pub use linker::{
    CompositionGraph, ExportType, ImportType, LinkError, LinkedComposition, LinkedModule,
    ModuleExport, ModuleImport, ModuleInterface, ModuleLinker, ValueType,
};

/// A component interface definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    /// Interface name.
    pub name: String,
    /// Functions exposed.
    pub functions: Vec<FunctionSignature>,
    /// Types defined.
    pub types: Vec<TypeDefinition>,
}

/// Function signature in an interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSignature {
    /// Function name.
    pub name: String,
    /// Parameter types.
    pub params: Vec<(String, WitType)>,
    /// Return type.
    pub returns: Option<WitType>,
}

/// WIT (WebAssembly Interface Type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WitType {
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
    List(Box<WitType>),
    Option(Box<WitType>),
    Result { ok: Option<Box<WitType>>, err: Option<Box<WitType>> },
    Record(String),
    Variant(String),
    Enum(String),
    Flags(String),
    Tuple(Vec<WitType>),
}

/// Type definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDefinition {
    /// Type name.
    pub name: String,
    /// Type kind.
    pub kind: TypeKind,
}

/// Kind of type definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeKind {
    Record(Vec<(String, WitType)>),
    Variant(Vec<(String, Option<WitType>)>),
    Enum(Vec<String>),
    Flags(Vec<String>),
    Alias(WitType),
}

/// A component in the composition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    /// Component ID.
    pub id: String,
    /// Component name.
    pub name: String,
    /// Exported interfaces.
    pub exports: Vec<Interface>,
    /// Imported interfaces.
    pub imports: Vec<Interface>,
    /// Component bytes.
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

/// A link between components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentLink {
    /// Source component.
    pub from: String,
    /// Source interface.
    pub from_interface: String,
    /// Target component.
    pub to: String,
    /// Target interface.
    pub to_interface: String,
}

/// Composed component configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompositionConfig {
    /// Root component ID.
    pub root: Option<String>,
    /// Components in composition.
    pub components: HashMap<String, Component>,
    /// Links between components.
    pub links: Vec<ComponentLink>,
    /// Entry function.
    pub entry: Option<String>,
}

/// Component composer.
pub struct Composer {
    config: CompositionConfig,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// Create a new composer.
    pub fn new() -> Self {
        Self { config: CompositionConfig::default() }
    }

    /// Add a component.
    pub fn add_component(&mut self, component: Component) {
        self.config.components.insert(component.id.clone(), component);
    }

    /// Link two components.
    pub fn link(&mut self, link: ComponentLink) -> Result<(), ComposeError> {
        // Validate link
        if !self.config.components.contains_key(&link.from) {
            return Err(ComposeError::ComponentNotFound(link.from));
        }
        if !self.config.components.contains_key(&link.to) {
            return Err(ComposeError::ComponentNotFound(link.to));
        }

        self.config.links.push(link);
        Ok(())
    }

    /// Set root component.
    pub fn set_root(&mut self, component_id: &str) -> Result<(), ComposeError> {
        if !self.config.components.contains_key(component_id) {
            return Err(ComposeError::ComponentNotFound(component_id.to_string()));
        }
        self.config.root = Some(component_id.to_string());
        Ok(())
    }

    /// Compose components into a single module.
    pub fn compose(&self) -> Result<ComposedModule, ComposeError> {
        if self.config.root.is_none() {
            return Err(ComposeError::NoRootComponent);
        }

        // Validate all links
        for link in &self.config.links {
            self.validate_link(link)?;
        }

        // Check for unresolved imports
        self.check_unresolved_imports()?;

        // Check for cycles using topological sort
        let execution_order = self.topological_sort()?;

        Ok(ComposedModule {
            bytes: Vec::new(),
            components: execution_order,
            links: self.config.links.clone(),
        })
    }

    /// Perform topological sort to determine execution order and detect cycles.
    fn topological_sort(&self) -> Result<Vec<String>, ComposeError> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for id in self.config.components.keys() {
            in_degree.entry(id.clone()).or_insert(0);
            adj.entry(id.clone()).or_default();
        }

        for link in &self.config.links {
            adj.entry(link.from.clone()).or_default().push(link.to.clone());
            *in_degree.entry(link.to.clone()).or_insert(0) += 1;
        }

        let mut queue: Vec<String> =
            in_degree.iter().filter(|(_, &d)| d == 0).map(|(k, _)| k.clone()).collect();
        queue.sort();

        let mut order = Vec::new();
        while let Some(node) = queue.pop() {
            order.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            let pos = queue.binary_search(neighbor).unwrap_or_else(|p| p);
                            queue.insert(pos, neighbor.clone());
                        }
                    }
                }
            }
        }

        if order.len() != self.config.components.len() {
            let in_order: std::collections::HashSet<&str> =
                order.iter().map(|s| s.as_str()).collect();
            let cycle: Vec<String> = self
                .config
                .components
                .keys()
                .filter(|k| !in_order.contains(k.as_str()))
                .cloned()
                .collect();
            return Err(ComposeError::CycleDetected(cycle));
        }

        Ok(order)
    }

    /// Check for unresolved imports (imports with no matching link).
    fn check_unresolved_imports(&self) -> Result<(), ComposeError> {
        let linked_imports: std::collections::HashSet<(&str, &str)> =
            self.config.links.iter().map(|l| (l.to.as_str(), l.to_interface.as_str())).collect();

        for (id, component) in &self.config.components {
            for import in &component.imports {
                if Some(id.as_str()) == self.config.root.as_deref() {
                    continue; // root imports are provided by the host
                }
                if !linked_imports.contains(&(id.as_str(), import.name.as_str())) {
                    // Check if any other component provides this interface
                    let provided =
                        self.config.components.values().any(|c| {
                            c.id != *id && c.exports.iter().any(|e| e.name == import.name)
                        });
                    if !provided {
                        return Err(ComposeError::UnresolvedImport {
                            component: id.clone(),
                            interface: import.name.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate interface type compatibility between linked components.
    pub fn validate_types(&self) -> Vec<ComposeError> {
        let mut errors = Vec::new();
        for link in &self.config.links {
            let from = self.config.components.get(&link.from);
            let to = self.config.components.get(&link.to);

            if let (Some(from), Some(to)) = (from, to) {
                let export = from.exports.iter().find(|i| i.name == link.from_interface);
                let import = to.imports.iter().find(|i| i.name == link.to_interface);

                if let (Some(exp), Some(imp)) = (export, import) {
                    // Check function counts match
                    if exp.functions.len() != imp.functions.len() {
                        errors.push(ComposeError::TypeMismatch {
                            expected: format!("{} functions", imp.functions.len()),
                            actual: format!("{} functions", exp.functions.len()),
                        });
                    }
                    // Check function signatures match
                    for (ef, imf) in exp.functions.iter().zip(imp.functions.iter()) {
                        if ef.params.len() != imf.params.len() || ef.returns != imf.returns {
                            errors.push(ComposeError::TypeMismatch {
                                expected: format!("{}({})", imf.name, imf.params.len()),
                                actual: format!("{}({})", ef.name, ef.params.len()),
                            });
                        }
                    }
                }
            }
        }
        errors
    }

    fn validate_link(&self, link: &ComponentLink) -> Result<(), ComposeError> {
        let from = self
            .config
            .components
            .get(&link.from)
            .ok_or_else(|| ComposeError::ComponentNotFound(link.from.clone()))?;
        let to = self
            .config
            .components
            .get(&link.to)
            .ok_or_else(|| ComposeError::ComponentNotFound(link.to.clone()))?;

        // Check interface compatibility
        let export = from.exports.iter().find(|i| i.name == link.from_interface);
        let import = to.imports.iter().find(|i| i.name == link.to_interface);

        if export.is_none() || import.is_none() {
            return Err(ComposeError::InterfaceMismatch(
                link.from_interface.clone(),
                link.to_interface.clone(),
            ));
        }

        Ok(())
    }
}

/// Composed module output.
#[derive(Debug, Clone)]
pub struct ComposedModule {
    /// Composed WASM bytes.
    pub bytes: Vec<u8>,
    /// Component IDs included.
    pub components: Vec<String>,
    /// Links applied.
    pub links: Vec<ComponentLink>,
}

/// Composition error.
#[derive(Debug, Clone)]
pub enum ComposeError {
    /// Component not found.
    ComponentNotFound(String),
    /// No root component set.
    NoRootComponent,
    /// Interface mismatch.
    InterfaceMismatch(String, String),
    /// Cycle detected.
    CycleDetected(Vec<String>),
    /// Type mismatch between linked interfaces.
    TypeMismatch { expected: String, actual: String },
    /// Unresolved import.
    UnresolvedImport { component: String, interface: String },
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentNotFound(id) => write!(f, "Component not found: {}", id),
            Self::NoRootComponent => write!(f, "No root component set"),
            Self::InterfaceMismatch(a, b) => write!(f, "Interface mismatch: {} vs {}", a, b),
            Self::CycleDetected(modules) => {
                write!(f, "Cycle detected among: {}", modules.join(", "))
            }
            Self::TypeMismatch { expected, actual } => {
                write!(f, "Type mismatch: expected {}, got {}", expected, actual)
            }
            Self::UnresolvedImport { component, interface } => {
                write!(f, "Unresolved import: {} requires {}", component, interface)
            }
        }
    }
}

impl std::error::Error for ComposeError {}

// ---------------------------------------------------------------------------
// WIT Interface Parser
// ---------------------------------------------------------------------------

/// Parses a simplified WIT interface definition into an [`Interface`].
///
/// Supports a subset of the WIT syntax:
/// ```text
/// interface my-api {
///     greet: func(name: string) -> string
///     add: func(a: u32, b: u32) -> u32
/// }
/// ```
pub struct WitParser;

impl WitParser {
    /// Parse a WIT interface definition string into an [`Interface`].
    pub fn parse_interface(input: &str) -> Result<Interface, ComposeError> {
        let input = input.trim();
        let (name, body) = Self::extract_block(input, "interface")?;
        let mut functions = Vec::new();
        let mut types = Vec::new();

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            if line.contains(": func(") || line.contains(": func()") {
                functions.push(Self::parse_function(line)?);
            } else if line.starts_with("record ")
                || line.starts_with("enum ")
                || line.starts_with("flags ")
            {
                types.push(Self::parse_type_def(line)?);
            }
        }

        Ok(Interface { name, functions, types })
    }

    fn extract_block(input: &str, keyword: &str) -> Result<(String, String), ComposeError> {
        let prefix = format!("{} ", keyword);
        if !input.starts_with(&prefix) {
            return Err(ComposeError::InterfaceMismatch(
                format!("expected '{keyword}' block"),
                input.chars().take(20).collect(),
            ));
        }
        let rest = &input[prefix.len()..];
        let brace_start = rest.find('{').ok_or_else(|| {
            ComposeError::InterfaceMismatch("missing '{'".to_string(), rest.to_string())
        })?;
        let name = rest[..brace_start].trim().to_string();
        let brace_end = rest.rfind('}').ok_or_else(|| {
            ComposeError::InterfaceMismatch("missing '}'".to_string(), rest.to_string())
        })?;
        let body = rest[brace_start + 1..brace_end].to_string();
        Ok((name, body))
    }

    fn parse_function(line: &str) -> Result<FunctionSignature, ComposeError> {
        let colon_pos = line.find(':').ok_or_else(|| {
            ComposeError::InterfaceMismatch("missing ':'".to_string(), line.to_string())
        })?;
        let name = line[..colon_pos].trim().to_string();
        let rest = line[colon_pos + 1..].trim();

        // Parse "func(params) -> return_type" or "func(params)"
        let func_prefix = "func(";
        if !rest.starts_with(func_prefix) {
            return Err(ComposeError::InterfaceMismatch(
                "expected 'func('".to_string(),
                rest.to_string(),
            ));
        }
        let paren_close = rest.find(')').ok_or_else(|| {
            ComposeError::InterfaceMismatch("missing ')'".to_string(), rest.to_string())
        })?;
        let params_str = &rest[func_prefix.len()..paren_close];
        let params = Self::parse_params(params_str);

        let returns = if let Some(arrow_pos) = rest.find("->") {
            let ret_str = rest[arrow_pos + 2..].trim();
            Some(Self::parse_wit_type(ret_str))
        } else {
            None
        };

        Ok(FunctionSignature { name, params, returns })
    }

    fn parse_params(params_str: &str) -> Vec<(String, WitType)> {
        if params_str.trim().is_empty() {
            return Vec::new();
        }
        params_str
            .split(',')
            .filter_map(|p| {
                let p = p.trim();
                let colon = p.find(':')?;
                let name = p[..colon].trim().to_string();
                let ty = Self::parse_wit_type(p[colon + 1..].trim());
                Some((name, ty))
            })
            .collect()
    }

    fn parse_wit_type(s: &str) -> WitType {
        match s.trim() {
            "bool" => WitType::Bool,
            "u8" => WitType::U8,
            "u16" => WitType::U16,
            "u32" => WitType::U32,
            "u64" => WitType::U64,
            "s8" => WitType::S8,
            "s16" => WitType::S16,
            "s32" => WitType::S32,
            "s64" => WitType::S64,
            "f32" => WitType::F32,
            "f64" => WitType::F64,
            "char" => WitType::Char,
            "string" => WitType::String,
            other if other.starts_with("list<") => {
                let inner = &other[5..other.len() - 1];
                WitType::List(Box::new(Self::parse_wit_type(inner)))
            }
            other if other.starts_with("option<") => {
                let inner = &other[7..other.len() - 1];
                WitType::Option(Box::new(Self::parse_wit_type(inner)))
            }
            other => WitType::Record(other.to_string()),
        }
    }

    fn parse_type_def(line: &str) -> Result<TypeDefinition, ComposeError> {
        if let Some(rest) = line.strip_prefix("enum ") {
            let name = rest.trim().to_string();
            Ok(TypeDefinition { name, kind: TypeKind::Enum(Vec::new()) })
        } else if let Some(rest) = line.strip_prefix("flags ") {
            let name = rest.trim().to_string();
            Ok(TypeDefinition { name, kind: TypeKind::Flags(Vec::new()) })
        } else if let Some(rest) = line.strip_prefix("record ") {
            let name = rest.trim().to_string();
            Ok(TypeDefinition { name, kind: TypeKind::Record(Vec::new()) })
        } else {
            Err(ComposeError::InterfaceMismatch(
                "unknown type definition".to_string(),
                line.to_string(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Capability boundaries for cross-component permissions
// ---------------------------------------------------------------------------

/// Capability boundary configuration for a component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentCapabilities {
    /// Component ID.
    pub component_id: String,
    /// Capabilities granted to this component.
    pub granted: Vec<String>,
    /// Whether this component can delegate capabilities to linked components.
    pub can_delegate: bool,
}

/// Validates capability boundaries across component links.
pub struct CapabilityBoundaryValidator;

impl CapabilityBoundaryValidator {
    /// Validate that all component links respect capability boundaries.
    ///
    /// A component can only provide an interface to another component if it
    /// has the capabilities required by that interface.
    pub fn validate(
        config: &CompositionConfig,
        boundaries: &[ComponentCapabilities],
    ) -> Vec<CapabilityBoundaryViolation> {
        let mut violations = Vec::new();
        let cap_map: HashMap<String, &ComponentCapabilities> =
            boundaries.iter().map(|b| (b.component_id.clone(), b)).collect();

        for link in &config.links {
            // The provider (from) must have capabilities for the interface it exports
            if let Some(from_caps) = cap_map.get(&link.from) {
                if let Some(to_caps) = cap_map.get(&link.to) {
                    // Check if the provider can delegate to the consumer
                    if !from_caps.can_delegate {
                        violations.push(CapabilityBoundaryViolation {
                            from_component: link.from.clone(),
                            to_component: link.to.clone(),
                            interface: link.from_interface.clone(),
                            reason: format!(
                                "Component '{}' cannot delegate capabilities",
                                link.from
                            ),
                        });
                    }

                    // Check that consumer has at least read access
                    let has_access =
                        to_caps.granted.iter().any(|g| g == &link.to_interface || g == "*");
                    if !has_access && !to_caps.granted.is_empty() {
                        violations.push(CapabilityBoundaryViolation {
                            from_component: link.from.clone(),
                            to_component: link.to.clone(),
                            interface: link.to_interface.clone(),
                            reason: format!(
                                "Component '{}' lacks capability for interface '{}'",
                                link.to, link.to_interface
                            ),
                        });
                    }
                }
            }
        }

        violations
    }
}

/// A capability boundary violation in a composition.
#[derive(Debug, Clone)]
pub struct CapabilityBoundaryViolation {
    /// Source component.
    pub from_component: String,
    /// Target component.
    pub to_component: String,
    /// Interface in question.
    pub interface: String,
    /// Human-readable reason.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_component(id: &str) -> Component {
        Component {
            id: id.to_string(),
            name: id.to_string(),
            exports: vec![Interface { name: "api".to_string(), functions: vec![], types: vec![] }],
            imports: vec![Interface { name: "api".to_string(), functions: vec![], types: vec![] }],
            bytes: vec![],
        }
    }

    #[test]
    fn test_composer_add_component() {
        let mut composer = Composer::new();
        composer.add_component(create_test_component("comp-1"));
        assert!(composer.config.components.contains_key("comp-1"));
    }

    #[test]
    fn test_composer_link() {
        let mut composer = Composer::new();
        composer.add_component(create_test_component("comp-1"));
        composer.add_component(create_test_component("comp-2"));

        let link = ComponentLink {
            from: "comp-1".to_string(),
            from_interface: "api".to_string(),
            to: "comp-2".to_string(),
            to_interface: "api".to_string(),
        };

        assert!(composer.link(link).is_ok());
    }

    #[test]
    fn test_composer_compose() {
        let mut composer = Composer::new();
        composer.add_component(create_test_component("root"));
        composer.set_root("root").unwrap();

        let composed = composer.compose().unwrap();
        assert_eq!(composed.components.len(), 1);
    }

    #[test]
    fn test_composer_cycle_detection() {
        let mut composer = Composer::new();

        // Both components export and import the same "shared" interface
        let a = Component {
            id: "a".to_string(),
            name: "a".to_string(),
            exports: vec![Interface {
                name: "from-a".to_string(),
                functions: vec![],
                types: vec![],
            }],
            imports: vec![Interface {
                name: "from-b".to_string(),
                functions: vec![],
                types: vec![],
            }],
            bytes: vec![],
        };
        let b = Component {
            id: "b".to_string(),
            name: "b".to_string(),
            exports: vec![Interface {
                name: "from-b".to_string(),
                functions: vec![],
                types: vec![],
            }],
            imports: vec![Interface {
                name: "from-a".to_string(),
                functions: vec![],
                types: vec![],
            }],
            bytes: vec![],
        };

        composer.add_component(a);
        composer.add_component(b);
        composer.set_root("a").unwrap();

        // a exports from-a → b imports from-a
        composer
            .link(ComponentLink {
                from: "a".to_string(),
                from_interface: "from-a".to_string(),
                to: "b".to_string(),
                to_interface: "from-a".to_string(),
            })
            .unwrap();
        // b exports from-b → a imports from-b
        composer
            .link(ComponentLink {
                from: "b".to_string(),
                from_interface: "from-b".to_string(),
                to: "a".to_string(),
                to_interface: "from-b".to_string(),
            })
            .unwrap();

        let result = composer.compose();
        match &result {
            Err(ComposeError::CycleDetected(_)) => {} // expected
            other => panic!("Expected CycleDetected, got {:?}", other),
        }
    }

    #[test]
    fn test_composer_topological_order() {
        let mut composer = Composer::new();

        let mut app = create_test_component("app");
        app.imports =
            vec![Interface { name: "logger-api".to_string(), functions: vec![], types: vec![] }];

        let logger = create_test_component("logger");

        composer.add_component(app);
        composer.add_component(logger);
        composer.set_root("app").unwrap();

        composer
            .link(ComponentLink {
                from: "logger".to_string(),
                from_interface: "api".to_string(),
                to: "app".to_string(),
                to_interface: "logger-api".to_string(),
            })
            .unwrap();

        let composed = composer.compose().unwrap();
        // logger should come before app in execution order
        let logger_pos = composed.components.iter().position(|c| c == "logger");
        let app_pos = composed.components.iter().position(|c| c == "app");
        assert!(logger_pos < app_pos);
    }

    #[test]
    fn test_composer_validate_types() {
        let mut composer = Composer::new();

        let mut a = create_test_component("a");
        a.exports = vec![Interface {
            name: "api".to_string(),
            functions: vec![FunctionSignature {
                name: "greet".to_string(),
                params: vec![("name".to_string(), WitType::String)],
                returns: Some(WitType::String),
            }],
            types: vec![],
        }];

        let mut b = create_test_component("b");
        b.imports = vec![Interface {
            name: "api".to_string(),
            functions: vec![FunctionSignature {
                name: "greet".to_string(),
                params: vec![("name".to_string(), WitType::String)],
                returns: Some(WitType::String),
            }],
            types: vec![],
        }];

        composer.add_component(a);
        composer.add_component(b);

        composer
            .link(ComponentLink {
                from: "a".to_string(),
                from_interface: "api".to_string(),
                to: "b".to_string(),
                to_interface: "api".to_string(),
            })
            .unwrap();

        let errors = composer.validate_types();
        assert!(errors.is_empty(), "Expected no type errors, got {:?}", errors);
    }

    #[test]
    fn test_compose_error_display() {
        let err = ComposeError::CycleDetected(vec!["a".to_string(), "b".to_string()]);
        assert!(err.to_string().contains("a, b"));

        let err = ComposeError::UnresolvedImport {
            component: "app".to_string(),
            interface: "logger".to_string(),
        };
        assert!(err.to_string().contains("app"));
    }

    // -----------------------------------------------------------------------
    // WIT Parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_wit_parser_simple_interface() {
        let wit = r#"interface greeter {
            greet: func(name: string) -> string
        }"#;
        let iface = WitParser::parse_interface(wit).unwrap();
        assert_eq!(iface.name, "greeter");
        assert_eq!(iface.functions.len(), 1);
        assert_eq!(iface.functions[0].name, "greet");
        assert_eq!(iface.functions[0].params.len(), 1);
        assert_eq!(iface.functions[0].params[0].0, "name");
        assert_eq!(iface.functions[0].params[0].1, WitType::String);
        assert_eq!(iface.functions[0].returns, Some(WitType::String));
    }

    #[test]
    fn test_wit_parser_multiple_params() {
        let wit = r#"interface math {
            add: func(a: u32, b: u32) -> u32
        }"#;
        let iface = WitParser::parse_interface(wit).unwrap();
        assert_eq!(iface.functions[0].params.len(), 2);
        assert_eq!(iface.functions[0].params[0].1, WitType::U32);
        assert_eq!(iface.functions[0].returns, Some(WitType::U32));
    }

    #[test]
    fn test_wit_parser_no_return() {
        let wit = r#"interface logger {
            log: func(msg: string)
        }"#;
        let iface = WitParser::parse_interface(wit).unwrap();
        assert!(iface.functions[0].returns.is_none());
    }

    #[test]
    fn test_wit_parser_list_type() {
        let wit = r#"interface data {
            process: func(items: list<u32>) -> list<string>
        }"#;
        let iface = WitParser::parse_interface(wit).unwrap();
        assert_eq!(iface.functions[0].params[0].1, WitType::List(Box::new(WitType::U32)));
    }

    #[test]
    fn test_wit_parser_with_comments() {
        let wit = r#"interface api {
            // This is a comment
            hello: func() -> string
        }"#;
        let iface = WitParser::parse_interface(wit).unwrap();
        assert_eq!(iface.functions.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Capability boundary tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_capability_boundary_no_violations() {
        let config = CompositionConfig {
            root: Some("app".to_string()),
            components: HashMap::new(),
            links: vec![ComponentLink {
                from: "logger".to_string(),
                from_interface: "log-api".to_string(),
                to: "app".to_string(),
                to_interface: "log-api".to_string(),
            }],
            entry: None,
        };

        let boundaries = vec![
            ComponentCapabilities {
                component_id: "logger".to_string(),
                granted: vec!["log-api".to_string()],
                can_delegate: true,
            },
            ComponentCapabilities {
                component_id: "app".to_string(),
                granted: vec!["log-api".to_string()],
                can_delegate: false,
            },
        ];

        let violations = CapabilityBoundaryValidator::validate(&config, &boundaries);
        assert!(violations.is_empty());
    }

    #[test]
    fn test_capability_boundary_delegation_violation() {
        let config = CompositionConfig {
            root: Some("app".to_string()),
            components: HashMap::new(),
            links: vec![ComponentLink {
                from: "data".to_string(),
                from_interface: "data-api".to_string(),
                to: "app".to_string(),
                to_interface: "data-api".to_string(),
            }],
            entry: None,
        };

        let boundaries = vec![
            ComponentCapabilities {
                component_id: "data".to_string(),
                granted: vec!["data-api".to_string()],
                can_delegate: false, // Cannot delegate!
            },
            ComponentCapabilities {
                component_id: "app".to_string(),
                granted: vec!["data-api".to_string()],
                can_delegate: false,
            },
        ];

        let violations = CapabilityBoundaryValidator::validate(&config, &boundaries);
        assert!(!violations.is_empty());
        assert!(violations[0].reason.contains("cannot delegate"));
    }
}
