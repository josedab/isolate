//! Breakpoint management.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// Unique breakpoint identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BreakpointId(pub u64);

impl BreakpointId {
    /// Generate a new unique breakpoint ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for BreakpointId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BreakpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bp-{}", self.0)
    }
}

/// Type of breakpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointType {
    /// Break at a specific function by name.
    Function,
    /// Break at a specific instruction address.
    Address,
    /// Break when memory location is accessed.
    Watchpoint,
    /// Break on specific WASI call.
    WasiCall,
    /// Break on capability check.
    CapabilityCheck,
    /// Break on resource limit threshold.
    ResourceThreshold,
}

/// Condition for conditional breakpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointCondition {
    /// Expression to evaluate.
    pub expression: String,
    /// Expected value (if any).
    pub expected_value: Option<String>,
    /// Hit count required before triggering.
    pub hit_count: Option<u64>,
    /// Only trigger if expression is true.
    pub when_true: bool,
}

impl BreakpointCondition {
    /// Create a new condition that triggers after N hits.
    pub fn hit_count(count: u64) -> Self {
        Self {
            expression: String::new(),
            expected_value: None,
            hit_count: Some(count),
            when_true: true,
        }
    }

    /// Create a new condition with an expression.
    pub fn expression(expr: impl Into<String>) -> Self {
        Self { expression: expr.into(), expected_value: None, hit_count: None, when_true: true }
    }

    /// Create a condition that checks a value equals expected.
    pub fn equals(expr: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            expression: expr.into(),
            expected_value: Some(value.into()),
            hit_count: None,
            when_true: true,
        }
    }
}

/// A debug breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Unique breakpoint ID.
    pub id: BreakpointId,
    /// Breakpoint type.
    pub bp_type: BreakpointType,
    /// Whether the breakpoint is enabled.
    pub enabled: bool,
    /// Function name (for function breakpoints).
    pub name: Option<String>,
    /// Address (for address/watchpoint breakpoints).
    pub address: Option<u64>,
    /// Memory size to watch (for watchpoints).
    pub watch_size: Option<usize>,
    /// Watch for reads, writes, or both.
    pub watch_mode: WatchMode,
    /// WASI call name (for WASI breakpoints).
    pub wasi_call: Option<String>,
    /// Capability to watch (for capability breakpoints).
    pub capability: Option<String>,
    /// Resource type and threshold (for resource breakpoints).
    pub resource_threshold: Option<ResourceThreshold>,
    /// Optional condition.
    pub condition: Option<BreakpointCondition>,
    /// Number of times this breakpoint has been hit.
    pub hit_count: u64,
    /// Associated sandbox ID (if sandbox-specific).
    pub sandbox_id: Option<Uuid>,
    /// User-provided label.
    pub label: Option<String>,
}

/// Watch mode for watchpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WatchMode {
    /// Watch for reads.
    Read,
    /// Watch for writes.
    Write,
    /// Watch for both reads and writes.
    #[default]
    ReadWrite,
}

/// Resource threshold configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceThreshold {
    /// Resource type.
    pub resource: ResourceType,
    /// Threshold value.
    pub threshold: u64,
    /// Trigger when above (true) or below (false) threshold.
    pub trigger_above: bool,
}

/// Type of resource to monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    /// Memory usage in bytes.
    Memory,
    /// Fuel/instructions consumed.
    Fuel,
    /// CPU time in milliseconds.
    CpuTime,
    /// I/O bytes transferred.
    IoBytes,
}

