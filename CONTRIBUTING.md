# Contributing to Isolate

Thank you for your interest in contributing to Isolate! This document provides guidelines and information for contributors.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for everyone.

## Getting Started

### Prerequisites

- Rust 1.75.0 or later
- Git
- (Optional) Python 3.9+ for Python bindings development

### Setting Up the Development Environment

1. Clone the repository:
   ```bash
   git clone https://github.com/OWNER/isolate.git
   cd isolate
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run the tests:
   ```bash
   cargo test
   ```

4. Run clippy for linting:
   ```bash
   cargo clippy --all-targets --all-features
   ```

## Project Structure

```
isolate/
├── isolate-core/        # Core library
├── isolate-cli/         # Command-line tool
├── isolate-server/      # gRPC server
├── isolate-python/      # Python bindings
├── docs/                # Documentation
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

4. Run the full test suite:
   ```bash
   cargo test --workspace --all-features
   ```

5. Run formatting and linting:
   ```bash
   cargo fmt --all
   cargo clippy --all-targets --all-features -- -D warnings
   ```

6. Commit your changes with a clear message:
   ```bash
   git commit -m "feat: add new capability for X"
   ```

7. Push and create a pull request.

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
2. Email security@example.com with details
3. Allow time for a fix before public disclosure

## Getting Help

- Open a [Discussion](https://github.com/OWNER/isolate/discussions) for questions
- Check existing [Issues](https://github.com/OWNER/isolate/issues)
- Read the [Documentation](https://OWNER.github.io/isolate/)

## Recognition

Contributors are recognized in:
- Release notes
- CONTRIBUTORS.md file
- GitHub's contributor graphs

Thank you for contributing to Isolate!
