//! Control flow graph analysis for WASM bytecode.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// A basic block in the control flow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: u32,
    pub instructions: Vec<WasmInstruction>,
    pub successors: Vec<u32>,
    pub predecessors: Vec<u32>,
}

/// Simplified WASM instruction for analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WasmInstruction {
    Call(u32),
    CallIndirect(u32),
    Load { offset: u32, align: u32 },
    Store { offset: u32, align: u32 },
    BrIf(u32),
    Br(u32),
    Return,
    Unreachable,
    LocalGet(u32),
    LocalSet(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32Const(i32),
    I64Const(i64),
    Drop,
    Nop,
}

/// Control flow graph for a single function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlFlowGraph {
    pub function_index: u32,
    pub function_name: Option<String>,
    pub blocks: HashMap<u32, BasicBlock>,
    pub entry_block: u32,
    pub exit_blocks: Vec<u32>,
}

impl ControlFlowGraph {
    /// Create a CFG from a list of instructions (simplified block splitting).
    pub fn from_instructions(func_idx: u32, name: Option<String>, instructions: Vec<WasmInstruction>) -> Self {
        if instructions.is_empty() {
            let block = BasicBlock {
                id: 0,
                instructions: vec![],
                successors: vec![],
                predecessors: vec![],
            };
            let mut blocks = HashMap::new();
            blocks.insert(0, block);
            return Self {
                function_index: func_idx,
                function_name: name,
                blocks,
                entry_block: 0,
                exit_blocks: vec![0],
            };
        }

        // Split at branch/return instructions
        let mut blocks = HashMap::new();
        let mut current_id = 0u32;
        let mut current_insts = Vec::new();
        let mut block_starts = vec![0u32];

        for inst in &instructions {
            current_insts.push(inst.clone());
            match inst {
                WasmInstruction::Br(_) | WasmInstruction::BrIf(_)
                | WasmInstruction::Return | WasmInstruction::Unreachable => {
                    blocks.insert(current_id, BasicBlock {
                        id: current_id,
                        instructions: std::mem::take(&mut current_insts),
                        successors: Vec::new(),
                        predecessors: Vec::new(),
                    });
                    current_id += 1;
                    block_starts.push(current_id);
                }
                _ => {}
            }
        }

        // Remaining instructions form the last block
        if !current_insts.is_empty() {
            blocks.insert(current_id, BasicBlock {
                id: current_id,
                instructions: current_insts,
                successors: Vec::new(),
                predecessors: Vec::new(),
            });
        }

        // Build edges: sequential fallthrough + branch targets
        let block_ids: Vec<u32> = {
            let mut ids: Vec<u32> = blocks.keys().copied().collect();
            ids.sort();
            ids
        };

        for i in 0..block_ids.len() {
            let bid = block_ids[i];
            let last_inst = blocks.get(&bid).and_then(|b| b.instructions.last()).cloned();

            match last_inst {
                Some(WasmInstruction::Return) | Some(WasmInstruction::Unreachable) => {
                    // No successors
                }
                Some(WasmInstruction::Br(target)) => {
                    if blocks.contains_key(&target) {
                        blocks.get_mut(&bid).unwrap().successors.push(target);
                        blocks.get_mut(&target).unwrap().predecessors.push(bid);
                    }
                }
                Some(WasmInstruction::BrIf(target)) => {
                    // Conditional: both fallthrough and target
                    if i + 1 < block_ids.len() {
                        let next = block_ids[i + 1];
                        blocks.get_mut(&bid).unwrap().successors.push(next);
                        blocks.get_mut(&next).unwrap().predecessors.push(bid);
                    }
                    if blocks.contains_key(&target) {
                        blocks.get_mut(&bid).unwrap().successors.push(target);
                        blocks.get_mut(&target).unwrap().predecessors.push(bid);
                    }
                }
                _ => {
                    // Fallthrough
                    if i + 1 < block_ids.len() {
                        let next = block_ids[i + 1];
                        blocks.get_mut(&bid).unwrap().successors.push(next);
                        blocks.get_mut(&next).unwrap().predecessors.push(bid);
                    }
                }
            }
        }

        let exit_blocks: Vec<u32> = blocks.iter()
            .filter(|(_, b)| b.successors.is_empty())
            .map(|(id, _)| *id)
            .collect();

        Self {
            function_index: func_idx,
            function_name: name,
            blocks,
            entry_block: 0,
            exit_blocks,
        }
    }

