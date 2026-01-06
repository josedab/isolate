//! Benchmarks for the WASM engine.
//!
//! Run with: `cargo bench --package isolate-core --bench engine`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use isolate_core::config::{ModuleHash, WasmModule};
use std::time::Duration;

// Test fixtures
const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// Benchmark module hash computation.
fn bench_module_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_hash");

    group.bench_function("minimal_wasm", |b| {
        b.iter(|| black_box(ModuleHash::from_bytes(MINIMAL_WASM)))
    });

    group.bench_function("hello_wasm", |b| {
        b.iter(|| black_box(ModuleHash::from_bytes(HELLO_WASM)))
    });

    // Benchmark with larger synthetic modules
    let large_wasm = create_padded_wasm(1024 * 1024); // 1MB
    group.bench_function("1mb_module", |b| {
        b.iter(|| black_box(ModuleHash::from_bytes(&large_wasm)))
    });

    group.finish();
}

/// Benchmark WASM module validation.
fn bench_module_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_validation");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("minimal_wasm", |b| {
        b.iter(|| black_box(WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap()))
    });

    group.bench_function("hello_wasm", |b| {
        b.iter(|| black_box(WasmModule::from_bytes(HELLO_WASM.to_vec()).unwrap()))
    });

    // Invalid WASM should fail fast
    let invalid_wasm = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    group.bench_function("invalid_wasm_rejection", |b| {
        b.iter(|| {
            let result = WasmModule::from_bytes(invalid_wasm.clone());
            black_box(result.is_err())
        })
    });

    group.finish();
}

/// Create a padded WASM module of approximately the given size.
fn create_padded_wasm(target_size: usize) -> Vec<u8> {
    // Start with minimal valid WASM
    let mut wasm = vec![
        0x00, 0x61, 0x73, 0x6d, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ];

    // Add a custom section with padding to reach target size
    if target_size > wasm.len() + 10 {
        let padding_size = target_size - wasm.len() - 10;

        // Custom section header
        wasm.push(0x00); // section id (custom)

        // Calculate LEB128 encoding for section size
        let section_size = padding_size + 5; // name length + name + padding
        let mut size_bytes = Vec::new();
        let mut size = section_size;
        loop {
            let mut byte = (size & 0x7f) as u8;
            size >>= 7;
            if size != 0 {
                byte |= 0x80;
            }
            size_bytes.push(byte);
            if size == 0 {
                break;
            }
        }
        wasm.extend_from_slice(&size_bytes);

        // Custom section name
        wasm.push(0x04); // name length
        wasm.extend_from_slice(b"pad\0"); // name

        // Padding data
        wasm.resize(wasm.len() + padding_size, 0x00);
    }

    wasm
}

criterion_group!(benches, bench_module_hash, bench_module_validation,);

criterion_main!(benches);
