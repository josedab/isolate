//! State snapshots for time-travel debugging.
//!
//! Snapshots capture the complete state of a sandbox at a point in time,
//! enabling efficient backward navigation in the timeline.

use super::EventId;
use crate::error::{Error, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

/// A snapshot of sandbox state at a specific point in execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Event ID at which this snapshot was taken.
    pub event_id: EventId,
    /// Timestamp when the snapshot was taken.
    pub timestamp: DateTime<Utc>,
    /// Instruction pointer value.
    pub instruction_pointer: u64,
    /// Current stack depth.
    pub stack_depth: u32,
    /// Fuel consumed at this point.
    pub fuel_consumed: u64,
    /// Memory state (sparse representation).
    pub memory: MemorySnapshot,
    /// Register/local state.
    pub registers: RegisterSnapshot,
    /// Global variable state.
    pub globals: GlobalSnapshot,
    /// Call stack frames.
    pub call_stack: Vec<StackFrame>,
    /// Size of this snapshot in bytes.
    snapshot_size: usize,
}

impl StateSnapshot {
    /// Create a new state snapshot.
    pub fn new(event_id: EventId) -> Self {
        Self {
            event_id,
            timestamp: Utc::now(),
            instruction_pointer: 0,
            stack_depth: 0,
            fuel_consumed: 0,
            memory: MemorySnapshot::new(),
            registers: RegisterSnapshot::new(),
            globals: GlobalSnapshot::new(),
            call_stack: Vec::new(),
            snapshot_size: 0,
        }
    }

    /// Create a snapshot with memory state.
    pub fn with_memory(mut self, memory: MemorySnapshot) -> Self {
        self.memory = memory;
        self
    }

    /// Create a snapshot with register state.
    pub fn with_registers(mut self, registers: RegisterSnapshot) -> Self {
        self.registers = registers;
        self
    }

    /// Create a snapshot with global state.
    pub fn with_globals(mut self, globals: GlobalSnapshot) -> Self {
        self.globals = globals;
        self
    }

    /// Set the instruction pointer.
    pub fn with_ip(mut self, ip: u64) -> Self {
        self.instruction_pointer = ip;
        self
    }

    /// Set the stack depth.
    pub fn with_stack_depth(mut self, depth: u32) -> Self {
        self.stack_depth = depth;
        self
    }

    /// Set fuel consumed.
    pub fn with_fuel(mut self, fuel: u64) -> Self {
        self.fuel_consumed = fuel;
        self
    }

    /// Add a stack frame.
    pub fn with_stack_frame(mut self, frame: StackFrame) -> Self {
        self.call_stack.push(frame);
        self
    }

    /// Calculate the size of this snapshot.
    pub fn calculate_size(&mut self) -> usize {
        let mut size = std::mem::size_of::<Self>();
        size += self.memory.approximate_size();
        size += self.registers.approximate_size();
        size += self.globals.approximate_size();
        size += self.call_stack.len() * std::mem::size_of::<StackFrame>();
        self.snapshot_size = size;
        size
    }

    /// Get the snapshot size.
    pub fn size(&self) -> usize {
        self.snapshot_size
    }

    /// Compute a diff between this snapshot and another.
    pub fn diff(&self, other: &StateSnapshot) -> SnapshotDiff {
        SnapshotDiff {
            from_event: self.event_id,
            to_event: other.event_id,
            memory_changes: self.memory.diff(&other.memory),
            register_changes: self.registers.diff(&other.registers),
            global_changes: self.globals.diff(&other.globals),
            stack_depth_change: other.stack_depth as i32 - self.stack_depth as i32,
        }
    }
}

/// Sparse memory snapshot using page-based storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshot {
    /// Memory pages (4KB each), keyed by page number.
    pages: HashMap<u32, MemoryPage>,
    /// Total memory size.
    total_size: usize,
}

impl MemorySnapshot {
    /// Create a new empty memory snapshot.
    pub fn new() -> Self {
        Self { pages: HashMap::new(), total_size: 0 }
    }

    /// Page size (4KB).
    pub const PAGE_SIZE: usize = 4096;

    /// Add a memory page.
    pub fn add_page(&mut self, page_num: u32, data: Vec<u8>) {
        self.pages.insert(page_num, MemoryPage { data });
        self.total_size = self.pages.len() * Self::PAGE_SIZE;
    }

