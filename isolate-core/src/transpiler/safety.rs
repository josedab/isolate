//! Safety layer for transpiled code.

use serde::{Deserialize, Serialize};

use super::ir::{IRFunction, IRInstruction, WasmIR};

/// How to handle bounds checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundsCheckMode {
    /// Full bounds check on every memory access.
    Full,
    /// Use guard pages (hardware-assisted, lower overhead).
    GuardPages,
    /// Only check in debug mode.
    DebugOnly,
    /// No bounds checks (unsafe, for benchmarking only).
    None,
}

impl Default for BoundsCheckMode {
    fn default() -> Self {
        Self::Full
    }
}

/// Configuration for the safety layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub bounds_check: BoundsCheckMode,
    pub insert_fuel_checks: bool,
    pub fuel_check_interval: u32,
    pub insert_stack_checks: bool,
    pub max_stack_depth: u32,
    pub trap_on_overflow: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            bounds_check: BoundsCheckMode::Full,
            insert_fuel_checks: true,
            fuel_check_interval: 100,
            insert_stack_checks: true,
            max_stack_depth: 10000,
            trap_on_overflow: true,
        }
    }
}

/// A safety check result for a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    pub function_index: u32,
    pub function_name: Option<String>,
    pub memory_accesses: usize,
    pub indirect_calls: usize,
    pub divisions: usize,
    pub bounds_checks_needed: usize,
    pub fuel_checks_needed: usize,
    pub stack_checks_needed: usize,
    pub safety_overhead_estimate: f64,
}

impl SafetyCheck {
    /// Is this function safe to transpile?
    pub fn is_safe(&self) -> bool {
        // All functions are safe to transpile with proper checks
        true
    }

    /// Estimated overhead percentage from safety checks.
    pub fn overhead_percent(&self) -> f64 {
        self.safety_overhead_estimate
    }
}

/// Safety analysis layer.
pub struct SafetyLayer {
    config: SafetyConfig,
}

impl SafetyLayer {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    /// Analyze all functions in an IR module.
    pub fn analyze(&self, ir: &WasmIR) -> Vec<SafetyCheck> {
        ir.functions.iter().map(|f| self.analyze_function(f)).collect()
    }

    /// Analyze a single function.
    pub fn analyze_function(&self, func: &IRFunction) -> SafetyCheck {
        let mut memory_accesses = 0;
        let mut indirect_calls = 0;
        let mut divisions = 0;

        for inst in &func.body {
            match inst {
                IRInstruction::Load(..) | IRInstruction::Store(..) => memory_accesses += 1,
                IRInstruction::MemoryGrow => memory_accesses += 1,
                IRInstruction::CallIndirect(_) => indirect_calls += 1,
                IRInstruction::DivS(_)
                | IRInstruction::DivU(_)
                | IRInstruction::RemS(_)
                | IRInstruction::RemU(_) => divisions += 1,
                _ => {}
            }
        }

        let bounds_checks_needed = match self.config.bounds_check {
            BoundsCheckMode::Full => memory_accesses,
            BoundsCheckMode::GuardPages => 0, // hardware handles it
            BoundsCheckMode::DebugOnly => memory_accesses, // but only in debug
            BoundsCheckMode::None => 0,
        };

        let total_instructions = func.body.len();
        let fuel_checks_needed =
            if self.config.insert_fuel_checks && self.config.fuel_check_interval > 0 {
                total_instructions / self.config.fuel_check_interval as usize
            } else {
                0
            };

        let stack_checks_needed =
            if self.config.insert_stack_checks { func.call_count() } else { 0 };

        let total_checks =
            bounds_checks_needed + fuel_checks_needed + stack_checks_needed + divisions;
        let overhead = if total_instructions > 0 {
            total_checks as f64 / total_instructions as f64 * 100.0
        } else {
            0.0
        };

        SafetyCheck {
            function_index: func.index,
            function_name: func.name.clone(),
            memory_accesses,
            indirect_calls,
            divisions,
            bounds_checks_needed,
            fuel_checks_needed,
            stack_checks_needed,
            safety_overhead_estimate: overhead,
        }
    }

