//! Function-level WASM profiler with flamegraph output.
//!
//! Provides detailed profiling of WASM module execution at the function
//! level, tracking CPU time, memory allocations, and I/O operations.
//! Generates flamegraph-compatible output for visualization.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a profiled function.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionId {
    /// Module name or hash.
    pub module: String,
    /// Function name or index.
    pub name: String,
}

impl FunctionId {
    /// Create a new function identifier.
    pub fn new(module: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
        }
    }

    /// Display name for flamegraph output.
    pub fn display_name(&self) -> String {
        if self.module.is_empty() {
            self.name.clone()
        } else {
            format!("{}::{}", self.module, self.name)
        }
    }
}

/// Sampled profile data for a single function.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionProfile {
    /// Total wall-clock time spent in this function (inclusive of children).
    pub total_time_us: u64,
    /// Self time (exclusive of children).
    pub self_time_us: u64,
    /// Number of times this function was called.
    pub call_count: u64,
    /// Total fuel consumed by this function.
    pub fuel_consumed: u64,
    /// Total memory allocated within this function.
    pub memory_allocated_bytes: u64,
    /// Total bytes written (I/O).
    pub io_bytes_written: u64,
    /// Total bytes read (I/O).
    pub io_bytes_read: u64,
    /// Peak memory usage during this function.
    pub peak_memory_bytes: u64,
}

impl FunctionProfile {
    /// Average time per call in microseconds.
    pub fn avg_time_us(&self) -> f64 {
        if self.call_count == 0 {
            return 0.0;
        }
        self.total_time_us as f64 / self.call_count as f64
    }

    /// Fuel per call.
    pub fn fuel_per_call(&self) -> f64 {
        if self.call_count == 0 {
            return 0.0;
        }
        self.fuel_consumed as f64 / self.call_count as f64
    }

    /// Memory allocated per call.
    pub fn memory_per_call(&self) -> f64 {
        if self.call_count == 0 {
            return 0.0;
        }
        self.memory_allocated_bytes as f64 / self.call_count as f64
    }
}

/// A call stack sample for call-graph construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackSample {
    /// Timestamp when this sample was taken.
    pub timestamp_us: u64,
    /// Stack frames from bottom (main) to top (current function).
    pub frames: Vec<FunctionId>,
    /// Resource metrics at this sample point.
    pub fuel_at_sample: u64,
    pub memory_at_sample: u64,
}

/// Profile analysis result for a WASM execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionProfileReport {
    /// Module identifier.
    pub module_id: String,
    /// Total execution time.
    pub total_duration_us: u64,
    /// Total fuel consumed.
    pub total_fuel: u64,
    /// Peak memory usage.
    pub peak_memory_bytes: u64,
    /// Total I/O bytes.
    pub total_io_bytes: u64,
    /// Per-function profiles.
    pub functions: HashMap<String, FunctionProfile>,
    /// Hot functions (sorted by self_time descending).
    pub hot_functions: Vec<HotFunction>,
    /// Optimization suggestions.
    pub suggestions: Vec<ProfileSuggestion>,
}

/// A function identified as a performance hotspot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotFunction {
    pub name: String,
    pub self_time_us: u64,
    /// Percentage of total execution time.
    pub self_time_pct: f64,
    pub call_count: u64,
    pub category: HotspotCategory,
}

/// Category of performance hotspot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HotspotCategory {
    /// CPU-bound (high fuel consumption).
    CpuBound,
    /// Memory-bound (high allocation rate).
    MemoryBound,
    /// I/O-bound (high read/write volume).
    IoBound,
    /// Frequently called (high call count).
    FrequentlyCalled,
}

/// Optimization suggestion from the profiler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSuggestion {
    pub severity: SuggestionSeverity,
    pub function: Option<String>,
    pub message: String,
    pub category: String,
}

/// Severity of a profiler suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionSeverity {
    Info,
    Warning,
    Critical,
}

/// Collects profiling data during WASM execution.
pub struct WasmProfiler {
    module_id: String,
    functions: parking_lot::Mutex<HashMap<String, FunctionProfile>>,
    samples: parking_lot::Mutex<Vec<StackSample>>,
    start_time_us: u64,
    total_fuel: parking_lot::Mutex<u64>,
    peak_memory: parking_lot::Mutex<u64>,
    total_io: parking_lot::Mutex<u64>,
}

