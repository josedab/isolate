# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Language profiles for multi-language WASM support
- Sandbox profiles for reusable configuration presets

## [0.1.0] - 2025-12-31

### Added

- WASM sandbox execution with Wasmtime 27
- Capability-based security system
  - Filesystem read/write capabilities with path restrictions
  - Network capabilities (HTTP, TCP, DNS)
  - Environment variable access control
  - Clock and timer capabilities
- Resource limits enforcement
  - Memory limits via Wasmtime StoreLimits
  - CPU time limits via fuel metering
  - I/O bandwidth limits with metering
- Epoch-based timeout interruption (10ms tick interval)
- Language profiles for multi-language WASM compilation targets
- Sandbox profiles for reusable sandbox configurations
- WASI Preview 1 support for standard I/O operations
- gRPC server (`isolate-server`)
- Command-line interface (`isolate-cli`)
- Audit logging for security-relevant operations
- Prometheus metrics integration
- OpenTelemetry tracing support
- Python bindings (`isolate-python`)

### Experimental

The following features are included but considered experimental and may change significantly:

- GPU sandboxing support
- Distributed sandbox mesh/clustering
- Enclave/TEE integration
- Hot patching support
- Formal verification module
- Linux-specific security (seccomp, Landlock)

[Unreleased]: https://github.com/josedab/isolate/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/josedab/isolate/releases/tag/v0.1.0
