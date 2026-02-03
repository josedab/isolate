//! Circuit breaker pattern for protecting downstream services.
//!
//! Implements the standard three-state circuit breaker (Closed → Open → Half-Open)
//! with configurable thresholds and automatic recovery.

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Circuit breaker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Normal operation, requests flow through.
    Closed,
    /// Circuit tripped, all requests are rejected.
    Open,
    /// Testing recovery, limited requests allowed.
    HalfOpen,
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures to trip the circuit.
    pub failure_threshold: u32,
    /// How long to wait before testing recovery.
    pub open_duration: Duration,
    /// Successful requests needed to close the circuit from half-open.
    pub success_threshold: u32,
    /// Window duration for counting failures.
    pub window_duration: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 3,
            window_duration: Duration::from_secs(60),
        }
    }
}

/// Circuit breaker that protects against cascading failures.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: parking_lot::RwLock<CircuitState>,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure: parking_lot::Mutex<Option<Instant>>,
    opened_at: parking_lot::Mutex<Option<Instant>>,
    total_rejected: AtomicU64,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: parking_lot::RwLock::new(CircuitState::Closed),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure: parking_lot::Mutex::new(None),
            opened_at: parking_lot::Mutex::new(None),
            total_rejected: AtomicU64::new(0),
        }
    }

    /// Check if a request should be allowed through.
    pub fn allow_request(&self) -> bool {
        let state = *self.state.read();
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if open duration has elapsed
                if let Some(opened_at) = *self.opened_at.lock() {
                    if opened_at.elapsed() >= self.config.open_duration {
                        *self.state.write() = CircuitState::HalfOpen;
                        self.success_count.store(0, Ordering::Relaxed);
                        return true; // Allow first request in half-open
                    }
                }
                self.total_rejected.fetch_add(1, Ordering::Relaxed);
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        let state = *self.state.read();
        match state {
            CircuitState::HalfOpen => {
                let count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                if count >= self.config.success_threshold as u64 {
                    *self.state.write() = CircuitState::Closed;
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.success_count.store(0, Ordering::Relaxed);
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success in normal window
                if let Some(last) = *self.last_failure.lock() {
                    if last.elapsed() > self.config.window_duration {
                        self.failure_count.store(0, Ordering::Relaxed);
                    }
                }
            }
            _ => {}
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        let state = *self.state.read();
        match state {
            CircuitState::Closed => {
                let count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                *self.last_failure.lock() = Some(Instant::now());

                if count >= self.config.failure_threshold as u64 {
                    *self.state.write() = CircuitState::Open;
                    *self.opened_at.lock() = Some(Instant::now());
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open → re-open
                *self.state.write() = CircuitState::Open;
                *self.opened_at.lock() = Some(Instant::now());
                self.success_count.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Get current circuit state.
    pub fn state(&self) -> CircuitState {
        *self.state.read()
    }

    /// Total requests rejected by the circuit breaker.
    pub fn total_rejected(&self) -> u64 {
        self.total_rejected.load(Ordering::Relaxed)
    }

    /// Reset the circuit breaker to closed state.
    pub fn reset(&self) {
        *self.state.write() = CircuitState::Closed;
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        *self.opened_at.lock() = None;
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration: Duration::from_millis(50),
            success_threshold: 2,
            window_duration: Duration::from_secs(60),
        }
    }

    #[test]
    fn test_initial_state_closed() {
        let cb = CircuitBreaker::new(test_config());
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_trip_on_failures() {
        let cb = CircuitBreaker::new(test_config());
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);

        cb.record_failure(); // 3rd failure → Open
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn test_recovery_to_half_open() {
        let cb = CircuitBreaker::new(test_config());
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for open duration
        std::thread::sleep(Duration::from_millis(60));
        assert!(cb.allow_request());
        assert_eq!(cb.state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_success_closes() {
        let cb = CircuitBreaker::new(test_config());
        for _ in 0..3 {
            cb.record_failure();
        }
        std::thread::sleep(Duration::from_millis(60));
        cb.allow_request(); // transitions to HalfOpen

        cb.record_success();
        cb.record_success(); // 2nd success → Closed
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(test_config());
        for _ in 0..3 {
            cb.record_failure();
        }
        std::thread::sleep(Duration::from_millis(60));
        cb.allow_request(); // HalfOpen

        cb.record_failure(); // → Open again
        assert_eq!(cb.state(), CircuitState::Open);
    }

    #[test]
    fn test_reset() {
        let cb = CircuitBreaker::new(test_config());
        for _ in 0..3 {
            cb.record_failure();
        }
        assert_eq!(cb.state(), CircuitState::Open);

        cb.reset();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn test_total_rejected() {
        let cb = CircuitBreaker::new(test_config());
        for _ in 0..3 {
            cb.record_failure();
        }
        cb.allow_request(); // rejected
        cb.allow_request(); // rejected
        assert_eq!(cb.total_rejected(), 2);
    }
}