    /// Read bytes from the snapshot.
    pub fn read(&self, address: u64, size: usize) -> Option<Vec<u8>> {
        let mut result = Vec::with_capacity(size);
        let mut addr = address;
        let mut remaining = size;

        while remaining > 0 {
            let page_num = (addr / Self::PAGE_SIZE as u64) as u32;
            let page_offset = (addr % Self::PAGE_SIZE as u64) as usize;
            let bytes_in_page = (Self::PAGE_SIZE - page_offset).min(remaining);

            if let Some(page) = self.pages.get(&page_num) {
                let end = (page_offset + bytes_in_page).min(page.data.len());
                result.extend_from_slice(&page.data[page_offset..end]);
            } else {
                // Page not captured, fill with zeros
                result.extend(std::iter::repeat(0).take(bytes_in_page));
            }

            addr += bytes_in_page as u64;
            remaining -= bytes_in_page;
        }

        Some(result)
    }

    /// Get the number of captured pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Get total memory size.
    pub fn total_size(&self) -> usize {
        self.total_size
    }

    /// Approximate size in bytes.
    pub fn approximate_size(&self) -> usize {
        self.pages.len() * (Self::PAGE_SIZE + std::mem::size_of::<u32>())
    }

    /// Compute a diff with another memory snapshot.
    pub fn diff(&self, other: &MemorySnapshot) -> Vec<MemoryChange> {
        let mut changes = Vec::new();

        // Find changed pages
        for (page_num, other_page) in &other.pages {
            match self.pages.get(page_num) {
                Some(self_page) if self_page.data != other_page.data => {
                    changes.push(MemoryChange {
                        address: (*page_num as u64) * Self::PAGE_SIZE as u64,
                        size: Self::PAGE_SIZE,
                        change_type: MemoryChangeType::Modified,
                    });
                }
                None => {
                    changes.push(MemoryChange {
                        address: (*page_num as u64) * Self::PAGE_SIZE as u64,
                        size: Self::PAGE_SIZE,
                        change_type: MemoryChangeType::Added,
                    });
                }
                _ => {}
            }
        }

        // Find removed pages
        for page_num in self.pages.keys() {
            if !other.pages.contains_key(page_num) {
                changes.push(MemoryChange {
                    address: (*page_num as u64) * Self::PAGE_SIZE as u64,
                    size: Self::PAGE_SIZE,
                    change_type: MemoryChangeType::Removed,
                });
            }
        }

        changes
    }
}

impl Default for MemorySnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// A memory page (4KB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPage {
    /// Page data.
    pub data: Vec<u8>,
}

/// Type of memory change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryChangeType {
    /// Memory was added.
    Added,
    /// Memory was modified.
    Modified,
    /// Memory was removed.
    Removed,
}

/// A memory change between snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChange {
    /// Address of the change.
    pub address: u64,
    /// Size of the change.
    pub size: usize,
    /// Type of change.
    pub change_type: MemoryChangeType,
}

/// Register/local variable snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegisterSnapshot {
    /// Named registers and their values.
    values: HashMap<String, u64>,
}

impl RegisterSnapshot {
    /// Create a new register snapshot.
    pub fn new() -> Self {
        Self { values: HashMap::new() }
    }

    /// Set a register value.
    pub fn set(&mut self, name: impl Into<String>, value: u64) {
        self.values.insert(name.into(), value);
    }

    /// Get a register value.
    pub fn get(&self, name: &str) -> Option<u64> {
        self.values.get(name).copied()
    }

    /// Get all register values.
    pub fn all(&self) -> &HashMap<String, u64> {
        &self.values
    }

    /// Approximate size in bytes.
    pub fn approximate_size(&self) -> usize {
        self.values.len() * (32 + std::mem::size_of::<u64>())
    }

    /// Compute a diff with another register snapshot.
    pub fn diff(&self, other: &RegisterSnapshot) -> Vec<RegisterChange> {
        let mut changes = Vec::new();

        for (name, &new_value) in &other.values {
            match self.values.get(name) {
                Some(&old_value) if old_value != new_value => {
                    changes.push(RegisterChange {
                        name: name.clone(),
                        old_value: Some(old_value),
                        new_value,
                    });
                }
                None => {
                    changes.push(RegisterChange { name: name.clone(), old_value: None, new_value });
                }
                _ => {}
            }
        }

        changes
    }
}

/// A register change between snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterChange {
    /// Register name.
    pub name: String,
    /// Old value (None if register was added).
    pub old_value: Option<u64>,
    /// New value.
    pub new_value: u64,
}

/// Global variable snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalSnapshot {
    /// Global indices and their values.
    values: HashMap<u32, GlobalValue>,
}

