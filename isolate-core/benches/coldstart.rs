//! Benchmarks for cold start optimization: PrecompileCache vs fresh compilation.
//!
//! Run with: `cargo bench --package isolate-core --bench coldstart`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use isolate_core::coldstart::PrecompileCache;
use isolate_core::config::WasmModule;
use isolate_core::engine::WasmEngine;
use std::time::Duration;
use tempfile::TempDir;

const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// Benchmark fresh compilation (no cache).
fn bench_fresh_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("coldstart_fresh");
    group.measurement_time(Duration::from_secs(5));

    let engine = WasmEngine::new().unwrap();

    group.bench_function("minimal_module", |b| {
        let module = WasmModule::from_bytes(MINIMAL_WASM.to_vec()).unwrap();
        b.iter(|| {
            engine.clear_cache();
            black_box(engine.compile(&module).unwrap());
        })
    });

    group.bench_function("hello_module", |b| {
        let module = WasmModule::from_bytes(HELLO_WASM.to_vec()).unwrap();
        b.iter(|| {
            engine.clear_cache();
            black_box(engine.compile(&module).unwrap());
        })
    });

    group.finish();
}

/// Benchmark cached load (pre-compiled AOT modules from disk).
fn bench_cached_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("coldstart_cached");
    group.measurement_time(Duration::from_secs(5));

    let temp_dir = TempDir::new().unwrap();
    let cache = PrecompileCache::new(temp_dir.path().to_path_buf(), 100).unwrap();
    let engine = WasmEngine::new().unwrap();

    // Populate cache
    let minimal_hash = cache.precompile(&engine, MINIMAL_WASM).unwrap();
    let hello_hash = cache.precompile(&engine, HELLO_WASM).unwrap();

    group.bench_function("minimal_module", |b| {
        b.iter(|| {
            black_box(cache.load(&engine, &minimal_hash).unwrap().unwrap());
        })
    });

    group.bench_function("hello_module", |b| {
        b.iter(|| {
            black_box(cache.load(&engine, &hello_hash).unwrap().unwrap());
        })
    });

    group.finish();
}

/// Benchmark cache store (precompile + write to disk).
fn bench_cache_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("coldstart_store");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("minimal_module", |b| {
        let temp_dir = TempDir::new().unwrap();
        let cache = PrecompileCache::new(temp_dir.path().to_path_buf(), 100).unwrap();
        let engine = WasmEngine::new().unwrap();

        b.iter(|| {
            black_box(cache.precompile(&engine, MINIMAL_WASM).unwrap());
        })
    });

    group.bench_function("hello_module", |b| {
        let temp_dir = TempDir::new().unwrap();
        let cache = PrecompileCache::new(temp_dir.path().to_path_buf(), 100).unwrap();
        let engine = WasmEngine::new().unwrap();

        b.iter(|| {
            black_box(cache.precompile(&engine, HELLO_WASM).unwrap());
        })
    });

    group.finish();
}

/// Direct comparison: fresh compile vs cached deserialize for the same module.
fn bench_fresh_vs_cached(c: &mut Criterion) {
    let mut group = c.benchmark_group("coldstart_comparison");
    group.measurement_time(Duration::from_secs(5));

    let engine = WasmEngine::new().unwrap();
    let temp_dir = TempDir::new().unwrap();
    let cache = PrecompileCache::new(temp_dir.path().to_path_buf(), 100).unwrap();
    let hello_hash = cache.precompile(&engine, HELLO_WASM).unwrap();
    let hello_module = WasmModule::from_bytes(HELLO_WASM.to_vec()).unwrap();

    group.bench_function("hello_fresh_compile", |b| {
        b.iter(|| {
            engine.clear_cache();
            black_box(engine.compile(&hello_module).unwrap());
        })
    });

    group.bench_function("hello_cached_load", |b| {
        b.iter(|| {
            black_box(cache.load(&engine, &hello_hash).unwrap().unwrap());
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fresh_compilation,
    bench_cached_load,
    bench_cache_store,
    bench_fresh_vs_cached
);
criterion_main!(benches);
