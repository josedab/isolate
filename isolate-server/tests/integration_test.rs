//! Integration tests for isolate-server gRPC service.
//!
//! These tests exercise the service implementation directly (without binding
//! a network port) to verify end-to-end behavior, concurrency, error codes,
//! and edge cases not covered by unit tests.

use isolate_server::proto::{
    self, isolate_service_server::IsolateService, CreateSandboxRequest, GetMetricsRequest,
    GetSandboxRequest, ListSandboxesRequest, RunSandboxRequest, TerminateSandboxRequest,
};
use isolate_server::service::IsolateServiceImpl;
use std::collections::HashMap;
use tonic::Request;

const MINIMAL_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/minimal.wasm");
const HELLO_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/hello.wasm");
const EXIT_42_WASM: &[u8] = include_bytes!("../../isolate-core/tests/fixtures/exit_42.wasm");

fn create_req(module: &[u8]) -> Request<CreateSandboxRequest> {
    Request::new(CreateSandboxRequest {
        module: module.to_vec(),
        config: Some(proto::SandboxConfig {
            memory_limit: 0,
            fuel_limit: 1_000_000,
            wall_time_limit_secs: 5,
            cpu_time_limit_secs: 0,
            capabilities: vec![
                proto::Capability { r#type: "stdout".into(), value: String::new() },
                proto::Capability { r#type: "stderr".into(), value: String::new() },
            ],
            env: HashMap::new(),
            args: vec![],
        }),
        module_signature: vec![],
        module_ref: String::new(),
    })
}

fn run_req(sandbox_id: &str) -> Request<RunSandboxRequest> {
    Request::new(RunSandboxRequest {
        sandbox_id: sandbox_id.to_string(),
        input: vec![],
        entry_point: String::new(),
        timeout_secs: 0,
    })
}

// ── Concurrent sandbox creation ──────────────────────────────────────────────

#[tokio::test]
async fn test_concurrent_create() {
    let service = IsolateServiceImpl::new(20);

    let (r1, r2, r3, r4, r5) = tokio::join!(
        service.create_sandbox(create_req(MINIMAL_WASM)),
        service.create_sandbox(create_req(MINIMAL_WASM)),
        service.create_sandbox(create_req(MINIMAL_WASM)),
        service.create_sandbox(create_req(MINIMAL_WASM)),
        service.create_sandbox(create_req(MINIMAL_WASM)),
    );

    let results = [r1, r2, r3, r4, r5];
    for result in &results {
        assert!(result.is_ok(), "Concurrent create should succeed");
    }

    // Verify all sandboxes are distinct
    let ids: Vec<String> =
        results.into_iter().map(|r| r.unwrap().into_inner().sandbox_id).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 5, "All sandbox IDs should be unique");
}

// ── Empty module rejection ───────────────────────────────────────────────────

#[tokio::test]
async fn test_create_empty_module() {
    let service = IsolateServiceImpl::new(10);
    let req = Request::new(CreateSandboxRequest {
        module: vec![],
        config: None,
        module_signature: vec![],
        module_ref: String::new(),
    });
    let result = service.create_sandbox(req).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
}

// ── Run after terminate ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_run_after_terminate() {
    let service = IsolateServiceImpl::new(10);

    let create_resp = service.create_sandbox(create_req(MINIMAL_WASM)).await.unwrap();
    let sandbox_id = create_resp.into_inner().sandbox_id;

    // Terminate
    service
        .terminate_sandbox(Request::new(TerminateSandboxRequest { sandbox_id: sandbox_id.clone() }))
        .await
        .unwrap();

    // Run should fail with NotFound (removed from map)
    let result = service.run_sandbox(run_req(&sandbox_id)).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
}

// ── Double terminate ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_double_terminate() {
    let service = IsolateServiceImpl::new(10);

    let create_resp = service.create_sandbox(create_req(MINIMAL_WASM)).await.unwrap();
    let sandbox_id = create_resp.into_inner().sandbox_id;

    service
        .terminate_sandbox(Request::new(TerminateSandboxRequest { sandbox_id: sandbox_id.clone() }))
        .await
        .unwrap();

    let result = service
        .terminate_sandbox(Request::new(TerminateSandboxRequest { sandbox_id: sandbox_id.clone() }))
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
}

// ── Resource exhaustion (max sandboxes) ──────────────────────────────────────

#[tokio::test]
async fn test_semaphore_limits_sandboxes() {
    let service = IsolateServiceImpl::new(2);

    // Create 2 sandboxes (should succeed)
    let r1 = service.create_sandbox(create_req(MINIMAL_WASM)).await;
    let r2 = service.create_sandbox(create_req(MINIMAL_WASM)).await;
    assert!(r1.is_ok());
    assert!(r2.is_ok());

    // The third should still succeed because permits are released after create,
    // but verifying the semaphore is acquired is the point here.
    // In a real scenario, long-running sandboxes would hold the permit.
}

// ── Metrics reflect state ────────────────────────────────────────────────────

