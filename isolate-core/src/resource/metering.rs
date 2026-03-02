//! Resource metering and usage tracking.

use super::ResourceLimits;
use crate::error::{Error, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Resource usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// Peak memory usage in bytes.
    pub peak_memory: usize,

    /// Current memory usage in bytes.
    pub current_memory: usize,

    /// Total fuel consumed.
    pub fuel_consumed: u64,

    /// CPU time consumed.
    pub cpu_time: Duration,

    /// Wall clock time elapsed.
    pub wall_time: Duration,

    /// Total bytes read.
    pub bytes_read: u64,

    /// Total bytes written.
    pub bytes_written: u64,

    /// Total I/O operations.
    pub io_operations: u64,

    /// I/O read operation count (distinct from bytes_read).
    pub io_read_ops: u64,

    /// I/O write operation count (distinct from bytes_written).
    pub io_write_ops: u64,

    /// Fuel consumed per function name (populated when tracing is enabled).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub fuel_per_function: std::collections::HashMap<String, u64>,

    /// Memory usage samples over time (timestamp_ms, bytes).
    /// Populated when memory watermark tracking is enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_timeline: Vec<(u64, usize)>,
}

/// Resource utilization as a fraction of configured limits (0.0–1.0).
///
/// Values are `None` when the corresponding limit is unlimited.
/// Values may exceed 1.0 if usage was sampled after a limit was raised.
///
/// # Examples
///
/// ```
/// use isolate_core::resource::{ResourceUsage, ResourceLimits, ResourceUtilization};
///
/// let limits = ResourceLimits::restrictive();
/// let usage = ResourceUsage {
///     peak_memory: 32 * 1024 * 1024, // 32MB
///     fuel_consumed: 500_000,         // half of 1M limit
///     ..Default::default()
/// };
/// let util = usage.utilization(&limits);
/// assert!(util.memory.unwrap() > 0.0);
/// assert!((util.fuel.unwrap() - 0.5).abs() < f64::EPSILON);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// Peak memory as fraction of heap_max (0.0–1.0). None if unlimited.
    pub memory: Option<f64>,
    /// Fuel consumed as fraction of fuel limit. None if unlimited.
    pub fuel: Option<f64>,
    /// CPU time as fraction of cpu_time limit. None if unlimited.
    pub cpu_time: Option<f64>,
    /// Wall time as fraction of wall_time limit. None if unlimited.
    pub wall_time: Option<f64>,
    /// Bytes read as fraction of read limit. None if unlimited.
    pub io_read: Option<f64>,
    /// Bytes written as fraction of write limit. None if unlimited.
    pub io_write: Option<f64>,
}

impl ResourceUtilization {
    /// The highest utilization across all limited dimensions.
    ///
    /// Returns `None` if no dimensions are limited.
    pub fn max(&self) -> Option<f64> {
        [self.memory, self.fuel, self.cpu_time, self.wall_time, self.io_read, self.io_write]
            .into_iter()
            .flatten()
            .reduce(f64::max)
    }
}

impl ResourceUsage {
    /// Create a new empty resource usage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute utilization as a fraction of the given limits.
    ///
    /// Each dimension returns `Some(fraction)` when the limit is set,
    /// or `None` when that dimension is unlimited.
    pub fn utilization(&self, limits: &ResourceLimits) -> ResourceUtilization {
        ResourceUtilization {
            memory: if limits.memory.heap_max > 0 {
                Some(self.peak_memory as f64 / limits.memory.heap_max as f64)
            } else {
                None
            },
            fuel: limits.cpu.fuel.map(|limit| self.fuel_consumed as f64 / limit as f64),
            cpu_time: limits
                .cpu
                .cpu_time
                .map(|limit| self.cpu_time.as_secs_f64() / limit.as_secs_f64()),
            wall_time: limits
                .time
                .wall_time
                .map(|limit| self.wall_time.as_secs_f64() / limit.as_secs_f64()),
            io_read: limits.io.read_bytes.map(|limit| self.bytes_read as f64 / limit as f64),
            io_write: limits.io.write_bytes.map(|limit| self.bytes_written as f64 / limit as f64),
        }
    }
}

