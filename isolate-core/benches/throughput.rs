//! Enhanced benchmarks: warm start, throughput, and concurrent execution.
//!
//! Run with: `cargo bench --package isolate-core --bench throughput`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use isolate_core::{capability::Capability, engine::WasmEngine, Sandbox, SandboxConfig};
use std::time::Duration;

const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

/// Benchmark warm start: reuse a shared WasmEngine across sandbox creations.
fn bench_warm_start(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("warm_start");
    group.measurement_time(Duration::from_secs(10));

    // Pre-create the engine with cached modules
    let engine = rt.block_on(async {
        let engine = WasmEngine::new().expect("engine creation");
        // Pre-compile module
        let config = SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();
        let _ = Sandbox::create(config).await.unwrap();
        engine
    });

    group.bench_function("minimal_reuse_engine", |b| {
        b.to_async(&rt).iter(|| async {
            let config =
                SandboxConfig::builder().module(black_box(MINIMAL_WASM)).unwrap().build().unwrap();
            black_box(Sandbox::create(config).await.unwrap())
        })
    });

    group.bench_function("hello_reuse_engine", |b| {
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

    drop(engine);
    group.finish();
}

/// Benchmark throughput: sandboxes created and executed per second.
fn bench_throughput(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("throughput");
    group.measurement_time(Duration::from_secs(15));

    for batch_size in [1u64, 5, 10].iter() {
        group.throughput(Throughput::Elements(*batch_size));

        group.bench_with_input(
            BenchmarkId::new("create_batch", batch_size),
            batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async move {
                    for _ in 0..size {
                        let config =
                            SandboxConfig::builder().module(MINIMAL_WASM).unwrap().build().unwrap();
                        black_box(Sandbox::create(config).await.unwrap());
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("create_and_run_batch", batch_size),
            batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async move {
                    for _ in 0..size {
                        let config = SandboxConfig::builder()
                            .module(HELLO_WASM)
                            .unwrap()
                            .capability(Capability::stdout())
                            .build()
                            .unwrap();
                        let mut sandbox = Sandbox::create(config).await.unwrap();
                        black_box(sandbox.run(&[]).await.unwrap());
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark concurrent sandbox execution using tokio tasks.
fn bench_concurrent_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut group = c.benchmark_group("concurrent");
    group.measurement_time(Duration::from_secs(15));

    for concurrency in [2u64, 4, 8].iter() {
        group.throughput(Throughput::Elements(*concurrency));

        group.bench_with_input(
            BenchmarkId::new("parallel_create", concurrency),
            concurrency,
            |b, &n| {
                b.to_async(&rt).iter(|| async move {
                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            tokio::spawn(async {
                                let config = SandboxConfig::builder()
                                    .module(MINIMAL_WASM)
                                    .unwrap()
                                    .build()
                                    .unwrap();
                                Sandbox::create(config).await.unwrap()
                            })
                        })
                        .collect();

                    for h in handles {
                        black_box(h.await.unwrap());
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("parallel_run", concurrency),
            concurrency,
            |b, &n| {
                b.to_async(&rt).iter(|| async move {
                    let handles: Vec<_> = (0..n)
                        .map(|_| {
                            tokio::spawn(async {
                                let config = SandboxConfig::builder()
                                    .module(HELLO_WASM)
                                    .unwrap()
                                    .capability(Capability::stdout())
                                    .build()
                                    .unwrap();
                                let mut sandbox = Sandbox::create(config).await.unwrap();
                                sandbox.run(&[]).await.unwrap()
                            })
                        })
                        .collect();

                    for h in handles {
                        black_box(h.await.unwrap());
                    }
                })
            },
        );
    }

    group.finish();
}

/// Benchmark configuration construction overhead.
fn bench_config_variations(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_variations");

    group.bench_function("capabilities_1", |b| {
        b.iter(|| {
            black_box(
                SandboxConfig::builder()
                    .module(MINIMAL_WASM)
                    .unwrap()
                    .capability(Capability::stdout())
                    .build()
                    .unwrap(),
            )
        })
    });

    group.bench_function("capabilities_5", |b| {
        b.iter(|| {
            black_box(
                SandboxConfig::builder()
                    .module(MINIMAL_WASM)
                    .unwrap()
                    .capability(Capability::stdout())
                    .capability(Capability::stderr())
                    .capability(Capability::filesystem_read("/tmp"))
                    .capability(Capability::env_all())
                    .capability(Capability::system_clock())
                    .build()
                    .unwrap(),
            )
        })
    });

    group.bench_function("env_vars_10", |b| {
        b.iter(|| {
            let mut builder = SandboxConfig::builder().module(MINIMAL_WASM).unwrap();
            for i in 0..10 {
                builder = builder.env(format!("KEY_{}", i), format!("value_{}", i));
            }
            black_box(builder.build().unwrap())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_warm_start,
    bench_throughput,
    bench_concurrent_execution,
    bench_config_variations,
);

criterion_main!(benches);
