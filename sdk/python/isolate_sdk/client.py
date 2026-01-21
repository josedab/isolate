"""Synchronous and asynchronous gRPC clients for the Isolate service.

This module provides two client classes:

* :class:`IsolateClient` -- a synchronous (blocking) client suitable for
  scripts, CLI tools, and synchronous web frameworks.
* :class:`AsyncIsolateClient` -- an ``async``/``await`` client built on
  :mod:`grpc.aio`, suitable for asyncio-based applications.

Both classes support context-manager protocols so that connections are
released deterministically even when exceptions occur.

Example (synchronous)::

    with IsolateClient("localhost:50051") as client:
        resp = client.create_sandbox(module=wasm_bytes)
        print(resp.sandbox_id)

Example (asynchronous)::

    async with AsyncIsolateClient("localhost:50051") as client:
        resp = await client.create_sandbox(module=wasm_bytes)
        print(resp.sandbox_id)
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass, field
from typing import (
    Any,
    AsyncIterator,
    Dict,
    Iterator,
    List,
    Optional,
    Sequence,
    Union,
)

import grpc
import grpc.aio

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
from isolate_sdk.models import (
    Capability,
    CreateSandboxResponse,
    GetMetricsResponse,
    GetSandboxResponse,
    ListSandboxesResponse,
    OutputChunk,
    ResourceUsage,
    RunSandboxResponse,
    SandboxConfig,
    SandboxInfo,
    SandboxMetrics,
    TerminateSandboxResponse,
)

# The generated protobuf / gRPC stubs are imported lazily inside the
# helper functions so that the rest of the SDK can be imported even when
# the generated code has not been installed.  When the stubs *are*
# available they are used directly; otherwise we fall back to building
# protobuf messages dynamically from the proto file.
#
# For simplicity, this SDK ships a thin shim that constructs the raw
# protobuf messages in-line -- see ``_proto`` helpers below.

logger = logging.getLogger("isolate_sdk")

# ---------------------------------------------------------------------------
# Client configuration
# ---------------------------------------------------------------------------

_DEFAULT_TIMEOUT: float = 30.0
_DEFAULT_MAX_RETRIES: int = 3
_DEFAULT_RETRY_BACKOFF: float = 0.5  # seconds, doubles each attempt
_DEFAULT_MAX_MESSAGE_LENGTH: int = 64 * 1024 * 1024  # 64 MiB

# gRPC status codes that are safe to retry automatically.
_RETRYABLE_STATUS_CODES = frozenset(
    {
        grpc.StatusCode.UNAVAILABLE,
        grpc.StatusCode.DEADLINE_EXCEEDED,
        grpc.StatusCode.ABORTED,
    }
)


@dataclass
class ClientConfig:
    """Shared configuration for both sync and async clients.

    Attributes:
        target: gRPC server address (e.g. ``"localhost:50051"``).
        timeout: Default per-RPC deadline in seconds.
        max_retries: Maximum number of automatic retry attempts for
            transient failures.
        retry_backoff: Initial back-off duration in seconds; doubles
            after each failed attempt.
        tls: Whether to use TLS.  When *True* with no explicit
            credentials, the system root certificates are used.
        root_certificates: PEM-encoded root CA certificate(s) for TLS
            verification.  Ignored when *tls* is ``False``.
        private_key: PEM-encoded client private key for mutual TLS.
        certificate_chain: PEM-encoded client certificate chain for
            mutual TLS.
        metadata: Extra gRPC metadata (headers) attached to every RPC.
        options: Additional gRPC channel options.
        max_message_length: Maximum inbound/outbound message size in bytes.
    """

    target: str = "localhost:50051"
    timeout: float = _DEFAULT_TIMEOUT
    max_retries: int = _DEFAULT_MAX_RETRIES
    retry_backoff: float = _DEFAULT_RETRY_BACKOFF
    tls: bool = False
    root_certificates: Optional[bytes] = None
    private_key: Optional[bytes] = None
    certificate_chain: Optional[bytes] = None
    metadata: Dict[str, str] = field(default_factory=dict)
    options: Dict[str, Any] = field(default_factory=dict)
    max_message_length: int = _DEFAULT_MAX_MESSAGE_LENGTH


# ---------------------------------------------------------------------------
# Protobuf conversion helpers
# ---------------------------------------------------------------------------
# These functions translate between the SDK's public dataclass models and
# the raw protobuf wire types defined in ``isolate/v1/isolate.proto``.
# We import from the grpc-generated stubs if available, otherwise fall
# back to a lightweight dynamic approach.

try:
    # Generated stubs -- preferred.
    from isolate.v1 import isolate_pb2, isolate_pb2_grpc  # type: ignore[import-untyped]

    _HAS_GENERATED_STUBS = True
except ImportError:
    _HAS_GENERATED_STUBS = False
    isolate_pb2 = None  # type: ignore[assignment]
    isolate_pb2_grpc = None  # type: ignore[assignment]


def _ensure_stubs() -> None:
    """Verify that protobuf stubs are importable.

    If the generated stubs were not found at import time we attempt a
    late import here so that users who install the stubs after the
    package has already been imported still succeed.
    """
    global _HAS_GENERATED_STUBS, isolate_pb2, isolate_pb2_grpc  # noqa: PLW0603

    if _HAS_GENERATED_STUBS:
        return

    try:
        from isolate.v1 import isolate_pb2 as _pb2  # type: ignore[import-untyped]
        from isolate.v1 import isolate_pb2_grpc as _pb2_grpc  # type: ignore[import-untyped]

        isolate_pb2 = _pb2
        isolate_pb2_grpc = _pb2_grpc
        _HAS_GENERATED_STUBS = True
    except ImportError as exc:
        raise IsolateError(
            "Generated protobuf stubs not found.  "
            "Run `python -m grpc_tools.protoc` to generate them, or "
            "install the 'isolate-sdk[codegen]' extra.",
            details=str(exc),
        ) from exc


# -- To-proto helpers ------------------------------------------------------


def _capability_to_proto(cap: Capability) -> Any:
    return isolate_pb2.Capability(type=cap.type, value=cap.value)


def _sandbox_config_to_proto(cfg: SandboxConfig) -> Any:
    return isolate_pb2.SandboxConfig(
        memory_limit=cfg.memory_limit,
        fuel_limit=cfg.fuel_limit,
        wall_time_limit_secs=cfg.wall_time_limit_secs,
        cpu_time_limit_secs=cfg.cpu_time_limit_secs,
        capabilities=[_capability_to_proto(c) for c in cfg.capabilities],
        env=cfg.env,
        args=cfg.args,
    )


# -- From-proto helpers ----------------------------------------------------


def _resource_usage_from_proto(proto: Any) -> Optional[ResourceUsage]:
    if proto is None:
        return None
    return ResourceUsage(
        peak_memory=proto.peak_memory,
        fuel_consumed=proto.fuel_consumed,
        cpu_time_ms=proto.cpu_time_ms,
        wall_time_ms=proto.wall_time_ms,
        bytes_read=proto.bytes_read,
        bytes_written=proto.bytes_written,
    )


def _sandbox_metrics_from_proto(proto: Any) -> Optional[SandboxMetrics]:
    if proto is None:
        return None
    return SandboxMetrics(
        run_count=proto.run_count,
        success_count=proto.success_count,
        failure_count=proto.failure_count,
        total_run_duration_ms=proto.total_run_duration_ms,
        last_run_duration_ms=proto.last_run_duration_ms,
    )


def _sandbox_info_from_proto(proto: Any) -> SandboxInfo:
    return SandboxInfo(
        id=proto.id,
        state=proto.state,
        module_hash=proto.module_hash,
        created_at=proto.created_at,
        age_secs=proto.age_secs,
        metrics=_sandbox_metrics_from_proto(
            proto.metrics if proto.HasField("metrics") else None
        ),
    )


def _output_chunk_from_proto(proto: Any) -> OutputChunk:
    return OutputChunk(
        stream=proto.stream,
        data=bytes(proto.data),
        timestamp=proto.timestamp,
    )


# ---------------------------------------------------------------------------
# Error mapping
# ---------------------------------------------------------------------------


def _grpc_error_to_exception(err: grpc.RpcError) -> IsolateError:
    """Convert a gRPC :class:`RpcError` to the appropriate SDK exception."""
    code = err.code()  # type: ignore[union-attr]
    details = err.details()  # type: ignore[union-attr]

    mapping = {
        grpc.StatusCode.INVALID_ARGUMENT: InvalidArgumentError,
        grpc.StatusCode.NOT_FOUND: NotFoundError,
        grpc.StatusCode.ALREADY_EXISTS: AlreadyExistsError,
        grpc.StatusCode.PERMISSION_DENIED: PermissionDeniedError,
        grpc.StatusCode.RESOURCE_EXHAUSTED: ResourceExhaustedError,
        grpc.StatusCode.DEADLINE_EXCEEDED: TimeoutError,
        grpc.StatusCode.UNAVAILABLE: ConnectionError,
        grpc.StatusCode.UNAUTHENTICATED: UnauthenticatedError,
    }

    exc_cls = mapping.get(code, ServerError)
    return exc_cls(
        message=details or str(err),
        details=f"gRPC status: {code.name}" if code else None,
    )


# ---------------------------------------------------------------------------
# Channel factories
# ---------------------------------------------------------------------------


def _channel_options(cfg: ClientConfig) -> List[tuple]:
    """Build the list of gRPC channel options from *cfg*."""
    opts: List[tuple] = [
        ("grpc.max_send_message_length", cfg.max_message_length),
        ("grpc.max_receive_message_length", cfg.max_message_length),
    ]
    for key, value in cfg.options.items():
        opts.append((key, value))
    return opts


def _make_channel_credentials(cfg: ClientConfig) -> grpc.ChannelCredentials:
    """Create TLS channel credentials from *cfg*."""
    return grpc.ssl_channel_credentials(
        root_certificates=cfg.root_certificates,
        private_key=cfg.private_key,
        certificate_chain=cfg.certificate_chain,
    )


def _make_sync_channel(cfg: ClientConfig) -> grpc.Channel:
    options = _channel_options(cfg)
    if cfg.tls:
        creds = _make_channel_credentials(cfg)
        return grpc.secure_channel(cfg.target, creds, options=options)
    return grpc.insecure_channel(cfg.target, options=options)


def _make_async_channel(cfg: ClientConfig) -> grpc.aio.Channel:
    options = _channel_options(cfg)
    if cfg.tls:
        creds = _make_channel_credentials(cfg)
        return grpc.aio.secure_channel(cfg.target, creds, options=options)
    return grpc.aio.insecure_channel(cfg.target, options=options)


def _make_metadata(cfg: ClientConfig) -> Optional[Sequence[tuple]]:
    """Return metadata tuples or ``None`` if empty."""
    if not cfg.metadata:
        return None
    return list(cfg.metadata.items())


# ===================================================================
# Synchronous client
# ===================================================================


class IsolateClient:
    """Synchronous (blocking) client for the Isolate gRPC service.

    The client manages a single gRPC channel internally.  Use it as a
    context manager to ensure the channel is closed on exit::

        with IsolateClient("localhost:50051") as client:
            result = client.create_sandbox(module=wasm_bytes)

    Parameters:
        target: gRPC server address.
        timeout: Default per-RPC timeout in seconds.
        max_retries: Maximum automatic retries for transient errors.
        retry_backoff: Initial retry back-off in seconds.
        tls: Enable TLS transport encryption.
        root_certificates: PEM root CA for TLS verification.
        private_key: PEM client key for mTLS.
        certificate_chain: PEM client certificate chain for mTLS.
        metadata: Extra gRPC metadata attached to every call.
        options: Additional gRPC channel options.
        max_message_length: Maximum gRPC message size in bytes.
    """

    def __init__(
        self,
        target: str = "localhost:50051",
        *,
        timeout: float = _DEFAULT_TIMEOUT,
        max_retries: int = _DEFAULT_MAX_RETRIES,
        retry_backoff: float = _DEFAULT_RETRY_BACKOFF,
        tls: bool = False,
        root_certificates: Optional[bytes] = None,
        private_key: Optional[bytes] = None,
        certificate_chain: Optional[bytes] = None,
        metadata: Optional[Dict[str, str]] = None,
        options: Optional[Dict[str, Any]] = None,
        max_message_length: int = _DEFAULT_MAX_MESSAGE_LENGTH,
    ) -> None:
        _ensure_stubs()

        self._config = ClientConfig(
            target=target,
            timeout=timeout,
            max_retries=max_retries,
            retry_backoff=retry_backoff,
            tls=tls,
            root_certificates=root_certificates,
            private_key=private_key,
            certificate_chain=certificate_chain,
            metadata=metadata or {},
            options=options or {},
            max_message_length=max_message_length,
        )

        self._channel: grpc.Channel = _make_sync_channel(self._config)
        self._stub = isolate_pb2_grpc.IsolateServiceStub(self._channel)
        self._closed = False

    # -- Context manager ---------------------------------------------------

    def __enter__(self) -> IsolateClient:
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    # -- Lifecycle ---------------------------------------------------------

    def close(self) -> None:
        """Close the underlying gRPC channel.

        It is safe to call this method more than once.
        """
        if not self._closed:
            self._channel.close()
            self._closed = True

    # -- RPC helpers -------------------------------------------------------

    def _call_with_retry(self, method: str, request: Any, timeout: Optional[float] = None) -> Any:
        """Invoke *method* on the stub with automatic retry for transient errors."""
        rpc_timeout = timeout if timeout is not None else self._config.timeout
        metadata = _make_metadata(self._config)
        stub_method = getattr(self._stub, method)
        last_exc: Optional[Exception] = None

        for attempt in range(1, self._config.max_retries + 1):
            try:
                return stub_method(
                    request,
                    timeout=rpc_timeout,
                    metadata=metadata,
                )
            except grpc.RpcError as exc:
                last_exc = exc
                code = exc.code()  # type: ignore[union-attr]
                if code not in _RETRYABLE_STATUS_CODES or attempt == self._config.max_retries:
                    raise _grpc_error_to_exception(exc) from exc
                backoff = self._config.retry_backoff * (2 ** (attempt - 1))
                logger.warning(
                    "Transient gRPC error (attempt %d/%d, code=%s). "
                    "Retrying in %.1fs ...",
                    attempt,
                    self._config.max_retries,
                    code.name,
                    backoff,
                )
                time.sleep(backoff)

        # Should be unreachable, but satisfy the type checker.
        assert last_exc is not None
        raise _grpc_error_to_exception(last_exc) from last_exc  # type: ignore[arg-type]

    # -- Public API --------------------------------------------------------

    def create_sandbox(
        self,
        module: bytes,
        config: Optional[SandboxConfig] = None,
        *,
        timeout: Optional[float] = None,
    ) -> CreateSandboxResponse:
        """Create a new sandbox from a WASM module.

        Args:
            module: Raw WASM module bytes.
            config: Optional sandbox configuration (limits, capabilities, etc.).
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`CreateSandboxResponse` with the assigned sandbox ID.

        Raises:
            InvalidArgumentError: If the WASM module is malformed.
            ResourceExhaustedError: If the server has reached its sandbox limit.
            ConnectionError: If the server is unreachable.
        """
        proto_config = _sandbox_config_to_proto(config) if config else None
        request = isolate_pb2.CreateSandboxRequest(
            module=module,
            config=proto_config,
        )
        resp = self._call_with_retry("CreateSandbox", request, timeout=timeout)
        return CreateSandboxResponse(
            sandbox_id=resp.sandbox_id,
            module_hash=resp.module_hash,
            creation_time_ms=resp.creation_time_ms,
        )

    def run_sandbox(
        self,
        sandbox_id: str,
        *,
        input: bytes = b"",
        entry_point: str = "_start",
        timeout: Optional[float] = None,
    ) -> RunSandboxResponse:
        """Run an existing sandbox.

        Args:
            sandbox_id: Identifier of the sandbox to run.
            input: Optional bytes fed to the sandbox via stdin.
            entry_point: WASM function to call (default ``"_start"``).
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`RunSandboxResponse` with exit code, captured output,
            and resource usage.

        Raises:
            NotFoundError: If *sandbox_id* does not exist.
            ResourceExhaustedError: If a resource limit was exceeded.
            TimeoutError: If execution exceeded the wall-time limit.
        """
        request = isolate_pb2.RunSandboxRequest(
            sandbox_id=sandbox_id,
            input=input,
            entry_point=entry_point,
        )
        resp = self._call_with_retry("RunSandbox", request, timeout=timeout)
        return RunSandboxResponse(
            exit_code=resp.exit_code,
            stdout=bytes(resp.stdout),
            stderr=bytes(resp.stderr),
            duration_ms=resp.duration_ms,
            resource_usage=_resource_usage_from_proto(
                resp.resource_usage if resp.HasField("resource_usage") else None
            ),
        )

    def get_sandbox(
        self,
        sandbox_id: str,
        *,
        timeout: Optional[float] = None,
    ) -> GetSandboxResponse:
        """Get the status and information for a sandbox.

        Args:
            sandbox_id: Identifier of the sandbox to query.
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`GetSandboxResponse` containing :class:`SandboxInfo`.

        Raises:
            NotFoundError: If *sandbox_id* does not exist.
        """
        request = isolate_pb2.GetSandboxRequest(sandbox_id=sandbox_id)
        resp = self._call_with_retry("GetSandbox", request, timeout=timeout)
        return GetSandboxResponse(
            sandbox=_sandbox_info_from_proto(resp.sandbox),
        )

    def terminate_sandbox(
        self,
        sandbox_id: str,
        *,
        timeout: Optional[float] = None,
    ) -> TerminateSandboxResponse:
        """Terminate a sandbox and release its resources.

        Args:
            sandbox_id: Identifier of the sandbox to terminate.
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`TerminateSandboxResponse` with final metrics.

        Raises:
            NotFoundError: If *sandbox_id* does not exist.
        """
        request = isolate_pb2.TerminateSandboxRequest(sandbox_id=sandbox_id)
        resp = self._call_with_retry("TerminateSandbox", request, timeout=timeout)
        return TerminateSandboxResponse(
            terminated=resp.terminated,
            metrics=_sandbox_metrics_from_proto(
                resp.metrics if resp.HasField("metrics") else None
            ),
        )

    def list_sandboxes(
        self,
        *,
        state_filter: str = "",
        limit: int = 0,
        offset: int = 0,
        timeout: Optional[float] = None,
    ) -> ListSandboxesResponse:
        """List sandboxes managed by the server.

        Args:
            state_filter: If set, only sandboxes in this state are returned.
            limit: Maximum number of results.
            offset: Pagination offset.
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`ListSandboxesResponse` with sandbox info and total count.
        """
        request = isolate_pb2.ListSandboxesRequest(
            state_filter=state_filter,
            limit=limit,
            offset=offset,
        )
        resp = self._call_with_retry("ListSandboxes", request, timeout=timeout)
        return ListSandboxesResponse(
            sandboxes=[_sandbox_info_from_proto(s) for s in resp.sandboxes],
            total=resp.total,
        )

    def get_metrics(
        self,
        format: str = "prometheus",
        *,
        timeout: Optional[float] = None,
    ) -> GetMetricsResponse:
        """Retrieve server metrics.

        Args:
            format: Desired format -- ``"prometheus"`` or ``"json"``.
            timeout: Per-call timeout override in seconds.

        Returns:
            A :class:`GetMetricsResponse` with the serialised metrics data.
        """
        request = isolate_pb2.GetMetricsRequest(format=format)
        resp = self._call_with_retry("GetMetrics", request, timeout=timeout)
        return GetMetricsResponse(data=resp.data)

    def stream_output(
        self,
        sandbox_id: str,
        *,
        follow_stdout: bool = True,
        follow_stderr: bool = True,
        timeout: Optional[float] = None,
    ) -> Iterator[OutputChunk]:
        """Stream live output from a running sandbox.

        This is a server-streaming RPC.  Iterate over the returned
        iterator to receive :class:`OutputChunk` objects as they arrive.

        Args:
            sandbox_id: Identifier of the sandbox.
            follow_stdout: Include stdout in the stream.
            follow_stderr: Include stderr in the stream.
            timeout: Per-call timeout override in seconds.

        Yields:
            :class:`OutputChunk` instances.

        Raises:
            NotFoundError: If *sandbox_id* does not exist.
        """
        rpc_timeout = timeout if timeout is not None else self._config.timeout
        metadata = _make_metadata(self._config)
        request = isolate_pb2.StreamOutputRequest(
            sandbox_id=sandbox_id,
            follow_stdout=follow_stdout,
            follow_stderr=follow_stderr,
        )
        try:
            for chunk in self._stub.StreamOutput(
                request, timeout=rpc_timeout, metadata=metadata
            ):
                yield _output_chunk_from_proto(chunk)
        except grpc.RpcError as exc:
            raise _grpc_error_to_exception(exc) from exc


# ===================================================================
# Asynchronous client
# ===================================================================


class AsyncIsolateClient:
    """Asynchronous (``async``/``await``) client for the Isolate gRPC service.

    Built on :mod:`grpc.aio`.  Use as an async context manager::

        async with AsyncIsolateClient("localhost:50051") as client:
            resp = await client.create_sandbox(module=wasm_bytes)

    The constructor accepts the same parameters as :class:`IsolateClient`.
    """

    def __init__(
        self,
        target: str = "localhost:50051",
        *,
        timeout: float = _DEFAULT_TIMEOUT,
        max_retries: int = _DEFAULT_MAX_RETRIES,
        retry_backoff: float = _DEFAULT_RETRY_BACKOFF,
        tls: bool = False,
        root_certificates: Optional[bytes] = None,
        private_key: Optional[bytes] = None,
        certificate_chain: Optional[bytes] = None,
        metadata: Optional[Dict[str, str]] = None,
        options: Optional[Dict[str, Any]] = None,
        max_message_length: int = _DEFAULT_MAX_MESSAGE_LENGTH,
    ) -> None:
        _ensure_stubs()

        self._config = ClientConfig(
            target=target,
            timeout=timeout,
            max_retries=max_retries,
            retry_backoff=retry_backoff,
            tls=tls,
            root_certificates=root_certificates,
            private_key=private_key,
            certificate_chain=certificate_chain,
            metadata=metadata or {},
            options=options or {},
            max_message_length=max_message_length,
        )

        self._channel: grpc.aio.Channel = _make_async_channel(self._config)
        self._stub = isolate_pb2_grpc.IsolateServiceStub(self._channel)
        self._closed = False

    # -- Context manager ---------------------------------------------------

    async def __aenter__(self) -> AsyncIsolateClient:
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.close()

    # -- Lifecycle ---------------------------------------------------------

    async def close(self) -> None:
        """Close the underlying gRPC channel.

        It is safe to call this method more than once.
        """
        if not self._closed:
            await self._channel.close()
            self._closed = True

    # -- RPC helpers -------------------------------------------------------

    async def _call_with_retry(
        self, method: str, request: Any, timeout: Optional[float] = None
    ) -> Any:
        """Invoke *method* on the stub with automatic retry for transient errors."""
        import asyncio

        rpc_timeout = timeout if timeout is not None else self._config.timeout
        metadata = _make_metadata(self._config)
        stub_method = getattr(self._stub, method)
        last_exc: Optional[Exception] = None

        for attempt in range(1, self._config.max_retries + 1):
            try:
                return await stub_method(
                    request,
                    timeout=rpc_timeout,
                    metadata=metadata,
                )
            except grpc.aio.AioRpcError as exc:
                last_exc = exc
                code = exc.code()
                if code not in _RETRYABLE_STATUS_CODES or attempt == self._config.max_retries:
                    raise _grpc_error_to_exception(exc) from exc
                backoff = self._config.retry_backoff * (2 ** (attempt - 1))
                logger.warning(
                    "Transient gRPC error (attempt %d/%d, code=%s). "
                    "Retrying in %.1fs ...",
                    attempt,
                    self._config.max_retries,
                    code.name,
                    backoff,
                )
                await asyncio.sleep(backoff)

        assert last_exc is not None
        raise _grpc_error_to_exception(last_exc) from last_exc  # type: ignore[arg-type]

    # -- Public API --------------------------------------------------------

    async def create_sandbox(
        self,
        module: bytes,
        config: Optional[SandboxConfig] = None,
        *,
        timeout: Optional[float] = None,
    ) -> CreateSandboxResponse:
        """Create a new sandbox from a WASM module.

        See :meth:`IsolateClient.create_sandbox` for full documentation.
        """
        proto_config = _sandbox_config_to_proto(config) if config else None
        request = isolate_pb2.CreateSandboxRequest(
            module=module,
            config=proto_config,
        )
        resp = await self._call_with_retry("CreateSandbox", request, timeout=timeout)
        return CreateSandboxResponse(
            sandbox_id=resp.sandbox_id,
            module_hash=resp.module_hash,
            creation_time_ms=resp.creation_time_ms,
        )

    async def run_sandbox(
        self,
        sandbox_id: str,
        *,
        input: bytes = b"",
        entry_point: str = "_start",
        timeout: Optional[float] = None,
    ) -> RunSandboxResponse:
        """Run an existing sandbox.

        See :meth:`IsolateClient.run_sandbox` for full documentation.
        """
        request = isolate_pb2.RunSandboxRequest(
            sandbox_id=sandbox_id,
            input=input,
            entry_point=entry_point,
        )
        resp = await self._call_with_retry("RunSandbox", request, timeout=timeout)
        return RunSandboxResponse(
            exit_code=resp.exit_code,
            stdout=bytes(resp.stdout),
            stderr=bytes(resp.stderr),
            duration_ms=resp.duration_ms,
            resource_usage=_resource_usage_from_proto(
                resp.resource_usage if resp.HasField("resource_usage") else None
            ),
        )

    async def get_sandbox(
        self,
        sandbox_id: str,
        *,
        timeout: Optional[float] = None,
    ) -> GetSandboxResponse:
        """Get the status and information for a sandbox.

        See :meth:`IsolateClient.get_sandbox` for full documentation.
        """
        request = isolate_pb2.GetSandboxRequest(sandbox_id=sandbox_id)
        resp = await self._call_with_retry("GetSandbox", request, timeout=timeout)
        return GetSandboxResponse(
            sandbox=_sandbox_info_from_proto(resp.sandbox),
        )

    async def terminate_sandbox(
        self,
        sandbox_id: str,
        *,
        timeout: Optional[float] = None,
    ) -> TerminateSandboxResponse:
        """Terminate a sandbox and release its resources.

        See :meth:`IsolateClient.terminate_sandbox` for full documentation.
        """
        request = isolate_pb2.TerminateSandboxRequest(sandbox_id=sandbox_id)
        resp = await self._call_with_retry("TerminateSandbox", request, timeout=timeout)
        return TerminateSandboxResponse(
            terminated=resp.terminated,
            metrics=_sandbox_metrics_from_proto(
                resp.metrics if resp.HasField("metrics") else None
            ),
        )

    async def list_sandboxes(
        self,
        *,
        state_filter: str = "",
        limit: int = 0,
        offset: int = 0,
        timeout: Optional[float] = None,
    ) -> ListSandboxesResponse:
        """List sandboxes managed by the server.

        See :meth:`IsolateClient.list_sandboxes` for full documentation.
        """
        request = isolate_pb2.ListSandboxesRequest(
            state_filter=state_filter,
            limit=limit,
            offset=offset,
        )
        resp = await self._call_with_retry("ListSandboxes", request, timeout=timeout)
        return ListSandboxesResponse(
            sandboxes=[_sandbox_info_from_proto(s) for s in resp.sandboxes],
            total=resp.total,
        )

    async def get_metrics(
        self,
        format: str = "prometheus",
        *,
        timeout: Optional[float] = None,
    ) -> GetMetricsResponse:
        """Retrieve server metrics.

        See :meth:`IsolateClient.get_metrics` for full documentation.
        """
        request = isolate_pb2.GetMetricsRequest(format=format)
        resp = await self._call_with_retry("GetMetrics", request, timeout=timeout)
        return GetMetricsResponse(data=resp.data)

    async def stream_output(
        self,
        sandbox_id: str,
        *,
        follow_stdout: bool = True,
        follow_stderr: bool = True,
        timeout: Optional[float] = None,
    ) -> AsyncIterator[OutputChunk]:
        """Stream live output from a running sandbox.

        This is a server-streaming RPC.  Use ``async for`` to consume
        chunks as they arrive::

            async for chunk in client.stream_output(sandbox_id):
                print(chunk.stream, chunk.data)

        See :meth:`IsolateClient.stream_output` for full documentation.
        """
        rpc_timeout = timeout if timeout is not None else self._config.timeout
        metadata = _make_metadata(self._config)
        request = isolate_pb2.StreamOutputRequest(
            sandbox_id=sandbox_id,
            follow_stdout=follow_stdout,
            follow_stderr=follow_stderr,
        )
        try:
            async for chunk in self._stub.StreamOutput(
                request, timeout=rpc_timeout, metadata=metadata
            ):
                yield _output_chunk_from_proto(chunk)
        except grpc.aio.AioRpcError as exc:
            raise _grpc_error_to_exception(exc) from exc
