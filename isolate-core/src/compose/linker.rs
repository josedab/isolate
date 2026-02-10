//! Module linking and composition for WebAssembly modules.
//!
//! This module provides types and logic for describing module interfaces,
//! resolving imports/exports between modules, building dependency graphs,
//! and producing a linked composition with a valid execution order.
//!
//! # Example
//!
//! ```rust
//! use isolate_core::compose::linker::*;
//!
//! let mut linker = ModuleLinker::new();
//!
//! // Register a "logger" module that exports a "log" function.
//! let logger = ModuleInterface {
//!     name: "logger".to_string(),
//!     imports: vec![],
//!     exports: vec![ModuleExport {
//!         name: "log".to_string(),
//!         export_type: ExportType::Function {
//!             params: vec![ValueType::I32, ValueType::I32],
//!             results: vec![],
//!         },
//!     }],
//! };
//!
//! // Register an "app" module that imports "log" from "logger".
//! let app = ModuleInterface {
//!     name: "app".to_string(),
//!     imports: vec![ModuleImport {
//!         module_name: "logger".to_string(),
//!         field_name: "log".to_string(),
//!         import_type: ImportType::Function {
//!             params: vec![ValueType::I32, ValueType::I32],
//!             results: vec![],
//!         },
//!     }],
//!     exports: vec![ModuleExport {
//!         name: "_start".to_string(),
//!         export_type: ExportType::Function {
//!             params: vec![],
//!             results: vec![],
//!         },
//!     }],
//! };
//!
//! linker.register(logger).unwrap();
//! linker.register(app).unwrap();
//!
//! let composition = linker.link().unwrap();
//! assert_eq!(composition.execution_order, vec!["logger", "app"]);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// Value and type descriptors
// ---------------------------------------------------------------------------

/// Describes a WebAssembly value type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit floating point.
    F32,
    /// 64-bit floating point.
    F64,
    /// 128-bit SIMD vector.
    V128,
    /// Function reference.
    FuncRef,
    /// External reference.
    ExternRef,
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueType::I32 => write!(f, "i32"),
            ValueType::I64 => write!(f, "i64"),
            ValueType::F32 => write!(f, "f32"),
            ValueType::F64 => write!(f, "f64"),
            ValueType::V128 => write!(f, "v128"),
            ValueType::FuncRef => write!(f, "funcref"),
            ValueType::ExternRef => write!(f, "externref"),
        }
    }
}

// ---------------------------------------------------------------------------
// Import types
// ---------------------------------------------------------------------------

/// Describes the type of an import that a module requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportType {
    /// A function import with the given parameter and result types.
    Function {
        /// Parameter types.
        params: Vec<ValueType>,
        /// Result types.
        results: Vec<ValueType>,
    },
    /// A linear memory import.
    Memory {
        /// Minimum number of 64 KiB pages.
        min_pages: u32,
        /// Optional maximum number of 64 KiB pages.
        max_pages: Option<u32>,
    },
    /// A table import.
    Table {
        /// The element type stored in the table.
        element_type: ValueType,
        /// Minimum number of elements.
        min: u32,
        /// Optional maximum number of elements.
        max: Option<u32>,
    },
    /// A global import.
    Global {
        /// The value type of the global.
        value_type: ValueType,
        /// Whether the global is mutable.
        mutable: bool,
    },
}

