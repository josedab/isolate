---
sidebar_position: 4
---

# Monitoring & Metrics

Isolate provides comprehensive observability through Prometheus metrics, structured logging, and OpenTelemetry tracing.

## Prometheus Metrics

### Enabling Metrics

```rust
use isolate_core::metrics::MetricsExporter;

// Start the metrics server
let exporter = MetricsExporter::new("0.0.0.0:9090");
exporter.start().await?;
```

### Available Metrics

#### Sandbox Lifecycle

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_sandboxes_created_total` | Counter | Total sandboxes created |
| `isolate_sandboxes_active` | Gauge | Currently active sandboxes |
| `isolate_sandbox_creation_duration_seconds` | Histogram | Time to create a sandbox |

#### Execution

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_sandbox_runs_total` | Counter | Total sandbox executions |
| `isolate_sandbox_duration_seconds` | Histogram | Execution duration |
| `isolate_sandbox_exit_code` | Counter | Exit codes (labeled) |

#### Resources

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_sandbox_fuel_consumed` | Histogram | Fuel consumption |
| `isolate_sandbox_memory_peak_bytes` | Histogram | Peak memory usage |
| `isolate_sandbox_io_read_bytes` | Counter | Total bytes read |
| `isolate_sandbox_io_write_bytes` | Counter | Total bytes written |

#### Capabilities

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_capability_checks_total` | Counter | Capability checks performed |
| `isolate_capability_denials_total` | Counter | Denied capability requests |

### Prometheus Scrape Config

```yaml
scrape_configs:
  - job_name: 'isolate'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Grafana Dashboard

A pre-built Grafana dashboard is available at `examples/grafana/isolate-dashboard.json`.

Key panels:
- Sandbox creation rate
- Active sandboxes over time
- Execution duration percentiles
- Resource usage distribution
- Capability denial rate

## Structured Logging

### Setup

```rust
use tracing_subscriber::{fmt, EnvFilter};

tracing_subscriber::fmt()
    .json()  // JSON format for log aggregation
    .with_env_filter(EnvFilter::from_default_env())
    .init();
```

### Log Levels

| Level | Content |
|-------|---------|
| `error` | Execution failures, system errors |
| `warn` | Capability denials, resource limit hits |
| `info` | Sandbox lifecycle events |
| `debug` | Detailed execution info |
| `trace` | All capability checks, WASM calls |

### Example Log Output

```json
{
  "timestamp": "2024-01-15T10:30:00.123Z",
  "level": "INFO",
  "target": "isolate::sandbox",
  "message": "Sandbox created",
  "sandbox_id": "550e8400-e29b-41d4-a716-446655440000",
  "module_hash": "sha256:abc123...",
  "cold_start_ms": 3.2
}
```

### Filtering by Component

```bash
# All isolate logs
RUST_LOG=isolate=info

# Only capability audit logs
RUST_LOG=isolate::capability::audit=warn

# Sandbox lifecycle + errors
RUST_LOG=isolate::sandbox=info,isolate=error
```

## OpenTelemetry Tracing

### Setup

```rust
use opentelemetry::global;
use tracing_subscriber::layer::SubscriberExt;

// Initialize OTLP exporter
let tracer = opentelemetry_otlp::new_pipeline()
    .tracing()
    .with_exporter(opentelemetry_otlp::new_exporter().tonic())
    .install_batch(opentelemetry::runtime::Tokio)?;

// Create a tracing layer
let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

tracing_subscriber::registry()
    .with(telemetry)
    .init();
```

### Trace Spans

Isolate creates spans for:

- `sandbox.create` - Sandbox creation
- `sandbox.run` - Execution
- `capability.check` - Each capability check
- `wasm.call` - WASM function calls

### Span Attributes

| Attribute | Description |
|-----------|-------------|
| `sandbox.id` | Unique sandbox identifier |
| `sandbox.module_hash` | SHA-256 of WASM module |
| `sandbox.exit_code` | Exit code (on completion) |
| `sandbox.fuel_consumed` | Fuel used |
| `capability.name` | Capability being checked |
| `capability.granted` | Whether it was granted |

## Health Checks

### HTTP Health Endpoint

```rust
use isolate_core::health::HealthServer;

let health = HealthServer::new()
    .with_liveness("/health/live")
    .with_readiness("/health/ready");

health.start("0.0.0.0:8080").await?;
```

### Kubernetes Probes

```yaml
livenessProbe:
  httpGet:
    path: /health/live
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /health/ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 5
```

## Alerting Rules

### Prometheus Alert Examples

```yaml
groups:
  - name: isolate
    rules:
      - alert: HighCapabilityDenialRate
        expr: rate(isolate_capability_denials_total[5m]) > 10
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High capability denial rate"

      - alert: SandboxTimeout
        expr: rate(isolate_sandbox_exit_code{code="timeout"}[5m]) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Sandboxes are timing out"

      - alert: HighMemoryUsage
        expr: isolate_sandbox_memory_peak_bytes > 100000000
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "Sandbox using >100MB memory"
```

## Best Practices

1. **Set appropriate log levels in production** - Use `info` for most components, `warn` for audit
2. **Use structured logging** - JSON format for easy parsing
3. **Monitor capability denials** - May indicate misconfiguration or attacks
4. **Track resource usage trends** - Helps optimize limits
5. **Set up alerts for anomalies** - Early warning for issues

## See Also

- [Resource Limits](./resource-limits) - Configure resource tracking
- [Security Model](./security-model) - Understanding audit logging
- [gRPC Server](./grpc-server) - Monitoring the server
