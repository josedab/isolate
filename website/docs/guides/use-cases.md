---
sidebar_position: 7
---

# Use Cases

This guide covers common use cases for Isolate with detailed implementation examples.

## Plugin Systems

Run third-party plugins without risking your host application. Each plugin runs in complete isolation with explicit capabilities.

### Scenario

You're building an extensible application where users can install plugins from a marketplace. Plugins should be able to:
- Read configuration from a specific directory
- Write output to stdout
- Access a limited set of host functions

But plugins must NOT be able to:
- Access the filesystem outside their designated area
- Make network requests
- Access environment variables with secrets

### Implementation

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability, WasmEngine};
use std::sync::Arc;
use std::path::PathBuf;

pub struct PluginHost {
    engine: Arc<WasmEngine>,
    plugin_dir: PathBuf,
}

impl PluginHost {
    pub fn new(plugin_dir: PathBuf) -> Self {
        Self {
            engine: Arc::new(WasmEngine::new().expect("engine")),
            plugin_dir,
        }
    }

    pub async fn run_plugin(
        &self,
        plugin_name: &str,
        input: &[u8],
    ) -> isolate_core::Result<String> {
        // Load the plugin WASM
        let plugin_path = self.plugin_dir.join(format!("{}.wasm", plugin_name));
        let wasm_bytes = std::fs::read(&plugin_path)?;

        // Create isolated config for this plugin
        let config_dir = self.plugin_dir.join(plugin_name).join("config");

        let config = SandboxConfig::builder()
            .module(&wasm_bytes)?
            // Resource limits prevent DoS
            .memory_limit(64 * 1024 * 1024)  // 64MB
            .fuel(10_000_000)                 // ~10M instructions
            .cpu_time_limit(std::time::Duration::from_secs(5))
            // Minimal capabilities
            .capability(Capability::stdout())
            .capability(Capability::filesystem_read(&config_dir))
            .build()?;

        // Run in shared engine for better performance
        let mut sandbox = Sandbox::create_with_engine(config, self.engine.clone()).await?;
        let output = sandbox.run(input).await?;

        if output.exit_code != 0 {
            return Err(isolate_core::Error::Execution(
                format!("Plugin {} failed with exit code {}", plugin_name, output.exit_code)
            ));
        }

        Ok(output.stdout_str())
    }
}

