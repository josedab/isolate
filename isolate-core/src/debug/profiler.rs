//! Profiling capabilities for sandbox execution.
//!
//! This module provides profiling support for measuring and analyzing
//! sandbox performance, including function-level timing, memory allocation
//! tracking, fuel consumption, and WASI call overhead.
//!
//! # Features
//!
//! - **Function Profiling**: Track entry/exit times and aggregate per-function stats
//! - **Memory Tracking**: Monitor allocations and frees
//! - **Fuel Metering**: Record fuel consumption over time
//! - **WASI Call Profiling**: Measure overhead of WASI host calls
//! - **Sampling**: Configurable sampling rate to reduce overhead
//!
//! # Example
//!
//! ```rust,ignore
//! use isolate_core::debug::profiler::{SandboxProfiler, ProfileEvent};
//! use std::time::{Duration, Instant};
//!
//! let profiler = SandboxProfiler::new();
//! let session_id = profiler.start_session("sandbox-1");
//!
//! // Record events during execution
//! profiler.record(&session_id, ProfileEvent::FunctionEntry {
//!     name: "main".to_string(),
//!     timestamp: Instant::now(),
//! });
//!
//! // End session and get aggregated profile
//! let profile = profiler.end_session(&session_id).unwrap();
//! for func in profile.hottest_functions(5) {
//!     println!("{}: {} calls, avg {:?}", func.name, func.call_count, func.avg_duration());
//! }
//! ```

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Serde helper module to skip `Instant` fields during serialization
/// and provide `Instant::now()` as the default during deserialization.
mod serde_instant {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Instant;

    pub fn serialize<S>(_instant: &Instant, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as a unit / null; Instant has no meaningful portable representation.
        serializer.serialize_none()
    }

    pub fn deserialize<'de, D>(_deserializer: D) -> Result<Instant, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Consume whatever token is present and return Instant::now().
        let _ = Option::<()>::deserialize(_deserializer)?;
        Ok(Instant::now())
    }
}

/// An event captured during profiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProfileEvent {
    /// A function was entered.
    FunctionEntry {
        /// Name of the function being entered.
        name: String,
        /// Timestamp when the function was entered.
        #[serde(with = "serde_instant")]
        timestamp: Instant,
    },

    /// A function exited.
    FunctionExit {
        /// Name of the function that exited.
        name: String,
        /// Timestamp when the function exited.
        #[serde(with = "serde_instant")]
        timestamp: Instant,
        /// Duration spent in the function.
        duration: Duration,
    },

    /// A memory allocation occurred.
    MemoryAllocation {
        /// Number of bytes allocated.
        bytes: u64,
        /// Address of the allocation.
        address: u64,
    },

    /// A memory free occurred.
    MemoryFree {
        /// Number of bytes freed.
        bytes: u64,
        /// Address that was freed.
        address: u64,
    },

    /// Fuel was consumed by execution.
    FuelConsumed {
        /// Amount of fuel consumed.
        amount: u64,
        /// Remaining fuel after consumption.
        remaining: u64,
    },

    /// A WASI host call was made.
    WasiCall {
        /// Name of the WASI call.
        name: String,
        /// Duration of the WASI call.
        duration: Duration,
    },
}

/// A profiling session that collects events for a single sandbox execution.
pub struct ProfileSession {
    /// Unique session identifier.
    id: String,
    /// Identifier of the sandbox being profiled.
    sandbox_id: String,
    /// When the session started.
    started_at: Instant,
    /// Collected profile events.
    events: Mutex<Vec<ProfileEvent>>,
}

impl ProfileSession {
    /// Create a new profiling session for the given sandbox.
    pub fn new(sandbox_id: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            sandbox_id: sandbox_id.into(),
            started_at: Instant::now(),
            events: Mutex::new(Vec::new()),
        }
    }

    /// Get the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the sandbox ID associated with this session.
    pub fn sandbox_id(&self) -> &str {
        &self.sandbox_id
    }

    /// Get the instant when this session was started.
    pub fn started_at(&self) -> Instant {
        self.started_at
    }

    /// Record a profile event into this session.
    pub fn record(&self, event: ProfileEvent) {
        self.events.lock().push(event);
    }

    /// Get the elapsed time since the session started.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Get the number of events recorded so far.
    pub fn event_count(&self) -> usize {
        self.events.lock().len()
    }

    /// Take all recorded events, draining the internal buffer.
    fn take_events(&self) -> Vec<ProfileEvent> {
        std::mem::take(&mut *self.events.lock())
    }
}

