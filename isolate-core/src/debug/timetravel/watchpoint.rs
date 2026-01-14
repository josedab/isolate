//! Data watchpoints for time-travel debugging.
//!
//! Watch memory locations and expressions for changes during execution replay.

use super::{EventType, ExecutionEvent};
use serde::{Deserialize, Serialize};

/// A watchpoint that triggers when a condition is met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watchpoint {
    /// Unique watchpoint ID.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What to watch.
    pub watch_type: WatchType,
    /// Condition for triggering.
    pub condition: WatchCondition,
    /// Whether the watchpoint is active.
    pub enabled: bool,
    /// Number of times this watchpoint has triggered.
    pub hit_count: u64,
}

/// What kind of data to watch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchType {
    /// Watch a specific memory address.
    MemoryAddress { offset: u64, size: usize },
    /// Watch a range of memory addresses.
    MemoryRange { start: u64, end: u64 },
    /// Watch a global variable by index.
    GlobalVariable { index: u32 },
    /// Watch for entry into a named function.
    FunctionEntry { name: String },
    /// Watch for exit from a named function.
    FunctionExit { name: String },
}

/// Condition under which a watchpoint triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchCondition {
    /// Trigger on any change.
    AnyChange,
    /// Trigger when value equals specific bytes.
    ValueEquals(Vec<u8>),
    /// Trigger when value changes between specific values.
    ValueChanged { from: Option<Vec<u8>>, to: Option<Vec<u8>> },
    /// Trigger when access count reaches a threshold.
    AccessCount { threshold: u64 },
}

/// Record of a watchpoint being triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchpointHit {
    /// ID of the watchpoint that triggered.
    pub watchpoint_id: String,
    /// Position in the timeline.
    pub position: usize,
    /// Value before the change.
    pub old_value: Option<Vec<u8>>,
    /// Value after the change.
    pub new_value: Option<Vec<u8>>,
    /// The event that triggered the watchpoint.
    pub event: ExecutionEvent,
}

/// Manages watchpoints and evaluates them against events.
pub struct WatchpointManager {
    watchpoints: Vec<Watchpoint>,
    hits: Vec<WatchpointHit>,
    next_id: u64,
}

impl WatchpointManager {
    /// Create a new watchpoint manager.
    pub fn new() -> Self {
        Self { watchpoints: Vec::new(), hits: Vec::new(), next_id: 1 }
    }

    /// Add a new watchpoint. Returns the watchpoint ID.
    pub fn add(
        &mut self,
        name: String,
        watch_type: WatchType,
        condition: WatchCondition,
    ) -> String {
        let id = format!("wp-{}", self.next_id);
        self.next_id += 1;

        self.watchpoints.push(Watchpoint {
            id: id.clone(),
            name,
            watch_type,
            condition,
            enabled: true,
            hit_count: 0,
        });

        id
    }

    /// Remove a watchpoint by ID.
    pub fn remove(&mut self, id: &str) -> bool {
        let len = self.watchpoints.len();
        self.watchpoints.retain(|w| w.id != id);
        self.watchpoints.len() < len
    }

    /// Enable a watchpoint by ID.
    pub fn enable(&mut self, id: &str) -> bool {
        if let Some(wp) = self.watchpoints.iter_mut().find(|w| w.id == id) {
            wp.enabled = true;
            true
        } else {
            false
        }
    }

    /// Disable a watchpoint by ID.
    pub fn disable(&mut self, id: &str) -> bool {
        if let Some(wp) = self.watchpoints.iter_mut().find(|w| w.id == id) {
            wp.enabled = false;
            true
        } else {
            false
        }
    }

    /// Evaluate all enabled watchpoints against an event. Returns any hits.
    pub fn evaluate(&mut self, event: &ExecutionEvent, position: usize) -> Vec<WatchpointHit> {
        let mut new_hits = Vec::new();

        for wp in &mut self.watchpoints {
            if !wp.enabled {
                continue;
            }

            if let Some(hit) = Self::check_watchpoint(wp, event, position) {
                new_hits.push(hit);
            }
        }

        self.hits.extend(new_hits.clone());
        new_hits
    }

    /// Get all hits for a specific watchpoint.
    pub fn hits_for_watchpoint(&self, id: &str) -> Vec<&WatchpointHit> {
        self.hits.iter().filter(|h| h.watchpoint_id == id).collect()
    }

    /// Get all recorded hits.
    pub fn all_hits(&self) -> &[WatchpointHit] {
        &self.hits
    }

    /// Clear all recorded hits.
    pub fn clear_hits(&mut self) {
        self.hits.clear();
        for wp in &mut self.watchpoints {
            wp.hit_count = 0;
        }
    }

    /// List all watchpoints.
    pub fn list_watchpoints(&self) -> &[Watchpoint] {
        &self.watchpoints
    }

    /// Get a watchpoint by ID.
    pub fn get(&self, id: &str) -> Option<&Watchpoint> {
        self.watchpoints.iter().find(|w| w.id == id)
    }

