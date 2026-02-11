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
use isolate_core::dashboard::DashboardState;

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
    /// Dashboard state (shared with HTTP endpoints).
    dashboard: Arc<DashboardState>,
}

impl IsolateServiceImpl {
    /// Create a new service.
    pub fn new(max_sandboxes: usize) -> Self {
        Self {
            engine: Arc::new(WasmEngine::new().expect("Failed to create WASM engine")),
            sandboxes: DashMap::new(),
            semaphore: Arc::new(Semaphore::new(max_sandboxes)),
            max_sandboxes,
            dashboard: Arc::new(DashboardState::new(1000)),
        }
    }

    /// Get a shared reference to the dashboard state.
    pub fn dashboard(&self) -> Arc<DashboardState> {
        self.dashboard.clone()
    }

    /// Parse capabilities from proto.
    fn parse_capabilities(caps: &[proto::Capability]) -> Result<Vec<Capability>, Box<Status>> {
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
                    return Err(Box::new(Status::invalid_argument(format!(
                        "Unknown capability type: {}",
                        cap.r#type
                    ))))
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
    /// Create a new sandbox from a WASM module.
    ///
    /// # Arguments
    /// * `request` - Contains the WASM module bytes and optional configuration
    ///   (memory/fuel/time limits, capabilities, env vars, args).
    ///
    /// # Errors
    /// * `InvalidArgument` - Invalid WASM module or configuration.
    /// * `ResourceExhausted` - Maximum sandbox limit reached.
    /// * `Internal` - Failed to create the sandbox instance.
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

            let capabilities = Self::parse_capabilities(&config.capabilities).map_err(|e| *e)?;
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

        let sandbox_id = sandbox.id();
        let sandbox_id_str = sandbox_id.to_string();
        let module_hash = sandbox.module_hash().to_string();
        let creation_time = start.elapsed();

        // Record span attributes
        Span::current().record("sandbox.id", &sandbox_id_str);
        Span::current().record("sandbox.module_hash", &module_hash);

        // Store sandbox
        self.sandboxes.insert(sandbox_id_str.clone(), Arc::new(tokio::sync::Mutex::new(sandbox)));

        // Track in dashboard
        self.dashboard.register_sandbox(sandbox_id, module_hash.clone());

        tracing::info!(
            sandbox_id = %sandbox_id_str,
            module_hash = %module_hash,
            creation_time_ms = creation_time.as_secs_f64() * 1000.0,
            "Sandbox created via gRPC"
        );

        Ok(Response::new(CreateSandboxResponse {
            sandbox_id: sandbox_id_str,
            module_hash,
            creation_time_ms: creation_time.as_secs_f64() * 1000.0,
        }))
    }

    /// Execute a sandbox and return its output.
    ///
    /// # Arguments
    /// * `request` - Contains the sandbox ID and optional input bytes.
    ///
    /// # Errors
    /// * `NotFound` - Sandbox does not exist.
    /// * `Internal` - Execution failed (e.g., timeout, resource exhaustion, trap).
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

        // Track in dashboard
        let sandbox_id_parsed = guard.id();
        self.dashboard.record_run(
            &sandbox_id_parsed,
            output.duration,
            output.resource_usage.clone(),
            output.exit_code == 0,
        );
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

    /// Retrieve information about a sandbox.
    ///
    /// # Arguments
    /// * `request` - Contains the sandbox ID.
    ///
    /// # Errors
    /// * `NotFound` - Sandbox does not exist.
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

    /// Terminate a sandbox and return its accumulated metrics.
    ///
    /// # Arguments
    /// * `request` - Contains the sandbox ID.
    ///
    /// # Errors
    /// * `NotFound` - Sandbox does not exist.
    /// * `Internal` - Failed to terminate the sandbox.
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
        let sandbox_uuid = guard.id();
        let metrics = guard
            .terminate()
            .await
            .map_err(|e| Status::internal(format!("Failed to terminate: {}", e)))?;

