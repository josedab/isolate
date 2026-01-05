"""Isolate: Secure WebAssembly Sandbox Runtime for Python.

This module provides Python bindings for the Isolate sandbox runtime,
allowing you to execute untrusted WebAssembly code safely.

Example:
    >>> import isolate
    >>>
    >>> # Create a sandbox configuration
    >>> config = isolate.SandboxConfig.builder() \\
    ...     .module_from_file("hello.wasm") \\
    ...     .memory_limit(128 * 1024 * 1024) \\
    ...     .fuel(1_000_000) \\
    ...     .capability(isolate.Capability.stdout()) \\
    ...     .capability(isolate.Capability.stderr()) \\
    ...     .build()
    >>>
    >>> # Create and run the sandbox
    >>> sandbox = isolate.Sandbox.create(config)
    >>> output = sandbox.run()
    >>>
    >>> print(f"Exit code: {output.exit_code}")
    >>> print(f"Stdout: {output.stdout_str()}")

For simpler use cases, use the convenience functions:
    >>> output = isolate.run_wasm_file("hello.wasm")
    >>> print(output.stdout_str())
"""

from isolate._isolate import (
    # Core classes
    Capability,
    SandboxConfig,
    SandboxConfigBuilder,
    Sandbox,
    Output,
    # Convenience functions
    run_wasm,
    run_wasm_file,
    version,
)

__all__ = [
    # Core classes
    "Capability",
    "SandboxConfig",
    "SandboxConfigBuilder",
    "Sandbox",
    "Output",
    # Convenience functions
    "run_wasm",
    "run_wasm_file",
    "version",
]

__version__ = version()
