# Fuzz Corpus

This directory contains seed files for fuzz testing. Each subdirectory corresponds
to a fuzz target in `fuzz/fuzz_targets/`.

## Directory Structure

```
corpus/
├── fuzz_wasm_module/     # Seeds for WASM module parsing
├── fuzz_config/          # Seeds for configuration fuzzing
├── fuzz_capability_parsing/  # Seeds for capability system
└── fuzz_sandbox_input/   # Seeds for sandbox input handling
```

## Seed File Types

### fuzz_wasm_module/
- `minimal_valid.wasm` - Minimal valid WASM module (magic + version)
- `empty.bin` - Empty file (should be rejected)
- `wrong_version.wasm` - WASM with invalid version
- `invalid_magic.bin` - File with invalid magic bytes

### fuzz_config/, fuzz_capability_parsing/, fuzz_sandbox_input/
These targets use the `arbitrary` crate which processes raw bytes. The seed
files provide starting points for the fuzzer to mutate.

## Running Fuzz Tests

```bash
# Run a specific fuzz target
cargo +nightly fuzz run fuzz_wasm_module

# Run with corpus
cargo +nightly fuzz run fuzz_wasm_module -- -max_len=65536

# Run for a limited time
cargo +nightly fuzz run fuzz_wasm_module -- -max_total_time=60
```

## Adding New Seeds

When adding seeds, consider:
1. Valid inputs (happy path)
2. Invalid inputs (error handling)
3. Edge cases (boundaries, empty, maximum values)
4. Previously-found crash inputs (regression prevention)
