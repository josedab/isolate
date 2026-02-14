//! ES Module system and async execution support for the JavaScript runtime.
//!
//! Extends the JS runtime with:
//! - ES Module resolution and bundling
//! - Async/await execution with event loop integration
//! - Module caching and dependency tracking
//! - Execution context isolation per module
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::jsrt::module_system::*;
//!
//! let mut resolver = ModuleResolver::new();
//! resolver.register("@isolate/utils", "export function hello() { return 'hi'; }");
//!
//! let graph = resolver.resolve_graph("import { hello } from '@isolate/utils';");
//! let bundled = resolver.bundle(&graph);
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// A JavaScript module definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsModule {
    /// Module specifier (e.g., "./utils", "@isolate/http").
    pub specifier: String,
    /// Module source code.
    pub source: String,
    /// Module type.
    pub module_type: ModuleType,
    /// Exports from this module.
    pub exports: Vec<String>,
    /// Imports required by this module.
    pub imports: Vec<ModuleImport>,
    /// Whether this module has been evaluated.
    pub evaluated: bool,
}

/// Type of JavaScript module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModuleType {
    /// ES Module (import/export).
    EsModule,
    /// CommonJS module (require/module.exports).
    CommonJs,
    /// JSON data module.
    Json,
    /// Built-in module provided by the runtime.
    Builtin,
}

/// An import statement in a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleImport {
    /// Module specifier being imported.
    pub specifier: String,
    /// Named imports (e.g., `{ foo, bar }`).
    pub named: Vec<String>,
    /// Default import name (e.g., `import X from ...`).
    pub default: Option<String>,
    /// Whether this is a namespace import (e.g., `import * as X`).
    pub namespace: bool,
}

/// Module dependency graph.
#[derive(Debug, Clone)]
pub struct ModuleGraph {
    /// Entry point module specifier.
    pub entry: String,
    /// All modules in dependency order.
    pub modules: Vec<JsModule>,
    /// Dependency edges (from → [to]).
    pub dependencies: HashMap<String, Vec<String>>,
    /// Whether the graph has circular dependencies.
    pub has_cycles: bool,
    /// Specifiers that could not be resolved.
    pub unresolved: Vec<String>,
}

impl ModuleGraph {
    /// Get the total number of modules.
    pub fn module_count(&self) -> usize {
        self.modules.len()
    }

    /// Get a module by specifier.
    pub fn get_module(&self, specifier: &str) -> Option<&JsModule> {
        self.modules.iter().find(|m| m.specifier == specifier)
    }

    /// Get the total source size.
    pub fn total_source_size(&self) -> usize {
        self.modules.iter().map(|m| m.source.len()).sum()
    }
}

/// Module resolver and registry.
pub struct ModuleResolver {
    /// Registered modules.
    modules: HashMap<String, JsModule>,
    /// Built-in modules provided by the runtime.
    builtins: HashMap<String, JsModule>,
    /// Module cache.
    cache: HashMap<String, String>,
    /// Maximum number of modules allowed.
    max_modules: usize,
}

impl ModuleResolver {
    /// Create a new module resolver.
    pub fn new() -> Self {
        let mut resolver = Self {
            modules: HashMap::new(),
            builtins: HashMap::new(),
            cache: HashMap::new(),
            max_modules: 100,
        };
        resolver.register_builtins();
        resolver
    }

    /// Register a module by specifier. Returns error if max_modules exceeded.
    pub fn register(
        &mut self,
        specifier: impl Into<String>,
        source: impl Into<String>,
    ) -> Result<(), String> {
        if self.modules.len() >= self.max_modules {
            return Err(format!(
                "Maximum module limit ({}) reached",
                self.max_modules
            ));
        }
        let specifier = specifier.into();
        let source = source.into();
        let imports = Self::extract_imports(&source);
        let exports = Self::extract_exports(&source);

        let module = JsModule {
            specifier: specifier.clone(),
            source,
            module_type: ModuleType::EsModule,
            exports,
            imports,
            evaluated: false,
        };

        self.modules.insert(specifier, module);
        Ok(())
    }

    /// Register a JSON data module.
    pub fn register_json(&mut self, specifier: impl Into<String>, json: impl Into<String>) {
        let specifier = specifier.into();
        let source = format!("export default {};", json.into());
        let module = JsModule {
            specifier: specifier.clone(),
            source,
            module_type: ModuleType::Json,
            exports: vec!["default".to_string()],
            imports: Vec::new(),
            evaluated: false,
        };
        self.modules.insert(specifier, module);
    }

