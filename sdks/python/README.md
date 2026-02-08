# Isolate Python SDK

Python client library for the [Isolate](https://github.com/josedab/isolate) gRPC sandbox service.

## Requirements

- Python 3.9+
- A running Isolate gRPC server

## Installation

```bash
pip install isolate-sdk
```

To generate protobuf stubs from the `.proto` file (only needed during development):

```bash
pip install isolate-sdk[codegen]
```

## Generating Protobuf Stubs

Before using the SDK you must generate the Python gRPC stubs from the proto definition:

```bash
python -m grpc_tools.protoc \
    -I../../proto \
    --python_out=. \
    --grpc_python_out=. \
    ../../proto/isolate.proto
```

This produces the `isolate/v1/isolate_pb2.py` and `isolate/v1/isolate_pb2_grpc.py` files that the SDK imports at runtime.

## Quick Start

### Synchronous Client

```python
from isolate_sdk import IsolateClient, SandboxConfig, Capability

with IsolateClient("localhost:50051") as client:
    # Load a WASM module
    with open("module.wasm", "rb") as f:
        wasm_bytes = f.read()

    # Create a sandbox
    create_resp = client.create_sandbox(
        module=wasm_bytes,
        config=SandboxConfig(
            memory_limit=64 * 1024 * 1024,  # 64 MB
            fuel_limit=10_000_000,
            capabilities=[
                Capability.stdout(),
                Capability.stderr(),
            ],
        ),
    )
    print(f"Sandbox ID: {create_resp.sandbox_id}")

    # Run the sandbox
    run_resp = client.run_sandbox(create_resp.sandbox_id)
    print(f"Exit code: {run_resp.exit_code}")
    print(f"Output: {run_resp.stdout.decode()}")

    # Terminate when done
    term_resp = client.terminate_sandbox(create_resp.sandbox_id)
    print(f"Total runs: {term_resp.metrics.run_count}")
```

### Asynchronous Client

```python
import asyncio
from isolate_sdk import AsyncIsolateClient, SandboxConfig, Capability

async def main():
    async with AsyncIsolateClient("localhost:50051") as client:
        with open("module.wasm", "rb") as f:
            wasm_bytes = f.read()

        create_resp = await client.create_sandbox(
            module=wasm_bytes,
            config=SandboxConfig(
                memory_limit=64 * 1024 * 1024,
                capabilities=[Capability.stdout()],
            ),
        )

        result = await client.run_sandbox(create_resp.sandbox_id)
        print(f"Exit code: {result.exit_code}")
        print(f"Output: {result.stdout.decode()}")

asyncio.run(main())
```

## Client Configuration

### Basic Connection

```python
client = IsolateClient("localhost:50051")
```

### With TLS

```python
client = IsolateClient(
    "isolate.example.com:50051",
    tls=True,
)
```

### With Mutual TLS (mTLS)

```python
client = IsolateClient(
    "isolate.example.com:50051",
    tls=True,
    root_certificates=open("ca.crt", "rb").read(),
    private_key=open("client.key", "rb").read(),
    certificate_chain=open("client.crt", "rb").read(),
)
```

### Timeout and Retry Settings

```python
client = IsolateClient(
    "localhost:50051",
    timeout=60.0,        # Per-RPC timeout in seconds
    max_retries=5,       # Retry transient failures up to 5 times
    retry_backoff=1.0,   # Initial backoff of 1s, doubles each attempt
)
```

### Custom Metadata

```python
client = IsolateClient(
    "localhost:50051",
    metadata={"authorization": "Bearer <token>"},
)
```

## API Reference

### Sandbox Lifecycle

| Method | Description |
|--------|-------------|
| `create_sandbox(module, config)` | Create a new sandbox from WASM bytes |
| `run_sandbox(sandbox_id, *, input, entry_point)` | Run an existing sandbox |
| `get_sandbox(sandbox_id)` | Query sandbox status and info |
| `terminate_sandbox(sandbox_id)` | Terminate and clean up a sandbox |
| `list_sandboxes(*, state_filter, limit, offset)` | List sandboxes with optional filters |
| `get_metrics(format)` | Retrieve server metrics |
| `stream_output(sandbox_id)` | Stream live output from a sandbox |

All methods accept an optional `timeout` keyword argument to override the client default.

### Capabilities

Capabilities are created via class methods on `Capability`:

```python
Capability.stdout()              # Write to stdout
Capability.stderr()              # Write to stderr
Capability.stdin()               # Read from stdin
Capability.fs_read("/data")      # Read files under /data
Capability.fs_write("/tmp/out")  # Write files under /tmp/out
Capability.temp_dir()            # Temporary directory access
Capability.http("example.com")   # HTTP access to host
Capability.dns()                 # DNS resolution
Capability.system_clock()        # Wall clock access
Capability.monotonic_clock()     # Monotonic clock access
Capability.random()              # Cryptographic randomness
Capability.env("API_KEY")        # Environment variable access
```

### Resource Limits

Configure via `SandboxConfig`:

```python
config = SandboxConfig(
    memory_limit=128 * 1024 * 1024,  # 128 MB heap
    fuel_limit=50_000_000,            # ~50M instructions
    wall_time_limit_secs=60,          # 60 second wall-clock timeout
    cpu_time_limit_secs=30,           # 30 second CPU time limit
)
```

## Error Handling

All exceptions inherit from `IsolateError`:

```python
from isolate_sdk import (
    IsolateClient,
    IsolateError,
    InvalidArgumentError,
    NotFoundError,
    ResourceExhaustedError,
    TimeoutError,
    ConnectionError,
    PermissionDeniedError,
)

with IsolateClient("localhost:50051") as client:
    try:
        result = client.run_sandbox("nonexistent-id")
    except NotFoundError:
        print("Sandbox does not exist")
    except ResourceExhaustedError:
        print("Resource limit exceeded")
    except TimeoutError:
        print("Execution timed out")
    except ConnectionError:
        print("Cannot reach server")
    except PermissionDeniedError:
        print("Missing required capability")
    except IsolateError as exc:
        print(f"Unexpected error: {exc}")
```

### Exception Hierarchy

```
IsolateError
  +-- ConnectionError          (UNAVAILABLE)
  +-- TimeoutError             (DEADLINE_EXCEEDED)
  +-- InvalidArgumentError     (INVALID_ARGUMENT)
  +-- NotFoundError            (NOT_FOUND)
  +-- PermissionDeniedError    (PERMISSION_DENIED)
  +-- ResourceExhaustedError   (RESOURCE_EXHAUSTED)
  +-- AlreadyExistsError       (ALREADY_EXISTS)
  +-- UnauthenticatedError     (UNAUTHENTICATED)
  +-- SandboxExecutionError    (INTERNAL - sandbox runtime)
  +-- ServerError              (INTERNAL / UNKNOWN / other)
```

## Streaming Output

Stream stdout/stderr from a running sandbox:

```python
# Synchronous
for chunk in client.stream_output(sandbox_id, follow_stdout=True, follow_stderr=True):
    print(f"[{chunk.stream}] {chunk.data.decode()}", end="")

# Asynchronous
async for chunk in client.stream_output(sandbox_id):
    print(f"[{chunk.stream}] {chunk.data.decode()}", end="")
```

## Complete Example

```python
from isolate_sdk import (
    IsolateClient,
    SandboxConfig,
    Capability,
    IsolateError,
)

def process_data(wasm_path: str, input_data: bytes) -> bytes:
    """Run a WASM module with input and return its stdout."""
    with IsolateClient("localhost:50051", timeout=60.0) as client:
        with open(wasm_path, "rb") as f:
            wasm_bytes = f.read()

        create_resp = client.create_sandbox(
            module=wasm_bytes,
            config=SandboxConfig(
                memory_limit=64 * 1024 * 1024,
                fuel_limit=10_000_000,
                wall_time_limit_secs=30,
                capabilities=[
                    Capability.stdout(),
                    Capability.stderr(),
                    Capability.fs_read("/data"),
                    Capability.system_clock(),
                ],
                env={"LOG_LEVEL": "info"},
                args=["--format=json"],
            ),
        )

        try:
            result = client.run_sandbox(
                create_resp.sandbox_id,
                input=input_data,
            )

            if result.exit_code != 0:
                raise RuntimeError(
                    f"Module exited with code {result.exit_code}: "
                    f"{result.stderr.decode()}"
                )

            if result.resource_usage:
                print(f"Peak memory: {result.resource_usage.peak_memory} bytes")
                print(f"Fuel consumed: {result.resource_usage.fuel_consumed}")
                print(f"Duration: {result.duration_ms:.1f} ms")

            return result.stdout
        finally:
            client.terminate_sandbox(create_resp.sandbox_id)
```

## See Also

- [gRPC Server Documentation](../../website/docs/guides/grpc-server.md)
- [Go SDK](../../website/docs/guides/sdk-go.md)
- [TypeScript SDK](../../website/docs/guides/sdk-typescript.md)