/// Format a byte count as a human-readable string (e.g., "1.5 MB").
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a duration as a human-readable string (e.g., "1.23s", "456ms").
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs >= 1.0 {
        format!("{:.2}s", secs)
    } else {
        format!("{:.1}ms", secs * 1000.0)
    }
}

impl std::fmt::Display for ResourceUsage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mem={} fuel={} wall={} io={}r/{}w",
            format_bytes(self.peak_memory as u64),
            self.fuel_consumed,
            format_duration(self.wall_time),
            format_bytes(self.bytes_read),
            format_bytes(self.bytes_written),
        )
    }
}

impl ResourceUsage {
    /// Format resource usage as a human-readable multi-line report.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let usage = ResourceUsage {
    ///     peak_memory: 32 * 1024 * 1024,
    ///     fuel_consumed: 500_000,
    ///     wall_time: Duration::from_millis(42),
    ///     bytes_read: 1024,
    ///     bytes_written: 512,
    ///     ..Default::default()
    /// };
    /// let report = usage.format_human();
    /// assert!(report.contains("Memory"));
    /// assert!(report.contains("Fuel"));
    /// ```
    pub fn format_human(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("  Memory (peak): {}", format_bytes(self.peak_memory as u64)));
        if self.current_memory > 0 {
            lines.push(format!("  Memory (current): {}", format_bytes(self.current_memory as u64)));
        }
        lines.push(format!("  Fuel consumed: {}", self.fuel_consumed));
        lines.push(format!("  Wall time: {}", format_duration(self.wall_time)));
        if self.cpu_time > Duration::ZERO {
            lines.push(format!("  CPU time: {}", format_duration(self.cpu_time)));
        }
        if self.bytes_read > 0 || self.bytes_written > 0 {
            lines.push(format!(
                "  I/O: {} read, {} written ({} ops)",
                format_bytes(self.bytes_read),
                format_bytes(self.bytes_written),
                self.io_operations,
            ));
        }
        lines.join("\n")
    }

    /// Format resource usage as a compact single-line string for logging.
    ///
    /// # Examples
    ///
    /// ```
    /// use isolate_core::resource::ResourceUsage;
    /// use std::time::Duration;
    ///
    /// let usage = ResourceUsage {
    ///     peak_memory: 1024,
    ///     fuel_consumed: 100,
    ///     wall_time: Duration::from_millis(5),
    ///     ..Default::default()
    /// };
    /// let compact = usage.format_compact();
    /// assert!(compact.contains("mem="));
    /// assert!(compact.contains("fuel="));
    /// ```
    pub fn format_compact(&self) -> String {
        format!("{}", self)
    }
}

/// Resource meter for tracking and enforcing limits.
#[derive(Debug, Clone)]
pub struct ResourceMeter {
    inner: Arc<ResourceMeterInner>,
}

#[derive(Debug)]
struct ResourceMeterInner {
    limits: ResourceLimits,
    usage: Mutex<MutableUsage>,
    counters: AtomicCounters,
    start_time: Instant,
    fuel_limit_override: AtomicU64,
}

#[derive(Debug, Default)]
struct MutableUsage {
    peak_memory: usize,
    current_memory: usize,
    cpu_time: Duration,
    fuel_per_function: std::collections::HashMap<String, u64>,
    memory_timeline: Vec<(u64, usize)>,
    /// Duration already elapsed before last reset, subtracted from wall_time.
    elapsed_at_reset: Duration,
}

#[derive(Debug, Default)]
struct AtomicCounters {
    fuel_consumed: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    io_operations: AtomicU64,
    io_read_ops: AtomicU64,
    io_write_ops: AtomicU64,
}

impl ResourceMeter {
    /// Create a new resource meter with the given limits.
    pub fn new(limits: ResourceLimits) -> Self {
        let fuel_override = limits.cpu.fuel.unwrap_or(0);
        Self {
            inner: Arc::new(ResourceMeterInner {
                limits,
                usage: Mutex::new(MutableUsage::default()),
                counters: AtomicCounters::default(),
                start_time: Instant::now(),
                fuel_limit_override: AtomicU64::new(fuel_override),
            }),
        }
    }

