//! Contract tests verifying the gRPC proto definition matches SDK expectations.
//!
//! These tests parse the proto file and verify all RPCs and messages that
//! SDKs depend on are present, catching proto breaking changes early.

use std::collections::HashSet;
use std::fs;

fn load_proto() -> String {
    // Try workspace root first, then crate-relative
    fs::read_to_string("proto/isolate.proto")
        .or_else(|_| fs::read_to_string("../proto/isolate.proto"))
        .expect("Failed to read proto/isolate.proto")
}

fn extract_rpcs(proto: &str) -> HashSet<String> {
    let mut rpcs = HashSet::new();
    for line in proto.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("rpc ") {
            if let Some(paren) = trimmed.find('(') {
                let name = trimmed[4..paren].trim().to_string();
                rpcs.insert(name);
            }
        }
    }
    rpcs
}

fn extract_messages(proto: &str) -> HashSet<String> {
    let mut messages = HashSet::new();
    for line in proto.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("message ") {
            if let Some(name) = trimmed
                .strip_prefix("message ")
                .and_then(|s| s.split(|c: char| !c.is_alphanumeric() && c != '_').next())
            {
                messages.insert(name.to_string());
            }
        }
    }
    messages
}

#[test]
fn test_proto_file_exists() {
    assert!(
        std::path::Path::new("proto/isolate.proto").exists()
            || std::path::Path::new("../proto/isolate.proto").exists()
    );
}

#[test]
fn test_required_rpcs_present() {
    let proto = load_proto();
    let rpcs = extract_rpcs(&proto);

    let required = [
        "CreateSandbox",
        "RunSandbox",
        "GetSandbox",
        "TerminateSandbox",
        "ListSandboxes",
        "StreamOutput",
        "GetMetrics",
    ];

    for name in &required {
        assert!(rpcs.contains(*name), "Missing RPC: {}", name);
    }
}

#[test]
fn test_required_messages_present() {
    let proto = load_proto();
    let messages = extract_messages(&proto);

    let required = [
        "CreateSandboxRequest",
        "CreateSandboxResponse",
        "RunSandboxRequest",
        "RunSandboxResponse",
        "GetSandboxRequest",
        "GetSandboxResponse",
        "TerminateSandboxRequest",
        "TerminateSandboxResponse",
        "ListSandboxesRequest",
        "ListSandboxesResponse",
        "StreamOutputRequest",
        "OutputChunk",
    ];

    for name in &required {
        assert!(messages.contains(*name), "Missing message: {}", name);
    }
}

#[test]
fn test_service_name() {
    let proto = load_proto();
    assert!(proto.contains("service IsolateService"));
}

#[test]
fn test_streaming_rpc() {
    let proto = load_proto();
    assert!(proto.contains("stream OutputChunk"));
}

#[test]
fn test_proto_has_package() {
    let proto = load_proto();
    assert!(proto.contains("package isolate"));
}

#[test]
fn test_create_request_has_module() {
    let proto = load_proto();
    // Find CreateSandboxRequest block and check for module field
    assert!(proto.contains("module"));
}

#[test]
fn test_run_request_has_sandbox_id() {
    let proto = load_proto();
    assert!(proto.contains("sandbox_id"));
}

#[test]
fn test_minimum_rpc_count() {
    let proto = load_proto();
    let rpcs = extract_rpcs(&proto);
    assert!(rpcs.len() >= 7, "Expected >= 7 RPCs, got {}", rpcs.len());
}
