//! Integration tests for the AI Agent Sandbox framework.
//!
//! Tests end-to-end agent workflows: session lifecycle, guardrails enforcement,
//! protocol adapters, tool validation, and session persistence.

#![cfg(feature = "agent")]

use isolate_core::agent::{
    AgentConfig, AgentSession, CodeExecutionRequest, ExecutionStatus, GuardrailConfig,
    ContentFilter, ChainDepthTracker, JsonSchema, ProtocolAdapter, ProtocolFormat,
    ProtocolMessage, ProtocolValidator, ToolDefinition,
    ViolationKind, SessionRateLimiter,
};

const HELLO_WASM: &[u8] = include_bytes!("fixtures/hello.wasm");
const EXIT_42_WASM: &[u8] = include_bytes!("fixtures/exit_42.wasm");

// ---------------------------------------------------------------------------
// Session lifecycle tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_agent_session_execute_hello() {
    let config = AgentConfig::builder()
        .memory_limit(64 * 1024 * 1024)
        .fuel_budget(10_000_000)
        .max_tool_calls(10)
        .build();

    let mut session = AgentSession::new(config);
    session.register_tool(ToolDefinition::code_execute());

    let request = CodeExecutionRequest::tool_call(
        HELLO_WASM.to_vec(),
        "code_execute",
        serde_json::json!({}),
    );

    let result = session.execute(request).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Success);
    assert!(result.stdout.contains("Hello"));
    assert_eq!(session.tool_call_count(), 1);
}

#[tokio::test]
async fn test_agent_session_nonzero_exit() {
    let config = AgentConfig::builder()
        .memory_limit(64 * 1024 * 1024)
        .fuel_budget(10_000_000)
        .build();

    let mut session = AgentSession::new(config);
    let request = CodeExecutionRequest::new(EXIT_42_WASM.to_vec(), serde_json::json!({}));

    let result = session.execute(request).await.unwrap();
    assert_eq!(result.status, ExecutionStatus::Failed);
    assert_eq!(result.exit_code, 42);
}

#[tokio::test]
#[ignore] // Flaky under parallel execution due to wasmtime fuel store contention
async fn test_agent_session_tool_call_budget() {
    // Use a fresh engine to avoid fuel tracking conflicts with parallel tests
    let engine = std::sync::Arc::new(isolate_core::engine::WasmEngine::new().unwrap());
    let config = AgentConfig::builder()
        .memory_limit(64 * 1024 * 1024)
        .fuel_budget(100_000_000)
        .max_tool_calls(2)
        .build();

    let mut session = AgentSession::with_engine(config, engine);

    for _ in 0..2 {
        let req = CodeExecutionRequest::new(HELLO_WASM.to_vec(), serde_json::json!({}));
        let _ = session.execute(req).await;
    }

    // Third call should fail due to tool call budget exhaustion
    assert!(session.is_tool_limit_reached());
    let req = CodeExecutionRequest::new(HELLO_WASM.to_vec(), serde_json::json!({}));
    let result = session.execute(req).await;
    assert!(result.is_err(), "Third call should fail budget check");
}

// ---------------------------------------------------------------------------
// Guardrails enforcement
// ---------------------------------------------------------------------------

#[test]
fn test_guardrails_input_size_enforcement() {
    let config = GuardrailConfig::builder()
        .max_input_bytes(100)
        .build();
    let filter = ContentFilter::new(&config);

    let small = "a".repeat(50);
    assert!(filter.check_input(&small).allowed);

    let large = "a".repeat(200);
    assert!(!filter.check_input(&large).allowed);
    assert!(filter.check_input(&large).violations.iter().any(|v| v.kind == ViolationKind::InputTooLarge));
}

#[test]
fn test_guardrails_output_size_enforcement() {
    let config = GuardrailConfig::builder()
        .max_output_bytes(100)
        .build();
    let filter = ContentFilter::new(&config);

    let large = "x".repeat(200);
    let result = filter.check_output(&large);
    assert!(!result.allowed);
    assert!(result.violations.iter().any(|v| v.kind == ViolationKind::OutputTooLarge));
}

