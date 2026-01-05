"""Type stubs for the Isolate native module."""

from typing import Optional

class Capability:
    """Capability granting specific permissions to a sandbox."""

    @staticmethod
    def stdout() -> Capability:
        """Grant stdout access."""
        ...

    @staticmethod
    def stderr() -> Capability:
        """Grant stderr access."""
        ...

    @staticmethod
    def stdin() -> Capability:
        """Grant stdin access."""
        ...

    @staticmethod
    def filesystem_read(path: str) -> Capability:
        """Grant read access to a filesystem path."""
        ...

    @staticmethod
    def filesystem_write(path: str) -> Capability:
        """Grant read-write access to a filesystem path."""
        ...

    @staticmethod
    def env_all() -> Capability:
        """Grant access to all environment variables."""
        ...

    @staticmethod
    def env_var(name: str) -> Capability:
        """Grant access to a specific environment variable."""
        ...

    @staticmethod
    def system_clock() -> Capability:
        """Grant system clock access."""
        ...

    @staticmethod
    def monotonic_clock() -> Capability:
        """Grant monotonic clock access."""
        ...

    @staticmethod
    def timers() -> Capability:
        """Grant timer access (sleep, intervals)."""
        ...

    @staticmethod
    def secure_random() -> Capability:
        """Grant secure random number generation access."""
        ...

    @staticmethod
    def seeded_random(seed: int) -> Capability:
        """Grant seeded (deterministic) random number generation."""
        ...

    @staticmethod
    def http_client(hosts: list[str]) -> Capability:
        """Grant HTTP client access to specific hosts."""
        ...

    @staticmethod
    def temp_dir() -> Capability:
        """Grant temporary directory access."""
        ...


class SandboxConfigBuilder:
    """Builder for sandbox configuration."""

    def module(self, wasm_bytes: bytes) -> SandboxConfigBuilder:
        """Set the WASM module from bytes."""
        ...

    def module_from_file(self, path: str) -> SandboxConfigBuilder:
        """Set the WASM module from a file path."""
        ...

    def memory_limit(self, bytes: int) -> SandboxConfigBuilder:
        """Set the memory limit in bytes."""
        ...

    def fuel(self, amount: int) -> SandboxConfigBuilder:
        """Set the fuel limit (instruction count)."""
        ...

    def cpu_time_limit(self, seconds: float) -> SandboxConfigBuilder:
        """Set the CPU time limit in seconds."""
        ...

    def capability(self, cap: Capability) -> SandboxConfigBuilder:
        """Add a capability."""
        ...

    def env(self, key: str, value: str) -> SandboxConfigBuilder:
        """Set an environment variable."""
        ...

    def envs(self, vars: dict[str, str]) -> SandboxConfigBuilder:
        """Set environment variables from a dictionary."""
        ...

    def arg(self, value: str) -> SandboxConfigBuilder:
        """Add a command-line argument."""
        ...

    def args(self, values: list[str]) -> SandboxConfigBuilder:
        """Set command-line arguments."""
        ...

    def build(self) -> SandboxConfig:
        """Build the configuration."""
        ...


class SandboxConfig:
    """Sandbox configuration."""

    @staticmethod
    def builder() -> SandboxConfigBuilder:
        """Create a new configuration builder."""
        ...


class Output:
    """Output from a sandbox execution."""

    @property
    def exit_code(self) -> int:
        """Exit code from the sandbox."""
        ...

    @property
    def stdout(self) -> bytes:
        """Standard output bytes."""
        ...

    @property
    def stderr(self) -> bytes:
        """Standard error bytes."""
        ...

    @property
    def duration_secs(self) -> float:
        """Execution duration in seconds."""
        ...

    @property
    def fuel_consumed(self) -> int:
        """Fuel consumed."""
        ...

    def stdout_str(self) -> str:
        """Get stdout as a string."""
        ...

    def stderr_str(self) -> str:
        """Get stderr as a string."""
        ...

    def is_success(self) -> bool:
        """Check if execution was successful (exit code 0)."""
        ...


class Sandbox:
    """A secure WebAssembly sandbox."""

    @staticmethod
    def create(config: SandboxConfig) -> Sandbox:
        """Create a new sandbox with the given configuration."""
        ...

    @property
    def id(self) -> str:
        """Get the sandbox ID."""
        ...

    @property
    def state(self) -> str:
        """Get the sandbox state."""
        ...

    def run(self, input: Optional[bytes] = None) -> Output:
        """Run the sandbox with optional input."""
        ...

    def terminate(self) -> None:
        """Terminate the sandbox."""
        ...


def run_wasm(
    wasm_bytes: bytes,
    memory_limit: Optional[int] = None,
    fuel: Optional[int] = None,
    stdin: Optional[bytes] = None,
    env: Optional[dict[str, str]] = None,
) -> Output:
    """Run a WASM module with simple configuration."""
    ...


def run_wasm_file(
    path: str,
    memory_limit: Optional[int] = None,
    fuel: Optional[int] = None,
    stdin: Optional[bytes] = None,
    env: Optional[dict[str, str]] = None,
) -> Output:
    """Run a WASM file with simple configuration."""
    ...


def version() -> str:
    """Get the version of the isolate library."""
    ...
