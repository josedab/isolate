# Isolate Python Bindings

Python bindings for [Isolate](https://github.com/example/isolate) - a secure WebAssembly sandbox runtime.

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

## License

MIT OR Apache-2.0
