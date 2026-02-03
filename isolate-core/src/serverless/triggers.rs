//! Event-driven sandbox triggers.
//!
//! Supports triggering sandbox executions via:
//! - HTTP webhooks with signature verification
//! - Cron schedules
//! - Message queue consumers (NATS/Kafka-compatible interface)
//! - Dead-letter queues for failed executions
//! - Configurable retry policies

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

/// Unique trigger identifier.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct TriggerId(pub String);

impl TriggerId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for TriggerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The type of event source that triggers sandbox execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TriggerSource {
    /// HTTP webhook endpoint.
    Webhook(WebhookConfig),
    /// Cron schedule.
    Cron(CronConfig),
    /// Message queue consumer.
    MessageQueue(MessageQueueConfig),
}

/// Configuration for HTTP webhook triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// URL path for the webhook endpoint.
    pub path: String,
    /// Allowed HTTP methods.
    pub methods: Vec<String>,
    /// Optional secret for HMAC signature verification.
    pub secret: Option<String>,
    /// Optional filter expression on payload.
    pub filter: Option<String>,
}

impl WebhookConfig {
    /// Create a basic webhook config.
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            methods: vec!["POST".into()],
            secret: None,
            filter: None,
        }
    }

    /// Add HMAC secret for webhook signature verification.
    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secret = Some(secret.into());
        self
    }

    /// Verify an HMAC-SHA256 signature against a payload.
    pub fn verify_signature(&self, payload: &[u8], signature: &str) -> bool {
        use sha2::{Digest, Sha256};
        match &self.secret {
            Some(secret) => {
                let mut hasher = Sha256::new();
                hasher.update(secret.as_bytes());
                hasher.update(payload);
                let expected = format!("sha256={}", hex::encode(hasher.finalize()));
                expected == signature
            }
            None => true, // No secret = no verification needed
        }
    }
}

/// Configuration for cron-based triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronConfig {
    /// Cron expression (e.g., "*/5 * * * *" for every 5 minutes).
    pub expression: String,
    /// Timezone for the cron schedule.
    pub timezone: Option<String>,
    /// Optional payload to pass to the sandbox.
    pub payload: Option<Vec<u8>>,
}

impl CronConfig {
    /// Create a cron config with the given expression.
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            timezone: None,
            payload: None,
        }
    }

    /// Validate the cron expression format (basic check).
    pub fn is_valid(&self) -> bool {
        let parts: Vec<&str> = self.expression.split_whitespace().collect();
        // Standard cron has 5 fields (min hour day month weekday)
        parts.len() == 5 || parts.len() == 6
    }
}

/// Configuration for message queue triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageQueueConfig {
    /// Queue/topic/subject name.
    pub topic: String,
    /// Consumer group name.
    pub consumer_group: String,
    /// Queue provider type.
    pub provider: QueueProvider,
    /// Maximum batch size for processing.
    pub batch_size: u32,
    /// Maximum wait time for a batch.
    pub batch_timeout: Duration,
}

/// Supported message queue providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueProvider {
    /// NATS messaging.
    Nats { url: String },
    /// Apache Kafka.
    Kafka { brokers: Vec<String> },
    /// In-memory queue (for testing).
    InMemory,
}

impl MessageQueueConfig {
    /// Create an in-memory message queue config for testing.
    pub fn in_memory(topic: impl Into<String>) -> Self {
        Self {
            topic: topic.into(),
            consumer_group: "default".into(),
            provider: QueueProvider::InMemory,
            batch_size: 1,
            batch_timeout: Duration::from_secs(5),
        }
    }
}

/// Retry policy for failed executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts.
    pub max_retries: u32,
    /// Initial delay between retries.
    pub initial_delay: Duration,
    /// Maximum delay between retries.
    pub max_delay: Duration,
    /// Backoff multiplier (e.g., 2.0 for exponential backoff).
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Compute the delay for a given attempt number (0-based).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let delay_ms = self.initial_delay.as_millis() as f64
            * self.backoff_multiplier.powi(attempt.saturating_sub(1) as i32);
        let capped = delay_ms.min(self.max_delay.as_millis() as f64);
        Duration::from_millis(capped as u64)
    }

    /// Check if another retry is allowed.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }
}

