//! Flame graph generation from profiling data.
//!
//! Generates flame graphs in the **folded stack** format compatible with
//! [Brendan Gregg's FlameGraph tools](https://github.com/brendangregg/FlameGraph)
//! and [inferno](https://github.com/jonhoo/inferno).
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::debug::flamegraph::{FlameGraphBuilder, FlameGraphOptions};
//!
//! let builder = FlameGraphBuilder::new();
//! builder.add_stack(&["main", "_start", "compute"], 150);
//! builder.add_stack(&["main", "_start", "io_write"], 50);
//! builder.add_stack(&["main", "cleanup"], 30);
//!
//! let folded = builder.to_folded_stacks();
//! // Output: "main;_start;compute 150\nmain;_start;io_write 50\nmain;cleanup 30\n"
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Options for flame graph generation.
#[derive(Debug, Clone)]
pub struct FlameGraphOptions {
    /// Minimum sample count to include in output.
    pub min_samples: u64,
    /// Stack separator character.
    pub separator: char,
    /// Whether to reverse stacks (caller-callee → callee-caller).
    pub reverse: bool,
    /// Title for the flame graph (used by SVG renderers).
    pub title: String,
    /// Unit label for values (e.g., "microseconds", "fuel", "bytes").
    pub count_unit: String,
}

impl Default for FlameGraphOptions {
    fn default() -> Self {
        Self {
            min_samples: 0,
            separator: ';',
            reverse: false,
            title: "Isolate Sandbox Profile".to_string(),
            count_unit: "microseconds".to_string(),
        }
    }
}

/// A node in the flame graph tree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlameNode {
    /// Function name.
    pub name: String,
    /// Self time (not including children).
    pub self_value: u64,
    /// Total time (including children).
    pub total_value: u64,
    /// Children nodes.
    pub children: HashMap<String, FlameNode>,
}

impl FlameNode {
    fn new(name: &str) -> Self {
        Self { name: name.to_string(), self_value: 0, total_value: 0, children: HashMap::new() }
    }

    fn add_stack(&mut self, stack: &[&str], value: u64) {
        self.total_value += value;

        if stack.is_empty() {
            self.self_value += value;
            return;
        }

        let child =
            self.children.entry(stack[0].to_string()).or_insert_with(|| FlameNode::new(stack[0]));
        child.add_stack(&stack[1..], value);
    }
}

/// Builder for constructing flame graphs from profiling data.
pub struct FlameGraphBuilder {
    root: FlameNode,
    stacks: Vec<(Vec<String>, u64)>,
    options: FlameGraphOptions,
}

impl Default for FlameGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FlameGraphBuilder {
    /// Create a new flame graph builder.
    pub fn new() -> Self {
        Self {
            root: FlameNode::new("root"),
            stacks: Vec::new(),
            options: FlameGraphOptions::default(),
        }
    }

    /// Set options for the flame graph.
    pub fn with_options(mut self, options: FlameGraphOptions) -> Self {
        self.options = options;
        self
    }

    /// Add a stack trace with a value (e.g., duration in microseconds).
    pub fn add_stack(&mut self, stack: &[&str], value: u64) {
        let stack_owned: Vec<String> = stack.iter().map(|s| s.to_string()).collect();
        self.stacks.push((stack_owned, value));

        let stack_refs: Vec<&str> = stack.to_vec();
        self.root.add_stack(&stack_refs, value);
    }

    /// Add a stack from function entry/exit pairs.
    pub fn add_stack_owned(&mut self, stack: Vec<String>, value: u64) {
        let stack_refs: Vec<&str> = stack.iter().map(|s| s.as_str()).collect();
        self.root.add_stack(&stack_refs, value);
        self.stacks.push((stack, value));
    }

    /// Generate folded stack output (compatible with FlameGraph tools).
    ///
    /// Each line has the format: `frame1;frame2;frame3 count`
    pub fn to_folded_stacks(&self) -> String {
        let mut output = String::new();
        let sep = self.options.separator;

        // Aggregate identical stacks
        let mut aggregated: HashMap<String, u64> = HashMap::new();
        for (stack, value) in &self.stacks {
            let frames: Vec<&str> = if self.options.reverse {
                stack.iter().rev().map(|s| s.as_str()).collect()
            } else {
                stack.iter().map(|s| s.as_str()).collect()
            };
            let key = frames.join(&sep.to_string());
            *aggregated.entry(key).or_default() += value;
        }

        // Sort by stack name for deterministic output
        let mut entries: Vec<_> = aggregated.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (stack, value) in entries {
            if value >= self.options.min_samples {
                output.push_str(&format!("{} {}\n", stack, value));
            }
        }

        output
    }

    /// Build from an execution profile.
    ///
    /// Converts function profiles into flame graph stacks, using total duration
    /// in microseconds as the value.
    pub fn from_execution_profile(profile: &super::profiler::ExecutionProfile) -> Self {
        let mut builder = Self::new();

        // Add function profiles as single-frame stacks
        for (name, fp) in &profile.functions {
            let duration_us = fp.total_duration.as_micros() as u64;
            if duration_us > 0 {
                builder.add_stack(&[name.as_str()], duration_us);
            }
        }

        // Add WASI call profiles under a "wasi" frame
        for (name, fp) in &profile.wasi_calls {
            let duration_us = fp.total_duration.as_micros() as u64;
            if duration_us > 0 {
                builder.add_stack(&["[wasi]", name.as_str()], duration_us);
            }
        }

        builder
    }

