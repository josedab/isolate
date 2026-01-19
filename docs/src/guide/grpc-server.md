# gRPC Server

The `isolate-server` provides remote sandbox management via gRPC.

## Installation

```bash
cargo install isolate-server
```

## Starting the Server

```bash
isolate-server --addr 0.0.0.0:50051
```

### Options

```bash
isolate-server \
    --addr 0.0.0.0:50051 \
    --metrics-addr 0.0.0.0:9090 \
    --max-sandboxes 100 \
    --default-memory-limit 128M \
    --default-timeout 30s
```

## API Overview

### Service Definition

```protobuf
service SandboxService {
    // Create a new sandbox
    rpc CreateSandbox(CreateSandboxRequest) returns (CreateSandboxResponse);

    // Run a sandbox
    rpc RunSandbox(RunSandboxRequest) returns (RunSandboxResponse);

    // Get sandbox status
    rpc GetSandbox(GetSandboxRequest) returns (GetSandboxResponse);

    // Terminate a sandbox
    rpc TerminateSandbox(TerminateSandboxRequest) returns (TerminateSandboxResponse);

    // Stream sandbox output
    rpc StreamOutput(StreamOutputRequest) returns (stream OutputChunk);
}
```

## Client Usage

### Rust Client

```rust
use isolate_client::SandboxClient;

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = SandboxClient::connect("http://localhost:50051").await?;

    // Create a sandbox
    let response = client.create_sandbox(CreateSandboxRequest {
        wasm_bytes: wasm_module.to_vec(),
        config: Some(SandboxConfig {
            memory_limit: 128 * 1024 * 1024,
            fuel: Some(1_000_000),
            capabilities: vec![
                Capability::Stdout,
                Capability::Stderr,
            ],
            ..Default::default()
        }),
    }).await?;

    let sandbox_id = response.sandbox_id;

    // Run the sandbox
    let output = client.run_sandbox(RunSandboxRequest {
        sandbox_id: sandbox_id.clone(),
        input: b"input data".to_vec(),
    }).await?;

    println!("Exit code: {}", output.exit_code);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));

    // Terminate
    client.terminate_sandbox(TerminateSandboxRequest {
        sandbox_id,
    }).await?;

    Ok(())
}
```

### Python Client

```python
import grpc
from isolate_pb2 import CreateSandboxRequest, RunSandboxRequest
from isolate_pb2_grpc import SandboxServiceStub

channel = grpc.insecure_channel('localhost:50051')
client = SandboxServiceStub(channel)

# Create sandbox
response = client.CreateSandbox(CreateSandboxRequest(
    wasm_bytes=wasm_module,
    config={'memory_limit': 128 * 1024 * 1024}
))
sandbox_id = response.sandbox_id

# Run sandbox
output = client.RunSandbox(RunSandboxRequest(
    sandbox_id=sandbox_id,
    input=b'input data'
))

print(f"Exit code: {output.exit_code}")
print(f"Stdout: {output.stdout.decode()}")
```

### cURL (via gRPC-Web)

```bash
curl -X POST http://localhost:50051/isolate.SandboxService/CreateSandbox \
    -H "Content-Type: application/json" \
    -d '{"wasm_bytes": "AGFzbQEAAAA=", "config": {"memory_limit": 134217728}}'
```

## Configuration

### Server Config File

```toml
# isolate-server.toml

[server]
addr = "0.0.0.0:50051"
max_connections = 1000

[metrics]
enabled = true
addr = "0.0.0.0:9090"

[limits]
max_sandboxes = 100
max_wasm_size = "10MB"
default_memory_limit = "128MB"
default_timeout = "30s"

[pool]
enabled = true
min_size = 10
max_size = 50
ttl = "5m"

[tls]
enabled = false
cert_path = "/etc/isolate/cert.pem"
key_path = "/etc/isolate/key.pem"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `ISOLATE_SERVER_ADDR` | Server bind address |
| `ISOLATE_METRICS_ADDR` | Metrics bind address |
| `ISOLATE_MAX_SANDBOXES` | Maximum concurrent sandboxes |
| `ISOLATE_LOG` | Log level |

## Docker Deployment

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --package isolate-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/isolate-server /usr/local/bin/
EXPOSE 50051 9090
CMD ["isolate-server", "--addr", "0.0.0.0:50051"]
```

```yaml
# docker-compose.yml
version: '3.8'
services:
  isolate:
    build: .
    ports:
      - "50051:50051"
      - "9090:9090"
    environment:
      - ISOLATE_LOG=info
      - ISOLATE_MAX_SANDBOXES=100
```

## Load Balancing

For high availability, deploy multiple instances behind a load balancer:

```yaml
# kubernetes deployment
apiVersion: apps/v1
kind: Deployment
metadata:
  name: isolate-server
spec:
  replicas: 3
  template:
    spec:
      containers:
        - name: isolate
          image: isolate-server:latest
          ports:
            - containerPort: 50051
          resources:
            limits:
              memory: "2Gi"
              cpu: "2"
```

## Health Checks

### gRPC Health Check

```bash
grpcurl -plaintext localhost:50051 grpc.health.v1.Health/Check
```

### HTTP Health Endpoints

```
GET /health/live   -> 200 OK
GET /health/ready  -> 200 OK / 503 Service Unavailable
```

## See Also

- [CLI Usage](./cli.md) - Local execution
- [Monitoring](./monitoring.md) - Metrics and tracing
- [Resource Limits](./resource-limits.md) - Server-side limits
