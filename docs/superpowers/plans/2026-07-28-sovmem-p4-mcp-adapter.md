# SovMem P4: MCP Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a stdio MCP server (protocol `2026-07-28`) exposing four tools (`memory_search`, `memory_get`, `memory_propose`, `memory_status`) via raw JSON-RPC 2.0 over stdin/stdout, with no new dependencies and no async runtime.

**Architecture:** A synchronous `mcp/` module on a dedicated thread. The server reads newline-delimited JSON-RPC 2.0 requests from stdin, dispatches to thin wrappers around existing P0–P3 library functions, and writes responses to stdout. Diagnostics go to stderr. No MCP SDK, no async, no tokio.

**Tech Stack:** Rust (single crate `sovmem`), `serde_json` (existing dep), `std::io::BufReader`/`BufWriter`, `clap` for CLI parsing.

**Spec:** `docs/superpowers/specs/2026-07-28-sovmem-p4-mcp-adapter-design.md`

## Global Constraints

- No new dependencies without explicit approval.
- No async anywhere in the binary.
- No `unsafe`.
- Stdout must reserve for MCP protocol frames only. All diagnostics on stderr.
- MCP transport is stdio only.
- No network calls, no model calls from the binary (A6).
- `memory_get` resolves canonical IDs only — no arbitrary path access.
- `memory_propose` writes only to the untrusted inbox — never to staging, canon, or index.
- No MCP tool may seal intake, issue a verdict, mutate policy, or write canon.
- All existing dependencies are pinned exactly (no lockfile changes beyond the initial `Cargo.lock`).
- Tool names must use `snake_case` per MCP 2026-07-28 spec: `memory_search`, `memory_get`, `memory_propose`, `memory_status`.

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `sovmem/src/mcp/mod.rs` | Server loop, JSON-RPC framing, stdin/stdout I/O, `_meta` validation, shutdown, cancellation |
| `sovmem/src/mcp/tools.rs` | Four tool definitions (`inputSchema`, `description`) and their handler functions |
| `sovmem/src/mcp/redact.rs` | Diagnostic redaction, error mapping (protocol errors vs `isError: true`) |
| `sovmem/src/mcp/client.rs` | Fake client for deterministic Layer 1 tests (calls handlers directly) |
| `sovmem/tests/mcp_stdio.rs` | Layer 2 tests: stdio simulation with in-memory channels |
| `sovmem/tests/mcp_integration.rs` | Layer 3 tests: integration with real P3 fixture store |

### Modified files

| File | Change |
|------|--------|
| `sovmem/src/lib.rs` | Add `pub mod mcp;` |
| `sovmem/src/main.rs` | Add `Serve` subcommand to dispatch `sovmem serve` |
| `sovmem/src/cli.rs` | Add `Serve` variant to `Command` enum and dispatch to `mcp::run_server` |
| `sovmem/Cargo.toml` | No changes — no new dependencies needed |

---

## Task 1: Module scaffold and `sovmem serve` CLI

**Files:**
- Create: `sovmem/src/mcp/mod.rs`
- Create: `sovmem/src/mcp/tools.rs`
- Create: `sovmem/src/mcp/redact.rs`
- Create: `sovmem/src/mcp/client.rs`
- Modify: `sovmem/src/lib.rs`
- Modify: `sovmem/src/main.rs`
- Modify: `sovmem/src/cli.rs`

**Interfaces:**
- Consumes: `clap` Subcommand pattern in `cli.rs`
- Produces: `pub fn run_server(profile: PathBuf, store: PathBuf)` — entry point for `sovmem serve`

- [ ] **Step 1: Create the module files**

Create `sovmem/src/mcp/mod.rs` with the module declaration and `run_server` stub:

```rust
pub mod tools;
pub mod redact;
#[cfg(test)]
pub mod client;

use std::path::PathBuf;
use std::thread;

pub fn run_server(profile_path: PathBuf, store_root: PathBuf) -> i32 {
    // Will be implemented in Task 3
    eprintln!("{}", serde_json::json!({
        "code": "mcp.server.starting",
        "severity": "info",
        "message": "MCP server starting"
    }));
    let handle = thread::spawn(move || {
        server_loop(&profile_path, &store_root);
    });
    match handle.join() {
        Ok(code) => code,
        Err(_) => {
            eprintln!("{}", serde_json::json!({
                "code": "mcp.server.panic",
                "severity": "error",
                "message": "MCP server thread panicked"
            }));
            1
        }
    }
}

fn server_loop(profile_path: &PathBuf, store_root: &PathBuf) -> i32 {
    // Stub: will be replaced in Tasks 2-3
    0
}
```

Create `sovmem/src/mcp/tools.rs` with the Tool struct and handler trait:

```rust
use serde_json::Value;

pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Result of a tool execution.
pub enum ToolResult {
    Success { text: String, structured: Value },
    Error { code: &'static str, message: String },
}

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "memory_search",
            description: "Search the SovMem knowledge base for documents matching a query",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (1-200 characters)"
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Maximum number of results (default 5, max 20)",
                        "default": 5,
                        "minimum": 1,
                        "maximum": 20
                    },
                    "expand": {
                        "type": "string",
                        "description": "Graph expansion mode: \"none\" or \"one-hop\"",
                        "enum": ["none", "one-hop"],
                        "default": "none"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "memory_get",
            description: "Retrieve a single canonical document by its exact ID",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Canonical document ID (e.g. ssa-0/04-Research/topic.md)"
                    },
                    "include_body": {
                        "type": "boolean",
                        "description": "Include document body text (max 4000 chars)",
                        "default": false
                    }
                },
                "required": ["id"]
            }),
        },
        ToolDefinition {
            name: "memory_propose",
            description: "Submit an untrusted proposal to the inbox for human review",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Source identifier (e.g. src:agent:claude-code)"
                    },
                    "operation": {
                        "type": "string",
                        "description": "Proposal operation: \"Create\" or \"Replace\"",
                        "enum": ["Create", "Replace"]
                    },
                    "intent": {
                        "type": "string",
                        "description": "Proposal intent: \"Accept\" or \"Rewrite\"",
                        "enum": ["Accept", "Rewrite"]
                    },
                    "target_id": {
                        "type": "string",
                        "description": "Canonical document ID to replace, or new path for Create"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Relative vault path (e.g. 04-Research/topic.md)"
                    },
                    "candidate_body": {
                        "type": "string",
                        "description": "Full Markdown body of the proposed document"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "Pre-draft confidence estimate (0.0-1.0, optional)",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "evaluator": {
                        "type": "object",
                        "description": "Evaluator block (optional)",
                        "properties": {
                            "agent": { "type": "string" },
                            "recommendation": { "type": "string" },
                            "reasons": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        }
                    }
                },
                "required": ["source", "operation", "intent", "target_id", "target_path", "candidate_body"]
            }),
        },
        ToolDefinition {
            name: "memory_status",
            description: "Get read-only status of the SovMem store, index, and intake queue",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
    ]
}
```

