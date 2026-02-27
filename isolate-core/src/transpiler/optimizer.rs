//! Optimization passes for the WASM IR.

use serde::{Deserialize, Serialize};

use super::ir::{IRInstruction, WasmIR};

/// Optimization level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimizationLevel {
    /// No optimizations.
    None,
    /// Basic optimizations (dead code, constant folding).
    Basic,
    /// Standard optimizations (+ strength reduction, peephole).
    Standard,
    /// Aggressive optimizations (+ inlining, loop opts).
    Aggressive,
}

/// Statistics from optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationStats {
    pub passes_run: usize,
    pub instructions_before: usize,
    pub instructions_after: usize,
    pub nops_eliminated: usize,
    pub constants_folded: usize,
    pub dead_code_removed: usize,
}

impl OptimizationStats {
    pub fn reduction_percent(&self) -> f64 {
        if self.instructions_before == 0 {
            return 0.0;
        }
        (1.0 - self.instructions_after as f64 / self.instructions_before as f64) * 100.0
    }
}

/// An individual optimization pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptimizationPass {
    /// Remove Nop instructions.
    NopElimination,
    /// Fold constant expressions.
    ConstantFolding,
    /// Remove dead code after unconditional branches.
    DeadCodeElimination,
    /// Replace expensive ops with cheaper equivalents.
    StrengthReduction,
    /// Peephole optimizations on instruction windows.
    Peephole,
}

impl OptimizationPass {
    /// Apply this pass to a WASM IR module.
    pub fn apply(&self, ir: &mut WasmIR) -> (usize, usize, usize) {
        let mut nops = 0usize;
        let mut constants = 0usize;
        let mut dead = 0usize;

        for func in &mut ir.functions {
            match self {
                Self::NopElimination => {
                    let before = func.body.len();
                    func.body.retain(|i| !matches!(i, IRInstruction::Nop));
                    nops += before - func.body.len();
                }
                Self::ConstantFolding => {
                    constants += Self::fold_constants(&mut func.body);
                }
                Self::DeadCodeElimination => {
                    dead += Self::eliminate_dead_code(&mut func.body);
                }
                Self::StrengthReduction => {
                    Self::reduce_strength(&mut func.body);
                }
                Self::Peephole => {
                    Self::peephole_optimize(&mut func.body);
                }
            }
        }

        (nops, constants, dead)
    }

    fn fold_constants(body: &mut Vec<IRInstruction>) -> usize {
        let mut folded = 0;
        let len = body.len();
        if len < 3 {
            return 0;
        }

        let mut i = 0;
        while i + 2 < body.len() {
            if let (IRInstruction::Const(t1, a), IRInstruction::Const(t2, b), op) =
                (&body[i], &body[i + 1], &body[i + 2])
            {
                if t1 == t2 {
                    let result = match op {
                        IRInstruction::Add(_) => Some(a.wrapping_add(*b)),
                        IRInstruction::Sub(_) => Some(a.wrapping_sub(*b)),
                        IRInstruction::Mul(_) => Some(a.wrapping_mul(*b)),
                        _ => None,
                    };

                    if let Some(val) = result {
                        let ty = *t1;
                        body[i] = IRInstruction::Const(ty, val);
                        body[i + 1] = IRInstruction::Nop;
                        body[i + 2] = IRInstruction::Nop;
                        folded += 1;
                    }
                }
            }
            i += 1;
        }
        folded
    }

    fn eliminate_dead_code(body: &mut Vec<IRInstruction>) -> usize {
        let mut dead = 0;
        let mut found_return = false;

        for inst in body.iter_mut() {
            if found_return {
                if !matches!(inst, IRInstruction::End | IRInstruction::Else | IRInstruction::Nop) {
                    *inst = IRInstruction::Nop;
                    dead += 1;
                }
            }
            if matches!(inst, IRInstruction::Return | IRInstruction::Unreachable) {
                found_return = true;
            }
            // Reset at block boundaries
            if matches!(
                inst,
                IRInstruction::Block(_)
                    | IRInstruction::Loop(_)
                    | IRInstruction::If(_)
                    | IRInstruction::Else
                    | IRInstruction::End
            ) {
                found_return = false;
            }
        }
        dead
    }

