//! Runtime inspection utilities.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Stack frame information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// Frame index (0 = top of stack).
    pub index: usize,
    /// Function name (if available).
    pub function_name: Option<String>,
    /// Module name.
    pub module_name: Option<String>,
    /// Instruction pointer/program counter.
    pub instruction_ptr: u64,
    /// Function index in WASM module.
    pub func_index: Option<u32>,
    /// Local variables in this frame.
    pub locals: Vec<Variable>,
    /// Source file (if debug info available).
    pub source_file: Option<String>,
    /// Line number (if debug info available).
    pub line_number: Option<u32>,
    /// Column number (if debug info available).
    pub column_number: Option<u32>,
}

impl StackFrame {
    /// Create a new stack frame.
    pub fn new(index: usize, instruction_ptr: u64) -> Self {
        Self {
            index,
            function_name: None,
            module_name: None,
            instruction_ptr,
            func_index: None,
            locals: Vec::new(),
            source_file: None,
            line_number: None,
            column_number: None,
        }
    }

    /// Set the function name.
    pub fn with_function_name(mut self, name: impl Into<String>) -> Self {
        self.function_name = Some(name.into());
        self
    }

    /// Set the module name.
    pub fn with_module_name(mut self, name: impl Into<String>) -> Self {
        self.module_name = Some(name.into());
        self
    }

    /// Set the function index.
    pub fn with_func_index(mut self, index: u32) -> Self {
        self.func_index = Some(index);
        self
    }

    /// Add a local variable.
    pub fn with_local(mut self, var: Variable) -> Self {
        self.locals.push(var);
        self
    }

    /// Set source location.
    pub fn with_source(mut self, file: impl Into<String>, line: u32, column: u32) -> Self {
        self.source_file = Some(file.into());
        self.line_number = Some(line);
        self.column_number = Some(column);
        self
    }

    /// Get a formatted source location.
    pub fn source_location(&self) -> Option<String> {
        match (&self.source_file, self.line_number) {
            (Some(file), Some(line)) => {
                if let Some(col) = self.column_number {
                    Some(format!("{}:{}:{}", file, line, col))
                } else {
                    Some(format!("{}:{}", file, line))
                }
            }
            _ => None,
        }
    }
}

/// Variable type information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableType {
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
    /// 128-bit vector.
    V128,
    /// Function reference.
    FuncRef,
    /// External reference.
    ExternRef,
    /// Unknown/opaque type.
    Unknown,
}

impl std::fmt::Display for VariableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VariableType::I32 => write!(f, "i32"),
            VariableType::I64 => write!(f, "i64"),
            VariableType::F32 => write!(f, "f32"),
            VariableType::F64 => write!(f, "f64"),
            VariableType::V128 => write!(f, "v128"),
            VariableType::FuncRef => write!(f, "funcref"),
            VariableType::ExternRef => write!(f, "externref"),
            VariableType::Unknown => write!(f, "unknown"),
        }
    }
}

/// A variable with name, type, and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variable {
    /// Variable name (if available).
    pub name: Option<String>,
    /// Variable index (local index in WASM).
    pub index: u32,
    /// Variable type.
    pub var_type: VariableType,
    /// Raw value bytes.
    pub value_bytes: Vec<u8>,
    /// Formatted string value.
    pub value_string: String,
}

impl Variable {
    /// Create a new i32 variable.
    pub fn i32(index: u32, name: Option<String>, value: i32) -> Self {
        Self {
            name,
            index,
            var_type: VariableType::I32,
            value_bytes: value.to_le_bytes().to_vec(),
            value_string: value.to_string(),
        }
    }

    /// Create a new i64 variable.
    pub fn i64(index: u32, name: Option<String>, value: i64) -> Self {
        Self {
            name,
            index,
            var_type: VariableType::I64,
            value_bytes: value.to_le_bytes().to_vec(),
            value_string: value.to_string(),
        }
    }

