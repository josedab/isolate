---
sidebar_position: 9
---

# Deployment Guide

This guide covers deploying Isolate in production environments, including containerization, Kubernetes, configuration best practices, and security hardening.

## Production Checklist

Before deploying to production, ensure you've addressed:

- [ ] Resource limits configured for all sandboxes
- [ ] Capabilities follow principle of least privilege
- [ ] Monitoring and alerting set up
- [ ] Audit logging enabled
- [ ] Security hardening applied (Linux)
- [ ] Performance tested under expected load

## Configuration for Production

### Environment Variables

Configure Isolate through environment variables:

```bash
# Logging
RUST_LOG=isolate=info,isolate::capability::audit=warn

# Metrics
ISOLATE_METRICS_PORT=9090
ISOLATE_METRICS_PATH=/metrics

# Engine settings
ISOLATE_MODULE_CACHE_SIZE=1000
ISOLATE_EPOCH_TICK_MS=10
```

### Recommended Defaults

```rust
use isolate_core::{SandboxConfig, capability::Capability};
use std::time::Duration;

fn production_config(wasm_bytes: &[u8]) -> isolate_core::Result<SandboxConfig> {
    SandboxConfig::builder()
        .module(wasm_bytes)?
        // Conservative memory limit
        .memory_limit(64 * 1024 * 1024)  // 64MB
        // CPU protection
        .fuel(10_000_000)
        .wall_time_limit(Duration::from_secs(30))
        // I/O limits
        .io_write_limit(1024 * 1024)  // 1MB output
        // Minimal capabilities
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()
}
```

## Docker Deployment

### Dockerfile

```dockerfile
# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app
COPY . .

RUN cargo build --release --package isolate-server

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/isolate-server /usr/local/bin/

# Non-root user
RUN useradd -r -s /bin/false isolate
USER isolate

EXPOSE 50051 9090

ENTRYPOINT ["isolate-server"]
CMD ["--addr", "0.0.0.0:50051", "--metrics-addr", "0.0.0.0:9090"]
```

### Docker Compose

```yaml
version: '3.8'

services:
  isolate:
    build: .
    ports:
      - "50051:50051"  # gRPC
      - "9090:9090"    # Metrics
    environment:
      - RUST_LOG=isolate=info
      - ISOLATE_MODULE_CACHE_SIZE=500
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 2G
        reservations:
          cpus: '0.5'
          memory: 512M
    healthcheck:
      test: ["CMD", "grpc-health-probe", "-addr=:50051"]
      interval: 10s
      timeout: 5s
      retries: 3

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9091:9090"
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    volumes:
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards
```

### Prometheus Configuration

```yaml
# prometheus.yml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'isolate'
    static_configs:
      - targets: ['isolate:9090']
```

## Kubernetes Deployment

### Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isolate-server
  labels:
    app: isolate
spec:
  replicas: 3
  selector:
    matchLabels:
      app: isolate
  template:
    metadata:
      labels:
        app: isolate
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "9090"
        prometheus.io/path: "/metrics"
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        fsGroup: 1000
      containers:
        - name: isolate
          image: ghcr.io/josedab/isolate:latest
          ports:
            - containerPort: 50051
              name: grpc
            - containerPort: 9090
              name: metrics
          env:
            - name: RUST_LOG
              value: "isolate=info"
            - name: ISOLATE_MODULE_CACHE_SIZE
              value: "500"
          resources:
            requests:
              cpu: "500m"
              memory: "512Mi"
            limits:
              cpu: "2000m"
              memory: "2Gi"
          livenessProbe:
            grpc:
              port: 50051
            initialDelaySeconds: 5
            periodSeconds: 10
          readinessProbe:
            grpc:
              port: 50051
            initialDelaySeconds: 5
            periodSeconds: 5
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: true
            capabilities:
              drop:
                - ALL
```

### Service

```yaml
apiVersion: v1
kind: Service
metadata:
  name: isolate-server
  labels:
    app: isolate
spec:
  type: ClusterIP
  ports:
    - port: 50051
      targetPort: grpc
      name: grpc
    - port: 9090
      targetPort: metrics
      name: metrics
  selector:
    app: isolate
```

### Horizontal Pod Autoscaler

```yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: isolate-server
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: isolate-server
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
```

### Pod Disruption Budget

```yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: isolate-server
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: isolate
```

### Network Policy

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: isolate-server
spec:
  podSelector:
    matchLabels:
      app: isolate
  policyTypes:
    - Ingress
    - Egress
  ingress:
    - from:
        - podSelector:
            matchLabels:
              role: api-gateway
      ports:
        - protocol: TCP
          port: 50051
    - from:
        - namespaceSelector:
            matchLabels:
              name: monitoring
      ports:
        - protocol: TCP
          port: 9090
  egress:
    - to:
        - podSelector: {}
      ports:
        - protocol: TCP
          port: 443
```

## Security Hardening

### Linux Security Modules

On Linux, enable additional OS-level protection:

```rust
use isolate_core::experimental::security::{SeccompPolicy, SecurityConfig};

// Enable seccomp-bpf filtering (when available)
let security = SecurityConfig::builder()
    .seccomp(SeccompPolicy::Strict)
    .build()?;
```

