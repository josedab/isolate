"""Isolate Guest SDK for Python.

Provides helpers for writing WASM modules that run inside Isolate sandboxes.
Handles JSON I/O protocol, environment access, and structured logging.

Typical usage::

    from isolate_guest import guest_main, log_info

    @guest_main
    def main(input_data: dict) -> dict:
        log_info("Processing request")
        return {"result": "ok"}
"""

import json
import os
import sys
from typing import Any, Callable, Dict, List, Optional, Tuple, Type, TypeVar

T = TypeVar("T")


# ---------------------------------------------------------------------------
# Error types
# ---------------------------------------------------------------------------


class GuestError(Exception):
    """Error raised by guest SDK operations.

    Wraps JSON parsing errors, I/O errors, and user-level errors with
    a consistent interface.
    """

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message

    def __str__(self) -> str:
        return f"guest error: {self.message}"


# ---------------------------------------------------------------------------
# Input
# ---------------------------------------------------------------------------


def read_input() -> dict:
    """Read and parse JSON input from stdin.

    Returns:
        Parsed JSON data as a dictionary. Returns ``{}`` if stdin is empty.

    Raises:
        GuestError: If stdin does not contain valid JSON.
    """
    raw = sys.stdin.buffer.read()
    if not raw:
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError as e:
        raise GuestError(f"Invalid JSON input: {e}") from e


def read_input_as(cls: Type[T]) -> T:
    """Read JSON input and validate it has expected keys.

    This is a typed convenience that reads JSON and returns it as-is.
    For real dataclass mapping, combine with a library like ``dacite``.

    Args:
        cls: Expected type (used for documentation; returns plain dict).

    Returns:
        Parsed JSON data.

    Raises:
        GuestError: If stdin does not contain valid JSON.
    """
    return read_input()  # type: ignore[return-value]


def read_raw() -> bytes:
    """Read raw bytes from stdin without JSON parsing.

    Returns:
        The raw bytes from stdin.
    """
    return sys.stdin.buffer.read()


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------


def write_output(data: Any) -> None:
    """Serialize data as JSON and write to stdout.

    Args:
        data: Any JSON-serializable value.

    Raises:
        GuestError: If serialization fails.
    """
    try:
        output = json.dumps(data)
    except (TypeError, ValueError) as e:
        raise GuestError(f"JSON serialization error: {e}") from e
    sys.stdout.write(output + "\n")
    sys.stdout.flush()


def write_raw(data: bytes) -> None:
    """Write raw bytes to stdout without JSON encoding.

    Args:
        data: Raw bytes to write.
    """
    sys.stdout.buffer.write(data)
    sys.stdout.buffer.flush()


# ---------------------------------------------------------------------------
# Environment access
# ---------------------------------------------------------------------------


def get_env(name: str) -> Optional[str]:
    """Get an environment variable by name.

    Args:
        name: The environment variable name.

    Returns:
        The variable value, or ``None`` if not set or not permitted.
    """
    return os.environ.get(name)


def get_all_env() -> Dict[str, str]:
    """Get all available environment variables.

    Returns:
        A dictionary of all environment variables accessible to this sandbox.
    """
    return dict(os.environ)


def get_args() -> List[str]:
    """Get command-line arguments passed to the sandbox.

    Returns:
        The argument list (``sys.argv``).
    """
    return list(sys.argv)


# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------


def log_debug(msg: str) -> None:
    """Log a debug message to stderr."""
    print(f"[DEBUG] {msg}", file=sys.stderr, flush=True)


def log_info(msg: str) -> None:
    """Log an informational message to stderr."""
    print(f"[INFO] {msg}", file=sys.stderr, flush=True)


def log_warn(msg: str) -> None:
    """Log a warning message to stderr."""
    print(f"[WARN] {msg}", file=sys.stderr, flush=True)


def log_error(msg: str) -> None:
    """Log an error message to stderr."""
    print(f"[ERROR] {msg}", file=sys.stderr, flush=True)


# ---------------------------------------------------------------------------
# Main entry point helper
# ---------------------------------------------------------------------------


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
        except GuestError as e:
            log_error(str(e))
            sys.exit(1)
        except Exception as e:
            log_error(f"Unhandled exception: {e}")
            sys.exit(1)

    # Auto-invoke when used as a script entry point
    wrapper()
    return wrapper
