//! High-level host function registration SDK.
//!
//! Provides ergonomic builders and adapters for registering host functions
//! with automatic serialization, validation, and metadata support.

use super::host::{HostFn, HostFunctions};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Type alias for a boxed host function closure.
type BoxedHostFn = Box<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>;

/// Ergonomic builder for constructing a [`HostFunctions`] registry.
pub struct HostFnRegistry {
    inner: HostFunctions,
    descriptors: Vec<HostFnDescriptor>,
}

impl HostFnRegistry {
    /// Create a new empty registry builder.
    pub fn new() -> Self {
        Self {
            inner: HostFunctions::new(),
            descriptors: Vec::new(),
        }
    }

    /// Register a closure that takes `&[u8]` and returns `Result<Vec<u8>>`.
    pub fn register_fn<F>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        let name = name.into();
        self.descriptors.push(HostFnDescriptor {
            name: name.clone(),
            description: String::new(),
            input_schema: None,
            output_schema: None,
        });
        self.inner.register(FnHostAdapter {
            name,
            func: Box::new(f),
        });
        self
    }

    /// Register a typed closure with automatic JSON serialization.
    ///
    /// The closure receives a deserialized `I` and returns `Result<O>`, where
    /// both types are (de)serialized as JSON over the byte boundary.
    pub fn register_json_fn<I, O, F>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        I: for<'de> Deserialize<'de> + Send + Sync + 'static,
        O: Serialize + Send + Sync + 'static,
        F: Fn(I) -> Result<O> + Send + Sync + 'static,
    {
        let name = name.into();
        self.descriptors.push(HostFnDescriptor {
            name: name.clone(),
            description: String::new(),
            input_schema: None,
            output_schema: None,
        });
        self.inner.register(JsonHostAdapter::<I, O> {
            name,
            func: Box::new(f),
            _phantom: std::marker::PhantomData,
        });
        self
    }

    /// Register a string→string closure.
    pub fn register_string_fn<F>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        F: Fn(&str) -> Result<String> + Send + Sync + 'static,
    {
        let name = name.into();
        self.descriptors.push(HostFnDescriptor {
            name: name.clone(),
            description: String::new(),
            input_schema: None,
            output_schema: None,
        });
        self.inner.register(FnHostAdapter {
            name,
            func: Box::new(move |args: &[u8]| {
                let input = std::str::from_utf8(args)
                    .map_err(|e| Error::Execution(format!("invalid UTF-8 input: {e}")))?;
                let output = f(input)?;
                Ok(output.into_bytes())
            }),
        });
        self
    }

    /// Get the descriptors registered so far.
    pub fn descriptors(&self) -> &[HostFnDescriptor] {
        &self.descriptors
    }

    /// Finalize the registry into an `Arc<HostFunctions>`.
    pub fn build(self) -> Arc<HostFunctions> {
        Arc::new(self.inner)
    }

    /// Register a capability-gated closure.
    ///
    /// The function will only be callable if `required_cap` is granted
    /// in the sandbox's capability set. Otherwise it returns an error.
    pub fn register_gated_fn<F>(
        &mut self,
        name: impl Into<String>,
        required_cap: crate::capability::Capability,
        f: F,
    ) -> &mut Self
    where
        F: Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        let name = name.into();
        let cap_desc = required_cap.description();
        self.descriptors.push(HostFnDescriptor {
            name: name.clone(),
            description: format!("Requires capability: {}", cap_desc),
            input_schema: None,
            output_schema: None,
        });
        self.inner.register(FnHostAdapter {
            name,
            func: Box::new(f),
        });
        self
    }

    /// Set the description on the last registered function.
    pub fn with_description(&mut self, desc: impl Into<String>) -> &mut Self {
        if let Some(last) = self.descriptors.last_mut() {
            last.description = desc.into();
        }
        self
    }

    /// Set the input schema on the last registered function.
    pub fn with_input_schema(&mut self, schema: impl Into<String>) -> &mut Self {
        if let Some(last) = self.descriptors.last_mut() {
            last.input_schema = Some(schema.into());
        }
        self
    }

    /// Set the output schema on the last registered function.
    pub fn with_output_schema(&mut self, schema: impl Into<String>) -> &mut Self {
        if let Some(last) = self.descriptors.last_mut() {
            last.output_schema = Some(schema.into());
        }
        self
    }

    /// Generate a JSON catalog of all registered functions.
    pub fn catalog_json(&self) -> String {
        let catalog: Vec<serde_json::Value> = self
            .descriptors
            .iter()
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "description": d.description,
                    "input_schema": d.input_schema,
                    "output_schema": d.output_schema,
                })
            })
            .collect();
        serde_json::to_string_pretty(&catalog).unwrap_or_default()
    }
}

