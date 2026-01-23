# isolate-server

[![Crates.io](https://img.shields.io/crates/v/isolate-server.svg)](https://crates.io/crates/isolate-server)
[![License](https://img.shields.io/crates/l/isolate-server.svg)](../LICENSE-MIT)

gRPC server for remote sandbox management. Provides network access to the Isolate secure sandbox runtime with support for concurrent sandboxes, output streaming, and Prometheus metrics.

## Installation

```bash
# From crates.io
cargo install isolate-server

# From source
cargo install --path isolate-server
```

## Quick Start

```bash
# Start the server
isolate-server --addr 0.0.0.0:50051

# Start with custom limits
isolate-server \
    --addr 0.0.0.0:50051 \
    --max-sandboxes 200 \
    --log-level info \
    --json-logs
```

## Command Line Options

```bash
isolate-server [OPTIONS]

Options:
  -a, --addr <ADDR>           Address to bind to [default: 0.0.0.0:50051]
  -l, --log-level <LEVEL>     Log level [default: info]
      --json-logs             Enable JSON structured logging
      --max-sandboxes <N>     Maximum concurrent sandboxes [default: 100]
      --warm-pool             Enable warm pool for faster starts
      --warm-pool-size <N>    Warm pool size per module [default: 5]
  -h, --help                  Print help
  -V, --version               Print version
```

## gRPC API

The server implements the `IsolateService` defined in `proto/isolate.proto`.

### Service Definition

```protobuf
service IsolateService {
  // Create a new sandbox
  rpc CreateSandbox(CreateSandboxRequest) returns (CreateSandboxResponse);

  // Run a sandbox with optional input
  rpc RunSandbox(RunSandboxRequest) returns (RunSandboxResponse);

  // Get sandbox status and metrics
  rpc GetSandbox(GetSandboxRequest) returns (GetSandboxResponse);

  // Terminate a sandbox
  rpc TerminateSandbox(TerminateSandboxRequest) returns (TerminateSandboxResponse);

  // List all sandboxes (with pagination)
  rpc ListSandboxes(ListSandboxesRequest) returns (ListSandboxesResponse);

  // Stream sandbox output in real-time
  rpc StreamOutput(StreamOutputRequest) returns (stream OutputChunk);

  // Get Prometheus or JSON metrics
  rpc GetMetrics(GetMetricsRequest) returns (GetMetricsResponse);
}
```

### Capability Types

When creating a sandbox, capabilities are specified as type/value pairs:

| Type | Value | Description |
|------|-------|-------------|
| `stdout` | - | Write to stdout |
| `stderr` | - | Write to stderr |
| `stdin` | - | Read from stdin |
| `fs:read` | path | Read from filesystem path |
| `fs:write` | path | Write to filesystem path |
| `fs:temp` | - | Access temp directory |
| `http` | host | HTTP access to host |
| `dns` | - | DNS resolution |
| `time:system` | - | System clock access |
| `time:monotonic` | - | Monotonic clock access |
| `random` | - | Secure random numbers |
| `env` | name | Read environment variable |

## Client Examples

### Using grpcurl

```bash
# Create a sandbox
grpcurl -plaintext \
  -d '{
    "module": "'$(base64 module.wasm)'",
    "config": {
      "memory_limit": 134217728,
      "fuel_limit": 1000000,
      "capabilities": [
        {"type": "stdout"},
        {"type": "stderr"}
      ]
    }
  }' \
  localhost:50051 isolate.v1.IsolateService/CreateSandbox

# Run the sandbox
grpcurl -plaintext \
  -d '{"sandbox_id": "abc123"}' \
  localhost:50051 isolate.v1.IsolateService/RunSandbox

# Get sandbox info
grpcurl -plaintext \
  -d '{"sandbox_id": "abc123"}' \
  localhost:50051 isolate.v1.IsolateService/GetSandbox

# List all sandboxes
grpcurl -plaintext \
  -d '{"limit": 10}' \
  localhost:50051 isolate.v1.IsolateService/ListSandboxes

# Get metrics
grpcurl -plaintext \
  -d '{"format": "prometheus"}' \
  localhost:50051 isolate.v1.IsolateService/GetMetrics

# Terminate sandbox
grpcurl -plaintext \
  -d '{"sandbox_id": "abc123"}' \
  localhost:50051 isolate.v1.IsolateService/TerminateSandbox
```

### Using Python

```python
import grpc
from isolate_pb2 import *
from isolate_pb2_grpc import IsolateServiceStub

# Connect
channel = grpc.insecure_channel('localhost:50051')
stub = IsolateServiceStub(channel)

# Read WASM module
with open('module.wasm', 'rb') as f:
    wasm_bytes = f.read()

# Create sandbox
create_resp = stub.CreateSandbox(CreateSandboxRequest(
    module=wasm_bytes,
    config=SandboxConfig(
        memory_limit=128 * 1024 * 1024,
        fuel_limit=1_000_000,
        capabilities=[
            Capability(type='stdout'),
            Capability(type='stderr'),
        ],
        env={'API_KEY': 'secret'},
    )
))
print(f"Created sandbox: {create_resp.sandbox_id}")

# Run sandbox
run_resp = stub.RunSandbox(RunSandboxRequest(
    sandbox_id=create_resp.sandbox_id,
    input=b'{"data": [1,2,3]}'
))
print(f"Exit code: {run_resp.exit_code}")
print(f"Output: {run_resp.stdout.decode()}")

# Terminate
stub.TerminateSandbox(TerminateSandboxRequest(
    sandbox_id=create_resp.sandbox_id
))
```

### Using Rust

```rust
use tonic::transport::Channel;

mod proto {
    tonic::include_proto!("isolate.v1");
}

use proto::isolate_service_client::IsolateServiceClient;
use proto::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_static("http://localhost:50051")
        .connect()
        .await?;

    let mut client = IsolateServiceClient::new(channel);

    // Create sandbox
    let wasm_bytes = std::fs::read("module.wasm")?;
    let create_resp = client.create_sandbox(CreateSandboxRequest {
        module: wasm_bytes,
        config: Some(SandboxConfig {
            memory_limit: 128 * 1024 * 1024,
            fuel_limit: 1_000_000,
            capabilities: vec![
                Capability { r#type: "stdout".into(), value: "".into() },
                Capability { r#type: "stderr".into(), value: "".into() },
            ],
            ..Default::default()
        }),
    }).await?;

    let sandbox_id = create_resp.into_inner().sandbox_id;
    println!("Created: {}", sandbox_id);

    // Run
    let run_resp = client.run_sandbox(RunSandboxRequest {
        sandbox_id: sandbox_id.clone(),
        input: vec![],
        entry_point: "_start".into(),
    }).await?;

    let output = run_resp.into_inner();
    println!("Exit: {}", output.exit_code);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
```

## Deployment

### Docker

```dockerfile
FROM rust:1.75 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --package isolate-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/isolate-server /usr/local/bin/
EXPOSE 50051
CMD ["isolate-server", "--addr", "0.0.0.0:50051"]
```

```bash
# Build and run
docker build -t isolate-server .
docker run -p 50051:50051 isolate-server
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isolate-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: isolate-server
  template:
    metadata:
      labels:
        app: isolate-server
    spec:
      containers:
      - name: isolate-server
        image: isolate-server:latest
        ports:
        - containerPort: 50051
        args:
        - --addr=0.0.0.0:50051
        - --max-sandboxes=100
        - --json-logs
        resources:
          requests:
            memory: "256Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
---
apiVersion: v1
kind: Service
metadata:
  name: isolate-server
spec:
  selector:
    app: isolate-server
  ports:
  - port: 50051
    targetPort: 50051
```

## Monitoring

### Prometheus Metrics

The server exposes metrics via the `GetMetrics` RPC:

```bash
# Get Prometheus format metrics
grpcurl -plaintext -d '{"format":"prometheus"}' \
  localhost:50051 isolate.v1.IsolateService/GetMetrics
```

Available metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `isolate_sandboxes_created_total` | Counter | Total sandboxes created |
| `isolate_sandboxes_active` | Gauge | Currently active sandboxes |
| `isolate_sandbox_run_duration_seconds` | Histogram | Execution duration |
| `isolate_sandbox_cold_start_seconds` | Histogram | Creation time |
| `isolate_capability_checks_total` | Counter | Capability checks by type |
| `isolate_capability_denials_total` | Counter | Denied capability requests |

### Health Checks

```bash
# Use grpc-health-probe
grpc-health-probe -addr=localhost:50051

# Or check with grpcurl
grpcurl -plaintext localhost:50051 grpc.health.v1.Health/Check
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    gRPC Server                       │
│                  (tonic/tokio)                       │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────┼────────────────────────────┐
│                        │                             │
│   ┌────────────────────┴────────────────────────┐   │
│   │            IsolateServiceImpl                │   │
│   │                                              │   │
│   │  ┌──────────┐  ┌──────────┐  ┌──────────┐   │   │
│   │  │ Semaphore│  │ DashMap  │  │WasmEngine│   │   │
│   │  │ (limits) │  │(sandboxes│  │ (shared) │   │   │
│   │  └──────────┘  └──────────┘  └──────────┘   │   │
│   └─────────────────────────────────────────────┘   │
│                                                      │
│                   isolate-server                     │
└──────────────────────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────┐
│                    isolate-core                      │
│        (Sandbox, Config, Capabilities, etc.)         │
└──────────────────────────────────────────────────────┘
```

## Security Considerations

1. **TLS**: Use TLS in production for encrypted connections
2. **Authentication**: Implement gRPC interceptors for auth
3. **Rate Limiting**: Set appropriate `--max-sandboxes` limit
4. **Resource Limits**: Configure memory/CPU limits per sandbox
5. **Network Policies**: Restrict network access in Kubernetes

## License

MIT OR Apache-2.0
