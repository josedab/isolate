//! Host functions and state management.

use crate::capability::CapabilityEnforcer;
use crate::error::Result;
use crate::resource::ResourceMeter;
use std::collections::HashMap;
use std::sync::Arc;

/// Host-provided functions that can be called from WASM.
pub struct HostFunctions {
    functions: HashMap<String, Box<dyn HostFn>>,
}

/// Trait for host functions.
pub trait HostFn: Send + Sync {
    /// Call the host function with the given arguments.
    fn call(&self, args: &[u8]) -> Result<Vec<u8>>;

    /// Get the function name.
    fn name(&self) -> &str;
}

impl HostFunctions {
    /// Create a new empty host functions registry.
    pub fn new() -> Self {
        Self { functions: HashMap::new() }
    }

    /// Register a host function.
    pub fn register<F: HostFn + 'static>(&mut self, func: F) {
        self.functions.insert(func.name().to_string(), Box::new(func));
    }

    /// Call a host function by name.
    pub fn call(&self, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        match self.functions.get(name) {
            Some(func) => func.call(args),
            None => Err(crate::error::Error::FunctionNotFound(name.to_string())),
        }
    }

    /// Check if a function is registered.
    pub fn has(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Get all registered function names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.functions.keys().map(|s| s.as_str())
    }
}

impl Default for HostFunctions {
    fn default() -> Self {
        Self::new()
    }
}

/// State shared with the WASM instance.
pub struct HostState {
    /// Capability enforcer.
    enforcer: CapabilityEnforcer,
    /// Resource meter.
    meter: ResourceMeter,
    /// Custom host functions.
    host_functions: Arc<HostFunctions>,
}

impl HostState {
    /// Create a new host state.
    pub fn new(enforcer: CapabilityEnforcer, meter: ResourceMeter) -> Self {
        Self { enforcer, meter, host_functions: Arc::new(HostFunctions::new()) }
    }

    /// Create with custom host functions.
    pub fn with_host_functions(
        enforcer: CapabilityEnforcer,
        meter: ResourceMeter,
        host_functions: Arc<HostFunctions>,
    ) -> Self {
        Self { enforcer, meter, host_functions }
    }

    /// Get the capability enforcer.
    pub fn enforcer(&self) -> &CapabilityEnforcer {
        &self.enforcer
    }

    /// Get the resource meter.
    pub fn meter(&self) -> &ResourceMeter {
        &self.meter
    }

    /// Get the host functions.
    pub fn host_functions(&self) -> &HostFunctions {
        &self.host_functions
    }

    /// Call a host function.
    pub fn call_host_function(&self, name: &str, args: &[u8]) -> Result<Vec<u8>> {
        // Check capability first
        self.enforcer.check_host_function(name)?;
        // Then call
        self.host_functions.call(name, args)
    }
}

/// A simple logging host function.
#[allow(dead_code)] // Public API: available for consumers to register as a host function
pub struct LogFunction;

impl HostFn for LogFunction {
    fn call(&self, args: &[u8]) -> Result<Vec<u8>> {
        let message = String::from_utf8_lossy(args);
        tracing::info!(target: "sandbox", message = %message, "guest log");
        Ok(Vec::new())
    }

    fn name(&self) -> &str {
        "log"
    }
}

/// A host function that returns the current time.
#[allow(dead_code)] // Public API: available for consumers to register as a host function
pub struct TimeFunction;

impl HostFn for TimeFunction {
    fn call(&self, _args: &[u8]) -> Result<Vec<u8>> {
        let now =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();

        Ok(now.as_secs().to_le_bytes().to_vec())
    }

    fn name(&self) -> &str {
        "time"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::resource::ResourceLimits;
    use uuid::Uuid;

    #[test]
    fn test_host_functions_registry() {
        let mut funcs = HostFunctions::new();

        funcs.register(LogFunction);
        funcs.register(TimeFunction);

        assert!(funcs.has("log"));
        assert!(funcs.has("time"));
        assert!(!funcs.has("unknown"));

        let names: Vec<_> = funcs.names().collect();
        assert_eq!(names.len(), 2);
    }

    #[test]
    fn test_host_functions_call() {
        let mut funcs = HostFunctions::new();
        funcs.register(LogFunction);

        let result = funcs.call("log", b"test message");
        assert!(result.is_ok());
    }

    #[test]
    fn test_host_state() {
        let caps = CapabilitySet::new();
        let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
        let meter = ResourceMeter::new(ResourceLimits::default());

        let state = HostState::new(enforcer, meter);

        assert!(state.host_functions().names().next().is_none());
    }
}
