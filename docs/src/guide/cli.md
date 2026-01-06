# CLI Usage

The `isolate` CLI provides a command-line interface for running and managing WASM sandboxes.

## Installation

```bash
cargo install isolate-cli
```

## Commands

### Run a Module

```bash
isolate run module.wasm
```

### Options

```bash
isolate run module.wasm \
    --cap-stdout \
    --cap-stderr \
    --cap-fs-read /data \
    --cap-fs-write /tmp/output \
    --cap-http api.example.com \
    --cap-env API_KEY \
    --env API_KEY=secret \
    --memory-limit 128M \
    --fuel 1000000 \
    --timeout 30s \
    --arg "--verbose" \
    --arg "--format=json"
```

### Input/Output

```bash
# Pipe input
echo "input data" | isolate run processor.wasm

# From file
isolate run processor.wasm < input.txt

# To file
isolate run processor.wasm > output.txt
```

### Validate a Module

```bash
isolate validate module.wasm
```

### Inspect a Module

```bash
isolate inspect module.wasm
```

Output:

```
Module: module.wasm
  Hash: a1b2c3d4e5f6...
  Size: 1.2 MB
  Exports:
    - _start (function)
    - memory (memory)
  Imports:
    - wasi_snapshot_preview1::fd_write
    - wasi_snapshot_preview1::clock_time_get
```

### Benchmark a Module

```bash
isolate bench module.wasm --iterations 100
```

Output:

```
Benchmark Results (100 iterations):
  Cold start (p50): 2.3ms
  Cold start (p99): 4.8ms
  Execution (p50): 15.2ms
  Execution (p99): 23.1ms
  Memory peak: 12.4 MB
```

## Configuration File

Create `.isolate.toml` for default settings:

```toml
# .isolate.toml

[defaults]
memory_limit = "128M"
fuel = 10_000_000
timeout = "30s"

[capabilities]
stdout = true
stderr = true
clock = true

[capabilities.filesystem]
read = ["/data", "/config"]
write = ["/tmp/output"]

[capabilities.http]
allowed_hosts = ["api.example.com"]

[capabilities.env]
allowed = ["API_KEY", "CONFIG_PATH"]
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `ISOLATE_LOG` | Log level (trace, debug, info, warn, error) |
| `ISOLATE_CONFIG` | Path to config file |
| `ISOLATE_METRICS_ADDR` | Prometheus metrics address |

## Examples

### Basic Execution

```bash
isolate run hello.wasm --cap-stdout
```

### With Resource Limits

```bash
isolate run compute.wasm \
    --memory-limit 64M \
    --fuel 100000 \
    --timeout 5s
```

### Filesystem Access

```bash
isolate run processor.wasm \
    --cap-fs-read /input \
    --cap-fs-write /output
```

### HTTP Client

```bash
isolate run api-client.wasm \
    --cap-http api.example.com \
    --cap-http cdn.example.com
```

### Full Example

```bash
isolate run my-service.wasm \
    --cap-stdout \
    --cap-stderr \
    --cap-fs-read /etc/myservice \
    --cap-http api.internal \
    --cap-env SERVICE_KEY \
    --env SERVICE_KEY="$SERVICE_KEY" \
    --memory-limit 256M \
    --fuel 50000000 \
    --timeout 60s \
    --arg "--config=/etc/myservice/config.json"
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | WASM module error |
| 2 | Configuration error |
| 3 | Resource limit exceeded |
| 4 | Capability denied |
| 5 | Timeout |
| 126 | Module not executable |
| 127 | Module not found |

## See Also

- [gRPC Server](./grpc-server.md) - Remote sandbox management
- [Resource Limits](./resource-limits.md) - Limit options
- [Capabilities](./capabilities.md) - Capability flags
