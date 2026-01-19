# Add Benchmark for Capability Checking Overhead

## Task Description

Add a Criterion benchmark to measure the overhead of capability checking operations.
This helps ensure the security layer doesn't introduce significant performance penalties.

## Background Context

Every WASI operation goes through capability checking. Understanding this overhead
helps with:
- Performance optimization decisions
- Setting expectations for users
- Detecting performance regressions

The benchmark should measure:
- Time to check a single capability
- Time to check against a large capability set
- Overhead compared to baseline (no checking)

## Files to Modify

- `isolate-core/benches/capability.rs` - Create new benchmark file
- `isolate-core/Cargo.toml` - Add benchmark entry if needed

## Acceptance Criteria

- [ ] Benchmark measures capability checking time
- [ ] Tests both small (1-5) and large (100+) capability sets
- [ ] Results are displayed in nanoseconds
- [ ] Benchmark runs as part of `cargo bench`
- [ ] Comments explain what is being measured

## Example Benchmark Structure

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use isolate_core::capability::{Capability, CapabilitySet};

fn bench_capability_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("capability_check");

    // Small set (typical use case)
    let mut small_set = CapabilitySet::default();
    small_set.grant(Capability::stdout());
    small_set.grant(Capability::stderr());
    small_set.grant(Capability::filesystem_read("/tmp"));

    group.bench_function("small_set_hit", |b| {
        b.iter(|| small_set.has(&Capability::stdout()))
    });

    group.bench_function("small_set_miss", |b| {
        b.iter(|| small_set.has(&Capability::stdin()))
    });

    // Large set (stress test)
    let mut large_set = CapabilitySet::default();
    for i in 0..100 {
        large_set.grant(Capability::filesystem_read(format!("/path/{}", i)));
    }

    group.bench_function("large_set", |b| {
        b.iter(|| large_set.has(&Capability::filesystem_read("/path/50")))
    });

    group.finish();
}

criterion_group!(benches, bench_capability_check);
criterion_main!(benches);
```

## Helpful Resources

- Criterion documentation: https://bheisler.github.io/criterion.rs/book/
- Existing benchmarks in `isolate-core/benches/`
- CapabilitySet implementation in `capability/mod.rs`

## Estimated Difficulty

Medium (1-4 hours)
