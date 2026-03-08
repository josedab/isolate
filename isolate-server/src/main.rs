//! Isolate gRPC Server
//!
//! Provides remote sandbox management via gRPC.
//!
//! ## Health Checks
//!
//! The server provides two health check mechanisms:
//!
//! 1. **gRPC Health Checking Protocol** (port 50051):
//!    - Standard gRPC health checking at `grpc.health.v1.Health/Check`
//!    - Compatible with Kubernetes gRPC probes and grpc-health-probe
//!    - Usage: `grpc-health-probe -addr=localhost:50051`
//!
//! 2. **HTTP Health Endpoint** (configurable port, default 8080):
//!    - `GET /healthz` - Returns 200 OK if healthy
//!    - `GET /readyz` - Returns 200 OK when ready to serve traffic
//!    - Compatible with Kubernetes HTTP probes
//!
//! ## OpenTelemetry Tracing
//!
//! The server supports distributed tracing via OpenTelemetry. Enable by providing
//! an OTLP endpoint:
//!
//! ```bash
//! isolate-server --otlp-endpoint http://localhost:4317
//! ```
//!
//! ### Environment Variables
//!
//! - `OTEL_EXPORTER_OTLP_ENDPOINT` - Alternative to --otlp-endpoint flag
//! - `OTEL_SERVICE_NAME` - Override service name (default: isolate-server)
//!
//! ### Exported Spans
//!
//! - `grpc.create_sandbox` - Sandbox creation including WASM compilation
//! - `grpc.run_sandbox` - Sandbox execution
//! - `grpc.terminate_sandbox` - Sandbox cleanup
//! - `grpc.get_sandbox` - Sandbox status retrieval
//! - `grpc.list_sandboxes` - List operations
//!
//! ## Example Kubernetes Configuration
//!
//! ```yaml
//! livenessProbe:
//!   httpGet:
//!     path: /healthz
//!     port: 8080
//!   initialDelaySeconds: 5
//!   periodSeconds: 10
//!
//! readinessProbe:
//!   grpc:
//!     port: 50051
//!   initialDelaySeconds: 5
//!   periodSeconds: 10
//! ```

mod auth;
mod service;

use clap::Parser;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use opentelemetry::{trace::TracerProvider, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace::Sampler, Resource};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic_health::server::health_reporter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[allow(dead_code)]
mod proto {
    tonic::include_proto!("isolate.v1");
}

use isolate_core::dashboard::DashboardState;
use proto::isolate_service_server::IsolateServiceServer;
use service::IsolateServiceImpl;

/// Isolate gRPC Server
#[derive(Parser, Debug)]
#[command(name = "isolate-server")]
#[command(about = "gRPC server for the Isolate secure sandbox runtime")]
struct Args {
    /// gRPC address to bind to
    #[arg(short, long, default_value = "0.0.0.0:50051", env = "ISOLATE_ADDR")]
    addr: SocketAddr,

    /// HTTP health check address to bind to
    #[arg(long, default_value = "0.0.0.0:8080", env = "ISOLATE_HEALTH_ADDR")]
    health_addr: SocketAddr,

    /// Disable HTTP health endpoint
    #[arg(long, env = "ISOLATE_NO_HEALTH_HTTP")]
    no_health_http: bool,

    /// Log level
    #[arg(short, long, default_value = "info", env = "ISOLATE_LOG_LEVEL")]
    log_level: String,

    /// Enable JSON logging
    #[arg(long, env = "ISOLATE_JSON_LOGS")]
    json_logs: bool,

    /// Maximum number of concurrent sandboxes
    #[arg(long, default_value = "100", env = "ISOLATE_MAX_SANDBOXES")]
    max_sandboxes: usize,

    /// Enable warm pool
    #[arg(long, env = "ISOLATE_WARM_POOL")]
    warm_pool: bool,

    /// Warm pool size per module
    #[arg(long, default_value = "5", env = "ISOLATE_WARM_POOL_SIZE")]
    warm_pool_size: usize,

    // OpenTelemetry options
    /// OTLP endpoint for OpenTelemetry export (e.g., http://localhost:4317)
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,

    /// Service name for tracing (default: isolate-server)
    #[arg(long, env = "OTEL_SERVICE_NAME", default_value = "isolate-server")]
    service_name: String,

