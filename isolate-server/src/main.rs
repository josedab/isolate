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
use tonic::transport::Server;
use tonic_health::server::health_reporter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod proto {
    tonic::include_proto!("isolate.v1");
}

use proto::isolate_service_server::IsolateServiceServer;
use service::IsolateServiceImpl;
use isolate_core::dashboard::DashboardState;

/// Isolate gRPC Server
#[derive(Parser, Debug)]
#[command(name = "isolate-server")]
#[command(about = "gRPC server for the Isolate secure sandbox runtime")]
struct Args {
    /// gRPC address to bind to
    #[arg(short, long, default_value = "0.0.0.0:50051")]
    addr: SocketAddr,

    /// HTTP health check address to bind to
    #[arg(long, default_value = "0.0.0.0:8080")]
    health_addr: SocketAddr,

    /// Disable HTTP health endpoint
    #[arg(long)]
    no_health_http: bool,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Enable JSON logging
    #[arg(long)]
    json_logs: bool,

    /// Maximum number of concurrent sandboxes
    #[arg(long, default_value = "100")]
    max_sandboxes: usize,

    /// Enable warm pool
    #[arg(long)]
    warm_pool: bool,

    /// Warm pool size per module
    #[arg(long, default_value = "5")]
    warm_pool_size: usize,

    // OpenTelemetry options
    /// OTLP endpoint for OpenTelemetry export (e.g., http://localhost:4317)
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,

    /// Service name for tracing (default: isolate-server)
    #[arg(long, env = "OTEL_SERVICE_NAME", default_value = "isolate-server")]
    service_name: String,

    /// Sampling ratio for traces (0.0 to 1.0, default: 1.0 for all traces)
    #[arg(long, default_value = "1.0")]
    sampling_ratio: f64,

    /// Disable OpenTelemetry tracing
    #[arg(long)]
    no_tracing: bool,
}

/// HTTP health check and dashboard API handler.
async fn health_handler(
    req: Request<hyper::body::Incoming>,
    service_healthy: Arc<std::sync::atomic::AtomicBool>,
    dashboard: Arc<DashboardState>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let path = req.uri().path();

    let response = match path {
        "/healthz" | "/health" | "/livez" => {
            Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(r#"{"status":"healthy"}"#)))
                .expect("static health response")
        }
        "/readyz" | "/ready" => {
            if service_healthy.load(std::sync::atomic::Ordering::Relaxed) {
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
                        .body(Full::new(Bytes::from(r#"{"error":"sandbox not found"}"#)))
                        .expect("static not_found response"),
                },
                Err(_) => Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Content-Type", "application/json")
                    .body(Full::new(Bytes::from(r#"{"error":"invalid sandbox id"}"#)))
                    .expect("static bad_request response"),
            }
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
    let service = IsolateServiceImpl::new(args.max_sandboxes);
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
    Server::builder()
        .add_service(health_service)
        .add_service(IsolateServiceServer::new(service))
        .serve(args.addr)
        .await?;

    Ok(())
}