    fn check_watchpoint(
        wp: &mut Watchpoint,
        event: &ExecutionEvent,
        position: usize,
    ) -> Option<WatchpointHit> {
        match &wp.watch_type {
            WatchType::MemoryAddress { offset, size } => {
                let offset = *offset;
                let size = *size;
                for mc in &event.memory_changes {
                    let mc_end = mc.address + mc.new_value.len() as u64;
                    let wp_end = offset + size as u64;

                    if mc.address < wp_end && mc_end > offset {
                        wp.hit_count += 1;
                        if Self::check_condition(
                            &wp.condition,
                            &mc.old_value,
                            &mc.new_value,
                            wp.hit_count,
                        ) {
                            return Some(WatchpointHit {
                                watchpoint_id: wp.id.clone(),
                                position,
                                old_value: Some(mc.old_value.clone()),
                                new_value: Some(mc.new_value.clone()),
                                event: event.clone(),
                            });
                        }
                    }
                }
                None
            }
            WatchType::MemoryRange { start, end } => {
                let start = *start;
                let end = *end;
                for mc in &event.memory_changes {
                    let mc_end = mc.address + mc.new_value.len() as u64;
                    if mc.address < end && mc_end > start {
                        wp.hit_count += 1;
                        if Self::check_condition(
                            &wp.condition,
                            &mc.old_value,
                            &mc.new_value,
                            wp.hit_count,
                        ) {
                            return Some(WatchpointHit {
                                watchpoint_id: wp.id.clone(),
                                position,
                                old_value: Some(mc.old_value.clone()),
                                new_value: Some(mc.new_value.clone()),
                                event: event.clone(),
                            });
                        }
                    }
                }
                None
            }
            WatchType::GlobalVariable { index } => {
                let index = *index;
                for rc in &event.register_changes {
                    if rc.name == format!("global_{}", index) {
                        wp.hit_count += 1;
                        let old_bytes = rc.old_value.to_le_bytes().to_vec();
                        let new_bytes = rc.new_value.to_le_bytes().to_vec();
                        if Self::check_condition(
                            &wp.condition,
                            &old_bytes,
                            &new_bytes,
                            wp.hit_count,
                        ) {
                            return Some(WatchpointHit {
                                watchpoint_id: wp.id.clone(),
                                position,
                                old_value: Some(old_bytes),
                                new_value: Some(new_bytes),
                                event: event.clone(),
                            });
                        }
                    }
                }
                None
            }
            WatchType::FunctionEntry { name } => {
                let name = name.clone();
                if event.event_type == EventType::FunctionCall
                    && event.function_name.as_ref() == Some(&name)
                {
                    wp.hit_count += 1;
                    if Self::check_condition(&wp.condition, &[], &[], wp.hit_count) {
                        return Some(WatchpointHit {
                            watchpoint_id: wp.id.clone(),
                            position,
                            old_value: None,
                            new_value: None,
                            event: event.clone(),
                        });
                    }
                }
                None
            }
            WatchType::FunctionExit { name } => {
                let name = name.clone();
                if event.event_type == EventType::FunctionReturn
                    && event.function_name.as_ref() == Some(&name)
                {
                    wp.hit_count += 1;
                    if Self::check_condition(&wp.condition, &[], &[], wp.hit_count) {
                        return Some(WatchpointHit {
                            watchpoint_id: wp.id.clone(),
                            position,
                            old_value: None,
                            new_value: None,
                            event: event.clone(),
                        });
                    }
                }
                None
            }
        }
    }

    fn check_condition(
        condition: &WatchCondition,
        old_value: &[u8],
        new_value: &[u8],
        current_count: u64,
    ) -> bool {
        match condition {
            WatchCondition::AnyChange => {
                if old_value.is_empty() && new_value.is_empty() {
                    return true;
                }
                old_value != new_value
            }
            WatchCondition::ValueEquals(expected) => new_value == expected.as_slice(),
            WatchCondition::ValueChanged { from, to } => {
                let from_ok = from.as_ref().map_or(true, |f| old_value == f.as_slice());
                let to_ok = to.as_ref().map_or(true, |t| new_value == t.as_slice());
                from_ok && to_ok
            }
            WatchCondition::AccessCount { threshold } => current_count >= *threshold,
        }
    }
}

