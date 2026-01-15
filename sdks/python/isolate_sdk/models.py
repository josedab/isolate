"""Data models for the Isolate Python SDK.

All request and response types are defined as :mod:`dataclasses` with full
type annotations.  They provide a Pythonic interface that is independent of
the underlying protobuf/gRPC transport.

Conversion helpers (:meth:`to_proto` / :func:`from_proto_*`) are kept as
module-level functions so that the public surface stays clean and free of
protobuf imports for consumers of the SDK.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Dict, Iterator, List, Optional


# ---------------------------------------------------------------------------
# Capability helpers
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Capability:
    """A single sandbox capability.

    Capabilities control which host resources a WASM module is allowed to
    access.  By default a sandbox has *no* capabilities.

    Attributes:
        type: The capability kind (e.g. ``"stdout"``, ``"fs_read"``).
        value: An optional scope such as a filesystem path or hostname.
    """

    type: str
    value: str = ""

    # -- Convenience constructors ------------------------------------------

    @classmethod
    def stdout(cls) -> Capability:
        """Allow the sandbox to write to stdout."""
        return cls(type="stdout")

    @classmethod
    def stderr(cls) -> Capability:
        """Allow the sandbox to write to stderr."""
        return cls(type="stderr")

    @classmethod
    def stdin(cls) -> Capability:
        """Allow the sandbox to read from stdin."""
        return cls(type="stdin")

    @classmethod
    def fs_read(cls, path: str) -> Capability:
        """Allow read access to *path* inside the sandbox."""
        return cls(type="fs_read", value=path)

    @classmethod
    def fs_write(cls, path: str) -> Capability:
        """Allow write access to *path* inside the sandbox."""
        return cls(type="fs_write", value=path)

    @classmethod
    def temp_dir(cls) -> Capability:
        """Grant access to a temporary directory."""
        return cls(type="temp_dir")

    @classmethod
    def http(cls, host: str) -> Capability:
        """Allow outbound HTTP requests to *host*.

        Wildcards (e.g. ``*.example.com``) are supported.
        """
        return cls(type="http", value=host)

    @classmethod
    def dns(cls) -> Capability:
        """Allow DNS resolution."""
        return cls(type="dns")

    @classmethod
    def system_clock(cls) -> Capability:
        """Allow access to the wall clock."""
        return cls(type="system_clock")

    @classmethod
    def monotonic_clock(cls) -> Capability:
        """Allow access to the monotonic clock."""
        return cls(type="monotonic_clock")

    @classmethod
    def random(cls) -> Capability:
        """Allow access to cryptographic randomness."""
        return cls(type="random")

    @classmethod
    def env(cls, name: str) -> Capability:
        """Allow reading the environment variable *name*."""
        return cls(type="env", value=name)


# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------


@dataclass
class SandboxConfig:
    """Configuration for a new sandbox.

    Attributes:
        memory_limit: Maximum heap memory in bytes (0 = server default).
        fuel_limit: Maximum instruction fuel (0 = unlimited).
        wall_time_limit_secs: Wall-clock timeout in seconds (0 = server default).
        cpu_time_limit_secs: CPU time limit in seconds (0 = server default).
        capabilities: Capabilities granted to the sandbox.
        env: Environment variables passed to the WASM module.
        args: Command-line arguments passed to the WASM module.
    """

    memory_limit: int = 0
    fuel_limit: int = 0
    wall_time_limit_secs: int = 0
    cpu_time_limit_secs: int = 0
    capabilities: List[Capability] = field(default_factory=list)
    env: Dict[str, str] = field(default_factory=dict)
    args: List[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Resource types
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class ResourceUsage:
    """Resource consumption recorded during sandbox execution.

    Attributes:
        peak_memory: Peak heap usage in bytes.
        fuel_consumed: Total fuel (instruction count) consumed.
        cpu_time_ms: CPU time in milliseconds.
        wall_time_ms: Wall-clock time in milliseconds.
        bytes_read: Total bytes read through I/O streams.
        bytes_written: Total bytes written through I/O streams.
    """

    peak_memory: int = 0
    fuel_consumed: int = 0
    cpu_time_ms: float = 0.0
    wall_time_ms: float = 0.0
    bytes_read: int = 0
    bytes_written: int = 0


@dataclass(frozen=True)
class SandboxMetrics:
    """Aggregate metrics for a sandbox across all runs.

    Attributes:
        run_count: Total number of runs.
        success_count: Runs that exited with code 0.
        failure_count: Runs that exited with a non-zero code.
        total_run_duration_ms: Sum of all run durations in milliseconds.
        last_run_duration_ms: Duration of the most recent run in milliseconds.
    """

    run_count: int = 0
    success_count: int = 0
    failure_count: int = 0
    total_run_duration_ms: float = 0.0
    last_run_duration_ms: float = 0.0


# ---------------------------------------------------------------------------
# Request models
# ---------------------------------------------------------------------------


@dataclass
class CreateSandboxRequest:
    """Parameters for creating a new sandbox.

    Attributes:
        module: Raw WASM module bytes.
        config: Sandbox configuration.
    """

    module: bytes
    config: Optional[SandboxConfig] = None


@dataclass
class RunSandboxRequest:
    """Parameters for running an existing sandbox.

    Attributes:
        sandbox_id: Identifier of the sandbox to run.
        input: Optional bytes fed to the module via stdin.
        entry_point: Function name to invoke (default ``"_start"``).
    """

    sandbox_id: str
    input: bytes = b""
    entry_point: str = "_start"


@dataclass
class GetSandboxRequest:
    """Parameters for querying sandbox status.

    Attributes:
        sandbox_id: Identifier of the sandbox to query.
    """

    sandbox_id: str


@dataclass
class TerminateSandboxRequest:
    """Parameters for terminating a sandbox.

    Attributes:
        sandbox_id: Identifier of the sandbox to terminate.
    """

    sandbox_id: str


@dataclass
class ListSandboxesRequest:
    """Parameters for listing sandboxes.

    Attributes:
        state_filter: If set, only sandboxes in this state are returned.
        limit: Maximum number of sandboxes to return (0 = no limit).
        offset: Pagination offset.
    """

    state_filter: str = ""
    limit: int = 0
    offset: int = 0


@dataclass
class GetMetricsRequest:
    """Parameters for retrieving server metrics.

    Attributes:
        format: Desired output format (``"prometheus"`` or ``"json"``).
    """

    format: str = "prometheus"


@dataclass
class StreamOutputRequest:
    """Parameters for streaming sandbox output.

    Attributes:
        sandbox_id: Identifier of the sandbox whose output to stream.
        follow_stdout: Whether to include stdout chunks.
        follow_stderr: Whether to include stderr chunks.
    """

    sandbox_id: str
    follow_stdout: bool = True
    follow_stderr: bool = True


# ---------------------------------------------------------------------------
# Response models
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CreateSandboxResponse:
    """Result of creating a new sandbox.

    Attributes:
        sandbox_id: Unique identifier assigned to the sandbox.
        module_hash: Hash of the uploaded WASM module.
        creation_time_ms: Time taken to create the sandbox in milliseconds.
    """

    sandbox_id: str
    module_hash: str
    creation_time_ms: float


@dataclass(frozen=True)
class RunSandboxResponse:
    """Result of running a sandbox.

    Attributes:
        exit_code: Process exit code (0 = success).
        stdout: Captured standard output bytes.
        stderr: Captured standard error bytes.
        duration_ms: Execution duration in milliseconds.
        resource_usage: Detailed resource consumption metrics.
    """

    exit_code: int
    stdout: bytes
    stderr: bytes
    duration_ms: float
    resource_usage: Optional[ResourceUsage] = None


@dataclass(frozen=True)
class GetSandboxResponse:
    """Result of querying a sandbox's status.

    Attributes:
        sandbox: Detailed sandbox information.
    """

    sandbox: SandboxInfo


@dataclass(frozen=True)
class TerminateSandboxResponse:
    """Result of terminating a sandbox.

    Attributes:
        terminated: Whether the sandbox was successfully terminated.
        metrics: Final aggregate metrics for the sandbox.
    """

    terminated: bool
    metrics: Optional[SandboxMetrics] = None


@dataclass(frozen=True)
class ListSandboxesResponse:
    """Result of listing sandboxes.

    Attributes:
        sandboxes: List of sandbox information entries.
        total: Total number of sandboxes matching the filter.
    """

    sandboxes: List[SandboxInfo]
    total: int


@dataclass(frozen=True)
class GetMetricsResponse:
    """Result of retrieving server metrics.

    Attributes:
        data: The metrics payload in the requested format.
    """

    data: str


@dataclass(frozen=True)
class OutputChunk:
    """A single chunk of streaming output.

    Attributes:
        stream: Which stream this chunk belongs to (``"stdout"`` or ``"stderr"``).
        data: The raw bytes of this chunk.
        timestamp: Server-side timestamp in Unix epoch milliseconds.
    """

    stream: str
    data: bytes
    timestamp: int


# ---------------------------------------------------------------------------
# Sandbox info (used in several responses)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SandboxInfo:
    """Detailed information about a sandbox.

    Attributes:
        id: Unique sandbox identifier.
        state: Current state (e.g. ``"ready"``, ``"running"``, ``"terminated"``).
        module_hash: Hash of the WASM module loaded into this sandbox.
        created_at: Creation timestamp in Unix epoch seconds.
        age_secs: Age of the sandbox in seconds.
        metrics: Aggregate run metrics.
    """

    id: str
    state: str
    module_hash: str
    created_at: int
    age_secs: float
    metrics: Optional[SandboxMetrics] = None
