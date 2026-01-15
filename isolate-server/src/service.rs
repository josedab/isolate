//! gRPC service implementation.

use crate::proto::{
    self, isolate_service_server::IsolateService, CreateSandboxRequest, CreateSandboxResponse,
    GetMetricsRequest, GetMetricsResponse, GetSandboxRequest, GetSandboxResponse,
    ListSandboxesRequest, ListSandboxesResponse, OutputChunk, RunSandboxRequest,
    RunSandboxResponse, SandboxInfo, SandboxMetrics as ProtoSandboxMetrics, StreamOutputRequest,
    TerminateSandboxRequest, TerminateSandboxResponse,
};

use isolate_core::{
    capability::Capability, engine::WasmEngine, metrics::global_registry, Sandbox, SandboxConfig,
};

use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};
use tracing::{instrument, Span};

/// The Isolate gRPC service implementation.
pub struct IsolateServiceImpl {
    /// Shared WASM engine.
    engine: Arc<WasmEngine>,
    /// Active sandboxes.
    sandboxes: DashMap<String, Arc<tokio::sync::Mutex<Sandbox>>>,
    /// Semaphore to limit concurrent sandboxes.
    semaphore: Arc<Semaphore>,
    /// Maximum sandboxes.
    max_sandboxes: usize,
}

impl IsolateServiceImpl {
    /// Create a new service.
    pub fn new(max_sandboxes: usize) -> Self {
        Self {
            engine: Arc::new(WasmEngine::new().expect("Failed to create WASM engine")),
            sandboxes: DashMap::new(),
            semaphore: Arc::new(Semaphore::new(max_sandboxes)),
            max_sandboxes,
        }
    }

    /// Parse capabilities from proto.
    fn parse_capabilities(caps: &[proto::Capability]) -> Result<Vec<Capability>, Status> {
        let mut result = Vec::new();
        for cap in caps {
            let capability = match cap.r#type.as_str() {
                "stdout" => Capability::stdout(),
                "stderr" => Capability::stderr(),
                "stdin" => Capability::stdin(),
                "fs:read" => Capability::filesystem_read(&cap.value),
                "fs:write" => Capability::filesystem_write(&cap.value),
                "fs:temp" => Capability::temp_dir(),
                "http" => Capability::http_client(vec![cap.value.clone()]),
                "dns" => Capability::dns_resolve(),
                "time:system" => Capability::system_clock(),
                "time:monotonic" => Capability::monotonic_clock(),
                "random" => Capability::secure_random(),
                "env" => Capability::env_var(&cap.value),
                _ => {
                    return Err(Status::invalid_argument(format!(
                        "Unknown capability type: {}",
                        cap.r#type
                    )))
                }
            };
            result.push(capability);
        }
        Ok(result)
    }

    /// Convert sandbox to proto info.
    fn sandbox_to_info(sandbox: &Sandbox) -> SandboxInfo {
        let metrics = sandbox.metrics();
        SandboxInfo {
            id: sandbox.id().to_string(),
            state: sandbox.state().to_string(),
            module_hash: sandbox.module_hash().to_string(),
            created_at: chrono::Utc::now().timestamp(),
            age_secs: sandbox.age().as_secs_f64(),
            metrics: Some(ProtoSandboxMetrics {
                run_count: metrics.run_count,
                success_count: metrics.success_count,
                failure_count: metrics.failure_count,
                total_run_duration_ms: metrics.total_run_duration.as_secs_f64() * 1000.0,
                last_run_duration_ms: metrics
                    .last_run_duration
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0),
            }),
        }
    }
}

