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
}

impl ResourceUsage {
    /// Create a new empty resource usage.
    pub fn new() -> Self {
        Self::default()
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
}

#[derive(Debug, Default)]
struct MutableUsage {
    peak_memory: usize,
    current_memory: usize,
    cpu_time: Duration,
}

#[derive(Debug, Default)]
struct AtomicCounters {
    fuel_consumed: AtomicU64,
    bytes_read: AtomicU64,
    bytes_written: AtomicU64,
    io_operations: AtomicU64,
}

impl ResourceMeter {
    /// Create a new resource meter with the given limits.
    pub fn new(limits: ResourceLimits) -> Self {
        Self {
            inner: Arc::new(ResourceMeterInner {
                limits,
                usage: Mutex::new(MutableUsage::default()),
                counters: AtomicCounters::default(),
                start_time: Instant::now(),
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
            wall_time: self.inner.start_time.elapsed(),
            bytes_read: self.inner.counters.bytes_read.load(Ordering::Acquire),
            bytes_written: self.inner.counters.bytes_written.load(Ordering::Acquire),
            io_operations: self.inner.counters.io_operations.load(Ordering::Acquire),
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
                requested: new_size,
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
        if self.inner.limits.memory.exceeds_heap(size) {
            return Err(Error::MemoryLimitExceeded {
                limit: self.inner.limits.memory.heap_max,
                requested: size,
            });
        }

        let mut usage = self.inner.usage.lock();
        usage.current_memory = size;
        usage.peak_memory = usage.peak_memory.max(size);
        Ok(())
    }

    /// Record fuel consumption.
    pub fn record_fuel(&self, amount: u64) -> Result<()> {
        let consumed =
            self.inner.counters.fuel_consumed.fetch_add(amount, Ordering::AcqRel) + amount;

        if let Some(limit) = self.inner.limits.cpu.fuel {
            if consumed > limit {
                return Err(Error::FuelExhausted { limit });
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
            let elapsed = self.inner.start_time.elapsed();
            if elapsed > limit {
                return Err(Error::Timeout(limit));
            }
        }
        Ok(())
    }

    /// Get elapsed wall time.
    pub fn elapsed(&self) -> Duration {
        self.inner.start_time.elapsed()
    }

    /// Record bytes read.
    pub fn record_read(&self, bytes: u64) -> Result<()> {
        let total = self.inner.counters.bytes_read.fetch_add(bytes, Ordering::AcqRel) + bytes;
        self.inner.counters.io_operations.fetch_add(1, Ordering::AcqRel);

        if let Some(limit) = self.inner.limits.io.read_bytes {
            if total > limit {
                // Rollback the addition since we exceeded the limit
                self.inner.counters.bytes_read.fetch_sub(bytes, Ordering::AcqRel);
                self.inner.counters.io_operations.fetch_sub(1, Ordering::AcqRel);
                return Err(Error::Execution("I/O read limit exceeded".to_string()));
            }
        }

        Ok(())
    }

    /// Record bytes written.
    pub fn record_write(&self, bytes: u64) -> Result<()> {
        let total = self.inner.counters.bytes_written.fetch_add(bytes, Ordering::AcqRel) + bytes;
        self.inner.counters.io_operations.fetch_add(1, Ordering::AcqRel);

        if let Some(limit) = self.inner.limits.io.write_bytes {
            if total > limit {
                // Rollback the addition since we exceeded the limit
                self.inner.counters.bytes_written.fetch_sub(bytes, Ordering::AcqRel);
                self.inner.counters.io_operations.fetch_sub(1, Ordering::AcqRel);
                return Err(Error::Execution("I/O write limit exceeded".to_string()));
            }
        }

        Ok(())
    }

    /// Reset the meter for reuse (e.g., after snapshot restore).
    pub fn reset(&self) {
        let mut usage = self.inner.usage.lock();
        usage.current_memory = 0;
        usage.peak_memory = 0;
        usage.cpu_time = Duration::ZERO;

        self.inner.counters.fuel_consumed.store(0, Ordering::Release);
        self.inner.counters.bytes_read.store(0, Ordering::Release);
        self.inner.counters.bytes_written.store(0, Ordering::Release);
        self.inner.counters.io_operations.store(0, Ordering::Release);
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
}