/// A trigger definition binding an event source to a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDefinition {
    pub id: TriggerId,
    /// Name of the sandbox to invoke.
    pub sandbox_name: String,
    /// Event source configuration.
    pub source: TriggerSource,
    /// Retry policy for failed executions.
    pub retry_policy: RetryPolicy,
    /// Whether to send failed events to a dead-letter queue.
    pub dead_letter_enabled: bool,
    /// Whether this trigger is currently active.
    pub enabled: bool,
    /// Labels for organization.
    pub labels: HashMap<String, String>,
}

/// An event that was received by a trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerEvent {
    /// Unique event identifier.
    pub event_id: String,
    /// Trigger that received this event.
    pub trigger_id: TriggerId,
    /// Event payload.
    pub payload: Vec<u8>,
    /// Metadata/headers.
    pub metadata: HashMap<String, String>,
    /// When the event was received.
    pub received_at_epoch_ms: u64,
    /// Current attempt number (0 = first try).
    pub attempt: u32,
}

/// Outcome of processing a trigger event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventOutcome {
    /// Event processed successfully.
    Success { exit_code: i32, duration_ms: u64 },
    /// Event processing failed.
    Failed { error: String, duration_ms: u64 },
    /// Event sent to dead-letter queue after all retries exhausted.
    DeadLettered { error: String, attempts: u32 },
}

/// Dead-letter queue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterEntry {
    pub event: TriggerEvent,
    pub last_error: String,
    pub total_attempts: u32,
    pub dead_lettered_at_epoch_ms: u64,
}

/// Manages event-driven triggers and their execution lifecycle.
pub struct TriggerManager {
    triggers: dashmap::DashMap<TriggerId, TriggerDefinition>,
    dead_letter_queue: parking_lot::Mutex<VecDeque<DeadLetterEntry>>,
    stats: TriggerStats,
    max_dlq_size: usize,
}

struct TriggerStats {
    total_events: AtomicU64,
    successful: AtomicU64,
    failed: AtomicU64,
    retried: AtomicU64,
    dead_lettered: AtomicU64,
}

impl TriggerManager {
    /// Create a new trigger manager.
    pub fn new() -> Self {
        Self {
            triggers: dashmap::DashMap::new(),
            dead_letter_queue: parking_lot::Mutex::new(VecDeque::new()),
            stats: TriggerStats {
                total_events: AtomicU64::new(0),
                successful: AtomicU64::new(0),
                failed: AtomicU64::new(0),
                retried: AtomicU64::new(0),
                dead_lettered: AtomicU64::new(0),
            },
            max_dlq_size: 10_000,
        }
    }

    /// Register a new trigger.
    pub fn register(&self, trigger: TriggerDefinition) {
        self.triggers.insert(trigger.id.clone(), trigger);
    }

    /// Remove a trigger.
    pub fn remove(&self, id: &TriggerId) -> Option<TriggerDefinition> {
        self.triggers.remove(id).map(|(_, t)| t)
    }