impl WasmProfiler {
    /// Create a new profiler for a WASM module execution.
    pub fn new(module_id: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        Self {
            module_id: module_id.into(),
            functions: parking_lot::Mutex::new(HashMap::new()),
            samples: parking_lot::Mutex::new(Vec::new()),
            start_time_us: now,
            total_fuel: parking_lot::Mutex::new(0),
            peak_memory: parking_lot::Mutex::new(0),
            total_io: parking_lot::Mutex::new(0),
        }
    }

    /// Record a function call with its metrics.
    pub fn record_function_call(
        &self,
        func_name: &str,
        total_time_us: u64,
        self_time_us: u64,
        fuel: u64,
        memory_alloc: u64,
        io_read: u64,
        io_write: u64,
    ) {
        let mut funcs = self.functions.lock();
        let profile = funcs.entry(func_name.to_string()).or_default();
        profile.call_count += 1;
        profile.total_time_us += total_time_us;
        profile.self_time_us += self_time_us;
        profile.fuel_consumed += fuel;
        profile.memory_allocated_bytes += memory_alloc;
        profile.io_bytes_read += io_read;
        profile.io_bytes_written += io_write;

        *self.total_fuel.lock() += fuel;
        *self.total_io.lock() += io_read + io_write;
    }

    /// Record a memory high-water mark.
    pub fn record_memory_peak(&self, bytes: u64) {
        let mut peak = self.peak_memory.lock();
        if bytes > *peak {
            *peak = bytes;
        }
    }

    /// Record a stack sample.
    pub fn record_sample(&self, frames: Vec<FunctionId>, fuel: u64, memory: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        self.samples.lock().push(StackSample {
            timestamp_us: now - self.start_time_us,
            frames,
            fuel_at_sample: fuel,
            memory_at_sample: memory,
        });
    }

    /// Generate the final profile report.
    pub fn finish(&self) -> ExecutionProfileReport {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        let total_duration = now - self.start_time_us;
        let functions = self.functions.lock().clone();
        let total_fuel = *self.total_fuel.lock();
        let peak_memory = *self.peak_memory.lock();
        let total_io = *self.total_io.lock();

        let mut hot_functions: Vec<HotFunction> = functions
            .iter()
            .map(|(name, profile)| {
                let self_time_pct = if total_duration > 0 {
                    profile.self_time_us as f64 / total_duration as f64 * 100.0
                } else {
                    0.0
                };

                let category = Self::categorize_hotspot(profile, total_duration);

                HotFunction {
                    name: name.clone(),
                    self_time_us: profile.self_time_us,
                    self_time_pct,
                    call_count: profile.call_count,
                    category,
                }
            })
            .collect();

        hot_functions.sort_by(|a, b| b.self_time_us.cmp(&a.self_time_us));
        hot_functions.truncate(20); // Top 20

        let suggestions = Self::generate_suggestions(&functions, total_duration, total_fuel, peak_memory);

        ExecutionProfileReport {
            module_id: self.module_id.clone(),
            total_duration_us: total_duration,
            total_fuel,
            peak_memory_bytes: peak_memory,
            total_io_bytes: total_io,
            functions,
            hot_functions,
            suggestions,
        }
    }

    /// Generate flamegraph-compatible folded stacks from collected samples.
    pub fn generate_flamegraph(&self) -> Vec<FlamegraphEntry> {
        let samples = self.samples.lock();
        let mut stack_counts: HashMap<String, u64> = HashMap::new();

        for sample in samples.iter() {
            let stack: String = sample
                .frames
                .iter()
                .map(|f| f.display_name())
                .collect::<Vec<_>>()
                .join(";");

            if !stack.is_empty() {
                *stack_counts.entry(stack).or_insert(0) += 1;
            }
        }

        let mut entries: Vec<FlamegraphEntry> = stack_counts
            .into_iter()
            .map(|(stack, count)| FlamegraphEntry { stack, count })
            .collect();
        entries.sort_by(|a, b| b.count.cmp(&a.count));
        entries
    }

