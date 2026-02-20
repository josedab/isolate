//! Plugin System
//!
//! Extensibility through plugins:
//! - Custom host functions
//! - Capability providers
//! - Middleware hooks
//! - Event handlers
//! - Custom metrics exporters

#![allow(missing_docs)]
// This module is experimental and not all APIs are used yet.
// Allow dead code until the feature stabilizes.


pub mod reference;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

impl PluginId {
    /// Create new plugin ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin ID.
    pub id: PluginId,
    /// Plugin name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Description.
    pub description: Option<String>,
    /// Author.
    pub author: Option<String>,
    /// License.
    pub license: Option<String>,
    /// Plugin type.
    pub plugin_type: PluginType,
    /// Required capabilities.
    pub required_capabilities: Vec<String>,
    /// Provided host functions.
    pub host_functions: Vec<HostFunctionSpec>,
    /// Event subscriptions.
    pub event_subscriptions: Vec<EventType>,
    /// Configuration schema.
    pub config_schema: Option<serde_json::Value>,
}

/// Plugin type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    /// Provides host functions.
    HostFunctions,
    /// Provides capabilities.
    CapabilityProvider,
    /// Middleware for request/response.
    Middleware,
    /// Event handler.
    EventHandler,
    /// Metrics exporter.
    MetricsExporter,
    /// Combined/multi-purpose.
    Composite,
}

/// Host function specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostFunctionSpec {
    /// Function name.
    pub name: String,
    /// Module name.
    pub module: String,
    /// Parameter types.
    pub params: Vec<ValueType>,
    /// Return types.
    pub returns: Vec<ValueType>,
    /// Description.
    pub description: Option<String>,
}

/// WASM value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}

/// Event types for plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    /// Sandbox created.
    SandboxCreated,
    /// Sandbox started.
    SandboxStarted,
    /// Sandbox completed.
    SandboxCompleted,
    /// Sandbox failed.
    SandboxFailed,
    /// Sandbox terminated.
    SandboxTerminated,
    /// Resource limit warning.
    ResourceLimitWarning,
    /// Resource limit exceeded.
    ResourceLimitExceeded,
    /// Capability check.
    CapabilityCheck,
    /// Capability denied.
    CapabilityDenied,
    /// Host function called.
    HostFunctionCalled,
    /// Snapshot created.
    SnapshotCreated,
    /// Snapshot restored.
    SnapshotRestored,
}

/// Event data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Event type.
    pub event_type: EventType,
    /// Sandbox ID.
    pub sandbox_id: Option<String>,
    /// Timestamp.
    pub timestamp: std::time::SystemTime,
    /// Event-specific data.
    pub data: HashMap<String, serde_json::Value>,
}

/// Plugin state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// Registered but not loaded.
    Registered,
    /// Loading.
    Loading,
    /// Active.
    Active,
    /// Disabled.
    Disabled,
    /// Failed.
    Failed,
}

/// Plugin instance.
pub struct PluginInstance {
    manifest: PluginManifest,
    state: PluginState,
    config: HashMap<String, serde_json::Value>,
    handler: Box<dyn PluginHandler>,
}

impl PluginInstance {
    /// Get manifest.
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Get state.
    pub fn state(&self) -> PluginState {
        self.state
    }

    /// Get config.
    pub fn config(&self) -> &HashMap<String, serde_json::Value> {
        &self.config
    }
}

/// Plugin handler trait.
pub trait PluginHandler: Send + Sync {
    /// Initialize the plugin.
    fn init(&mut self, config: &HashMap<String, serde_json::Value>) -> Result<(), PluginError>;

    /// Handle an event.
    fn handle_event(&self, event: &Event) -> Result<(), PluginError>;

    /// Invoke a host function.
    fn invoke_host_function(
        &self,
        name: &str,
        params: &[serde_json::Value],
    ) -> Result<Vec<serde_json::Value>, PluginError>;

    /// Shutdown the plugin.
    fn shutdown(&mut self) -> Result<(), PluginError>;
}

/// No-op plugin handler.
#[derive(Default)]
pub struct NoopHandler;

impl PluginHandler for NoopHandler {
    fn init(&mut self, _config: &HashMap<String, serde_json::Value>) -> Result<(), PluginError> {
        Ok(())
    }

