# Contributing to Isolate

Thank you for your interest in contributing to Isolate! This document provides guidelines and information for contributors.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## Getting Started

### Prerequisites

- Rust 1.75.0 or later
- Git
- (Optional) Python 3.9+ with development headers for `isolate-python` bindings

> **Note:** `isolate-python` is excluded from the default workspace build via
> `default-members` in `Cargo.toml`. Standard `cargo build` / `cargo test` work
> without Python installed.

### Setting Up the Development Environment

1. Clone the repository:
   ```bash
   git clone https://github.com/josedab/isolate.git
   cd isolate
   ```

2. Verify your environment:
   ```bash
   cargo xtask doctor
   ```

3. Build the project:
   ```bash
   cargo build
   ```

4. Run the tests:
   ```bash
   cargo test
   ```

## Quick Workflow Reference

The project provides `cargo xtask` commands to streamline development:

```bash
cargo xtask doctor      # Verify your environment is set up correctly
cargo xtask check       # Run fmt + lint + test (use before pushing)
cargo xtask test        # Run all tests with --all-features
cargo xtask test-core   # Run core crate tests only (faster feedback)
cargo xtask fmt         # Format all code
cargo xtask lint        # Run clippy
cargo xtask pre-commit  # Full pre-push validation
cargo xtask docs        # Generate documentation
cargo xtask help        # Show all commands
```

If you prefer, `just` recipes are also available (see `justfile`).

## Project Structure

```
isolate/
├── isolate-core/        # Core library
├── isolate-cli/         # Command-line tool
├── isolate-server/      # gRPC server
├── isolate-python/      # Python bindings (requires python3-dev)
├── xtask/               # Developer workflow commands
├── docs/                # Documentation (Docusaurus)
└── .github/             # GitHub workflows and templates
```

## Development Workflow

### Branching Strategy

- `main` - Stable release branch
- `develop` - Development branch (default for PRs)
- `feature/*` - Feature branches
- `fix/*` - Bug fix branches
- `release/*` - Release preparation branches

### Making Changes

1. Create a new branch from `develop`:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes, following the coding standards below.

3. Write tests for new functionality.

4. Validate your changes:
   ```bash
   cargo xtask check
   ```

   This runs formatting checks, clippy, and the full test suite. You can also
   run individual steps:
   ```bash
   cargo xtask fmt         # Format code
   cargo xtask lint        # Clippy lints
   cargo xtask test-core   # Fast core-only tests
   cargo xtask test        # Full test suite
   ```

5. Commit your changes with a clear message:
   ```bash
   git commit -m "feat: add new capability for X"
   ```

6. Push and create a pull request.

## Coding Standards

### Rust Style

- Follow the official [Rust Style Guide](https://doc.rust-lang.org/style-guide/)
- Use `cargo fmt` for consistent formatting
- Address all `clippy` warnings
- Document public APIs with rustdoc comments
- Use `///` for item documentation
- Use `//!` for module-level documentation

### Error Handling

- Use the project's `Error` type from `isolate_core::error`
- Avoid `.unwrap()` in library code; use proper error propagation
- Provide meaningful error messages

### Testing

- Write unit tests for new functionality
- Place unit tests in the same file as the code they test
- Place integration tests in `tests/`
- Aim for high test coverage on critical paths

### Documentation

- Document all public APIs
- Include code examples in documentation
- Keep the README and CHANGELOG up to date

## Commit Messages

Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification:

- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation changes
- `style:` - Code style changes (formatting, etc.)
- `refactor:` - Code refactoring
- `test:` - Adding or updating tests
- `chore:` - Maintenance tasks
- `perf:` - Performance improvements
- `ci:` - CI/CD changes
- `deps:` - Dependency updates

Examples:
```
feat: add support for WASI Preview2
fix: resolve memory leak in sandbox cleanup
docs: update API documentation for capabilities
refactor: simplify resource metering logic
```

## Pull Request Process

1. Ensure all tests pass
2. Update documentation if needed
3. Fill out the PR template completely
4. Request review from maintainers
5. Address review feedback
6. Squash commits if requested

### PR Review Criteria

- Code quality and style
- Test coverage
- Documentation
- Performance considerations
- Security implications
- Backwards compatibility

## Release Process

Releases are managed by maintainers. The process:

1. Update version in `Cargo.toml` files
2. Update CHANGELOG.md
3. Create a release PR
4. After merge, create a git tag
5. GitHub Actions handles publishing

## Security

If you discover a security vulnerability:

1. **DO NOT** open a public issue
2. Use [GitHub's private vulnerability reporting](https://github.com/josedab/isolate/security/advisories/new) or contact [@josedab](https://github.com/josedab) directly
3. Allow time for a fix before public disclosure

## Getting Help

- Open a [Discussion](https://github.com/josedab/isolate/discussions) for questions
- Check existing [Issues](https://github.com/josedab/isolate/issues)
- Read the [Documentation](https://josedab.github.io/isolate/)

## Recognition

Contributors are recognized in:
- Release notes
- CONTRIBUTORS.md file
- GitHub's contributor graphs

Thank you for contributing to Isolate!