    fn reduce_strength(body: &mut [IRInstruction]) {
        let len = body.len();
        if len < 2 {
            return;
        }

        for i in 0..len.saturating_sub(1) {
            let (ty, val) = match &body[i] {
                IRInstruction::Const(ty, val) => (*ty, *val),
                _ => continue,
            };
            if !matches!(&body[i + 1], IRInstruction::Mul(mul_ty) if *mul_ty == ty) {
                continue;
            }

            if val == 2 {
                body[i] = IRInstruction::Const(ty, 1);
                body[i + 1] = IRInstruction::Shl(ty);
            } else if val == 1 {
                body[i] = IRInstruction::Nop;
                body[i + 1] = IRInstruction::Nop;
            } else if val == 0 {
                body[i] = IRInstruction::Nop;
                body[i + 1] = IRInstruction::Nop;
            }
        }
    }

    fn peephole_optimize(body: &mut [IRInstruction]) {
        let len = body.len();
        if len < 2 {
            return;
        }

        for i in 0..len.saturating_sub(1) {
            let should_nop = match (&body[i], &body[i + 1]) {
                (IRInstruction::LocalGet(a), IRInstruction::LocalSet(b)) if a == b => true,
                (IRInstruction::LocalGet(_), IRInstruction::Drop) => true,
                _ => false,
            };

            if should_nop {
                body[i] = IRInstruction::Nop;
                body[i + 1] = IRInstruction::Nop;
            }
        }
    }
}

/// Manages and runs optimization passes.
pub struct PassManager {
    level: OptimizationLevel,
    passes: Vec<OptimizationPass>,
}

impl PassManager {
    pub fn new(level: OptimizationLevel) -> Self {
        let passes = match level {
            OptimizationLevel::None => vec![],
            OptimizationLevel::Basic => vec![
                OptimizationPass::NopElimination,
                OptimizationPass::ConstantFolding,
                OptimizationPass::DeadCodeElimination,
            ],
            OptimizationLevel::Standard => vec![
                OptimizationPass::ConstantFolding,
                OptimizationPass::DeadCodeElimination,
                OptimizationPass::StrengthReduction,
                OptimizationPass::Peephole,
                OptimizationPass::NopElimination, // run last to clean up
            ],
            OptimizationLevel::Aggressive => vec![
                OptimizationPass::ConstantFolding,
                OptimizationPass::DeadCodeElimination,
                OptimizationPass::StrengthReduction,
                OptimizationPass::Peephole,
                OptimizationPass::ConstantFolding, // second pass
                OptimizationPass::NopElimination,
            ],
        };

        Self { level, passes }
    }

    /// Run all passes on the IR.
    pub fn run(&mut self, ir: &mut WasmIR) -> OptimizationStats {
        let instructions_before = ir.total_instructions();
        let mut total_nops = 0;
        let mut total_constants = 0;
        let mut total_dead = 0;

        for pass in &self.passes {
            let (nops, constants, dead) = pass.apply(ir);
            total_nops += nops;
            total_constants += constants;
            total_dead += dead;
        }

        OptimizationStats {
            passes_run: self.passes.len(),
            instructions_before,
            instructions_after: ir.total_instructions(),
            nops_eliminated: total_nops,
            constants_folded: total_constants,
            dead_code_removed: total_dead,
        }
    }