        tracing::info!(sandbox_id = %req.sandbox_id, "Sandbox terminated via gRPC");

        // Track in dashboard
        self.dashboard.update_state(&sandbox_uuid, isolate_core::sandbox::SandboxState::Terminated);
        self.dashboard.remove_sandbox(&sandbox_uuid);

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

    /// List active sandboxes with optional state filtering and pagination.
    ///
    /// # Arguments
    /// * `request` - Contains optional `state_filter`, `offset`, and `limit` fields.
    ///
    /// # Errors
    /// Returns an empty list if no sandboxes match the filter.
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

    /// Stream real-time stdout/stderr output from a running sandbox.
    ///
    /// # Arguments
    /// * `request` - Contains the sandbox ID and boolean flags `follow_stdout`
    ///   and `follow_stderr` to select which streams to follow.
    ///
    /// # Errors
    /// * `NotFound` - Sandbox does not exist.
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

        let sandbox = self
            .sandboxes
            .get(&req.sandbox_id)
            .ok_or_else(|| Status::not_found("Sandbox not found"))?
            .clone();

        let follow_stdout = req.follow_stdout;
        let follow_stderr = req.follow_stderr;

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // Acquire the sandbox lock and start a streaming execution.
        // The sandbox must be in the Ready state for run_streaming to succeed.
        let mut guard = sandbox.lock().await;

        let streaming_result = guard
            .run_streaming(&[], 64)
            .await
            .map_err(|e| Status::failed_precondition(format!("Cannot stream: {}", e)))?;

        let (mut output_rx, _join_handle) = streaming_result;
        drop(guard);

