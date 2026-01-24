# Monitoring & Metrics

Isolate provides comprehensive observability through Prometheus metrics, OpenTelemetry tracing, and audit logging.

## Prometheus Metrics

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_sandbox_created_total` | Counter | Total sandboxes created |
| `isolate_sandbox_runs_total` | Counter | Total sandbox executions |
| `isolate_sandbox_errors_total` | Counter | Total execution errors |
| `isolate_sandbox_duration_seconds` | Histogram | Execution duration |
| `isolate_sandbox_cold_start_seconds` | Histogram | Sandbox creation time |
| `isolate_sandbox_fuel_consumed` | Histogram | Fuel units consumed |
| `isolate_sandbox_memory_peak_bytes` | Gauge | Peak memory usage |
| `isolate_sandbox_active` | Gauge | Currently active sandboxes |

### Exporting Metrics

```rust
use prometheus::{Encoder, TextEncoder};

// Get the metrics registry
let metrics = sandbox.metrics();

// Export in Prometheus format
let encoder = TextEncoder::new();
let mut buffer = Vec::new();
encoder.encode(&prometheus::gather(), &mut buffer)?;
println!("{}", String::from_utf8(buffer)?);
```

### Prometheus Scrape Endpoint

For production, expose a `/metrics` endpoint:

```rust
use axum::{routing::get, Router};
use prometheus::{Encoder, TextEncoder};