impl Breakpoint {
    /// Create a new function breakpoint.
    pub fn function(name: impl Into<String>) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::Function,
            enabled: true,
            name: Some(name.into()),
            address: None,
            watch_size: None,
            watch_mode: WatchMode::default(),
            wasi_call: None,
            capability: None,
            resource_threshold: None,
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Create a new address breakpoint.
    pub fn address(addr: u64) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::Address,
            enabled: true,
            name: None,
            address: Some(addr),
            watch_size: None,
            watch_mode: WatchMode::default(),
            wasi_call: None,
            capability: None,
            resource_threshold: None,
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Create a memory watchpoint.
    pub fn watchpoint(addr: u64, size: usize, mode: WatchMode) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::Watchpoint,
            enabled: true,
            name: None,
            address: Some(addr),
            watch_size: Some(size),
            watch_mode: mode,
            wasi_call: None,
            capability: None,
            resource_threshold: None,
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Create a WASI call breakpoint.
    pub fn wasi_call(call_name: impl Into<String>) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::WasiCall,
            enabled: true,
            name: None,
            address: None,
            watch_size: None,
            watch_mode: WatchMode::default(),
            wasi_call: Some(call_name.into()),
            capability: None,
            resource_threshold: None,
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Create a capability check breakpoint.
    pub fn capability(cap: impl Into<String>) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::CapabilityCheck,
            enabled: true,
            name: None,
            address: None,
            watch_size: None,
            watch_mode: WatchMode::default(),
            wasi_call: None,
            capability: Some(cap.into()),
            resource_threshold: None,
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Create a resource threshold breakpoint.
    pub fn resource(resource: ResourceType, threshold: u64, trigger_above: bool) -> Self {
        Self {
            id: BreakpointId::new(),
            bp_type: BreakpointType::ResourceThreshold,
            enabled: true,
            name: None,
            address: None,
            watch_size: None,
            watch_mode: WatchMode::default(),
            wasi_call: None,
            capability: None,
            resource_threshold: Some(ResourceThreshold { resource, threshold, trigger_above }),
            condition: None,
            hit_count: 0,
            sandbox_id: None,
            label: None,
        }
    }

    /// Set the breakpoint condition.
    pub fn with_condition(mut self, condition: BreakpointCondition) -> Self {
        self.condition = Some(condition);
        self
    }

    /// Set a label for this breakpoint.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Associate with a specific sandbox.
    pub fn for_sandbox(mut self, sandbox_id: Uuid) -> Self {
        self.sandbox_id = Some(sandbox_id);
        self
    }

    /// Enable the breakpoint.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the breakpoint.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Increment hit count and return the new value.
    pub fn record_hit(&mut self) -> u64 {
        self.hit_count += 1;
        self.hit_count
    }

    /// Check if this breakpoint should trigger.
    pub fn should_trigger(&self) -> bool {
        if !self.enabled {
            return false;
        }

        // Check hit count condition
        if let Some(ref condition) = self.condition {
            if let Some(required_hits) = condition.hit_count {
                if self.hit_count < required_hits {
                    return false;
                }
            }
        }

        true
    }

