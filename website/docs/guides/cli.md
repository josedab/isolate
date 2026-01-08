---
sidebar_position: 5
---

# CLI Usage

The `isolate` CLI allows you to run WASM modules from the command line with full capability and resource control.

## Installation

```bash
cargo install isolate-cli
```

## Basic Usage

### Running a Module

```bash
isolate run module.wasm
```

### With Capabilities

```bash
isolate run module.wasm \
    --cap-stdout \
    --cap-stderr \
    --cap-fs-read /data \
    --cap-http api.example.com
```

### With Resource Limits

```bash
isolate run module.wasm \
    --memory-limit 128M \
    --fuel 1000000 \
    --timeout 30s
```

### With Input

```bash
# From stdin
echo "input data" | isolate run processor.wasm --cap-stdin

# From file
isolate run processor.wasm --input data.txt
```

## Command Reference

### `isolate run`

Run a WASM module in a sandbox.

```
USAGE:
    isolate run [OPTIONS] <MODULE>

ARGS:
    <MODULE>    Path to the WASM module

OPTIONS:
    -h, --help                       Print help information
    -V, --version                    Print version information

CAPABILITIES:
    --cap-stdout                     Allow writing to stdout
    --cap-stderr                     Allow writing to stderr
    --cap-stdin                      Allow reading from stdin
    --cap-fs-read <PATH>             Allow reading from PATH
    --cap-fs-write <PATH>            Allow writing to PATH
    --cap-http <HOST>                Allow HTTP requests to HOST
    --cap-clock                      Allow clock access
    --cap-random                     Allow random number generation
    --cap-env <VAR>                  Allow reading environment variable

RESOURCE LIMITS:
    --memory-limit <SIZE>            Memory limit (e.g., 128M, 1G)
    --stack-size <SIZE>              Stack size limit
    --fuel <N>                       Instruction fuel limit
    --timeout <DURATION>             Wall clock timeout (e.g., 30s, 5m)
    --io-read-limit <SIZE>           I/O read limit
    --io-write-limit <SIZE>          I/O write limit

ENVIRONMENT:
    --env <KEY=VALUE>                Set environment variable
    --arg <ARG>                      Pass argument to module

OUTPUT:
    --json                           Output results as JSON
    --quiet                          Suppress non-error output
    --verbose                        Enable verbose output
```

### `isolate inspect`

Inspect a WASM module without running it.

```bash
isolate inspect module.wasm
```

Output:

```
Module: module.wasm
Size: 1.2 MB
Hash: sha256:abc123...

Exports:
  - _start (function)
  - memory (memory)
  - process (function)

Imports:
  - wasi_snapshot_preview1::fd_write
  - wasi_snapshot_preview1::clock_time_get
  - wasi_snapshot_preview1::random_get

Estimated capabilities needed:
  - stdout (fd_write)
  - clock (clock_time_get)
  - random (random_get)
```

### `isolate validate`

Validate a WASM module.

```bash
isolate validate module.wasm
```

### `isolate version`

Print version information.

```bash
isolate version
```

## Examples

### Simple Execution

```bash
# Run a hello world module
isolate run hello.wasm --cap-stdout
```

### Data Processing Pipeline

```bash
# Process JSON data
cat input.json | isolate run processor.wasm \
    --cap-stdin \
    --cap-stdout \
    --memory-limit 256M \
    --timeout 60s > output.json
```

### Sandboxed Script Execution

```bash
# Run untrusted code with strict limits
isolate run untrusted.wasm \
    --memory-limit 16M \
    --fuel 100000 \
    --timeout 100ms \
    --cap-stdout
```

### File Processing

```bash
# Process files in a directory
isolate run image-processor.wasm \
    --cap-fs-read /input \
    --cap-fs-write /output \
    --memory-limit 512M \
    --timeout 5m
```

### API Client

```bash
# Make HTTP requests
isolate run api-client.wasm \
    --cap-http api.example.com \
    --cap-stdout \
    --env API_KEY=$API_KEY \
    --cap-env API_KEY
```

## JSON Output

Use `--json` for machine-readable output:

```bash
isolate run module.wasm --cap-stdout --json
```

```json
{
  "exit_code": 0,
  "duration_ms": 123.45,
  "stdout": "Hello, World!\n",
  "stderr": "",
  "resource_usage": {
    "fuel_consumed": 50000,
    "memory_peak_bytes": 1048576,
    "io_read_bytes": 0,
    "io_write_bytes": 14
  }
}
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (module exit code 0) |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Module not found |
| 4 | Module validation failed |
| 5 | Capability denied |
| 6 | Resource limit exceeded |
| 7 | Timeout |
| 100+ | Module exit code + 100 |

## Configuration File

Create `~/.config/isolate/config.toml` for default settings:

```toml
[defaults]
memory_limit = "128M"
timeout = "30s"
fuel = 10000000

[capabilities]
# Default capabilities (applied unless --no-defaults)
stdout = true
stderr = true

[logging]
level = "info"
format = "json"
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `ISOLATE_LOG` | Log level (error, warn, info, debug, trace) |
| `ISOLATE_CONFIG` | Path to config file |
| `ISOLATE_NO_COLOR` | Disable colored output |

## See Also

- [Quick Start](../getting-started/quick-start) - Getting started with Isolate
- [Capabilities](./capabilities) - Understanding capabilities
- [Resource Limits](./resource-limits) - Setting resource limits