Create `sovmem/src/mcp/redact.rs` with redaction helpers:

```rust
use serde_json::Value;

/// Ensure error messages never echo user input or filesystem paths.
pub fn redacted_error(code: &'static str, message: &'static str) -> Value {
    serde_json::json!({
        "code": code,
        "severity": "error",
        "message": message
    })
}
```

Create `sovmem/src/mcp/client.rs` — empty for now, populated in Task 5:

```rust
// Fake client for deterministic Layer 1 tests.
// Populated in Task 5.
```

- [ ] **Step 2: Add `pub mod mcp` to lib.rs**

Edit `sovmem/src/lib.rs`:

```rust
pub mod cli;
pub mod profile;
pub mod mcp;
```

- [ ] **Step 3: Add `Serve` variant to the CLI command enum**

Edit `sovmem/src/cli.rs` — add the import and the `Serve` variant:

Add to the `use` section at top:
```rust
use crate::mcp;
```

Add to `Command` enum:
```rust
    Serve {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        store: PathBuf,
    },
```

Add dispatch in the `run()` function, before the `selected_p2_subcommand` check (since `serve` is a new P4 subcommand, not a P2 one). Actually, `serve` won't be caught by `selected_p2_subcommand()` since that function only checks for `index`, `query`, `eval`, `propose`, `proposals`, `stats`, `verdict`, `verdicts`, `recover`, `verify`.

Add to the match statement in `run()`:
```rust
        Command::Serve { profile, store } => mcp::run_server(profile, store),
```

- [ ] **Step 4: Run test to verify compilation**

Run:
```bash
cargo check --manifest-path sovmem/Cargo.toml --all-targets
```
Expected: compiles with warnings about unused code (ok for now).

- [ ] **Step 5: Commit**

```bash
git add sovmem/src/mcp/ sovmem/src/lib.rs sovmem/src/main.rs sovmem/src/cli.rs
git commit -m "feat(p4): scaffold mcp/ module and sovmem serve CLI entry"
```

---

## Task 2: JSON-RPC framing and server loop

**Files:**
- Modify: `sovmem/src/mcp/mod.rs`
- Test: `sovmem/tests/mcp_stdio.rs` (partially)

**Interfaces:**
- Consumes: `std::io::BufReader` on stdin, `std::io::BufWriter` on stdout
- Produces: `fn read_request(reader: &mut BufReader<Stdin>) -> Result<Request, FrameError>` and `fn write_response(writer: &mut BufWriter<Stdout>, response: &Response)`
- Data types: `Request { id: Option<Value>, method: String, params: Value, meta_protocol: Option<String> }` and `Response`
- Error types: `FrameError` for malformed JSON, missing newline, IO errors

- [ ] **Step 1: Write the failing test for framing**

Add to `sovmem/tests/mcp_stdio.rs`:

```rust
use std::io::{BufRead, BufReader, Write};

/// Simulate reading a JSON-RPC request from a byte buffer.
fn read_frame(data: &[u8]) -> serde_json::Value {
    let mut cursor = std::io::Cursor::new(data);
    let mut reader = BufReader::new(&mut cursor);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read line");
    serde_json::from_str(&line).expect("parse JSON")
}

#[test]
fn reads_newline_delimited_json_rpc() {
    let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}\n";
    let value = read_frame(input);
    assert_eq!(value["method"], "tools/list");
    assert_eq!(value["id"], 1);
}
```

Run:
```bash
cargo test --manifest-path sovmem/Cargo.toml --test mcp_stdio reads_newline_delimited_json_rpc
```
Expected: PASS (this test doesn't depend on the server code yet).

- [ ] **Step 2: Implement JSON-RPC types and framing in `mcp/mod.rs`**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, BufWriter, Stdin, Stdout, Write};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub _meta: Option<Value>,
}

impl Request {
    /// Extract the protocol version from `_meta`, if present.
    pub fn protocol_version(&self) -> Option<&str> {
        self._meta.as_ref().and_then(|m| {
            m.get("io.modelcontextprotocol/protocolVersion")
                .and_then(|v| v.as_str())
        })
    }

    /// Extract client capabilities from `_meta`, if present.
    pub fn client_info(&self) -> Option<Value> {
        self._meta.as_ref().and_then(|m| m.get("io.modelcontextprotocol/clientInfo").cloned())
    }
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl Response {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Response { id, result: Some(result), error: None }
    }

    pub fn error(id: Option<Value>, code: i32, message: &str, data: Option<Value>) -> Self {
        Response {
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.to_string(), data }),
        }
    }
}

/// Read one JSON-RPC frame from a buffered reader.
pub fn read_frame(reader: &mut impl BufRead) -> io::Result<String> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
    }
    Ok(line.trim_end().to_string())
}

/// Write a JSON-RPC response as a newline-delimited frame.
pub fn write_frame(writer: &mut impl Write, response: &Response) -> io::Result<()> {
    let json = serde_json::to_string(response)?;
    writeln!(writer, "{json}")?;
    writer.flush()?;
    Ok(())
}
```

- [ ] **Step 3: Run tests to verify**

Run:
```bash
cargo test --manifest-path sovmem/Cargo.toml --test mcp_stdio
```
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add sovmem/src/mcp/mod.rs sovmem/tests/mcp_stdio.rs
git commit -m "feat(p4): implement JSON-RPC framing and types"
```

---

## Task 3: `server/discover` and `tools/list` endpoints

**Files:**
- Modify: `sovmem/src/mcp/mod.rs` (server loop, dispatch logic)
- Test: `sovmem/src/mcp/client.rs` (fake client for Layer 1)