impl GlobalSnapshot {
    /// Create a new global snapshot.
    pub fn new() -> Self {
        Self { values: HashMap::new() }
    }

    /// Set a global value.
    pub fn set(&mut self, index: u32, value: GlobalValue) {
        self.values.insert(index, value);
    }

    /// Get a global value.
    pub fn get(&self, index: u32) -> Option<&GlobalValue> {
        self.values.get(&index)
    }

    /// Get all globals.
    pub fn all(&self) -> &HashMap<u32, GlobalValue> {
        &self.values
    }

    /// Approximate size in bytes.
    pub fn approximate_size(&self) -> usize {
        self.values.len() * (std::mem::size_of::<u32>() + std::mem::size_of::<GlobalValue>())
    }

    /// Compute a diff with another global snapshot.
    pub fn diff(&self, other: &GlobalSnapshot) -> Vec<GlobalChange> {
        let mut changes = Vec::new();

        for (&index, new_value) in &other.values {
            match self.values.get(&index) {
                Some(old_value) if old_value != new_value => {
                    changes.push(GlobalChange {
                        index,
                        old_value: Some(old_value.clone()),
                        new_value: new_value.clone(),
                    });
                }
                None => {
                    changes.push(GlobalChange {
                        index,
                        old_value: None,
                        new_value: new_value.clone(),
                    });
                }
                _ => {}
            }
        }

        changes
    }
}

/// A global value (supports different WASM types).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GlobalValue {
    /// 32-bit integer.
    I32(i32),
    /// 64-bit integer.
    I64(i64),
    /// 32-bit float.
    F32(f32),
    /// 64-bit float.
    F64(f64),
    /// V128 value.
    V128([u8; 16]),
    /// Reference (externref or funcref).
    Ref(Option<u32>),
}

/// A global change between snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalChange {
    /// Global index.
    pub index: u32,
    /// Old value (None if global was added).
    pub old_value: Option<GlobalValue>,
    /// New value.
    pub new_value: GlobalValue,
}

/// A call stack frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// Function index.
    pub function_index: u32,
    /// Function name (if available).
    pub function_name: Option<String>,
    /// Return address.
    pub return_address: u64,
    /// Local variables.
    pub locals: Vec<u64>,
    /// Operand stack.
    pub operand_stack: Vec<u64>,
}

impl StackFrame {
    /// Create a new stack frame.
    pub fn new(function_index: u32, return_address: u64) -> Self {
        Self {
            function_index,
            function_name: None,
            return_address,
            locals: Vec::new(),
            operand_stack: Vec::new(),
        }
    }

    /// Set the function name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.function_name = Some(name.into());
        self
    }

    /// Set locals.
    pub fn with_locals(mut self, locals: Vec<u64>) -> Self {
        self.locals = locals;
        self
    }

    /// Set operand stack.
    pub fn with_operand_stack(mut self, stack: Vec<u64>) -> Self {
        self.operand_stack = stack;
        self
    }
}

/// Diff between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDiff {
    /// Source event ID.
    pub from_event: EventId,
    /// Target event ID.
    pub to_event: EventId,
    /// Memory changes.
    pub memory_changes: Vec<MemoryChange>,
    /// Register changes.
    pub register_changes: Vec<RegisterChange>,
    /// Global changes.
    pub global_changes: Vec<GlobalChange>,
    /// Stack depth change.
    pub stack_depth_change: i32,
}

/// Manages snapshots for efficient state restoration.
pub struct SnapshotManager {
    /// Stored snapshots, keyed by event ID.
    snapshots: Arc<RwLock<BTreeMap<EventId, StateSnapshot>>>,
    /// Snapshot interval (take snapshot every N events).
    interval: u64,
    /// Maximum number of snapshots to keep.
    max_snapshots: usize,
    /// Maximum total size of snapshots in bytes.
    max_size: usize,
    /// Current total size.
    current_size: Arc<RwLock<usize>>,
}

impl SnapshotManager {
    /// Create a new snapshot manager.
    pub fn new(interval: u64, max_snapshots: usize) -> Self {
        Self {
            snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            interval,
            max_snapshots,
            max_size: 512 * 1024 * 1024, // 512MB default
            current_size: Arc::new(RwLock::new(0)),
        }
    }

