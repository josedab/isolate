"""Unit tests for the Isolate Python SDK.

Tests cover model construction, capability builders, serialization,
and error handling — all without requiring a running gRPC server.
"""

import pytest
from isolate_sdk.models import (
    Capability,
    SandboxConfig,
    CreateSandboxRequest,
    RunSandboxRequest,
    GetSandboxRequest,
    TerminateSandboxRequest,
    ListSandboxesRequest,
    ResourceUsage,
)
from isolate_sdk.exceptions import (
    IsolateError,
    NotFoundError,
    InvalidArgumentError,
    AlreadyExistsError,
    TimeoutError,
    ConnectionError,
    ResourceExhaustedError,
)


# ---------------------------------------------------------------------------
# Capability tests
# ---------------------------------------------------------------------------


class TestCapability:
    def test_stdout(self):
        cap = Capability.stdout()
        assert cap.type == "stdout"
        assert cap.value == ""

    def test_stderr(self):
        cap = Capability.stderr()
        assert cap.type == "stderr"

    def test_stdin(self):
        cap = Capability.stdin()
        assert cap.type == "stdin"

    def test_fs_read(self):
        cap = Capability.fs_read("/data")
        assert cap.type == "fs_read"
        assert cap.value == "/data"

    def test_fs_write(self):
        cap = Capability.fs_write("/output")
        assert cap.type == "fs_write"
        assert cap.value == "/output"

    def test_temp_dir(self):
        cap = Capability.temp_dir()
        assert cap.type == "temp_dir"

    def test_http(self):
        cap = Capability.http("api.example.com")
        assert cap.type == "http"
        assert cap.value == "api.example.com"

    def test_dns(self):
        cap = Capability.dns()
        assert cap.type == "dns"

    def test_system_clock(self):
        cap = Capability.system_clock()
        assert cap.type == "system_clock"

    def test_monotonic_clock(self):
        cap = Capability.monotonic_clock()
        assert cap.type == "monotonic_clock"

    def test_random(self):
        cap = Capability.random()
        assert cap.type == "random"

    def test_env(self):
        cap = Capability.env("API_KEY")
        assert cap.type == "env"
        assert cap.value == "API_KEY"

    def test_equality(self):
        assert Capability.stdout() == Capability.stdout()
        assert Capability.stdout() != Capability.stderr()

    def test_frozen(self):
        cap = Capability.stdout()
        with pytest.raises(AttributeError):
            cap.type = "modified"


# ---------------------------------------------------------------------------
# Model tests
# ---------------------------------------------------------------------------


class TestSandboxConfig:
    def test_default_config(self):
        config = SandboxConfig()
        assert config.capabilities == []
        assert config.env == {}

    def test_config_with_capabilities(self):
        config = SandboxConfig(
            capabilities=[Capability.stdout(), Capability.stderr()],
            memory_limit=128 * 1024 * 1024,
            fuel=1_000_000,
        )
        assert len(config.capabilities) == 2
        assert config.memory_limit == 128 * 1024 * 1024
        assert config.fuel == 1_000_000

    def test_config_with_env(self):
        config = SandboxConfig(env={"KEY": "value", "OTHER": "data"})
        assert config.env["KEY"] == "value"
        assert len(config.env) == 2


class TestRequestModels:
    def test_create_request(self):
        req = CreateSandboxRequest(module=b"\x00asm\x01\x00\x00\x00")
        assert req.module == b"\x00asm\x01\x00\x00\x00"

    def test_run_request(self):
        req = RunSandboxRequest(sandbox_id="sb-123", input=b"hello")
        assert req.sandbox_id == "sb-123"
        assert req.input == b"hello"

    def test_get_request(self):
        req = GetSandboxRequest(sandbox_id="sb-456")
        assert req.sandbox_id == "sb-456"

    def test_terminate_request(self):
        req = TerminateSandboxRequest(sandbox_id="sb-789")
        assert req.sandbox_id == "sb-789"

    def test_list_request(self):
        req = ListSandboxesRequest()
        assert req is not None


# ---------------------------------------------------------------------------
# Exception tests
# ---------------------------------------------------------------------------


class TestExceptions:
    def test_isolate_error_base(self):
        err = IsolateError("test error")
        assert str(err) == "test error"
        assert isinstance(err, Exception)

    def test_not_found_error(self):
        err = NotFoundError("sandbox not found")
        assert isinstance(err, IsolateError)

    def test_invalid_argument_error(self):
        err = InvalidArgumentError("bad input")
        assert isinstance(err, IsolateError)

    def test_already_exists_error(self):
        err = AlreadyExistsError("duplicate")
        assert isinstance(err, IsolateError)

    def test_timeout_error(self):
        err = TimeoutError("timed out")
        assert isinstance(err, IsolateError)

    def test_connection_error(self):
        err = ConnectionError("unreachable")
        assert isinstance(err, IsolateError)

    def test_resource_exhausted_error(self):
        err = ResourceExhaustedError("out of memory")
        assert isinstance(err, IsolateError)


# ---------------------------------------------------------------------------
# Resource usage tests
# ---------------------------------------------------------------------------


class TestResourceUsage:
    def test_default(self):
        usage = ResourceUsage()
        assert usage.fuel_consumed == 0
        assert usage.wall_time_ms == 0

    def test_with_values(self):
        usage = ResourceUsage(
            fuel_consumed=50000,
            wall_time_ms=150,
            peak_memory=1024 * 1024,
        )
        assert usage.fuel_consumed == 50000
        assert usage.wall_time_ms == 150
        assert usage.peak_memory == 1024 * 1024