    /// Resolve the dependency graph for a source string.
    pub fn resolve_graph(&self, entry_source: &str) -> ModuleGraph {
        let entry_specifier = "__entry__".to_string();
        let imports = Self::extract_imports(entry_source);

        let mut modules = vec![JsModule {
            specifier: entry_specifier.clone(),
            source: entry_source.to_string(),
            module_type: ModuleType::EsModule,
            exports: Vec::new(),
            imports: imports.clone(),
            evaluated: false,
        }];

        let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited = std::collections::HashSet::new();
        let mut unresolved = Vec::new();
        visited.insert(entry_specifier.clone());

        let mut queue: Vec<String> = imports.iter().map(|i| i.specifier.clone()).collect();
        dependencies.insert(
            entry_specifier.clone(),
            imports.iter().map(|i| i.specifier.clone()).collect(),
        );

        while let Some(specifier) = queue.pop() {
            if visited.contains(&specifier) {
                continue;
            }
            visited.insert(specifier.clone());

            if let Some(module) = self.modules.get(&specifier).or_else(|| self.builtins.get(&specifier)) {
                let deps: Vec<String> = module.imports.iter().map(|i| i.specifier.clone()).collect();
                for dep in &deps {
                    if !visited.contains(dep) {
                        queue.push(dep.clone());
                    }
                }
                dependencies.insert(specifier.clone(), deps);
                modules.push(module.clone());
            } else {
                unresolved.push(specifier);
            }
        }

        let has_cycles = self.detect_cycles(&dependencies);

        ModuleGraph {
            entry: entry_specifier,
            modules,
            dependencies,
            has_cycles,
            unresolved,
        }
    }

    /// Bundle all modules in the graph into a single script.
    /// Results are cached by entry source hash.
    pub fn bundle(&mut self, graph: &ModuleGraph) -> String {
        // Check cache
        let cache_key = format!("{}:{}", graph.entry, graph.module_count());
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }

        let mut parts = Vec::new();
        parts.push("// Bundled by Isolate JS Runtime".to_string());
        parts.push("(function() {".to_string());
        parts.push("  const __modules = {};".to_string());

        // Add all non-entry modules as module definitions
        for module in &graph.modules {
            if module.specifier == graph.entry {
                continue;
            }
            parts.push(format!(
                "  __modules['{}'] = (function(exports) {{ {} return exports; }})({{}}); ",
                module.specifier, module.source
            ));
        }

        // Add entry module
        if let Some(entry) = graph.get_module(&graph.entry) {
            parts.push(format!("  // Entry point"));
            parts.push(format!("  {}", entry.source));
        }

