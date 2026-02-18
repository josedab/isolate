# Makefile — thin wrapper for contributors without `just` installed.
# Delegates to `cargo xtask` for the most common development commands.
# For the full task list, run: cargo xtask help

.PHONY: check test fmt lint doctor

check: ## Run all checks (fmt, clippy, test)
	cargo xtask check

test: ## Run tests (default members)
	cargo xtask test

fmt: ## Format all code
	cargo xtask fmt

lint: ## Run clippy lints
	cargo xtask lint

doctor: ## Verify development environment
	cargo xtask doctor

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'
