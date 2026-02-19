//! WASM-to-Native Transpiler.
//!
//! AOT compilation of WASM modules to native code with sandbox
//! compatibility layer for maintaining isolation guarantees.
//!
//! # Features
//!
//! - **WASM IR**: Intermediate representation for analysis and optimization
//! - **Optimization Passes**: Dead code elimination, constant folding, inlining
//! - **Safety Layer**: Bounds checking, trap handling, capability enforcement
//! - **Codegen Abstraction**: Target-independent native code generation



pub mod ir;
pub mod optimizer;
pub mod safety;

pub use ir::{WasmIR, IRFunction, IRInstruction, IRType};
pub use optimizer::{OptimizationPass, PassManager, OptimizationLevel, OptimizationStats};
pub use safety::{SafetyLayer, SafetyConfig, SafetyCheck, BoundsCheckMode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_pipeline() {
        // Build IR
        let mut ir = WasmIR::new("test-module");
        ir.add_function(IRFunction {
            index: 0,
            name: Some("add".into()),
            params: vec![IRType::I32, IRType::I32],
            results: vec![IRType::I32],
            body: vec![
                IRInstruction::LocalGet(0),
                IRInstruction::LocalGet(1),
                IRInstruction::Add(IRType::I32),
                IRInstruction::Return,
            ],
            is_exported: true,
        });

        // Optimize
        let mut pm = PassManager::new(OptimizationLevel::Standard);
        let stats = pm.run(&mut ir);
        assert!(stats.passes_run > 0);

        // Safety layer
        let safety = SafetyLayer::new(SafetyConfig::default());
        let checks = safety.analyze(&ir);
        assert!(!checks.is_empty());
    }
}
