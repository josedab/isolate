# Quickstart

Get from clone to your first passing test in under 60 seconds.

## Prerequisites

- [Rust 1.75.0+](https://rustup.rs/)
- (Optional) [protobuf compiler](https://grpc.io/docs/protoc-installation/) — only needed for `isolate-server`

## 5 Commands

```bash
# 1. Clone
git clone https://github.com/josedab/isolate.git && cd isolate

# 2. Verify your environment
cargo xtask doctor

# 3. Build
cargo build

# 4. Run tests
cargo test

# 5. Run the example sandbox
cargo run --package isolate-core --example basic_sandbox
```

## Make a Change

1. Open `isolate-core/src/sandbox.rs` and explore the `Sandbox` struct.
2. Run the core tests for fast feedback:
   ```bash
   cargo test --package isolate-core
   ```
3. Check formatting and lints before pushing:
   ```bash
   cargo xtask check
   ```

## Next Steps

- Read [CONTRIBUTING.md](../CONTRIBUTING.md) for the full development workflow.
- Read [ARCHITECTURE.md](../ARCHITECTURE.md) for a deep dive into the design.
- Browse [issues labeled `good first issue`](https://github.com/josedab/isolate/labels/good%20first%20issue).