// Usage
#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    let host = PluginHost::new(PathBuf::from("/var/lib/myapp/plugins"));

    // Run a plugin with input
    let result = host.run_plugin("analytics", b"user_id=123").await?;
    println!("Plugin output: {}", result);

    Ok(())
}
```

### Security Considerations

- **Validate plugin sources**: Only load plugins from trusted sources or verify signatures
- **Use unique sandbox IDs**: For audit logging and tracking
- **Set appropriate limits**: Base limits on expected plugin behavior
- **Monitor resource usage**: Track which plugins consume the most resources

## Serverless Functions

Execute user-provided code in multi-tenant environments with guaranteed isolation between tenants.

### Scenario

You're building a serverless platform where customers deploy WASM functions. Each function:
- Is triggered by HTTP requests
- Can make outbound HTTP calls to allowlisted domains
- Has access to a per-tenant data store
- Must complete within a timeout

### Implementation

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability, WasmEngine, Output};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;

pub struct FunctionRuntime {
    engine: Arc<WasmEngine>,
}

#[derive(Clone)]
pub struct FunctionConfig {
    pub wasm_bytes: Vec<u8>,
    pub tenant_id: String,
    pub allowed_hosts: Vec<String>,
    pub env_vars: HashMap<String, String>,
    pub timeout: Duration,
    pub memory_mb: usize,
}

impl FunctionRuntime {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(WasmEngine::new().expect("engine")),
        }
    }

    pub async fn invoke(
        &self,
        func_config: &FunctionConfig,
        request_body: &[u8],
    ) -> isolate_core::Result<Output> {
        let data_path = format!("/data/tenants/{}", func_config.tenant_id);

        let mut builder = SandboxConfig::builder()
            .module(&func_config.wasm_bytes)?
            // Resource limits
            .memory_limit(func_config.memory_mb * 1024 * 1024)
            .wall_time_limit(func_config.timeout)
            .cpu_time_limit(func_config.timeout)
            .fuel(100_000_000)  // 100M instructions
            .io_read_limit(10 * 1024 * 1024)   // 10MB read
            .io_write_limit(10 * 1024 * 1024)  // 10MB write
            // Capabilities
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .capability(Capability::filesystem_read(&data_path))
            .capability(Capability::filesystem_write(&data_path))
            .capability(Capability::system_clock())
            .capability(Capability::secure_random());

        // Add HTTP capability for allowed hosts
        if !func_config.allowed_hosts.is_empty() {
            builder = builder.capability(
                Capability::http_client(func_config.allowed_hosts.clone())
            );
        }

        // Add environment variables
        for (key, value) in &func_config.env_vars {
            builder = builder
                .capability(Capability::env_var(key))
                .env(key, value);
        }

        let config = builder.build()?;

        let mut sandbox = Sandbox::create_with_engine(config, self.engine.clone()).await?;
        sandbox.run(request_body).await
    }
}

// HTTP handler example
async fn handle_function_request(
    runtime: &FunctionRuntime,
    tenant_id: &str,
    function_name: &str,
    request_body: Vec<u8>,
) -> Result<Vec<u8>, String> {
    // Load function config from database (simplified)
    let func_config = load_function_config(tenant_id, function_name).await?;

    match runtime.invoke(&func_config, &request_body).await {
        Ok(output) => {
            if output.success() {
                Ok(output.stdout)
            } else {
                Err(format!("Function failed: {}", output.stderr_str()))
            }
        }
        Err(e) => Err(format!("Execution error: {}", e)),
    }
}
```

### Multi-Tenant Isolation

```rust
// Each tenant gets completely isolated sandboxes
// - Separate filesystem paths
// - Separate environment variables
// - No shared memory between tenants
// - Individual resource quotas

pub struct TenantQuota {
    pub max_concurrent_executions: usize,
    pub max_memory_mb: usize,
    pub max_cpu_time_per_request: Duration,
    pub max_requests_per_minute: u32,
}

pub async fn enforce_tenant_quota(
    tenant_id: &str,
    quota: &TenantQuota,
) -> Result<(), QuotaError> {
    // Check concurrent executions
    // Check rate limits
    // Reserve resources
    // ...
}
```

## Code Sandboxing

Safely run untrusted code snippets for testing, education, or CI/CD pipelines.

### Scenario: Online Code Playground

You're building an online code playground where users can write and execute code. The code must be:
- Completely isolated from other users
- Unable to access the network
- Limited in execution time and memory
- Able to produce stdout/stderr output

### Implementation

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability, Output};
use std::time::Duration;

pub struct CodePlayground {
    supported_languages: Vec<LanguageRuntime>,
}

pub struct LanguageRuntime {
    pub name: String,
    pub compiler_wasm: Vec<u8>,  // WASM compiler/interpreter
    pub file_extension: String,
}

pub struct ExecutionRequest {
    pub language: String,
    pub source_code: String,
    pub stdin: String,
}

pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub execution_time_ms: u64,
    pub memory_used_bytes: usize,
}