impl std::fmt::Debug for ProfileSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileSession")
            .field("id", &self.id)
            .field("sandbox_id", &self.sandbox_id)
            .field("elapsed", &self.elapsed())
            .field("event_count", &self.event_count())
            .finish()
    }
}

/// Aggregated profiling statistics for a single function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionProfile {
    /// Name of the function.
    pub name: String,
    /// Total number of times this function was called.
    pub call_count: u64,
    /// Total time spent in this function across all calls.
    pub total_duration: Duration,
    /// Minimum duration of a single call.
    pub min_duration: Duration,
    /// Maximum duration of a single call.
    pub max_duration: Duration,
}

impl FunctionProfile {
    /// Create a new function profile with the first observed duration.
    fn new(name: String, duration: Duration) -> Self {
        Self {
            name,
            call_count: 1,
            total_duration: duration,
            min_duration: duration,
            max_duration: duration,
        }
    }

    /// Compute the average duration per call.
    ///
    /// Returns `Duration::ZERO` if `call_count` is zero.
    pub fn avg_duration(&self) -> Duration {
        if self.call_count == 0 {
            return Duration::ZERO;
        }
        self.total_duration / self.call_count as u32
    }

    /// Record an additional call with the given duration.
    fn record(&mut self, duration: Duration) {
        self.call_count += 1;
        self.total_duration += duration;
        if duration < self.min_duration {
            self.min_duration = duration;
        }
        if duration > self.max_duration {
            self.max_duration = duration;
        }
    }
}

/// A complete execution profile aggregated from a profiling session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionProfile {
    /// Total wall-clock duration of the profiled execution.
    pub total_duration: Duration,
    /// Per-function profiling statistics.
    pub functions: HashMap<String, FunctionProfile>,
    /// Peak memory usage observed during execution (in bytes).
    pub peak_memory: u64,
    /// Total fuel consumed during execution.
    pub total_fuel_consumed: u64,
    /// Per-WASI-call profiling statistics.
    pub wasi_calls: HashMap<String, FunctionProfile>,
}

impl ExecutionProfile {
    /// Return the top N hottest functions, sorted by total duration descending.
    pub fn hottest_functions(&self, n: usize) -> Vec<&FunctionProfile> {
        let mut profiles: Vec<&FunctionProfile> = self.functions.values().collect();
        profiles.sort_by(|a, b| b.total_duration.cmp(&a.total_duration));
        profiles.truncate(n);
        profiles
    }

    /// Build an `ExecutionProfile` from a completed `ProfileSession`.
    ///
    /// This consumes all events from the session and aggregates them into
    /// per-function and per-WASI-call statistics.
    pub fn from_session(session: &ProfileSession) -> Self {
        let events = session.take_events();
        let total_duration = session.elapsed();

        let mut functions: HashMap<String, FunctionProfile> = HashMap::new();
        let mut wasi_calls: HashMap<String, FunctionProfile> = HashMap::new();
        let mut peak_memory: u64 = 0;
        let mut current_memory: u64 = 0;
        let mut total_fuel_consumed: u64 = 0;

        for event in &events {
            match event {
                ProfileEvent::FunctionExit { name, duration, .. } => {
                    functions
                        .entry(name.clone())
                        .and_modify(|fp| fp.record(*duration))
                        .or_insert_with(|| FunctionProfile::new(name.clone(), *duration));
                }
                ProfileEvent::MemoryAllocation { bytes, .. } => {
                    current_memory += bytes;
                    if current_memory > peak_memory {
                        peak_memory = current_memory;
                    }
                }
                ProfileEvent::MemoryFree { bytes, .. } => {
                    current_memory = current_memory.saturating_sub(*bytes);
                }
                ProfileEvent::FuelConsumed { amount, .. } => {
                    total_fuel_consumed += amount;
                }
                ProfileEvent::WasiCall { name, duration } => {
                    wasi_calls
                        .entry(name.clone())
                        .and_modify(|fp| fp.record(*duration))
                        .or_insert_with(|| FunctionProfile::new(name.clone(), *duration));
                }
                ProfileEvent::FunctionEntry { .. } => {
                    // Function entries are paired with exits; no aggregation needed here.
                }
            }
        }

        Self { total_duration, functions, peak_memory, total_fuel_consumed, wasi_calls }
    }
}