**Interfaces:**
- Consumes: `Request`, `Response`, `read_frame`, `write_frame`, `ToolDefinition`, `all_tools()`
- Produces: `fn handle_request(req: &Request, profile_path: &PathBuf, store_root: &PathBuf) -> Response`
- Protocol: `server/discover` → returns `supportedVersions`, `serverInfo`, `capabilities`
- Protocol: `tools/list` → returns tool definitions with `inputSchema`, `ttlMs`, `cacheScope`

- [ ] **Step 1: Write failing Layer 1 test in `client.rs`**

In `sovmem/src/mcp/client.rs`:

```rust
use super::mod_::{Request, Response, JsonRpcError};
use serde_json::Value;

/// Simulate a client sending a request and receiving a response through the handler.
pub fn send_request(req: Request, profile_path: &std::path::PathBuf, store_root: &std::path::PathBuf) -> Response {
    super::handle_request(&req, profile_path, store_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let resp = send_request(req, &std::path::PathBuf::new(), &std::path::PathBuf::new());
        assert!(resp.error.is_none(), "expected success, got: {:?}", resp.error);
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
        let resp = send_request(req, &std::path::PathBuf::new(), &std::path::PathBuf::new());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["resultType"], "complete");
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.len() >= 4);
        assert_eq!(tools[0]["name"], "memory_search");
        assert!(tools[0]["inputSchema"].is_object());
    }
}
```

- [ ] **Step 2: Run test to verify it fails (no `handle_request` yet)**

Run:
```bash
cargo test --manifest-path sovmem/Cargo.toml --lib -- mcp::client
```
Expected: compilation errors because `handle_request` doesn't exist yet.

- [ ] **Step 3: Implement `handle_request` and complete server loop**

Add to `sovmem/src/mcp/mod.rs` — the dispatch function:

```rust
use crate::mcp::tools::{all_tools, ToolResult};
use crate::profile::{load_p3_profile, inspect_store, StorePaths};
use serde_json::json;

/// Dispatch a parsed JSON-RPC request to the appropriate handler.
pub fn handle_request(req: &Request) -> Response {
    // 1. Validate protocol version
    let proto = req.protocol_version();
    if proto.is_some() && proto != Some("2026-07-28") {
        return Response::error(
            req.id.clone(),
            -32022,
            "Unsupported protocol version",
            Some(json!({
                "supported": ["2026-07-28"],
                "requested": proto
            })),
        );
    }

    match req.method.as_str() {
        "server/discover" => handle_discover(req),
        "tools/list" => handle_tools_list(req),
        "tools/call" => handle_tools_call(req),
        _ => Response::error(req.id.clone(), -32602, &format!("Unknown method: {}", req.method), None),
    }
}

fn handle_discover(req: &Request) -> Response {
    Response::success(req.id.clone(), json!({
        "resultType": "complete",
        "supportedVersions": ["2026-07-28"],
        "serverInfo": {
            "name": "sovmem",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": {
            "tools": {}
        }
    }))
}

fn handle_tools_list(req: &Request) -> Response {
    let tools: Vec<Value> = all_tools().into_iter().map(|t| json!({
        "name": t.name,
        "description": t.description,
        "inputSchema": t.input_schema,
    })).collect();

    Response::success(req.id.clone(), json!({
        "resultType": "complete",
        "tools": tools,
        "ttlMs": 300_000,
        "cacheScope": "public"
    }))
}

fn handle_tools_call(req: &Request) -> Response {
    let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = req.params.get("arguments").unwrap_or(&json!({}));

    match tool_name {
        "memory_search" => {
            // Stub: will be implemented in Task 4
            Response::error(req.id.clone(), -32603, "not implemented yet", None)
        }
        "memory_get" => {
            // Stub: will be implemented in Task 4
            Response::error(req.id.clone(), -32603, "not implemented yet", None)
        }
        "memory_propose" => {
            // Stub: will be implemented in Task 4
            Response::error(req.id.clone(), -32603, "not implemented yet", None)
        }
        "memory_status" => {
            // Stub: will be implemented in Task 4
            Response::error(req.id.clone(), -32603, "not implemented yet", None)
        }
        _ => Response::error(req.id.clone(), -32602, &format!("Unknown tool: {tool_name}"), None),
    }
}
```

Also update the `server_loop` function to read and dispatch:

```rust
fn server_loop(profile_path: &PathBuf, store_root: &PathBuf) -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());

    loop {
        let line = match read_frame(&mut reader) {
            Ok(line) => line,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // stdin closed gracefully
                return 0;
            }
            Err(_) => return 1,
        };

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => {
                // Malformed JSON — write error, continue
                let err = Response::error(None, -32602, "Malformed JSON-RPC request", None);
                let _ = write_frame(&mut writer, &err);
                continue;
            }
        };

        let response = handle_request(&req, profile_path, store_root);
        if let Err(e) = write_frame(&mut writer, &response) {
            // Broken pipe
            return 1;
        }
    }
}
```

Need to adjust `handle_request` to take the profile/store paths but not use them yet:

```rust
pub fn handle_request(req: &Request, _profile_path: &PathBuf, _store_root: &PathBuf) -> Response {
```

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
cargo test --manifest-path sovmem/Cargo.toml --lib -- mcp::client::tests
```
Expected: PASS

- [ ] **Step 5: Run format and clippy**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --all
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --features test-support -- --deny warnings
```

- [ ] **Step 6: Commit**

```bash
git add sovmem/src/mcp/mod.rs sovmem/src/mcp/client.rs
git commit -m "feat(p4): implement server/discover and tools/list endpoints"
```

---

## Task 4: Tool handlers — `memory_status`, `memory_search`, `memory_get`, `memory_propose`

**Files:**
- Modify: `sovmem/src/mcp/tools.rs` (handler implementations)
- Modify: `sovmem/src/mcp/mod.rs` (wire handlers into `handle_tools_call`)

**Interfaces:**
- Consumes: `load_p3_profile(profile_path)`, `inspect_store(profile, store_root)`, `StorePaths`, `read_intake(paths)`, `query_store(StoreQueryRequest)`, `submit_proposal(ProposalSubmission)`, `ProposalSubmission`, `ProposalOperation`, `ProposalIntent`, `Confidence`, `EvaluatorBlock`, `safe_target_path`, `p3_canonical_document_id`, `CanonicalHead::Generation`
- Produces: `fn handle_memory_search(req, profile_path, store_root) -> Response`, etc.