    fn handle_event(&self, _event: &Event) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        _params: &[serde_json::Value],
    ) -> Result<Vec<serde_json::Value>, PluginError> {
        Err(PluginError::FunctionNotFound(name.to_string()))
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Plugin registry.
pub struct PluginRegistry {
    plugins: HashMap<PluginId, PluginInstance>,
    event_handlers: HashMap<EventType, Vec<PluginId>>,
    host_functions: HashMap<String, PluginId>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create new registry.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            event_handlers: HashMap::new(),
            host_functions: HashMap::new(),
        }
    }

    /// Register a plugin.
    pub fn register(
        &mut self,
        manifest: PluginManifest,
        handler: Box<dyn PluginHandler>,
    ) -> Result<(), PluginError> {
        if self.plugins.contains_key(&manifest.id) {
            return Err(PluginError::AlreadyRegistered(manifest.id.0.clone()));
        }

        // Register event handlers
        for event_type in &manifest.event_subscriptions {
            self.event_handlers.entry(*event_type).or_default().push(manifest.id.clone());
        }

        // Register host functions
        for func in &manifest.host_functions {
            let full_name = format!("{}::{}", func.module, func.name);
            if self.host_functions.contains_key(&full_name) {
                return Err(PluginError::FunctionConflict(full_name));
            }
            self.host_functions.insert(full_name, manifest.id.clone());
        }

        let instance = PluginInstance {
            manifest,
            state: PluginState::Registered,
            config: HashMap::new(),
            handler,
        };

        self.plugins.insert(instance.manifest.id.clone(), instance);
        Ok(())
    }

    /// Load a plugin with configuration.
    pub fn load(
        &mut self,
        plugin_id: &PluginId,
        config: HashMap<String, serde_json::Value>,
    ) -> Result<(), PluginError> {
        let instance = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.0.clone()))?;

        instance.state = PluginState::Loading;
        instance.config = config.clone();

        match instance.handler.init(&config) {
            Ok(()) => {
                instance.state = PluginState::Active;
                Ok(())
            }
            Err(e) => {
                instance.state = PluginState::Failed;
                Err(e)
            }
        }
    }

    /// Disable a plugin.
    pub fn disable(&mut self, plugin_id: &PluginId) -> Result<(), PluginError> {
        let instance = self
            .plugins
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.0.clone()))?;

        instance.handler.shutdown()?;
        instance.state = PluginState::Disabled;
        Ok(())
    }

    /// Unregister a plugin.
    pub fn unregister(&mut self, plugin_id: &PluginId) -> Result<(), PluginError> {
        let instance = self
            .plugins
            .remove(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.0.clone()))?;

        // Remove event handlers
        for event_type in &instance.manifest.event_subscriptions {
            if let Some(handlers) = self.event_handlers.get_mut(event_type) {
                handlers.retain(|id| id != plugin_id);
            }
        }

        // Remove host functions
        for func in &instance.manifest.host_functions {
            let full_name = format!("{}::{}", func.module, func.name);
            self.host_functions.remove(&full_name);
        }

        Ok(())
    }

    /// Dispatch an event.
    pub fn dispatch_event(&self, event: &Event) -> Vec<Result<(), PluginError>> {
        let mut results = Vec::new();

        if let Some(handler_ids) = self.event_handlers.get(&event.event_type) {
            for plugin_id in handler_ids {
                if let Some(instance) = self.plugins.get(plugin_id) {
                    if instance.state == PluginState::Active {
                        results.push(instance.handler.handle_event(event));
                    }
                }
            }
        }

        results
    }

    /// Invoke a host function.
    pub fn invoke_host_function(
        &self,
        module: &str,
        name: &str,
        params: &[serde_json::Value],
    ) -> Result<Vec<serde_json::Value>, PluginError> {
        let full_name = format!("{}::{}", module, name);

        let plugin_id = self
            .host_functions
            .get(&full_name)
            .ok_or_else(|| PluginError::FunctionNotFound(full_name.clone()))?;

        let instance = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.0.clone()))?;

        if instance.state != PluginState::Active {
            return Err(PluginError::NotActive(plugin_id.0.clone()));
        }

        instance.handler.invoke_host_function(name, params)
    }

    /// Get plugin by ID.
    pub fn get(&self, plugin_id: &PluginId) -> Option<&PluginInstance> {
        self.plugins.get(plugin_id)
    }

    /// List all plugins.
    pub fn list(&self) -> Vec<&PluginManifest> {
        self.plugins.values().map(|p| &p.manifest).collect()
    }

    /// List active plugins.
    pub fn list_active(&self) -> Vec<&PluginManifest> {
        self.plugins
            .values()
            .filter(|p| p.state == PluginState::Active)
            .map(|p| &p.manifest)
            .collect()
    }

    /// Get available host functions.
    pub fn available_host_functions(&self) -> Vec<&HostFunctionSpec> {
        self.plugins
            .values()
            .filter(|p| p.state == PluginState::Active)
            .flat_map(|p| &p.manifest.host_functions)
            .collect()
    }
}