    /// Sampling ratio for traces (0.0 to 1.0, default: 1.0 for all traces)
    #[arg(long, default_value = "1.0", env = "OTEL_SAMPLING_RATIO")]
    sampling_ratio: f64,

    /// Disable OpenTelemetry tracing
    #[arg(long, env = "ISOLATE_NO_TRACING")]
    no_tracing: bool,

    // TLS options
    /// Path to TLS certificate file (PEM format)
    #[arg(long, env = "ISOLATE_TLS_CERT")]
    tls_cert: Option<String>,

    /// Path to TLS private key file (PEM format)
    #[arg(long, env = "ISOLATE_TLS_KEY")]
    tls_key: Option<String>,

    /// Path to TLS CA certificate for client verification (enables mTLS)
    #[arg(long, env = "ISOLATE_TLS_CA")]
    tls_ca: Option<String>,

    // Shutdown options
    /// Graceful shutdown timeout in seconds
    #[arg(long, default_value = "30", env = "ISOLATE_SHUTDOWN_TIMEOUT")]
    shutdown_timeout: u64,

    /// Maximum WASM module upload size in bytes (default: 50MB)
    #[arg(long, default_value = "52428800", env = "ISOLATE_MAX_MODULE_SIZE")]
    max_module_size: usize,

    /// API key for gRPC authentication (if unset, all requests allowed)
    #[arg(long, env = "ISOLATE_API_KEY")]
    api_key: Option<String>,

    /// Rate limit: maximum requests per second (default: 100)
    #[arg(long, default_value = "100", env = "ISOLATE_RATE_LIMIT")]
    rate_limit: u32,

    /// Rate limit: burst capacity (default: 200)
    #[arg(long, default_value = "200", env = "ISOLATE_RATE_BURST")]
    rate_burst: u32,
}