    /// Get current resource usage.
    pub fn usage(&self) -> ResourceUsage {
        let usage = self.inner.usage.lock();
        ResourceUsage {
            peak_memory: usage.peak_memory,
            current_memory: usage.current_memory,
            fuel_consumed: self.inner.counters.fuel_consumed.load(Ordering::Acquire),
            cpu_time: usage.cpu_time,
            wall_time: self.inner.start_time.elapsed().saturating_sub(usage.elapsed_at_reset),
            bytes_read: self.inner.counters.bytes_read.load(Ordering::Acquire),
            bytes_written: self.inner.counters.bytes_written.load(Ordering::Acquire),
            io_operations: self.inner.counters.io_operations.load(Ordering::Acquire),
            io_read_ops: self.inner.counters.io_read_ops.load(Ordering::Acquire),
            io_write_ops: self.inner.counters.io_write_ops.load(Ordering::Acquire),
            fuel_per_function: usage.fuel_per_function.clone(),
            memory_timeline: usage.memory_timeline.clone(),
        }
    }

    /// Get the resource limits.
    pub fn limits(&self) -> &ResourceLimits {
        &self.inner.limits
    }

    /// Record memory allocation.
    pub fn record_memory_alloc(&self, size: usize) -> Result<()> {
        let mut usage = self.inner.usage.lock();
        let new_size = usage.current_memory.saturating_add(size);

        if self.inner.limits.memory.exceeds_heap(new_size) {
            return Err(Error::MemoryLimitExceeded {
                limit: self.inner.limits.memory.heap_max,
                requested: size,
                current_usage: usage.current_memory,
            });
        }

        usage.current_memory = new_size;
        usage.peak_memory = usage.peak_memory.max(new_size);
        Ok(())
    }

    /// Record memory deallocation.
    pub fn record_memory_free(&self, size: usize) {
        let mut usage = self.inner.usage.lock();
        usage.current_memory = usage.current_memory.saturating_sub(size);
    }

    /// Set current memory usage directly.
    pub fn set_memory_usage(&self, size: usize) -> Result<()> {
        let mut usage = self.inner.usage.lock();
        let prev = usage.current_memory;

        if self.inner.limits.memory.exceeds_heap(size) {
            return Err(Error::MemoryLimitExceeded {
                limit: self.inner.limits.memory.heap_max,
                requested: size,
                current_usage: prev,
            });
        }

        usage.current_memory = size;
        usage.peak_memory = usage.peak_memory.max(size);
        Ok(())
    }

    /// Record fuel consumption.
    pub fn record_fuel(&self, amount: u64) -> Result<()> {
        let prev = self.inner.counters.fuel_consumed.fetch_add(amount, Ordering::AcqRel);
        // Use saturating add to prevent overflow in the consumed calculation
        let consumed = prev.saturating_add(amount);

        // Check dynamic override first, then static limit
        let dynamic_limit = self.inner.fuel_limit_override.load(Ordering::Acquire);
        let limit =
            if dynamic_limit > 0 { Some(dynamic_limit) } else { self.inner.limits.cpu.fuel };

        if let Some(limit) = limit {
            if consumed > limit {
                return Err(Error::FuelExhausted { limit, consumed });
            }
        }

        Ok(())
    }

    /// Get remaining fuel, if limited.
    pub fn remaining_fuel(&self) -> Option<u64> {
        self.inner.limits.cpu.fuel.map(|limit| {
            let consumed = self.inner.counters.fuel_consumed.load(Ordering::Acquire);
            limit.saturating_sub(consumed)
        })
    }

    /// Record CPU time.
    pub fn record_cpu_time(&self, duration: Duration) -> Result<()> {
        let mut usage = self.inner.usage.lock();
        usage.cpu_time += duration;

        if let Some(limit) = self.inner.limits.cpu.cpu_time {
            if usage.cpu_time > limit {
                return Err(Error::Timeout(limit));
            }
        }

        Ok(())
    }

    /// Check wall time limit.
    pub fn check_wall_time(&self) -> Result<()> {
        if let Some(limit) = self.inner.limits.time.wall_time {
            let elapsed = self.elapsed();
            if elapsed > limit {
                return Err(Error::Timeout(limit));
            }
        }
        Ok(())
    }

    /// Get elapsed wall time since creation or last reset.
    pub fn elapsed(&self) -> Duration {
        let usage = self.inner.usage.lock();
        self.inner.start_time.elapsed().saturating_sub(usage.elapsed_at_reset)
    }

