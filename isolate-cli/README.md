# isolate-cli

[![Crates.io](https://img.shields.io/crates/v/isolate-cli.svg)](https://crates.io/crates/isolate-cli)
[![License](https://img.shields.io/crates/l/isolate-cli.svg)](../LICENSE-MIT)

Command-line interface for the Isolate secure sandbox runtime. Execute WASM modules with rich terminal output, progress indicators, and interactive capability selection.

## Installation

```bash
# From crates.io
cargo install isolate-cli

# From source
cargo install --path isolate-cli
```

## Quick Start

```bash
# Run a WASM module with stdout capability
isolate run module.wasm --cap-stdout

# Run with multiple capabilities and limits
isolate run module.wasm \
    --cap-stdio \
    --cap-fs-read /data \
    --memory-limit 128M \
    --fuel 1000000 \
    --timeout 30

# Interactive mode with capability prompts
isolate interactive module.wasm
```

## Commands

### `run` - Execute a WASM module

```bash
isolate run <MODULE> [OPTIONS]

Arguments:
  <MODULE>  Path to the WASM module

Options:
  -m, --memory-limit <SIZE>   Memory limit (e.g., 128M, 1G) [default: 256M]
  -f, --fuel <AMOUNT>         Fuel limit (instruction count)
  -t, --timeout <SECONDS>     Wall-clock timeout [default: 60]
      --cpu-time <SECONDS>    CPU time limit
      --cap-stdout            Grant stdout capability
      --cap-stderr            Grant stderr capability
      --cap-stdin             Grant stdin capability
      --cap-stdio             Grant all stdio capabilities
      --cap-fs-read <PATH>    Grant filesystem read access
      --cap-fs-write <PATH>   Grant filesystem write access
      --cap-http <HOST>       Grant HTTP access to host
      --cap-dns               Grant DNS resolution
      --cap-time              Grant system clock access
      --cap-random            Grant random number access
  -e, --env <KEY=VALUE>       Set environment variable
      --entry <FUNCTION>      Entry point function [default: _start]
      --stdin                 Read input from stdin
      --show-stats            Show resource usage after execution
```

**Examples:**

```bash
# Simple execution
isolate run hello.wasm --cap-stdout

# With environment variables
isolate run processor.wasm --cap-stdio --env API_KEY=secret --env DEBUG=1

# With filesystem access
isolate run analyzer.wasm --cap-fs-read /data --cap-fs-write /output

# With network access
isolate run fetcher.wasm --cap-http api.example.com --cap-dns

# Pipeline with stdin
echo '{"data": [1,2,3]}' | isolate run transform.wasm --cap-stdio --stdin
```

### `validate` - Validate a WASM module

```bash
isolate validate <MODULE>

# Example
isolate validate module.wasm
# ✓ Module is valid WASM
```

### `info` - Show module information

```bash
isolate info <MODULE> [OPTIONS]

Options:
      --exports    Show exported functions
      --imports    Show imported functions

# Example
isolate info module.wasm
# ┌─────────────┬──────────────────────────────────────┐
# │ File        │ module.wasm                          │
# │ Size        │ 12.34 KB                             │
# │ Hash        │ a1b2c3d4e5f6...                      │
# │ WASM Version│ 1                                    │
# └─────────────┴──────────────────────────────────────┘
```

### `benchmark` - Performance benchmarking

```bash
isolate benchmark <MODULE> [OPTIONS]

Options:
  -i, --iterations <N>    Number of iterations [default: 100]
      --warmup <N>        Warmup iterations [default: 10]
      --include-run       Include execution time (not just creation)

# Example
isolate benchmark module.wasm --iterations 1000
#
# Results
# ──────────────────────────────────────────────────────
# ┌──────────────┬──────────┐
# │ Percentile   │ Time     │
# ├──────────────┼──────────┤
# │ Min          │ 1.82ms   │
# │ p50 (Median) │ 2.14ms   │
# │ Average      │ 2.31ms   │
# │ p95          │ 3.42ms   │
# │ p99          │ 4.18ms   │
# │ Max          │ 5.67ms   │
# └──────────────┴──────────┘
#
#   Performance Rating: Excellent
#   ⚡ Sub-5ms cold start achieved!
```

### `interactive` - Interactive mode

Launch an interactive session with capability selection prompts:

```bash
isolate interactive module.wasm

# Select capabilities to grant:
# > [x] stdout - Write to standard output
# > [x] stderr - Write to standard error
# > [ ] stdin - Read from standard input
# > [ ] time - Access system clock
# > [ ] random - Secure random numbers
# > [ ] dns - DNS resolution
#
# Execute with these capabilities? [Y/n]
```

### `snapshot` - Snapshot management

```bash
isolate snapshot list              # List stored snapshots
isolate snapshot info <ID>         # Show snapshot details
isolate snapshot delete <ID>       # Delete a snapshot
```

## Global Options

```bash
Options:
  -l, --log-level <LEVEL>   Log level [default: warn]
                            Values: trace, debug, info, warn, error
  -f, --format <FORMAT>     Output format [default: pretty]
                            Values: text, json, pretty
      --no-color            Disable colored output
  -q, --quiet               Quiet mode - only output the result
  -h, --help                Print help
  -V, --version             Print version
```

## Output Formats

### Pretty (default)

Rich terminal output with colors, tables, and progress indicators.

### JSON

Machine-readable JSON output:

```bash
isolate run module.wasm --cap-stdout --format json
```

```json
{
  "exit_code": 0,
  "stdout": "Hello, World!\n",
  "stderr": "",
  "duration_ms": 1.234,
  "creation_time_ms": 2.567,
  "resource_usage": {
    "peak_memory": 1048576,
    "fuel_consumed": 12345,
    "cpu_time_ms": 0.892
  },
  "capabilities_granted": 1
}
```

### Text

Plain text output (stdout/stderr only, no formatting):

```bash
isolate run module.wasm --cap-stdout --format text
# Hello, World!
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | CLI error (invalid args, file not found) |
| N | WASM module exit code (passed through) |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Override log level (e.g., `isolate=debug`) |
| `NO_COLOR` | Disable colored output (any value) |

## Examples

### Data Processing Pipeline

```bash
# Process JSON data through a WASM transformer
cat data.json | isolate run transform.wasm --cap-stdio --stdin > output.json
```

### Serverless Function Testing

```bash
# Test a serverless function locally
isolate run function.wasm \
    --cap-stdio \
    --cap-http api.example.com \
    --env STAGE=development \
    --timeout 30 \
    --show-stats
```

### Batch Benchmarking

```bash
# Benchmark multiple modules
for module in modules/*.wasm; do
    echo "Benchmarking $module"
    isolate benchmark "$module" --iterations 100 --format json
done
```

### CI/CD Integration

```bash
# Validate and test modules in CI
isolate validate module.wasm || exit 1
isolate run module.wasm --cap-stdout --timeout 10 --quiet
```

## License

MIT OR Apache-2.0
