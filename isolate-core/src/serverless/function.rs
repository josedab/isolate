use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// A serverless function backed by an Isolate sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerlessFunction {
    pub name: String,
    pub description: String,
    pub handler: HandlerConfig,
    pub runtime: RuntimeConfig,
    pub triggers: Vec<Trigger>,
    pub scaling: ScalingConfig,
    pub environment: HashMap<String, String>,
    pub labels: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerConfig {
    pub module_source: ModuleSource,
    pub entry_point: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModuleSource {
    LocalPath(String),
    Registry { name: String, version: String },
    Inline(Vec<u8>),
    Url(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub memory_mb: u64,
    pub timeout: Duration,
    pub fuel_limit: Option<u64>,
    pub concurrency: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            memory_mb: 128,
            timeout: Duration::from_secs(30),
            fuel_limit: Some(100_000_000),
            concurrency: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Trigger {
    Http { path: String, methods: Vec<HttpMethod> },
    Schedule { cron: String },
    Queue { queue_name: String, batch_size: u32 },
    Event { source: String, event_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingConfig {
    pub min_instances: u32,
    pub max_instances: u32,
    pub scale_to_zero: bool,
    pub target_concurrency: u32,
    pub scale_up_threshold: f64,
    pub cool_down_secs: u64,
}

impl Default for ScalingConfig {
    fn default() -> Self {
        Self {
            min_instances: 0,
            max_instances: 100,
            scale_to_zero: true,
            target_concurrency: 5,
            scale_up_threshold: 0.7,
            cool_down_secs: 300,
        }
    }
}

/// Builder for ServerlessFunction.
pub struct FunctionBuilder {
    function: ServerlessFunction,
}

impl FunctionBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            function: ServerlessFunction {
                name: name.into(),
                description: String::new(),
                handler: HandlerConfig {
                    module_source: ModuleSource::LocalPath(String::new()),
                    entry_point: "_start".to_string(),
                    capabilities: Vec::new(),
                },
                runtime: RuntimeConfig::default(),
                triggers: Vec::new(),
                scaling: ScalingConfig::default(),
                environment: HashMap::new(),
                labels: HashMap::new(),
            },
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.function.description = desc.into();
        self
    }

    pub fn module_path(mut self, path: impl Into<String>) -> Self {
        self.function.handler.module_source = ModuleSource::LocalPath(path.into());
        self
    }

    pub fn module_registry(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.function.handler.module_source =
            ModuleSource::Registry { name: name.into(), version: version.into() };
        self
    }

    pub fn entry_point(mut self, ep: impl Into<String>) -> Self {
        self.function.handler.entry_point = ep.into();
        self
    }

    pub fn capability(mut self, cap: impl Into<String>) -> Self {
        self.function.handler.capabilities.push(cap.into());
        self
    }

    pub fn memory_mb(mut self, mb: u64) -> Self {
        self.function.runtime.memory_mb = mb;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.function.runtime.timeout = timeout;
        self
    }

    pub fn concurrency(mut self, concurrency: u32) -> Self {
        self.function.runtime.concurrency = concurrency;
        self
    }

    pub fn fuel_limit(mut self, fuel: u64) -> Self {
        self.function.runtime.fuel_limit = Some(fuel);
        self
    }

    pub fn http_trigger(mut self, path: impl Into<String>, methods: Vec<HttpMethod>) -> Self {
        self.function.triggers.push(Trigger::Http { path: path.into(), methods });
        self
    }

    pub fn schedule_trigger(mut self, cron: impl Into<String>) -> Self {
        self.function.triggers.push(Trigger::Schedule { cron: cron.into() });
        self
    }

    pub fn queue_trigger(mut self, queue: impl Into<String>, batch_size: u32) -> Self {
        self.function.triggers.push(Trigger::Queue { queue_name: queue.into(), batch_size });
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.function.environment.insert(key.into(), value.into());
        self
    }

    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.function.labels.insert(key.into(), value.into());
        self
    }

    pub fn scaling(mut self, config: ScalingConfig) -> Self {
        self.function.scaling = config;
        self
    }

    pub fn build(self) -> ServerlessFunction {
        self.function
    }
}

/// Invocation request mapped to sandbox input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationRequest {
    pub function_name: String,
    pub payload: serde_json::Value,
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub request_id: String,
}

/// Invocation response from sandbox output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationResponse {
    pub status_code: u16,
    pub body: serde_json::Value,
    pub headers: HashMap<String, String>,
    pub duration_ms: u64,
    pub request_id: String,
    pub cold_start: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_builder_basic() {
        let func = FunctionBuilder::new("my-func")
            .description("A test function")
            .module_path("/path/to/module.wasm")
            .build();

        assert_eq!(func.name, "my-func");
        assert_eq!(func.description, "A test function");
        assert!(matches!(
            func.handler.module_source,
            ModuleSource::LocalPath(ref p) if p == "/path/to/module.wasm"
        ));
    }

    #[test]
    fn test_function_builder_with_triggers() {
        let func = FunctionBuilder::new("api-func")
            .http_trigger("/api/hello", vec![HttpMethod::Get, HttpMethod::Post])
            .schedule_trigger("0 * * * *")
            .queue_trigger("my-queue", 10)
            .build();

        assert_eq!(func.triggers.len(), 3);
        assert!(
            matches!(&func.triggers[0], Trigger::Http { path, methods } if path == "/api/hello" && methods.len() == 2)
        );
        assert!(matches!(&func.triggers[1], Trigger::Schedule { cron } if cron == "0 * * * *"));
        assert!(
            matches!(&func.triggers[2], Trigger::Queue { queue_name, batch_size } if queue_name == "my-queue" && *batch_size == 10)
        );
    }

    #[test]
    fn test_function_builder_with_capabilities() {
        let func = FunctionBuilder::new("cap-func")
            .capability("stdout")
            .capability("filesystem_read")
            .build();

        assert_eq!(func.handler.capabilities.len(), 2);
        assert_eq!(func.handler.capabilities[0], "stdout");
        assert_eq!(func.handler.capabilities[1], "filesystem_read");
    }

    #[test]
    fn test_function_builder_runtime_config() {
        let func = FunctionBuilder::new("rt-func")
            .memory_mb(256)
            .timeout(Duration::from_secs(60))
            .concurrency(20)
            .fuel_limit(500_000)
            .build();

        assert_eq!(func.runtime.memory_mb, 256);
        assert_eq!(func.runtime.timeout, Duration::from_secs(60));
        assert_eq!(func.runtime.concurrency, 20);
        assert_eq!(func.runtime.fuel_limit, Some(500_000));
    }

    #[test]
    fn test_function_builder_env_and_labels() {
        let func = FunctionBuilder::new("env-func")
            .env("DATABASE_URL", "postgres://localhost")
            .env("LOG_LEVEL", "debug")
            .label("team", "backend")
            .build();

        assert_eq!(func.environment.len(), 2);
        assert_eq!(func.environment["DATABASE_URL"], "postgres://localhost");
        assert_eq!(func.labels["team"], "backend");
    }

    #[test]
    fn test_function_builder_module_registry() {
        let func = FunctionBuilder::new("reg-func").module_registry("my-module", "1.2.3").build();

        assert!(matches!(
            func.handler.module_source,
            ModuleSource::Registry { ref name, ref version } if name == "my-module" && version == "1.2.3"
        ));
    }

    #[test]
    fn test_default_runtime_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.memory_mb, 128);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.fuel_limit, Some(100_000_000));
        assert_eq!(config.concurrency, 10);
    }

    #[test]
    fn test_default_scaling_config() {
        let config = ScalingConfig::default();
        assert_eq!(config.min_instances, 0);
        assert_eq!(config.max_instances, 100);
        assert!(config.scale_to_zero);
        assert_eq!(config.target_concurrency, 5);
        assert!((config.scale_up_threshold - 0.7).abs() < f64::EPSILON);
        assert_eq!(config.cool_down_secs, 300);
    }

    #[test]
    fn test_function_builder_scaling() {
        let scaling = ScalingConfig {
            min_instances: 2,
            max_instances: 50,
            scale_to_zero: false,
            ..ScalingConfig::default()
        };
        let func = FunctionBuilder::new("scaled-func").scaling(scaling).build();

        assert_eq!(func.scaling.min_instances, 2);
        assert_eq!(func.scaling.max_instances, 50);
        assert!(!func.scaling.scale_to_zero);
    }

    #[test]
    fn test_invocation_request_serialization() {
        let req = InvocationRequest {
            function_name: "my-func".to_string(),
            payload: serde_json::json!({"key": "value"}),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            request_id: "req-123".to_string(),
        };

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: InvocationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.function_name, "my-func");
        assert_eq!(deserialized.request_id, "req-123");
    }

    #[test]
    fn test_invocation_response_serialization() {
        let resp = InvocationResponse {
            status_code: 200,
            body: serde_json::json!({"result": "ok"}),
            headers: HashMap::from([("content-type".to_string(), "application/json".to_string())]),
            duration_ms: 42,
            request_id: "req-456".to_string(),
            cold_start: true,
        };

        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: InvocationResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status_code, 200);
        assert!(deserialized.cold_start);
        assert_eq!(deserialized.duration_ms, 42);
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(HttpMethod::Get.to_string(), "GET");
        assert_eq!(HttpMethod::Post.to_string(), "POST");
        assert_eq!(HttpMethod::Delete.to_string(), "DELETE");
    }

    #[test]
    fn test_function_default_entry_point() {
        let func = FunctionBuilder::new("default-ep").build();
        assert_eq!(func.handler.entry_point, "_start");
    }
}