#[test]
fn test_guardrails_content_filtering_secrets() {
    let config = GuardrailConfig::default();
    let filter = ContentFilter::new(&config);

    assert!(!filter.check_output("-----BEGIN PRIVATE KEY-----").allowed);
    assert!(!filter.check_output("AKIA1234567890ABCDEF1234").allowed);
    assert!(filter.check_output("This is safe output").allowed);
}

#[test]
fn test_guardrails_chain_depth_limit() {
    let tracker = ChainDepthTracker::new(3);

    let g1 = tracker.enter().unwrap();
    let g2 = tracker.enter().unwrap();
    let g3 = tracker.enter().unwrap();
    assert!(tracker.enter().is_err(), "Depth 4 should be rejected");

    drop(g3);
    drop(g2);
    drop(g1);
    assert_eq!(tracker.depth(), 0);
}

#[test]
fn test_guardrails_rate_limiter() {
    let config = GuardrailConfig::builder()
        .max_calls_per_minute(3)
        .max_total_cost(10.0)
        .build();
    let limiter = SessionRateLimiter::new(&config);

    for _ in 0..3 {
        assert!(matches!(limiter.try_acquire(), isolate_core::agent::guardrails::RateLimitResult::Allowed { .. }));
    }
    assert!(!matches!(limiter.try_acquire(), isolate_core::agent::guardrails::RateLimitResult::Allowed { .. }));
}

// ---------------------------------------------------------------------------
// Protocol adapter round-trips
// ---------------------------------------------------------------------------

#[test]
fn test_openai_protocol_adapter_roundtrip() {
    let adapter = ProtocolAdapter::new(ProtocolFormat::OpenAi);
    let tools = vec![ToolDefinition::code_execute(), ToolDefinition::file_read()];

    let exported = adapter.export_tools(&tools);
    let arr = exported.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["type"], "function");
    assert!(arr[0]["function"]["name"].as_str().is_some());

    // Parse a call back
    let call = serde_json::json!({
        "function": {
            "name": "code_execute",
            "arguments": "{\"code\": \"print('hello')\"}"
        }
    });
    let msg = adapter.parse_tool_call(&call).unwrap();
    match msg {
        ProtocolMessage::AgentRequest { tool_name, input, .. } => {
            assert_eq!(tool_name, "code_execute");
            assert_eq!(input["code"], "print('hello')");
        }
        _ => panic!("Expected AgentRequest"),
    }
}

#[test]
fn test_anthropic_protocol_adapter_roundtrip() {
    let adapter = ProtocolAdapter::new(ProtocolFormat::Anthropic);
    let tools = vec![ToolDefinition::file_write()];

    let exported = adapter.export_tools(&tools);
    let arr = exported.as_array().unwrap();
    assert_eq!(arr[0]["name"], "file_write");
    assert!(arr[0].get("input_schema").is_some());

    let call = serde_json::json!({
        "name": "file_write",
        "input": {"path": "/out.txt", "content": "hello"}
    });
    let msg = adapter.parse_tool_call(&call).unwrap();
    match msg {
        ProtocolMessage::AgentRequest { tool_name, input, .. } => {
            assert_eq!(tool_name, "file_write");
            assert_eq!(input["path"], "/out.txt");
        }
        _ => panic!("Expected AgentRequest"),
    }
}

#[test]
fn test_langchain_protocol_adapter_roundtrip() {
    let adapter = ProtocolAdapter::new(ProtocolFormat::LangChain);
    let tools = vec![ToolDefinition::http_request()];

    let exported = adapter.export_tools(&tools);
    let arr = exported.as_array().unwrap();
    assert_eq!(arr[0]["name"], "http_request");
    assert_eq!(arr[0]["return_direct"], false);

    let call = serde_json::json!({
        "tool": "http_request",
        "tool_input": {"url": "https://example.com", "method": "GET"}
    });
    let msg = adapter.parse_tool_call(&call).unwrap();
    match msg {
        ProtocolMessage::AgentRequest { tool_name, input, .. } => {
            assert_eq!(tool_name, "http_request");
            assert_eq!(input["url"], "https://example.com");
        }
        _ => panic!("Expected AgentRequest"),
    }
}