impl fmt::Display for ImportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportType::Function { params, results } => {
                let params_str: Vec<String> = params.iter().map(|p| p.to_string()).collect();
                let results_str: Vec<String> = results.iter().map(|r| r.to_string()).collect();
                write!(f, "func({}) -> ({})", params_str.join(", "), results_str.join(", "))
            }
            ImportType::Memory { min_pages, max_pages } => {
                if let Some(max) = max_pages {
                    write!(f, "memory(min={}, max={})", min_pages, max)
                } else {
                    write!(f, "memory(min={})", min_pages)
                }
            }
            ImportType::Table { element_type, min, max } => {
                if let Some(max) = max {
                    write!(f, "table({}, min={}, max={})", element_type, min, max)
                } else {
                    write!(f, "table({}, min={})", element_type, min)
                }
            }
            ImportType::Global { value_type, mutable } => {
                if *mutable {
                    write!(f, "global(mut {})", value_type)
                } else {
                    write!(f, "global({})", value_type)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Export types
// ---------------------------------------------------------------------------

/// Describes the type of an export that a module provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportType {
    /// A function export with the given parameter and result types.
    Function {
        /// Parameter types.
        params: Vec<ValueType>,
        /// Result types.
        results: Vec<ValueType>,
    },
    /// A linear memory export.
    Memory,
    /// A table export.
    Table,
    /// A global export.
    Global,
}

impl fmt::Display for ExportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportType::Function { params, results } => {
                let params_str: Vec<String> = params.iter().map(|p| p.to_string()).collect();
                let results_str: Vec<String> = results.iter().map(|r| r.to_string()).collect();
                write!(f, "func({}) -> ({})", params_str.join(", "), results_str.join(", "))
            }
            ExportType::Memory => write!(f, "memory"),
            ExportType::Table => write!(f, "table"),
            ExportType::Global => write!(f, "global"),
        }
    }
}

// ---------------------------------------------------------------------------
// Module import / export descriptors
// ---------------------------------------------------------------------------

/// Describes a single import that a module requires from another module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleImport {
    /// The name of the module that should provide this import.
    pub module_name: String,
    /// The field name within that module.
    pub field_name: String,
    /// The type of the import.
    pub import_type: ImportType,
}

/// Describes a single export that a module provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleExport {
    /// The name of the exported item.
    pub name: String,
    /// The type of the export.
    pub export_type: ExportType,
}

// ---------------------------------------------------------------------------
// Module interface
// ---------------------------------------------------------------------------

/// Describes the full interface of a WebAssembly module: its name, the
/// imports it requires, and the exports it provides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInterface {
    /// The module's unique name within a composition.
    pub name: String,
    /// Imports required by the module.
    pub imports: Vec<ModuleImport>,
    /// Exports provided by the module.
    pub exports: Vec<ModuleExport>,
}

