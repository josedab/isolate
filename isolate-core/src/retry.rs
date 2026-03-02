//! Retry utilities for transient errors.
//!
//! Provides configurable retry logic with exponential backoff and jitter,
//! designed to work with [`Error::is_retryable()`](crate::Error::is_retryable).
//!
//! # Examples
//!
//! ```rust
//! use isolate_core::retry::{RetryPolicy, retry};
//! use isolate_core::error::{Error, Result};
//! use std::time::Duration;
//! use std::sync::atomic::{AtomicU32, Ordering};
//!
//! # #[tokio::main]
//! # async fn main() -> Result<()> {
//! let attempt = AtomicU32::new(0);
//!
//! let result = retry(RetryPolicy::default(), || async {
//!     let n = attempt.fetch_add(1, Ordering::SeqCst);
//!     if n < 2 {
//!         Err(Error::PoolExhausted) // retryable
//!     } else {
//!         Ok(42)
//!     }
//! })
//! .await;
//!
//! assert_eq!(result.unwrap(), 42);
//! assert_eq!(attempt.load(Ordering::SeqCst), 3); // 2 retries + 1 success
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use std::future::Future;
use std::time::Duration;

/// Configuration for retry behavior.
///
/// # Examples
///
/// ```
/// use isolate_core::retry::RetryPolicy;
/// use std::time::Duration;
///
/// // Quick retries for tests
/// let fast = RetryPolicy::new(3, Duration::from_millis(10));
///
/// // Production defaults
/// let prod = RetryPolicy::default();
/// assert_eq!(prod.max_retries, 3);
/// ```
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Base delay before the first retry. Subsequent retries use exponential backoff.
    pub base_delay: Duration,
    /// Maximum delay between retries (caps the exponential growth).
    pub max_delay: Duration,
    /// Whether to add random jitter to the delay (±25%).
    pub jitter: bool,
}

impl RetryPolicy {
    /// Create a new retry policy with the given max retries and base delay.
    pub fn new(max_retries: u32, base_delay: Duration) -> Self {
        Self { max_retries, base_delay, max_delay: Duration::from_secs(30), jitter: true }
    }

    /// Set the maximum delay cap.
    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Disable jitter (useful for deterministic tests).
    pub fn without_jitter(mut self) -> Self {
        self.jitter = false;
        self
    }

    /// Compute the delay for the given attempt (0-indexed).
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        // Exponential backoff: base * 2^attempt
        let exp = self.base_delay.saturating_mul(1u32.wrapping_shl(attempt));
        let capped = exp.min(self.max_delay);

        if self.jitter {
            apply_jitter(capped)
        } else {
            capped
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            jitter: true,
        }
    }
}

/// Outcome of a retry operation.
#[derive(Debug)]
pub struct RetryOutcome<T> {
    /// The final result (success or last error).
    pub result: Result<T>,
    /// Total number of attempts made (1 = no retries).
    pub attempts: u32,
}

/// Retry an async operation with the given policy.
///
/// The operation is called repeatedly until it succeeds, returns a
/// non-retryable error, or exhausts all retry attempts.
///
/// Only errors where [`Error::is_retryable()`] returns `true` trigger
/// a retry. Non-retryable errors are returned immediately.
///
/// # Examples
///
/// ```rust
/// use isolate_core::retry::{RetryPolicy, retry};
/// use isolate_core::error::Error;
///
/// # #[tokio::main]
/// # async fn main() {
/// // Non-retryable errors return immediately
/// let policy = RetryPolicy::new(5, std::time::Duration::from_millis(1));
/// let result: Result<(), _> = retry(policy, || async {
///     Err(Error::InvalidConfig("bad".to_string()))
/// }).await;
/// assert!(result.is_err());
/// # }
/// ```
pub async fn retry<T, F, Fut>(policy: RetryPolicy, mut operation: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let outcome = retry_with_outcome(policy, &mut operation).await;
    outcome.result
}

