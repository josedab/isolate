# Examples

Runnable examples demonstrating Isolate's key features.

## Quick Start

```bash
# Run the basic hello world sandbox
cargo run --example hello_sandbox --package isolate-core

# Run with resource limits
cargo run --example resource_limits --package isolate-core

# Run with capability-based security
cargo run --example capabilities --package isolate-core
```

## Available Examples

| Example | Description |
|---------|-------------|
| [`hello_sandbox`](../isolate-core/examples/hello_sandbox.rs) | Minimal sandbox creation and execution |
| [`basic_sandbox`](../isolate-core/examples/basic_sandbox.rs) | Basic sandbox with configuration |
| [`basic`](../isolate-core/examples/basic.rs) | Simplest possible sandbox usage |
| [`resource_limits`](../isolate-core/examples/resource_limits.rs) | Memory, fuel, and timeout enforcement |
| [`capabilities`](../isolate-core/examples/capabilities.rs) | Fine-grained capability grants |
| [`capability_guide`](../isolate-core/examples/capability_guide.rs) | Comprehensive capability walkthrough |
| [`error_handling`](../isolate-core/examples/error_handling.rs) | Proper error handling patterns |
| [`common_errors`](../isolate-core/examples/common_errors.rs) | Common error scenarios and recovery |
| [`multi_sandbox`](../isolate-core/examples/multi_sandbox.rs) | Running multiple sandboxes concurrently |
| [`real_execution`](../isolate-core/examples/real_execution.rs) | Real-world execution patterns |

## Running Examples

All examples live in [`isolate-core/examples/`](../isolate-core/examples/) and can be run with:

```bash
cargo run --example <name> --package isolate-core
```
