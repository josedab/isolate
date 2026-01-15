//! Token bucket rate limiter implementation.

use parking_lot::Mutex;
use std::time::Instant;

/// A token bucket rate limiter for controlling request throughput.
///
/// Tokens refill at a steady rate. Each request consumes tokens.
/// If the bucket is empty, requests are denied until tokens refill.
pub struct TokenBucket {
    state: Mutex<BucketState>,
    rate: f64,      // tokens per second
    capacity: u64,  // max tokens
}

struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    /// Create a new token bucket.
    ///
    /// * `rate` - Tokens added per second (sustained rate)
    /// * `capacity` - Maximum tokens (burst size)
    pub fn new(rate: f64, capacity: u64) -> Self {
        Self {
            state: Mutex::new(BucketState {
                tokens: capacity as f64, // Start full
                last_refill: Instant::now(),
            }),
            rate,
            capacity,
        }
    }

    /// Try to consume `count` tokens. Returns true if successful.
    pub fn try_acquire(&self, count: u64) -> bool {
        let mut state = self.state.lock();
        self.refill(&mut state);

        let needed = count as f64;
        if state.tokens >= needed {
            state.tokens -= needed;
            true
        } else {
            false
        }
    }

    /// Get the number of currently available tokens (approximate).
    pub fn available_tokens(&self) -> u64 {
        let mut state = self.state.lock();
        self.refill(&mut state);
        state.tokens as u64
    }

    /// Get the sustained rate (tokens per second).
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Get the bucket capacity (burst size).
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    fn refill(&self, state: &mut BucketState) {
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        let new_tokens = elapsed * self.rate;

        if new_tokens > 0.0 {
            state.tokens = (state.tokens + new_tokens).min(self.capacity as f64);
            state.last_refill = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_bucket_initial_burst() {
        let bucket = TokenBucket::new(10.0, 5);

        // Should have 5 tokens initially
        assert!(bucket.try_acquire(5));
        assert!(!bucket.try_acquire(1)); // Empty now
    }

    #[test]
    fn test_token_bucket_partial_consume() {
        let bucket = TokenBucket::new(100.0, 10);

        assert!(bucket.try_acquire(3));
        assert!(bucket.try_acquire(3));
        assert!(bucket.try_acquire(3));
        assert!(!bucket.try_acquire(3)); // Only ~1 left
    }

    #[test]
    fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(1000.0, 10);

        // Drain all
        assert!(bucket.try_acquire(10));
        assert!(!bucket.try_acquire(1));

        // Wait a tiny bit for refill
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Should have refilled some tokens
        assert!(bucket.available_tokens() > 0);
    }

    #[test]
    fn test_token_bucket_capacity() {
        let bucket = TokenBucket::new(10.0, 5);
        assert_eq!(bucket.capacity(), 5);
        assert_eq!(bucket.rate(), 10.0);
    }
}