/// HTTP health check and dashboard API handler.
async fn health_handler(
    req: Request<hyper::body::Incoming>,
    service_healthy: Arc<std::sync::atomic::AtomicBool>,
    dashboard: Arc<DashboardState>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let response = match path {
        "/healthz" | "/health" | "/livez" => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(r#"{"status":"healthy"}"#)))
            .expect("static health response"),
        "/readyz" | "/ready" => {
            if service_healthy.load(std::sync::atomic::Ordering::Acquire) {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"status":"ready"}"#)))
                    .expect("static ready response")
            } else {
                Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"status":"not_ready"}"#)))
                    .expect("static not_ready response")
            }
        }
        // Deep readiness check — same as /readyz but with engine verification info
        "/readyz/deep" => {
            let healthy = service_healthy.load(std::sync::atomic::Ordering::Acquire);
            let sandboxes = dashboard.overview();
            let status = if healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let json = serde_json::json!({
                "status": if healthy { "ready" } else { "not_ready" },
                "active_sandboxes": sandboxes.active_sandboxes,
                "total_created": sandboxes.total_created,
            });
            Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json.to_string())))
                .expect("deep readyz response")
        }
        path if path == "/api/dashboard/overview"
            || path == "/api/dashboard/sandboxes"
            || path == "/api/dashboard/events"
            || path.starts_with("/api/dashboard/sandboxes/") =>
        {
            // Validate API key for all dashboard endpoints
            let api_key_valid = match std::env::var("ISOLATE_DASHBOARD_API_KEY") {
                Ok(expected_key) if !expected_key.is_empty() => {
                    req.headers().get("x-api-key").and_then(|v| v.to_str().ok()).is_some_and(|k| {
                        // Constant-time comparison to prevent timing attacks
                        let k_bytes = k.as_bytes();
                        let expected_bytes = expected_key.as_bytes();
                        if k_bytes.len() != expected_bytes.len() {
                            return false;
                        }
                        let mut result = 0u8;
                        for (a, b) in k_bytes.iter().zip(expected_bytes.iter()) {
                            result |= a ^ b;
                        }
                        result == 0
                    })
                }
                _ => false, // No API key configured; deny access
            };

            if !api_key_valid {
                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"unauthorized"}"#)))
                    .expect("static unauthorized response")
            } else {
                match path {
                    "/api/dashboard/overview" => {
                        let overview = dashboard.overview();
                        let json = serde_json::to_string(&overview).unwrap_or_default();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(json)))
                            .expect("dashboard overview response")
                    }
                    "/api/dashboard/sandboxes" => {
                        let sandboxes = dashboard.list_sandboxes();
                        let json = serde_json::to_string(&sandboxes).unwrap_or_default();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(json)))
                            .expect("dashboard sandboxes response")
                    }
                    "/api/dashboard/events" => {
                        let events = dashboard.recent_events(50);
                        let json = serde_json::to_string(&events).unwrap_or_default();
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("Content-Type", "application/json")
                            .body(Full::new(Bytes::from(json)))
                            .expect("dashboard events response")
                    }
                    _ if path.starts_with("/api/dashboard/sandboxes/") => {
                        let id_str = &path["/api/dashboard/sandboxes/".len()..];
                        match id_str.parse::<isolate_core::sandbox::SandboxId>() {
                            Ok(id) => match dashboard.get_sandbox(&id) {
                                Some(sandbox) => {
                                    let json = serde_json::to_string(&sandbox).unwrap_or_default();
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .header("Content-Type", "application/json")
                                        .body(Full::new(Bytes::from(json)))
                                        .expect("sandbox detail response")
                                }
                                None => Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(
                                        r#"{"error":"sandbox not found"}"#,
                                    )))
                                    .expect("static not_found response"),
                            },
                            Err(_) => Response::builder()
                                .status(StatusCode::BAD_REQUEST)
                                .header("Content-Type", "application/json")
                                .body(Full::new(Bytes::from(r#"{"error":"invalid sandbox id"}"#)))
                                .expect("static bad_request response"),
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        // v1 API endpoints (alias for dashboard data)
        "/api/v1/overview" => {
            let overview = dashboard.overview();
            let json = serde_json::to_string(&overview).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .expect("v1 overview response")
        }
        "/api/v1/sandboxes" => {
            let sandboxes = dashboard.list_sandboxes();
            let json = serde_json::to_string(&sandboxes).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .expect("v1 sandboxes response")
        }
        "/api/v1/events" => {
            let events = dashboard.recent_events(100);
            let json = serde_json::to_string(&events).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .expect("v1 events response")
        }
        "/api/v1/health" => {
            let healthy = service_healthy.load(std::sync::atomic::Ordering::Acquire);
            let status_code =
                if healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
            let json = format!(r#"{{"healthy":{healthy}}}"#);
            Response::builder()
                .status(status_code)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(json)))
                .expect("v1 health response")
        }
        "/api/v1/ws/events" => {
            // WebSocket endpoint metadata — actual upgrade requires tokio-tungstenite
            let info = serde_json::json!({
                "protocol": "websocket",
                "status": "planned",
                "channels": [
                    "sandbox.created",
                    "sandbox.completed",
                    "sandbox.failed",
                    "resource.threshold",
                    "alert.triggered"
                ],
                "note": "WebSocket upgrade not yet implemented. Use /api/v1/events for polling."
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(info.to_string())))
                .expect("ws info response")
        }
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .expect("static 404 response"),
    };

    Ok(response)
}

