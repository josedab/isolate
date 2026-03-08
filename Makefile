# Makefile — thin wrapper for contributors without `just` installed.
# For the full task list (37+ targets), install `just` and run: just --list
# See: https://github.com/casey/just
#
# This Makefile provides the most common commands. For benchmarks, coverage,
# security audits, and more, use the justfile directly.

.PHONY: check test fmt lint doctor build pre-commit clean

check: ## Run all checks (fmt, clippy, test)
	cargo xtask check

test: ## Run tests (default members)
	cargo xtask test

test-all: ## Run tests with all features
	cargo test --workspace

fmt: ## Format all code
	cargo xtask fmt

lint: ## Run clippy lints
	cargo xtask lint

doctor: ## Verify development environment
	cargo xtask doctor

build: ## Build all default crates
	cargo build

build-release: ## Build optimized release binary
	cargo build --release --package isolate-server

pre-commit: ## Run full pre-commit validation
	cargo xtask pre-commit

clean: ## Remove build artifacts
	cargo clean

help: ## Show available targets
	@echo "Common targets (for full list: just --list)"
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