/// Retry an async operation and return detailed outcome including attempt count.
///
/// Like [`retry()`] but returns a [`RetryOutcome`] with the attempt count.
pub async fn retry_with_outcome<T, F, Fut>(
    policy: RetryPolicy,
    operation: &mut F,
) -> RetryOutcome<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempts = 0u32;

    loop {
        attempts += 1;
        match operation().await {
            Ok(value) => {
                return RetryOutcome { result: Ok(value), attempts };
            }
            Err(err) => {
                let retries_left = policy.max_retries.saturating_sub(attempts.saturating_sub(1));
                if !err.is_retryable() || retries_left == 0 {
                    return RetryOutcome { result: Err(err), attempts };
                }

                let delay = policy.delay_for_attempt(attempts - 1);
                tracing::debug!(
                    attempt = attempts,
                    retries_left = retries_left - 1,
                    delay_ms = delay.as_millis() as u64,
                    error = %err,
                    "Retrying after transient error"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Apply ±25% jitter to a duration using simple deterministic hashing.
fn apply_jitter(base: Duration) -> Duration {
    // Use a simple time-based source for jitter to avoid rand dependency.
    // The nanos portion of current time provides sufficient randomness for jitter.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();

    // Map to range [0.75, 1.25]
    let factor = 0.75 + (nanos as f64 / u32::MAX as f64) * 0.5;
    base.mul_f64(factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn fast_policy(max_retries: u32) -> RetryPolicy {
        RetryPolicy::new(max_retries, Duration::from_millis(1)).without_jitter()
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let result = retry(fast_policy(3), || async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_succeeds_after_retries() {
        let attempt = AtomicU32::new(0);
        let result: Result<&str> = retry(fast_policy(3), || async {
            let n = attempt.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(Error::PoolExhausted)
            } else {
                Ok("done")
            }
        })
        .await;

        assert_eq!(result.unwrap(), "done");
        assert_eq!(attempt.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausts_retries() {
        let attempt = AtomicU32::new(0);
        let result: Result<()> = retry(fast_policy(2), || async {
            attempt.fetch_add(1, Ordering::SeqCst);
            Err(Error::PoolExhausted)
        })
        .await;

        assert!(result.is_err());
        // 1 initial + 2 retries = 3 attempts
        assert_eq!(attempt.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_non_retryable_returns_immediately() {
        let attempt = AtomicU32::new(0);
        let result: Result<()> = retry(fast_policy(5), || async {
            attempt.fetch_add(1, Ordering::SeqCst);
            Err(Error::InvalidConfig("bad config".to_string()))
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempt.load(Ordering::SeqCst), 1); // No retries
    }

    #[tokio::test]
    async fn test_retry_with_outcome() {
        let attempt = AtomicU32::new(0);
        let outcome = retry_with_outcome(fast_policy(3), &mut || async {
            let n = attempt.fetch_add(1, Ordering::SeqCst);
            if n < 1 {
                Err(Error::PoolExhausted)
            } else {
                Ok(99)
            }
        })
        .await;

        assert_eq!(outcome.result.unwrap(), 99);
        assert_eq!(outcome.attempts, 2);
    }

    #[tokio::test]
    async fn test_retry_zero_retries() {
        let attempt = AtomicU32::new(0);
        let result: Result<()> = retry(fast_policy(0), || async {
            attempt.fetch_add(1, Ordering::SeqCst);
            Err(Error::PoolExhausted)
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempt.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_retry_policy_defaults() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay, Duration::from_millis(100));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
        assert!(policy.jitter);
    }

    #[test]
    fn test_retry_policy_builder() {
        let policy = RetryPolicy::new(5, Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(60))
            .without_jitter();

        assert_eq!(policy.max_retries, 5);
        assert_eq!(policy.base_delay, Duration::from_secs(1));
        assert_eq!(policy.max_delay, Duration::from_secs(60));
        assert!(!policy.jitter);
    }

    #[test]
    fn test_delay_exponential_backoff() {
        let policy = RetryPolicy::new(5, Duration::from_millis(100)).without_jitter();

        assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(400));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(800));
    }

    #[test]
    fn test_delay_capped_at_max() {
        let policy = RetryPolicy::new(10, Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(5))
            .without_jitter();

        // 2^4 = 16s > 5s cap
        assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(5));
        assert_eq!(policy.delay_for_attempt(10), Duration::from_secs(5));
    }

    #[test]
    fn test_jitter_stays_in_range() {
        let policy = RetryPolicy::new(3, Duration::from_secs(1));
        for _ in 0..100 {
            let delay = policy.delay_for_attempt(0);
            // ±25% of 1s = [750ms, 1250ms]
            assert!(delay >= Duration::from_millis(750), "delay too low: {:?}", delay);
            assert!(delay <= Duration::from_millis(1250), "delay too high: {:?}", delay);
        }
    }
}