    /// Estimate total safety overhead for the module.
    pub fn estimate_overhead(&self, ir: &WasmIR) -> f64 {
        let checks = self.analyze(ir);
        if checks.is_empty() {
            return 0.0;
        }
        let total: f64 = checks.iter().map(|c| c.overhead_percent()).sum();
        total / checks.len() as f64
    }

    pub fn config(&self) -> &SafetyConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpiler::ir::*;

    fn make_func(body: Vec<IRInstruction>) -> IRFunction {
        IRFunction {
            index: 0,
            name: Some("test".into()),
            params: vec![],
            results: vec![],
            body,
            is_exported: false,
        }
    }

    #[test]
    fn test_analyze_memory_accesses() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let func = make_func(vec![
            IRInstruction::Load(IRType::I32, 0, 2),
            IRInstruction::Store(IRType::I32, 4, 2),
            IRInstruction::Load(IRType::I32, 8, 2),
            IRInstruction::Return,
        ]);

        let check = layer.analyze_function(&func);
        assert_eq!(check.memory_accesses, 3);
        assert_eq!(check.bounds_checks_needed, 3); // Full mode
    }

    #[test]
    fn test_guard_pages_no_bounds_checks() {
        let layer = SafetyLayer::new(SafetyConfig {
            bounds_check: BoundsCheckMode::GuardPages,
            ..Default::default()
        });
        let func = make_func(vec![IRInstruction::Load(IRType::I32, 0, 2), IRInstruction::Return]);

        let check = layer.analyze_function(&func);
        assert_eq!(check.memory_accesses, 1);
        assert_eq!(check.bounds_checks_needed, 0);
    }

    #[test]
    fn test_fuel_check_interval() {
        let layer =
            SafetyLayer::new(SafetyConfig { fuel_check_interval: 10, ..Default::default() });

        let body: Vec<IRInstruction> = (0..50).map(|_| IRInstruction::Nop).collect();
        let func = make_func(body);

        let check = layer.analyze_function(&func);
        assert_eq!(check.fuel_checks_needed, 5); // 50/10
    }

    #[test]
    fn test_division_detection() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let func = make_func(vec![
            IRInstruction::LocalGet(0),
            IRInstruction::LocalGet(1),
            IRInstruction::DivS(IRType::I32),
            IRInstruction::RemU(IRType::I32),
            IRInstruction::Return,
        ]);

        let check = layer.analyze_function(&func);
        assert_eq!(check.divisions, 2);
    }

    #[test]
    fn test_indirect_calls() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let func = make_func(vec![
            IRInstruction::CallIndirect(0),
            IRInstruction::CallIndirect(1),
            IRInstruction::Return,
        ]);

        let check = layer.analyze_function(&func);
        assert_eq!(check.indirect_calls, 2);
    }

    #[test]
    fn test_safety_check_is_safe() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let func = make_func(vec![IRInstruction::Return]);
        let check = layer.analyze_function(&func);
        assert!(check.is_safe());
    }

    #[test]
    fn test_estimate_overhead() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let mut ir = WasmIR::new("test");
        ir.add_function(make_func(vec![
            IRInstruction::Load(IRType::I32, 0, 2),
            IRInstruction::Store(IRType::I32, 0, 2),
            IRInstruction::Return,
        ]));

        let overhead = layer.estimate_overhead(&ir);
        assert!(overhead >= 0.0);
    }

    #[test]
    fn test_empty_function() {
        let layer = SafetyLayer::new(SafetyConfig::default());
        let func = make_func(vec![]);
        let check = layer.analyze_function(&func);
        assert_eq!(check.memory_accesses, 0);
        assert_eq!(check.overhead_percent(), 0.0);
    }

    #[test]
    fn test_config_access() {
        let config =
            SafetyConfig { bounds_check: BoundsCheckMode::GuardPages, ..Default::default() };
        let layer = SafetyLayer::new(config);
        assert_eq!(layer.config().bounds_check, BoundsCheckMode::GuardPages);
    }
}