/// Plugin error.
#[derive(Debug, Clone)]
pub enum PluginError {
    /// Plugin not found.
    NotFound(String),
    /// Plugin already registered.
    AlreadyRegistered(String),
    /// Plugin not active.
    NotActive(String),
    /// Function not found.
    FunctionNotFound(String),
    /// Function conflict.
    FunctionConflict(String),
    /// Initialization failed.
    InitFailed(String),
    /// Handler error.
    HandlerError(String),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Plugin not found: {}", id),
            Self::AlreadyRegistered(id) => write!(f, "Plugin already registered: {}", id),
            Self::NotActive(id) => write!(f, "Plugin not active: {}", id),
            Self::FunctionNotFound(name) => write!(f, "Host function not found: {}", name),
            Self::FunctionConflict(name) => write!(f, "Host function conflict: {}", name),
            Self::InitFailed(msg) => write!(f, "Initialization failed: {}", msg),
            Self::HandlerError(msg) => write!(f, "Handler error: {}", msg),
        }
    }
}

impl std::error::Error for PluginError {}

/// Middleware plugin for request/response processing.
pub trait Middleware: Send + Sync {
    /// Process before sandbox execution.
    fn before_execute(&self, ctx: &mut MiddlewareContext) -> Result<(), PluginError>;

    /// Process after sandbox execution.
    fn after_execute(&self, ctx: &mut MiddlewareContext) -> Result<(), PluginError>;
}

/// Middleware context.
#[derive(Debug, Default)]
pub struct MiddlewareContext {
    /// Sandbox ID.
    pub sandbox_id: String,
    /// Input data.
    pub input: Vec<u8>,
    /// Output data.
    pub output: Option<Vec<u8>>,
    /// Metadata.
    pub metadata: HashMap<String, String>,
    /// Should skip execution.
    pub skip: bool,
    /// Modified input.
    pub modified_input: Option<Vec<u8>>,
}

/// Builder for plugin manifests.
pub struct ManifestBuilder {
    manifest: PluginManifest,
}

