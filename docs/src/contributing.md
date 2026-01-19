# Contributing

Thank you for your interest in contributing to Isolate!

## Getting Started

See [CONTRIBUTING.md](https://github.com/josedab/isolate/blob/main/CONTRIBUTING.md) in the repository for detailed guidelines.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/josedab/isolate.git
cd isolate

# Build
cargo build

# Run tests
cargo test --all-features --workspace

# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
```

## Areas to Contribute

### Good First Issues

Look for issues labeled [`good first issue`](https://github.com/josedab/isolate/labels/good%20first%20issue).

### Documentation

- Improve existing documentation
- Add examples
- Fix typos

### Testing

- Add unit tests
- Add integration tests
- Improve test coverage

### Features

- Implement new capabilities
- Add resource metering features
- Improve performance

## Code Style

- Follow `rustfmt` formatting
- Address all `clippy` warnings
- Write documentation for public APIs
- Include tests for new functionality

## Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests and clippy
5. Submit a pull request

## Questions?

- Open a [Discussion](https://github.com/josedab/isolate/discussions)
- Check existing [Issues](https://github.com/josedab/isolate/issues)