impl CodePlayground {
    pub async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult, String> {
        let runtime = self.supported_languages
            .iter()
            .find(|r| r.name == request.language)
            .ok_or_else(|| format!("Unsupported language: {}", request.language))?;

        // Strict limits for untrusted code
        let config = SandboxConfig::builder()
            .module(&runtime.compiler_wasm)
            .map_err(|e| e.to_string())?
            // Very strict resource limits
            .memory_limit(128 * 1024 * 1024)  // 128MB max
            .stack_size(1024 * 1024)           // 1MB stack
            .fuel(50_000_000)                  // 50M instructions
            .cpu_time_limit(Duration::from_secs(10))
            .wall_time_limit(Duration::from_secs(15))
            .io_write_limit(1024 * 1024)       // 1MB output max
            // Minimal capabilities - no filesystem, no network
            .capability(Capability::stdout())
            .capability(Capability::stderr())
            .capability(Capability::stdin())
            // Deterministic random for reproducibility
            .capability(Capability::seeded_random(42))
            .build()
            .map_err(|e| e.to_string())?;

        // Combine source code and stdin as input
        let input = format!("{}\n---STDIN---\n{}", request.source_code, request.stdin);

        let mut sandbox = Sandbox::create(config).await.map_err(|e| e.to_string())?;
        let start = std::time::Instant::now();
        let output: Output = sandbox.run(input.as_bytes()).await.map_err(|e| e.to_string())?;
        let elapsed = start.elapsed();

        Ok(ExecutionResult {
            stdout: output.stdout_str(),
            stderr: output.stderr_str(),
            exit_code: output.exit_code,
            execution_time_ms: elapsed.as_millis() as u64,
            memory_used_bytes: output.resource_usage.memory_peak,
        })
    }
}
```

### Scenario: CI/CD Test Runner

Run user-defined tests in isolation during CI/CD:

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

pub async fn run_test_suite(
    test_wasm: &[u8],
    test_data_dir: &str,
) -> Result<TestResults, String> {
    let config = SandboxConfig::builder()
        .module(test_wasm)
        .map_err(|e| e.to_string())?
        // Generous limits for test suites
        .memory_limit(512 * 1024 * 1024)
        .fuel(1_000_000_000)
        .wall_time_limit(std::time::Duration::from_secs(300))
        // Test-specific capabilities
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .capability(Capability::filesystem_read(test_data_dir))
        .capability(Capability::temp_dir())
        .capability(Capability::system_clock())  // For timing tests
        .capability(Capability::secure_random()) // For randomized tests
        .build()
        .map_err(|e| e.to_string())?;

    let mut sandbox = Sandbox::create(config).await.map_err(|e| e.to_string())?;
    let output = sandbox.run(&[]).await.map_err(|e| e.to_string())?;

    parse_test_output(&output.stdout_str())
}
```

## Edge Computing

Deploy lightweight, isolated workloads close to users with minimal cold start latency.

### Scenario

You're building an edge computing platform where WASM modules run at edge locations worldwide. Requirements:
- Sub-10ms cold start for user-facing latency
- Module caching for warm starts
- Geographic routing
- Per-request isolation

### Implementation

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability, WasmEngine};
use std::sync::Arc;
use dashmap::DashMap;

pub struct EdgeWorkerRuntime {
    engine: Arc<WasmEngine>,
    // Cache compiled modules by hash
    module_cache: DashMap<String, Vec<u8>>,
}