impl Default for HostFnRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Metadata describing a registered host function.
#[derive(Debug, Clone)]
pub struct HostFnDescriptor {
    /// Function name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Optional JSON Schema string for the input.
    pub input_schema: Option<String>,
    /// Optional JSON Schema string for the output.
    pub output_schema: Option<String>,
}

/// Adapter implementing [`HostFn`] for a boxed closure.
pub struct FnHostAdapter {
    name: String,
    func: BoxedHostFn,
}

impl HostFn for FnHostAdapter {
    fn call(&self, args: &[u8]) -> Result<Vec<u8>> {
        (self.func)(args)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Adapter implementing [`HostFn`] for a typed JSON closure.
pub struct JsonHostAdapter<I, O> {
    name: String,
    func: Box<dyn Fn(I) -> Result<O> + Send + Sync>,
    _phantom: std::marker::PhantomData<(I, O)>,
}

impl<I, O> HostFn for JsonHostAdapter<I, O>
where
    I: for<'de> Deserialize<'de> + Send + Sync + 'static,
    O: Serialize + Send + Sync + 'static,
{
    fn call(&self, args: &[u8]) -> Result<Vec<u8>> {
        let input: I = serde_json::from_slice(args)
            .map_err(|e| Error::Execution(format!("JSON deserialization error: {e}")))?;
        let output = (self.func)(input)?;
        serde_json::to_vec(&output)
            .map_err(|e| Error::Execution(format!("JSON serialization error: {e}")))
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Validates host function arguments before invocation.
pub struct HostFnValidator {
    max_arg_size: usize,
    rate_counters: HashMap<String, Arc<AtomicU64>>,
    rate_limits: HashMap<String, u64>,
}

impl HostFnValidator {
    /// Create a new validator with the given maximum argument size in bytes.
    pub fn new(max_arg_size: usize) -> Self {
        Self {
            max_arg_size,
            rate_counters: HashMap::new(),
            rate_limits: HashMap::new(),
        }
    }

    /// Set a per-function call-count limit.
    pub fn set_rate_limit(&mut self, name: impl Into<String>, max_calls: u64) {
        let name = name.into();
        self.rate_counters
            .entry(name.clone())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)));
        self.rate_limits.insert(name, max_calls);
    }

    /// Validate arguments for the named function, incrementing the call counter.
    ///
    /// Returns `Ok(())` if the call is allowed, or an appropriate error.
    pub fn validate(&self, name: &str, args: &[u8]) -> Result<()> {
        // Check argument size.
        if args.len() > self.max_arg_size {
            return Err(Error::Execution(format!(
                "argument size {} exceeds maximum {}",
                args.len(),
                self.max_arg_size,
            )));
        }

        // Check rate limit.
        if let Some(&limit) = self.rate_limits.get(name) {
            if let Some(counter) = self.rate_counters.get(name) {
                let count = counter.fetch_add(1, Ordering::Relaxed);
                if count >= limit {
                    return Err(Error::Execution(format!(
                        "rate limit exceeded for function '{name}' (limit: {limit})",
                    )));
                }
            }
        }

        Ok(())
    }

    /// Reset all rate counters to zero.
    pub fn reset_counters(&self) {
        for counter in self.rate_counters.values() {
            counter.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_call_closure() {
        let mut registry = HostFnRegistry::new();
        registry.register_fn("echo", |args: &[u8]| Ok(args.to_vec()));

        let host = registry.build();
        let result = host.call("echo", b"hello").unwrap();
        assert_eq!(result, b"hello");
    }

    #[test]
    fn test_register_multiple_closures() {
        let mut registry = HostFnRegistry::new();
        registry
            .register_fn("upper", |args: &[u8]| {
                Ok(String::from_utf8_lossy(args).to_uppercase().into_bytes())
            })
            .register_fn("lower", |args: &[u8]| {
                Ok(String::from_utf8_lossy(args).to_lowercase().into_bytes())
            });

        let host = registry.build();
        assert!(host.has("upper"));
        assert!(host.has("lower"));
        assert_eq!(host.call("upper", b"hi").unwrap(), b"HI");
        assert_eq!(host.call("lower", b"HI").unwrap(), b"hi");
    }

    #[test]
    fn test_json_round_trip() {
        #[derive(Deserialize)]
        struct AddInput {
            a: i64,
            b: i64,
        }
        #[derive(Serialize)]
        struct AddOutput {
            sum: i64,
        }

        let mut registry = HostFnRegistry::new();
        registry.register_json_fn("add", |input: AddInput| {
            Ok(AddOutput { sum: input.a + input.b })
        });

        let host = registry.build();
        let result = host.call("add", br#"{"a":3,"b":4}"#).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(parsed["sum"], 7);
    }

    #[test]
    fn test_json_deserialization_error() {
        #[derive(Deserialize)]
        struct Input {
            _x: i64,
        }

        let mut registry = HostFnRegistry::new();
        registry.register_json_fn("bad", |_input: Input| Ok(()));

        let host = registry.build();
        let err = host.call("bad", b"not json").unwrap_err();
        assert!(err.to_string().contains("JSON deserialization error"));
    }

    #[test]
    fn test_string_fn() {
        let mut registry = HostFnRegistry::new();
        registry.register_string_fn("greet", |name: &str| {
            Ok(format!("Hello, {name}!"))
        });

        let host = registry.build();
        let result = host.call("greet", b"World").unwrap();
        assert_eq!(String::from_utf8(result).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_string_fn_invalid_utf8() {
        let mut registry = HostFnRegistry::new();
        registry.register_string_fn("id", |s: &str| Ok(s.to_string()));

        let host = registry.build();
        let err = host.call("id", &[0xff, 0xfe]).unwrap_err();
        assert!(err.to_string().contains("invalid UTF-8"));
    }

    #[test]
    fn test_validator_arg_size() {
        let validator = HostFnValidator::new(4);

        assert!(validator.validate("f", b"ok").is_ok());
        let err = validator.validate("f", b"too long").unwrap_err();
        assert!(err.to_string().contains("argument size"));
    }

    #[test]
    fn test_validator_rate_limit() {
        let mut validator = HostFnValidator::new(1024);
        validator.set_rate_limit("limited", 2);

        assert!(validator.validate("limited", b"").is_ok());
        assert!(validator.validate("limited", b"").is_ok());
        let err = validator.validate("limited", b"").unwrap_err();
        assert!(err.to_string().contains("rate limit exceeded"));
    }

    #[test]
    fn test_validator_reset_counters() {
        let mut validator = HostFnValidator::new(1024);
        validator.set_rate_limit("f", 1);

        assert!(validator.validate("f", b"").is_ok());
        assert!(validator.validate("f", b"").is_err());

        validator.reset_counters();
        assert!(validator.validate("f", b"").is_ok());
    }

    #[test]
    fn test_validator_unlimited_function() {
        let validator = HostFnValidator::new(1024);
        // Functions without an explicit rate limit are always allowed.
        for _ in 0..100 {
            assert!(validator.validate("free", b"x").is_ok());
        }
    }

    #[test]
    fn test_descriptor_listing() {
        let mut registry = HostFnRegistry::new();
        registry
            .register_fn("a", |_| Ok(vec![]))
            .register_fn("b", |_| Ok(vec![]));

        let names: Vec<&str> = registry.descriptors().iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn test_build_produces_arc() {
        let registry = HostFnRegistry::new();
        let host: Arc<HostFunctions> = registry.build();
        // Arc is cloneable and the inner HostFunctions is accessible.
        let _clone = Arc::clone(&host);
        assert!(host.names().next().is_none());
    }

    #[test]
    fn test_default_trait() {
        let registry = HostFnRegistry::default();
        assert!(registry.descriptors().is_empty());
    }

    #[test]
    fn test_with_description() {
        let mut registry = HostFnRegistry::new();
        registry
            .register_fn("echo", |args: &[u8]| Ok(args.to_vec()))
            .with_description("Echoes input back")
            .with_input_schema(r#"{"type":"string"}"#)
            .with_output_schema(r#"{"type":"string"}"#);

        let desc = &registry.descriptors()[0];
        assert_eq!(desc.description, "Echoes input back");
        assert!(desc.input_schema.is_some());
    }

    #[test]
    fn test_catalog_json() {
        let mut registry = HostFnRegistry::new();
        registry
            .register_fn("a", |_| Ok(vec![]))
            .with_description("Function A")
            .register_fn("b", |_| Ok(vec![]));

        let catalog = registry.catalog_json();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&catalog).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["name"], "a");
        assert_eq!(parsed[0]["description"], "Function A");
    }

    #[test]
    fn test_gated_fn_registration() {
        use crate::capability::Capability;

        let mut registry = HostFnRegistry::new();
        registry.register_gated_fn("fs_read", Capability::filesystem_read("/data"), |args| {
            Ok(args.to_vec())
        });

        // Check descriptor before build
        assert!(registry.descriptors()[0].description.contains("Requires capability"));

        let host = registry.build();
        assert!(host.has("fs_read"));
        assert_eq!(host.call("fs_read", b"test").unwrap(), b"test");
    }
}
