//! Resource limits and metering.
//!
//! This module provides resource limiting capabilities for sandboxes:
//! - Memory limits (heap and stack)
//! - CPU limits (fuel metering and time)
//! - I/O limits (bandwidth and IOPS)
//! - Time limits (wall clock and CPU time)

mod limits;
mod metering;

pub use limits::{CpuLimits, IoLimits, MemoryLimits, ResourceLimits, TimeLimits};
pub use metering::{ResourceMeter, ResourceUsage};