    /// Create a new f32 variable.
    pub fn f32(index: u32, name: Option<String>, value: f32) -> Self {
        Self {
            name,
            index,
            var_type: VariableType::F32,
            value_bytes: value.to_le_bytes().to_vec(),
            value_string: format!("{:.6}", value),
        }
    }

    /// Create a new f64 variable.
    pub fn f64(index: u32, name: Option<String>, value: f64) -> Self {
        Self {
            name,
            index,
            var_type: VariableType::F64,
            value_bytes: value.to_le_bytes().to_vec(),
            value_string: format!("{:.10}", value),
        }
    }

    /// Get the value as i32 (if applicable).
    pub fn as_i32(&self) -> Option<i32> {
        if self.var_type == VariableType::I32 && self.value_bytes.len() == 4 {
            Some(i32::from_le_bytes(self.value_bytes[..4].try_into().ok()?))
        } else {
            None
        }
    }

    /// Get the value as i64 (if applicable).
    pub fn as_i64(&self) -> Option<i64> {
        if self.var_type == VariableType::I64 && self.value_bytes.len() == 8 {
            Some(i64::from_le_bytes(self.value_bytes[..8].try_into().ok()?))
        } else {
            None
        }
    }

    /// Get the value as f32 (if applicable).
    pub fn as_f32(&self) -> Option<f32> {
        if self.var_type == VariableType::F32 && self.value_bytes.len() == 4 {
            Some(f32::from_le_bytes(self.value_bytes[..4].try_into().ok()?))
        } else {
            None
        }
    }

    /// Get the value as f64 (if applicable).
    pub fn as_f64(&self) -> Option<f64> {
        if self.var_type == VariableType::F64 && self.value_bytes.len() == 8 {
            Some(f64::from_le_bytes(self.value_bytes[..8].try_into().ok()?))
        } else {
            None
        }
    }
}

/// A view into sandbox memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryView {
    /// Starting address.
    pub address: u64,
    /// Memory bytes.
    pub data: Vec<u8>,
    /// Memory region name (if known).
    pub region_name: Option<String>,
    /// Whether memory is readable.
    pub readable: bool,
    /// Whether memory is writable.
    pub writable: bool,
    /// Whether memory is executable.
    pub executable: bool,
    /// Timestamp of capture.
    pub captured_at: DateTime<Utc>,
}

impl MemoryView {
    /// Create a new memory view.
    pub fn new(address: u64, data: Vec<u8>) -> Self {
        Self {
            address,
            data,
            region_name: None,
            readable: true,
            writable: true,
            executable: false,
            captured_at: Utc::now(),
        }
    }

    /// Set the region name.
    pub fn with_region_name(mut self, name: impl Into<String>) -> Self {
        self.region_name = Some(name.into());
        self
    }

    /// Set permissions.
    pub fn with_permissions(mut self, read: bool, write: bool, exec: bool) -> Self {
        self.readable = read;
        self.writable = write;
        self.executable = exec;
        self
    }

    /// Get the length of the memory view.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the memory view is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get end address.
    pub fn end_address(&self) -> u64 {
        self.address + self.data.len() as u64
    }