    pub fn level(&self) -> OptimizationLevel {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transpiler::ir::IRFunction;

    fn make_ir(body: Vec<IRInstruction>) -> WasmIR {
        let mut ir = WasmIR::new("test");
        ir.add_function(IRFunction {
            index: 0,
            name: Some("test".into()),
            params: vec![],
            results: vec![IRType::I32],
            body,
            is_exported: true,
        });
        ir
    }

    #[test]
    fn test_nop_elimination() {
        let mut ir = make_ir(vec![
            IRInstruction::Nop,
            IRInstruction::Const(IRType::I32, 42),
            IRInstruction::Nop,
            IRInstruction::Return,
        ]);

        let (nops, _, _) = OptimizationPass::NopElimination.apply(&mut ir);
        assert_eq!(nops, 2);
        assert_eq!(ir.functions[0].body.len(), 2);
    }

    #[test]
    fn test_constant_folding() {
        let mut ir = make_ir(vec![
            IRInstruction::Const(IRType::I32, 10),
            IRInstruction::Const(IRType::I32, 20),
            IRInstruction::Add(IRType::I32),
            IRInstruction::Return,
        ]);

        let (_, constants, _) = OptimizationPass::ConstantFolding.apply(&mut ir);
        assert_eq!(constants, 1);
        // First instruction should now be Const(30)
        assert_eq!(ir.functions[0].body[0], IRInstruction::Const(IRType::I32, 30));
    }

    #[test]
    fn test_dead_code_elimination() {
        let mut ir = make_ir(vec![
            IRInstruction::Const(IRType::I32, 42),
            IRInstruction::Return,
            IRInstruction::Const(IRType::I32, 99), // dead
            IRInstruction::Call(1),                // dead
        ]);

        let (_, _, dead) = OptimizationPass::DeadCodeElimination.apply(&mut ir);
        assert_eq!(dead, 2);
    }

    #[test]
    fn test_strength_reduction_mul2() {
        let mut ir = make_ir(vec![
            IRInstruction::LocalGet(0),
            IRInstruction::Const(IRType::I32, 2),
            IRInstruction::Mul(IRType::I32),
            IRInstruction::Return,
        ]);

        OptimizationPass::StrengthReduction.apply(&mut ir);
        // Should convert mul(2) to shl(1)
        assert!(ir.functions[0].body.iter().any(|i| matches!(i, IRInstruction::Shl(IRType::I32))));
    }

    #[test]
    fn test_peephole_local_get_set() {
        let mut ir = make_ir(vec![
            IRInstruction::LocalGet(0),
            IRInstruction::LocalSet(0), // redundant
            IRInstruction::Return,
        ]);

        OptimizationPass::Peephole.apply(&mut ir);
        assert_eq!(ir.functions[0].body[0], IRInstruction::Nop);
        assert_eq!(ir.functions[0].body[1], IRInstruction::Nop);
    }

    #[test]
    fn test_pass_manager_standard() {
        let mut ir = make_ir(vec![
            IRInstruction::Const(IRType::I32, 5),
            IRInstruction::Const(IRType::I32, 10),
            IRInstruction::Add(IRType::I32),
            IRInstruction::Nop,
            IRInstruction::Return,
        ]);

        let mut pm = PassManager::new(OptimizationLevel::Standard);
        let stats = pm.run(&mut ir);
        assert!(stats.passes_run >= 4);
        assert!(stats.instructions_after <= stats.instructions_before);
    }

    #[test]
    fn test_pass_manager_none() {
        let mut ir = make_ir(vec![IRInstruction::Nop, IRInstruction::Nop, IRInstruction::Return]);

        let mut pm = PassManager::new(OptimizationLevel::None);
        let stats = pm.run(&mut ir);
        assert_eq!(stats.passes_run, 0);
    }

    #[test]
    fn test_reduction_percent() {
        let stats = OptimizationStats {
            passes_run: 3,
            instructions_before: 100,
            instructions_after: 70,
            nops_eliminated: 10,
            constants_folded: 5,
            dead_code_removed: 15,
        };
        assert!((stats.reduction_percent() - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_empty_module_optimization() {
        let mut ir = WasmIR::new("empty");
        let mut pm = PassManager::new(OptimizationLevel::Aggressive);
        let stats = pm.run(&mut ir);
        assert_eq!(stats.instructions_before, 0);
        assert_eq!(stats.instructions_after, 0);
    }
}
