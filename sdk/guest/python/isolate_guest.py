"""Isolate Guest SDK for Python.

Provides helpers for writing WASM modules that run inside Isolate sandboxes.
Handles JSON I/O protocol, environment access, and structured logging.
"""

import json
import os
import sys
from typing import Any, Callable, Optional


def read_input() -> dict:
    """Read and parse JSON input from stdin.

    Returns:
        Parsed JSON data as a dictionary.

    Raises:
        ValueError: If stdin does not contain valid JSON.
    """
    raw = sys.stdin.buffer.read()
    if not raw:
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError as e:
        raise ValueError(f"Invalid JSON input: {e}") from e


def write_output(data: Any) -> None:
    """Serialize data as JSON and write to stdout.

    Args:
        data: Any JSON-serializable value.
    """
    output = json.dumps(data)
    sys.stdout.write(output + "\n")
    sys.stdout.flush()


def get_env(name: str) -> Optional[str]:
    """Get an environment variable by name.

    Args:
        name: The environment variable name.

    Returns:
        The variable value, or None if not set or not permitted.
    """
    return os.environ.get(name)


def log_info(msg: str) -> None:
    """Log an informational message to stderr."""
    print(f"[INFO] {msg}", file=sys.stderr, flush=True)


def log_warn(msg: str) -> None:
    """Log a warning message to stderr."""
    print(f"[WARN] {msg}", file=sys.stderr, flush=True)


def log_error(msg: str) -> None:
    """Log an error message to stderr."""
    print(f"[ERROR] {msg}", file=sys.stderr, flush=True)


def guest_main(func: Callable[[dict], Any]) -> Callable[[], None]:
    """Decorator that wraps a function with Isolate JSON I/O protocol.

    The decorated function receives parsed JSON input from stdin and should
    return a JSON-serializable value that will be written to stdout.
    Exceptions are caught, logged, and cause exit code 1.

    Example::

        @guest_main
        def main(input_data: dict) -> dict:
            return {"greeting": f"Hello, {input_data['name']}!"}
    """

    def wrapper() -> None:
        try:
            input_data = read_input()
            result = func(input_data)
            write_output(result)
        except Exception as e:
            log_error(str(e))
            sys.exit(1)

    # Auto-invoke when used as a script entry point
    wrapper()
    return wrapper
