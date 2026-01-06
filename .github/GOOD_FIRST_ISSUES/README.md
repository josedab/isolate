# Good First Issues

This directory contains pre-written good first issues ready to be created on GitHub.
Each file describes a task suitable for new contributors.

## Creating Issues

To create these issues on GitHub:

1. Go to https://github.com/josedab/isolate/issues/new/choose
2. Select "Good First Issue" template
3. Copy the content from the appropriate file below
4. Submit the issue

## Available Issues

| Issue | Difficulty | Files |
|-------|------------|-------|
| [Add --json flag to CLI](001-cli-json-output.md) | Easy | isolate-cli/src/main.rs |
| [Add doc comments to Capability](002-capability-docs.md) | Easy | isolate-core/src/capability/types.rs |
| [Implement Display for SandboxState](003-sandbox-state-display.md) | Easy | isolate-core/src/sandbox.rs |
| [Add capability checking benchmark](004-capability-benchmark.md) | Medium | isolate-core/benches/ |
| [Add WASI call tracing](005-wasi-tracing.md) | Medium | isolate-core/src/engine/wasm.rs |

## Guidelines for Good First Issues

- Well-scoped with clear acceptance criteria
- Not time-sensitive or blocking other work
- Educational about the codebase
- Include pointers to relevant files and documentation
