# ADR-0006: Hierarchical Error Types with Categorization

## Status

Accepted

## Context

A secure sandbox runtime can fail in many ways: compilation errors, capability denials, resource exhaustion, execution failures, I/O errors, and more. Early error handling using `String` or generic errors made it difficult to:

- Programmatically handle specific error types
- Distinguish user errors from system errors
- Provide actionable error messages
- Track error categories in metrics

We needed an error system that:

- Provides exhaustive coverage of failure modes
- Enables pattern matching for error handling
- Supports categorization for metrics and logging
- Integrates well with Rust's `?` operator

## Decision

We implemented **hierarchical error types** using `thiserror` with categorization methods.

### Error Enum

```rust
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    // Creation/Compilation
    #[error("Failed to create sandbox: {0}")]
    Create(String),

    #[error("WASM compilation error: {0}")]
    Compilation(String),

    #[error("WASM instantiation error: {0}")]
    Instantiation(String),

    // Execution
    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Execution timed out after {0:?}")]
    Timeout(Duration),

    // Resource Limits
    #[error("CPU fuel exhausted (limit: {limit} units)")]
    FuelExhausted { limit: u64 },

    #[error("Memory limit exceeded (limit: {limit}, requested: {requested})")]
    MemoryLimitExceeded { limit: usize, requested: usize },

    // Capability Denials
    #[error("Capability not granted: {0}")]
    CapabilityDenied(Capability),

    #[error("Filesystem access denied: {path}")]
    FilesystemAccessDenied { path: PathBuf },

    #[error("Network access denied: {host}")]
    NetworkAccessDenied { host: String },

    // State Errors
    #[error("Invalid sandbox state: expected {expected}, got {actual}")]
    InvalidState { expected: String, actual: String },

    // Module Errors
    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    // I/O
    #[error("I/O error: {source}")]
    Io { #[from] source: std::io::Error },
}
```

### Categorization Methods

```rust
impl Error {
    /// Returns true if this is a timeout error.
    pub fn is_timeout(&self) -> bool {
        matches!(self, Error::Timeout(_))
    }

    /// Returns true if this is a resource limit error.
    pub fn is_resource_limit(&self) -> bool {
        matches!(
            self,
            Error::FuelExhausted { .. }
            | Error::MemoryLimitExceeded { .. }
            | Error::Timeout(_)
        )
    }

    /// Returns true if this is a capability error.
    pub fn is_capability_error(&self) -> bool {
        matches!(
            self,
            Error::CapabilityDenied(_)
            | Error::FilesystemAccessDenied { .. }
            | Error::NetworkAccessDenied { .. }
        )
    }
}
```

### Type Alias

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## Consequences

### Positive

- **Exhaustive matching**: Compiler ensures all error cases are handled
- **Rich context**: Structured variants like `MemoryLimitExceeded { limit, requested }` provide details
- **Easy categorization**: `is_resource_limit()` simplifies error handling logic
- **Metrics-friendly**: Error variants can be mapped to metric labels
- **Ergonomic**: `#[from]` enables automatic conversion from `std::io::Error`
- **Non-exhaustive**: `#[non_exhaustive]` allows adding variants without breaking changes

### Negative

- **Verbose definitions**: Each error case needs explicit handling
- **String fallbacks**: Some variants like `Execution(String)` lose type information
- **Conversion boilerplate**: Wasmtime errors require manual conversion

### Implications

- All public functions should return `Result<T>` (the aliased type)
- Error messages should be user-actionable when possible
- New error types should consider which category they belong to
- Tests should verify specific error variants, not just `Err(_)`