#[tonic::async_trait]
impl IsolateService for IsolateServiceImpl {
    #[instrument(
        name = "grpc.create_sandbox",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "CreateSandbox",
        )
    )]
    async fn create_sandbox(
        &self,
        request: Request<CreateSandboxRequest>,
    ) -> Result<Response<CreateSandboxResponse>, Status> {
        let req = request.into_inner();

        // Acquire semaphore permit
        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Status::resource_exhausted("Maximum sandbox limit reached"))?;

        let start = std::time::Instant::now();

        // Build configuration
        let mut builder = SandboxConfig::builder()
            .module(&req.module)
            .map_err(|e| Status::invalid_argument(format!("Invalid WASM module: {}", e)))?;

        if let Some(config) = req.config {
            if config.memory_limit > 0 {
                builder = builder.memory_limit(config.memory_limit as usize);
            }
            if config.fuel_limit > 0 {
                builder = builder.fuel(config.fuel_limit);
            }
            if config.wall_time_limit_secs > 0 {
                builder = builder
                    .wall_time_limit(Duration::from_secs(config.wall_time_limit_secs as u64));
            }
            if config.cpu_time_limit_secs > 0 {
                builder =
                    builder.cpu_time_limit(Duration::from_secs(config.cpu_time_limit_secs as u64));
            }

            let capabilities = Self::parse_capabilities(&config.capabilities)?;
            builder = builder.capabilities(capabilities);

            for (key, value) in config.env {
                builder = builder.env(key, value);
            }

            if !config.args.is_empty() {
                builder = builder.args(config.args);
            }
        }

        let sandbox_config = builder
            .build()
            .map_err(|e| Status::invalid_argument(format!("Invalid configuration: {}", e)))?;

        // Create sandbox
        let sandbox = Sandbox::create_with_engine(sandbox_config, self.engine.clone())
            .await
            .map_err(|e| Status::internal(format!("Failed to create sandbox: {}", e)))?;

        let sandbox_id = sandbox.id().to_string();
        let module_hash = sandbox.module_hash().to_string();
        let creation_time = start.elapsed();

        // Record span attributes
        Span::current().record("sandbox.id", &sandbox_id);
        Span::current().record("sandbox.module_hash", &module_hash);

        // Store sandbox
        self.sandboxes.insert(sandbox_id.clone(), Arc::new(tokio::sync::Mutex::new(sandbox)));

        tracing::info!(
            sandbox_id = %sandbox_id,
            module_hash = %module_hash,
            creation_time_ms = creation_time.as_secs_f64() * 1000.0,
            "Sandbox created via gRPC"
        );

        Ok(Response::new(CreateSandboxResponse {
            sandbox_id,
            module_hash,
            creation_time_ms: creation_time.as_secs_f64() * 1000.0,
        }))
    }

    #[instrument(
        name = "grpc.run_sandbox",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "RunSandbox",
            sandbox.id = %request.get_ref().sandbox_id,
        )
    )]
    async fn run_sandbox(
        &self,
        request: Request<RunSandboxRequest>,
    ) -> Result<Response<RunSandboxResponse>, Status> {
        let req = request.into_inner();

        let sandbox = self
            .sandboxes
            .get(&req.sandbox_id)
            .ok_or_else(|| Status::not_found("Sandbox not found"))?
            .clone();

        let mut guard = sandbox.lock().await;

        let output = guard
            .run(&req.input)
            .await
            .map_err(|e| Status::internal(format!("Execution failed: {}", e)))?;

        // Record execution results in span
        Span::current().record("sandbox.exit_code", output.exit_code);
        tracing::info!(
            sandbox_id = %req.sandbox_id,
            exit_code = output.exit_code,
            duration_ms = output.duration.as_secs_f64() * 1000.0,
            "Sandbox execution completed"
        );

        let usage = &output.resource_usage;
        Ok(Response::new(RunSandboxResponse {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
            duration_ms: output.duration.as_secs_f64() * 1000.0,
            resource_usage: Some(proto::ResourceUsage {
                peak_memory: usage.peak_memory as u64,
                fuel_consumed: usage.fuel_consumed,
                cpu_time_ms: usage.cpu_time.as_secs_f64() * 1000.0,
                wall_time_ms: usage.wall_time.as_secs_f64() * 1000.0,
                bytes_read: usage.bytes_read,
                bytes_written: usage.bytes_written,
            }),
        }))
    }

    #[instrument(
        name = "grpc.get_sandbox",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "GetSandbox",
            sandbox.id = %request.get_ref().sandbox_id,
        )
    )]
    async fn get_sandbox(
        &self,
        request: Request<GetSandboxRequest>,
    ) -> Result<Response<GetSandboxResponse>, Status> {
        let req = request.into_inner();

        let sandbox = self
            .sandboxes
            .get(&req.sandbox_id)
            .ok_or_else(|| Status::not_found("Sandbox not found"))?;

        let guard = sandbox.lock().await;
        let info = Self::sandbox_to_info(&guard);

        Ok(Response::new(GetSandboxResponse { sandbox: Some(info) }))
    }

    #[instrument(
        name = "grpc.terminate_sandbox",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "TerminateSandbox",
            sandbox.id = %request.get_ref().sandbox_id,
        )
    )]
    async fn terminate_sandbox(
        &self,
        request: Request<TerminateSandboxRequest>,
    ) -> Result<Response<TerminateSandboxResponse>, Status> {
        let req = request.into_inner();

        let sandbox = self
            .sandboxes
            .remove(&req.sandbox_id)
            .ok_or_else(|| Status::not_found("Sandbox not found"))?;

        let mut guard = sandbox.1.lock().await;
        let metrics = guard
            .terminate()
            .await
            .map_err(|e| Status::internal(format!("Failed to terminate: {}", e)))?;

        tracing::info!(sandbox_id = %req.sandbox_id, "Sandbox terminated via gRPC");

        Ok(Response::new(TerminateSandboxResponse {
            terminated: true,
            metrics: Some(ProtoSandboxMetrics {
                run_count: metrics.run_count,
                success_count: metrics.success_count,
                failure_count: metrics.failure_count,
                total_run_duration_ms: metrics.total_run_duration.as_secs_f64() * 1000.0,
                last_run_duration_ms: metrics
                    .last_run_duration
                    .map(|d| d.as_secs_f64() * 1000.0)
                    .unwrap_or(0.0),
            }),
        }))
    }

    #[instrument(
        name = "grpc.list_sandboxes",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "ListSandboxes",
        )
    )]
    async fn list_sandboxes(
        &self,
        request: Request<ListSandboxesRequest>,
    ) -> Result<Response<ListSandboxesResponse>, Status> {
        let req = request.into_inner();

        let mut sandboxes = Vec::new();

        for entry in self.sandboxes.iter() {
            let guard = entry.value().lock().await;

            // Apply state filter if provided
            if !req.state_filter.is_empty() && guard.state().to_string() != req.state_filter {
                continue;
            }

            sandboxes.push(Self::sandbox_to_info(&guard));
        }

        let total = sandboxes.len() as i32;

        // Apply pagination
        let offset = req.offset.max(0) as usize;
        let limit = if req.limit > 0 { req.limit as usize } else { sandboxes.len() };

        let sandboxes: Vec<_> = sandboxes.into_iter().skip(offset).take(limit).collect();

        Ok(Response::new(ListSandboxesResponse { sandboxes, total }))
    }

    type StreamOutputStream = tokio_stream::wrappers::ReceiverStream<Result<OutputChunk, Status>>;

    #[instrument(
        name = "grpc.stream_output",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "StreamOutput",
            sandbox.id = %request.get_ref().sandbox_id,
        )
    )]
    async fn stream_output(
        &self,
        request: Request<StreamOutputRequest>,
    ) -> Result<Response<Self::StreamOutputStream>, Status> {
        let req = request.into_inner();

        // Verify sandbox exists
        let _sandbox = self
            .sandboxes
            .get(&req.sandbox_id)
            .ok_or_else(|| Status::not_found("Sandbox not found"))?;

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // For now, return an empty stream
        // Full implementation would require streaming from sandbox output
        tokio::spawn(async move {
            // Stream would be populated here during execution
            drop(tx);
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    #[instrument(
        name = "grpc.get_metrics",
        skip(self, request),
        fields(
            otel.kind = "server",
            rpc.system = "grpc",
            rpc.service = "isolate.v1.IsolateService",
            rpc.method = "GetMetrics",
        )
    )]
    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let req = request.into_inner();

        let data = match req.format.as_str() {
            "json" => {
                // Return basic stats as JSON
                let stats = serde_json::json!({
                    "sandboxes_active": self.sandboxes.len(),
                    "max_sandboxes": self.max_sandboxes,
                });
                serde_json::to_string(&stats).unwrap_or_default()
            }
            _ => {
                // Default to Prometheus format
                global_registry().gather_text()
            }
        };

        Ok(Response::new(GetMetricsResponse { data }))
    }
}