async fn metrics_handler() -> String {
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

let app = Router::new().route("/metrics", get(metrics_handler));
```

### Example Prometheus Config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'isolate'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Grafana Dashboard

Key panels to include:

1. **Sandbox Creation Rate**: `rate(isolate_sandbox_created_total[5m])`
2. **Error Rate**: `rate(isolate_sandbox_errors_total[5m])`
3. **P99 Latency**: `histogram_quantile(0.99, rate(isolate_sandbox_duration_seconds_bucket[5m]))`
4. **Memory Usage**: `isolate_sandbox_memory_peak_bytes`
5. **Active Sandboxes**: `isolate_sandbox_active`

## OpenTelemetry Tracing

The gRPC server includes built-in OpenTelemetry support for distributed tracing. Enable it by providing an OTLP endpoint.

### Server Configuration

```bash
# Enable OTLP export to a local Jaeger/OTEL collector
isolate-server --otlp-endpoint http://localhost:4317

# With custom service name and sampling
isolate-server \
    --otlp-endpoint http://otel-collector:4317 \
    --service-name my-isolate-service \
    --sampling-ratio 0.1

# Using environment variables
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
export OTEL_SERVICE_NAME=my-isolate-service
isolate-server
```

### Server CLI Options

| Flag | Environment Variable | Description |
|------|---------------------|-------------|
| `--otlp-endpoint` | `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP gRPC endpoint (e.g., `http://localhost:4317`) |
| `--service-name` | `OTEL_SERVICE_NAME` | Service name for traces (default: `isolate-server`) |
| `--sampling-ratio` | - | Trace sampling ratio, 0.0-1.0 (default: 1.0) |
| `--no-tracing` | - | Disable OpenTelemetry tracing |

### Exported Spans

The server exports the following spans for each gRPC operation:

| Span Name | Description |
|-----------|-------------|
| `grpc.create_sandbox` | Sandbox creation including WASM compilation |
| `grpc.run_sandbox` | Sandbox execution with input/output |
| `grpc.terminate_sandbox` | Sandbox cleanup and termination |
| `grpc.get_sandbox` | Sandbox status retrieval |
| `grpc.list_sandboxes` | List sandbox operations |
| `grpc.stream_output` | Output streaming |
| `grpc.get_metrics` | Metrics retrieval |

### Span Attributes

All gRPC spans include standard OpenTelemetry semantic conventions:

- `otel.kind`: `server`
- `rpc.system`: `grpc`
- `rpc.service`: `isolate.v1.IsolateService`
- `rpc.method`: The specific method name

Sandbox-specific spans include:

- `sandbox.id`: The sandbox UUID
- `sandbox.module_hash`: Hash of the WASM module
- `sandbox.exit_code`: Exit code (for run operations)

### Example: Running with Jaeger

```bash
# Start Jaeger
docker run -d --name jaeger \
    -p 16686:16686 \
    -p 4317:4317 \
    jaegertracing/all-in-one:latest

# Start isolate-server with OTEL
isolate-server --otlp-endpoint http://localhost:4317

# View traces at http://localhost:16686
```

### Custom Library Integration

For custom applications using `isolate-core`, you can also integrate OpenTelemetry:

```rust
use opentelemetry::global;
use opentelemetry_sdk::trace::TracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;

// Initialize OpenTelemetry
let tracer = opentelemetry_otlp::new_pipeline()
    .tracing()
    .with_exporter(opentelemetry_otlp::new_exporter().tonic())
    .install_batch(opentelemetry_sdk::runtime::Tokio)?;

// Set up tracing subscriber
tracing_subscriber::registry()
    .with(OpenTelemetryLayer::new(tracer))
    .with(tracing_subscriber::fmt::layer())
    .init();
```

### Trace Structure

```
Trace: sandbox_execution
├── Span: create_sandbox
│   ├── Span: compile_module
│   └── Span: create_instance
├── Span: run_sandbox
│   ├── Span: check_capabilities
│   ├── Span: execute_wasm
│   └── Span: capture_output
└── Span: cleanup
```

### Adding Custom Spans

```rust
use tracing::{instrument, info_span};

#[instrument(skip(sandbox))]
async fn process_request(sandbox: &mut Sandbox, input: &[u8]) -> Result<Output> {
    let span = info_span!("process_request", input_size = input.len());
    let _guard = span.enter();

    sandbox.run(input).await
}
```

## Audit Logging

### Enable Audit Logs

```rust
tracing_subscriber::fmt()
    .with_env_filter("isolate::capability::audit=info")
    .json()  // Structured logging
    .init();
```

### Log Format

```json
{
  "timestamp": "2024-01-15T10:30:00Z",
  "level": "INFO",
  "target": "isolate::capability::audit",
  "sandbox_id": "550e8400-e29b-41d4-a716-446655440000",
  "event": "capability_granted",
  "capability": "filesystem_read",
  "path": "/data/input.json"
}
```

### Security Events

| Event | Level | Description |
|-------|-------|-------------|
| `capability_granted` | INFO | Capability check passed |
| `capability_denied` | WARN | Capability check failed |
| `resource_limit_hit` | WARN | Resource limit exceeded |
| `sandbox_created` | INFO | New sandbox created |
| `sandbox_terminated` | INFO | Sandbox terminated |

## Health Checks

### Liveness Check

```rust
async fn liveness() -> impl IntoResponse {
    StatusCode::OK
}
```

### Readiness Check

```rust
async fn readiness(pool: &SandboxPool) -> impl IntoResponse {
    if pool.available() > 0 {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
```

## Alerting Rules

### Prometheus Alerting

```yaml
# alerts.yml
groups:
  - name: isolate
    rules:
      - alert: HighErrorRate
        expr: rate(isolate_sandbox_errors_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High sandbox error rate"

      - alert: SlowSandboxCreation
        expr: histogram_quantile(0.99, rate(isolate_sandbox_cold_start_seconds_bucket[5m])) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Slow sandbox creation (>100ms p99)"

      - alert: MemoryPressure
        expr: isolate_sandbox_memory_peak_bytes > 1e9
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "Sandbox using >1GB memory"
```

## Structured Logging Best Practices

### Use Structured Fields

```rust
tracing::info!(
    sandbox_id = %sandbox.id(),
    module_hash = %sandbox.module_hash(),
    duration_ms = output.duration.as_millis(),
    exit_code = output.exit_code,
    "Sandbox execution completed"
);
```

### Consistent Field Names

| Field | Type | Description |
|-------|------|-------------|
| `sandbox_id` | UUID | Sandbox identifier |
| `module_hash` | String | WASM module hash |
| `duration_ms` | u64 | Execution time in ms |
| `exit_code` | i32 | Process exit code |
| `error` | String | Error message |
| `capability` | String | Capability name |

## See Also

- [gRPC Server](./grpc-server.md) - Remote monitoring
- [Security Model](./security-model.md) - Audit requirements
- [Resource Limits](./resource-limits.md) - Metrics for resource usage
