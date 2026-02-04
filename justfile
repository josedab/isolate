# Justfile for Isolate
# https://github.com/casey/just
#
# Run `just --list` to see all available recipes

# Default recipe - show help
default:
    @just --list

# Run all checks (format, lint, test)
check: fmt-check lint test-default
    @echo "All checks passed!"

# Run tests (default members only, no extra features required)
test-default:
    cargo test

# Run tests (all workspace crates and features; requires Python dev headers)
test-all:
    cargo test --all-features --workspace

# Run tests with verbose output (default members only)
test-verbose:
    cargo test -- --nocapture

# Run tests with verbose output (all workspace crates; requires Python dev headers)
test-verbose-all:
    cargo test --all-features --workspace -- --nocapture

# Run tests in release mode
test-release:
    cargo test --all-features --workspace --release

# Run specific test
test-one TEST:
    cargo test --all-features --workspace {{TEST}}

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Format code
fmt:
    cargo fmt --all

# Run clippy lints (default members only)
lint:
    cargo clippy --all-targets -- -D warnings

# Run clippy lints (all workspace crates; requires Python dev headers)
lint-all:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with pedantic lints (informational, may have many warnings)
lint-pedantic:
    cargo clippy --all-targets -- \
        -W clippy::pedantic \
        -W clippy::nursery \
        -A clippy::module_name_repetitions \
        -A clippy::must_use_candidate \
        -A clippy::missing_errors_doc \
        -A clippy::missing_panics_doc \
        -A clippy::redundant_pub_crate \
        -A clippy::significant_drop_tightening \
        -A clippy::future_not_send \
        -A clippy::useless_vec

# Run clippy with pedantic lints for all crates (requires Python dev headers)
lint-pedantic-all:
    cargo clippy --all-targets --all-features -- \
        -W clippy::pedantic \
        -W clippy::nursery \
        -A clippy::module_name_repetitions \
        -A clippy::must_use_candidate \
        -A clippy::missing_errors_doc \
        -A clippy::missing_panics_doc \
        -A clippy::redundant_pub_crate \
        -A clippy::significant_drop_tightening \
        -A clippy::future_not_send \
        -A clippy::useless_vec

# Build debug (default members only)
build:
    cargo build

# Build debug (all workspace crates; requires Python dev headers)
build-all:
    cargo build --all-features --workspace

# Build release (default members only)
build-release:
    cargo build --release

# Build release (all workspace crates; requires Python dev headers)
build-release-all:
    cargo build --release --all-features --workspace

# Clean build artifacts
clean:
    cargo clean

# Generate documentation (default members only)
docs:
    cargo doc --no-deps

# Generate documentation (all workspace crates; requires Python dev headers)
docs-all:
    cargo doc --no-deps --all-features --workspace

# Generate and open documentation (default members only)
docs-open:
    cargo doc --no-deps --open

# Generate and open documentation (all workspace crates; requires Python dev headers)
docs-open-all:
    cargo doc --no-deps --all-features --workspace --open

# Run benchmarks (default members only)
bench:
    cargo bench

# Run benchmarks (all workspace crates; requires Python dev headers)
bench-all:
    cargo bench --workspace --all-features

# Run security audit
audit:
    cargo audit

# Run cargo deny checks
deny:
    cargo deny check

# Run all security checks
security: audit deny
    @echo "Security checks passed!"

# Run code coverage (default members only)
coverage:
    cargo tarpaulin --out html --skip-clean
    @echo "Coverage report generated: tarpaulin-report.html"

# Run code coverage (all workspace crates; requires Python dev headers)
coverage-all:
    cargo tarpaulin --workspace --all-features --out html --skip-clean
    @echo "Coverage report generated: tarpaulin-report.html"

# Check MSRV compatibility (default members only)
msrv:
    cargo +1.75.0 check

# Check MSRV compatibility (all workspace crates; requires Python dev headers)
msrv-all:
    cargo +1.75.0 check --all-features --workspace

# Run the CLI tool
run *ARGS:
    cargo run --package isolate-cli -- {{ARGS}}

# Run the gRPC server
serve *ARGS:
    cargo run --package isolate-server -- {{ARGS}}

# Install the CLI locally
install:
    cargo install --path isolate-cli

# Create a new release (dry run)
release-dry VERSION:
    @echo "Would release version {{VERSION}}"
    @echo "Current version in Cargo.toml:"
    @grep "^version" Cargo.toml | head -1

# Update dependencies
update:
    cargo update

# Check for outdated dependencies
outdated:
    cargo outdated --workspace

# Run pre-commit checks (useful before pushing)
pre-commit: fmt-check lint test-default
    @echo "Pre-commit checks passed! Ready to push."

# Watch for changes and run tests
watch:
    cargo watch -x "test"

# Watch for changes and run clippy
watch-lint:
    cargo watch -x "clippy --all-targets --all-features"

# Generate dependency tree
tree:
    cargo tree --workspace

# Check compilation without building (default members only)
check-compile:
    cargo check --all-targets

# Check compilation without building (all workspace crates; requires Python dev headers)
check-compile-all:
    cargo check --all-targets --all-features --workspace

# Verify development environment is set up correctly
doctor:
    cargo xtask doctor
