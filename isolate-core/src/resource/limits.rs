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
    /// Validate that resource limits are internally consistent.
    ///
    /// Returns a list of validation errors. Empty means valid.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.memory.heap_max > self.memory.total_max {
            errors.push(format!(
                "heap_max ({}) exceeds total_max ({})",
                self.memory.heap_max, self.memory.total_max
            ));
        }

        if self.memory.stack_max > self.memory.total_max {
            errors.push(format!(
                "stack_max ({}) exceeds total_max ({})",
                self.memory.stack_max, self.memory.total_max
            ));
        }

        // Combined heap + stack should not exceed total
        if self.memory.heap_max.saturating_add(self.memory.stack_max) > self.memory.total_max {
            errors.push(format!(
                "heap_max ({}) + stack_max ({}) exceeds total_max ({})",
                self.memory.heap_max, self.memory.stack_max, self.memory.total_max
            ));
        }

        if let (Some(cpu), Some(wall)) = (self.cpu.cpu_time, self.time.wall_time) {
            if cpu > wall {
                errors.push(format!("cpu_time ({:?}) exceeds wall_time ({:?})", cpu, wall));
            }
        }

        // Duplicate cpu_time in CpuLimits and TimeLimits should be consistent
        if let (Some(cpu_in_cpu), Some(cpu_in_time)) = (self.cpu.cpu_time, self.time.cpu_time) {
            if cpu_in_cpu != cpu_in_time {
                errors.push(format!(
                    "cpu.cpu_time ({:?}) differs from time.cpu_time ({:?})",
                    cpu_in_cpu, cpu_in_time
                ));
            }
        }

        // Preemption interval should be reasonable (1ms to 10s)
        if self.cpu.preemption_interval < Duration::from_millis(1) {
            errors.push(format!(
                "preemption_interval ({:?}) is below 1ms minimum",
                self.cpu.preemption_interval
            ));
        }
        if self.cpu.preemption_interval > Duration::from_secs(10) {
            errors.push(format!(
                "preemption_interval ({:?}) exceeds 10s — timeouts may be very imprecise",
                self.cpu.preemption_interval
            ));
        }

        errors
    }

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
            io: IoLimits { read_bytes: None, write_bytes: None, iops: None },
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
        Self { fuel: None, cpu_time: None, preemption_interval: Duration::from_millis(100) }
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
        Self { read_bytes: None, write_bytes: None, iops: None }
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
        Self { wall_time: Some(Duration::from_secs(60)), cpu_time: Some(Duration::from_secs(30)) }
    }
}

impl TimeLimits {
    /// Create unlimited time limits.
    pub fn unlimited() -> Self {
        Self { wall_time: None, cpu_time: None }
    }

    /// Check if wall time limiting is enabled.
    pub fn has_wall_time_limit(&self) -> bool {
        self.wall_time.is_some()
    }
}

impl std::fmt::Display for ResourceLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Resources({}, {}, {}, {})", self.memory, self.cpu, self.io, self.time)
    }
}

impl std::fmt::Display for MemoryLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mem[heap={}MB, stack={}KB, total={}MB]",
            self.heap_max / (1024 * 1024),
            self.stack_max / 1024,
            self.total_max / (1024 * 1024),
        )
    }
}

impl std::fmt::Display for CpuLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.fuel, self.cpu_time) {
            (Some(fuel), Some(time)) => write!(f, "cpu[fuel={fuel}, time={time:?}]"),
            (Some(fuel), None) => write!(f, "cpu[fuel={fuel}]"),
            (None, Some(time)) => write!(f, "cpu[time={time:?}]"),
            (None, None) => write!(f, "cpu[unlimited]"),
        }
    }
}

impl std::fmt::Display for IoLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.is_limited() {
            return write!(f, "io[unlimited]");
        }
        write!(f, "io[")?;
        let mut first = true;
        if let Some(r) = self.read_bytes {
            write!(f, "read={}KB", r / 1024)?;
            first = false;
        }
        if let Some(w) = self.write_bytes {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "write={}KB", w / 1024)?;
            first = false;
        }
        if let Some(iops) = self.iops {
            if !first {
                write!(f, ", ")?;
            }
            write!(f, "iops={iops}")?;
        }
        write!(f, "]")
    }
}

impl std::fmt::Display for TimeLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.wall_time, self.cpu_time) {
            (Some(wall), Some(cpu)) => write!(f, "time[wall={wall:?}, cpu={cpu:?}]"),
            (Some(wall), None) => write!(f, "time[wall={wall:?}]"),
            (None, Some(cpu)) => write!(f, "time[cpu={cpu:?}]"),
            (None, None) => write!(f, "time[unlimited]"),
        }
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

    #[test]
    fn test_display_resource_limits() {
        let limits = ResourceLimits::restrictive();
        let display = format!("{}", limits);
        assert!(display.contains("mem["));
        assert!(display.contains("cpu["));
        assert!(display.contains("io["));
        assert!(display.contains("time["));
    }

    #[test]
    fn test_display_unlimited() {
        let limits = ResourceLimits::permissive();
        let display = format!("{}", limits.cpu);
        // Permissive has no fuel or cpu time limit
        assert!(display.contains("unlimited"));
    }

    #[test]
    fn test_validate_heap_plus_stack_exceeds_total() {
        let limits = ResourceLimits {
            memory: MemoryLimits {
                heap_max: 200 * 1024 * 1024,
                stack_max: 200 * 1024 * 1024,
                total_max: 300 * 1024 * 1024,
            },
            ..Default::default()
        };
        let errors = limits.validate();
        assert!(errors
            .iter()
            .any(|e| e.contains("heap_max") && e.contains("stack_max") && e.contains("total_max")));
    }

    #[test]
    fn test_validate_preemption_interval_too_small() {
        let mut limits = ResourceLimits::restrictive();
        limits.cpu.preemption_interval = Duration::from_nanos(100);
        let errors = limits.validate();
        assert!(errors.iter().any(|e| e.contains("preemption_interval") && e.contains("1ms")));
    }

    #[test]
    fn test_validate_preemption_interval_too_large() {
        let mut limits = ResourceLimits::restrictive();
        limits.cpu.preemption_interval = Duration::from_secs(60);
        let errors = limits.validate();
        assert!(errors.iter().any(|e| e.contains("preemption_interval") && e.contains("10s")));
    }

    #[test]
    fn test_validate_restrictive_is_valid() {
        let limits = ResourceLimits::restrictive();
        assert!(limits.validate().is_empty());
    }

    #[test]
    fn test_validate_permissive_is_valid() {
        let limits = ResourceLimits::permissive();
        assert!(limits.validate().is_empty());
    }
}