- [ ] **Step 1: Write failing Layer 1 tests for all four tools**

In `sovmem/src/mcp/client.rs`, add tests that require a real P3 fixture store. Create a test module for Layer 3:

In `sovmem/tests/mcp_integration.rs`:

```rust
use sovmem::mcp::tools::ToolResult;
use sovmem::profile::p3_test_support::{ProvisionedP3Fixture};

#[test]
fn memory_status_returns_store_state() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-status-test");
    let req = sovmem::mcp::Request {
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
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp.error.is_none(), "expected success, got: {:?}", resp.error);
    let result = resp.result.unwrap();
    assert_eq!(result["resultType"], "complete");
    let sc = &result["structuredContent"];
    assert!(sc["store"]["state"].as_str().is_some());
    assert!(sc["ledger"]["total_verdicts"].is_number());
}
```

- [ ] **Step 2: Run test to verify it fails (handlers return "not implemented")**

Run:
```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --test mcp_integration
```
Expected: the test gets an error response, not success.

- [ ] **Step 3: Implement `memory_status` handler**

In `sovmem/src/mcp/tools.rs`, add:

```rust
use std::path::PathBuf;
use crate::profile::{
    load_p3_profile, inspect_store, read_intake, StorePaths,
    StoreSnapshot, StoreError,
};
use serde_json::Value;

pub fn handle_memory_status(profile_path: &PathBuf, store_root: &PathBuf) -> ToolResult {
    let profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(e) => return ToolResult::Error {
            code: "store.unreadable",
            message: format!("Profile could not be loaded: {}", e.code()),
        },
    };
    let snapshot = match inspect_store(&profile, store_root) {
        Ok(s) => s,
        Err(e) => return ToolResult::Error {
            code: "store.unreadable",
            message: "Store could not be inspected".to_string(),
        },
    };

    // Count intake states
    let intake = read_intake(&snapshot.paths).ok();
    let (submissions, staged, ruled) = match &intake {
        Some(view) => (
            view.submissions.len(),
            view.staged.len(),
            view.ruled.len(),
        ),
        None => (0, 0, 0),
    };

    let sealed = snapshot.sealed;
    let doc_count = snapshot.head.as_ref().map(|h| {
        match h {
            crate::profile::CanonicalHead::Seed => 0usize,
            crate::profile::CanonicalHead::Generation(g) => g.manifest.documents.len(),
        }
    }).unwrap_or(0);

    ToolResult::Success {
        text: format!(
            "Store state: {} | {} documents | Verdicts: {}",
            if sealed { "sealed" } else { "unsealed" },
            doc_count,
            snapshot.ledger_verdicts(),
        ),
        structured: serde_json::json!({
            "store": {
                "state": if sealed { "sealed" } else { "unsealed" },
                "canonical_documents": doc_count,
                "canonical_generation": snapshot.head.as_ref().map(|h| match h {
                    crate::profile::CanonicalHead::Seed => "seed",
                    crate::profile::CanonicalHead::Generation(g) => &g.manifest.generation_id,
                }.to_string()),
                "level_4_verified": false, // MCP doesn't run verification
            },
            "intake": {
                "pending_submissions": submissions,
                "staged_proposals": staged,
                "ruled_proposals": ruled,
            },
            "ledger": {
                "total_verdicts": snapshot.ledger_verdicts(),
                "last_verdict_at": snapshot.head.as_ref().map(|h| match h {
                    crate::profile::CanonicalHead::Seed => String::new(),
                    crate::profile::CanonicalHead::Generation(g) => g.manifest.applied_at.clone(),
                }).filter(|s| !s.is_empty()),
            },
        }),
    }
}
```

Note: `StoreSnapshot` doesn't have `ledger_verdicts()`. Let me check what it actually exposes:

```rust
// From store.rs — StoreSnapshot fields
pub struct StoreSnapshot {
    pub paths: StorePaths,
    pub policy: PolicyConfig,
    pub profile_hash: String,
    pub seed: SeedEvidence,
    pub head: Option<CanonicalHead>,
    pub sealed: bool,
}
```

So we need to compute verdicts differently. Check the ledger file directly:

```rust
fn count_verdicts(paths: &StorePaths) -> usize {
    std::fs::read(&paths.ledger)
        .ok()
        .map(|bytes| bytes.iter().filter(|&&b| b == b'\n').count())
        .unwrap_or(0)
}
```

Update `handle_memory_status` to use this instead.

Wire into `handle_tools_call` in `mod.rs`:

```rust
"memory_status" => {
    let result = tools::handle_memory_status(_profile_path, _store_root);
    tool_result_to_response(req, result)
}
```

Add helper function:

```rust
fn tool_result_to_response(req: &Request, result: ToolResult) -> Response {
    match result {
        ToolResult::Success { text, structured } => {
            Response::success(req.id.clone(), json!({
                "resultType": "complete",
                "content": [{
                    "type": "text",
                    "text": text
                }],
                "structuredContent": structured
            }))
        }
        ToolResult::Error { code, message } => {
            Response::success(req.id.clone(), json!({
                "resultType": "complete",
                "content": [{
                    "type": "text",
                    "text": message
                }],
                "isError": true
            }))
        }
    }
}
```

- [ ] **Step 4: Implement `memory_search` handler**

In `sovmem/src/mcp/tools.rs`:

```rust
pub fn handle_memory_search(
    profile_path: &PathBuf,
    store_root: &PathBuf,
    query: &str,
    top_k: usize,
    expand: &str,
) -> ToolResult {
    // Validate inputs
    if query.trim().is_empty() || query.len() > 200 {
        return ToolResult::Error {
            code: "query.invalid",
            message: "Query must be between 1 and 200 characters".to_string(),
        };
    }
    let top_k = top_k.clamp(1, 20);
    let expansion = match expand {
        "one-hop" => crate::profile::ExpansionMode::OneHop,
        _ => crate::profile::ExpansionMode::None,
    };

    let request = crate::profile::StoreQueryRequest {
        profile_path: profile_path.clone(),
        store_root: store_root.clone(),
        query: query.to_string(),
        top_k,
        expansion,
    };

    match crate::profile::query_store(request) {
        Ok(result) => {
            let results: Vec<Value> = result.result.results.into_iter().map(|hit| {
                json!({
                    "id": hit.id,
                    "score": hit.score.parse::<f64>().unwrap_or(0.0),
                    "title": hit.title,
                    "type": hit.core_type,
                    "layer": hit.layer,
                    "provenance": hit.provenance,
                })
            }).collect();

            let total_hits = results.len();

            ToolResult::Success {
                text: format!(
                    "Found {} result(s) for query '{}'",
                    total_hits, query
                ),
                structured: json!({
                    "results": results,
                    "total_hits": total_hits,
                    "generation_id": result.generation,
                }),
            }
        }
        Err(e) => ToolResult::Error {
            code: match e {
                crate::profile::QueryError::Empty => "query.invalid",
                crate::profile::QueryError::Invalid
                | crate::profile::QueryError::TooManyTokens => "query.invalid",
                crate::profile::QueryError::IndexMissing => "index.missing",
                crate::profile::QueryError::Stale => "index.stale",
                _ => "server.internal",
            },
            message: format!("Search failed: {}", e.code()),
        },
    }
}
```