/// The main profiler that manages profiling sessions for sandboxes.
///
/// `SandboxProfiler` can be shared across threads via `Arc` and supports
/// concurrent session management through internal locking.
pub struct SandboxProfiler {
    /// Whether profiling is enabled.
    enabled: bool,
    /// Active profiling sessions, keyed by session ID.
    sessions: RwLock<HashMap<String, Arc<ProfileSession>>>,
    /// Sampling rate from 0.0 (no sampling) to 1.0 (sample every event).
    sampling_rate: f64,
}

impl SandboxProfiler {
    /// Create a new profiler with profiling enabled and a sampling rate of 1.0.
    pub fn new() -> Self {
        Self { enabled: true, sessions: RwLock::new(HashMap::new()), sampling_rate: 1.0 }
    }

    /// Set the sampling rate for the profiler.
    ///
    /// The rate is clamped to the range `[0.0, 1.0]`. A rate of `1.0` means
    /// every event is recorded; `0.0` means no events are recorded.
    pub fn with_sampling_rate(mut self, rate: f64) -> Self {
        self.sampling_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Start a new profiling session for the given sandbox.
    ///
    /// Returns the session ID that can be used to record events and
    /// later end the session.
    pub fn start_session(&self, sandbox_id: impl Into<String>) -> String {
        let session = Arc::new(ProfileSession::new(sandbox_id));
        let session_id = session.id().to_string();
        self.sessions.write().insert(session_id.clone(), session);
        session_id
    }

    /// Record a profile event into the specified session.
    ///
    /// If the session does not exist, this is a no-op.
    /// Events may be skipped based on the configured sampling rate.
    pub fn record(&self, session_id: &str, event: ProfileEvent) {
        if !self.enabled {
            return;
        }

        if self.sampling_rate < 1.0 {
            // Simple deterministic sampling: use a hash of the session_id
            // and event count to decide whether to sample. For production
            // use, a proper random sampling approach is preferred.
            let sessions = self.sessions.read();
            if let Some(session) = sessions.get(session_id) {
                let count = session.event_count() as f64;
                // Sample based on the fractional part of count * sampling_rate
                if (count * self.sampling_rate).fract() > self.sampling_rate {
                    return;
                }
            }
        }

        let sessions = self.sessions.read();
        if let Some(session) = sessions.get(session_id) {
            session.record(event);
        }
    }

    /// End a profiling session and return the aggregated execution profile.
    ///
    /// Returns `None` if no session with the given ID exists.
    pub fn end_session(&self, session_id: &str) -> Option<ExecutionProfile> {
        let session = self.sessions.write().remove(session_id)?;
        Some(ExecutionProfile::from_session(&session))
    }

    /// Check whether the profiler is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the configured sampling rate.
    pub fn sampling_rate(&self) -> f64 {
        self.sampling_rate
    }

    /// Get a reference to an active session by ID.
    pub fn get_session(&self, session_id: &str) -> Option<Arc<ProfileSession>> {
        self.sessions.read().get(session_id).cloned()
    }

    /// Get the number of active profiling sessions.
    pub fn active_session_count(&self) -> usize {
        self.sessions.read().len()
    }
}

impl Default for SandboxProfiler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SandboxProfiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxProfiler")
            .field("enabled", &self.enabled)
            .field("sampling_rate", &self.sampling_rate)
            .field("active_sessions", &self.active_session_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_profile_event_variants() {
        let now = Instant::now();

        let entry = ProfileEvent::FunctionEntry { name: "main".to_string(), timestamp: now };
        assert!(matches!(entry, ProfileEvent::FunctionEntry { .. }));

        let exit = ProfileEvent::FunctionExit {
            name: "main".to_string(),
            timestamp: now,
            duration: Duration::from_millis(10),
        };
        assert!(matches!(exit, ProfileEvent::FunctionExit { .. }));

        let alloc = ProfileEvent::MemoryAllocation { bytes: 1024, address: 0x1000 };
        assert!(matches!(alloc, ProfileEvent::MemoryAllocation { .. }));

        let free = ProfileEvent::MemoryFree { bytes: 1024, address: 0x1000 };
        assert!(matches!(free, ProfileEvent::MemoryFree { .. }));

        let fuel = ProfileEvent::FuelConsumed { amount: 500, remaining: 9500 };
        assert!(matches!(fuel, ProfileEvent::FuelConsumed { .. }));

        let wasi = ProfileEvent::WasiCall {
            name: "fd_write".to_string(),
            duration: Duration::from_micros(50),
        };
        assert!(matches!(wasi, ProfileEvent::WasiCall { .. }));
    }

    #[test]
    fn test_profile_session_new() {
        let session = ProfileSession::new("sandbox-1");
        assert_eq!(session.sandbox_id(), "sandbox-1");
        assert!(!session.id().is_empty());
        assert_eq!(session.event_count(), 0);
    }

    #[test]
    fn test_profile_session_record() {
        let session = ProfileSession::new("sandbox-1");

        session.record(ProfileEvent::FunctionEntry {
            name: "main".to_string(),
            timestamp: Instant::now(),
        });
        assert_eq!(session.event_count(), 1);

        session.record(ProfileEvent::FunctionExit {
            name: "main".to_string(),
            timestamp: Instant::now(),
            duration: Duration::from_millis(5),
        });
        assert_eq!(session.event_count(), 2);
    }

    #[test]
    fn test_profile_session_elapsed() {
        let session = ProfileSession::new("sandbox-1");
        // Elapsed should be non-negative (and very small).
        let elapsed = session.elapsed();
        assert!(elapsed < Duration::from_secs(1));
    }

    #[test]
    fn test_profile_session_debug() {
        let session = ProfileSession::new("sandbox-1");
        let debug_str = format!("{:?}", session);
        assert!(debug_str.contains("ProfileSession"));
        assert!(debug_str.contains("sandbox-1"));
    }

    #[test]
    fn test_function_profile_new() {
        let fp = FunctionProfile::new("test_fn".to_string(), Duration::from_millis(10));
        assert_eq!(fp.name, "test_fn");
        assert_eq!(fp.call_count, 1);
        assert_eq!(fp.total_duration, Duration::from_millis(10));
        assert_eq!(fp.min_duration, Duration::from_millis(10));
        assert_eq!(fp.max_duration, Duration::from_millis(10));
    }

    #[test]
    fn test_function_profile_avg_duration() {
        let mut fp = FunctionProfile::new("test_fn".to_string(), Duration::from_millis(10));
        fp.record(Duration::from_millis(20));
        fp.record(Duration::from_millis(30));

        assert_eq!(fp.call_count, 3);
        assert_eq!(fp.total_duration, Duration::from_millis(60));
        assert_eq!(fp.avg_duration(), Duration::from_millis(20));
    }

    #[test]
    fn test_function_profile_avg_duration_zero_calls() {
        let fp = FunctionProfile {
            name: "empty".to_string(),
            call_count: 0,
            total_duration: Duration::ZERO,
            min_duration: Duration::MAX,
            max_duration: Duration::ZERO,
        };
        assert_eq!(fp.avg_duration(), Duration::ZERO);
    }

    #[test]
    fn test_function_profile_min_max() {
        let mut fp = FunctionProfile::new("test_fn".to_string(), Duration::from_millis(10));
        fp.record(Duration::from_millis(5));
        fp.record(Duration::from_millis(20));

        assert_eq!(fp.min_duration, Duration::from_millis(5));
        assert_eq!(fp.max_duration, Duration::from_millis(20));
    }

    #[test]
    fn test_execution_profile_from_session() {
        let session = ProfileSession::new("sandbox-1");
        let now = Instant::now();

        // Simulate function calls
        session.record(ProfileEvent::FunctionEntry { name: "main".to_string(), timestamp: now });
        session.record(ProfileEvent::FunctionExit {
            name: "main".to_string(),
            timestamp: now + Duration::from_millis(100),
            duration: Duration::from_millis(100),
        });
        session.record(ProfileEvent::FunctionEntry {
            name: "helper".to_string(),
            timestamp: now + Duration::from_millis(10),
        });
        session.record(ProfileEvent::FunctionExit {
            name: "helper".to_string(),
            timestamp: now + Duration::from_millis(30),
            duration: Duration::from_millis(20),
        });
        session.record(ProfileEvent::FunctionEntry {
            name: "helper".to_string(),
            timestamp: now + Duration::from_millis(50),
        });
        session.record(ProfileEvent::FunctionExit {
            name: "helper".to_string(),
            timestamp: now + Duration::from_millis(60),
            duration: Duration::from_millis(10),
        });

        // Memory events
        session.record(ProfileEvent::MemoryAllocation { bytes: 1024, address: 0x1000 });
        session.record(ProfileEvent::MemoryAllocation { bytes: 2048, address: 0x2000 });
        session.record(ProfileEvent::MemoryFree { bytes: 1024, address: 0x1000 });

        // Fuel events
        session.record(ProfileEvent::FuelConsumed { amount: 500, remaining: 9500 });
        session.record(ProfileEvent::FuelConsumed { amount: 300, remaining: 9200 });

        // WASI calls
        session.record(ProfileEvent::WasiCall {
            name: "fd_write".to_string(),
            duration: Duration::from_micros(50),
        });
        session.record(ProfileEvent::WasiCall {
            name: "fd_write".to_string(),
            duration: Duration::from_micros(30),
        });
        session.record(ProfileEvent::WasiCall {
            name: "fd_read".to_string(),
            duration: Duration::from_micros(100),
        });

        let profile = ExecutionProfile::from_session(&session);

        // Check function profiles
        assert_eq!(profile.functions.len(), 2);

        let main_profile = &profile.functions["main"];
        assert_eq!(main_profile.call_count, 1);
        assert_eq!(main_profile.total_duration, Duration::from_millis(100));

        let helper_profile = &profile.functions["helper"];
        assert_eq!(helper_profile.call_count, 2);
        assert_eq!(helper_profile.total_duration, Duration::from_millis(30));
        assert_eq!(helper_profile.min_duration, Duration::from_millis(10));
        assert_eq!(helper_profile.max_duration, Duration::from_millis(20));

        // Check memory
        // Peak: 1024 + 2048 = 3072, then freed 1024 -> current = 2048, but peak was 3072
        assert_eq!(profile.peak_memory, 3072);

        // Check fuel
        assert_eq!(profile.total_fuel_consumed, 800);

        // Check WASI calls
        assert_eq!(profile.wasi_calls.len(), 2);
        let fd_write_profile = &profile.wasi_calls["fd_write"];
        assert_eq!(fd_write_profile.call_count, 2);
        let fd_read_profile = &profile.wasi_calls["fd_read"];
        assert_eq!(fd_read_profile.call_count, 1);
    }

    #[test]
    fn test_execution_profile_hottest_functions() {
        let session = ProfileSession::new("sandbox-1");
        let now = Instant::now();

        // Record three functions with different durations
        session.record(ProfileEvent::FunctionExit {
            name: "slow".to_string(),
            timestamp: now,
            duration: Duration::from_millis(100),
        });
        session.record(ProfileEvent::FunctionExit {
            name: "medium".to_string(),
            timestamp: now,
            duration: Duration::from_millis(50),
        });
        session.record(ProfileEvent::FunctionExit {
            name: "fast".to_string(),
            timestamp: now,
            duration: Duration::from_millis(10),
        });

        let profile = ExecutionProfile::from_session(&session);
        let hottest = profile.hottest_functions(2);

        assert_eq!(hottest.len(), 2);
        assert_eq!(hottest[0].name, "slow");
        assert_eq!(hottest[1].name, "medium");
    }

    #[test]
    fn test_execution_profile_hottest_functions_more_than_available() {
        let session = ProfileSession::new("sandbox-1");
        let now = Instant::now();

        session.record(ProfileEvent::FunctionExit {
            name: "only_one".to_string(),
            timestamp: now,
            duration: Duration::from_millis(10),
        });

        let profile = ExecutionProfile::from_session(&session);
        let hottest = profile.hottest_functions(10);

        assert_eq!(hottest.len(), 1);
        assert_eq!(hottest[0].name, "only_one");
    }

    #[test]
    fn test_execution_profile_empty_session() {
        let session = ProfileSession::new("sandbox-1");
        let profile = ExecutionProfile::from_session(&session);

        assert!(profile.functions.is_empty());
        assert!(profile.wasi_calls.is_empty());
        assert_eq!(profile.peak_memory, 0);
        assert_eq!(profile.total_fuel_consumed, 0);
    }

    #[test]
    fn test_execution_profile_memory_free_without_alloc() {
        let session = ProfileSession::new("sandbox-1");

        // Free without a prior allocation should not underflow
        session.record(ProfileEvent::MemoryFree { bytes: 1024, address: 0x1000 });

        let profile = ExecutionProfile::from_session(&session);
        assert_eq!(profile.peak_memory, 0);
    }

    #[test]
    fn test_sandbox_profiler_new() {
        let profiler = SandboxProfiler::new();
        assert!(profiler.is_enabled());
        assert_eq!(profiler.sampling_rate(), 1.0);
        assert_eq!(profiler.active_session_count(), 0);
    }

    #[test]
    fn test_sandbox_profiler_default() {
        let profiler = SandboxProfiler::default();
        assert!(profiler.is_enabled());
        assert_eq!(profiler.sampling_rate(), 1.0);
    }

    #[test]
    fn test_sandbox_profiler_with_sampling_rate() {
        let profiler = SandboxProfiler::new().with_sampling_rate(0.5);
        assert_eq!(profiler.sampling_rate(), 0.5);
    }

    #[test]
    fn test_sandbox_profiler_sampling_rate_clamped() {
        let profiler_low = SandboxProfiler::new().with_sampling_rate(-1.0);
        assert_eq!(profiler_low.sampling_rate(), 0.0);

        let profiler_high = SandboxProfiler::new().with_sampling_rate(2.0);
        assert_eq!(profiler_high.sampling_rate(), 1.0);
    }

    #[test]
    fn test_sandbox_profiler_start_end_session() {
        let profiler = SandboxProfiler::new();
        let session_id = profiler.start_session("sandbox-1");

        assert_eq!(profiler.active_session_count(), 1);
        assert!(profiler.get_session(&session_id).is_some());

        let profile = profiler.end_session(&session_id);
        assert!(profile.is_some());
        assert_eq!(profiler.active_session_count(), 0);
    }

    #[test]
    fn test_sandbox_profiler_end_nonexistent_session() {
        let profiler = SandboxProfiler::new();
        let profile = profiler.end_session("nonexistent");
        assert!(profile.is_none());
    }

    #[test]
    fn test_sandbox_profiler_record_event() {
        let profiler = SandboxProfiler::new();
        let session_id = profiler.start_session("sandbox-1");

        profiler.record(
            &session_id,
            ProfileEvent::FunctionEntry { name: "main".to_string(), timestamp: Instant::now() },
        );

        let session = profiler.get_session(&session_id).unwrap();
        assert_eq!(session.event_count(), 1);
    }

    #[test]
    fn test_sandbox_profiler_record_to_nonexistent_session() {
        let profiler = SandboxProfiler::new();

        // Should be a no-op, not panic.
        profiler.record("nonexistent", ProfileEvent::FuelConsumed { amount: 100, remaining: 900 });
    }

    #[test]
    fn test_sandbox_profiler_multiple_sessions() {
        let profiler = SandboxProfiler::new();
        let id1 = profiler.start_session("sandbox-1");
        let id2 = profiler.start_session("sandbox-2");

        assert_eq!(profiler.active_session_count(), 2);

        profiler.record(&id1, ProfileEvent::FuelConsumed { amount: 100, remaining: 900 });
        profiler.record(&id2, ProfileEvent::FuelConsumed { amount: 200, remaining: 800 });

        let profile1 = profiler.end_session(&id1).unwrap();
        assert_eq!(profile1.total_fuel_consumed, 100);
        assert_eq!(profiler.active_session_count(), 1);

        let profile2 = profiler.end_session(&id2).unwrap();
        assert_eq!(profile2.total_fuel_consumed, 200);
        assert_eq!(profiler.active_session_count(), 0);
    }

    #[test]
    fn test_sandbox_profiler_full_workflow() {
        let profiler = SandboxProfiler::new();
        let session_id = profiler.start_session("sandbox-workflow");

        let now = Instant::now();

        // Simulate a complete execution flow
        profiler.record(
            &session_id,
            ProfileEvent::FunctionEntry { name: "_start".to_string(), timestamp: now },
        );
        profiler
            .record(&session_id, ProfileEvent::MemoryAllocation { bytes: 4096, address: 0x10000 });
        profiler.record(
            &session_id,
            ProfileEvent::WasiCall {
                name: "fd_write".to_string(),
                duration: Duration::from_micros(100),
            },
        );
        profiler.record(&session_id, ProfileEvent::FuelConsumed { amount: 1000, remaining: 9000 });
        profiler.record(
            &session_id,
            ProfileEvent::FunctionExit {
                name: "_start".to_string(),
                timestamp: now + Duration::from_millis(50),
                duration: Duration::from_millis(50),
            },
        );

        let profile = profiler.end_session(&session_id).unwrap();

        assert_eq!(profile.functions.len(), 1);
        assert_eq!(profile.functions["_start"].call_count, 1);
        assert_eq!(profile.peak_memory, 4096);
        assert_eq!(profile.total_fuel_consumed, 1000);
        assert_eq!(profile.wasi_calls.len(), 1);
        assert_eq!(profile.wasi_calls["fd_write"].call_count, 1);
    }

    #[test]
    fn test_sandbox_profiler_debug() {
        let profiler = SandboxProfiler::new();
        let debug_str = format!("{:?}", profiler);
        assert!(debug_str.contains("SandboxProfiler"));
        assert!(debug_str.contains("enabled"));
        assert!(debug_str.contains("sampling_rate"));
    }

    #[test]
    fn test_sandbox_profiler_concurrent_access() {
        let profiler = Arc::new(SandboxProfiler::new());
        let session_id = profiler.start_session("sandbox-concurrent");

        let mut handles = Vec::new();
        for i in 0..10 {
            let profiler_clone = Arc::clone(&profiler);
            let sid = session_id.clone();
            let handle = thread::spawn(move || {
                profiler_clone.record(
                    &sid,
                    ProfileEvent::FuelConsumed { amount: i * 10, remaining: 10000 - i * 10 },
                );
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let session = profiler.get_session(&session_id).unwrap();
        assert_eq!(session.event_count(), 10);
    }

    #[test]
    fn test_profile_event_serialization() {
        let event = ProfileEvent::MemoryAllocation { bytes: 2048, address: 0x5000 };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ProfileEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            deserialized,
            ProfileEvent::MemoryAllocation { bytes: 2048, address: 0x5000 }
        ));
    }

    #[test]
    fn test_profile_event_serialization_function_exit_timestamp_is_null() {
        let event = ProfileEvent::FunctionExit {
            name: "test".to_string(),
            timestamp: Instant::now(),
            duration: Duration::from_millis(42),
        };
        let json = serde_json::to_string(&event).unwrap();
        // The timestamp field is serialized as null (Instant has no portable representation).
        assert!(json.contains("\"timestamp\":null"));
        assert!(json.contains("duration"));

        // Verify round-trip deserialization works
        let deserialized: ProfileEvent = serde_json::from_str(&json).unwrap();
        if let ProfileEvent::FunctionExit { name, duration, .. } = deserialized {
            assert_eq!(name, "test");
            assert_eq!(duration, Duration::from_millis(42));
        } else {
            panic!("Expected FunctionExit variant");
        }
    }

    #[test]
    fn test_function_profile_serialization() {
        let fp = FunctionProfile::new("test_fn".to_string(), Duration::from_millis(10));
        let json = serde_json::to_string(&fp).unwrap();
        let deserialized: FunctionProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test_fn");
        assert_eq!(deserialized.call_count, 1);
    }

    #[test]
    fn test_execution_profile_serialization() {
        let session = ProfileSession::new("sandbox-1");
        session.record(ProfileEvent::FunctionExit {
            name: "main".to_string(),
            timestamp: Instant::now(),
            duration: Duration::from_millis(42),
        });
        session.record(ProfileEvent::FuelConsumed { amount: 500, remaining: 9500 });

        let profile = ExecutionProfile::from_session(&session);
        let json = serde_json::to_string(&profile).unwrap();
        let deserialized: ExecutionProfile = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.functions.len(), 1);
        assert_eq!(deserialized.total_fuel_consumed, 500);
    }
}
