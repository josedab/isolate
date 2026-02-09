//! Reference Plugin Implementations
//!
//! Five ready-to-use plugins that demonstrate the plugin system and provide
//! real utility:
//!
//! 1. **JsonValidatorPlugin** — Validates JSON payloads against configurable schemas
//! 2. **RateLimiterPlugin** — Token-bucket rate limiting per sandbox
//! 3. **ContentFilterPlugin** — Filters/sanitizes stdout content against deny-lists
//! 4. **StructuredLogPlugin** — Captures events and formats them as structured JSON logs
//! 5. **ResourceGuardPlugin** — Monitors resource usage and emits warnings

use super::{
    Event, EventType, ManifestBuilder, PluginError, PluginHandler, PluginManifest, PluginType,
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════════════
// 1. JSON Validator Plugin
// ═══════════════════════════════════════════════════════════════════

/// Validates JSON payloads against configurable constraints.
///
/// Config keys:
/// - `max_depth` (u64): Maximum JSON nesting depth (default: 10)
/// - `max_keys` (u64): Maximum number of keys in any object (default: 100)
/// - `required_fields` (array of strings): Fields that must be present at the top level
/// - `denied_fields` (array of strings): Fields that must NOT be present
pub struct JsonValidatorPlugin {
    max_depth: u64,
    max_keys: u64,
    required_fields: Vec<String>,
    denied_fields: Vec<String>,
    validations_performed: AtomicU64,
}

impl JsonValidatorPlugin {
    pub fn new() -> Self {
        Self {
            max_depth: 10,
            max_keys: 100,
            required_fields: Vec::new(),
            denied_fields: Vec::new(),
            validations_performed: AtomicU64::new(0),
        }
    }

    pub fn manifest() -> PluginManifest {
        ManifestBuilder::new("json-validator", "JSON Validator")
            .version("1.0.0")
            .plugin_type(PluginType::HostFunctions)
            .description("host_function:validate_json")
            .build()
    }

    /// Validates a JSON value against the configured constraints.
    pub fn validate(&self, value: &JsonValue) -> Result<(), String> {
        self.check_depth(value, 0)?;
        self.check_keys(value)?;

        if let Some(obj) = value.as_object() {
            for field in &self.required_fields {
                if !obj.contains_key(field) {
                    return Err(format!("Missing required field: {}", field));
                }
            }
            for field in &self.denied_fields {
                if obj.contains_key(field) {
                    return Err(format!("Denied field present: {}", field));
                }
            }
        }

        self.validations_performed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn validations_count(&self) -> u64 {
        self.validations_performed.load(Ordering::Relaxed)
    }

    fn check_depth(&self, value: &JsonValue, depth: u64) -> Result<(), String> {
        if depth > self.max_depth {
            return Err(format!("JSON depth {} exceeds maximum {}", depth, self.max_depth));
        }
        match value {
            JsonValue::Object(map) => {
                for v in map.values() {
                    self.check_depth(v, depth + 1)?;
                }
            }
            JsonValue::Array(arr) => {
                for v in arr {
                    self.check_depth(v, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn check_keys(&self, value: &JsonValue) -> Result<(), String> {
        if let JsonValue::Object(map) = value {
            if map.len() as u64 > self.max_keys {
                return Err(format!(
                    "Object has {} keys, exceeds maximum {}",
                    map.len(),
                    self.max_keys
                ));
            }
            for v in map.values() {
                self.check_keys(v)?;
            }
        }
        Ok(())
    }
}

impl Default for JsonValidatorPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHandler for JsonValidatorPlugin {
    fn init(&mut self, config: &HashMap<String, JsonValue>) -> Result<(), PluginError> {
        if let Some(d) = config.get("max_depth").and_then(|v| v.as_u64()) {
            self.max_depth = d;
        }
        if let Some(k) = config.get("max_keys").and_then(|v| v.as_u64()) {
            self.max_keys = k;
        }
        if let Some(arr) = config.get("required_fields").and_then(|v| v.as_array()) {
            self.required_fields =
                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
        if let Some(arr) = config.get("denied_fields").and_then(|v| v.as_array()) {
            self.denied_fields = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
        Ok(())
    }

    fn handle_event(&self, _event: &Event) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        params: &[JsonValue],
    ) -> Result<Vec<JsonValue>, PluginError> {
        match name {
            "validate_json" => {
                let input = params.first().ok_or(PluginError::FunctionNotFound(
                    "validate_json requires 1 param".into(),
                ))?;

                match self.validate(input) {
                    Ok(()) => Ok(vec![JsonValue::Bool(true)]),
                    Err(msg) => Ok(vec![JsonValue::Bool(false), JsonValue::String(msg)]),
                }
            }
            _ => Err(PluginError::FunctionNotFound(name.to_string())),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. Rate Limiter Plugin
// ═══════════════════════════════════════════════════════════════════

/// Token-bucket rate limiter that tracks invocations per sandbox.
///
/// Config keys:
/// - `max_tokens` (u64): Maximum tokens in bucket (default: 100)
/// - `refill_rate` (u64): Tokens added per second (default: 10)
pub struct RateLimiterPlugin {
    max_tokens: u64,
    refill_rate: u64,
    buckets: Mutex<HashMap<String, TokenBucket>>,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: f64, refill_rate: f64) -> Self {
        Self { tokens: max_tokens, max_tokens, refill_rate, last_refill: Instant::now() }
    }

    fn try_consume(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed().as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = Instant::now();
    }

    fn available_tokens(&mut self) -> f64 {
        self.refill();
        self.tokens
    }
}

impl RateLimiterPlugin {
    pub fn new() -> Self {
        Self { max_tokens: 100, refill_rate: 10, buckets: Mutex::new(HashMap::new()) }
    }

    pub fn manifest() -> PluginManifest {
        ManifestBuilder::new("rate-limiter", "Rate Limiter")
            .version("1.0.0")
            .plugin_type(PluginType::Middleware)
            .description("rate_limit")
            .build()
    }

    /// Checks if a request is allowed for the given sandbox ID.
    pub fn check_rate(&self, sandbox_id: &str) -> bool {
        let mut buckets = self.buckets.lock().expect("rate limiter buckets lock poisoned");
        let bucket = buckets
            .entry(sandbox_id.to_string())
            .or_insert_with(|| TokenBucket::new(self.max_tokens as f64, self.refill_rate as f64));
        bucket.try_consume()
    }

    /// Returns available tokens for a sandbox.
    pub fn available_tokens(&self, sandbox_id: &str) -> f64 {
        let mut buckets = self.buckets.lock().expect("rate limiter buckets lock poisoned");
        buckets.get_mut(sandbox_id).map(|b| b.available_tokens()).unwrap_or(self.max_tokens as f64)
    }

    /// Resets the rate limiter for a specific sandbox.
    pub fn reset(&self, sandbox_id: &str) {
        let mut buckets = self.buckets.lock().expect("rate limiter buckets lock poisoned");
        buckets.remove(sandbox_id);
    }
}

impl Default for RateLimiterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHandler for RateLimiterPlugin {
    fn init(&mut self, config: &HashMap<String, JsonValue>) -> Result<(), PluginError> {
        if let Some(t) = config.get("max_tokens").and_then(|v| v.as_u64()) {
            self.max_tokens = t;
        }
        if let Some(r) = config.get("refill_rate").and_then(|v| v.as_u64()) {
            self.refill_rate = r;
        }
        Ok(())
    }

    fn handle_event(&self, event: &Event) -> Result<(), PluginError> {
        // Clean up buckets when sandbox terminates
        if event.event_type == EventType::SandboxTerminated {
            if let Some(id) = &event.sandbox_id {
                self.reset(id);
            }
        }
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        params: &[JsonValue],
    ) -> Result<Vec<JsonValue>, PluginError> {
        match name {
            "check_rate" => {
                let sandbox_id = params.first().and_then(|v| v.as_str()).ok_or(
                    PluginError::FunctionNotFound("check_rate requires sandbox_id".into()),
                )?;
                Ok(vec![JsonValue::Bool(self.check_rate(sandbox_id))])
            }
            _ => Err(PluginError::FunctionNotFound(name.to_string())),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        self.buckets.lock().expect("rate limiter buckets lock poisoned").clear();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 3. Content Filter Plugin
// ═══════════════════════════════════════════════════════════════════

/// Filters sandbox output against configurable deny-lists.
///
/// Config keys:
/// - `deny_patterns` (array of strings): Substring patterns to block
/// - `replacement` (string): Replacement text (default: `[FILTERED]`)
pub struct ContentFilterPlugin {
    deny_patterns: Vec<String>,
    replacement: String,
    filtered_count: AtomicU64,
}

impl ContentFilterPlugin {
    pub fn new() -> Self {
        Self {
            deny_patterns: Vec::new(),
            replacement: "[FILTERED]".to_string(),
            filtered_count: AtomicU64::new(0),
        }
    }

    pub fn manifest() -> PluginManifest {
        ManifestBuilder::new("content-filter", "Content Filter")
            .version("1.0.0")
            .plugin_type(PluginType::Middleware)
            .description("content_filter")
            .build()
    }

    /// Filters content, replacing any denied patterns.
    pub fn filter(&self, content: &str) -> String {
        let mut result = content.to_string();
        for pattern in &self.deny_patterns {
            if result.contains(pattern.as_str()) {
                self.filtered_count.fetch_add(1, Ordering::Relaxed);
                result = result.replace(pattern.as_str(), &self.replacement);
            }
        }
        result
    }

    /// Checks whether content contains any denied patterns.
    pub fn contains_denied(&self, content: &str) -> bool {
        self.deny_patterns.iter().any(|p| content.contains(p.as_str()))
    }

    pub fn filtered_count(&self) -> u64 {
        self.filtered_count.load(Ordering::Relaxed)
    }
}

impl Default for ContentFilterPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHandler for ContentFilterPlugin {
    fn init(&mut self, config: &HashMap<String, JsonValue>) -> Result<(), PluginError> {
        if let Some(arr) = config.get("deny_patterns").and_then(|v| v.as_array()) {
            self.deny_patterns = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
        }
        if let Some(r) = config.get("replacement").and_then(|v| v.as_str()) {
            self.replacement = r.to_string();
        }
        Ok(())
    }

    fn handle_event(&self, _event: &Event) -> Result<(), PluginError> {
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        params: &[JsonValue],
    ) -> Result<Vec<JsonValue>, PluginError> {
        match name {
            "filter" => {
                let input = params
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or(PluginError::FunctionNotFound("filter requires string param".into()))?;
                Ok(vec![JsonValue::String(self.filter(input))])
            }
            "check" => {
                let input = params
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or(PluginError::FunctionNotFound("check requires string param".into()))?;
                Ok(vec![JsonValue::Bool(!self.contains_denied(input))])
            }
            _ => Err(PluginError::FunctionNotFound(name.to_string())),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. Structured Log Plugin
// ═══════════════════════════════════════════════════════════════════

/// Captures events and formats them as structured JSON log entries.
///
/// Config keys:
/// - `include_data` (bool): Include event data in logs (default: true)
/// - `max_entries` (u64): Maximum log entries to retain (default: 1000)
pub struct StructuredLogPlugin {
    include_data: bool,
    max_entries: usize,
    entries: Mutex<Vec<LogEntry>>,
}

/// A structured log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub event_type: String,
    pub sandbox_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<HashMap<String, JsonValue>>,
}

impl StructuredLogPlugin {
    pub fn new() -> Self {
        Self { include_data: true, max_entries: 1000, entries: Mutex::new(Vec::new()) }
    }

    pub fn manifest() -> PluginManifest {
        ManifestBuilder::new("structured-log", "Structured Logger")
            .version("1.0.0")
            .plugin_type(PluginType::EventHandler)
            .description("event_handler")
            .build()
    }

    /// Returns all captured log entries.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.lock().expect("structured log entries lock poisoned").clone()
    }

    /// Returns the number of entries.
    pub fn entry_count(&self) -> usize {
        self.entries.lock().expect("structured log entries lock poisoned").len()
    }

    /// Renders all entries as a newline-delimited JSON string.
    pub fn render_ndjson(&self) -> String {
        self.entries
            .lock()
            .expect("structured log entries lock poisoned")
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clears all entries.
    pub fn clear(&self) {
        self.entries.lock().expect("structured log entries lock poisoned").clear();
    }

    fn level_for_event(event_type: &EventType) -> &'static str {
        match event_type {
            EventType::SandboxFailed
            | EventType::ResourceLimitExceeded
            | EventType::CapabilityDenied => "ERROR",
            EventType::ResourceLimitWarning => "WARN",
            _ => "INFO",
        }
    }
}

impl Default for StructuredLogPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHandler for StructuredLogPlugin {
    fn init(&mut self, config: &HashMap<String, JsonValue>) -> Result<(), PluginError> {
        if let Some(b) = config.get("include_data").and_then(|v| v.as_bool()) {
            self.include_data = b;
        }
        if let Some(n) = config.get("max_entries").and_then(|v| v.as_u64()) {
            self.max_entries = n as usize;
        }
        Ok(())
    }

    fn handle_event(&self, event: &Event) -> Result<(), PluginError> {
        let entry = LogEntry {
            timestamp: format!("{:?}", event.timestamp),
            level: Self::level_for_event(&event.event_type).to_string(),
            event_type: format!("{:?}", event.event_type),
            sandbox_id: event.sandbox_id.clone(),
            data: if self.include_data { Some(event.data.clone()) } else { None },
        };

        let mut entries = self.entries.lock().expect("structured log entries lock poisoned");
        if entries.len() >= self.max_entries {
            entries.remove(0);
        }
        entries.push(entry);
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        _params: &[JsonValue],
    ) -> Result<Vec<JsonValue>, PluginError> {
        match name {
            "get_logs" => {
                let entries = self.entries();
                let json = serde_json::to_value(&entries).unwrap_or(JsonValue::Null);
                Ok(vec![json])
            }
            "count" => Ok(vec![JsonValue::Number(serde_json::Number::from(self.entry_count()))]),
            _ => Err(PluginError::FunctionNotFound(name.to_string())),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        self.clear();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 5. Resource Guard Plugin
// ═══════════════════════════════════════════════════════════════════

/// Monitors resource events and emits warnings when thresholds are reached.
///
/// Config keys:
/// - `memory_warn_pct` (u64): Warn when memory usage exceeds this percentage (default: 80)
/// - `fuel_warn_pct` (u64): Warn when fuel consumption exceeds this percentage (default: 90)
pub struct ResourceGuardPlugin {
    memory_warn_pct: u64,
    fuel_warn_pct: u64,
    warnings: Mutex<Vec<ResourceWarning>>,
}

/// A resource warning emitted by the guard.
#[derive(Debug, Clone)]
pub struct ResourceWarning {
    pub sandbox_id: String,
    pub category: String,
    pub message: String,
    pub timestamp: Instant,
}

impl ResourceGuardPlugin {
    pub fn new() -> Self {
        Self { memory_warn_pct: 80, fuel_warn_pct: 90, warnings: Mutex::new(Vec::new()) }
    }

    pub fn manifest() -> PluginManifest {
        ManifestBuilder::new("resource-guard", "Resource Guard")
            .version("1.0.0")
            .plugin_type(PluginType::EventHandler)
            .description("resource_monitor")
            .build()
    }

    /// Returns all warnings.
    pub fn warnings(&self) -> Vec<ResourceWarning> {
        self.warnings.lock().expect("resource guard warnings lock poisoned").clone()
    }

    /// Returns warning count.
    pub fn warning_count(&self) -> usize {
        self.warnings.lock().expect("resource guard warnings lock poisoned").len()
    }

    /// Checks resource usage and returns a warning if thresholds are exceeded.
    pub fn check_usage(
        &self,
        sandbox_id: &str,
        memory_pct: u64,
        fuel_pct: u64,
    ) -> Vec<ResourceWarning> {
        let mut warnings = Vec::new();

        if memory_pct >= self.memory_warn_pct {
            warnings.push(ResourceWarning {
                sandbox_id: sandbox_id.to_string(),
                category: "memory".to_string(),
                message: format!(
                    "Memory usage at {}% (threshold: {}%)",
                    memory_pct, self.memory_warn_pct
                ),
                timestamp: Instant::now(),
            });
        }

        if fuel_pct >= self.fuel_warn_pct {
            warnings.push(ResourceWarning {
                sandbox_id: sandbox_id.to_string(),
                category: "fuel".to_string(),
                message: format!(
                    "Fuel consumption at {}% (threshold: {}%)",
                    fuel_pct, self.fuel_warn_pct
                ),
                timestamp: Instant::now(),
            });
        }

        let mut all_warnings = self.warnings.lock().expect("resource guard warnings lock poisoned");
        all_warnings.extend(warnings.clone());

        warnings
    }

    /// Clears all warnings.
    pub fn clear(&self) {
        self.warnings.lock().expect("resource guard warnings lock poisoned").clear();
    }
}

impl Default for ResourceGuardPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginHandler for ResourceGuardPlugin {
    fn init(&mut self, config: &HashMap<String, JsonValue>) -> Result<(), PluginError> {
        if let Some(m) = config.get("memory_warn_pct").and_then(|v| v.as_u64()) {
            self.memory_warn_pct = m;
        }
        if let Some(f) = config.get("fuel_warn_pct").and_then(|v| v.as_u64()) {
            self.fuel_warn_pct = f;
        }
        Ok(())
    }

    fn handle_event(&self, event: &Event) -> Result<(), PluginError> {
        if event.event_type == EventType::ResourceLimitWarning {
            if let Some(sandbox_id) = &event.sandbox_id {
                let mem_pct = event.data.get("memory_pct").and_then(|v| v.as_u64()).unwrap_or(0);
                let fuel_pct = event.data.get("fuel_pct").and_then(|v| v.as_u64()).unwrap_or(0);
                self.check_usage(sandbox_id, mem_pct, fuel_pct);
            }
        }
        Ok(())
    }

    fn invoke_host_function(
        &self,
        name: &str,
        _params: &[JsonValue],
    ) -> Result<Vec<JsonValue>, PluginError> {
        match name {
            "warning_count" => {
                Ok(vec![JsonValue::Number(serde_json::Number::from(self.warning_count()))])
            }
            _ => Err(PluginError::FunctionNotFound(name.to_string())),
        }
    }

    fn shutdown(&mut self) -> Result<(), PluginError> {
        self.clear();
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // -- JSON Validator --

    #[test]
    fn test_json_validator_valid() {
        let v = JsonValidatorPlugin::new();
        let json: JsonValue = serde_json::json!({"name": "test", "value": 42});
        assert!(v.validate(&json).is_ok());
    }

    #[test]
    fn test_json_validator_depth_exceeded() {
        let mut v = JsonValidatorPlugin::new();
        v.max_depth = 2;
        let json: JsonValue = serde_json::json!({"a": {"b": {"c": "too deep"}}});
        assert!(v.validate(&json).is_err());
    }

    #[test]
    fn test_json_validator_too_many_keys() {
        let mut v = JsonValidatorPlugin::new();
        v.max_keys = 2;
        let json: JsonValue = serde_json::json!({"a": 1, "b": 2, "c": 3});
        assert!(v.validate(&json).is_err());
    }

    #[test]
    fn test_json_validator_required_fields() {
        let mut v = JsonValidatorPlugin::new();
        v.required_fields = vec!["name".to_string()];
        let ok: JsonValue = serde_json::json!({"name": "test"});
        let bad: JsonValue = serde_json::json!({"value": 42});
        assert!(v.validate(&ok).is_ok());
        assert!(v.validate(&bad).is_err());
    }

    #[test]
    fn test_json_validator_denied_fields() {
        let mut v = JsonValidatorPlugin::new();
        v.denied_fields = vec!["password".to_string()];
        let bad: JsonValue = serde_json::json!({"password": "secret"});
        assert!(v.validate(&bad).is_err());
    }

    #[test]
    fn test_json_validator_host_function() {
        let v = JsonValidatorPlugin::new();
        let input = serde_json::json!({"key": "value"});
        let result = v.invoke_host_function("validate_json", &[input]).unwrap();
        assert_eq!(result[0], JsonValue::Bool(true));
    }

    #[test]
    fn test_json_validator_init_config() {
        let mut v = JsonValidatorPlugin::new();
        let mut config = HashMap::new();
        config.insert("max_depth".to_string(), serde_json::json!(5));
        config.insert("required_fields".to_string(), serde_json::json!(["name"]));
        v.init(&config).unwrap();
        assert_eq!(v.max_depth, 5);
        assert_eq!(v.required_fields, vec!["name"]);
    }

    // -- Rate Limiter --

    #[test]
    fn test_rate_limiter_allows() {
        let rl = RateLimiterPlugin::new();
        assert!(rl.check_rate("sandbox-1"));
    }

    #[test]
    fn test_rate_limiter_exhaustion() {
        let mut rl = RateLimiterPlugin::new();
        rl.max_tokens = 3;
        rl.refill_rate = 0; // No refill

        assert!(rl.check_rate("sandbox-1"));
        assert!(rl.check_rate("sandbox-1"));
        assert!(rl.check_rate("sandbox-1"));
        assert!(!rl.check_rate("sandbox-1")); // Exhausted
    }

    #[test]
    fn test_rate_limiter_separate_buckets() {
        let mut rl = RateLimiterPlugin::new();
        rl.max_tokens = 1;
        rl.refill_rate = 0;

        assert!(rl.check_rate("sandbox-1"));
        assert!(rl.check_rate("sandbox-2")); // Separate bucket
        assert!(!rl.check_rate("sandbox-1")); // Exhausted
    }

    #[test]
    fn test_rate_limiter_reset() {
        let mut rl = RateLimiterPlugin::new();
        rl.max_tokens = 1;
        rl.refill_rate = 0;

        assert!(rl.check_rate("sandbox-1"));
        assert!(!rl.check_rate("sandbox-1"));

        rl.reset("sandbox-1");
        assert!(rl.check_rate("sandbox-1")); // New bucket
    }

    // -- Content Filter --

    #[test]
    fn test_content_filter_no_patterns() {
        let f = ContentFilterPlugin::new();
        assert_eq!(f.filter("hello world"), "hello world");
    }

    #[test]
    fn test_content_filter_replaces_pattern() {
        let mut f = ContentFilterPlugin::new();
        f.deny_patterns = vec!["secret".to_string()];
        assert_eq!(f.filter("my secret data"), "my [FILTERED] data");
    }

    #[test]
    fn test_content_filter_custom_replacement() {
        let mut f = ContentFilterPlugin::new();
        f.deny_patterns = vec!["bad".to_string()];
        f.replacement = "***".to_string();
        assert_eq!(f.filter("bad word"), "*** word");
    }

    #[test]
    fn test_content_filter_contains_denied() {
        let mut f = ContentFilterPlugin::new();
        f.deny_patterns = vec!["password".to_string()];
        assert!(f.contains_denied("my password here"));
        assert!(!f.contains_denied("clean content"));
    }

    #[test]
    fn test_content_filter_host_function() {
        let mut f = ContentFilterPlugin::new();
        f.deny_patterns = vec!["secret".to_string()];
        let result = f
            .invoke_host_function("filter", &[JsonValue::String("my secret".to_string())])
            .unwrap();
        assert_eq!(result[0], JsonValue::String("my [FILTERED]".to_string()));
    }

    // -- Structured Log --

    #[test]
    fn test_structured_log_captures_events() {
        let log = StructuredLogPlugin::new();
        let event = Event {
            event_type: EventType::SandboxCreated,
            sandbox_id: Some("sb-1".to_string()),
            timestamp: std::time::SystemTime::now(),
            data: HashMap::new(),
        };
        log.handle_event(&event).unwrap();
        assert_eq!(log.entry_count(), 1);
    }

    #[test]
    fn test_structured_log_level_assignment() {
        assert_eq!(StructuredLogPlugin::level_for_event(&EventType::SandboxCreated), "INFO");
        assert_eq!(StructuredLogPlugin::level_for_event(&EventType::SandboxFailed), "ERROR");
        assert_eq!(StructuredLogPlugin::level_for_event(&EventType::ResourceLimitWarning), "WARN");
    }

    #[test]
    fn test_structured_log_render_ndjson() {
        let log = StructuredLogPlugin::new();
        let event = Event {
            event_type: EventType::SandboxStarted,
            sandbox_id: Some("sb-1".to_string()),
            timestamp: std::time::SystemTime::now(),
            data: HashMap::new(),
        };
        log.handle_event(&event).unwrap();

        let ndjson = log.render_ndjson();
        assert!(!ndjson.is_empty());
        assert!(ndjson.contains("SandboxStarted"));
    }

    #[test]
    fn test_structured_log_max_entries() {
        let mut log = StructuredLogPlugin::new();
        log.max_entries = 2;

        for _ in 0..5 {
            let event = Event {
                event_type: EventType::SandboxCreated,
                sandbox_id: None,
                timestamp: std::time::SystemTime::now(),
                data: HashMap::new(),
            };
            log.handle_event(&event).unwrap();
        }

        assert_eq!(log.entry_count(), 2);
    }

    // -- Resource Guard --

    #[test]
    fn test_resource_guard_no_warning() {
        let guard = ResourceGuardPlugin::new();
        let warnings = guard.check_usage("sb-1", 50, 50);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_resource_guard_memory_warning() {
        let guard = ResourceGuardPlugin::new(); // default 80%
        let warnings = guard.check_usage("sb-1", 85, 50);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].category, "memory");
    }

    #[test]
    fn test_resource_guard_fuel_warning() {
        let guard = ResourceGuardPlugin::new(); // default 90%
        let warnings = guard.check_usage("sb-1", 50, 95);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].category, "fuel");
    }

    #[test]
    fn test_resource_guard_both_warnings() {
        let guard = ResourceGuardPlugin::new();
        let warnings = guard.check_usage("sb-1", 85, 95);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn test_resource_guard_accumulates() {
        let guard = ResourceGuardPlugin::new();
        guard.check_usage("sb-1", 85, 50);
        guard.check_usage("sb-2", 85, 95);
        assert_eq!(guard.warning_count(), 3);

        guard.clear();
        assert_eq!(guard.warning_count(), 0);
    }

    #[test]
    fn test_resource_guard_custom_thresholds() {
        let mut guard = ResourceGuardPlugin::new();
        let mut config = HashMap::new();
        config.insert("memory_warn_pct".to_string(), serde_json::json!(50));
        config.insert("fuel_warn_pct".to_string(), serde_json::json!(60));
        guard.init(&config).unwrap();

        let warnings = guard.check_usage("sb-1", 55, 65);
        assert_eq!(warnings.len(), 2);
    }

    // -- Manifests --

    #[test]
    fn test_all_manifests() {
        let manifests = vec![
            JsonValidatorPlugin::manifest(),
            RateLimiterPlugin::manifest(),
            ContentFilterPlugin::manifest(),
            StructuredLogPlugin::manifest(),
            ResourceGuardPlugin::manifest(),
        ];

        for m in &manifests {
            assert!(!m.name.is_empty());
            assert!(!m.version.is_empty());
        }

        let names: Vec<&str> = manifests.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"JSON Validator"));
        assert!(names.contains(&"Rate Limiter"));
        assert!(names.contains(&"Content Filter"));
        assert!(names.contains(&"Structured Logger"));
        assert!(names.contains(&"Resource Guard"));
    }
}