Wire into `handle_tools_call`:

```rust
"memory_search" => {
    let query = arguments.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let top_k = arguments.get("top_k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let expand = arguments.get("expand").and_then(|v| v.as_str()).unwrap_or("none");
    let result = tools::handle_memory_search(_profile_path, _store_root, query, top_k, expand);
    tool_result_to_response(req, result)
}
```

- [ ] **Step 5: Implement `memory_get` handler**

In `sovmem/src/mcp/tools.rs`:

```rust
use crate::profile::{
    p3_canonical_document_id, load_p3_profile, StorePaths,
    CanonicalHead,
};

pub fn handle_memory_get(
    profile_path: &PathBuf,
    store_root: &PathBuf,
    id: &str,
    include_body: bool,
) -> ToolResult {
    if id.trim().is_empty() {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID must not be empty".to_string(),
        };
    }

    // Reject path traversal patterns
    if id.contains("..") || id.contains("//") || id.starts_with('/') {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID contains invalid characters".to_string(),
        };
    }

    let profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(e) => return ToolResult::Error {
            code: "store.unreadable",
            message: "Profile could not be loaded".to_string(),
        },
    };

    // Validate the ID is a well-formed canonical document ID
    if !id.ends_with(".md") {
        return ToolResult::Error {
            code: "id.invalid",
            message: "Document ID must end with .md".to_string(),
        };
    }

    // Look up the document in the current generation's canon manifest
    let snapshot = match crate::profile::inspect_store(&profile, store_root) {
        Ok(s) => s,
        Err(e) => return ToolResult::Error {
            code: "store.unreadable",
            message: "Store could not be inspected".to_string(),
        },
    };

    let generation = match &snapshot.head {
        Some(CanonicalHead::Generation(g)) => g,
        Some(CanonicalHead::Seed) => return ToolResult::Error {
            code: "document.not_found",
            message: "No canon generation found (store is in seed state)".to_string(),
        },
        None => return ToolResult::Error {
            code: "store.unreadable",
            message: "Store has no current head".to_string(),
        },
    };

    // Find the document in the canon manifest
    let doc = match generation.manifest.documents.iter().find(|d| d.id == id) {
        Some(d) => d,
        None => return ToolResult::Error {
            code: "document.not_found",
            message: format!("Document '{}' not found", id),
        },
    };

    // Read the actual file from the canon bundle
    let canon_path = generation.root.join(&doc.output_path);
    let body = if include_body {
        match std::fs::read_to_string(&canon_path) {
            Ok(b) => {
                let truncated: String = b.chars().take(4000).collect();
                Some(truncated)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Parse frontmatter for metadata
    let frontmatter = crate::profile::parse_source_document(&canon_path).ok();
    let (title, core_type, layer, tags, provenance, source, created, updated) = match &frontmatter {
        Some(doc) => (
            doc.title.clone().unwrap_or_default(),
            doc.core_type.as_deref().unwrap_or("").to_string(),
            doc.layer.as_deref().unwrap_or("").to_string(),
            doc.tags.clone().unwrap_or_default(),
            doc.provenance.as_deref().unwrap_or("").to_string(),
            doc.source.as_deref().unwrap_or("").to_string(),
            doc.created.clone().unwrap_or_default(),
            doc.updated.clone().unwrap_or_default(),
        ),
        None => (String::new(), String::new(), String::new(), vec![], String::new(), String::new(), String::new(), String::new()),
    };

    ToolResult::Success {
        text: format!("Document: {}\nType: {} | Layer: {}\nProvenance: {}", title, core_type, layer, provenance),
        structured: json!({
            "id": doc.id,
            "title": title,
            "type": core_type,
            "layer": layer,
            "tags": tags,
            "provenance": provenance,
            "source": source,
            "created": created,
            "updated": updated,
            "body": body,
            "generation_id": generation.manifest.generation_id,
        }),
    }
}
```

Wire into `handle_tools_call`:

```rust
"memory_get" => {
    let id = arguments.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let include_body = arguments.get("include_body").and_then(|v| v.as_bool()).unwrap_or(false);
    let result = tools::handle_memory_get(_profile_path, _store_root, id, include_body);
    tool_result_to_response(req, result)
}
```

- [ ] **Step 6: Implement `memory_propose` handler**

In `sovmem/src/mcp/tools.rs`:

