# Fuzz Corpus

This directory contains seed files for fuzz testing. Each subdirectory corresponds
to a fuzz target in `fuzz/fuzz_targets/`.

## Directory Structure

```
corpus/
├── fuzz_wasm_module/         # Seeds for WASM module parsing (14 seeds)
├── fuzz_config/              # Seeds for configuration fuzzing (5 seeds)
├── fuzz_capability_parsing/  # Seeds for capability system (5 seeds)
└── fuzz_sandbox_input/       # Seeds for sandbox input handling (8 seeds)
```

## Seed File Types

### fuzz_wasm_module/
- `minimal_valid.wasm` - Minimal valid WASM module (magic + version)
- `valid_custom_section.wasm` - Valid header + custom section
- `empty_type_section.wasm` - Valid header + empty type section
- `memory_section.wasm` - Valid header + memory section
- `empty.bin` - Empty file (should be rejected)
- `wrong_version.wasm` - WASM with invalid version
- `version_2.wasm` - Future WASM version 2
- `invalid_magic.bin` - File with invalid magic bytes
- `reversed_magic.bin` - Reversed magic bytes
- `magic_only.wasm` - Only magic bytes, no version
- `invalid_section_id.wasm` - Invalid section ID 0xFF
- `truncated_section.wasm` - Truncated LEB128 section length
- `large_padded.wasm` - 64KB padded module
- `all_ff.bin` - All 0xFF bytes (pure garbage)

### fuzz_sandbox_input/
- `ascii_hello.bin` - Simple ASCII text
- `null_bytes.bin` - 64 null bytes
- `large_ascii.bin` - 4KB of repeated characters
- `binary_pattern.bin` - All 256 byte values
- `utf8_multibyte.bin` - UTF-8 multibyte characters
- `mixed_newlines.bin` - Mixed line endings
- `long_line.bin` - 64KB single line

### fuzz_config/
- `small_values.bin` - Minimal valid parameters
- `max_values.bin` - Maximum u64 values
- `boundary_values.bin` - WASM page boundary values
- `with_capabilities.bin` - Config with capability flags

### fuzz_capability_parsing/
- `grant_revoke.bin` - Grant and revoke operations
- `long_paths.bin` - Long filesystem paths
- `unicode_paths.bin` - Unicode characters in paths
- `many_ops.bin` - Many sequential operations

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