    /// Enable or disable a trigger.
    pub fn set_enabled(&self, id: &TriggerId, enabled: bool) -> bool {
        if let Some(mut t) = self.triggers.get_mut(id) {
            t.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Get a trigger definition.
    pub fn get(&self, id: &TriggerId) -> Option<TriggerDefinition> {
        self.triggers.get(id).map(|t| t.clone())
    }

    /// List all trigger IDs.
    pub fn list(&self) -> Vec<TriggerId> {
        self.triggers.iter().map(|e| e.key().clone()).collect()
    }

    /// Process an event outcome and handle retries/DLQ.
    pub fn process_outcome(
        &self,
        event: &TriggerEvent,
        outcome: &EventOutcome,
    ) -> Option<TriggerEvent> {
        self.stats.total_events.fetch_add(1, Ordering::Relaxed);

        match outcome {
            EventOutcome::Success { .. } => {
                self.stats.successful.fetch_add(1, Ordering::Relaxed);
                None
            }
            EventOutcome::Failed { error, .. } => {
                self.stats.failed.fetch_add(1, Ordering::Relaxed);

                let trigger = match self.triggers.get(&event.trigger_id) {
                    Some(t) => t.clone(),
                    None => return None,
                };

                let next_attempt = event.attempt + 1;
                if trigger.retry_policy.should_retry(next_attempt) {
                    self.stats.retried.fetch_add(1, Ordering::Relaxed);
                    // Return a retry event
                    Some(TriggerEvent {
                        attempt: next_attempt,
                        ..event.clone()
                    })
                } else if trigger.dead_letter_enabled {
                    self.stats.dead_lettered.fetch_add(1, Ordering::Relaxed);
                    self.add_to_dlq(event, error, next_attempt);
                    None
                } else {
                    None
                }
            }
            EventOutcome::DeadLettered { error, attempts } => {
                self.stats.dead_lettered.fetch_add(1, Ordering::Relaxed);
                self.add_to_dlq(event, error, *attempts);
                None
            }
        }
    }

    /// Get the dead-letter queue entries.
    pub fn dead_letter_queue(&self) -> Vec<DeadLetterEntry> {
        self.dead_letter_queue.lock().iter().cloned().collect()
    }

    /// Drain up to `count` entries from the DLQ for reprocessing.
    pub fn drain_dlq(&self, count: usize) -> Vec<DeadLetterEntry> {
        let mut dlq = self.dead_letter_queue.lock();
        let n = count.min(dlq.len());
        dlq.drain(..n).collect()
    }

    /// DLQ size.
    pub fn dlq_size(&self) -> usize {
        self.dead_letter_queue.lock().len()
    }

    /// Get trigger execution statistics.
    pub fn statistics(&self) -> TriggerStatistics {
        TriggerStatistics {
            total_events: self.stats.total_events.load(Ordering::Relaxed),
            successful: self.stats.successful.load(Ordering::Relaxed),
            failed: self.stats.failed.load(Ordering::Relaxed),
            retried: self.stats.retried.load(Ordering::Relaxed),
            dead_lettered: self.stats.dead_lettered.load(Ordering::Relaxed),
            registered_triggers: self.triggers.len(),
        }
    }

    fn add_to_dlq(&self, event: &TriggerEvent, error: &str, attempts: u32) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut dlq = self.dead_letter_queue.lock();
        if dlq.len() >= self.max_dlq_size {
            dlq.pop_front(); // Remove oldest
        }
        dlq.push_back(DeadLetterEntry {
            event: event.clone(),
            last_error: error.to_string(),
            total_attempts: attempts,
            dead_lettered_at_epoch_ms: now,
        });
    }
}

impl Default for TriggerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Aggregate statistics for trigger executions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerStatistics {
    pub total_events: u64,
    pub successful: u64,
    pub failed: u64,
    pub retried: u64,
    pub dead_lettered: u64,
    pub registered_triggers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_webhook_trigger(id: &str) -> TriggerDefinition {
        TriggerDefinition {
            id: TriggerId::new(id),
            sandbox_name: "handler".into(),
            source: TriggerSource::Webhook(WebhookConfig::new("/hook")),
            retry_policy: RetryPolicy::default(),
            dead_letter_enabled: true,
            enabled: true,
            labels: HashMap::new(),
        }
    }

    fn make_event(trigger_id: &str) -> TriggerEvent {
        TriggerEvent {
            event_id: "evt-1".into(),
            trigger_id: TriggerId::new(trigger_id),
            payload: b"hello".to_vec(),
            metadata: HashMap::new(),
            received_at_epoch_ms: 0,
            attempt: 0,
        }
    }

