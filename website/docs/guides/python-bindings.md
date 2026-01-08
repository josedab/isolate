---
sidebar_position: 8
---

# Python Bindings

Isolate provides Python bindings through the `isolate-python` crate, allowing Python developers to execute untrusted WebAssembly code safely.

:::warning Experimental
Python bindings are currently experimental. The API may change in future releases.
:::

## Installation

### From PyPI (when available)

```bash
pip install isolate
```

### From Source

```bash
# Clone the repository
git clone https://github.com/josedab/isolate.git
cd isolate/isolate-python

# Install maturin (build tool for Rust Python extensions)
pip install maturin

# Build and install
maturin develop --release
```

## Quick Start

```python
import isolate

# Load a WASM module
config = isolate.SandboxConfig.builder() \
    .module_from_file("hello.wasm") \
    .memory_limit(64 * 1024 * 1024) \
    .capability(isolate.Capability.stdout()) \
    .build()

# Create and run the sandbox
sandbox = isolate.Sandbox.create(config)
output = sandbox.run()

print(f"Exit code: {output.exit_code}")
print(f"Output: {output.stdout_str()}")
```

## API Reference

### Capability

Capabilities grant specific permissions to a sandbox.

```python
# Standard I/O
isolate.Capability.stdout()      # Write to stdout
isolate.Capability.stderr()      # Write to stderr
isolate.Capability.stdin()       # Read from stdin

# Filesystem
isolate.Capability.filesystem_read("/data")   # Read access
isolate.Capability.filesystem_write("/tmp")   # Write access
isolate.Capability.temp_dir()                 # Temp directory

# Environment
isolate.Capability.env_var("API_KEY")    # Single variable
isolate.Capability.env_all()              # All variables

# Time
isolate.Capability.system_clock()     # Wall clock time
isolate.Capability.monotonic_clock()  # Duration measurement
isolate.Capability.timers()           # Sleep and intervals

# Random
isolate.Capability.secure_random()    # Cryptographic random
isolate.Capability.seeded_random(42)  # Deterministic random

# Network
isolate.Capability.http_client(["api.example.com"])  # HTTP access
```

### SandboxConfig

Configuration is built using the builder pattern.

```python
config = isolate.SandboxConfig.builder() \
    # WASM module (required)
    .module(wasm_bytes)              # From bytes
    .module_from_file("module.wasm") # From file

    # Resource limits
    .memory_limit(128 * 1024 * 1024) # 128MB
    .fuel(10_000_000)                # CPU fuel
    .cpu_time_limit(30.0)            # 30 seconds

    # Capabilities
    .capability(isolate.Capability.stdout())
    .capability(isolate.Capability.filesystem_read("/data"))

    # Environment
    .env("KEY", "value")             # Single variable
    .envs({"K1": "v1", "K2": "v2"})  # Multiple variables

    # Arguments
    .arg("--verbose")
    .args(["--input", "data.json"])

    .build()
```

### Sandbox

The sandbox executes WASM code.

```python
# Create a sandbox
sandbox = isolate.Sandbox.create(config)

# Properties
print(sandbox.id)     # Unique identifier
print(sandbox.state)  # Current state (Ready, Running, Terminated)

# Run the sandbox
output = sandbox.run()           # No input
output = sandbox.run(b"input")   # With input bytes

# Terminate explicitly (optional, happens automatically)
sandbox.terminate()
```

### Output

Results from sandbox execution.

```python
output = sandbox.run()

# Properties
output.exit_code      # int: Exit code (0 = success)
output.stdout         # bytes: Raw stdout
output.stderr         # bytes: Raw stderr
output.duration_secs  # float: Execution time
output.fuel_consumed  # int: Fuel used

# Methods
output.stdout_str()   # str: Stdout as UTF-8
output.stderr_str()   # str: Stderr as UTF-8
output.is_success()   # bool: Exit code == 0
```

## Convenience Functions

For simple use cases, use the helper functions:

```python
import isolate

# Run WASM bytes
output = isolate.run_wasm(
    wasm_bytes,
    memory_limit=64 * 1024 * 1024,
    fuel=1_000_000,
    stdin=b"input data",
    env={"KEY": "value"}
)

# Run WASM file
output = isolate.run_wasm_file(
    "module.wasm",
    memory_limit=64 * 1024 * 1024,
    fuel=1_000_000
)
```

## Examples

### Running a Data Processor

