# isolate-python

> **🔌 In-Process Embedding** — These are native Python bindings (via PyO3) that run Isolate directly in your Python process with no server needed.
> For a **remote gRPC client** that connects to an Isolate server, see [`sdk/python/`](../sdk/python/) instead.

[![PyPI](https://img.shields.io/pypi/v/isolate-sandbox.svg)](https://pypi.org/project/isolate-sandbox/)
[![License](https://img.shields.io/pypi/l/isolate-sandbox.svg)](../LICENSE-MIT)
[![Python](https://img.shields.io/pypi/pyversions/isolate-sandbox.svg)](https://pypi.org/project/isolate-sandbox/)

Python bindings for [Isolate](https://github.com/josedab/isolate) - a secure WebAssembly sandbox runtime. Execute untrusted WASM code with capability-based security and resource controls.

## Installation

```bash
pip install isolate-sandbox
```

## Quick Start

```python
import isolate

# Simple: Run a WASM file directly
output = isolate.run_wasm_file("hello.wasm")
print(output.stdout_str())

# Advanced: Configure sandbox with fine-grained control
config = isolate.SandboxConfig.builder() \
    .module_from_file("module.wasm") \
    .memory_limit(128 * 1024 * 1024)  \
    .fuel(1_000_000) \
    .capability(isolate.Capability.stdout()) \
    .capability(isolate.Capability.stderr()) \
    .capability(isolate.Capability.filesystem_read("/data")) \
    .env("API_KEY", "secret") \
    .build()

sandbox = isolate.Sandbox.create(config)
output = sandbox.run()

print(f"Exit code: {output.exit_code}")
print(f"Output: {output.stdout_str()}")
print(f"Duration: {output.duration_secs}s")
print(f"Fuel consumed: {output.fuel_consumed}")
```

## Features

- **Secure Isolation**: Execute untrusted WebAssembly code safely
- **Capability-Based Security**: Fine-grained permission control
- **Resource Limits**: Control CPU, memory, and I/O usage
- **Fast Execution**: Sub-5ms cold start times
- **Full Type Hints**: Complete type annotations for IDE support

## Capabilities

Control what the sandbox can access:

```python
# Standard I/O
isolate.Capability.stdout()
isolate.Capability.stderr()
isolate.Capability.stdin()

# Filesystem access
isolate.Capability.filesystem_read("/data")
isolate.Capability.filesystem_write("/output")
isolate.Capability.temp_dir()

# Environment
isolate.Capability.env_all()
isolate.Capability.env_var("API_KEY")

# Time and random
isolate.Capability.system_clock()
isolate.Capability.monotonic_clock()
isolate.Capability.timers()
isolate.Capability.secure_random()
isolate.Capability.seeded_random(12345)

# Network
isolate.Capability.http_client(["api.example.com", "*.trusted.com"])
```

## Resource Limits

```python
config = isolate.SandboxConfig.builder() \
    .module(wasm_bytes) \
    .memory_limit(64 * 1024 * 1024)  # 64MB max memory
    .fuel(1_000_000)  # Instruction count limit
    .cpu_time_limit(30.0)  # 30 second timeout
    .build()
```

## Passing Input

```python
# Pass stdin data to the sandbox
output = sandbox.run(b"input data here")

# Or use the convenience function
output = isolate.run_wasm(wasm_bytes, stdin=b"input data")
```

## Error Handling

```python
try:
    config = isolate.SandboxConfig.builder() \
        .module_from_file("nonexistent.wasm") \
        .build()
except RuntimeError as e:
    print(f"Failed to load module: {e}")

try:
    sandbox = isolate.Sandbox.create(config)
    output = sandbox.run()
except RuntimeError as e:
    print(f"Execution failed: {e}")
```

## Building from Source

Requires:
- Python 3.8+
- Rust 1.70+
- maturin

```bash
# Install maturin
pip install maturin

# Build and install in development mode
cd isolate-python
maturin develop

# Build wheel for distribution
maturin build --release
```

## API Reference

### Module Functions

#### `isolate.run_wasm(wasm_bytes, memory_limit=None, fuel=None, stdin=None, env=None)`

Run WASM bytes with basic configuration.

```python
output = isolate.run_wasm(
    wasm_bytes,
    memory_limit=64 * 1024 * 1024,
    fuel=1_000_000,
    stdin=b"input data",
    env={"KEY": "value"}
)
```

#### `isolate.run_wasm_file(path, memory_limit=None, fuel=None, stdin=None, env=None)`

Run a WASM file with basic configuration.

```python
output = isolate.run_wasm_file("module.wasm")
```

#### `isolate.version()`

Get the library version.

```python
print(isolate.version())  # "0.1.0"
```

### Classes

#### `isolate.SandboxConfig`

Configuration for sandbox creation.

```python
# Create via builder pattern
config = isolate.SandboxConfig.builder() \
    .module(wasm_bytes) \           # or .module_from_file("path.wasm")
    .memory_limit(128 * 1024 * 1024) \
    .fuel(1_000_000) \
    .cpu_time_limit(30.0) \
    .capability(isolate.Capability.stdout()) \
    .env("KEY", "value") \
    .envs({"A": "1", "B": "2"}) \
    .arg("--verbose") \
    .args(["--config", "prod"]) \
    .build()
```

#### `isolate.Sandbox`

A WebAssembly sandbox instance.

```python
# Create sandbox
sandbox = isolate.Sandbox.create(config)

# Properties
print(sandbox.id)      # UUID string
print(sandbox.state)   # "Ready", "Running", "Terminated"

# Run (consumes sandbox)
output = sandbox.run()           # No input
output = sandbox.run(b"input")   # With input

# Terminate without running
sandbox.terminate()
```

#### `isolate.Output`

Result from sandbox execution.

```python
output = sandbox.run()

# Properties
output.exit_code      # int: Exit code (0 = success)
output.stdout         # bytes: Raw stdout
output.stderr         # bytes: Raw stderr
output.duration_secs  # float: Execution time
output.fuel_consumed  # int: CPU fuel used

# Methods
output.stdout_str()   # str: Stdout as UTF-8 string
output.stderr_str()   # str: Stderr as UTF-8 string
output.is_success()   # bool: True if exit_code == 0
```

#### `isolate.Capability`

Permission grant for sandbox operations.

```python
# I/O
Capability.stdout()
Capability.stderr()
Capability.stdin()

# Filesystem
Capability.filesystem_read("/path")
Capability.filesystem_write("/path")
Capability.temp_dir()

# Environment
Capability.env_var("NAME")
Capability.env_all()

# Time
Capability.system_clock()
Capability.monotonic_clock()
Capability.timers()

# Random
Capability.secure_random()
Capability.seeded_random(12345)

# Network
Capability.http_client(["api.example.com"])
```

## Advanced Examples

### Data Processing Pipeline

```python
import json
import isolate

# Process JSON through WASM transformer
input_data = json.dumps({"numbers": [1, 2, 3, 4, 5]}).encode()

config = isolate.SandboxConfig.builder() \
    .module_from_file("transform.wasm") \
    .capability(isolate.Capability.stdout()) \
    .capability(isolate.Capability.stdin()) \
    .build()

sandbox = isolate.Sandbox.create(config)
output = sandbox.run(input_data)

result = json.loads(output.stdout_str())
print(result)  # {"sum": 15, "count": 5}
```

### Batch Processing

```python
import isolate
from concurrent.futures import ThreadPoolExecutor

def process_item(item):
    config = isolate.SandboxConfig.builder() \
        .module_from_file("processor.wasm") \
        .capability(isolate.Capability.stdout()) \
        .memory_limit(32 * 1024 * 1024) \
        .fuel(100_000) \
        .build()

    sandbox = isolate.Sandbox.create(config)
    return sandbox.run(item.encode())

items = ["item1", "item2", "item3"]
with ThreadPoolExecutor(max_workers=4) as executor:
    results = list(executor.map(process_item, items))
```

### Plugin System

```python
import isolate
from pathlib import Path

class PluginManager:
    def __init__(self, plugin_dir: str):
        self.plugin_dir = Path(plugin_dir)

    def run_plugin(self, name: str, data: bytes) -> bytes:
        plugin_path = self.plugin_dir / f"{name}.wasm"

        config = isolate.SandboxConfig.builder() \
            .module_from_file(str(plugin_path)) \
            .capability(isolate.Capability.stdout()) \
            .capability(isolate.Capability.stdin()) \
            .memory_limit(64 * 1024 * 1024) \
            .fuel(1_000_000) \
            .cpu_time_limit(10.0) \
            .build()

        sandbox = isolate.Sandbox.create(config)
        output = sandbox.run(data)

        if not output.is_success():
            raise RuntimeError(f"Plugin failed: {output.stderr_str()}")

        return output.stdout

# Usage
manager = PluginManager("/plugins")
result = manager.run_plugin("validator", b'{"email": "test@example.com"}')
```

## Type Hints

Full type annotations are provided for IDE support:

```python
from typing import Optional

def run_wasm(
    wasm_bytes: bytes,
    memory_limit: Optional[int] = None,
    fuel: Optional[int] = None,
    stdin: Optional[bytes] = None,
    env: Optional[dict[str, str]] = None
) -> Output: ...

class Output:
    exit_code: int
    duration_secs: float
    fuel_consumed: int

    @property
    def stdout(self) -> bytes: ...
    @property
    def stderr(self) -> bytes: ...
    def stdout_str(self) -> str: ...
    def stderr_str(self) -> str: ...
    def is_success(self) -> bool: ...
```

## License

MIT OR Apache-2.0