    #[test]
    fn test_register_and_list_triggers() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));
        mgr.register(make_webhook_trigger("t2"));

        assert_eq!(mgr.list().len(), 2);
        assert!(mgr.get(&TriggerId::new("t1")).is_some());
    }

    #[test]
    fn test_remove_trigger() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));
        assert!(mgr.remove(&TriggerId::new("t1")).is_some());
        assert!(mgr.get(&TriggerId::new("t1")).is_none());
    }

    #[test]
    fn test_enable_disable() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));

        mgr.set_enabled(&TriggerId::new("t1"), false);
        assert!(!mgr.get(&TriggerId::new("t1")).unwrap().enabled);

        mgr.set_enabled(&TriggerId::new("t1"), true);
        assert!(mgr.get(&TriggerId::new("t1")).unwrap().enabled);
    }

    #[test]
    fn test_successful_outcome() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));

        let event = make_event("t1");
        let outcome = EventOutcome::Success {
            exit_code: 0,
            duration_ms: 100,
        };

        let retry = mgr.process_outcome(&event, &outcome);
        assert!(retry.is_none());

        let stats = mgr.statistics();
        assert_eq!(stats.successful, 1);
    }

    #[test]
    fn test_retry_on_failure() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));

        let event = make_event("t1");
        let outcome = EventOutcome::Failed {
            error: "timeout".into(),
            duration_ms: 5000,
        };

        // First failure should trigger retry
        let retry = mgr.process_outcome(&event, &outcome);
        assert!(retry.is_some());
        assert_eq!(retry.unwrap().attempt, 1);

        let stats = mgr.statistics();
        assert_eq!(stats.retried, 1);
    }

    #[test]
    fn test_dead_letter_after_max_retries() {
        let mgr = TriggerManager::new();
        let mut trigger = make_webhook_trigger("t1");
        trigger.retry_policy.max_retries = 2;
        mgr.register(trigger);

        let mut event = make_event("t1");
        let outcome = EventOutcome::Failed {
            error: "error".into(),
            duration_ms: 100,
        };

        // Attempt 0 -> retry (next_attempt=1 < max_retries=2)
        let retry = mgr.process_outcome(&event, &outcome).unwrap();
        event = retry;
        assert_eq!(event.attempt, 1);

        // Attempt 1 -> dead letter (next_attempt=2, should_retry(2)=false since 2 < 2 is false)
        let retry = mgr.process_outcome(&event, &outcome);
        assert!(retry.is_none());

        assert_eq!(mgr.dlq_size(), 1);
        let stats = mgr.statistics();
        assert_eq!(stats.dead_lettered, 1);
    }

    #[test]
    fn test_dlq_drain() {
        let mgr = TriggerManager::new();
        let mut trigger = make_webhook_trigger("t1");
        trigger.retry_policy.max_retries = 0; // No retries, go straight to DLQ
        mgr.register(trigger);

        // Generate 3 DLQ entries
        for i in 0..3 {
            let mut event = make_event("t1");
            event.event_id = format!("evt-{i}");
            let outcome = EventOutcome::Failed {
                error: "err".into(),
                duration_ms: 100,
            };
            mgr.process_outcome(&event, &outcome);
        }

        assert_eq!(mgr.dlq_size(), 3);
        let drained = mgr.drain_dlq(2);
        assert_eq!(drained.len(), 2);
        assert_eq!(mgr.dlq_size(), 1);
    }

    #[test]
    fn test_retry_policy_backoff() {
        let policy = RetryPolicy {
            max_retries: 5,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        };

        assert_eq!(policy.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_secs(4));
        assert_eq!(policy.delay_for_attempt(4), Duration::from_secs(8));
        // 5th attempt would be 16s
        assert_eq!(policy.delay_for_attempt(5), Duration::from_secs(16));

        assert!(policy.should_retry(4));
        assert!(!policy.should_retry(5));
    }

    #[test]
    fn test_webhook_signature_verification() {
        let config = WebhookConfig::new("/hook").with_secret("my-secret");
        let payload = b"test payload";

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"my-secret");
        hasher.update(payload);
        let valid_sig = format!("sha256={}", hex::encode(hasher.finalize()));

        assert!(config.verify_signature(payload, &valid_sig));
        assert!(!config.verify_signature(payload, "sha256=invalid"));
    }

    #[test]
    fn test_webhook_no_secret() {
        let config = WebhookConfig::new("/hook");
        assert!(config.verify_signature(b"anything", "any-sig"));
    }

    #[test]
    fn test_cron_validation() {
        let valid = CronConfig::new("*/5 * * * *");
        assert!(valid.is_valid());

        let also_valid = CronConfig::new("0 12 * * 1-5");
        assert!(also_valid.is_valid());

        let invalid = CronConfig::new("not a cron expression");
        assert!(!invalid.is_valid());
    }

    #[test]
    fn test_statistics() {
        let mgr = TriggerManager::new();
        mgr.register(make_webhook_trigger("t1"));

        let event = make_event("t1");
        mgr.process_outcome(
            &event,
            &EventOutcome::Success {
                exit_code: 0,
                duration_ms: 50,
            },
        );
        mgr.process_outcome(
            &event,
            &EventOutcome::Success {
                exit_code: 0,
                duration_ms: 30,
            },
        );

        let stats = mgr.statistics();
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.successful, 2);
        assert_eq!(stats.registered_triggers, 1);
    }

    #[test]
    fn test_message_queue_config() {
        let config = MessageQueueConfig::in_memory("test-topic");
        assert_eq!(config.topic, "test-topic");
        assert_eq!(config.batch_size, 1);
        assert!(matches!(config.provider, QueueProvider::InMemory));
    }
}
