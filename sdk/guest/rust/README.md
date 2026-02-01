# Isolate Guest SDK — Rust

Write WASM modules for Isolate sandboxes in Rust.

## Quick Start

### Prerequisites

- Rust toolchain with the `wasm32-wasip1` target:

```bash
rustup target add wasm32-wasip1
```

### Project Setup

Create a new project and add the guest SDK dependency:

```bash
cargo init --lib my-guest-module
cd my-guest-module
```

Set the crate type in `Cargo.toml`:

```toml
[package]
name = "my-guest-module"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Writing a Guest Module

```rust
use serde::{Deserialize, Serialize};

// Define your input/output types
#[derive(Deserialize)]
struct Input {
    name: String,
}

#[derive(Serialize)]
struct Output {
    greeting: String,
}

// The guest entry point — called by the Isolate runtime
#[no_mangle]
pub extern "C" fn _start() {
    isolate_guest_rust::guest_main(|input: Input| {
        Ok(Output {
            greeting: format!("Hello, {}!", input.name),
        })
    });
}
```

### Building

```bash
cargo build --target wasm32-wasip1 --release
```

The compiled module will be at `target/wasm32-wasip1/release/my_guest_module.wasm`.

### Running

```bash
echo '{"name": "World"}' | isolate run \
    --capability stdout \
    --capability stdin \
    target/wasm32-wasip1/release/my_guest_module.wasm
# Output: {"greeting":"Hello, World!"}
```

## API Reference

### JSON I/O

```rust
use isolate_guest_rust::{GuestInput, GuestOutput};

// Read JSON input from stdin
let input = GuestInput::read::<MyInput>().expect("valid input");

// Write JSON output to stdout
GuestOutput::write(&my_output).expect("write output");
```

### Environment Variables

```rust
use std::env;

// Access environment variables (requires env capability)
let value = env::var("MY_VAR").ok();
```

### Filesystem Access

```rust
use std::fs;

// Read files (requires filesystem_read capability)
let data = fs::read_to_string("/data/config.json").expect("read file");

// Write files (requires filesystem_write capability)
fs::write("/output/result.txt", "done").expect("write file");
```

### Logging

```rust
use isolate_guest_rust::{log_info, log_warn, log_error};

log_info("processing started");
log_warn("input missing optional field");
log_error("failed to parse config");
```

### Error Handling

```rust
use isolate_guest_rust::GuestError;

// Errors are reported via exit code and stderr
fn process(input: Input) -> Result<Output, GuestError> {
    if input.name.is_empty() {
        return Err(GuestError::new("name must not be empty"));
    }
    Ok(Output { greeting: format!("Hello, {}!", input.name) })
}
```

## Examples

See [`isolate-core/tests/fixtures/`](../../../isolate-core/tests/fixtures/) for
compiled WASM examples used in integration tests.