    /// Record bytes read.
    pub fn record_read(&self, bytes: u64) -> Result<()> {
        // Check limit before modifying counters to avoid rollback inconsistency
        if let Some(limit) = self.inner.limits.io.read_bytes {
            let current = self.inner.counters.bytes_read.load(Ordering::Acquire);
            if current.saturating_add(bytes) > limit {
                return Err(Error::Execution("I/O read limit exceeded".to_string()));
            }
        }

        self.inner.counters.bytes_read.fetch_add(bytes, Ordering::AcqRel);
        self.inner.counters.io_operations.fetch_add(1, Ordering::AcqRel);
        self.inner.counters.io_read_ops.fetch_add(1, Ordering::AcqRel);

        Ok(())
    }

    /// Record bytes written.
    pub fn record_write(&self, bytes: u64) -> Result<()> {
        // Check limit before modifying counters to avoid rollback inconsistency
        if let Some(limit) = self.inner.limits.io.write_bytes {
            let current = self.inner.counters.bytes_written.load(Ordering::Acquire);
            if current.saturating_add(bytes) > limit {
                return Err(Error::Execution("I/O write limit exceeded".to_string()));
            }
        }

        self.inner.counters.bytes_written.fetch_add(bytes, Ordering::AcqRel);
        self.inner.counters.io_operations.fetch_add(1, Ordering::AcqRel);
        self.inner.counters.io_write_ops.fetch_add(1, Ordering::AcqRel);

        Ok(())
    }

    /// Dynamically adjust the fuel limit at runtime.
    ///
    /// The new limit must be greater than or equal to the fuel already consumed.
    /// Returns the previous limit.
    pub fn adjust_fuel_limit(&self, new_limit: u64) -> Result<u64> {
        let consumed = self.inner.counters.fuel_consumed.load(Ordering::Acquire);
        if new_limit < consumed {
            return Err(Error::InvalidConfig(format!(
                "Cannot set fuel limit to {} — already consumed {}",
                new_limit, consumed
            )));
        }
        let prev = self.inner.fuel_limit_override.swap(new_limit, Ordering::AcqRel);
        Ok(prev)
    }

    /// Record fuel consumed by a specific function.
    pub fn record_function_fuel(&self, function_name: &str, fuel: u64) {
        let mut usage = self.inner.usage.lock();
        *usage.fuel_per_function.entry(function_name.to_string()).or_insert(0) += fuel;
    }

    /// Take a memory watermark sample at the current timestamp.
    pub fn record_memory_watermark(&self) {
        let mut usage = self.inner.usage.lock();
        let elapsed_ms = self.inner.start_time.elapsed().as_millis() as u64;
        let current = usage.current_memory;
        usage.memory_timeline.push((elapsed_ms, current));
    }

