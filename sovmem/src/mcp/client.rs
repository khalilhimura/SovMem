// Fake client for deterministic Layer 1 tests.

use crate::mcp::{handle_request, Request, Response};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Simulate a client sending a request and receiving a response through the handler.
pub fn send_request(req: Request, profile_path: &Path, store_root: &Path) -> Response {
    handle_request(&req, profile_path, store_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_profile() -> PathBuf {
        PathBuf::new()
    }

    fn dummy_store() -> PathBuf {
        PathBuf::new()
    }

    #[test]
    fn server_discover_returns_supported_versions() {
        let req = Request {
            id: Some(json!(1)),
            method: "server/discover".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert!(
            resp.error.is_none(),
            "expected success, got: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["supportedVersions"][0], "2026-07-28");
        assert_eq!(result["serverInfo"]["name"], "sovmem");
    }

    #[test]
    fn tools_list_returns_tool_definitions() {
        let req = Request {
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["resultType"], "complete");
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.len() >= 4);
        assert_eq!(tools[0]["name"], "memory_search");
        assert!(tools[0]["inputSchema"].is_object());
    }

    #[test]
    fn unsupported_protocol_version_returns_error() {
        let req = Request {
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "1900-01-01"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert_eq!(resp.error.as_ref().unwrap().code, -32022);
    }

    #[test]
    fn unknown_method_returns_protocol_error() {
        let req = Request {
            id: Some(json!(1)),
            method: "unknown".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert_eq!(resp.error.as_ref().unwrap().code, -32602);
    }

    #[test]
    fn unknown_tool_returns_protocol_error() {
        let req = Request {
            id: Some(json!(1)),
            method: "tools/call".to_string(),
            params: json!({
                "name": "memory_verdict",
                "arguments": {}
            }),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert_eq!(resp.error.as_ref().unwrap().code, -32602);
    }

    #[test]
    fn notification_returns_no_response() {
        let req = Request {
            id: None,
            method: "notifications/initialized".to_string(),
            params: json!({}),
            _meta: None,
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        assert!(resp.id.is_none());
        assert!(resp.result.is_none());
        assert!(resp.error.is_none());
    }

    #[test]
    fn tools_list_ttl_and_cache_scope() {
        let req = Request {
            id: Some(json!(1)),
            method: "tools/list".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        let result = resp.result.unwrap();
        assert!(result.get("ttlMs").is_some());
        assert_eq!(result["cacheScope"], "public");
    }

    #[test]
    fn server_discover_has_capabilities() {
        let req = Request {
            id: Some(json!(1)),
            method: "server/discover".to_string(),
            params: json!({}),
            _meta: Some(json!({
                "io.modelcontextprotocol/protocolVersion": "2026-07-28"
            })),
        };
        let resp = send_request(req, &dummy_profile(), &dummy_store());
        let result = resp.result.unwrap();
        assert!(result.get("capabilities").is_some());
        assert!(result["capabilities"]["tools"].is_object());
    }
}