impl ModuleInterface {
    /// Returns `true` if every import declared by `other` that references
    /// this module by name can be satisfied by an export of matching name
    /// and compatible type from this module.
    pub fn satisfies(&self, other: &ModuleInterface) -> bool {
        for imp in &other.imports {
            if imp.module_name != self.name {
                continue;
            }
            let matching_export = self.exports.iter().find(|e| e.name == imp.field_name);
            match matching_export {
                Some(export) => {
                    if !types_compatible(&imp.import_type, &export.export_type) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

/// Checks whether an export type is compatible with an import type.
fn types_compatible(import: &ImportType, export: &ExportType) -> bool {
    match (import, export) {
        (
            ImportType::Function { params: ip, results: ir },
            ExportType::Function { params: ep, results: er },
        ) => ip == ep && ir == er,
        (ImportType::Memory { .. }, ExportType::Memory) => true,
        (ImportType::Table { .. }, ExportType::Table) => true,
        (ImportType::Global { .. }, ExportType::Global) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Link errors
// ---------------------------------------------------------------------------

/// Errors that can occur during module linking and composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkError {
    /// An import could not be resolved to any registered module export.
    UnresolvedImport {
        /// The module name referenced by the import.
        module: String,
        /// The field name that could not be resolved.
        field: String,
    },
    /// The types of an import and the matching export are incompatible.
    TypeMismatch {
        /// Description of the expected type (from the import).
        expected: String,
        /// Description of the actual type (from the export).
        actual: String,
    },
    /// A cycle was detected in the module dependency graph.
    CyclicDependency {
        /// The names of the modules involved in the cycle.
        modules: Vec<String>,
    },
    /// Two or more modules export the same name in the same namespace.
    DuplicateExport {
        /// The duplicated export name.
        name: String,
    },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::UnresolvedImport { module, field } => {
                write!(f, "unresolved import: {}.{}", module, field)
            }
            LinkError::TypeMismatch { expected, actual } => {
                write!(f, "type mismatch: expected {}, got {}", expected, actual)
            }
            LinkError::CyclicDependency { modules } => {
                write!(f, "cyclic dependency among modules: {}", modules.join(" -> "))
            }
            LinkError::DuplicateExport { name } => {
                write!(f, "duplicate export: {}", name)
            }
        }
    }
}

impl std::error::Error for LinkError {}

impl From<LinkError> for Error {
    fn from(e: LinkError) -> Self {
        Error::Compilation(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Composition graph
// ---------------------------------------------------------------------------

/// A directed dependency graph of modules used to detect cycles and
/// compute a valid execution order (topological sort).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompositionGraph {
    /// Registered module interfaces, keyed by module name.
    pub modules: HashMap<String, ModuleInterface>,
    /// Directed edges: `(from, to)` means module `from` depends on module `to`
    /// (i.e. `from` imports something that `to` exports).
    pub edges: Vec<(String, String)>,
}

impl CompositionGraph {
    /// Creates an empty composition graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module interface to the graph.
    ///
    /// Returns an error if a module with the same name is already registered.
    pub fn add_module(&mut self, interface: ModuleInterface) -> Result<()> {
        if self.modules.contains_key(&interface.name) {
            return Err(Error::Compilation(format!(
                "module '{}' is already registered",
                interface.name
            )));
        }
        self.modules.insert(interface.name.clone(), interface);
        Ok(())
    }

    /// Adds a dependency edge indicating that module `from` imports field
    /// `field` from module `to`.
    ///
    /// Both `from` and `to` must already be registered in the graph.
    pub fn add_link(&mut self, from: &str, to: &str, _field: &str) -> Result<()> {
        if !self.modules.contains_key(from) {
            return Err(Error::Compilation(format!("module '{}' not found in graph", from)));
        }
        if !self.modules.contains_key(to) {
            return Err(Error::Compilation(format!("module '{}' not found in graph", to)));
        }
        self.edges.push((from.to_string(), to.to_string()));
        Ok(())
    }

    /// Validates the graph: checks for unresolved imports, type mismatches,
    /// and cyclic dependencies.
    pub fn validate(&self) -> std::result::Result<(), LinkError> {
        // Check unresolved imports and type compatibility.
        for iface in self.modules.values() {
            for imp in &iface.imports {
                let provider = self.modules.get(&imp.module_name).ok_or_else(|| {
                    LinkError::UnresolvedImport {
                        module: imp.module_name.clone(),
                        field: imp.field_name.clone(),
                    }
                })?;
                let export =
                    provider.exports.iter().find(|e| e.name == imp.field_name).ok_or_else(
                        || LinkError::UnresolvedImport {
                            module: imp.module_name.clone(),
                            field: imp.field_name.clone(),
                        },
                    )?;
                if !types_compatible(&imp.import_type, &export.export_type) {
                    return Err(LinkError::TypeMismatch {
                        expected: imp.import_type.to_string(),
                        actual: export.export_type.to_string(),
                    });
                }
            }
        }

        // Check for cycles via topological sort.
        self.topological_sort()?;

        Ok(())
    }

    /// Returns a topological ordering of the module names such that every
    /// module appears after all modules it depends on. Returns a
    /// `CyclicDependency` error if the graph contains a cycle.
    pub fn topological_sort(&self) -> std::result::Result<Vec<String>, LinkError> {
        // Build adjacency list and in-degree map from *all* import
        // relationships (not just manually added edges).
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for name in self.modules.keys() {
            in_degree.entry(name.clone()).or_insert(0);
            adj.entry(name.clone()).or_default();
        }

        // Edges derived from module imports: an import from module A
        // referencing module B means A depends on B, so edge B -> A
        // (B must come before A). We track unique edges to avoid
        // double-counting.
        let mut seen_edges: HashSet<(String, String)> = HashSet::new();
        for iface in self.modules.values() {
            for imp in &iface.imports {
                if self.modules.contains_key(&imp.module_name) {
                    let edge = (imp.module_name.clone(), iface.name.clone());
                    if seen_edges.insert(edge.clone()) {
                        adj.entry(edge.0.clone()).or_default().push(edge.1.clone());
                        *in_degree.entry(edge.1).or_insert(0) += 1;
                    }
                }
            }
        }

        // Also incorporate manually added edges.
        for (from, to) in &self.edges {
            // `from` depends on `to`, so `to` must come first: edge to -> from.
            let edge = (to.clone(), from.clone());
            if seen_edges.insert(edge.clone()) {
                adj.entry(edge.0.clone()).or_default().push(edge.1.clone());
                *in_degree.entry(edge.1).or_insert(0) += 1;
            }
        }

        // Kahn's algorithm.
        let mut queue: Vec<String> =
            in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(name, _)| name.clone()).collect();
        // Sort the initial queue for deterministic output.
        queue.sort();

        let mut order: Vec<String> = Vec::new();
        while let Some(node) = queue.pop() {
            order.push(node.clone());
            if let Some(neighbors) = adj.get(&node) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            // Insert in sorted position for determinism.
                            let pos = queue.binary_search(neighbor).unwrap_or_else(|p| p);
                            queue.insert(pos, neighbor.clone());
                        }
                    }
                }
            }
        }

        if order.len() != self.modules.len() {
            // Collect the modules that remain (they form one or more cycles).
            let in_order: HashSet<&str> = order.iter().map(|s| s.as_str()).collect();
            let cycle_members: Vec<String> =
                self.modules.keys().filter(|k| !in_order.contains(k.as_str())).cloned().collect();
            return Err(LinkError::CyclicDependency { modules: cycle_members });
        }

        Ok(order)
    }

    /// Convenience method: returns `true` if [`validate`](Self::validate)
    /// succeeds.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }
}

// ---------------------------------------------------------------------------
// Linked module / composition
// ---------------------------------------------------------------------------

/// Represents a single module after all of its imports have been resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedModule {
    /// The module name.
    pub name: String,
    /// The original interface description.
    pub interface: ModuleInterface,
    /// A map from import field name to the name of the module that provides it.
    pub resolved_imports: HashMap<String, String>,
}