    /// Reset the meter for reuse (e.g., after snapshot restore).
    ///
    /// Clears all usage counters and resets wall-time tracking so that
    /// `elapsed()` returns time since this reset, not since creation.
    pub fn reset(&self) {
        let mut usage = self.inner.usage.lock();
        // Record current elapsed time so post-reset elapsed() starts from zero
        usage.elapsed_at_reset = self.inner.start_time.elapsed();
        usage.current_memory = 0;
        usage.peak_memory = 0;
        usage.cpu_time = Duration::ZERO;
        usage.fuel_per_function.clear();
        usage.memory_timeline.clear();

        self.inner.counters.fuel_consumed.store(0, Ordering::Release);
        self.inner.counters.bytes_read.store(0, Ordering::Release);
        self.inner.counters.bytes_written.store(0, Ordering::Release);
        self.inner.counters.io_operations.store(0, Ordering::Release);
        self.inner.counters.io_read_ops.store(0, Ordering::Release);
        self.inner.counters.io_write_ops.store(0, Ordering::Release);

        // Reset dynamic fuel limit override to the configured default
        let default_fuel = self.inner.limits.cpu.fuel.unwrap_or(0);
        self.inner.fuel_limit_override.store(default_fuel, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_meter_memory() {
        let limits = ResourceLimits {
            memory: super::super::MemoryLimits { heap_max: 1024, stack_max: 256, total_max: 2048 },
            ..Default::default()
        };

        let meter = ResourceMeter::new(limits);

        // Allocate within limits
        assert!(meter.record_memory_alloc(512).is_ok());
        assert_eq!(meter.usage().current_memory, 512);
        assert_eq!(meter.usage().peak_memory, 512);

        // Allocate more
        assert!(meter.record_memory_alloc(256).is_ok());
        assert_eq!(meter.usage().current_memory, 768);

        // Free some
        meter.record_memory_free(256);
        assert_eq!(meter.usage().current_memory, 512);
        assert_eq!(meter.usage().peak_memory, 768); // Peak unchanged

        // Try to exceed limit
        assert!(meter.record_memory_alloc(1024).is_err());
    }

    #[test]
    fn test_resource_meter_fuel() {
        let limits = ResourceLimits {
            cpu: super::super::CpuLimits { fuel: Some(1000), ..Default::default() },
            ..Default::default()
        };

        let meter = ResourceMeter::new(limits);

        assert!(meter.record_fuel(500).is_ok());
        assert_eq!(meter.remaining_fuel(), Some(500));

        assert!(meter.record_fuel(400).is_ok());
        assert_eq!(meter.remaining_fuel(), Some(100));

        // Exceed limit
        assert!(meter.record_fuel(200).is_err());
    }

    #[test]
    fn test_resource_meter_io() {
        let limits = ResourceLimits {
            io: super::super::IoLimits {
                read_bytes: Some(1000),
                write_bytes: Some(500),
                iops: None,
            },
            ..Default::default()
        };

        let meter = ResourceMeter::new(limits);

        assert!(meter.record_read(500).is_ok());
        assert!(meter.record_read(400).is_ok());
        assert!(meter.record_read(200).is_err()); // Exceeds 1000

        assert!(meter.record_write(400).is_ok());
        assert!(meter.record_write(200).is_err()); // Exceeds 500

        let usage = meter.usage();
        assert_eq!(usage.bytes_read, 900);
        assert_eq!(usage.bytes_written, 400);
        // Only successful operations are counted (3 successful: 2 reads + 1 write)
        assert_eq!(usage.io_operations, 3);
    }

    #[test]
    fn test_resource_meter_reset() {
        let limits = ResourceLimits::default();
        let meter = ResourceMeter::new(limits);

        meter.record_memory_alloc(512).unwrap();
        meter.record_fuel(100).unwrap();
        meter.record_read(256).unwrap();

        meter.reset();

        let usage = meter.usage();
        assert_eq!(usage.current_memory, 0);
        assert_eq!(usage.fuel_consumed, 0);
        assert_eq!(usage.bytes_read, 0);
    }

    #[test]
    fn test_resource_meter_reset_wall_time() {
        let limits = ResourceLimits::default();
        let meter = ResourceMeter::new(limits);

        // Let some time pass
        std::thread::sleep(std::time::Duration::from_millis(50));
        let before_reset = meter.elapsed();
        assert!(before_reset >= std::time::Duration::from_millis(40));

        meter.reset();

        // After reset, elapsed should be near zero
        let after_reset = meter.elapsed();
        assert!(
            after_reset < std::time::Duration::from_millis(20),
            "elapsed after reset should be near zero, got {:?}",
            after_reset
        );
    }

    #[test]
    fn test_utilization_with_limits() {
        let limits = ResourceLimits::restrictive();
        let usage = ResourceUsage {
            peak_memory: 32 * 1024 * 1024,                    // 32MB of 64MB limit
            fuel_consumed: 500_000,                           // half of 1M limit
            cpu_time: std::time::Duration::from_millis(2500), // half of 5s limit
            wall_time: std::time::Duration::from_secs(15),    // half of 30s limit
            bytes_read: 5 * 1024 * 1024,                      // half of 10MB limit
            bytes_written: 512 * 1024,                        // half of 1MB limit
            ..Default::default()
        };

        let util = usage.utilization(&limits);
        assert!((util.memory.unwrap() - 0.5).abs() < 0.01);
        assert!((util.fuel.unwrap() - 0.5).abs() < f64::EPSILON);
        assert!((util.cpu_time.unwrap() - 0.5).abs() < 0.01);
        assert!((util.wall_time.unwrap() - 0.5).abs() < 0.01);
        assert!((util.io_read.unwrap() - 0.5).abs() < 0.01);
        assert!((util.io_write.unwrap() - 0.5).abs() < 0.01);
        assert!((util.max().unwrap() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_utilization_unlimited() {
        let limits = ResourceLimits::permissive();
        let usage = ResourceUsage::default();

        let util = usage.utilization(&limits);
        assert!(util.fuel.is_none());
        assert!(util.io_read.is_none());
        assert!(util.io_write.is_none());
    }

    #[test]
    fn test_utilization_max() {
        let limits = ResourceLimits::restrictive();
        let usage = ResourceUsage {
            peak_memory: 16 * 1024 * 1024, // 25% of 64MB
            fuel_consumed: 750_000,        // 75% of 1M
            ..Default::default()
        };

        let util = usage.utilization(&limits);
        assert!((util.max().unwrap() - 0.75).abs() < 0.01);
    }

    // ── S4: I/O Limit Enforcement Boundary Tests ─────────────────────────

    #[test]
    fn test_io_read_at_exact_limit() {
        let limits = ResourceLimits {
            io: super::super::IoLimits { read_bytes: Some(100), write_bytes: None, iops: None },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        // Reading exactly up to the limit should succeed
        assert!(meter.record_read(100).is_ok());
        assert_eq!(meter.usage().bytes_read, 100);

        // One more byte should fail
        assert!(meter.record_read(1).is_err());
        // Counter should NOT have been incremented on failure
        assert_eq!(meter.usage().bytes_read, 100);
    }

    #[test]
    fn test_io_write_at_exact_limit() {
        let limits = ResourceLimits {
            io: super::super::IoLimits { read_bytes: None, write_bytes: Some(200), iops: None },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        assert!(meter.record_write(200).is_ok());
        assert_eq!(meter.usage().bytes_written, 200);

        assert!(meter.record_write(1).is_err());
        assert_eq!(meter.usage().bytes_written, 200);
    }

    #[test]
    fn test_io_ops_counted_only_on_success() {
        let limits = ResourceLimits {
            io: super::super::IoLimits { read_bytes: Some(10), write_bytes: Some(10), iops: None },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        assert!(meter.record_read(5).is_ok()); // op 1
        assert!(meter.record_write(5).is_ok()); // op 2
        assert!(meter.record_read(10).is_err()); // should NOT count
        assert!(meter.record_write(10).is_err()); // should NOT count

        let usage = meter.usage();
        assert_eq!(usage.io_operations, 2);
        assert_eq!(usage.io_read_ops, 1);
        assert_eq!(usage.io_write_ops, 1);
    }

    #[test]
    fn test_io_unlimited_accepts_large_values() {
        let limits = ResourceLimits {
            io: super::super::IoLimits { read_bytes: None, write_bytes: None, iops: None },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        assert!(meter.record_read(u64::MAX / 2).is_ok());
        assert!(meter.record_write(u64::MAX / 2).is_ok());
    }

    // ── S5: Metering Reset Edge Case Tests ───────────────────────────────

    #[test]
    fn test_double_reset_clears_correctly() {
        let limits = ResourceLimits::default();
        let meter = ResourceMeter::new(limits);

        meter.record_memory_alloc(512).unwrap();
        meter.record_fuel(100).unwrap();
        meter.reset();
        meter.reset(); // Double reset

        let usage = meter.usage();
        assert_eq!(usage.current_memory, 0);
        assert_eq!(usage.fuel_consumed, 0);
        assert_eq!(usage.bytes_read, 0);
    }

    #[test]
    fn test_reset_allows_reuse_within_limits() {
        let limits = ResourceLimits {
            cpu: super::super::CpuLimits { fuel: Some(100), ..Default::default() },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        // Consume all fuel
        assert!(meter.record_fuel(100).is_ok());
        assert!(meter.record_fuel(1).is_err());

        // Reset and verify fuel is available again
        meter.reset();
        assert_eq!(meter.remaining_fuel(), Some(100));
        assert!(meter.record_fuel(50).is_ok());
        assert_eq!(meter.remaining_fuel(), Some(50));
    }

    #[test]
    fn test_reset_clears_per_function_fuel() {
        let limits = ResourceLimits::default();
        let meter = ResourceMeter::new(limits);

        meter.record_function_fuel("main", 100);
        meter.record_function_fuel("helper", 50);
        assert_eq!(meter.usage().fuel_per_function.len(), 2);

        meter.reset();
        assert!(meter.usage().fuel_per_function.is_empty());
    }

    #[test]
    fn test_reset_clears_memory_timeline() {
        let limits = ResourceLimits::default();
        let meter = ResourceMeter::new(limits);

        meter.record_memory_alloc(256).unwrap();
        meter.record_memory_watermark();
        assert_eq!(meter.usage().memory_timeline.len(), 1);

        meter.reset();
        assert!(meter.usage().memory_timeline.is_empty());
    }

    // ── S6: Concurrent Safety Tests ──────────────────────────────────────

    #[test]
    fn test_concurrent_fuel_recording() {
        let limits = ResourceLimits {
            cpu: super::super::CpuLimits { fuel: Some(1_000_000), ..Default::default() },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        let threads: Vec<_> = (0..10)
            .map(|_| {
                let m = meter.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let _ = m.record_fuel(1);
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        // 10 threads × 100 = 1000 total fuel consumed
        assert_eq!(meter.usage().fuel_consumed, 1000);
    }

    #[test]
    fn test_concurrent_io_recording() {
        let limits = ResourceLimits {
            io: super::super::IoLimits { read_bytes: None, write_bytes: None, iops: None },
            ..Default::default()
        };
        let meter = ResourceMeter::new(limits);

        let threads: Vec<_> = (0..10)
            .map(|_| {
                let m = meter.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        m.record_read(10).unwrap();
                        m.record_write(5).unwrap();
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        let usage = meter.usage();
        // 10 threads × 50 × 10 bytes = 5000 read
        assert_eq!(usage.bytes_read, 5000);
        // 10 threads × 50 × 5 bytes = 2500 written
        assert_eq!(usage.bytes_written, 2500);
        // 10 threads × 50 × 2 ops = 1000 operations
        assert_eq!(usage.io_operations, 1000);
    }

    #[test]
    fn test_resource_usage_display() {
        let usage = ResourceUsage {
            peak_memory: 32 * 1024 * 1024,
            fuel_consumed: 500_000,
            wall_time: Duration::from_millis(42),
            bytes_read: 1024,
            bytes_written: 512,
            ..Default::default()
        };
        let display = format!("{}", usage);
        assert!(display.contains("mem=32.0 MB"));
        assert!(display.contains("fuel=500000"));
        assert!(display.contains("wall=42.0ms"));
        assert!(display.contains("1.0 KB"));
    }

    #[test]
    fn test_resource_usage_display_zero() {
        let usage = ResourceUsage::default();
        let display = format!("{}", usage);
        assert!(display.contains("mem=0 B"));
        assert!(display.contains("fuel=0"));
    }

    #[test]
    fn test_format_human() {
        let usage = ResourceUsage {
            peak_memory: 64 * 1024 * 1024,
            current_memory: 32 * 1024 * 1024,
            fuel_consumed: 1_000_000,
            wall_time: Duration::from_secs(2),
            cpu_time: Duration::from_millis(500),
            bytes_read: 10 * 1024 * 1024,
            bytes_written: 1024,
            io_operations: 50,
            ..Default::default()
        };
        let report = usage.format_human();
        assert!(report.contains("Memory (peak): 64.0 MB"));
        assert!(report.contains("Memory (current): 32.0 MB"));
        assert!(report.contains("Fuel consumed: 1000000"));
        assert!(report.contains("Wall time: 2.00s"));
        assert!(report.contains("CPU time: 500.0ms"));
        assert!(report.contains("I/O:"));
    }

    #[test]
    fn test_format_compact() {
        let usage = ResourceUsage {
            peak_memory: 1024,
            fuel_consumed: 100,
            wall_time: Duration::from_millis(5),
            ..Default::default()
        };
        let compact = usage.format_compact();
        assert_eq!(compact, format!("{}", usage));
    }

    #[test]
    fn test_format_bytes_ranges() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn test_format_duration_ranges() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0.0ms");
        assert_eq!(format_duration(Duration::from_millis(5)), "5.0ms");
        assert_eq!(format_duration(Duration::from_millis(500)), "500.0ms");
        assert_eq!(format_duration(Duration::from_secs(1)), "1.00s");
        assert_eq!(format_duration(Duration::from_secs(60)), "60.00s");
    }
}