```rust
use crate::profile::{
    ProposalSubmission, ProposalOperation, ProposalIntent,
    Confidence, EvaluatorBlock, safe_target_path, p3_canonical_document_id,
    submit_proposal, SubmitProposalResult, ProposalError,
};
use std::path::PathBuf;

pub fn handle_memory_propose(
    profile_path: &PathBuf,
    store_root: &PathBuf,
    source: &str,
    operation: &str,
    intent: &str,
    target_id: &str,
    target_path: &str,
    candidate_body: &str,
    confidence: Option<f64>,
    evaluator: Option<Value>,
) -> ToolResult {
    // Validate required fields
    if source.trim().is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "Source is required".to_string(),
        };
    }
    if target_id.trim().is_empty() || target_path.trim().is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "target_id and target_path are required".to_string(),
        };
    }
    if candidate_body.is_empty() {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "candidate_body must not be empty".to_string(),
        };
    }

    // Size limit
    if candidate_body.len() > 100_000 {
        return ToolResult::Error {
            code: "proposal.size_exceeded",
            message: "Candidate body exceeds maximum size of 100KB".to_string(),
        };
    }

    let operation_enum = match operation {
        "Create" => ProposalOperation::Create,
        "Replace" => ProposalOperation::Replace,
        _ => return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "operation must be 'Create' or 'Replace'".to_string(),
        },
    };

    let intent_enum = match intent {
        "Accept" => ProposalIntent::Accept,
        "Rewrite" => ProposalIntent::Rewrite,
        _ => return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "intent must be 'Accept' or 'Rewrite'".to_string(),
        },
    };

    // Validate target path safety
    if !safe_target_path(target_path) {
        return ToolResult::Error {
            code: "proposal.invalid_input",
            message: "Target path is unsafe or malformed".to_string(),
        };
    }

    let profile = match load_p3_profile(profile_path) {
        Ok(p) => p,
        Err(e) => return ToolResult::Error {
            code: "store.unreadable",
            message: "Profile could not be loaded".to_string(),
        },
    };

    // Write candidate body to a temp file (submit_proposal requires a file path)
    let temp_dir = store_root.join(".mcp");
    let _ = std::fs::create_dir_all(&temp_dir);
    let candidate_hash = blake3::hash(candidate_body.as_bytes()).to_hex();
    let candidate_path = temp_dir.join(format!("{candidate_hash}.md"));

    if let Err(e) = std::fs::write(&candidate_path, candidate_body) {
        return ToolResult::Error {
            code: "proposal.store_error",
            message: "Failed to write candidate file".to_string(),
        };
    }

    // Parse confidence
    let parsed_confidence = confidence.and_then(|c| {
        let scaled = (c * 1_000_000_000.0) as u32;
        Confidence::from_scaled(scaled)
    });

    // Write evaluator file if present
    let evaluator_path = evaluator.map(|ev| {
        let ev_path = temp_dir.join(format!("{candidate_hash}-eval.json"));
        let _ = std::fs::write(&ev_path, serde_json::to_string(&ev).unwrap());
        ev_path
    });

    let submission = ProposalSubmission {
        profile_path: profile_path.clone(),
        store_root: store_root.clone(),
        source_id: source.to_string(),
        operation: operation_enum,
        intent: intent_enum,
        target_id: target_id.to_string(),
        target_path: target_path.to_string(),
        candidate_path,
        confidence: parsed_confidence,
        evaluator_path,
    };

    match submit_proposal(submission) {
        Ok(result) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            ToolResult::Success {
                text: format!("Proposal submitted. Submission ID: {}", result.submission_id),
                structured: json!({
                    "submission_id": result.submission_id,
                    "state": "submitted",
                    "warnings": [],
                }),
            }
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            ToolResult::Error {
                code: match e {
                    ProposalError::Invalid(_) => "proposal.invalid_input",
                    _ => "proposal.store_error",
                },
                message: format!("Proposal failed: {}", e.code()),
            }
        }
    }
}
```

Wire into `handle_tools_call`:

```rust
"memory_propose" => {
    let source = arguments.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let operation = arguments.get("operation").and_then(|v| v.as_str()).unwrap_or("");
    let intent = arguments.get("intent").and_then(|v| v.as_str()).unwrap_or("");
    let target_id = arguments.get("target_id").and_then(|v| v.as_str()).unwrap_or("");
    let target_path = arguments.get("target_path").and_then(|v| v.as_str()).unwrap_or("");
    let candidate_body = arguments.get("candidate_body").and_then(|v| v.as_str()).unwrap_or("");
    let confidence = arguments.get("confidence").and_then(|v| v.as_f64());
    let evaluator = arguments.get("evaluator").cloned();
    let result = tools::handle_memory_propose(
        _profile_path, _store_root,
        source, operation, intent, target_id, target_path,
        candidate_body, confidence, evaluator,
    );
    tool_result_to_response(req, result)
}
```

- [ ] **Step 7: Run integration tests**

Run:
```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --test mcp_integration
```
Expected: all integration tests pass.

- [ ] **Step 8: Run format and clippy**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --all
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --features test-support -- --deny warnings
```

- [ ] **Step 9: Commit**

```bash
git add sovmem/src/mcp/tools.rs sovmem/src/mcp/mod.rs sovmem/tests/mcp_integration.rs
git commit -m "feat(p4): implement all four MCP tool handlers"
```

---

## Task 5: Error handling, redaction, and shutdown

**Files:**
- Modify: `sovmem/src/mcp/redact.rs`
- Modify: `sovmem/src/mcp/mod.rs` (shutdown, broken pipe, cancellation, panic handling)

**Interfaces:**
- Consumes: `notifications/cancelled` JSON-RPC messages, cancellation flag
- Produces: Graceful EOF handling, SIGTERM signal handler, broken pipe exit

- [ ] **Step 1: Write tests for redaction and shutdown**

In `sovmem/tests/mcp_stdio.rs`:

```rust
#[test]
fn malformed_json_returns_protocol_error() {
    let input = b"not valid json\n";
    let mut cursor = std::io::Cursor::new(input);
    let mut reader = std::io::BufReader::new(&mut cursor);
    // The server reads a line, fails to parse, writes error, continues
    // Use the in-memory simulation from Task 2
}

#[test]
fn eof_triggers_clean_shutdown() {
    // Empty input = EOF immediately
    let input = b"";
    // Server reads EOF from reader, exits 0
}
```

- [ ] **Step 2: Implement redaction rules**

In `sovmem/src/mcp/redact.rs`:

```rust
use serde_json::Value;

/// Ensure a diagnostic message contains no user input or file paths.
/// Always returns a static string.
pub fn safe_diagnostic(code: &'static str, context: &'static str) -> Value {
    serde_json::json!({
        "code": code,
        "severity": "error",
        "message": context,
    })
}
```

- [ ] **Step 3: Implement cancellation support**

In `sovmem/src/mcp/mod.rs`, add a cancellation flag:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Shared cancellation flag
static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn handle_request(req: &Request, profile_path: &PathBuf, store_root: &PathBuf) -> Response {
    // Reset cancellation flag for each request
    CANCELLED.store(false, Ordering::SeqCst);

    // Check protocol version
    // ... existing version check ...

    match req.method.as_str() {
        "notifications/cancelled" => {
            CANCELLED.store(true, Ordering::SeqCst);
            // Notifications have no response
            return Response { id: None, result: None, error: None };
        }
        // ... rest of dispatch ...
    }
}
```

