//! WASM intermediate representation.

use serde::{Deserialize, Serialize};

/// Value types in the IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IRType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}

impl std::fmt::Display for IRType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::I32 => write!(f, "i32"),
            Self::I64 => write!(f, "i64"),
            Self::F32 => write!(f, "f32"),
            Self::F64 => write!(f, "f64"),
            Self::V128 => write!(f, "v128"),
            Self::FuncRef => write!(f, "funcref"),
            Self::ExternRef => write!(f, "externref"),
        }
    }
}

/// IR instructions (simplified WASM instruction set).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IRInstruction {
    // Constants
    Const(IRType, i64),

    // Local/Global
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),

    // Arithmetic
    Add(IRType),
    Sub(IRType),
    Mul(IRType),
    DivS(IRType),
    DivU(IRType),
    RemS(IRType),
    RemU(IRType),

    // Comparison
    Eq(IRType),
    Ne(IRType),
    LtS(IRType),
    GtS(IRType),
    LeS(IRType),
    GeS(IRType),

    // Bitwise
    And(IRType),
    Or(IRType),
    Xor(IRType),
    Shl(IRType),
    ShrS(IRType),
    ShrU(IRType),

    // Memory
    Load(IRType, u32, u32),  // type, offset, align
    Store(IRType, u32, u32), // type, offset, align
    MemorySize,
    MemoryGrow,

    // Control flow
    Block(u32),
    Loop(u32),
    If(u32),
    Else,
    End,
    Br(u32),
    BrIf(u32),
    Call(u32),
    CallIndirect(u32),
    Return,
    Unreachable,

    // Stack
    Drop,
    Select,

    // Nop (can be inserted/removed by optimizer)
    Nop,
}

impl IRInstruction {
    /// Is this a side-effect-free instruction?
    pub fn is_pure(&self) -> bool {
        matches!(
            self,
            Self::Const(..) | Self::LocalGet(_) | Self::GlobalGet(_)
            | Self::Add(_) | Self::Sub(_) | Self::Mul(_)
            | Self::Eq(_) | Self::Ne(_) | Self::LtS(_) | Self::GtS(_)
            | Self::And(_) | Self::Or(_) | Self::Xor(_)
            | Self::Select | Self::Nop | Self::Drop
        )
    }

    /// Does this instruction have side effects?
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self,
            Self::LocalSet(_) | Self::LocalTee(_) | Self::GlobalSet(_)
            | Self::Store(..) | Self::MemoryGrow
            | Self::Call(_) | Self::CallIndirect(_)
        )
    }

    /// Is this a branch/control flow instruction?
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self,
            Self::Block(_) | Self::Loop(_) | Self::If(_) | Self::Else | Self::End
            | Self::Br(_) | Self::BrIf(_) | Self::Return | Self::Unreachable
        )
    }

    /// Is this a memory access?
    pub fn is_memory_access(&self) -> bool {
        matches!(self, Self::Load(..) | Self::Store(..) | Self::MemorySize | Self::MemoryGrow)
    }
}

/// A function in the IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IRFunction {
    pub index: u32,
    pub name: Option<String>,
    pub params: Vec<IRType>,
    pub results: Vec<IRType>,
    pub body: Vec<IRInstruction>,
    pub is_exported: bool,
}

impl IRFunction {
    /// Count instructions (excluding Nop).
    pub fn instruction_count(&self) -> usize {
        self.body.iter().filter(|i| !matches!(i, IRInstruction::Nop)).count()
    }

    /// Count memory operations.
    pub fn memory_op_count(&self) -> usize {
        self.body.iter().filter(|i| i.is_memory_access()).count()
    }

    /// Count call instructions.
    pub fn call_count(&self) -> usize {
        self.body.iter().filter(|i| matches!(i, IRInstruction::Call(_) | IRInstruction::CallIndirect(_))).count()
    }
}

/// Complete WASM module in IR form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmIR {
    pub module_name: String,
    pub functions: Vec<IRFunction>,
    pub memory_pages: u32,
    pub table_size: u32,
    pub globals: Vec<(IRType, bool)>, // (type, mutable)
}

impl WasmIR {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            module_name: name.into(),
            functions: Vec::new(),
            memory_pages: 1,
            table_size: 0,
            globals: Vec::new(),
        }
    }

    pub fn add_function(&mut self, func: IRFunction) {
        self.functions.push(func);
    }

    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    pub fn exported_functions(&self) -> Vec<&IRFunction> {
        self.functions.iter().filter(|f| f.is_exported).collect()
    }

    /// Total instruction count across all functions.
    pub fn total_instructions(&self) -> usize {
        self.functions.iter().map(|f| f.instruction_count()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_type_display() {
        assert_eq!(IRType::I32.to_string(), "i32");
        assert_eq!(IRType::F64.to_string(), "f64");
    }

    #[test]
    fn test_instruction_classification() {
        assert!(IRInstruction::Add(IRType::I32).is_pure());
        assert!(!IRInstruction::Call(0).is_pure());
        assert!(IRInstruction::Store(IRType::I32, 0, 2).has_side_effects());
        assert!(IRInstruction::Br(0).is_control_flow());
        assert!(IRInstruction::Load(IRType::I32, 0, 2).is_memory_access());
    }

    #[test]
    fn test_build_ir_module() {
        let mut ir = WasmIR::new("test");
        ir.add_function(IRFunction {
            index: 0,
            name: Some("main".into()),
            params: vec![],
            results: vec![IRType::I32],
            body: vec![
                IRInstruction::Const(IRType::I32, 42),
                IRInstruction::Return,
            ],
            is_exported: true,
        });
        ir.add_function(IRFunction {
            index: 1,
            name: Some("helper".into()),
            params: vec![IRType::I32],
            results: vec![IRType::I32],
            body: vec![
                IRInstruction::LocalGet(0),
                IRInstruction::Return,
            ],
            is_exported: false,
        });

        assert_eq!(ir.function_count(), 2);
        assert_eq!(ir.exported_functions().len(), 1);
        assert_eq!(ir.total_instructions(), 4);
    }

    #[test]
    fn test_function_metrics() {
        let func = IRFunction {
            index: 0,
            name: None,
            params: vec![],
            results: vec![],
            body: vec![
                IRInstruction::Load(IRType::I32, 0, 2),
                IRInstruction::Store(IRType::I32, 4, 2),
                IRInstruction::Call(1),
                IRInstruction::Nop,
                IRInstruction::Return,
            ],
            is_exported: false,
        };

        assert_eq!(func.instruction_count(), 4); // excluding Nop
        assert_eq!(func.memory_op_count(), 2);
        assert_eq!(func.call_count(), 1);
    }

    #[test]
    fn test_empty_module() {
        let ir = WasmIR::new("empty");
        assert_eq!(ir.function_count(), 0);
        assert_eq!(ir.total_instructions(), 0);
        assert!(ir.exported_functions().is_empty());
    }
}
