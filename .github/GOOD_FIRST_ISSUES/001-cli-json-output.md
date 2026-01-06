# Add --json Output Flag to CLI

## Task Description

Add a `--json` flag to the CLI that outputs sandbox execution results in JSON format.
This enables scripting and integration with other tools.

## Background Context

The CLI is built using clap with derive macros. Currently, the output is human-readable
text. Adding JSON output would make it easier to:
- Parse results in shell scripts
- Integrate with CI/CD pipelines
- Build tooling on top of isolate

The JSON output should include:
- Exit code
- stdout/stderr (base64 encoded if binary)
- Resource usage
- Execution duration

## Files to Modify

- `isolate-cli/src/main.rs` - Add flag and output formatting
- `isolate-cli/Cargo.toml` - Add serde_json if not already present

## Acceptance Criteria

- [ ] `isolate run --json module.wasm` outputs valid JSON
- [ ] JSON includes exit_code, stdout, stderr, duration
- [ ] JSON includes resource_usage when available
- [ ] Existing human-readable output unchanged without --json
- [ ] Tests added for JSON output
- [ ] CLI help updated

## Example Output

```json
{
  "exit_code": 0,
  "stdout": "Hello, World!\n",
  "stderr": "",
  "duration_ms": 12,
  "resource_usage": {
    "fuel_consumed": 42000,
    "memory_peak_bytes": 1048576
  }
}
```

## Helpful Resources

- Clap derive documentation: https://docs.rs/clap/latest/clap/_derive/
- serde_json: https://docs.rs/serde_json/
- See `Output` struct in `isolate-core/src/sandbox.rs`

## Estimated Difficulty

Easy (< 1 hour)