    /// Create with custom max size.
    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_size = max_size;
        self
    }

    /// Get the snapshot interval.
    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// Check if a snapshot should be taken at this event ID.
    pub fn should_snapshot(&self, event_id: EventId) -> bool {
        event_id % self.interval == 0
    }

    /// Store a snapshot.
    pub fn store(&self, mut snapshot: StateSnapshot) -> Result<()> {
        let size = snapshot.calculate_size();

        // Check if we need to evict old snapshots
        self.evict_if_needed(size)?;

        let mut snapshots = self
            .snapshots
            .write()
            .map_err(|_| Error::Engine("Failed to lock snapshots".to_string()))?;

        // Check max snapshot count
        if snapshots.len() >= self.max_snapshots {
            // Remove oldest snapshot
            if let Some(&oldest_id) = snapshots.keys().next() {
                if let Some(old_snapshot) = snapshots.remove(&oldest_id) {
                    let mut current_size = self
                        .current_size
                        .write()
                        .map_err(|_| Error::Engine("Failed to lock size".to_string()))?;
                    *current_size = current_size.saturating_sub(old_snapshot.size());
                }
            }
        }

        let event_id = snapshot.event_id;
        snapshots.insert(event_id, snapshot);

        let mut current_size = self
            .current_size
            .write()
            .map_err(|_| Error::Engine("Failed to lock size".to_string()))?;
        *current_size += size;

        Ok(())
    }

    /// Get a snapshot by event ID.
    pub fn get(&self, event_id: EventId) -> Option<StateSnapshot> {
        let snapshots = self.snapshots.read().ok()?;
        snapshots.get(&event_id).cloned()
    }

    /// Get the nearest snapshot at or before the given event ID.
    pub fn get_nearest(&self, event_id: EventId) -> Option<StateSnapshot> {
        let snapshots = self.snapshots.read().ok()?;
        snapshots.range(..=event_id).next_back().map(|(_, s)| s.clone())
    }

    /// Get all snapshot event IDs.
    pub fn snapshot_ids(&self) -> Vec<EventId> {
        let snapshots = self.snapshots.read().ok();
        snapshots.map(|s| s.keys().copied().collect()).unwrap_or_default()
    }

    /// Get the number of stored snapshots.
    pub fn count(&self) -> usize {
        self.snapshots.read().map(|s| s.len()).unwrap_or(0)
    }

    /// Get current total size.
    pub fn total_size(&self) -> usize {
        self.current_size.read().map(|guard| *guard).unwrap_or(0)
    }

    /// Clear all snapshots.
    pub fn clear(&self) -> Result<()> {
        let mut snapshots = self
            .snapshots
            .write()
            .map_err(|_| Error::Engine("Failed to lock snapshots".to_string()))?;
        snapshots.clear();

        let mut current_size = self
            .current_size
            .write()
            .map_err(|_| Error::Engine("Failed to lock size".to_string()))?;
        *current_size = 0;

        Ok(())
    }

    /// Evict old snapshots if needed to make room.
    fn evict_if_needed(&self, needed_size: usize) -> Result<()> {
        let current = self
            .current_size
            .read()
            .map_err(|_| Error::Engine("Failed to lock size".to_string()))?;

        if *current + needed_size <= self.max_size {
            return Ok(());
        }
        drop(current);

        // Need to evict snapshots
        let mut snapshots = self
            .snapshots
            .write()
            .map_err(|_| Error::Engine("Failed to lock snapshots".to_string()))?;

        let mut freed = 0usize;
        let target_free = needed_size;

        while freed < target_free && !snapshots.is_empty() {
            if let Some(&oldest_id) = snapshots.keys().next() {
                if let Some(old_snapshot) = snapshots.remove(&oldest_id) {
                    freed += old_snapshot.size();
                }
            }
        }

        let mut current_size = self
            .current_size
            .write()
            .map_err(|_| Error::Engine("Failed to lock size".to_string()))?;
        *current_size = current_size.saturating_sub(freed);

        Ok(())
    }
}

