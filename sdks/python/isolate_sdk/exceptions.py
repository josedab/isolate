"""Custom exception hierarchy for the Isolate Python SDK.

All exceptions raised by the SDK inherit from :class:`IsolateError`, making
it straightforward to catch any SDK-related error with a single except clause.
gRPC status codes are mapped to specific exception subclasses so that callers
can handle individual failure modes without inspecting error messages.
"""

from __future__ import annotations

from typing import Optional


class IsolateError(Exception):
    """Base exception for all Isolate SDK errors.

    Attributes:
        message: Human-readable description of the error.
        details: Optional additional context (e.g. the gRPC status details).
    """

    def __init__(self, message: str, details: Optional[str] = None) -> None:
        self.message = message
        self.details = details
        super().__init__(message)

    def __repr__(self) -> str:
        cls = type(self).__name__
        if self.details:
            return f"{cls}({self.message!r}, details={self.details!r})"
        return f"{cls}({self.message!r})"


# ---------------------------------------------------------------------------
# Connection & transport errors
# ---------------------------------------------------------------------------


class ConnectionError(IsolateError):
    """Raised when the client cannot reach the Isolate server.

    This typically means the server is down, the address is wrong, or a
    network partition occurred.  Corresponds to gRPC ``UNAVAILABLE``.
    """


class TimeoutError(IsolateError):
    """Raised when an RPC exceeds the configured deadline.

    Corresponds to gRPC ``DEADLINE_EXCEEDED``.
    """


# ---------------------------------------------------------------------------
# Client-side / validation errors
# ---------------------------------------------------------------------------


class InvalidArgumentError(IsolateError):
    """Raised when the server rejects a request due to invalid input.

    Examples include supplying an invalid WASM module, missing required
    fields, or out-of-range configuration values.  Corresponds to gRPC
    ``INVALID_ARGUMENT``.
    """


class NotFoundError(IsolateError):
    """Raised when the requested sandbox does not exist.

    Corresponds to gRPC ``NOT_FOUND``.
    """


# ---------------------------------------------------------------------------
# Server-side / execution errors
# ---------------------------------------------------------------------------


class PermissionDeniedError(IsolateError):
    """Raised when a capability check fails.

    The WASM module attempted an operation for which no capability was
    granted.  Corresponds to gRPC ``PERMISSION_DENIED``.
    """


class ResourceExhaustedError(IsolateError):
    """Raised when the sandbox exceeds a resource limit.

    This can be triggered by exceeding memory, fuel (instruction count),
    I/O byte limits, or the maximum number of concurrent sandboxes.
    Corresponds to gRPC ``RESOURCE_EXHAUSTED``.
    """


class SandboxExecutionError(IsolateError):
    """Raised when the sandbox execution fails for an unspecified reason.

    Corresponds to gRPC ``INTERNAL`` when the failure originates inside
    the sandbox runtime.
    """


class AlreadyExistsError(IsolateError):
    """Raised when attempting to create a resource that already exists.

    Corresponds to gRPC ``ALREADY_EXISTS``.
    """


class UnauthenticatedError(IsolateError):
    """Raised when authentication credentials are missing or invalid.

    Corresponds to gRPC ``UNAUTHENTICATED``.
    """


class ServerError(IsolateError):
    """Raised for unexpected server-side errors.

    Catches gRPC ``INTERNAL``, ``UNKNOWN``, ``UNIMPLEMENTED``, and other
    codes that do not map to a more specific exception.
    """