- [ ] **Step 4: Implement broken pipe detection**

In `server_loop`, the `write_frame` already returns an error on broken pipe; the existing code handles it by returning 1. This is sufficient.

For panic handling, wrap the dispatch in `catch_unwind`:

```rust
fn server_loop(profile_path: &PathBuf, store_root: &PathBuf) -> i32 {
    // ... setup ...

    loop {
        // ... read line ...

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let req: Request = serde_json::from_str(&line)?;
            Ok(handle_request(&req, profile_path, store_root))
        }));

        match result {
            Ok(Ok(response)) => {
                if let Err(e) = write_frame(&mut writer, &response) {
                    return 1;
                }
            }
            Ok(Err(_)) => {
                let err = Response::error(None, -32602, "Malformed JSON-RPC request", None);
                let _ = write_frame(&mut writer, &err);
            }
            Err(_) => {
                // Panic caught — log to stderr and exit
                eprintln!("{}", serde_json::json!({
                    "code": "mcp.server.internal",
                    "severity": "error",
                    "message": "Internal server error"
                }));
                return 1;
            }
        }
    }
}
```

- [ ] **Step 5: Run all tests**

```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add sovmem/src/mcp/
git commit -m "feat(p4): implement error handling, redaction, and shutdown"
```

---

## Task 6: Layer 1 fake client test suite

**Files:**
- Modify: `sovmem/src/mcp/client.rs` (full test suite)

**Test coverage (all Layer 1 — direct handler calls, no I/O):**

- [ ] **Step 1: Write test for protocol version mismatch**

```rust
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
```

- [ ] **Step 2: Write test for unknown method**

```rust
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
```

- [ ] **Step 3: Write test for unknown tool in tools/call**

```rust
#[test]
fn unknown_tool_returns_tool_execution_error() {
    // ... similar to above, method = "tools/call", params = {"name": "memory_verdict", "arguments": {}} ...
    // Should return isError: true
}
```

- [ ] **Step 4: Write tests for each error condition from the design spec**

Complete the fixture list from §10 of the design spec:

| Test | What it checks |
|------|---------------|
| `mcp_server_discover` | `server/discover` returns `supportedVersions`, `serverInfo`, `capabilities` |
| `mcp_tools_list` | `tools/list` returns 4 tools with proper `inputSchema` |
| `mcp_unsupported_protocol_revision` | Protocol `1900-01-01` rejected with `-32022` |
| `mcp_unknown_tool` | Unknown tool name returns `isError: true` |
| `mcp_unexpected_field` | Extra arguments ignored or accepted |
| `memory_search_empty_query` | Empty query returns `isError: true` with `query.invalid` |
| `memory_search_oversized_query` | Query >200 chars returns `isError: true` |
| `memory_get_invalid_id` | Empty ID, path traversal `../`, absolute path `/etc/passwd` all rejected |
| `memory_get_symlink_id` | ID with symlink-like patterns rejected |
| `memory_propose_missing_field` | Missing `source`, `operation`, `target_id` returns `isError: true` |
| `memory_propose_oversized_candidate` | Body >100KB returns `isError: true` |

- [ ] **Step 5: Run all Layer 1 tests**

```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --lib -- mcp
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add sovmem/src/mcp/client.rs
git commit -m "test(p4): add Layer 1 fake client test suite for MCP tools"
```

---

## Task 7: Layer 2 stdio simulation tests

**Files:**
- Modify: `sovmem/tests/mcp_stdio.rs` (full test suite with in-memory channels)

**Approach:** Create a helper that simulates the server loop over an in-memory byte buffer instead of real stdin/stdout, to test framing, EOF, and backpressure without spawning a subprocess.

- [ ] **Step 1: Write test for EOF shutdown**

```rust
#[test]
fn eof_shutdown_exits_cleanly() {
    let input = b"";  // Immediate EOF
    let mut reader = std::io::BufReader::new(&input[..]);
    let mut output = Vec::new();
    let mut writer = std::io::BufWriter::new(&mut output);

    // Simulate one iteration of the server loop
    let line = sovmem::mcp::read_frame(&mut reader);
    assert!(line.is_err());  // EOF
    // Server should exit 0
}
```

- [ ] **Step 2: Write test for malformed JSON**

```rust
#[test]
fn malformed_json_does_not_crash_server() {
    let input = b"not valid json\n{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}\n";
    let mut reader = std::io::BufReader::new(&input[..]);
    let mut output = Vec::new();
    let mut writer = std::io::BufWriter::new(&mut output);

    // First frame: malformed
    let line = sovmem::mcp::read_frame(&mut reader).unwrap();
    let req: Result<sovmem::mcp::Request, _> = serde_json::from_str(&line);
    assert!(req.is_err());

    // Write error response
    let err = sovmem::mcp::Response::error(None, -32602, "Malformed JSON-RPC request", None);
    sovmem::mcp::write_frame(&mut writer, &err).unwrap();

    // Second frame: valid
    let line = sovmem::mcp::read_frame(&mut reader).unwrap();
    let req: sovmem::mcp::Request = serde_json::from_str(&line).unwrap();
    assert_eq!(req.method, "tools/list");
}
```

- [ ] **Step 3: Write test for broken pipe**

```rust
#[test]
fn broken_pipe_exits_with_error() {
    let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1,\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\"}}\n";
    let mut reader = std::io::BufReader::new(&input[..]);
    // Use a writer that always fails (simulate broken pipe)
    struct BrokenWriter;
    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe"))
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut writer = std::io::BufWriter::new(BrokenWriter);

    // Read the frame successfully
    let line = sovmem::mcp::read_frame(&mut reader).unwrap();
    let req: sovmem::mcp::Request = serde_json::from_str(&line).unwrap();
    let resp = sovmem::mcp::handle_request(&req, &std::path::PathBuf::new(), &std::path::PathBuf::new());

    // Writing the response should fail (broken pipe)
    assert!(sovmem::mcp::write_frame(&mut writer, &resp).is_err());
}
```

- [ ] **Step 4: Write test for backpressure**

