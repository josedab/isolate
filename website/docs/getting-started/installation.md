---
sidebar_position: 1
---

# Installation

## Library (Rust)

Add `isolate-core` to your `Cargo.toml`:

```toml
[dependencies]
isolate-core = "0.1"
tokio = { version = "1", features = ["full"] }
```

Or use cargo:

```bash
cargo add isolate-core tokio --features tokio/full
```

## CLI Tool

### From crates.io

```bash
cargo install isolate-cli
```

### From Source

```bash
git clone https://github.com/josedab/isolate.git
cd isolate
cargo install --path isolate-cli
```

### Verify Installation

```bash
isolate --version
```

## gRPC Server

### From crates.io

```bash
cargo install isolate-server
```

### From Source

```bash
cargo install --path isolate-server
```

### Running the Server

```bash
isolate-server --addr 0.0.0.0:50051
```

## Python Bindings

:::caution Experimental
Python bindings are currently experimental and not recommended for production use.
:::

```bash
pip install isolate-py
```

## Development Setup

For contributing to Isolate:

### Prerequisites

- Rust 1.75.0 or later
- Git

### Clone and Build

```bash
git clone https://github.com/josedab/isolate.git
cd isolate
cargo build
```

### Run Tests

```bash
cargo test --all-features --workspace
```

### Optional Tools

For the best development experience, install:

```bash
# Task runner (recommended)
cargo install just

# Code coverage
cargo install cargo-tarpaulin

# Security audit
cargo install cargo-audit cargo-deny

# Fuzz testing (requires nightly)
cargo +nightly install cargo-fuzz
```

## System Requirements

### Minimum

| Requirement | Value |
|------------|-------|
| **OS** | Linux, macOS, Windows |
| **Rust** | 1.75.0 |
| **Memory** | 512MB RAM |
| **Disk** | 100MB for dependencies |

### Recommended

| Requirement | Value |
|------------|-------|
| **Memory** | 4GB+ RAM (for building and running tests) |
| **CPU** | Multi-core (for parallel compilation) |

## Next Steps

Once installed, proceed to the [Quick Start](./quick-start) guide.