### Container Security

```yaml
# In Kubernetes deployment
securityContext:
  runAsNonRoot: true
  runAsUser: 1000
  readOnlyRootFilesystem: true
  allowPrivilegeEscalation: false
  capabilities:
    drop:
      - ALL
  seccompProfile:
    type: RuntimeDefault
```

### Resource Quotas

Set cluster-level limits:

```yaml
apiVersion: v1
kind: ResourceQuota
metadata:
  name: isolate-quota
spec:
  hard:
    requests.cpu: "10"
    requests.memory: 20Gi
    limits.cpu: "20"
    limits.memory: 40Gi
    pods: "50"
```

## Monitoring Setup

### Key Metrics to Watch

| Metric | Alert Threshold | Description |
|--------|-----------------|-------------|
| `sandbox_executions_total` | - | Execution count |
| `sandbox_execution_duration_seconds` | p99 > 5s | Execution latency |
| `sandbox_fuel_consumed` | - | CPU usage per sandbox |
| `capability_denials_total` | > 100/min | Security events |
| `sandbox_errors_total` | > 10/min | Error rate |

### Grafana Dashboard

```json
{
  "title": "Isolate Overview",
  "panels": [
    {
      "title": "Executions per Second",
      "type": "graph",
      "targets": [
        {
          "expr": "rate(sandbox_executions_total[5m])",
          "legendFormat": "{{instance}}"
        }
      ]
    },
    {
      "title": "Execution Latency (p99)",
      "type": "graph",
      "targets": [
        {
          "expr": "histogram_quantile(0.99, rate(sandbox_execution_duration_seconds_bucket[5m]))",
          "legendFormat": "p99"
        }
      ]
    },
    {
      "title": "Capability Denials",
      "type": "stat",
      "targets": [
        {
          "expr": "sum(rate(capability_denials_total[5m]))"
        }
      ]
    }
  ]
}
```

### Alerting Rules

```yaml
# alerting-rules.yml
groups:
  - name: isolate
    rules:
      - alert: HighErrorRate
        expr: rate(sandbox_errors_total[5m]) > 0.1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: High sandbox error rate

      - alert: HighCapabilityDenials
        expr: rate(capability_denials_total[5m]) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: Unusual capability denial rate

      - alert: HighLatency
        expr: histogram_quantile(0.99, rate(sandbox_execution_duration_seconds_bucket[5m])) > 5
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: High sandbox execution latency
```

## Performance Tuning

### Engine Sharing

Share the WASM engine across multiple sandboxes:

```rust
use isolate_core::{Sandbox, SandboxConfig, engine::WasmEngine};
use std::sync::Arc;

// Create a shared engine
let engine = Arc::new(WasmEngine::new()?);

// Create multiple sandboxes sharing the engine
for _ in 0..100 {
    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .build()?;

    let sandbox = Sandbox::create_with_engine(config, engine.clone()).await?;
    // ...
}

// Modules are cached and reused
println!("Cached modules: {}", engine.cached_module_count());
```

### Connection Pooling

For the gRPC server, use connection pooling on the client side:

```rust
use tonic::transport::Channel;

// Create a channel with connection pooling
let channel = Channel::from_static("http://isolate-server:50051")
    .connect_timeout(Duration::from_secs(5))
    .timeout(Duration::from_secs(30))
    .concurrency_limit(100)
    .connect()
    .await?;
```

### Memory Management

Clear the module cache periodically if memory is constrained:

```rust
// Clear cache when memory pressure is detected
if memory_usage > threshold {
    engine.clear_cache();
}
```

## High Availability

### Multi-Region Deployment

```yaml
# Deploy to multiple regions with regional load balancing
apiVersion: v1
kind: Service
metadata:
  name: isolate-server-global
  annotations:
    cloud.google.com/neg: '{"ingress": true}'
spec:
  type: ClusterIP
  ports:
    - port: 50051
  selector:
    app: isolate
```

### Disaster Recovery

1. **Module Storage**: Store WASM modules in distributed storage (S3, GCS)
2. **State**: Isolate is stateless - no special DR needed for runtime
3. **Configuration**: Store config in version control or ConfigMaps
4. **Metrics**: Use remote write to preserve metrics history

## Troubleshooting Production Issues

### High Memory Usage

```bash
# Check module cache size
curl -s localhost:9090/metrics | grep module_cache

# Clear cache if needed (via API)
grpcurl -d '{}' localhost:50051 isolate.v1.Admin/ClearCache
```

### Slow Cold Starts

1. Pre-warm the module cache on startup
2. Increase module cache size
3. Use AOT compilation (Wasmtime cranelift)

### Capability Denials

```bash
# Check audit logs for denied capabilities
kubectl logs -l app=isolate | grep "capability_denied"

# Review capability configuration
```

## See Also

- [Monitoring](./monitoring) - Detailed monitoring setup
- [Security Model](./security-model) - Security architecture
- [Configuration Reference](../reference/configuration) - All configuration options
- [gRPC Server](./grpc-server) - gRPC API documentation
