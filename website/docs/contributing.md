---
sidebar_position: 6
---

# Contributing

Thank you for your interest in contributing to Isolate! This guide will help you get started.

## Getting Started

### Prerequisites

- Rust 1.75 or later
- Git
- A GitHub account

### Clone the Repository

```bash
git clone https://github.com/josedab/isolate.git
cd isolate
```

### Build the Project

```bash
cargo build
```

### Run Tests

```bash
cargo test
```

## Development Workflow

### 1. Find an Issue

- Check [GitHub Issues](https://github.com/josedab/isolate/issues) for open tasks
- Look for `good first issue` labels for beginner-friendly tasks
- Look for `help wanted` labels for tasks needing contributors

### 2. Create a Branch

```bash
git checkout -b feature/my-feature
# or
git checkout -b fix/my-bugfix
```

### 3. Make Changes

Follow the code conventions below and make your changes.

### 4. Test Your Changes

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture
```

### 5. Submit a Pull Request

1. Push your branch to GitHub
2. Open a pull request against `main`
3. Fill out the PR template
4. Wait for review

## Code Conventions

### Rust Style

We follow standard Rust conventions:

```rust
// Good: Use snake_case for functions and variables
fn create_sandbox(config: SandboxConfig) -> Result<Sandbox> {
    let sandbox_id = Uuid::new_v4();
    // ...
}

// Good: Use PascalCase for types
pub struct SandboxConfig {
    pub module_bytes: Vec<u8>,
    pub capabilities: Vec<Capability>,
}

// Good: Use SCREAMING_SNAKE_CASE for constants
const DEFAULT_FUEL_LIMIT: u64 = 1_000_000;
```

### Error Handling

Use `thiserror` for error definitions:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid module: {0}")]
    InvalidModule(String),

    #[error("Capability denied: {0}")]
    CapabilityDenied(String),
}
```

Never panic in library code:

```rust
// Bad
fn get_value(index: usize) -> i32 {
    self.values[index]  // May panic!
}

// Good
fn get_value(&self, index: usize) -> Option<i32> {
    self.values.get(index).copied()
}
```

### Documentation

Document public APIs with examples:

```rust
/// Creates a new sandbox with the given configuration.
///
/// # Arguments
///
/// * `config` - The sandbox configuration
///
/// # Returns
///
/// A `Result` containing the sandbox or an error
///
/// # Example
///
/// ```rust
/// let config = SandboxConfig::builder()
///     .module(&wasm_bytes)?
///     .build()?;
/// let sandbox = Sandbox::create(config).await?;
/// ```
pub async fn create(config: SandboxConfig) -> Result<Self> {
    // ...
}
```

### Testing

Write tests for new functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_check() {
        let enforcer = CapabilityEnforcer::new(vec![
            Capability::stdout(),
        ]);

        assert!(enforcer.check_stdout().is_ok());
        assert!(enforcer.check_stderr().is_err());
    }

    #[tokio::test]
    async fn test_sandbox_execution() {
        let config = SandboxConfig::builder()
            .module(HELLO_WASM)
            .expect("valid module")
            .capability(Capability::stdout())
            .build()
            .expect("valid config");

        let mut sandbox = Sandbox::create(config).await.expect("create");
        let output = sandbox.run(&[]).await.expect("run");

        assert_eq!(output.exit_code, 0);
    }
}
```

## Project Structure

```
isolate/
├── isolate-core/           # Core library
│   ├── src/
│   │   ├── lib.rs          # Crate root
│   │   ├── sandbox.rs      # Sandbox implementation
│   │   ├── config.rs       # Configuration
│   │   ├── error.rs        # Error types
│   │   ├── capability/     # Capability system
│   │   ├── engine/         # WASM engine
│   │   └── resource/       # Resource metering
│   └── tests/              # Integration tests
├── isolate-server/         # gRPC server
├── isolate-cli/            # CLI tool
└── website/                # Documentation
```

## Adding Features

### Adding a New Capability

1. **Define the capability** in `capability/types.rs`:

```rust
pub enum NetworkCapability {
    HttpClient(Vec<String>),
    TcpConnect(Vec<SocketAddr>),
    // Add your new capability here
    UdpSend(Vec<SocketAddr>),
}
```

2. **Add check method** in `capability/enforcer.rs`:

```rust
impl CapabilityEnforcer {
    pub fn check_udp_send(&self, addr: &SocketAddr) -> Result<()> {
        for cap in &self.capabilities {
            if let Capability::Network(NetworkCapability::UdpSend(addrs)) = cap {
                if addrs.contains(addr) {
                    return Ok(());
                }
            }
        }
        Err(Error::CapabilityDenied("udp_send".into()))
    }
}
```

3. **Wire into WASI** if needed in `engine/wasm.rs`

4. **Add tests**:

```rust
#[test]
fn test_udp_capability() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let enforcer = CapabilityEnforcer::new(vec![
        Capability::Network(NetworkCapability::UdpSend(vec![addr])),
    ]);

    assert!(enforcer.check_udp_send(&addr).is_ok());
}
```

5. **Update documentation**

### Adding Resource Limits

1. **Add limit field** in `resource/limits.rs`:

```rust
pub struct ResourceLimits {
    pub fuel: Option<u64>,
    pub memory_limit: Option<usize>,
    // Add your new limit
    pub table_elements: Option<u32>,
}
```

2. **Add builder method** in `config.rs`:

```rust
impl SandboxConfigBuilder {
    pub fn table_elements(mut self, limit: u32) -> Self {
        self.limits.table_elements = Some(limit);
        self
    }
}
```

3. **Enforce in engine** in `engine/wasm.rs`

4. **Add tests and documentation**

## Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench sandbox_creation
```

## Documentation

### Building Docs

```bash
# Build API docs
cargo doc --open

# Build website
cd website
npm install
npm run build
```

### Writing Documentation

- Use clear, concise language
- Include code examples
- Add diagrams for complex concepts (Mermaid supported)

## Release Process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create a tag: `git tag v0.1.0`
4. Push: `git push --tags`
5. CI will build and publish to crates.io

## Getting Help

- **Questions**: [GitHub Discussions](https://github.com/josedab/isolate/discussions)
- **Bugs**: [GitHub Issues](https://github.com/josedab/isolate/issues)
- **Security**: Email security@isolate.dev (do not open public issues)

## Code of Conduct

We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Be respectful and inclusive.

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (MIT OR Apache-2.0).

---

Thank you for contributing to Isolate!
