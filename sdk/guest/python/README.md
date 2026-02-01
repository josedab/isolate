# Isolate Guest SDK — Python

Write WASM modules for Isolate sandboxes in Python.

## Quick Start

### Prerequisites

Python guest modules require a Python-to-WASM compiler. Options include:

- [componentize-py](https://github.com/bytecodealliance/componentize-py) (recommended)
- [py2wasm](https://github.com/aspect-build/rules_py)

### Project Setup

Copy `isolate_guest.py` into your project directory.

### Writing a Guest Module

```python
from isolate_guest import guest_main, log_info

@guest_main
def main(input_data: dict) -> dict:
    log_info(f"Processing request for {input_data.get('name')}")

    return {
        "greeting": f"Hello, {input_data['name']}!"
    }
```

### Building

```bash
componentize-py -d ../wit/isolate-guest.wit -w isolate-guest \
    componentize my_module -o my_module.wasm
```

### Running

```bash
echo '{"name": "World"}' | isolate run \
    --capability stdout \
    --capability stdin \
    my_module.wasm
# Output: {"greeting":"Hello, World!"}
```

## API Reference

### `read_input() -> dict`

Reads and parses JSON input from stdin.

### `write_output(data) -> None`

Serializes the value as JSON and writes it to stdout.

### `get_env(name: str) -> Optional[str]`

Returns the value of an environment variable, or `None` if not set.

### `log_info(msg)`, `log_warn(msg)`, `log_error(msg)`

Write structured log messages to stderr.

### `@guest_main`

Decorator that wraps a function with JSON I/O protocol handling:

```python
@guest_main
def main(input_data: dict) -> dict:
    return {"result": "ok"}
```

The decorated function receives parsed JSON input and should return a
JSON-serializable value. Exceptions are caught, logged to stderr, and
cause the process to exit with code 1.

## Notes

- Python WASM support is evolving. Check toolchain documentation for the
  latest compilation instructions.
- For best compatibility, avoid native C extensions — use pure-Python
  libraries when possible.