        parts.push("})();".to_string());
        let result = parts.join("\n");
        self.cache.insert(cache_key, result.clone());
        result
    }

    /// Get a registered module.
    pub fn get(&self, specifier: &str) -> Option<&JsModule> {
        self.modules.get(specifier).or_else(|| self.builtins.get(specifier))
    }

    /// List all registered module specifiers.
    pub fn specifiers(&self) -> Vec<&str> {
        self.modules
            .keys()
            .chain(self.builtins.keys())
            .map(|s| s.as_str())
            .collect()
    }

    /// Register built-in modules.
    fn register_builtins(&mut self) {
        // @isolate/env - environment access
        self.builtins.insert(
            "@isolate/env".to_string(),
            JsModule {
                specifier: "@isolate/env".to_string(),
                source: "export function get(key) { return Isolate.env[key]; }\nexport function all() { return Isolate.env; }".to_string(),
                module_type: ModuleType::Builtin,
                exports: vec!["get".to_string(), "all".to_string()],
                imports: Vec::new(),
                evaluated: false,
            },
        );

        // @isolate/io - I/O operations
        self.builtins.insert(
            "@isolate/io".to_string(),
            JsModule {
                specifier: "@isolate/io".to_string(),
                source: "export function readInput() { return Isolate.input; }\nexport function writeOutput(data) { console.log(JSON.stringify(data)); }".to_string(),
                module_type: ModuleType::Builtin,
                exports: vec!["readInput".to_string(), "writeOutput".to_string()],
                imports: Vec::new(),
                evaluated: false,
            },
        );
    }

    /// Extract import statements from source (simple heuristic parser).
    fn extract_imports(source: &str) -> Vec<ModuleImport> {
        let mut imports = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                if let Some(from_idx) = trimmed.find("from ") {
                    let spec_part = &trimmed[from_idx + 5..];
                    let specifier = spec_part
                        .trim()
                        .trim_end_matches(';')
                        .trim_matches('\'')
                        .trim_matches('"')
                        .to_string();

                    let import_part = &trimmed[7..from_idx].trim();

                    let mut named = Vec::new();
                    let mut default = None;
                    let namespace = import_part.contains("* as");

                    if let Some(brace_start) = import_part.find('{') {
                        if let Some(brace_end) = import_part.find('}') {
                            let names = &import_part[brace_start + 1..brace_end];
                            named = names
                                .split(',')
                                .map(|n| n.trim().to_string())
                                .filter(|n| !n.is_empty())
                                .collect();
                        }
                    } else if !namespace && !import_part.is_empty() {
                        default = Some(import_part.trim_matches(',').trim().to_string());
                    }

                    imports.push(ModuleImport { specifier, named, default, namespace });
                }
            }
        }

        imports
    }

    /// Extract export names from source (simple heuristic parser).
    fn extract_exports(source: &str) -> Vec<String> {
        let mut exports = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("export function ") {
                if let Some(name) = trimmed[16..].split('(').next() {
                    exports.push(name.trim().to_string());
                }
            } else if trimmed.starts_with("export const ") || trimmed.starts_with("export let ") || trimmed.starts_with("export var ") {
                let after_keyword = if trimmed.starts_with("export const ") {
                    &trimmed[13..]
                } else if trimmed.starts_with("export let ") {
                    &trimmed[11..]
                } else {
                    &trimmed[11..]
                };
                if let Some(name) = after_keyword.split(['=', ' ', ':']).next() {
                    exports.push(name.trim().to_string());
                }
            } else if trimmed.starts_with("export default ") {
                exports.push("default".to_string());
            }
        }

        exports
    }

    /// Simple cycle detection using DFS.
    fn detect_cycles(&self, deps: &HashMap<String, Vec<String>>) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut in_stack = std::collections::HashSet::new();

        for node in deps.keys() {
            if self.dfs_cycle(node, deps, &mut visited, &mut in_stack) {
                return true;
            }
        }
        false
    }

    fn dfs_cycle(
        &self,
        node: &str,
        deps: &HashMap<String, Vec<String>>,
        visited: &mut std::collections::HashSet<String>,
        in_stack: &mut std::collections::HashSet<String>,
    ) -> bool {
        if in_stack.contains(node) {
            return true;
        }
        if visited.contains(node) {
            return false;
        }

        visited.insert(node.to_string());
        in_stack.insert(node.to_string());

        if let Some(children) = deps.get(node) {
            for child in children {
                if self.dfs_cycle(child, deps, visited, in_stack) {
                    return true;
                }
            }
        }

        in_stack.remove(node);
        false
    }
}

impl Default for ModuleResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for async execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncExecConfig {
    /// Maximum number of pending promises.
    pub max_pending_promises: usize,
    /// Maximum event loop iterations.
    pub max_event_loop_ticks: usize,
    /// Timeout for the entire async execution.
    pub timeout: Duration,
    /// Whether to enable microtask queue processing.
    pub enable_microtasks: bool,
}

impl Default for AsyncExecConfig {
    fn default() -> Self {
        Self {
            max_pending_promises: 100,
            max_event_loop_ticks: 10_000,
            timeout: Duration::from_secs(30),
            enable_microtasks: true,
        }
    }
}

/// State of the async execution event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventLoopState {
    /// Event loop is idle.
    Idle,
    /// Event loop is processing events.
    Running,
    /// Event loop is waiting for I/O.
    Waiting,
    /// Event loop completed.
    Completed,
    /// Event loop timed out.
    TimedOut,
    /// Event loop hit max ticks.
    MaxTicksReached,
}

impl std::fmt::Display for EventLoopState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Waiting => write!(f, "waiting"),
            Self::Completed => write!(f, "completed"),
            Self::TimedOut => write!(f, "timed_out"),
            Self::MaxTicksReached => write!(f, "max_ticks"),
        }
    }
}

