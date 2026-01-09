---
sidebar_position: 7
---

# Changelog

All notable changes to Isolate will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Docusaurus documentation site
- Comprehensive API reference
- Comparison guides (vs Wasmtime, Firecracker, gVisor)

### Changed
- Improved error messages with more context

### Fixed
- Documentation typos and formatting

---

## [0.1.0] - 2025-01-15

### Added

#### Core Features
- **Sandbox Runtime** - Secure WASM execution environment
- **Capability System** - Fine-grained, default-deny permissions
  - `Capability::stdout()` - Standard output access
  - `Capability::stderr()` - Standard error access
  - `Capability::stdin()` - Standard input access
  - `Capability::filesystem_read(path)` - Read-only filesystem access
  - `Capability::filesystem_write(path)` - Read-write filesystem access
  - `Capability::system_clock()` - System time access
  - `Capability::secure_random()` - Cryptographic randomness
  - `Capability::env_var(name)` - Environment variable access
  - `Capability::http_client(hosts)` - HTTP client access

#### Resource Control
- **Fuel Metering** - CPU instruction limiting
- **Memory Limits** - Configurable memory caps
- **I/O Limits** - stdout/stderr size limits
- **Wall Clock Timeout** - Epoch-based execution timeout

#### Engine
- **Wasmtime Integration** - Built on Wasmtime 27
- **Module Caching** - SHA-256 based module cache
- **WASI Support** - WASI preview1 implementation

#### Monitoring
- **Prometheus Metrics** - Built-in observability
  - `sandbox_executions_total`
  - `sandbox_execution_duration_seconds`
  - `sandbox_fuel_consumed`
  - `sandbox_memory_bytes`
  - `capability_grants_total`
  - `capability_denials_total`
- **Structured Logging** - tracing integration
- **Audit Logging** - Security event logging

#### API
- **Builder Pattern** - Fluent configuration API
- **Async Runtime** - Tokio-based async execution
- **Comprehensive Errors** - Detailed error types

### Infrastructure
- **isolate-core** - Core library crate
- **isolate-server** - gRPC server
- **isolate-cli** - Command-line interface
- **isolate-python** - Python bindings (experimental)

---

## Version History

### Versioning Policy

Isolate follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version for incompatible API changes
- **MINOR** version for backwards-compatible functionality
- **PATCH** version for backwards-compatible bug fixes

### Pre-1.0 Note

While Isolate is pre-1.0, minor version bumps may include breaking changes. We recommend pinning to exact versions in production:

```toml
[dependencies]
isolate-core = "=0.1.0"
```

### Stability Guarantees

| Component | Stability |
|-----------|-----------|
| Public API (`Sandbox`, `SandboxConfig`) | Stable |
| Capability types | Stable |
| Error types | Stable |
| Metrics names | Stable |
| Internal modules | Unstable |
| gRPC protocol | Unstable |

---

## Migration Guides

### Migrating from 0.x to 1.0

*Coming when 1.0 is released*

---

## Release Schedule

- **Patch releases**: As needed for bug fixes
- **Minor releases**: Monthly feature releases
- **Major releases**: When API changes are necessary

---

## Links

- [GitHub Releases](https://github.com/josedab/isolate/releases)
- [crates.io](https://crates.io/crates/isolate-core)
- [API Documentation](https://docs.rs/isolate-core)
