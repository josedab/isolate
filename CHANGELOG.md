# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2024-12-29

### Added

- Initial release of Isolate sandbox runtime
- Core sandbox execution with Wasmtime 27
- WASI preview1 support for standard I/O operations
- Capability-based security system
  - Filesystem read/write capabilities with path restrictions
  - Network capabilities (HTTP, TCP, DNS)
  - Environment variable access control
  - Clock and timer capabilities
- Resource limit enforcement
  - Memory limits via Wasmtime StoreLimits
  - CPU time limits via fuel metering
  - I/O bandwidth limits with metering
  - Execution timeout via epoch interruption
- Audit logging for security-relevant operations
- Prometheus metrics integration
- OpenTelemetry tracing support
- gRPC server (`isolate-server`)
- Command-line interface (`isolate-cli`)
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
