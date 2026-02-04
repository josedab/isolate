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

# Run tests with verbose output
test-verbose:
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

# Run clippy lints
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with pedantic lints (informational, may have many warnings)
lint-pedantic:
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

# Build debug
build:
    cargo build --all-features --workspace

# Build release
build-release:
    cargo build --release --all-features --workspace

# Clean build artifacts
clean:
    cargo clean

# Generate documentation
docs:
    cargo doc --no-deps --all-features --workspace

# Generate and open documentation
docs-open:
    cargo doc --no-deps --all-features --workspace --open

# Run benchmarks
bench:
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

# Run code coverage
coverage:
    cargo tarpaulin --workspace --all-features --out html --skip-clean
    @echo "Coverage report generated: tarpaulin-report.html"

# Check MSRV compatibility
msrv:
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

# Check compilation without building
check-compile:
    cargo check --all-targets --all-features --workspace

# Verify development environment is set up correctly
doctor:
    @echo "🔍 Checking development environment..."
    @echo ""
    @echo "Rust toolchain:"
    @rustc --version
    @cargo --version
    @echo ""
    @echo "Checking compilation..."
    @cargo check --all-features --workspace 2>/dev/null && echo "✅ Compilation OK" || echo "❌ Compilation failed"
    @echo ""
    @echo "Checking formatting..."
    @cargo fmt --all -- --check 2>/dev/null && echo "✅ Formatting OK" || echo "❌ Run 'just fmt' to fix formatting"
    @echo ""
    @echo "Running quick tests (core only)..."
    @cargo test --package isolate-core -q 2>/dev/null && echo "✅ Core tests OK" || echo "❌ Core tests failed"
    @echo ""
    @echo "🏁 Environment check complete!"