impl Default for WatchpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug::timetravel::event::MemoryChange;

    fn make_memory_event(id: u64, addr: u64, old: Vec<u8>, new: Vec<u8>) -> ExecutionEvent {
        ExecutionEvent::new(id, EventType::MemoryWrite, 0x1000)
            .with_memory_change(MemoryChange::new(addr, old, new))
    }

    #[test]
    fn test_add_and_list_watchpoints() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "test".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );
        assert_eq!(mgr.list_watchpoints().len(), 1);
        assert!(mgr.get(&id).is_some());
        assert_eq!(mgr.get(&id).unwrap().name, "test");
    }

    #[test]
    fn test_remove_watchpoint() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "wp1".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );
        assert!(mgr.remove(&id));
        assert_eq!(mgr.list_watchpoints().len(), 0);
        assert!(!mgr.remove("nonexistent"));
    }

    #[test]
    fn test_enable_disable() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "wp1".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );

        assert!(mgr.disable(&id));
        assert!(!mgr.get(&id).unwrap().enabled);

        assert!(mgr.enable(&id));
        assert!(mgr.get(&id).unwrap().enabled);

        assert!(!mgr.enable("nonexistent"));
        assert!(!mgr.disable("nonexistent"));
    }

    #[test]
    fn test_evaluate_memory_address_any_change() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "mem_watch".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );

        // Event with change at watched address
        let event = make_memory_event(1, 0x100, vec![0x00], vec![0x42]);
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].new_value, Some(vec![0x42]));

        // Event with no change (old == new)
        let event = make_memory_event(2, 0x100, vec![0x42], vec![0x42]);
        let hits = mgr.evaluate(&event, 1);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_evaluate_memory_range() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "range_watch".to_string(),
            WatchType::MemoryRange { start: 0x100, end: 0x200 },
            WatchCondition::AnyChange,
        );

        // Event inside range
        let event = make_memory_event(1, 0x150, vec![0x00], vec![0xFF]);
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);

        // Event outside range
        let event = make_memory_event(2, 0x300, vec![0x00], vec![0xFF]);
        let hits = mgr.evaluate(&event, 1);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_evaluate_value_equals() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "val_watch".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 1 },
            WatchCondition::ValueEquals(vec![0x42]),
        );

        // Value matches
        let event = make_memory_event(1, 0x100, vec![0x00], vec![0x42]);
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);

        // Value doesn't match
        let event = make_memory_event(2, 0x100, vec![0x00], vec![0x99]);
        let hits = mgr.evaluate(&event, 1);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_evaluate_function_entry() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "fn_watch".to_string(),
            WatchType::FunctionEntry { name: "my_func".to_string() },
            WatchCondition::AnyChange,
        );

        let event =
            ExecutionEvent::new(1, EventType::FunctionCall, 0x2000).with_function("my_func");
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);

        // Different function
        let event =
            ExecutionEvent::new(2, EventType::FunctionCall, 0x3000).with_function("other_func");
        let hits = mgr.evaluate(&event, 1);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_evaluate_function_exit() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "fn_exit_watch".to_string(),
            WatchType::FunctionExit { name: "my_func".to_string() },
            WatchCondition::AnyChange,
        );

        let event =
            ExecutionEvent::new(1, EventType::FunctionReturn, 0x2000).with_function("my_func");
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn test_disabled_watchpoint_no_hits() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "disabled".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );
        mgr.disable(&id);

        let event = make_memory_event(1, 0x100, vec![0x00], vec![0x42]);
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_hit_count_tracking() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "counter".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );

        for i in 0..3 {
            let event = make_memory_event(i, 0x100, vec![i as u8], vec![(i + 1) as u8]);
            mgr.evaluate(&event, i as usize);
        }

        assert_eq!(mgr.get(&id).unwrap().hit_count, 3);
        assert_eq!(mgr.all_hits().len(), 3);
        assert_eq!(mgr.hits_for_watchpoint(&id).len(), 3);
    }

    #[test]
    fn test_clear_hits() {
        let mut mgr = WatchpointManager::new();
        let id = mgr.add(
            "clearable".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 4 },
            WatchCondition::AnyChange,
        );

        let event = make_memory_event(1, 0x100, vec![0x00], vec![0x42]);
        mgr.evaluate(&event, 0);
        assert_eq!(mgr.all_hits().len(), 1);

        mgr.clear_hits();
        assert_eq!(mgr.all_hits().len(), 0);
        assert_eq!(mgr.get(&id).unwrap().hit_count, 0);
    }

    #[test]
    fn test_value_changed_condition() {
        let mut mgr = WatchpointManager::new();
        mgr.add(
            "changed".to_string(),
            WatchType::MemoryAddress { offset: 0x100, size: 1 },
            WatchCondition::ValueChanged { from: Some(vec![0x00]), to: Some(vec![0x42]) },
        );

        // Exact match
        let event = make_memory_event(1, 0x100, vec![0x00], vec![0x42]);
        let hits = mgr.evaluate(&event, 0);
        assert_eq!(hits.len(), 1);

        // Wrong from value
        let event = make_memory_event(2, 0x100, vec![0x01], vec![0x42]);
        let hits = mgr.evaluate(&event, 1);
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn test_access_count_condition() {
        let mut mgr = WatchpointManager::new();
        let _id = mgr.add(
            "counted".to_string(),
            WatchType::FunctionEntry { name: "target".to_string() },
            WatchCondition::AccessCount { threshold: 3 },
        );

        let event = ExecutionEvent::new(1, EventType::FunctionCall, 0x1000).with_function("target");

        // First two calls shouldn't trigger (count < threshold)
        let hits = mgr.evaluate(&event.clone(), 0);
        assert_eq!(hits.len(), 0);
        let hits = mgr.evaluate(&event.clone(), 1);
        assert_eq!(hits.len(), 0);

        // Third call should trigger (count == threshold)
        let hits = mgr.evaluate(&event, 2);
        assert_eq!(hits.len(), 1);
    }
}