        tokio::spawn(async move {
            use isolate_core::engine::OutputSource;

            while let Some(chunk) = output_rx.recv().await {
                let stream_name = match chunk.source {
                    OutputSource::Stdout => {
                        if !follow_stdout {
                            continue;
                        }
                        "stdout"
                    }
                    OutputSource::Stderr => {
                        if !follow_stderr {
                            continue;
                        }
                        "stderr"
                    }
                };

                let proto_chunk = OutputChunk {
                    stream: stream_name.to_string(),
                    data: chunk.data,
                    timestamp: chrono::Utc::now().timestamp(),
                };

                if tx.send(Ok(proto_chunk)).await.is_err() {
                    break; // Client disconnected
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    /// Retrieve server metrics in Prometheus or JSON format.
    ///
    /// # Arguments
    /// * `request` - Contains `format` field: `"json"` for JSON stats,
    ///   anything else for Prometheus text format.
    ///
    /// # Errors
    /// This method does not return errors under normal operation.
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

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/minimal.wasm");
    const HELLO_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/hello.wasm");
    const EXIT_42_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/exit_42.wasm");

    // -- parse_capabilities tests --

    #[test]
    fn test_parse_capabilities_stdout() {
        let caps = vec![proto::Capability {
            r#type: "stdout".into(),
            value: String::new(),
        }];
        let result = IsolateServiceImpl::parse_capabilities(&caps).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_parse_capabilities_multiple() {
        let caps = vec![
            proto::Capability { r#type: "stdout".into(), value: String::new() },
            proto::Capability { r#type: "stderr".into(), value: String::new() },
            proto::Capability { r#type: "stdin".into(), value: String::new() },
            proto::Capability { r#type: "fs:read".into(), value: "/tmp".into() },
            proto::Capability { r#type: "fs:write".into(), value: "/out".into() },
            proto::Capability { r#type: "fs:temp".into(), value: String::new() },
            proto::Capability { r#type: "dns".into(), value: String::new() },
            proto::Capability { r#type: "time:system".into(), value: String::new() },
            proto::Capability { r#type: "time:monotonic".into(), value: String::new() },
            proto::Capability { r#type: "random".into(), value: String::new() },
            proto::Capability { r#type: "env".into(), value: "HOME".into() },
        ];
        let result = IsolateServiceImpl::parse_capabilities(&caps).unwrap();
        assert_eq!(result.len(), 11);
    }

    #[test]
    fn test_parse_capabilities_unknown_type() {
        let caps = vec![proto::Capability {
            r#type: "nonexistent".into(),
            value: String::new(),
        }];
        let result = IsolateServiceImpl::parse_capabilities(&caps);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_capabilities_empty() {
        let result = IsolateServiceImpl::parse_capabilities(&[]).unwrap();
        assert!(result.is_empty());
    }

    // -- constructor tests --

    #[test]
    fn test_new_service() {
        let service = IsolateServiceImpl::new(10);
        assert_eq!(service.max_sandboxes, 10);
        assert_eq!(service.sandboxes.len(), 0);
    }

    #[test]
    fn test_dashboard_returns_shared_reference() {
        let service = IsolateServiceImpl::new(5);
        let d1 = service.dashboard();
        let d2 = service.dashboard();
        assert!(Arc::ptr_eq(&d1, &d2));
    }

    // -- gRPC method tests --

    #[tokio::test]
    async fn test_create_sandbox_minimal() {
        let service = IsolateServiceImpl::new(10);
        let req = Request::new(CreateSandboxRequest {
            module: MINIMAL_WASM.to_vec(),
            config: None,
        });
        let resp = service.create_sandbox(req).await.unwrap();
        let inner = resp.into_inner();
        assert!(!inner.sandbox_id.is_empty());
        assert!(!inner.module_hash.is_empty());
        assert!(inner.creation_time_ms >= 0.0);
    }

    #[tokio::test]
    async fn test_create_sandbox_with_config() {
        let service = IsolateServiceImpl::new(10);
        let req = Request::new(CreateSandboxRequest {
            module: MINIMAL_WASM.to_vec(),
            config: Some(proto::SandboxConfig {
                memory_limit: 1024 * 1024,
                fuel_limit: 100_000,
                wall_time_limit_secs: 5,
                cpu_time_limit_secs: 0,
                capabilities: vec![proto::Capability {
                    r#type: "stdout".into(),
                    value: String::new(),
                }],
                env: std::collections::HashMap::new(),
                args: vec![],
            }),
        });
        let resp = service.create_sandbox(req).await.unwrap();
        assert!(!resp.into_inner().sandbox_id.is_empty());
    }

    #[tokio::test]
    async fn test_create_sandbox_invalid_module() {
        let service = IsolateServiceImpl::new(10);
        let req = Request::new(CreateSandboxRequest {
            module: vec![0, 1, 2, 3],
            config: None,
        });
        let result = service.create_sandbox(req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_run_sandbox() {
        let service = IsolateServiceImpl::new(10);

        let create_resp = service
            .create_sandbox(Request::new(CreateSandboxRequest {
                module: HELLO_WASM.to_vec(),
                config: Some(proto::SandboxConfig {
                    memory_limit: 0,
                    fuel_limit: 1_000_000,
                    wall_time_limit_secs: 5,
                    cpu_time_limit_secs: 0,
                    capabilities: vec![proto::Capability {
                        r#type: "stdout".into(),
                        value: String::new(),
                    }],
                    env: std::collections::HashMap::new(),
                    args: vec![],
                }),
            }))
            .await
            .unwrap();
        let sandbox_id = create_resp.into_inner().sandbox_id;

        let run_resp = service
            .run_sandbox(Request::new(RunSandboxRequest {
                sandbox_id: sandbox_id.clone(),
                input: vec![],
                entry_point: String::new(),
            }))
            .await
            .unwrap();
        let output = run_resp.into_inner();
        assert_eq!(output.exit_code, 0);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello from WASM!\n");
    }

    #[tokio::test]
    async fn test_run_sandbox_not_found() {
        let service = IsolateServiceImpl::new(10);
        let result = service
            .run_sandbox(Request::new(RunSandboxRequest {
                sandbox_id: "nonexistent".into(),
                input: vec![],
                entry_point: String::new(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_get_sandbox() {
        let service = IsolateServiceImpl::new(10);

        let create_resp = service
            .create_sandbox(Request::new(CreateSandboxRequest {
                module: MINIMAL_WASM.to_vec(),
                config: None,
            }))
            .await
            .unwrap();
        let sandbox_id = create_resp.into_inner().sandbox_id;

        let get_resp = service
            .get_sandbox(Request::new(GetSandboxRequest {
                sandbox_id: sandbox_id.clone(),
            }))
            .await
            .unwrap();
        let info = get_resp.into_inner().sandbox.unwrap();
        assert_eq!(info.id, sandbox_id);
        assert!(!info.module_hash.is_empty());
    }

    #[tokio::test]
    async fn test_get_sandbox_not_found() {
        let service = IsolateServiceImpl::new(10);
        let result = service
            .get_sandbox(Request::new(GetSandboxRequest {
                sandbox_id: "nonexistent".into(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_terminate_sandbox() {
        let service = IsolateServiceImpl::new(10);

        let create_resp = service
            .create_sandbox(Request::new(CreateSandboxRequest {
                module: MINIMAL_WASM.to_vec(),
                config: None,
            }))
            .await
            .unwrap();
        let sandbox_id = create_resp.into_inner().sandbox_id;

        let term_resp = service
            .terminate_sandbox(Request::new(TerminateSandboxRequest {
                sandbox_id: sandbox_id.clone(),
            }))
            .await
            .unwrap();
        assert!(term_resp.into_inner().terminated);

        // Verify sandbox is removed
        let get_result = service
            .get_sandbox(Request::new(GetSandboxRequest { sandbox_id }))
            .await;
        assert!(get_result.is_err());
    }

    #[tokio::test]
    async fn test_terminate_sandbox_not_found() {
        let service = IsolateServiceImpl::new(10);
        let result = service
            .terminate_sandbox(Request::new(TerminateSandboxRequest {
                sandbox_id: "nonexistent".into(),
            }))
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_list_sandboxes_empty() {
        let service = IsolateServiceImpl::new(10);
        let resp = service
            .list_sandboxes(Request::new(ListSandboxesRequest {
                state_filter: String::new(),
                offset: 0,
                limit: 10,
            }))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert_eq!(inner.total, 0);
        assert!(inner.sandboxes.is_empty());
    }

    #[tokio::test]
    async fn test_list_sandboxes_with_entries() {
        let service = IsolateServiceImpl::new(10);

        for _ in 0..2 {
            service
                .create_sandbox(Request::new(CreateSandboxRequest {
                    module: MINIMAL_WASM.to_vec(),
                    config: None,
                }))
                .await
                .unwrap();
        }

        let resp = service
            .list_sandboxes(Request::new(ListSandboxesRequest {
                state_filter: String::new(),
                offset: 0,
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().total, 2);
    }

    #[tokio::test]
    async fn test_list_sandboxes_pagination() {
        let service = IsolateServiceImpl::new(10);

        for _ in 0..3 {
            service
                .create_sandbox(Request::new(CreateSandboxRequest {
                    module: MINIMAL_WASM.to_vec(),
                    config: None,
                }))
                .await
                .unwrap();
        }

        let resp = service
            .list_sandboxes(Request::new(ListSandboxesRequest {
                state_filter: String::new(),
                offset: 0,
                limit: 2,
            }))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert_eq!(inner.total, 3);
        assert_eq!(inner.sandboxes.len(), 2);
    }

    #[tokio::test]
    async fn test_get_metrics_json() {
        let service = IsolateServiceImpl::new(10);
        let resp = service
            .get_metrics(Request::new(GetMetricsRequest {
                format: "json".into(),
            }))
            .await
            .unwrap();
        let data = resp.into_inner().data;
        let parsed: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(parsed["sandboxes_active"], 0);
        assert_eq!(parsed["max_sandboxes"], 10);
    }

    #[tokio::test]
    async fn test_get_metrics_prometheus() {
        let service = IsolateServiceImpl::new(10);
        let resp = service
            .get_metrics(Request::new(GetMetricsRequest {
                format: "prometheus".into(),
            }))
            .await
            .unwrap();
        let _ = resp.into_inner().data;
    }

    #[tokio::test]
    async fn test_run_exit_code_42() {
        let service = IsolateServiceImpl::new(10);

        let create_resp = service
            .create_sandbox(Request::new(CreateSandboxRequest {
                module: EXIT_42_WASM.to_vec(),
                config: Some(proto::SandboxConfig {
                    memory_limit: 0,
                    fuel_limit: 1_000_000,
                    wall_time_limit_secs: 5,
                    cpu_time_limit_secs: 0,
                    capabilities: vec![],
                    env: std::collections::HashMap::new(),
                    args: vec![],
                }),
            }))
            .await
            .unwrap();
        let sandbox_id = create_resp.into_inner().sandbox_id;

        let run_resp = service
            .run_sandbox(Request::new(RunSandboxRequest {
                sandbox_id,
                input: vec![],
                entry_point: String::new(),
            }))
            .await
            .unwrap();
        assert_eq!(run_resp.into_inner().exit_code, 42);
    }

    #[tokio::test]
    async fn test_create_sandbox_with_invalid_capability() {
        let service = IsolateServiceImpl::new(10);
        let req = Request::new(CreateSandboxRequest {
            module: MINIMAL_WASM.to_vec(),
            config: Some(proto::SandboxConfig {
                memory_limit: 0,
                fuel_limit: 0,
                wall_time_limit_secs: 0,
                cpu_time_limit_secs: 0,
                capabilities: vec![proto::Capability {
                    r#type: "invalid_type".into(),
                    value: String::new(),
                }],
                env: std::collections::HashMap::new(),
                args: vec![],
            }),
        });
        let result = service.create_sandbox(req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_full_lifecycle() {
        let service = IsolateServiceImpl::new(10);

        // Create
        let create_resp = service
            .create_sandbox(Request::new(CreateSandboxRequest {
                module: MINIMAL_WASM.to_vec(),
                config: Some(proto::SandboxConfig {
                    memory_limit: 0,
                    fuel_limit: 1_000_000,
                    wall_time_limit_secs: 5,
                    cpu_time_limit_secs: 0,
                    capabilities: vec![],
                    env: std::collections::HashMap::new(),
                    args: vec![],
                }),
            }))
            .await
            .unwrap();
        let sandbox_id = create_resp.into_inner().sandbox_id;

        // Get
        let info = service
            .get_sandbox(Request::new(GetSandboxRequest {
                sandbox_id: sandbox_id.clone(),
            }))
            .await
            .unwrap()
            .into_inner()
            .sandbox
            .unwrap();
        assert_eq!(info.id, sandbox_id);

        // Run
        let run_resp = service
            .run_sandbox(Request::new(RunSandboxRequest {
                sandbox_id: sandbox_id.clone(),
                input: vec![],
                entry_point: String::new(),
            }))
            .await
            .unwrap();
        assert_eq!(run_resp.into_inner().exit_code, 0);

        // List
        let list_resp = service
            .list_sandboxes(Request::new(ListSandboxesRequest {
                state_filter: String::new(),
                offset: 0,
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(list_resp.into_inner().total, 1);

        // Terminate
        let term_resp = service
            .terminate_sandbox(Request::new(TerminateSandboxRequest {
                sandbox_id: sandbox_id.clone(),
            }))
            .await
            .unwrap();
        assert!(term_resp.into_inner().terminated);

        // Verify removed
        let list_resp = service
            .list_sandboxes(Request::new(ListSandboxesRequest {
                state_filter: String::new(),
                offset: 0,
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(list_resp.into_inner().total, 0);
    }
}