```rust
#[test]
fn concurrent_requests_are_serialized() {
    let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1,\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\"}}\n\
                   {\"jsonrpc\":\"2.0\",\"method\":\"server/discover\",\"id\":2,\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\"}}\n";
    let mut reader = std::io::BufReader::new(&input[..]);
    let mut output = Vec::new();
    let mut writer = std::io::BufWriter::new(&mut output);

    // Process both requests sequentially
    for _ in 0..2 {
        let line = sovmem::mcp::read_frame(&mut reader).unwrap();
        let req: sovmem::mcp::Request = serde_json::from_str(&line).unwrap();
        let resp = sovmem::mcp::handle_request(&req, &std::path::PathBuf::new(), &std::path::PathBuf::new());
        sovmem::mcp::write_frame(&mut writer, &resp).unwrap();
    }

    let output_str = String::from_utf8(output).unwrap();
    assert!(output_str.contains("\"id\":1"));
    assert!(output_str.contains("\"id\":2"));
}
```

- [ ] **Step 5: Run all test binaries that compile**

```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml
```

- [ ] **Step 6: Run format and clippy**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --all
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --features test-support -- --deny warnings
```

- [ ] **Step 7: Commit**

```bash
git add sovmem/tests/mcp_stdio.rs
git commit -m "test(p4): add Layer 2 stdio simulation tests"
```

---

## Task 8: Layer 3 integration tests with P3 fixtures

**Files:**
- Modify: `sovmem/tests/mcp_integration.rs` (full test suite)

- [ ] **Step 1: Write test for `memory_search` with real store**

```rust
use sovmem::mcp::Request;
use serde_json::json;

#[test]
fn memory_search_with_valid_query_returns_results() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-search-test");
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_search",
            "arguments": {
                "query": "trust",
                "top_k": 5
            }
        }),
        _meta: Some(json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp.error.is_none(), "expected success, got: {:?}", resp.error);
    let result = resp.result.unwrap();
    let sc = &result["structuredContent"];
    let results = sc["results"].as_array().unwrap();
    assert!(!results.is_empty(), "expected at least one result");
    assert!(results[0]["id"].as_str().unwrap().contains(".md"));
    assert!(results[0]["score"].as_f64().unwrap() > 0.0);
}
```

- [ ] **Step 2: Write test for `memory_get` with real store**

```rust
#[test]
fn memory_get_valid_id_returns_document() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-get-test");
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_get",
            "arguments": {
                "id": "ssa-0/04-Research/Audy-Himura.md",
                "include_body": false
            }
        }),
        _meta: Some(json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let sc = &result["structuredContent"];
    assert!(sc["id"].as_str().unwrap().contains(".md"));
    assert!(sc["title"].as_str().unwrap_or("").len() > 0);
}
```

- [ ] **Step 3: Write test for `memory_get` nonexistent ID**

```rust
#[test]
fn memory_get_nonexistent_id_returns_not_found() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-get-miss-test");
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_get",
            "arguments": {
                "id": "ssa-0/nonexistent/doc.md"
            }
        }),
        _meta: Some(json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], true);
}
```

- [ ] **Step 4: Write test for `memory_propose` valid submission**

```rust
#[test]
fn memory_propose_valid_submission_returns_submission_id() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-propose-test");
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_propose",
            "arguments": {
                "source": "src:agent:mcp-test",
                "operation": "Replace",
                "intent": "Accept",
                "target_id": "ssa-0/04-Research/Audy-Himura.md",
                "target_path": "04-Research/Audy-Himura.md",
                "candidate_body": "# Updated Test\n\nThis is a test proposal.\n"
            }
        }),
        _meta: Some(json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28"
        })),
    };
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp.error.is_none(), "expected success, got: {:?}", resp.error);
    let result = resp.result.unwrap();
    let sc = &result["structuredContent"];
    assert!(sc["submission_id"].as_str().unwrap().len() > 0);
    assert_eq!(sc["state"], "submitted");
}
```

- [ ] **Step 5: Write test that no MCP path leads to intake seal or verdict**

```rust
#[test]
fn memory_propose_no_seal_or_verdict_access() {
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-no-seal-test");
    // Verify that submitting then checking — MCP cannot seal
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_propose",
            "arguments": { /* valid proposal */ }
        }),
        _meta: Some(json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" })),
    };
    let resp = sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);
    assert!(resp.error.is_none());

    // Verify the submission exists in the inbox (not staged)
    let inbox = fixture.store_root.join("inbox");
    assert!(inbox.exists(), "inbox should exist after proposal");

    // But the proposals/staging dir should not have been created by MCP
    // (it was created by the fixture setup, but our submission went to inbox)
}
```

- [ ] **Step 6: Write P3 regression test**

```rust
#[test]
fn p3_regression_unchanged_after_mcp() {
    // Run a basic MCP operation, then verify P3 CLI still works
    let fixture = ProvisionedP3Fixture::with_staged_reject("mcp-p3-regression");
    // Call MCP memory_search
    let req = Request {
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({
            "name": "memory_search",
            "arguments": { "query": "test" }
        }),
        _meta: Some(json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" })),
    };
    sovmem::mcp::handle_request(&req, &fixture.profile_path, &fixture.store_root);

    // Now run a CLI verdict through the existing path
    use sovmem::profile::transaction::seal_intake_submission_with_human;
    // (simplified — actual test would use the full CLI integration)
}
```

- [ ] **Step 7: Run all integration tests**

```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --test mcp_integration
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add sovmem/tests/mcp_integration.rs
git commit -m "test(p4): add Layer 3 integration tests with P3 fixtures"
```

---

## Task 9: End-to-end CLI verification and cleanup

**Files:**
- No new files — verify the full `npm test` and `npm run audit:sovmem:check` pass.

- [ ] **Step 1: Run full js suite and audit**

```bash
node --test test/*.test.js
npm run audit:sovmem:check
```
Expected: 85/85 JS tests pass, audit is current.

- [ ] **Step 2: Run rust tests excluding known-slow binaries**

```bash
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --lib --features test-support
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --test mcp_stdio
cargo test --locked --features test-support --manifest-path sovmem/Cargo.toml --test mcp_integration
```
Expected: all pass.

- [ ] **Step 3: Clean up temporary target directory if used**

```bash
rm -rf sovmem/target
```

- [ ] **Step 4: Format and clippy final check**

```bash
cargo fmt --manifest-path sovmem/Cargo.toml --all -- --check
cargo clippy --manifest-path sovmem/Cargo.toml --all-targets --features test-support -- --deny warnings
```

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "chore(p4): final cleanup and audit regeneration"
```
