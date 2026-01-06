//! Benchmarks for sandbox creation and execution.
//!
//! Run with: `cargo bench --package isolate-core`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use isolate_core::{capability::Capability, Sandbox, SandboxConfig};
use std::time::Duration;

// Test fixtures
const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// Benchmark cold start time (sandbox creation from WASM bytes).
fn bench_cold_start(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("cold_start");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("minimal_module", |b| {
        b.to_async(&rt).iter(|| async {
            let config = SandboxConfig::builder()
                .module(black_box(MINIMAL_WASM))
                .unwrap()
                .build()
                .unwrap();
            black_box(Sandbox::create(config).await.unwrap())
        })
    });

    group.bench_function("hello_module", |b| {
        b.to_async(&rt).iter(|| async {
            let config = SandboxConfig::builder()
                .module(black_box(HELLO_WASM))
                .unwrap()
                .capability(Capability::stdout())
                .build()
                .unwrap();
            black_box(Sandbox::create(config).await.unwrap())
        })
    });

    group.finish();
}

/// Benchmark sandbox execution.
fn bench_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("execution");
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("hello_world", |b| {
        b.to_async(&rt).iter(|| async {
            let config = SandboxConfig::builder()
                .module(HELLO_WASM)
                .unwrap()
                .capability(Capability::stdout())
                .capability(Capability::stderr())
                .build()
                .unwrap();
            let mut sandbox = Sandbox::create(config).await.unwrap();
            black_box(sandbox.run(&[]).await.unwrap())
        })
    });

    group.finish();
}

/// Benchmark config building with various options.
fn bench_config_building(c: &mut Criterion) {
    let mut group = c.benchmark_group("config");

    group.bench_function("minimal_config", |b| {
        b.iter(|| {
            black_box(
                SandboxConfig::builder()
                    .module(MINIMAL_WASM)
                    .unwrap()
                    .build()
                    .unwrap(),
            )
        })
    });

    group.bench_function("full_config", |b| {
        b.iter(|| {
            black_box(
                SandboxConfig::builder()
                    .module(MINIMAL_WASM)
                    .unwrap()
                    .memory_limit(128 * 1024 * 1024)
                    .fuel(1_000_000)
                    .wall_time_limit(Duration::from_secs(30))
                    .capability(Capability::stdout())
                    .capability(Capability::stderr())
                    .capability(Capability::filesystem_read("/tmp"))
                    .env("KEY1", "value1")
                    .env("KEY2", "value2")
                    .arg("--verbose".to_string())
                    .build()
                    .unwrap(),
            )
        })
    });

    group.finish();
}

/// Benchmark with different memory limits.
fn bench_memory_limits(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("memory_limits");
    group.measurement_time(Duration::from_secs(10));

    for size_mb in [16, 64, 128, 256].iter() {
        let size_bytes = size_mb * 1024 * 1024;
        group.throughput(Throughput::Bytes(*size_mb as u64 * 1024 * 1024));

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}MB", size_mb)),
            &size_bytes,
            |b, &size| {
                b.to_async(&rt).iter(|| async move {
                    let config = SandboxConfig::builder()
                        .module(MINIMAL_WASM)
                        .unwrap()
                        .memory_limit(size)
                        .build()
                        .unwrap();
                    black_box(Sandbox::create(config).await.unwrap())
                })
            },
        );
    }

    group.finish();
}

/// Benchmark with different fuel limits.
fn bench_fuel_limits(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("fuel_limits");
    group.measurement_time(Duration::from_secs(10));

    for fuel in [100_000u64, 1_000_000, 10_000_000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(fuel), fuel, |b, &fuel| {
            b.to_async(&rt).iter(|| async move {
                let config = SandboxConfig::builder()
                    .module(HELLO_WASM)
                    .unwrap()
                    .fuel(fuel)
                    .capability(Capability::stdout())
                    .build()
                    .unwrap();
                let mut sandbox = Sandbox::create(config).await.unwrap();
                black_box(sandbox.run(&[]).await.unwrap())
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_cold_start,
    bench_execution,
    bench_config_building,
    bench_memory_limits,
    bench_fuel_limits,
);

criterion_main!(benches);