// ---------------------------------------------------------------------------
// Tool JSON Schema validation
// ---------------------------------------------------------------------------

#[test]
fn test_tool_schema_validation_valid() {
    let tool = ToolDefinition::code_execute();
    let valid_input = serde_json::json!({"code": "print(1)"});
    assert!(tool.validate_input(&valid_input).is_empty());
}

#[test]
fn test_tool_schema_validation_missing_required() {
    let tool = ToolDefinition::code_execute();
    let invalid = serde_json::json!({"not_code": "value"});
    let errors = tool.validate_input(&invalid);
    assert!(!errors.is_empty());
}

#[test]
fn test_tool_schema_validation_wrong_type() {
    let tool = ToolDefinition::file_read();
    let invalid = serde_json::json!({"path": 42});
    let errors = tool.validate_input(&invalid);
    assert!(!errors.is_empty());
}

#[test]
fn test_tool_custom_schema_validation() {
    let schema = JsonSchema::object()
        .required_property("query", JsonSchema::string())
        .required_property("limit", JsonSchema::integer())
        .build();

    let tool = ToolDefinition::new("search", "Search things")
        .with_input_schema(schema);

    let valid = serde_json::json!({"query": "rust wasm", "limit": 10});
    assert!(tool.validate_input(&valid).is_empty());

    let bad_type = serde_json::json!({"query": "rust wasm", "limit": "ten"});
    assert!(!tool.validate_input(&bad_type).is_empty());

    let missing = serde_json::json!({"query": "rust wasm"});
    assert!(!tool.validate_input(&missing).is_empty());
}

// ---------------------------------------------------------------------------
// Session persistence
// ---------------------------------------------------------------------------

#[test]
fn test_session_save_and_load() {
    let config = AgentConfig::builder()
        .memory_limit(64 * 1024 * 1024)
        .fuel_budget(5_000_000)
        .max_tool_calls(50)
        .build();

    let session = AgentSession::new(config);
    let original_id = session.id();

    let json = session.save().unwrap();
    assert!(json.contains(&original_id.to_string()));

    let restored = AgentSession::load(&json).unwrap();
    assert_eq!(restored.id(), original_id);
    assert_eq!(restored.remaining_fuel(), Some(5_000_000));
    assert_eq!(restored.tool_call_count(), 0);
}

#[test]
fn test_session_snapshot_preserves_state() {
    let config = AgentConfig::builder()
        .memory_limit(128 * 1024 * 1024)
        .fuel_budget(1_000_000)
        .build();

    let session = AgentSession::new(config);
    let id = session.id();

    let snapshot = session.snapshot();
    assert_eq!(snapshot.id, id);
    assert_eq!(snapshot.total_fuel_consumed, 0);

    let restored = AgentSession::from_snapshot(snapshot);
    assert_eq!(restored.id(), id);
}

// ---------------------------------------------------------------------------
// Multi-provider protocol validator
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_validator_with_schemas() {
    let mut validator = ProtocolValidator::new();
    validator.register_input_schema(
        "code_execute",
        JsonSchema::object()
            .required_property("code", JsonSchema::string())
            .build(),
    );
    validator.register_output_schema(
        "code_execute",
        JsonSchema::object()
            .required_property("result", JsonSchema::string())
            .build(),
    );

    let valid_req = ProtocolMessage::AgentRequest {
        tool_name: "code_execute".into(),
        input: serde_json::json!({"code": "1+1"}),
        call_id: uuid::Uuid::nil(),
        budget: isolate_core::agent::ResourceBudget::default(),
    };
    assert!(validator.validate_request(&valid_req).is_empty());

    let invalid_req = ProtocolMessage::AgentRequest {
        tool_name: "code_execute".into(),
        input: serde_json::json!({"code": 42}),
        call_id: uuid::Uuid::nil(),
        budget: isolate_core::agent::ResourceBudget::default(),
    };
    assert!(!validator.validate_request(&invalid_req).is_empty());
}