/// The result of a successful link operation: an ordered set of linked modules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedComposition {
    /// The linked modules with resolved imports.
    pub modules: Vec<LinkedModule>,
    /// The execution order (topologically sorted module names).
    pub execution_order: Vec<String>,
}

// ---------------------------------------------------------------------------
// Module linker
// ---------------------------------------------------------------------------

/// High-level linker that registers module interfaces, validates their
/// compatibility, and produces a [`LinkedComposition`].
#[derive(Debug, Clone, Default)]
pub struct ModuleLinker {
    /// The underlying dependency graph.
    pub graph: CompositionGraph,
    /// Modules that have been linked (populated after [`link`](Self::link)).
    pub linked_modules: HashMap<String, LinkedModule>,
}

impl ModuleLinker {
    /// Creates a new, empty linker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a module interface with the linker.
    ///
    /// Returns an error if a module with the same name is already registered.
    pub fn register(&mut self, interface: ModuleInterface) -> Result<()> {
        self.graph.add_module(interface)
    }

    /// Validates all registered modules without producing a linked composition.
    ///
    /// This checks for unresolved imports, type mismatches, and cycles.
    pub fn validate(&self) -> Result<()> {
        self.graph.validate().map_err(Error::from)
    }

    /// Resolves all imports, validates the graph, and produces a
    /// [`LinkedComposition`] containing every module in execution order.
    pub fn link(&mut self) -> Result<LinkedComposition> {
        // Validate first (covers unresolved imports, type mismatches, cycles).
        self.graph.validate().map_err(Error::from)?;

        let execution_order = self.graph.topological_sort().map_err(Error::from)?;

        let mut modules = Vec::with_capacity(execution_order.len());
        self.linked_modules.clear();

        for name in &execution_order {
            let iface = self.graph.modules.get(name).expect("validated module must exist").clone();

            let mut resolved_imports: HashMap<String, String> = HashMap::new();
            for imp in &iface.imports {
                resolved_imports.insert(imp.field_name.clone(), imp.module_name.clone());
            }

            let linked = LinkedModule { name: name.clone(), interface: iface, resolved_imports };

            self.linked_modules.insert(name.clone(), linked.clone());
            modules.push(linked);
        }

        Ok(LinkedComposition { modules, execution_order })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: creates a module interface that exports a single function.
    fn provider_module(
        name: &str,
        export_name: &str,
        params: Vec<ValueType>,
        results: Vec<ValueType>,
    ) -> ModuleInterface {
        ModuleInterface {
            name: name.to_string(),
            imports: vec![],
            exports: vec![ModuleExport {
                name: export_name.to_string(),
                export_type: ExportType::Function { params, results },
            }],
        }
    }

    /// Helper: creates a module interface that imports a single function.
    fn consumer_module(
        name: &str,
        import_module: &str,
        import_field: &str,
        params: Vec<ValueType>,
        results: Vec<ValueType>,
    ) -> ModuleInterface {
        ModuleInterface {
            name: name.to_string(),
            imports: vec![ModuleImport {
                module_name: import_module.to_string(),
                field_name: import_field.to_string(),
                import_type: ImportType::Function { params, results },
            }],
            exports: vec![ModuleExport {
                name: "_start".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        }
    }

    // -----------------------------------------------------------------------
    // Basic linking
    // -----------------------------------------------------------------------

    #[test]
    fn test_basic_linking() {
        let mut linker = ModuleLinker::new();

        let logger = provider_module("logger", "log", vec![ValueType::I32, ValueType::I32], vec![]);
        let app =
            consumer_module("app", "logger", "log", vec![ValueType::I32, ValueType::I32], vec![]);

        linker.register(logger).expect("register module logger");
        linker.register(app).expect("register module app");

        let composition = linker.link().expect("link modules");
        assert_eq!(composition.execution_order.len(), 2);
        // "logger" must come before "app" because "app" depends on "logger".
        assert_eq!(composition.execution_order[0], "logger");
        assert_eq!(composition.execution_order[1], "app");

        // Verify resolved imports.
        let app_linked = composition.modules.iter().find(|m| m.name == "app").expect("find module app");
        assert_eq!(app_linked.resolved_imports.get("log").expect("resolve import log"), "logger");
    }

    #[test]
    fn test_linking_no_imports() {
        let mut linker = ModuleLinker::new();

        let standalone = ModuleInterface {
            name: "standalone".to_string(),
            imports: vec![],
            exports: vec![ModuleExport {
                name: "_start".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };

        linker.register(standalone).expect("register module standalone");
        let composition = linker.link().expect("link modules");
        assert_eq!(composition.execution_order, vec!["standalone"]);
    }

    #[test]
    fn test_linking_chain() {
        // c depends on b, b depends on a
        let mut linker = ModuleLinker::new();

        let a = provider_module("a", "fa", vec![ValueType::I32], vec![ValueType::I32]);
        let b = ModuleInterface {
            name: "b".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function {
                    params: vec![ValueType::I32],
                    results: vec![ValueType::I32],
                },
            }],
            exports: vec![ModuleExport {
                name: "fb".to_string(),
                export_type: ExportType::Function {
                    params: vec![ValueType::I64],
                    results: vec![ValueType::I64],
                },
            }],
        };
        let c = consumer_module("c", "b", "fb", vec![ValueType::I64], vec![ValueType::I64]);

        linker.register(a).expect("register module a");
        linker.register(b).expect("register module b");
        linker.register(c).expect("register module c");

        let composition = linker.link().expect("link modules");
        // Must be a -> b -> c.
        let order = &composition.execution_order;
        assert!(
            order.iter().position(|n| n == "a").expect("find position of a")
                < order.iter().position(|n| n == "b").expect("find position of b")
        );
        assert!(
            order.iter().position(|n| n == "b").expect("find position of b")
                < order.iter().position(|n| n == "c").expect("find position of c")
        );
    }

    // -----------------------------------------------------------------------
    // Unresolved import detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_unresolved_import_missing_module() {
        let mut linker = ModuleLinker::new();

        let app = consumer_module("app", "nonexistent", "foo", vec![], vec![]);
        linker.register(app).expect("register module app");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unresolved import"), "error was: {}", msg);
    }

    #[test]
    fn test_unresolved_import_missing_field() {
        let mut linker = ModuleLinker::new();

        let logger = provider_module("logger", "log", vec![], vec![]);
        let app = consumer_module("app", "logger", "missing_fn", vec![], vec![]);

        linker.register(logger).expect("register module logger");
        linker.register(app).expect("register module app");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unresolved import"), "error was: {}", msg);
    }

    // -----------------------------------------------------------------------
    // Cycle detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_cycle_detection_simple() {
        let mut linker = ModuleLinker::new();

        // a imports from b, b imports from a
        let a = ModuleInterface {
            name: "a".to_string(),
            imports: vec![ModuleImport {
                module_name: "b".to_string(),
                field_name: "fb".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fa".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        let b = ModuleInterface {
            name: "b".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fb".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };

        linker.register(a).expect("register module a");
        linker.register(b).expect("register module b");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cyclic dependency"), "error was: {}", msg);
    }

    #[test]
    fn test_cycle_detection_three_modules() {
        let mut linker = ModuleLinker::new();

        // a -> b -> c -> a
        let a = ModuleInterface {
            name: "a".to_string(),
            imports: vec![ModuleImport {
                module_name: "c".to_string(),
                field_name: "fc".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fa".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        let b = ModuleInterface {
            name: "b".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fb".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        let c = ModuleInterface {
            name: "c".to_string(),
            imports: vec![ModuleImport {
                module_name: "b".to_string(),
                field_name: "fb".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fc".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };

        linker.register(a).expect("register module a");
        linker.register(b).expect("register module b");
        linker.register(c).expect("register module c");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cyclic dependency"), "error was: {}", msg);
    }

    // -----------------------------------------------------------------------
    // Topological sort
    // -----------------------------------------------------------------------

    #[test]
    fn test_topological_sort_diamond() {
        // Diamond: d depends on b and c; b and c both depend on a.
        let mut graph = CompositionGraph::new();

        let a = provider_module("a", "fa", vec![], vec![ValueType::I32]);
        let b = ModuleInterface {
            name: "b".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![ValueType::I32] },
            }],
            exports: vec![ModuleExport {
                name: "fb".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![ValueType::I32] },
            }],
        };
        let c = ModuleInterface {
            name: "c".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![ValueType::I32] },
            }],
            exports: vec![ModuleExport {
                name: "fc".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![ValueType::I32] },
            }],
        };
        let d = ModuleInterface {
            name: "d".to_string(),
            imports: vec![
                ModuleImport {
                    module_name: "b".to_string(),
                    field_name: "fb".to_string(),
                    import_type: ImportType::Function {
                        params: vec![],
                        results: vec![ValueType::I32],
                    },
                },
                ModuleImport {
                    module_name: "c".to_string(),
                    field_name: "fc".to_string(),
                    import_type: ImportType::Function {
                        params: vec![],
                        results: vec![ValueType::I32],
                    },
                },
            ],
            exports: vec![],
        };

        graph.add_module(a).expect("add module a");
        graph.add_module(b).expect("add module b");
        graph.add_module(c).expect("add module c");
        graph.add_module(d).expect("add module d");

        let order = graph.topological_sort().expect("topological sort");
        let pos = |name: &str| order.iter().position(|n| n == name).expect("find position of module");

        // "a" must come before "b" and "c", which must come before "d".
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

    #[test]
    fn test_topological_sort_independent_modules() {
        let mut graph = CompositionGraph::new();

        let x = provider_module("x", "fx", vec![], vec![]);
        let y = provider_module("y", "fy", vec![], vec![]);
        let z = provider_module("z", "fz", vec![], vec![]);

        graph.add_module(x).expect("add module x");
        graph.add_module(y).expect("add module y");
        graph.add_module(z).expect("add module z");

        let order = graph.topological_sort().expect("topological sort");
        assert_eq!(order.len(), 3);
        // All three should be present (order among them is deterministic
        // because we sort alphabetically).
        assert_eq!(order, vec!["z", "y", "x"]);
    }

    // -----------------------------------------------------------------------
    // Type mismatch detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_type_mismatch_function_params() {
        let mut linker = ModuleLinker::new();

        // logger exports log(i32, i32) -> ()
        let logger = provider_module("logger", "log", vec![ValueType::I32, ValueType::I32], vec![]);
        // app imports log(i64) -> () -- wrong parameter types
        let app = consumer_module("app", "logger", "log", vec![ValueType::I64], vec![]);

        linker.register(logger).expect("register module logger");
        linker.register(app).expect("register module app");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("type mismatch"), "error was: {}", msg);
    }