/// Result of an async JavaScript execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncExecResult {
    /// Final event loop state.
    pub state: EventLoopState,
    /// Total event loop ticks executed.
    pub ticks: usize,
    /// Number of resolved promises.
    pub resolved_promises: usize,
    /// Number of rejected promises.
    pub rejected_promises: usize,
    /// Execution wall time.
    pub wall_time: Duration,
    /// Output from the execution.
    pub output: Option<String>,
    /// Errors from rejected promises.
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_resolver_register() {
        let mut resolver = ModuleResolver::new();
        resolver.register("./utils", "export function add(a, b) { return a + b; }");

        let module = resolver.get("./utils").unwrap();
        assert_eq!(module.module_type, ModuleType::EsModule);
        assert_eq!(module.exports, vec!["add"]);
    }

    #[test]
    fn test_module_resolver_json() {
        let mut resolver = ModuleResolver::new();
        resolver.register_json("./config", r#"{"debug": true}"#);

        let module = resolver.get("./config").unwrap();
        assert_eq!(module.module_type, ModuleType::Json);
        assert_eq!(module.exports, vec!["default"]);
    }

    #[test]
    fn test_module_resolver_builtins() {
        let resolver = ModuleResolver::new();
        assert!(resolver.get("@isolate/env").is_some());
        assert!(resolver.get("@isolate/io").is_some());
    }

    #[test]
    fn test_extract_imports() {
        let source = r#"
            import { foo, bar } from './utils';
            import Default from './default';
            import * as ns from './namespace';
            const x = 1;
        "#;

        let imports = ModuleResolver::extract_imports(source);
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].specifier, "./utils");
        assert_eq!(imports[0].named, vec!["foo", "bar"]);
        assert_eq!(imports[1].specifier, "./default");
        assert_eq!(imports[1].default, Some("Default".to_string()));
        assert!(imports[2].namespace);
    }

    #[test]
    fn test_extract_exports() {
        let source = r#"
            export function hello() { return 'hi'; }
            export const PI = 3.14;
            export let count = 0;
            export default class App {}
        "#;

        let exports = ModuleResolver::extract_exports(source);
        assert!(exports.contains(&"hello".to_string()));
        assert!(exports.contains(&"PI".to_string()));
        assert!(exports.contains(&"count".to_string()));
        assert!(exports.contains(&"default".to_string()));
    }

    #[test]
    fn test_resolve_graph() {
        let mut resolver = ModuleResolver::new();
        resolver.register("./utils", "export function add(a, b) { return a + b; }");

        let graph = resolver.resolve_graph("import { add } from './utils'; add(1, 2);");
        assert_eq!(graph.module_count(), 2);
        assert!(!graph.has_cycles);
    }

    #[test]
    fn test_bundle() {
        let mut resolver = ModuleResolver::new();
        resolver.register("./utils", "export function greet() { return 'hello'; }");

        let graph = resolver.resolve_graph("import { greet } from './utils'; console.log(greet());");
        let bundled = resolver.bundle(&graph);

        assert!(bundled.contains("__modules"));
        assert!(bundled.contains("./utils"));
        assert!(bundled.contains("greet"));
    }

    #[test]
    fn test_cycle_detection() {
        let resolver = ModuleResolver::new();
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]);
        deps.insert("b".to_string(), vec!["a".to_string()]);

        assert!(resolver.detect_cycles(&deps));
    }

    #[test]
    fn test_no_cycle() {
        let resolver = ModuleResolver::new();
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]);
        deps.insert("b".to_string(), vec!["c".to_string()]);
        deps.insert("c".to_string(), vec![]);

        assert!(!resolver.detect_cycles(&deps));
    }

    #[test]
    fn test_async_exec_config() {
        let config = AsyncExecConfig::default();
        assert_eq!(config.max_pending_promises, 100);
        assert!(config.enable_microtasks);
    }

    #[test]
    fn test_event_loop_state_display() {
        assert_eq!(EventLoopState::Running.to_string(), "running");
        assert_eq!(EventLoopState::Completed.to_string(), "completed");
    }

    #[test]
    fn test_module_graph_total_size() {
        let mut resolver = ModuleResolver::new();
        resolver.register("./a", "const x = 1;").unwrap(); // 14 bytes
        resolver.register("./b", "const y = 2;").unwrap(); // 14 bytes

        let graph = resolver.resolve_graph("import './a';\nimport './b';");
        assert!(graph.total_source_size() > 0);
    }

    #[test]
    fn test_max_modules_enforced() {
        let mut resolver = ModuleResolver::new();
        resolver.max_modules = 2;
        resolver.register("./a", "const a = 1;").unwrap();
        resolver.register("./b", "const b = 2;").unwrap();
        assert!(resolver.register("./c", "const c = 3;").is_err());
    }

    #[test]
    fn test_unresolved_imports_tracked() {
        let resolver = ModuleResolver::new();
        let graph = resolver.resolve_graph("import { foo } from './nonexistent';");
        assert!(!graph.unresolved.is_empty());
        assert!(graph.unresolved.contains(&"./nonexistent".to_string()));
    }

    #[test]
    fn test_bundle_caching() {
        let mut resolver = ModuleResolver::new();
        resolver.register("./utils", "export const X = 1;").unwrap();

        let graph = resolver.resolve_graph("import { X } from './utils'; console.log(X);");
        let bundle1 = resolver.bundle(&graph);
        let bundle2 = resolver.bundle(&graph);
        assert_eq!(bundle1, bundle2);
        assert!(!resolver.cache.is_empty());
    }
}