impl EdgeWorkerRuntime {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(WasmEngine::new().expect("engine")),
            module_cache: DashMap::new(),
        }
    }

    pub async fn handle_request(
        &self,
        worker_id: &str,
        request: EdgeRequest,
    ) -> Result<EdgeResponse, String> {
        // Get or fetch module
        let wasm_bytes = self.get_module(worker_id).await?;

        // Fast sandbox creation with shared engine
        let config = SandboxConfig::builder()
            .module(&wasm_bytes)
            .map_err(|e| e.to_string())?
            // Edge-optimized limits
            .memory_limit(32 * 1024 * 1024)  // 32MB - keep it small
            .fuel(10_000_000)                 // Fast timeout
            .wall_time_limit(std::time::Duration::from_millis(50))
            // Edge worker capabilities
            .capability(Capability::stdout())
            .capability(Capability::system_clock())
            .capability(Capability::secure_random())
            // Pass request info via env
            .capability(Capability::env_var("REQUEST_METHOD"))
            .capability(Capability::env_var("REQUEST_PATH"))
            .env("REQUEST_METHOD", &request.method)
            .env("REQUEST_PATH", &request.path)
            .build()
            .map_err(|e| e.to_string())?;

        let mut sandbox = Sandbox::create_with_engine(
            config,
            self.engine.clone()
        ).await.map_err(|e| e.to_string())?;

        let output = sandbox.run(&request.body).await.map_err(|e| e.to_string())?;

        Ok(EdgeResponse {
            status: if output.success() { 200 } else { 500 },
            body: output.stdout,
            headers: parse_headers(&output.stderr_str()),
        })
    }

    async fn get_module(&self, worker_id: &str) -> Result<Vec<u8>, String> {
        // Check cache first
        if let Some(bytes) = self.module_cache.get(worker_id) {
            return Ok(bytes.clone());
        }

        // Fetch from origin
        let bytes = fetch_module_from_origin(worker_id).await?;
        self.module_cache.insert(worker_id.to_string(), bytes.clone());
        Ok(bytes)
    }
}

pub struct EdgeRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct EdgeResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}
```

### Performance Optimization

```rust
// Pre-warm sandboxes for frequently accessed workers
pub struct PrewarmPool {
    pools: DashMap<String, Vec<Sandbox>>,
    engine: Arc<WasmEngine>,
}

impl PrewarmPool {
    pub async fn get_or_create(
        &self,
        worker_id: &str,
        config: SandboxConfig,
    ) -> Result<Sandbox, String> {
        // Try to get a pre-warmed sandbox
        if let Some(mut pool) = self.pools.get_mut(worker_id) {
            if let Some(sandbox) = pool.pop() {
                return Ok(sandbox);
            }
        }

        // Create new sandbox
        Sandbox::create_with_engine(config, self.engine.clone())
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn prewarm(&self, worker_id: &str, config: SandboxConfig, count: usize) {
        for _ in 0..count {
            if let Ok(sandbox) = Sandbox::create_with_engine(
                config.clone(),
                self.engine.clone()
            ).await {
                self.pools
                    .entry(worker_id.to_string())
                    .or_insert_with(Vec::new)
                    .push(sandbox);
            }
        }
    }
}
```

## Data Processing Pipelines

Process sensitive data with isolation guarantees.

### Scenario

You're processing user-uploaded data (documents, images, etc.) and want to ensure:
- Processing code can't exfiltrate data
- Memory is cleared after processing
- Processing time is bounded

```rust
use isolate_core::{Sandbox, SandboxConfig, capability::Capability};

pub async fn process_document(
    processor_wasm: &[u8],
    document: &[u8],
) -> Result<Vec<u8>, String> {
    // No network, no filesystem - data can only flow through stdin/stdout
    let config = SandboxConfig::builder()
        .module(processor_wasm)
        .map_err(|e| e.to_string())?
        .memory_limit(256 * 1024 * 1024)
        .wall_time_limit(std::time::Duration::from_secs(30))
        // Only stdin/stdout - no data exfiltration possible
        .capability(Capability::stdin())
        .capability(Capability::stdout())
        .build()
        .map_err(|e| e.to_string())?;

    let mut sandbox = Sandbox::create(config).await.map_err(|e| e.to_string())?;
    let output = sandbox.run(document).await.map_err(|e| e.to_string())?;

    // Sandbox memory is automatically freed when dropped

    if output.success() {
        Ok(output.stdout)
    } else {
        Err(format!("Processing failed: {}", output.stderr_str()))
    }
}
```

## See Also

- [Capabilities](./capabilities) - Detailed capability reference
- [Resource Limits](./resource-limits) - Resource control options
- [Security Model](./security-model) - Defense-in-depth architecture
- [API Reference](../reference/api) - Complete API documentation