    /// Render flamegraph entries as folded stack text.
    pub fn render_flamegraph_folded(entries: &[FlamegraphEntry]) -> String {
        entries
            .iter()
            .map(|e| format!("{} {}", e.stack, e.count))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn categorize_hotspot(profile: &FunctionProfile, total_duration: u64) -> HotspotCategory {
        let time_pct = if total_duration > 0 {
            profile.self_time_us as f64 / total_duration as f64
        } else {
            0.0
        };

        let io_ratio = if profile.fuel_consumed > 0 {
            (profile.io_bytes_read + profile.io_bytes_written) as f64
                / profile.fuel_consumed as f64
        } else {
            0.0
        };

        let mem_ratio = if profile.fuel_consumed > 0 {
            profile.memory_allocated_bytes as f64 / profile.fuel_consumed as f64
        } else {
            0.0
        };

        if io_ratio > 10.0 {
            HotspotCategory::IoBound
        } else if mem_ratio > 100.0 {
            HotspotCategory::MemoryBound
        } else if profile.call_count > 1000 && time_pct < 0.1 {
            HotspotCategory::FrequentlyCalled
        } else {
            HotspotCategory::CpuBound
        }
    }

    fn generate_suggestions(
        functions: &HashMap<String, FunctionProfile>,
        total_duration: u64,
        total_fuel: u64,
        peak_memory: u64,
    ) -> Vec<ProfileSuggestion> {
        let mut suggestions = Vec::new();

        // Check for CPU-intensive functions
        for (name, profile) in functions {
            let time_pct = if total_duration > 0 {
                profile.self_time_us as f64 / total_duration as f64 * 100.0
            } else {
                0.0
            };

            if time_pct > 50.0 {
                suggestions.push(ProfileSuggestion {
                    severity: SuggestionSeverity::Critical,
                    function: Some(name.clone()),
                    message: format!(
                        "Function '{}' consumes {:.1}% of execution time. Consider optimizing or splitting.",
                        name, time_pct
                    ),
                    category: "cpu".into(),
                });
            }

            if profile.memory_allocated_bytes > 100 * 1024 * 1024 {
                suggestions.push(ProfileSuggestion {
                    severity: SuggestionSeverity::Warning,
                    function: Some(name.clone()),
                    message: format!(
                        "Function '{}' allocates {}MB. Consider reducing allocations.",
                        name,
                        profile.memory_allocated_bytes / (1024 * 1024)
                    ),
                    category: "memory".into(),
                });
            }

            if profile.call_count > 10_000 && profile.avg_time_us() > 100.0 {
                suggestions.push(ProfileSuggestion {
                    severity: SuggestionSeverity::Warning,
                    function: Some(name.clone()),
                    message: format!(
                        "Function '{}' called {} times at {:.0}μs avg. Consider caching or batching.",
                        name, profile.call_count, profile.avg_time_us()
                    ),
                    category: "frequency".into(),
                });
            }
        }

        // Global suggestions
        if peak_memory > 512 * 1024 * 1024 {
            suggestions.push(ProfileSuggestion {
                severity: SuggestionSeverity::Warning,
                function: None,
                message: format!(
                    "Peak memory usage is {}MB. Consider reducing buffer sizes or using streaming.",
                    peak_memory / (1024 * 1024)
                ),
                category: "memory".into(),
            });
        }

        if total_fuel > 100_000_000 {
            suggestions.push(ProfileSuggestion {
                severity: SuggestionSeverity::Info,
                function: None,
                message: format!(
                    "Total fuel consumption is {}M. Review hot functions for optimization opportunities.",
                    total_fuel / 1_000_000
                ),
                category: "cpu".into(),
            });
        }

        suggestions
    }
}

/// A flamegraph entry (folded stack with sample count).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlamegraphEntry {
    pub stack: String,
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_profile_basics() {
        let profiler = WasmProfiler::new("test-module");

        profiler.record_function_call("main", 1000, 500, 5000, 1024, 0, 0);
        profiler.record_function_call("helper", 400, 400, 2000, 512, 100, 200);
        profiler.record_function_call("main", 800, 300, 3000, 256, 0, 0);
        profiler.record_memory_peak(2048);

        let report = profiler.finish();
        assert_eq!(report.module_id, "test-module");
        assert_eq!(report.functions.len(), 2);

        let main_profile = &report.functions["main"];
        assert_eq!(main_profile.call_count, 2);
        assert_eq!(main_profile.total_time_us, 1800);
        assert_eq!(main_profile.self_time_us, 800);
        assert_eq!(main_profile.fuel_consumed, 8000);

        let helper_profile = &report.functions["helper"];
        assert_eq!(helper_profile.call_count, 1);
        assert_eq!(helper_profile.io_bytes_read, 100);
        assert_eq!(helper_profile.io_bytes_written, 200);
    }

    #[test]
    fn test_hot_functions_sorted() {
        let profiler = WasmProfiler::new("test");

        profiler.record_function_call("slow_func", 10000, 10000, 50000, 0, 0, 0);
        profiler.record_function_call("fast_func", 100, 100, 500, 0, 0, 0);
        profiler.record_function_call("medium_func", 5000, 5000, 25000, 0, 0, 0);

        let report = profiler.finish();
        assert_eq!(report.hot_functions[0].name, "slow_func");
        assert_eq!(report.hot_functions[1].name, "medium_func");
        assert_eq!(report.hot_functions[2].name, "fast_func");
    }