    /// Read a byte at an offset.
    pub fn read_u8(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    /// Read a u32 at an offset.
    pub fn read_u32_le(&self, offset: usize) -> Option<u32> {
        if offset + 4 <= self.data.len() {
            let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().ok()?;
            Some(u32::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Read a u64 at an offset.
    pub fn read_u64_le(&self, offset: usize) -> Option<u64> {
        if offset + 8 <= self.data.len() {
            let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().ok()?;
            Some(u64::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Format as hex dump.
    pub fn hex_dump(&self) -> String {
        let mut result = String::new();
        let bytes_per_line = 16;

        for (i, chunk) in self.data.chunks(bytes_per_line).enumerate() {
            let addr = self.address + (i * bytes_per_line) as u64;

            // Address
            result.push_str(&format!("{:08x}  ", addr));

            // Hex bytes
            for (j, byte) in chunk.iter().enumerate() {
                if j == 8 {
                    result.push(' ');
                }
                result.push_str(&format!("{:02x} ", byte));
            }

            // Padding for incomplete lines
            if chunk.len() < bytes_per_line {
                let padding = (bytes_per_line - chunk.len()) * 3;
                if chunk.len() < 8 {
                    result.push_str(&" ".repeat(padding + 1));
                } else {
                    result.push_str(&" ".repeat(padding));
                }
            }

            // ASCII representation
            result.push_str(" |");
            for byte in chunk {
                if *byte >= 0x20 && *byte < 0x7f {
                    result.push(*byte as char);
                } else {
                    result.push('.');
                }
            }
            result.push_str("|\n");
        }

        result
    }
}

/// Global variables snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalsSnapshot {
    /// Global variables.
    pub globals: Vec<Variable>,
    /// Capture timestamp.
    pub captured_at: DateTime<Utc>,
}

impl GlobalsSnapshot {
    /// Create a new globals snapshot.
    pub fn new(globals: Vec<Variable>) -> Self {
        Self {
            globals,
            captured_at: Utc::now(),
        }
    }

    /// Get a global by index.
    pub fn get(&self, index: u32) -> Option<&Variable> {
        self.globals.iter().find(|g| g.index == index)
    }

    /// Get a global by name.
    pub fn get_by_name(&self, name: &str) -> Option<&Variable> {
        self.globals
            .iter()
            .find(|g| g.name.as_deref() == Some(name))
    }
}

/// Runtime state inspector.
pub struct Inspector {
    /// Associated sandbox ID.
    sandbox_id: Uuid,
    /// Current call stack.
    call_stack: Vec<StackFrame>,
    /// Cached memory views.
    memory_cache: HashMap<u64, MemoryView>,
    /// Global variables.
    globals: Option<GlobalsSnapshot>,
    /// Watch expressions and their values.
    watches: HashMap<String, String>,
    /// Max stack depth to capture.
    max_stack_depth: usize,
    /// Max memory view size.
    max_memory_size: usize,
}

impl Inspector {
    /// Create a new inspector for a sandbox.
    pub fn new(sandbox_id: Uuid) -> Self {
        Self {
            sandbox_id,
            call_stack: Vec::new(),
            memory_cache: HashMap::new(),
            globals: None,
            watches: HashMap::new(),
            max_stack_depth: super::MAX_STACK_DEPTH,
            max_memory_size: super::MAX_MEMORY_VIEW,
        }
    }

    /// Get the sandbox ID.
    pub fn sandbox_id(&self) -> Uuid {
        self.sandbox_id
    }

    /// Set max stack depth.
    pub fn with_max_stack_depth(mut self, depth: usize) -> Self {
        self.max_stack_depth = depth;
        self
    }

    /// Set max memory size.
    pub fn with_max_memory_size(mut self, size: usize) -> Self {
        self.max_memory_size = size;
        self
    }

    /// Update the call stack.
    pub fn set_call_stack(&mut self, stack: Vec<StackFrame>) {
        self.call_stack = stack;
        if self.call_stack.len() > self.max_stack_depth {
            self.call_stack.truncate(self.max_stack_depth);
        }
    }

    /// Get the current call stack.
    pub fn call_stack(&self) -> &[StackFrame] {
        &self.call_stack
    }

    /// Get the current (top) stack frame.
    pub fn current_frame(&self) -> Option<&StackFrame> {
        self.call_stack.first()
    }

    /// Get a specific stack frame by index.
    pub fn frame(&self, index: usize) -> Option<&StackFrame> {
        self.call_stack.get(index)
    }

    /// Get stack depth.
    pub fn stack_depth(&self) -> usize {
        self.call_stack.len()
    }

    /// Cache a memory view.
    pub fn cache_memory(&mut self, view: MemoryView) {
        // Limit cache size
        if view.len() <= self.max_memory_size {
            self.memory_cache.insert(view.address, view);
        }
    }

    /// Get a cached memory view.
    pub fn get_memory(&self, address: u64) -> Option<&MemoryView> {
        self.memory_cache.get(&address)
    }

    /// Clear memory cache.
    pub fn clear_memory_cache(&mut self) {
        self.memory_cache.clear();
    }

    /// Update globals.
    pub fn set_globals(&mut self, globals: GlobalsSnapshot) {
        self.globals = Some(globals);
    }

    /// Get globals.
    pub fn globals(&self) -> Option<&GlobalsSnapshot> {
        self.globals.as_ref()
    }

    /// Add a watch expression.
    pub fn add_watch(&mut self, expr: impl Into<String>) {
        let expr = expr.into();
        self.watches.insert(expr, String::from("<not evaluated>"));
    }

    /// Update a watch value.
    pub fn update_watch(&mut self, expr: &str, value: impl Into<String>) {
        if let Some(v) = self.watches.get_mut(expr) {
            *v = value.into();
        }
    }

    /// Remove a watch.
    pub fn remove_watch(&mut self, expr: &str) {
        self.watches.remove(expr);
    }

    /// Get all watches.
    pub fn watches(&self) -> &HashMap<String, String> {
        &self.watches
    }

    /// Get all local variables from all frames.
    pub fn all_locals(&self) -> Vec<&Variable> {
        self.call_stack
            .iter()
            .flat_map(|f| f.locals.iter())
            .collect()
    }

    /// Find a variable by name in any scope.
    pub fn find_variable(&self, name: &str) -> Option<&Variable> {
        // Search frames from top to bottom
        for frame in &self.call_stack {
            if let Some(var) = frame
                .locals
                .iter()
                .find(|v| v.name.as_deref() == Some(name))
            {
                return Some(var);
            }
        }

        // Check globals
        if let Some(ref globals) = self.globals {
            if let Some(var) = globals.get_by_name(name) {
                return Some(var);
            }
        }

        None
    }
}

impl std::fmt::Debug for Inspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inspector")
            .field("sandbox_id", &self.sandbox_id)
            .field("stack_depth", &self.call_stack.len())
            .field("memory_cache_entries", &self.memory_cache.len())
            .field("watches", &self.watches.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_frame() {
        let frame = StackFrame::new(0, 0x1000)
            .with_function_name("main")
            .with_source("test.c", 42, 10);

        assert_eq!(frame.index, 0);
        assert_eq!(frame.function_name, Some("main".to_string()));
        assert_eq!(frame.source_location(), Some("test.c:42:10".to_string()));
    }

    #[test]
    fn test_variable_i32() {
        let var = Variable::i32(0, Some("x".to_string()), 42);
        assert_eq!(var.var_type, VariableType::I32);
        assert_eq!(var.as_i32(), Some(42));
        assert_eq!(var.value_string, "42");
    }

    #[test]
    fn test_variable_i64() {
        let var = Variable::i64(1, Some("big".to_string()), 1_000_000_000_000);
        assert_eq!(var.var_type, VariableType::I64);
        assert_eq!(var.as_i64(), Some(1_000_000_000_000));
    }

    #[test]
    fn test_variable_f32() {
        let var = Variable::f32(2, None, 3.14159);
        assert_eq!(var.var_type, VariableType::F32);
        assert!((var.as_f32().unwrap() - 3.14159).abs() < 0.0001);
    }

    #[test]
    fn test_variable_f64() {
        let var = Variable::f64(3, None, std::f64::consts::PI);
        assert_eq!(var.var_type, VariableType::F64);
        assert!((var.as_f64().unwrap() - std::f64::consts::PI).abs() < 0.0000001);
    }

    #[test]
    fn test_memory_view() {
        let data = vec![0x48, 0x65, 0x6c, 0x6c, 0x6f]; // "Hello"
        let view = MemoryView::new(0x1000, data);

        assert_eq!(view.address, 0x1000);
        assert_eq!(view.len(), 5);
        assert_eq!(view.end_address(), 0x1005);
        assert_eq!(view.read_u8(0), Some(0x48));
    }

    #[test]
    fn test_memory_view_read_u32() {
        let data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let view = MemoryView::new(0, data);

        assert_eq!(view.read_u32_le(0), Some(0x04030201));
        assert_eq!(view.read_u32_le(4), Some(0x08070605));
    }

    #[test]
    fn test_memory_view_hex_dump() {
        let data: Vec<u8> = (0..32).collect();
        let view = MemoryView::new(0, data);
        let dump = view.hex_dump();

        assert!(dump.contains("00000000"));
        assert!(dump.contains("00 01 02"));
    }

    #[test]
    fn test_globals_snapshot() {
        let globals = GlobalsSnapshot::new(vec![
            Variable::i32(0, Some("global_count".to_string()), 100),
            Variable::i64(1, Some("global_ptr".to_string()), 0x1000),
        ]);

        assert_eq!(globals.get(0).unwrap().as_i32(), Some(100));
        assert_eq!(
            globals.get_by_name("global_ptr").unwrap().as_i64(),
            Some(0x1000)
        );
    }

    #[test]
    fn test_inspector_new() {
        let sandbox_id = Uuid::new_v4();
        let inspector = Inspector::new(sandbox_id);

        assert_eq!(inspector.sandbox_id(), sandbox_id);
        assert_eq!(inspector.stack_depth(), 0);
    }

    #[test]
    fn test_inspector_call_stack() {
        let sandbox_id = Uuid::new_v4();
        let mut inspector = Inspector::new(sandbox_id);

        let stack = vec![
            StackFrame::new(0, 0x1000).with_function_name("inner"),
            StackFrame::new(1, 0x2000).with_function_name("outer"),
        ];
        inspector.set_call_stack(stack);

        assert_eq!(inspector.stack_depth(), 2);
        assert_eq!(
            inspector.current_frame().unwrap().function_name,
            Some("inner".to_string())
        );
    }

    #[test]
    fn test_inspector_memory_cache() {
        let sandbox_id = Uuid::new_v4();
        let mut inspector = Inspector::new(sandbox_id);

        let view = MemoryView::new(0x1000, vec![1, 2, 3, 4]);
        inspector.cache_memory(view);

        assert!(inspector.get_memory(0x1000).is_some());
        assert!(inspector.get_memory(0x2000).is_none());

        inspector.clear_memory_cache();
        assert!(inspector.get_memory(0x1000).is_none());
    }

    #[test]
    fn test_inspector_watches() {
        let sandbox_id = Uuid::new_v4();
        let mut inspector = Inspector::new(sandbox_id);

        inspector.add_watch("x + y");
        assert_eq!(
            inspector.watches().get("x + y"),
            Some(&"<not evaluated>".to_string())
        );

        inspector.update_watch("x + y", "42");
        assert_eq!(inspector.watches().get("x + y"), Some(&"42".to_string()));

        inspector.remove_watch("x + y");
        assert!(inspector.watches().get("x + y").is_none());
    }

    #[test]
    fn test_inspector_find_variable() {
        let sandbox_id = Uuid::new_v4();
        let mut inspector = Inspector::new(sandbox_id);

        // Add a frame with locals
        let frame = StackFrame::new(0, 0x1000).with_local(Variable::i32(
            0,
            Some("local_var".to_string()),
            10,
        ));
        inspector.set_call_stack(vec![frame]);

        // Add globals
        let globals =
            GlobalsSnapshot::new(vec![Variable::i32(0, Some("global_var".to_string()), 20)]);
        inspector.set_globals(globals);

        // Find local
        let local = inspector.find_variable("local_var").unwrap();
        assert_eq!(local.as_i32(), Some(10));

        // Find global
        let global = inspector.find_variable("global_var").unwrap();
        assert_eq!(global.as_i32(), Some(20));

        // Not found
        assert!(inspector.find_variable("unknown").is_none());
    }

    #[test]
    fn test_variable_type_display() {
        assert_eq!(VariableType::I32.to_string(), "i32");
        assert_eq!(VariableType::I64.to_string(), "i64");
        assert_eq!(VariableType::F32.to_string(), "f32");
        assert_eq!(VariableType::F64.to_string(), "f64");
        assert_eq!(VariableType::V128.to_string(), "v128");
    }
}