```python
import isolate
import json

def process_data(wasm_path: str, data: dict) -> dict:
    """Run a WASM data processor with JSON input/output."""
    config = isolate.SandboxConfig.builder() \
        .module_from_file(wasm_path) \
        .memory_limit(256 * 1024 * 1024) \
        .fuel(100_000_000) \
        .capability(isolate.Capability.stdout()) \
        .capability(isolate.Capability.stdin()) \
        .build()

    sandbox = isolate.Sandbox.create(config)
    input_bytes = json.dumps(data).encode('utf-8')
    output = sandbox.run(input_bytes)

    if not output.is_success():
        raise RuntimeError(f"Processing failed: {output.stderr_str()}")

    return json.loads(output.stdout_str())

# Usage
result = process_data("processor.wasm", {"values": [1, 2, 3]})
```

### Plugin System

```python
import isolate
from pathlib import Path

class PluginRunner:
    """Run untrusted plugins in sandboxed WASM."""

    def __init__(self, plugin_dir: Path):
        self.plugin_dir = plugin_dir
        self.default_capabilities = [
            isolate.Capability.stdout(),
            isolate.Capability.stderr(),
        ]

    def run_plugin(self, name: str, input_data: bytes = b"") -> isolate.Output:
        wasm_path = self.plugin_dir / f"{name}.wasm"
        if not wasm_path.exists():
            raise FileNotFoundError(f"Plugin not found: {name}")

        builder = isolate.SandboxConfig.builder() \
            .module_from_file(str(wasm_path)) \
            .memory_limit(64 * 1024 * 1024) \
            .fuel(10_000_000)

        for cap in self.default_capabilities:
            builder = builder.capability(cap)

        config = builder.build()
        sandbox = isolate.Sandbox.create(config)
        return sandbox.run(input_data)

# Usage
runner = PluginRunner(Path("./plugins"))
output = runner.run_plugin("my-plugin", b"hello")
print(output.stdout_str())
```

### Error Handling

```python
import isolate

try:
    config = isolate.SandboxConfig.builder() \
        .module_from_file("module.wasm") \
        .fuel(1000)  # Very low fuel limit
        .capability(isolate.Capability.stdout()) \
        .build()

    sandbox = isolate.Sandbox.create(config)
    output = sandbox.run()

except ValueError as e:
    # Configuration errors (invalid WASM, missing module)
    print(f"Configuration error: {e}")

except RuntimeError as e:
    # Execution errors (fuel exhausted, timeout, capability denied)
    print(f"Execution error: {e}")
```

## Best Practices

### 1. Always Set Resource Limits

```python
# Don't rely on defaults
config = isolate.SandboxConfig.builder() \
    .module_from_file("module.wasm") \
    .memory_limit(64 * 1024 * 1024)  # Explicit limit
    .fuel(10_000_000)                 # Explicit limit
    .build()
```

### 2. Grant Minimal Capabilities

```python
# Only grant what's needed
config = isolate.SandboxConfig.builder() \
    .module_from_file("module.wasm") \
    .capability(isolate.Capability.stdout())  # Only stdout
    # Don't add unnecessary capabilities
    .build()
```

### 3. Handle Errors Gracefully

```python
def safe_run(wasm_path: str) -> tuple[bool, str]:
    """Run WASM with proper error handling."""
    try:
        config = isolate.SandboxConfig.builder() \
            .module_from_file(wasm_path) \
            .memory_limit(64 * 1024 * 1024) \
            .fuel(10_000_000) \
            .capability(isolate.Capability.stdout()) \
            .build()

        sandbox = isolate.Sandbox.create(config)
        output = sandbox.run()

        if output.is_success():
            return True, output.stdout_str()
        else:
            return False, output.stderr_str()

    except (ValueError, RuntimeError) as e:
        return False, str(e)
```

### 4. Check Version Compatibility

```python
import isolate

print(f"Isolate version: {isolate.version()}")
```

## Limitations

- Python bindings are synchronous (blocking) - async support is planned
- Error messages may be less detailed than the Rust API
- Some advanced features (snapshots, custom host functions) are not yet exposed
- The sandbox is consumed after `run()` - create a new one for each execution

## See Also

- [Quick Start](../getting-started/quick-start) - Getting started with Isolate
- [Capabilities](./capabilities) - Understanding the capability system
- [Resource Limits](./resource-limits) - Configuring resource controls
