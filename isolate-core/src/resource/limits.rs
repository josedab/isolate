//! Resource limit definitions.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Complete resource limits for a sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Memory limits.
    pub memory: MemoryLimits,
    /// CPU limits.
    pub cpu: CpuLimits,
    /// I/O limits.
    pub io: IoLimits,
    /// Time limits.
    pub time: TimeLimits,
}

impl ResourceLimits {
    /// Create resource limits with sensible defaults for untrusted code.
    pub fn restrictive() -> Self {
        Self {
            memory: MemoryLimits {
                heap_max: 64 * 1024 * 1024,   // 64MB
                stack_max: 512 * 1024,        // 512KB
                total_max: 128 * 1024 * 1024, // 128MB
            },
            cpu: CpuLimits {
                fuel: Some(1_000_000),
                cpu_time: Some(Duration::from_secs(5)),
                preemption_interval: Duration::from_millis(10),
            },
            io: IoLimits {
                read_bytes: Some(10 * 1024 * 1024), // 10MB
                write_bytes: Some(1024 * 1024),     // 1MB
                iops: Some(100),
            },
            time: TimeLimits {
                wall_time: Some(Duration::from_secs(30)),
                cpu_time: Some(Duration::from_secs(5)),
            },
        }
    }

    /// Create resource limits that are very permissive (for trusted code).
    pub fn permissive() -> Self {
        Self {
            memory: MemoryLimits {
                heap_max: 4 * 1024 * 1024 * 1024,  // 4GB
                stack_max: 8 * 1024 * 1024,        // 8MB
                total_max: 8 * 1024 * 1024 * 1024, // 8GB
            },
            cpu: CpuLimits {
                fuel: None, // Unlimited
                cpu_time: None,
                preemption_interval: Duration::from_millis(100),
            },
            io: IoLimits {
                read_bytes: None,
                write_bytes: None,
                iops: None,
            },
            time: TimeLimits {
                wall_time: Some(Duration::from_secs(3600)), // 1 hour
                cpu_time: None,
            },
        }
    }
}

/// Memory limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLimits {
    /// Maximum heap size in bytes.
    pub heap_max: usize,
    /// Maximum stack size in bytes.
    pub stack_max: usize,
    /// Maximum total memory (heap + stack + overhead) in bytes.
    pub total_max: usize,
}

impl Default for MemoryLimits {
    fn default() -> Self {
        Self {
            heap_max: 256 * 1024 * 1024,  // 256MB
            stack_max: 1024 * 1024,       // 1MB
            total_max: 512 * 1024 * 1024, // 512MB
        }
    }
}

impl MemoryLimits {
    /// Convert heap size to WASM memory pages (64KB each).
    pub fn heap_pages(&self) -> u32 {
        const PAGE_SIZE: usize = 65536;
        (self.heap_max / PAGE_SIZE).min(u32::MAX as usize) as u32
    }

    /// Check if a memory size exceeds the heap limit.
    pub fn exceeds_heap(&self, size: usize) -> bool {
        size > self.heap_max
    }

    /// Check if a memory size exceeds the total limit.
    pub fn exceeds_total(&self, size: usize) -> bool {
        size > self.total_max
    }
}

/// CPU limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuLimits {
    /// Fuel limit for WASM instruction metering.
    /// Each WASM instruction consumes approximately 1 fuel unit.
    /// None means unlimited.
    pub fuel: Option<u64>,

    /// CPU time limit (execution time, excluding I/O wait).
    /// None means unlimited.
    pub cpu_time: Option<Duration>,

    /// Preemption interval for cooperative scheduling.
    /// The runtime will yield at approximately this interval.
    pub preemption_interval: Duration,
}

impl Default for CpuLimits {
    fn default() -> Self {
        Self {
            fuel: Some(10_000_000), // 10M instructions
            cpu_time: Some(Duration::from_secs(30)),
            preemption_interval: Duration::from_millis(10),
        }
    }
}

impl CpuLimits {
    /// Create unlimited CPU limits.
    pub fn unlimited() -> Self {
        Self {
            fuel: None,
            cpu_time: None,
            preemption_interval: Duration::from_millis(100),
        }
    }

    /// Check if fuel metering is enabled.
    pub fn has_fuel_limit(&self) -> bool {
        self.fuel.is_some()
    }

    /// Check if CPU time limiting is enabled.
    pub fn has_time_limit(&self) -> bool {
        self.cpu_time.is_some()
    }
}

/// I/O limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoLimits {
    /// Maximum bytes that can be read. None means unlimited.
    pub read_bytes: Option<u64>,

    /// Maximum bytes that can be written. None means unlimited.
    pub write_bytes: Option<u64>,

    /// Maximum I/O operations per second. None means unlimited.
    pub iops: Option<u32>,
}

impl Default for IoLimits {
    fn default() -> Self {
        Self {
            read_bytes: Some(100 * 1024 * 1024), // 100MB
            write_bytes: Some(10 * 1024 * 1024), // 10MB
            iops: Some(1000),
        }
    }
}

impl IoLimits {
    /// Create unlimited I/O limits.
    pub fn unlimited() -> Self {
        Self {
            read_bytes: None,
            write_bytes: None,
            iops: None,
        }
    }

    /// Check if any I/O limiting is enabled.
    pub fn is_limited(&self) -> bool {
        self.read_bytes.is_some() || self.write_bytes.is_some() || self.iops.is_some()
    }
}

/// Time limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeLimits {
    /// Maximum wall clock time. None means unlimited.
    pub wall_time: Option<Duration>,

    /// Maximum CPU time. None means unlimited.
    pub cpu_time: Option<Duration>,
}

impl Default for TimeLimits {
    fn default() -> Self {
        Self {
            wall_time: Some(Duration::from_secs(60)),
            cpu_time: Some(Duration::from_secs(30)),
        }
    }
}

impl TimeLimits {
    /// Create unlimited time limits.
    pub fn unlimited() -> Self {
        Self {
            wall_time: None,
            cpu_time: None,
        }
    }

    /// Check if wall time limiting is enabled.
    pub fn has_wall_time_limit(&self) -> bool {
        self.wall_time.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_limits_pages() {
        let limits = MemoryLimits {
            heap_max: 128 * 1024 * 1024, // 128MB
            stack_max: 1024 * 1024,
            total_max: 256 * 1024 * 1024,
        };

        // 128MB = 2048 pages (64KB each)
        assert_eq!(limits.heap_pages(), 2048);
    }

    #[test]
    fn test_memory_limits_exceeds() {
        let limits = MemoryLimits::default();

        assert!(!limits.exceeds_heap(100 * 1024 * 1024)); // 100MB < 256MB
        assert!(limits.exceeds_heap(300 * 1024 * 1024)); // 300MB > 256MB
    }

    #[test]
    fn test_resource_limits_restrictive() {
        let limits = ResourceLimits::restrictive();

        assert_eq!(limits.memory.heap_max, 64 * 1024 * 1024);
        assert_eq!(limits.cpu.fuel, Some(1_000_000));
        assert_eq!(limits.time.wall_time, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_cpu_limits_unlimited() {
        let limits = CpuLimits::unlimited();

        assert!(!limits.has_fuel_limit());
        assert!(!limits.has_time_limit());
    }

    #[test]
    fn test_io_limits_limited() {
        let default = IoLimits::default();
        assert!(default.is_limited());

        let unlimited = IoLimits::unlimited();
        assert!(!unlimited.is_limited());
    }
}
