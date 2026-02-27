//! Minimal example using inline WAT (WebAssembly Text Format).
//!
//! This example compiles WASM from source at build time — no external `.wasm`
//! files needed. Run with:
//!
//! ```bash
//! cargo run --package isolate-core --example hello_inline
//! ```

use isolate_core::{capability::Capability, Sandbox, SandboxConfig};

#[tokio::main]
async fn main() -> isolate_core::Result<()> {
    // Compile a minimal WASM module inline using WAT text format.
    // This module exports `_start` which writes "Hello from inline WAT!\n" to fd 1 (stdout).
    let wasm_bytes = wat::parse_str(
        r#"
        (module
          (import "wasi_snapshot_preview1" "fd_write"
            (func $fd_write (param i32 i32 i32 i32) (result i32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "Hello from inline WAT!\n")
          (func (export "_start")
            ;; iovec at offset 100: pointer=0, length=23
            (i32.store (i32.const 100) (i32.const 0))
            (i32.store (i32.const 104) (i32.const 23))
            ;; fd_write(fd=1, iovs=100, iovs_len=1, nwritten=200)
            (drop (call $fd_write (i32.const 1) (i32.const 100) (i32.const 1) (i32.const 200)))
          )
        )
        "#,
    )
    .expect("WAT should parse");

    let config = SandboxConfig::builder()
        .module(&wasm_bytes)?
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .build()?;

    let mut sandbox = Sandbox::create(config).await?;
    let output = sandbox.run(&[]).await?;

    println!("Exit code: {}", output.exit_code);
    println!("Output: {}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}
