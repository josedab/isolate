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

        // In production, would actually compose the WASM modules
        Ok(ComposedModule {
            bytes: Vec::new(),
            components: self.config.components.keys().cloned().collect(),
            links: self.config.links.clone(),
        })
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
    CycleDetected,
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentNotFound(id) => write!(f, "Component not found: {}", id),
            Self::NoRootComponent => write!(f, "No root component set"),
            Self::InterfaceMismatch(a, b) => write!(f, "Interface mismatch: {} vs {}", a, b),
            Self::CycleDetected => write!(f, "Cycle detected in composition"),
        }
    }
}

impl std::error::Error for ComposeError {}

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
}