impl Default for SnapshotManager {
    fn default() -> Self {
        Self::new(10_000, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_snapshot() {
        let snapshot = StateSnapshot::new(100).with_ip(0x1000).with_stack_depth(2).with_fuel(5000);

        assert_eq!(snapshot.event_id, 100);
        assert_eq!(snapshot.instruction_pointer, 0x1000);
        assert_eq!(snapshot.stack_depth, 2);
        assert_eq!(snapshot.fuel_consumed, 5000);
    }

    #[test]
    fn test_memory_snapshot() {
        let mut memory = MemorySnapshot::new();
        memory.add_page(0, vec![0; 4096]);
        memory.add_page(1, vec![42; 4096]);

        assert_eq!(memory.page_count(), 2);

        // Read from page 0
        let data = memory.read(0, 4).unwrap();
        assert_eq!(data, vec![0, 0, 0, 0]);

        // Read from page 1
        let data = memory.read(4096, 4).unwrap();
        assert_eq!(data, vec![42, 42, 42, 42]);

        // Read across page boundary
        let data = memory.read(4094, 4).unwrap();
        assert_eq!(data, vec![0, 0, 42, 42]);
    }

    #[test]
    fn test_register_snapshot() {
        let mut regs = RegisterSnapshot::new();
        regs.set("rax", 42);
        regs.set("rbx", 100);

        assert_eq!(regs.get("rax"), Some(42));
        assert_eq!(regs.get("rbx"), Some(100));
        assert_eq!(regs.get("rcx"), None);
    }

    #[test]
    fn test_register_diff() {
        let mut regs1 = RegisterSnapshot::new();
        regs1.set("rax", 42);
        regs1.set("rbx", 100);

        let mut regs2 = RegisterSnapshot::new();
        regs2.set("rax", 42); // Same
        regs2.set("rbx", 200); // Changed
        regs2.set("rcx", 50); // New

        let diff = regs1.diff(&regs2);
        assert_eq!(diff.len(), 2); // rbx changed, rcx added
    }

    #[test]
    fn test_global_snapshot() {
        let mut globals = GlobalSnapshot::new();
        globals.set(0, GlobalValue::I32(42));
        globals.set(1, GlobalValue::I64(1000));
        globals.set(2, GlobalValue::F32(2.5));

        assert_eq!(globals.get(0), Some(&GlobalValue::I32(42)));
        assert_eq!(globals.get(1), Some(&GlobalValue::I64(1000)));
    }

    #[test]
    fn test_stack_frame() {
        let frame = StackFrame::new(5, 0x1000).with_name("my_function").with_locals(vec![1, 2, 3]);

        assert_eq!(frame.function_index, 5);
        assert_eq!(frame.function_name, Some("my_function".to_string()));
        assert_eq!(frame.return_address, 0x1000);
        assert_eq!(frame.locals, vec![1, 2, 3]);
    }

    #[test]
    fn test_snapshot_diff() {
        let mut regs1 = RegisterSnapshot::new();
        regs1.set("rax", 0);

        let mut regs2 = RegisterSnapshot::new();
        regs2.set("rax", 42);

        let snap1 = StateSnapshot::new(0).with_stack_depth(1).with_registers(regs1);

        let snap2 = StateSnapshot::new(100).with_stack_depth(3).with_registers(regs2);

        let diff = snap1.diff(&snap2);
        assert_eq!(diff.from_event, 0);
        assert_eq!(diff.to_event, 100);
        assert_eq!(diff.stack_depth_change, 2);
        assert_eq!(diff.register_changes.len(), 1);
    }

    #[test]
    fn test_snapshot_manager() {
        let manager = SnapshotManager::new(10, 5);

        assert!(manager.should_snapshot(0));
        assert!(!manager.should_snapshot(5));
        assert!(manager.should_snapshot(10));
        assert!(manager.should_snapshot(20));
    }

    #[test]
    fn test_snapshot_manager_store_retrieve() {
        let manager = SnapshotManager::new(10, 5);

        let snap1 = StateSnapshot::new(0).with_ip(0x1000);
        let snap2 = StateSnapshot::new(10).with_ip(0x2000);
        let snap3 = StateSnapshot::new(20).with_ip(0x3000);

        manager.store(snap1).unwrap();
        manager.store(snap2).unwrap();
        manager.store(snap3).unwrap();

        assert_eq!(manager.count(), 3);

        let retrieved = manager.get(10).unwrap();
        assert_eq!(retrieved.instruction_pointer, 0x2000);

        let nearest = manager.get_nearest(15).unwrap();
        assert_eq!(nearest.event_id, 10);
    }

    #[test]
    fn test_snapshot_manager_max_count() {
        let manager = SnapshotManager::new(1, 3);

        for i in 0..5 {
            let snap = StateSnapshot::new(i);
            manager.store(snap).unwrap();
        }

        // Should only keep 3 most recent
        assert_eq!(manager.count(), 3);

        // Oldest (0, 1) should be evicted
        assert!(manager.get(0).is_none());
        assert!(manager.get(1).is_none());
        assert!(manager.get(2).is_some());
        assert!(manager.get(3).is_some());
        assert!(manager.get(4).is_some());
    }
}
