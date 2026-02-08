"""Isolate Python SDK -- a client library for the Isolate gRPC service.

This package provides both synchronous and asynchronous clients for
interacting with an Isolate sandbox server.

Quick start (synchronous)::

    from isolate_sdk import IsolateClient, SandboxConfig, Capability

    with IsolateClient("localhost:50051") as client:
        resp = client.create_sandbox(
            module=open("module.wasm", "rb").read(),
            config=SandboxConfig(
                memory_limit=64 * 1024 * 1024,
                capabilities=[Capability.stdout()],
            ),
        )
        result = client.run_sandbox(resp.sandbox_id)
        print(result.stdout.decode())

Quick start (asynchronous)::

    from isolate_sdk import AsyncIsolateClient, SandboxConfig, Capability

    async with AsyncIsolateClient("localhost:50051") as client:
        resp = await client.create_sandbox(
            module=open("module.wasm", "rb").read(),
            config=SandboxConfig(
                memory_limit=64 * 1024 * 1024,
                capabilities=[Capability.stdout()],
            ),
        )
        result = await client.run_sandbox(resp.sandbox_id)
        print(result.stdout.decode())
"""

from __future__ import annotations

__version__ = "0.1.0"

# Client classes
from isolate_sdk.client import AsyncIsolateClient, IsolateClient

# Exception hierarchy
from isolate_sdk.exceptions import (
    AlreadyExistsError,
    ConnectionError,
    InvalidArgumentError,
    IsolateError,
    NotFoundError,
    PermissionDeniedError,
    ResourceExhaustedError,
    SandboxExecutionError,
    ServerError,
    TimeoutError,
    UnauthenticatedError,
)

# Data models
from isolate_sdk.models import (
    Capability,
    CreateSandboxRequest,
    CreateSandboxResponse,
    GetMetricsRequest,
    GetMetricsResponse,
    GetSandboxRequest,
    GetSandboxResponse,
    ListSandboxesRequest,
    ListSandboxesResponse,
    OutputChunk,
    ResourceUsage,
    RunSandboxRequest,
    RunSandboxResponse,
    SandboxConfig,
    SandboxInfo,
    SandboxMetrics,
    StreamOutputRequest,
    TerminateSandboxRequest,
    TerminateSandboxResponse,
)

__all__ = [
    # Version
    "__version__",
    # Clients
    "IsolateClient",
    "AsyncIsolateClient",
    # Exceptions
    "IsolateError",
    "ConnectionError",
    "TimeoutError",
    "InvalidArgumentError",
    "NotFoundError",
    "PermissionDeniedError",
    "ResourceExhaustedError",
    "SandboxExecutionError",
    "AlreadyExistsError",
    "UnauthenticatedError",
    "ServerError",
    # Models -- configuration
    "Capability",
    "SandboxConfig",
    # Models -- responses & shared types
    "CreateSandboxResponse",
    "RunSandboxResponse",
    "GetSandboxResponse",
    "TerminateSandboxResponse",
    "ListSandboxesResponse",
    "GetMetricsResponse",
    "OutputChunk",
    "ResourceUsage",
    "SandboxInfo",
    "SandboxMetrics",
    # Models -- request types (advanced usage)
    "CreateSandboxRequest",
    "RunSandboxRequest",
    "GetSandboxRequest",
    "TerminateSandboxRequest",
    "ListSandboxesRequest",
    "GetMetricsRequest",
    "StreamOutputRequest",
]