    /// Get a display-friendly description.
    pub fn description(&self) -> String {
        match &self.bp_type {
            BreakpointType::Function => {
                format!("function {}", self.name.as_deref().unwrap_or("unknown"))
            }
            BreakpointType::Address => {
                format!("address 0x{:x}", self.address.unwrap_or(0))
            }
            BreakpointType::Watchpoint => {
                let mode = match self.watch_mode {
                    WatchMode::Read => "read",
                    WatchMode::Write => "write",
                    WatchMode::ReadWrite => "access",
                };
                format!(
                    "watchpoint {} @ 0x{:x} ({} bytes)",
                    mode,
                    self.address.unwrap_or(0),
                    self.watch_size.unwrap_or(0)
                )
            }
            BreakpointType::WasiCall => {
                format!("wasi::{}", self.wasi_call.as_deref().unwrap_or("unknown"))
            }
            BreakpointType::CapabilityCheck => {
                format!("capability {}", self.capability.as_deref().unwrap_or("unknown"))
            }
            BreakpointType::ResourceThreshold => {
                if let Some(ref rt) = self.resource_threshold {
                    let dir = if rt.trigger_above { ">" } else { "<" };
                    format!("{:?} {} {}", rt.resource, dir, rt.threshold)
                } else {
                    "resource".to_string()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breakpoint_id() {
        let id1 = BreakpointId::new();
        let id2 = BreakpointId::new();
        assert_ne!(id1, id2);
        assert!(id1.0 < id2.0);
    }

    #[test]
    fn test_function_breakpoint() {
        let bp = Breakpoint::function("_start");
        assert_eq!(bp.bp_type, BreakpointType::Function);
        assert_eq!(bp.name, Some("_start".to_string()));
        assert!(bp.enabled);
    }

    #[test]
    fn test_address_breakpoint() {
        let bp = Breakpoint::address(0x1000);
        assert_eq!(bp.bp_type, BreakpointType::Address);
        assert_eq!(bp.address, Some(0x1000));
    }

    #[test]
    fn test_watchpoint() {
        let bp = Breakpoint::watchpoint(0x2000, 4, WatchMode::Write);
        assert_eq!(bp.bp_type, BreakpointType::Watchpoint);
        assert_eq!(bp.address, Some(0x2000));
        assert_eq!(bp.watch_size, Some(4));
        assert_eq!(bp.watch_mode, WatchMode::Write);
    }

    #[test]
    fn test_wasi_breakpoint() {
        let bp = Breakpoint::wasi_call("fd_write");
        assert_eq!(bp.bp_type, BreakpointType::WasiCall);
        assert_eq!(bp.wasi_call, Some("fd_write".to_string()));
    }

    #[test]
    fn test_capability_breakpoint() {
        let bp = Breakpoint::capability("filesystem:read");
        assert_eq!(bp.bp_type, BreakpointType::CapabilityCheck);
        assert_eq!(bp.capability, Some("filesystem:read".to_string()));
    }

    #[test]
    fn test_resource_breakpoint() {
        let bp = Breakpoint::resource(ResourceType::Memory, 1024 * 1024, true);
        assert_eq!(bp.bp_type, BreakpointType::ResourceThreshold);
        let rt = bp.resource_threshold.unwrap();
        assert_eq!(rt.resource, ResourceType::Memory);
        assert_eq!(rt.threshold, 1024 * 1024);
        assert!(rt.trigger_above);
    }

    #[test]
    fn test_breakpoint_with_condition() {
        let bp = Breakpoint::function("main").with_condition(BreakpointCondition::hit_count(5));
        assert!(bp.condition.is_some());
        assert_eq!(bp.condition.unwrap().hit_count, Some(5));
    }

    #[test]
    fn test_breakpoint_with_label() {
        let bp = Breakpoint::function("main").with_label("entry point");
        assert_eq!(bp.label, Some("entry point".to_string()));
    }

    #[test]
    fn test_breakpoint_enable_disable() {
        let mut bp = Breakpoint::function("test");
        assert!(bp.enabled);

        bp.disable();
        assert!(!bp.enabled);

        bp.enable();
        assert!(bp.enabled);
    }

    #[test]
    fn test_breakpoint_hit_count() {
        let mut bp = Breakpoint::function("test");
        assert_eq!(bp.hit_count, 0);

        assert_eq!(bp.record_hit(), 1);
        assert_eq!(bp.record_hit(), 2);
        assert_eq!(bp.hit_count, 2);
    }

    #[test]
    fn test_should_trigger_disabled() {
        let mut bp = Breakpoint::function("test");
        bp.disable();
        assert!(!bp.should_trigger());
    }

    #[test]
    fn test_should_trigger_hit_count() {
        let mut bp = Breakpoint::function("test").with_condition(BreakpointCondition::hit_count(3));

        bp.record_hit();
        assert!(!bp.should_trigger()); // hit_count = 1 < 3

        bp.record_hit();
        assert!(!bp.should_trigger()); // hit_count = 2 < 3

        bp.record_hit();
        assert!(bp.should_trigger()); // hit_count = 3 >= 3
    }

    #[test]
    fn test_breakpoint_description() {
        assert!(Breakpoint::function("main").description().contains("main"));
        assert!(Breakpoint::address(0x1000).description().contains("0x1000"));
        assert!(Breakpoint::wasi_call("fd_read").description().contains("fd_read"));
    }

    #[test]
    fn test_breakpoint_condition_types() {
        let c1 = BreakpointCondition::hit_count(10);
        assert_eq!(c1.hit_count, Some(10));

        let c2 = BreakpointCondition::expression("x > 5");
        assert_eq!(c2.expression, "x > 5");

        let c3 = BreakpointCondition::equals("status", "200");
        assert_eq!(c3.expression, "status");
        assert_eq!(c3.expected_value, Some("200".to_string()));
    }

    #[test]
    fn test_breakpoint_for_sandbox() {
        let sandbox_id = Uuid::new_v4();
        let bp = Breakpoint::function("test").for_sandbox(sandbox_id);
        assert_eq!(bp.sandbox_id, Some(sandbox_id));
    }
}
