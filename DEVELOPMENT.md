# Development Quick Reference

Common commands for working on Isolate. All commands assume you're at the repo root.

## Setup

```bash
cargo xtask doctor        # Verify toolchain, run checks, install git hooks
```

## Daily Development

| Command | What it does |
|---------|-------------|
| `cargo xtask check` | Format + lint + test in one shot |
| `cargo xtask test` | Run tests (default members) |
| `cargo test -p isolate-core` | Run core tests only (fastest) |
| `cargo xtask fmt` | Format all code |
| `cargo xtask lint` | Run clippy with `-D warnings` |
| `cargo check` | Quick compilation check |
| `cargo xtask pre-commit` | Full pre-push validation |
| `cargo xtask docs` | Generate API documentation |
| `cargo build --release` | Release build |
| `cargo test --all-features` | Test everything (needs python3-dev) |

## Alternative Task Runners

The same commands are available through multiple runners:

```bash
# Using just (if installed)
just check

# Using make (delegates to cargo xtask)
make check

# Directly
cargo xtask check
```

## Project Structure

| Crate | Purpose |
|-------|---------|
| `isolate-core` | Core library — sandbox, engine, capabilities |
| `isolate-server` | gRPC server |
| `isolate-cli` | Command-line tool |
| `isolate-embed` | Embeddable API |
| `isolate-python` | Python bindings (optional, needs python3-dev) |
| `xtask` | Developer workflow commands |

## Feature Flags

Most modules are behind feature flags. Build specific features with:

```bash
cargo test -p isolate-core --features policy-engine
cargo test -p isolate-core --features kubernetes
cargo check --all-features    # Everything at once
```

## Useful Links

- [README.md](README.md) — Project overview and getting started
- [CONTRIBUTING.md](CONTRIBUTING.md) — Contribution guidelines
- [ARCHITECTURE.md](ARCHITECTURE.md) — System architecture
- [CHANGELOG.md](CHANGELOG.md) — Release history