/// Run the HTTP health and dashboard API server.
async fn run_health_server(
    addr: SocketAddr,
    service_healthy: Arc<std::sync::atomic::AtomicBool>,
    dashboard: Arc<DashboardState>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = %addr, "HTTP health/dashboard endpoint started");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let healthy = service_healthy.clone();
        let dash = dashboard.clone();

        tokio::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req| {
                        let healthy = healthy.clone();
                        let dash = dash.clone();
                        health_handler(req, healthy, dash)
                    }),
                )
                .await
            {
                tracing::debug!("Health server connection error: {}", err);
            }
        });
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Build the tracing subscriber
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default());

    // Initialize OpenTelemetry if OTLP endpoint is configured
    let tracer_provider = if let Some(ref endpoint) = args.otlp_endpoint {
        if !args.no_tracing {
            // Build the OTLP exporter
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create OTLP exporter: {}", e))?;

            // Configure sampling
            let sampler = if args.sampling_ratio >= 1.0 {
                Sampler::AlwaysOn
            } else if args.sampling_ratio <= 0.0 {
                Sampler::AlwaysOff
            } else {
                Sampler::TraceIdRatioBased(args.sampling_ratio)
            };

            // Build resource with service info
            let resource = Resource::new(vec![
                KeyValue::new("service.name", args.service_name.clone()),
                KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            ]);

            // Build tracer provider
            let provider = opentelemetry_sdk::trace::TracerProvider::builder()
                .with_batch_exporter(exporter, runtime::Tokio)
                .with_sampler(sampler)
                .with_resource(resource)
                .build();

            // Set as global provider and get a tracer
            opentelemetry::global::set_tracer_provider(provider.clone());
            Some(provider)
        } else {
            None
        }
    } else {
        None
    };

    // Create the base subscriber registry
    let subscriber = tracing_subscriber::registry().with(filter);

    // Add format layer (JSON or pretty) and optionally OpenTelemetry
    match (&tracer_provider, args.json_logs) {
        (Some(provider), true) => {
            let tracer = provider.tracer("isolate-server");
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
            subscriber.with(tracing_subscriber::fmt::layer().json()).with(otel_layer).init();
        }
        (Some(provider), false) => {
            let tracer = provider.tracer("isolate-server");
            let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
            subscriber.with(tracing_subscriber::fmt::layer()).with(otel_layer).init();
        }
        (None, true) => {
            subscriber.with(tracing_subscriber::fmt::layer().json()).init();
        }
        (None, false) => {
            subscriber.with(tracing_subscriber::fmt::layer()).init();
        }
    }

    if let Some(ref endpoint) = args.otlp_endpoint {
        tracing::info!(
            otlp_endpoint = %endpoint,
            service_name = %args.service_name,
            sampling_ratio = args.sampling_ratio,
            "OpenTelemetry tracing enabled"
        );
    }

    tracing::info!(grpc_addr = %args.addr, "Starting Isolate gRPC server");

    // Shared health status
    let service_healthy = Arc::new(std::sync::atomic::AtomicBool::new(true));

    // Create gRPC health reporter
    let (mut health_reporter, health_service) = health_reporter();

    // Set initial health status
    health_reporter.set_serving::<IsolateServiceServer<IsolateServiceImpl>>().await;

    // Create the service
    let service =
        IsolateServiceImpl::with_rate_limit(args.max_sandboxes, args.rate_limit, args.rate_burst);
    tracing::info!(
        rate_limit = args.rate_limit,
        rate_burst = args.rate_burst,
        "Rate limiter configured"
    );
    let dashboard = service.dashboard();

    // Start HTTP health server if enabled
    if !args.no_health_http {
        let health_addr = args.health_addr;
        let healthy = service_healthy.clone();
        let dash = dashboard.clone();
        tokio::spawn(async move {
            if let Err(e) = run_health_server(health_addr, healthy, dash).await {
                tracing::error!(error = %e, "HTTP health server failed");
            }
        });
    }

    // Start the gRPC server with health service
    let mut builder = Server::builder();

    // Configure TLS if cert and key are provided
    if let (Some(cert_path), Some(key_path)) = (&args.tls_cert, &args.tls_key) {
        let cert = tokio::fs::read(cert_path).await?;
        let key = tokio::fs::read(key_path).await?;
        let identity = Identity::from_pem(cert, key);
        let mut tls_config = ServerTlsConfig::new().identity(identity);

        if let Some(ca_path) = &args.tls_ca {
            let ca = tokio::fs::read(ca_path).await?;
            tls_config = tls_config.client_ca_root(Certificate::from_pem(ca));
            tracing::info!("mTLS enabled (client certificate verification)");
        }

        builder = builder.tls_config(tls_config)?;
        tracing::info!("TLS enabled for gRPC server");
    }

    let interceptor = auth::AuthInterceptor::new(args.api_key.clone());
    if args.api_key.is_some() {
        tracing::info!("gRPC API key authentication enabled");
    }

    let svc = IsolateServiceServer::new(service).max_decoding_message_size(args.max_module_size);
    let svc = tonic::service::interceptor::InterceptedService::new(svc, interceptor);

    let shutdown_timeout = Duration::from_secs(args.shutdown_timeout);
    let shutdown_healthy = service_healthy.clone();

    builder
        .add_service(health_service)
        .add_service(svc)
        .serve_with_shutdown(args.addr, async move {
            shutdown_signal().await;
            // Mark service as unhealthy so load balancers stop routing
            shutdown_healthy.store(false, std::sync::atomic::Ordering::Release);
            tracing::info!(
                timeout_secs = shutdown_timeout.as_secs(),
                "Shutdown signal received, draining in-flight requests..."
            );
        })
        .await?;

    tracing::info!("Server shut down gracefully");

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to listen for ctrl+c");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Empty;
    use hyper::body::Bytes;

    fn default_healthy() -> Arc<std::sync::atomic::AtomicBool> {
        Arc::new(std::sync::atomic::AtomicBool::new(true))
    }

    fn default_dashboard() -> Arc<DashboardState> {
        Arc::new(DashboardState::new(100))
    }

    /// Shared test API key — set once for all tests via ctor.
    const TEST_API_KEY: &str = "test-api-key-for-tests";

    /// Test the health handler by starting a real HTTP server and making requests.
    async fn test_request(path: &str) -> (StatusCode, String) {
        test_request_with_headers(path, default_healthy(), default_dashboard(), vec![]).await
    }

    /// Test with an authenticated dashboard request.
    #[allow(clippy::disallowed_methods)]
    async fn test_dashboard_request(path: &str) -> (StatusCode, String) {
        std::env::set_var("ISOLATE_DASHBOARD_API_KEY", TEST_API_KEY);
        test_request_with_headers(
            path,
            default_healthy(),
            default_dashboard(),
            vec![("x-api-key", TEST_API_KEY)],
        )
        .await
    }

    async fn test_request_with_headers(
        path: &str,
        healthy: Arc<std::sync::atomic::AtomicBool>,
        dashboard: Arc<DashboardState>,
        headers: Vec<(&str, &str)>,
    ) -> (StatusCode, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let h = healthy.clone();
        let d = dashboard.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req| {
                        let h = h.clone();
                        let d = d.clone();
                        health_handler(req, h, d)
                    }),
                )
                .await
                .unwrap();
        });

        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(conn);

        let mut builder = hyper::Request::builder().uri(path);
        for (key, value) in headers {
            builder = builder.header(key, value);
        }
        let req = builder.body(Empty::<Bytes>::new()).unwrap();
        let resp = sender.send_request(req).await.unwrap();
        let status = resp.status();
        let body_bytes =
            http_body_util::BodyExt::collect(resp.into_body()).await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body_bytes).to_string())
    }

    #[tokio::test]
    async fn test_healthz_returns_200() {
        let (status, _) = test_request("/healthz").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_alias_returns_200() {
        let (status, _) = test_request("/health").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_livez_returns_200() {
        let (status, _) = test_request("/livez").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_readyz_returns_200_when_healthy() {
        let (status, _) = test_request("/readyz").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_readyz_returns_503_when_not_healthy() {
        let healthy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (status, _) =
            test_request_with_headers("/readyz", healthy, default_dashboard(), vec![]).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_unknown_path_returns_404() {
        let (status, _) = test_request("/nonexistent").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_malformed_sandbox_id_returns_400() {
        let (status, body) =
            test_dashboard_request("/api/dashboard/sandboxes/not-a-valid-uuid").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("invalid sandbox id"));
    }

    #[tokio::test]
    async fn test_nonexistent_sandbox_returns_404() {
        let valid_uuid = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/dashboard/sandboxes/{}", valid_uuid);
        let (status, _) = test_dashboard_request(&path).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_dashboard_overview_returns_200() {
        let (status, _) = test_dashboard_request("/api/dashboard/overview").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dashboard_sandboxes_list_returns_200() {
        let (status, _) = test_dashboard_request("/api/dashboard/sandboxes").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dashboard_events_returns_200() {
        let (status, _) = test_dashboard_request("/api/dashboard/events").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    #[allow(clippy::disallowed_methods)]
    async fn test_dashboard_rejects_unauthenticated() {
        std::env::set_var("ISOLATE_DASHBOARD_API_KEY", TEST_API_KEY);
        let (status, body) = test_request("/api/dashboard/overview").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body.contains("unauthorized"));
    }
}
