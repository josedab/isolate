# ADR-0011: Dual Metrics Architecture

## Status

Accepted

## Context

Observability is critical for a sandbox runtime. Operators need to understand:

- How many sandboxes are active?
- What's the cold start latency distribution?
- How much fuel is being consumed?
- Are capability denials happening?

Two levels of metrics are needed:

1. **Global metrics**: Aggregate statistics across all sandboxes (Prometheus-style)
2. **Per-sandbox metrics**: Detailed stats for individual sandbox instances

Different consumers have different needs:

- Operations dashboards want aggregates (global)
- Billing systems need per-execution details (per-sandbox)
- Debugging requires instance-specific data (per-sandbox)

## Decision

We implemented a **dual metrics architecture** with a global Prometheus registry plus per-sandbox metric structs.

### Global Metrics Registry

```rust
static REGISTRY: OnceLock<MetricsRegistry> = OnceLock::new();

pub fn global_registry() -> &'static MetricsRegistry {
    REGISTRY.get_or_init(MetricsRegistry::new)
}

pub struct MetricsRegistry {
    registry: Registry,

    // Sandbox lifecycle
    sandboxes_created: Counter,
    sandboxes_active: Gauge,
    sandbox_runs: CounterVec,          // labels: status
    sandbox_run_duration: HistogramVec, // labels: status
    sandbox_cold_start: Histogram,

    // Resources
    memory_usage: GaugeVec,            // labels: sandbox_id, type
    fuel_consumed: CounterVec,         // labels: sandbox_id

    // Capabilities
    capability_checks: CounterVec,     // labels: capability, result
    capability_denials: CounterVec,    // labels: capability
}
```

### Prometheus Integration

Standard Prometheus metric types with pre-defined buckets:

```rust
let sandbox_cold_start = Histogram::with_opts(
    HistogramOpts::new(
        "isolate_sandbox_cold_start_seconds",
        "Sandbox cold start time in seconds",
    )
    .buckets(vec![
        0.0001, 0.0005, 0.001, 0.002, 0.003, 0.005, 0.01, 0.025, 0.05, 0.1
    ]),
).unwrap();

let sandbox_run_duration = HistogramVec::new(
    HistogramOpts::new(
        "isolate_sandbox_run_duration_seconds",
        "Sandbox run duration in seconds",
    )
    .buckets(vec![
        0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
    ]),
    &["status"],
).unwrap();
```

### Per-Sandbox Metrics

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxMetrics {
    pub sandbox_id: SandboxId,
    pub run_count: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub total_run_duration: Duration,
    pub last_run_duration: Option<Duration>,
    // ... more fields
}

impl SandboxMetrics {
    pub fn record_run_complete(&mut self, duration: Duration, success: bool) {
        self.run_count += 1;
        self.total_run_duration += duration;
        self.last_run_duration = Some(duration);

        if success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }

        // Update global metrics too
        global_registry().record_sandbox_run(duration, success);
    }

    pub fn success_rate(&self) -> f64 {
        if self.run_count > 0 {
            self.success_count as f64 / self.run_count as f64
        } else {
            0.0
        }
    }
}
```

### Timing Statistics

Detailed timing analysis with percentiles:

```rust
pub struct TimingStats {
    inner: Arc<RwLock<TimingStatsInner>>,
}

struct TimingStatsInner {
    count: u64,
    sum: Duration,
    min: Option<Duration>,
    max: Option<Duration>,
    samples: Vec<Duration>,  // Last 1000 for percentiles
}

impl TimingStats {
    pub fn percentile(&self, p: f64) -> Option<Duration> {
        let inner = self.inner.read();
        let mut sorted = inner.samples.clone();
        sorted.sort();
        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        Some(sorted[idx])
    }
}
```

## Consequences

### Positive

- **Prometheus compatibility**: Standard format, works with existing dashboards
- **Granular insights**: Per-sandbox metrics enable detailed analysis
- **Billing support**: Per-sandbox data supports usage-based billing
- **Low overhead**: Counters and gauges are cheap to update
- **Serializable**: Per-sandbox metrics can be stored/transmitted

### Negative

- **Memory usage**: Per-sandbox metrics accumulate (need cleanup)
- **Cardinality risk**: High-cardinality labels (sandbox_id) can overwhelm Prometheus
- **Dual updates**: Recording in both places adds code complexity
- **Global state**: Static registry is harder to test

### Implications

- Metrics endpoints should expose global registry (`/metrics`)
- Per-sandbox metrics returned in `Output` struct
- High-cardinality metrics should be optional or aggregated
- Tests should use isolated registries when possible
- Cleanup terminated sandbox metrics to prevent memory leaks
