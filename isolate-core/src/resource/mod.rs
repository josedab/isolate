//! Resource limits and metering.
//!
//! This module provides resource limiting capabilities for sandboxes:
//! - Memory limits (heap and stack)
//! - CPU limits (fuel metering and time)
//! - I/O limits (bandwidth and IOPS)
//! - Time limits (wall clock and CPU time)

mod limits;
mod metering;
pub mod profiler;
pub mod scheduler;
pub mod wasm_profiler;

pub use limits::{CpuLimits, IoLimits, MemoryLimits, ResourceLimits, TimeLimits};
pub use metering::{ResourceMeter, ResourceUsage};
pub use profiler::{
    CloudProvider, CostEstimate, ExecutionProfile, PricingModel, ProfileSummary, Recommendation,
    ResourceProfiler,
};
pub use scheduler::{
    ClusterUtilization, FairShareQuota, NodeCapacity, NodeId, PlacementStrategy, Priority,
    ResourceRequest, ResourceScheduler, ScheduleResult,
};
pub use wasm_profiler::{
    ExecutionProfileReport, FlamegraphEntry, FunctionId, FunctionProfile, HotFunction,
    HotspotCategory, ProfileSuggestion, StackSample, SuggestionSeverity, WasmProfiler,
};