    /// Get the root node of the flame graph tree.
    pub fn root(&self) -> &FlameNode {
        &self.root
    }

    /// Get total value across all stacks.
    pub fn total_value(&self) -> u64 {
        self.root.total_value
    }

    /// Get the number of unique stacks.
    pub fn stack_count(&self) -> usize {
        self.stacks.len()
    }

    /// Get summary statistics.
    pub fn summary(&self) -> FlameGraphSummary {
        let mut unique_functions: HashMap<String, u64> = HashMap::new();
        for (stack, value) in &self.stacks {
            for frame in stack {
                *unique_functions.entry(frame.clone()).or_default() += value;
            }
        }

        let mut top_functions: Vec<(String, u64)> = unique_functions.into_iter().collect();
        top_functions.sort_by(|a, b| b.1.cmp(&a.1));

        FlameGraphSummary {
            total_stacks: self.stacks.len(),
            total_value: self.root.total_value,
            unique_frames: top_functions.len(),
            top_functions: top_functions.into_iter().take(10).collect(),
            unit: self.options.count_unit.clone(),
        }
    }
}

/// Summary of a flame graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlameGraphSummary {
    /// Total number of stack samples.
    pub total_stacks: usize,
    /// Total value across all stacks.
    pub total_value: u64,
    /// Number of unique frames.
    pub unique_frames: usize,
    /// Top functions by total value.
    pub top_functions: Vec<(String, u64)>,
    /// Unit of measurement.
    pub unit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flame_graph_builder() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "io"], 50);

        assert_eq!(builder.total_value(), 150);
        assert_eq!(builder.stack_count(), 2);
    }

    #[test]
    fn test_folded_stacks_output() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "io"], 50);

        let folded = builder.to_folded_stacks();
        assert!(folded.contains("main;compute 100"));
        assert!(folded.contains("main;io 50"));
    }

    #[test]
    fn test_folded_stacks_aggregation() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "compute"], 50);

        let folded = builder.to_folded_stacks();
        assert!(folded.contains("main;compute 150"));
    }

    #[test]
    fn test_folded_stacks_min_samples() {
        let mut builder = FlameGraphBuilder::new()
            .with_options(FlameGraphOptions { min_samples: 60, ..Default::default() });
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "io"], 50);

        let folded = builder.to_folded_stacks();
        assert!(folded.contains("main;compute 100"));
        assert!(!folded.contains("main;io 50"));
    }

    #[test]
    fn test_folded_stacks_reverse() {
        let mut builder = FlameGraphBuilder::new()
            .with_options(FlameGraphOptions { reverse: true, ..Default::default() });
        builder.add_stack(&["main", "compute", "add"], 100);

        let folded = builder.to_folded_stacks();
        assert!(folded.contains("add;compute;main 100"));
    }

    #[test]
    fn test_flame_node_tree() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "io", "write"], 50);
        builder.add_stack(&["main", "io", "read"], 30);

        let root = builder.root();
        assert_eq!(root.total_value, 180);
        assert!(root.children.contains_key("main"));

        let main = &root.children["main"];
        assert_eq!(main.total_value, 180);
        assert!(main.children.contains_key("compute"));
        assert!(main.children.contains_key("io"));

        let io = &main.children["io"];
        assert_eq!(io.total_value, 80);
    }

    #[test]
    fn test_summary() {
        let mut builder = FlameGraphBuilder::new();
        builder.add_stack(&["main", "compute"], 100);
        builder.add_stack(&["main", "io"], 50);

        let summary = builder.summary();
        assert_eq!(summary.total_stacks, 2);
        assert_eq!(summary.total_value, 150);
        assert_eq!(summary.unique_frames, 3); // main, compute, io
    }

    #[test]
    fn test_empty_flame_graph() {
        let builder = FlameGraphBuilder::new();
        assert_eq!(builder.total_value(), 0);
        assert_eq!(builder.stack_count(), 0);
        assert_eq!(builder.to_folded_stacks(), "");
    }

    #[test]
    fn test_custom_separator() {
        let mut builder = FlameGraphBuilder::new()
            .with_options(FlameGraphOptions { separator: '/', ..Default::default() });
        builder.add_stack(&["a", "b", "c"], 42);

        let folded = builder.to_folded_stacks();
        assert!(folded.contains("a/b/c 42"));
    }

    #[test]
    fn test_from_execution_profile() {
        use super::super::profiler::{ExecutionProfile, FunctionProfile};
        use std::time::Duration;

        let mut functions = HashMap::new();
        functions.insert(
            "compute".to_string(),
            FunctionProfile {
                name: "compute".to_string(),
                call_count: 10,
                total_duration: Duration::from_micros(5000),
                min_duration: Duration::from_micros(100),
                max_duration: Duration::from_micros(1000),
            },
        );

        let mut wasi_calls = HashMap::new();
        wasi_calls.insert(
            "fd_write".to_string(),
            FunctionProfile {
                name: "fd_write".to_string(),
                call_count: 5,
                total_duration: Duration::from_micros(500),
                min_duration: Duration::from_micros(50),
                max_duration: Duration::from_micros(200),
            },
        );

        let profile = ExecutionProfile {
            total_duration: Duration::from_millis(10),
            functions,
            peak_memory: 1024,
            total_fuel_consumed: 100000,
            wasi_calls,
        };

        let builder = FlameGraphBuilder::from_execution_profile(&profile);
        let folded = builder.to_folded_stacks();

        assert!(folded.contains("compute 5000"));
        assert!(folded.contains("[wasi];fd_write 500"));
    }
}