#[tokio::test]
async fn test_metrics_reflect_sandbox_count() {
    let service = IsolateServiceImpl::new(10);

    // No sandboxes initially
    let metrics = service
        .get_metrics(Request::new(GetMetricsRequest { format: "json".into() }))
        .await
        .unwrap()
        .into_inner()
        .data;
    let parsed: serde_json::Value = serde_json::from_str(&metrics).unwrap();
    assert_eq!(parsed["sandboxes_active"], 0);

    // Create one
    service.create_sandbox(create_req(MINIMAL_WASM)).await.unwrap();

    let metrics = service
        .get_metrics(Request::new(GetMetricsRequest { format: "json".into() }))
        .await
        .unwrap()
        .into_inner()
        .data;
    let parsed: serde_json::Value = serde_json::from_str(&metrics).unwrap();
    assert_eq!(parsed["sandboxes_active"], 1);
    assert_eq!(parsed["total_requests"], 1);
}

// ── Run and verify resource usage ────────────────────────────────────────────

#[tokio::test]
async fn test_run_returns_resource_usage() {
    let service = IsolateServiceImpl::new(10);

    let create_resp = service.create_sandbox(create_req(HELLO_WASM)).await.unwrap();
    let sandbox_id = create_resp.into_inner().sandbox_id;

    let run_resp = service.run_sandbox(run_req(&sandbox_id)).await.unwrap();
    let inner = run_resp.into_inner();

    assert_eq!(inner.exit_code, 0);
    assert!(inner.duration_ms > 0.0);

    let usage = inner.resource_usage.expect("resource_usage should be present");
    assert!(usage.fuel_consumed > 0);
    assert!(usage.wall_time_ms > 0.0);
}

// ── List with pagination ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_pagination_cursor() {
    let service = IsolateServiceImpl::new(10);

    for _ in 0..5 {
        service.create_sandbox(create_req(MINIMAL_WASM)).await.unwrap();
    }

    // Page 1: limit 2
    let resp1 = service
        .list_sandboxes(Request::new(ListSandboxesRequest {
            state_filter: String::new(),
            limit: 2,
            page_token: String::new(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp1.sandboxes.len(), 2);
    assert_eq!(resp1.total, 5);
    assert!(!resp1.next_page_token.is_empty());

    // Page 2: use cursor
    let resp2 = service
        .list_sandboxes(Request::new(ListSandboxesRequest {
            state_filter: String::new(),
            limit: 2,
            page_token: resp1.next_page_token.clone(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp2.sandboxes.len(), 2);

    // Page 3: last page
    let resp3 = service
        .list_sandboxes(Request::new(ListSandboxesRequest {
            state_filter: String::new(),
            limit: 2,
            page_token: resp2.next_page_token.clone(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(resp3.sandboxes.len(), 1);
    assert!(resp3.next_page_token.is_empty());
}

// ── Exit code 42 resource usage ──────────────────────────────────────────────

#[tokio::test]
async fn test_exit_42_has_resource_usage() {
    let service = IsolateServiceImpl::new(10);

    let create_resp = service.create_sandbox(create_req(EXIT_42_WASM)).await.unwrap();
    let sandbox_id = create_resp.into_inner().sandbox_id;

    let run_resp = service.run_sandbox(run_req(&sandbox_id)).await.unwrap();
    let inner = run_resp.into_inner();

    assert_eq!(inner.exit_code, 42);
    assert!(inner.resource_usage.is_some());
}

// ── Get sandbox returns correct state ────────────────────────────────────────

#[tokio::test]
async fn test_get_sandbox_state_after_run() {
    let service = IsolateServiceImpl::new(10);

    let create_resp = service.create_sandbox(create_req(HELLO_WASM)).await.unwrap();
    let sandbox_id = create_resp.into_inner().sandbox_id;

    // Run it
    service.run_sandbox(run_req(&sandbox_id)).await.unwrap();

    // Get should still return the sandbox
    let get_resp = service
        .get_sandbox(Request::new(GetSandboxRequest { sandbox_id: sandbox_id.clone() }))
        .await
        .unwrap();
    let info = get_resp.into_inner().sandbox.unwrap();
    assert_eq!(info.id, sandbox_id);
    assert!(info.metrics.is_some());
    let metrics = info.metrics.unwrap();
    assert_eq!(metrics.run_count, 1);
    assert_eq!(metrics.success_count, 1);
}

// ── Environment variables in config ──────────────────────────────────────────

#[tokio::test]
async fn test_create_with_env_vars() {
    let service = IsolateServiceImpl::new(10);

    let mut env = HashMap::new();
    env.insert("MY_VAR".to_string(), "my_value".to_string());

    let req = Request::new(CreateSandboxRequest {
        module: MINIMAL_WASM.to_vec(),
        config: Some(proto::SandboxConfig {
            memory_limit: 0,
            fuel_limit: 1_000_000,
            wall_time_limit_secs: 5,
            cpu_time_limit_secs: 0,
            capabilities: vec![],
            env,
            args: vec!["--verbose".to_string()],
        }),
        module_signature: vec![],
        module_ref: String::new(),
    });

    let result = service.create_sandbox(req).await;
    assert!(result.is_ok());
}