    #[test]
    fn test_type_mismatch_function_results() {
        let mut linker = ModuleLinker::new();

        // Provider exports func() -> i32.
        let provider = provider_module("provider", "compute", vec![], vec![ValueType::I32]);
        // Consumer expects func() -> i64 -- wrong result type.
        let consumer =
            consumer_module("consumer", "provider", "compute", vec![], vec![ValueType::I64]);

        linker.register(provider).expect("register module provider");
        linker.register(consumer).expect("register module consumer");

        let err = linker.link().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("type mismatch"), "error was: {}", msg);
    }

    #[test]
    fn test_type_mismatch_import_kind() {
        // Import expects a memory, but export is a function.
        let mut graph = CompositionGraph::new();

        let provider = ModuleInterface {
            name: "provider".to_string(),
            imports: vec![],
            exports: vec![ModuleExport {
                name: "mem".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        let consumer = ModuleInterface {
            name: "consumer".to_string(),
            imports: vec![ModuleImport {
                module_name: "provider".to_string(),
                field_name: "mem".to_string(),
                import_type: ImportType::Memory { min_pages: 1, max_pages: None },
            }],
            exports: vec![],
        };

        graph.add_module(provider).expect("add module provider");
        graph.add_module(consumer).expect("add module consumer");

        let err = graph.validate().unwrap_err();
        assert!(matches!(err, LinkError::TypeMismatch { .. }));
    }

    // -----------------------------------------------------------------------
    // ModuleInterface::satisfies
    // -----------------------------------------------------------------------

    #[test]
    fn test_satisfies_positive() {
        let logger = provider_module("logger", "log", vec![ValueType::I32], vec![]);
        let app = consumer_module("app", "logger", "log", vec![ValueType::I32], vec![]);

        assert!(logger.satisfies(&app));
    }

    #[test]
    fn test_satisfies_negative_missing_export() {
        let logger = provider_module("logger", "log", vec![], vec![]);
        let app = consumer_module("app", "logger", "write", vec![], vec![]);

        assert!(!logger.satisfies(&app));
    }

    #[test]
    fn test_satisfies_negative_type_mismatch() {
        let logger = provider_module("logger", "log", vec![ValueType::I32], vec![]);
        let app = consumer_module("app", "logger", "log", vec![ValueType::I64], vec![]);

        assert!(!logger.satisfies(&app));
    }

    #[test]
    fn test_satisfies_ignores_unrelated_imports() {
        // If the import references a different module, `satisfies` should
        // ignore it and return true.
        let logger = provider_module("logger", "log", vec![], vec![]);
        let app = consumer_module("app", "other_module", "foo", vec![], vec![]);

        assert!(logger.satisfies(&app));
    }

    // -----------------------------------------------------------------------
    // CompositionGraph edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_duplicate_module_registration() {
        let mut graph = CompositionGraph::new();
        let m = provider_module("m", "f", vec![], vec![]);
        graph.add_module(m.clone()).expect("add module m");

        let err = graph.add_module(m);
        assert!(err.is_err());
    }

    #[test]
    fn test_add_link_unknown_module() {
        let mut graph = CompositionGraph::new();
        let m = provider_module("m", "f", vec![], vec![]);
        graph.add_module(m).expect("add module m");

        assert!(graph.add_link("m", "unknown", "f").is_err());
        assert!(graph.add_link("unknown", "m", "f").is_err());
    }

    #[test]
    fn test_graph_is_valid() {
        let mut graph = CompositionGraph::new();
        let a = provider_module("a", "fa", vec![], vec![]);
        let b = consumer_module("b", "a", "fa", vec![], vec![]);
        graph.add_module(a).expect("add module a");
        graph.add_module(b).expect("add module b");

        assert!(graph.is_valid());
    }

    #[test]
    fn test_graph_is_not_valid_with_cycle() {
        let mut graph = CompositionGraph::new();
        let a = ModuleInterface {
            name: "a".to_string(),
            imports: vec![ModuleImport {
                module_name: "b".to_string(),
                field_name: "fb".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fa".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        let b = ModuleInterface {
            name: "b".to_string(),
            imports: vec![ModuleImport {
                module_name: "a".to_string(),
                field_name: "fa".to_string(),
                import_type: ImportType::Function { params: vec![], results: vec![] },
            }],
            exports: vec![ModuleExport {
                name: "fb".to_string(),
                export_type: ExportType::Function { params: vec![], results: vec![] },
            }],
        };
        graph.add_module(a).expect("add module a");
        graph.add_module(b).expect("add module b");

        assert!(!graph.is_valid());
    }

    // -----------------------------------------------------------------------
    // Display / formatting
    // -----------------------------------------------------------------------

    #[test]
    fn test_value_type_display() {
        assert_eq!(ValueType::I32.to_string(), "i32");
        assert_eq!(ValueType::I64.to_string(), "i64");
        assert_eq!(ValueType::F32.to_string(), "f32");
        assert_eq!(ValueType::F64.to_string(), "f64");
        assert_eq!(ValueType::V128.to_string(), "v128");
        assert_eq!(ValueType::FuncRef.to_string(), "funcref");
        assert_eq!(ValueType::ExternRef.to_string(), "externref");
    }

    #[test]
    fn test_link_error_display() {
        let err =
            LinkError::UnresolvedImport { module: "env".to_string(), field: "memory".to_string() };
        assert_eq!(err.to_string(), "unresolved import: env.memory");

        let err = LinkError::TypeMismatch {
            expected: "func(i32) -> ()".to_string(),
            actual: "func(i64) -> ()".to_string(),
        };
        assert!(err.to_string().contains("type mismatch"));

        let err = LinkError::CyclicDependency { modules: vec!["a".to_string(), "b".to_string()] };
        assert!(err.to_string().contains("cyclic dependency"));

        let err = LinkError::DuplicateExport { name: "foo".to_string() };
        assert!(err.to_string().contains("duplicate export"));
    }
}