    #[test]
    fn test_flamegraph_generation() {
        let profiler = WasmProfiler::new("test");

        let main_id = FunctionId::new("mod", "main");
        let helper_id = FunctionId::new("mod", "helper");
        let io_id = FunctionId::new("mod", "do_io");

        // Sample: main → helper
        profiler.record_sample(vec![main_id.clone(), helper_id.clone()], 100, 1024);
        profiler.record_sample(vec![main_id.clone(), helper_id.clone()], 200, 1024);
        // Sample: main → do_io
        profiler.record_sample(vec![main_id.clone(), io_id.clone()], 300, 2048);
        // Sample: just main
        profiler.record_sample(vec![main_id.clone()], 400, 2048);

        let entries = profiler.generate_flamegraph();
        assert!(!entries.is_empty());

        // Check that the folded stacks exist
        let stacks: Vec<&str> = entries.iter().map(|e| e.stack.as_str()).collect();
        assert!(stacks.iter().any(|s| s.contains("mod::main;mod::helper")));
        assert!(stacks.iter().any(|s| s.contains("mod::main;mod::do_io")));

        // Most sampled stack should be first
        let top = &entries[0];
        assert_eq!(top.count, 2); // main→helper appeared twice
    }

    #[test]
    fn test_flamegraph_render() {
        let entries = vec![
            FlamegraphEntry {
                stack: "main;helper".into(),
                count: 5,
            },
            FlamegraphEntry {
                stack: "main;io".into(),
                count: 3,
            },
        ];

        let rendered = WasmProfiler::render_flamegraph_folded(&entries);
        assert!(rendered.contains("main;helper 5"));
        assert!(rendered.contains("main;io 3"));
    }

    #[test]
    fn test_function_profile_metrics() {
        let mut profile = FunctionProfile::default();
        assert_eq!(profile.avg_time_us(), 0.0);
        assert_eq!(profile.fuel_per_call(), 0.0);
        assert_eq!(profile.memory_per_call(), 0.0);

        profile.call_count = 10;
        profile.total_time_us = 5000;
        profile.fuel_consumed = 100_000;
        profile.memory_allocated_bytes = 10240;

        assert_eq!(profile.avg_time_us(), 500.0);
        assert_eq!(profile.fuel_per_call(), 10_000.0);
        assert_eq!(profile.memory_per_call(), 1024.0);
    }

    #[test]
    fn test_hotspot_categorization() {
        let profiler = WasmProfiler::new("test");

        // I/O bound function: lots of I/O relative to fuel
        profiler.record_function_call("io_heavy", 1000, 1000, 100, 0, 50000, 50000);
        let report = profiler.finish();
        assert_eq!(report.hot_functions[0].category, HotspotCategory::IoBound);
    }

    #[test]
    fn test_suggestions_cpu_heavy() {
        let profiler = WasmProfiler::new("test");

        // Single function consuming "most" of the time
        profiler.record_function_call("bottleneck", 1_000_000, 1_000_000, 50_000_000, 0, 0, 0);

        let report = profiler.finish();
        // Should have a CPU suggestion
        assert!(report
            .suggestions
            .iter()
            .any(|s| s.category == "cpu" && s.function.is_some()));
    }

    #[test]
    fn test_suggestions_high_memory() {
        let profiler = WasmProfiler::new("test");

        profiler.record_function_call(
            "alloc_heavy",
            1000,
            1000,
            1000,
            200 * 1024 * 1024, // 200MB allocated
            0,
            0,
        );

        let report = profiler.finish();
        assert!(report
            .suggestions
            .iter()
            .any(|s| s.category == "memory" && s.function.is_some()));
    }

    #[test]
    fn test_peak_memory_tracking() {
        let profiler = WasmProfiler::new("test");

        profiler.record_memory_peak(1024);
        profiler.record_memory_peak(4096);
        profiler.record_memory_peak(2048); // Lower, shouldn't override

        let report = profiler.finish();
        assert_eq!(report.peak_memory_bytes, 4096);
    }

    #[test]
    fn test_empty_profiler() {
        let profiler = WasmProfiler::new("empty");
        let report = profiler.finish();
        assert_eq!(report.functions.len(), 0);
        assert_eq!(report.hot_functions.len(), 0);
        assert!(report.suggestions.is_empty());
    }

    #[test]
    fn test_function_id_display() {
        let fid = FunctionId::new("mymod", "process");
        assert_eq!(fid.display_name(), "mymod::process");

        let fid_no_mod = FunctionId::new("", "main");
        assert_eq!(fid_no_mod.display_name(), "main");
    }
}
