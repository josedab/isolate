---
sidebar_position: 6
---

# gRPC Server

The `isolate-server` provides a gRPC API for remote sandbox management, suitable for building serverless platforms or distributed systems.

## Installation

```bash
cargo install isolate-server
```

## Starting the Server

```bash
isolate-server --addr 0.0.0.0:50051
```

### Options

```
USAGE:
    isolate-server [OPTIONS]

OPTIONS:
    --addr <ADDR>           Listen address [default: 0.0.0.0:50051]
    --tls-cert <PATH>       TLS certificate file
    --tls-key <PATH>        TLS key file
    --max-sandboxes <N>     Maximum concurrent sandboxes [default: 100]
    --metrics-addr <ADDR>   Prometheus metrics address [default: 0.0.0.0:9090]
    --log-level <LEVEL>     Log level [default: info]
    --config <PATH>         Configuration file path
```

## API Reference

### Service Definition

```protobuf
syntax = "proto3";

package isolate.v1;

service SandboxService {
  // Create a new sandbox
  rpc CreateSandbox(CreateSandboxRequest) returns (CreateSandboxResponse);

  // Run a sandbox
  rpc RunSandbox(RunSandboxRequest) returns (RunSandboxResponse);

  // Stream sandbox output
  rpc StreamOutput(StreamOutputRequest) returns (stream OutputChunk);

  // Get sandbox status
  rpc GetSandbox(GetSandboxRequest) returns (GetSandboxResponse);

  // List sandboxes
  rpc ListSandboxes(ListSandboxesRequest) returns (ListSandboxesResponse);

  // Terminate a sandbox
  rpc TerminateSandbox(TerminateSandboxRequest) returns (TerminateSandboxResponse);
}
```

### CreateSandbox

Create a new sandbox without running it.

```protobuf
message CreateSandboxRequest {
  bytes wasm_module = 1;
  ResourceLimits limits = 2;
  repeated Capability capabilities = 3;
  map<string, string> env = 4;
  repeated string args = 5;
}

message CreateSandboxResponse {
  string sandbox_id = 1;
  string module_hash = 2;
  int64 creation_time_ms = 3;
}
```

### RunSandbox

Run a sandbox (creates if needed, or runs existing).

```protobuf
message RunSandboxRequest {
  oneof target {
    string sandbox_id = 1;
    bytes wasm_module = 2;
  }
  bytes input = 3;
  ResourceLimits limits = 4;
  repeated Capability capabilities = 5;
}

message RunSandboxResponse {
  string sandbox_id = 1;
  int32 exit_code = 2;
  bytes stdout = 3;
  bytes stderr = 4;
  int64 duration_ms = 5;
  ResourceUsage resource_usage = 6;
}
```

## Client Examples

### Rust

```rust
use isolate_client::SandboxServiceClient;
use tonic::transport::Channel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_static("http://localhost:50051")
        .connect()
        .await?;

    let mut client = SandboxServiceClient::new(channel);

    let request = tonic::Request::new(RunSandboxRequest {
        target: Some(Target::WasmModule(wasm_bytes)),
        input: b"hello".to_vec(),
        limits: Some(ResourceLimits {
            memory_bytes: 128 * 1024 * 1024,
            fuel: 1_000_000,
            timeout_ms: 30_000,
        }),
        capabilities: vec![
            Capability { kind: "stdout".to_string(), ..Default::default() },
        ],
    });

    let response = client.run_sandbox(request).await?;
    let output = response.into_inner();

    println!("Exit code: {}", output.exit_code);
    println!("Stdout: {}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
```

### Python

```python
import grpc
from isolate.v1 import sandbox_pb2, sandbox_pb2_grpc

channel = grpc.insecure_channel('localhost:50051')
stub = sandbox_pb2_grpc.SandboxServiceStub(channel)

# Load WASM module
with open('module.wasm', 'rb') as f:
    wasm_bytes = f.read()

# Run sandbox
request = sandbox_pb2.RunSandboxRequest(
    wasm_module=wasm_bytes,
    input=b'hello',
    limits=sandbox_pb2.ResourceLimits(
        memory_bytes=128 * 1024 * 1024,
        fuel=1_000_000,
        timeout_ms=30_000,
    ),
    capabilities=[
        sandbox_pb2.Capability(kind='stdout'),
    ],
)

response = stub.RunSandbox(request)

print(f"Exit code: {response.exit_code}")
print(f"Stdout: {response.stdout.decode()}")
```

### Go

```go
package main

import (
    "context"
    "log"

    "google.golang.org/grpc"
    pb "github.com/josedab/isolate/api/v1"
)

func main() {
    conn, err := grpc.Dial("localhost:50051", grpc.WithInsecure())
    if err != nil {
        log.Fatal(err)
    }
    defer conn.Close()

    client := pb.NewSandboxServiceClient(conn)

    resp, err := client.RunSandbox(context.Background(), &pb.RunSandboxRequest{
        Target: &pb.RunSandboxRequest_WasmModule{
            WasmModule: wasmBytes,
        },
        Input: []byte("hello"),
        Limits: &pb.ResourceLimits{
            MemoryBytes: 128 * 1024 * 1024,
            Fuel:        1_000_000,
            TimeoutMs:   30_000,
        },
        Capabilities: []*pb.Capability{
            {Kind: "stdout"},
        },
    })
    if err != nil {
        log.Fatal(err)
    }

    log.Printf("Exit code: %d", resp.ExitCode)
    log.Printf("Stdout: %s", string(resp.Stdout))
}
```

## TLS Configuration

### Generate Certificates

```bash
# Generate CA
openssl genrsa -out ca.key 4096
openssl req -new -x509 -key ca.key -out ca.crt -days 365

# Generate server certificate
openssl genrsa -out server.key 4096
openssl req -new -key server.key -out server.csr
openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 365
```

### Start with TLS

```bash
isolate-server \
    --addr 0.0.0.0:50051 \
    --tls-cert server.crt \
    --tls-key server.key
```

## Docker Deployment

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --package isolate-server

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/isolate-server /usr/local/bin/
EXPOSE 50051 9090
ENTRYPOINT ["isolate-server"]
```

```bash
docker build -t isolate-server .
docker run -p 50051:50051 -p 9090:9090 isolate-server
```

## Kubernetes Deployment

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
              name: grpc
            - containerPort: 9090
              name: metrics
          resources:
            requests:
              memory: "512Mi"
              cpu: "500m"
            limits:
              memory: "2Gi"
              cpu: "2000m"
          livenessProbe:
            grpc:
              port: 50051
            initialDelaySeconds: 5
          readinessProbe:
            grpc:
              port: 50051
            initialDelaySeconds: 5
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
      name: grpc
    - port: 9090
      name: metrics
```

## See Also

- [Monitoring](./monitoring) - Server metrics and observability
- [Security Model](./security-model) - Authentication and authorization
- [Configuration](../reference/configuration) - Server configuration options