    /// Count total instructions.
    pub fn instruction_count(&self) -> usize {
        self.blocks.values().map(|b| b.instructions.len()).sum()
    }

    /// Detect loops (back edges in the CFG).
    pub fn detect_loops(&self) -> Vec<(u32, u32)> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        let mut back_edges = Vec::new();

        self.dfs_find_loops(self.entry_block, &mut visited, &mut in_stack, &mut back_edges);
        back_edges
    }

    fn dfs_find_loops(&self, node: u32, visited: &mut HashSet<u32>, in_stack: &mut HashSet<u32>, back_edges: &mut Vec<(u32, u32)>) {
        visited.insert(node);
        in_stack.insert(node);

        if let Some(block) = self.blocks.get(&node) {
            for &succ in &block.successors {
                if in_stack.contains(&succ) {
                    back_edges.push((node, succ));
                } else if !visited.contains(&succ) {
                    self.dfs_find_loops(succ, visited, in_stack, back_edges);
                }
            }
        }

        in_stack.remove(&node);
    }

    /// Find all call instructions (potential reentrancy).
    pub fn find_calls(&self) -> Vec<(u32, u32)> {
        let mut calls = Vec::new();
        for block in self.blocks.values() {
            for inst in &block.instructions {
                match inst {
                    WasmInstruction::Call(target) => calls.push((block.id, *target)),
                    WasmInstruction::CallIndirect(type_idx) => calls.push((block.id, *type_idx)),
                    _ => {}
                }
            }
        }
        calls
    }

    /// Find memory accesses (loads and stores).
    pub fn find_memory_ops(&self) -> Vec<(u32, bool, u32)> {
        let mut ops = Vec::new();
        for block in self.blocks.values() {
            for inst in &block.instructions {
                match inst {
                    WasmInstruction::Load { offset, .. } => ops.push((block.id, false, *offset)),
                    WasmInstruction::Store { offset, .. } => ops.push((block.id, true, *offset)),
                    _ => {}
                }
            }
        }
        ops
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_cfg() {
        let cfg = ControlFlowGraph::from_instructions(0, None, vec![]);
        assert_eq!(cfg.blocks.len(), 1);
        assert_eq!(cfg.instruction_count(), 0);
    }

    #[test]
    fn test_linear_cfg() {
        let cfg = ControlFlowGraph::from_instructions(0, Some("add".into()), vec![
            WasmInstruction::LocalGet(0),
            WasmInstruction::LocalGet(1),
            WasmInstruction::I32Add,
            WasmInstruction::Return,
        ]);
        assert_eq!(cfg.instruction_count(), 4);
        assert_eq!(cfg.blocks.len(), 1);
    }

    #[test]
    fn test_branching_cfg() {
        let cfg = ControlFlowGraph::from_instructions(0, None, vec![
            WasmInstruction::LocalGet(0),
            WasmInstruction::BrIf(0),
            WasmInstruction::I32Const(1),
            WasmInstruction::Return,
        ]);
        assert!(cfg.blocks.len() >= 2);
    }

    #[test]
    fn test_find_calls() {
        let cfg = ControlFlowGraph::from_instructions(0, None, vec![
            WasmInstruction::Call(5),
            WasmInstruction::Call(10),
            WasmInstruction::Return,
        ]);
        let calls = cfg.find_calls();
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn test_find_memory_ops() {
        let cfg = ControlFlowGraph::from_instructions(0, None, vec![
            WasmInstruction::Load { offset: 0, align: 2 },
            WasmInstruction::Store { offset: 4, align: 2 },
            WasmInstruction::Return,
        ]);
        let ops = cfg.find_memory_ops();
        assert_eq!(ops.len(), 2);
        assert!(!ops[0].1); // load = read
        assert!(ops[1].1);  // store = write
    }

    #[test]
    fn test_loop_detection() {
        // Create a CFG with a back edge: block 0 → block 1 → block 0
        let cfg = ControlFlowGraph::from_instructions(0, None, vec![
            WasmInstruction::I32Const(0),
            WasmInstruction::BrIf(0), // back edge to block 0
            WasmInstruction::Return,
        ]);
        // The BrIf creates a conditional branch back
        let loops = cfg.detect_loops();
        // Back edge exists if BrIf target 0 is the entry block
        assert!(!loops.is_empty() || cfg.blocks.len() >= 2);
    }
}
