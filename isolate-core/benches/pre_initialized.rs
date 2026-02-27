//! Benchmarks for PreInitializedPool warm-start vs cold-start.
//!
//! Compares cold-start (full Sandbox::create) against warm-start
//! (PreInitializedPool::try_instantiate with pre-linked InstancePre).
//!
//! Run with: `cargo bench --package isolate-core --bench pre_initialized`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use isolate_core::capability::{Capability, CapabilityEnforcer, CapabilitySet};
use isolate_core::config::SandboxConfig;
use isolate_core::engine::{PreInitConfig, PreInitializedPool, WasmEngine};
use isolate_core::resource::{ResourceLimits, ResourceMeter};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../tests/fixtures/hello.wasm");

fn make_config(wasm: &[u8]) -> SandboxConfig {
    SandboxConfig::builder()
        .module(wasm)
        .expect("valid module")
        .fuel(1_000_000)
        .capability(Capability::stdout())
        .capability(Capability::stderr())
        .build()
        .expect("valid config")
}

/// Benchmark cold-start: full Sandbox::create (compile + link + instantiate).
fn bench_cold_start(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("instantiation_cold");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("minimal_sandbox_create", |b| {
        b.iter(|| {
            let config = make_config(MINIMAL_WASM);
            rt.block_on(async {
                let sandbox = isolate_core::Sandbox::create(config).await.unwrap();
                black_box(sandbox);
            });
        })
    });

    group.bench_function("hello_sandbox_create", |b| {
        b.iter(|| {
            let config = make_config(HELLO_WASM);
            rt.block_on(async {
                let sandbox = isolate_core::Sandbox::create(config).await.unwrap();
                black_box(sandbox);
            });
        })
    });

    group.finish();
}

/// Benchmark warm-start: PreInitializedPool::try_instantiate (pre-linked).
fn bench_warm_start(c: &mut Criterion) {
    let engine = Arc::new(WasmEngine::new().unwrap());
    let pool = PreInitializedPool::new(engine.clone(), PreInitConfig::default());

    let config_minimal = make_config(MINIMAL_WASM);
    let config_hello = make_config(HELLO_WASM);

    // Pre-warm both modules
    pool.pre_warm(&config_minimal).unwrap();
    pool.pre_warm(&config_hello).unwrap();

    let mut group = c.benchmark_group("instantiation_warm");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("minimal_pool_instantiate", |b| {
        b.iter(|| {
            let caps = CapabilitySet::default();
            let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
            let meter = ResourceMeter::new(ResourceLimits::default());
            let inst = pool.try_instantiate(&config_minimal, enforcer, meter, None).unwrap();
            black_box(inst);
        })
    });

    group.bench_function("hello_pool_instantiate", |b| {
        b.iter(|| {
            let mut caps = CapabilitySet::default();
            caps.grant(Capability::stdout());
            let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
            let meter = ResourceMeter::new(ResourceLimits::default());
            let inst = pool.try_instantiate(&config_hello, enforcer, meter, None).unwrap();
            black_box(inst);
        })
    });

    group.finish();
}

/// Direct comparison: cold vs warm for the same module.
fn bench_cold_vs_warm(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let engine = Arc::new(WasmEngine::new().unwrap());
    let pool = PreInitializedPool::new(engine.clone(), PreInitConfig::default());

    let config = make_config(HELLO_WASM);
    pool.pre_warm(&config).unwrap();

    let mut group = c.benchmark_group("instantiation_comparison");
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("hello_cold_start", |b| {
        b.iter(|| {
            let cfg = make_config(HELLO_WASM);
            rt.block_on(async {
                let sandbox = isolate_core::Sandbox::create(cfg).await.unwrap();
                black_box(sandbox);
            });
        })
    });

    group.bench_function("hello_warm_start", |b| {
        b.iter(|| {
            let mut caps = CapabilitySet::default();
            caps.grant(Capability::stdout());
            let enforcer = CapabilityEnforcer::new(caps, Uuid::new_v4());
            let meter = ResourceMeter::new(ResourceLimits::default());
            let inst = pool.try_instantiate(&config, enforcer, meter, None).unwrap();
            black_box(inst);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_cold_start, bench_warm_start, bench_cold_vs_warm);
criterion_main!(benches);
