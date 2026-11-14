use sovmem::mcp::{handle_request, Request};
use sovmem::profile::p3_test_support::ProvisionedP3Fixture;

#[test]
fn memory_status_returns_store_state() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-status-test");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_status",
            "arguments": {}
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(
        resp.error.is_none(),
        "expected success, got: {:?}",
        resp.error
    );
    let result = resp.result.unwrap();
    assert_eq!(result["resultType"], "complete");
    let sc = &result["structuredContent"];
    assert!(sc["store"]["state"].as_str().is_some());
    assert!(sc["ledger"]["total_verdicts"].is_number());
}

#[test]
fn memory_search_returns_error_without_real_index() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-search-test");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_search",
            "arguments": {
                "query": "test query"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    // Without a real index, this will return an error (not a panic)
    assert!(
        resp.error.is_some()
            || resp
                .result
                .as_ref()
                .and_then(|r| r.get("isError"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    );
}

#[test]
fn memory_get_returns_error_for_missing_document() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-get-test");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_get",
            "arguments": {
                "id": "nonexistent/doc.md"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    // Returns isError=true, not a protocol error
    assert!(
        resp.error.is_none(),
        "expected success envelope, got: {:?}",
        resp.error
    );
    let result = resp.result.unwrap();
    assert!(result["isError"].as_bool().unwrap_or(false));
}

#[test]
fn memory_get_rejects_empty_id() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-get-empty");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_get",
            "arguments": {
                "id": ""
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_get_rejects_path_traversal() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-get-patraversal");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_get",
            "arguments": {
                "id": "../etc/passwd"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_search_rejects_empty_query() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-search-empty");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_search",
            "arguments": {
                "query": ""
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_search_rejects_oversized_query() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-search-long");
    let long_query = "a".repeat(201);
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_search",
            "arguments": {
                "query": long_query
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_propose_rejects_missing_source() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-prop-miss");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_propose",
            "arguments": {
                "operation": "Create",
                "intent": "Accept",
                "target_id": "test-ns/doc.md",
                "target_path": "04-Research/doc.md",
                "candidate_body": "# Test"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_propose_rejects_oversized_body() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-prop-big");
    let big_body = "x".repeat(100_001);
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_propose",
            "arguments": {
                "source": "src:agent:test",
                "operation": "Create",
                "intent": "Accept",
                "target_id": "test-ns/doc.md",
                "target_path": "04-Research/doc.md",
                "candidate_body": big_body
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_propose_rejects_invalid_operation() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-prop-op");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_propose",
            "arguments": {
                "source": "src:agent:test",
                "operation": "Delete",
                "intent": "Accept",
                "target_id": "test-ns/doc.md",
                "target_path": "04-Research/doc.md",
                "candidate_body": "# Test"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_propose_rejects_invalid_intent() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-prop-int");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_propose",
            "arguments": {
                "source": "src:agent:test",
                "operation": "Create",
                "intent": "Ignore",
                "target_id": "test-ns/doc.md",
                "target_path": "04-Research/doc.md",
                "candidate_body": "# Test"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_propose_rejects_missing_target_path() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-prop-tp");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_propose",
            "arguments": {
                "source": "src:agent:test",
                "operation": "Create",
                "intent": "Accept",
                "target_id": "test-ns/doc.md",
                "target_path": "",
                "candidate_body": "# Test"
            }
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp
        .result
        .as_ref()
        .and_then(|r| r.get("isError"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false));
}

#[test]
fn memory_status_is_read_only() {
    // Verify that the status endpoint doesn't modify store state
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-status-ro");
    let req = Request {
        id: Some(serde_json::json!(1)),
        method: "tools/call".to_string(),
        params: serde_json::json!({
            "name": "memory_status",
            "arguments": {}
        }),
        _meta: Some(serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp1 = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    let resp2 = handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert_eq!(resp1.result, resp2.result, "status must be idempotent");
}