impl ManifestBuilder {
    /// Create new builder.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            manifest: PluginManifest {
                id: PluginId::new(id),
                name: name.into(),
                version: "0.1.0".to_string(),
                description: None,
                author: None,
                license: None,
                plugin_type: PluginType::Composite,
                required_capabilities: Vec::new(),
                host_functions: Vec::new(),
                event_subscriptions: Vec::new(),
                config_schema: None,
            },
        }
    }

    /// Set version.
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.manifest.version = version.into();
        self
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.manifest.description = Some(desc.into());
        self
    }

    /// Set author.
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.manifest.author = Some(author.into());
        self
    }

    /// Set plugin type.
    pub fn plugin_type(mut self, t: PluginType) -> Self {
        self.manifest.plugin_type = t;
        self
    }

    /// Add host function.
    pub fn host_function(mut self, spec: HostFunctionSpec) -> Self {
        self.manifest.host_functions.push(spec);
        self
    }

    /// Subscribe to event.
    pub fn subscribe(mut self, event: EventType) -> Self {
        self.manifest.event_subscriptions.push(event);
        self
    }

    /// Build manifest.
    pub fn build(self) -> PluginManifest {
        self.manifest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler {
        init_called: std::sync::atomic::AtomicBool,
        events_received: std::sync::Mutex<Vec<EventType>>,
    }

    impl TestHandler {
        fn new() -> Self {
            Self {
                init_called: std::sync::atomic::AtomicBool::new(false),
                events_received: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl PluginHandler for TestHandler {
        fn init(
            &mut self,
            _config: &HashMap<String, serde_json::Value>,
        ) -> Result<(), PluginError> {
            self.init_called.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn handle_event(&self, event: &Event) -> Result<(), PluginError> {
            self.events_received.lock().unwrap().push(event.event_type);
            Ok(())
        }

        fn invoke_host_function(
            &self,
            name: &str,
            params: &[serde_json::Value],
        ) -> Result<Vec<serde_json::Value>, PluginError> {
            if name == "add" && params.len() == 2 {
                let a = params[0].as_i64().unwrap_or(0);
                let b = params[1].as_i64().unwrap_or(0);
                Ok(vec![serde_json::json!(a + b)])
            } else {
                Err(PluginError::FunctionNotFound(name.to_string()))
            }
        }

        fn shutdown(&mut self) -> Result<(), PluginError> {
            Ok(())
        }
    }

    #[test]
    fn test_plugin_registration() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("test-plugin", "Test Plugin")
            .version("1.0.0")
            .description("A test plugin")
            .plugin_type(PluginType::HostFunctions)
            .build();

        registry.register(manifest, Box::new(NoopHandler::default())).unwrap();
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn test_plugin_load() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("test", "Test").build();

        registry.register(manifest, Box::new(NoopHandler::default())).unwrap();
        registry.load(&PluginId::new("test"), HashMap::new()).unwrap();

        let plugin = registry.get(&PluginId::new("test")).unwrap();
        assert_eq!(plugin.state(), PluginState::Active);
    }

    #[test]
    fn test_event_dispatch() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("event-handler", "Event Handler")
            .subscribe(EventType::SandboxCreated)
            .subscribe(EventType::SandboxCompleted)
            .build();

        registry.register(manifest, Box::new(TestHandler::new())).unwrap();
        registry.load(&PluginId::new("event-handler"), HashMap::new()).unwrap();

        let event = Event {
            event_type: EventType::SandboxCreated,
            sandbox_id: Some("sb-1".to_string()),
            timestamp: std::time::SystemTime::now(),
            data: HashMap::new(),
        };

        let results = registry.dispatch_event(&event);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn test_host_function_invocation() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("math", "Math Plugin")
            .host_function(HostFunctionSpec {
                name: "add".to_string(),
                module: "math".to_string(),
                params: vec![ValueType::I64, ValueType::I64],
                returns: vec![ValueType::I64],
                description: Some("Add two numbers".to_string()),
            })
            .build();

        registry.register(manifest, Box::new(TestHandler::new())).unwrap();
        registry.load(&PluginId::new("math"), HashMap::new()).unwrap();

        let result = registry
            .invoke_host_function("math", "add", &[serde_json::json!(2), serde_json::json!(3)])
            .unwrap();

        assert_eq!(result[0], serde_json::json!(5));
    }

    #[test]
    fn test_plugin_disable() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("test", "Test").build();
        registry.register(manifest, Box::new(NoopHandler::default())).unwrap();
        registry.load(&PluginId::new("test"), HashMap::new()).unwrap();

        registry.disable(&PluginId::new("test")).unwrap();

        let plugin = registry.get(&PluginId::new("test")).unwrap();
        assert_eq!(plugin.state(), PluginState::Disabled);
    }

    #[test]
    fn test_plugin_unregister() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("temp", "Temporary").build();
        registry.register(manifest, Box::new(NoopHandler::default())).unwrap();

        registry.unregister(&PluginId::new("temp")).unwrap();
        assert!(registry.get(&PluginId::new("temp")).is_none());
    }

    #[test]
    fn test_duplicate_registration() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("dup", "Duplicate").build();
        registry.register(manifest.clone(), Box::new(NoopHandler::default())).unwrap();

        let result = registry.register(manifest, Box::new(NoopHandler::default()));
        assert!(matches!(result, Err(PluginError::AlreadyRegistered(_))));
    }

    #[test]
    fn test_function_conflict() {
        let mut registry = PluginRegistry::new();

        let manifest1 = ManifestBuilder::new("p1", "Plugin 1")
            .host_function(HostFunctionSpec {
                name: "func".to_string(),
                module: "shared".to_string(),
                params: vec![],
                returns: vec![],
                description: None,
            })
            .build();

        let manifest2 = ManifestBuilder::new("p2", "Plugin 2")
            .host_function(HostFunctionSpec {
                name: "func".to_string(),
                module: "shared".to_string(),
                params: vec![],
                returns: vec![],
                description: None,
            })
            .build();

        registry.register(manifest1, Box::new(NoopHandler::default())).unwrap();
        let result = registry.register(manifest2, Box::new(NoopHandler::default()));

        assert!(matches!(result, Err(PluginError::FunctionConflict(_))));
    }

    #[test]
    fn test_available_host_functions() {
        let mut registry = PluginRegistry::new();

        let manifest = ManifestBuilder::new("funcs", "Functions")
            .host_function(HostFunctionSpec {
                name: "f1".to_string(),
                module: "m".to_string(),
                params: vec![],
                returns: vec![],
                description: None,
            })
            .host_function(HostFunctionSpec {
                name: "f2".to_string(),
                module: "m".to_string(),
                params: vec![],
                returns: vec![],
                description: None,
            })
            .build();

        registry.register(manifest, Box::new(NoopHandler::default())).unwrap();
        registry.load(&PluginId::new("funcs"), HashMap::new()).unwrap();

        let funcs = registry.available_host_functions();
        assert_eq!(funcs.len(), 2);
    }
}
