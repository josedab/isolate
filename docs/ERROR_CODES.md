# Error Code Reference

Auto-generated from `isolate-core/src/error.rs` by `cargo xtask error-catalog`.

| Variant | Message | Category |
|---------|---------|----------|
| `Create` | Failed to create sandbox: {0} | Runtime |
| `Compilation` | WASM compilation error: {0} | Runtime |
| `Instantiation` | WASM instantiation error: {0} | Runtime |
| `Execution` | Execution error: {0} | Runtime |
| `Timeout` | Execution timed out after {0:?} | Runtime |
| `FuelExhausted` | CPU fuel exhausted (limit: {limit} units) | Resource |
| `MemoryLimitExceeded` | Memory limit exceeded (limit: {limit} bytes, requested: {requested} bytes) | Resource |
| `CapabilityDenied` | Capability not granted: {0} | Runtime |
| `InvalidCapability` | Invalid capability configuration: {0} | Security |
| `InvalidConfig` | Invalid configuration: {0} | Config |
| `InvalidState` | Invalid sandbox state: expected {expected}, got {actual} | Config |
| `Snapshot` | Snapshot error: {0} | Runtime |
| `SnapshotNotFound` | Snapshot not found: {0} | Runtime |
| `Io` | I/O error: {source} | Runtime |
| `FilesystemAccessDenied` | Filesystem access denied: {path} | Security |
| `NetworkAccessDenied` | Network access denied: {host} | Security |
| `Engine` | Internal engine error: {0} | Runtime |
| `ModuleValidation` | Module validation failed: {0} | Runtime |
| `FunctionNotFound` | Function not found: {0} | Runtime |
| `InvalidSignature` | Invalid function signature for '{name}': expected {expected}, got {actual} | Config |
| `PoolExhausted` | Warm pool exhausted, no available sandboxes | Runtime |
| `Http` | HTTP error: {0} | Runtime |
| `KvStore` | KV store error: {0} | Runtime |
| `Policy` | Policy error: {0} | Runtime |
| `Gateway` | Gateway error: {0} | Runtime |
| `Orchestrator` | Orchestrator error: {0} | Runtime |
| `Marketplace` | Marketplace error: {0} | Runtime |
