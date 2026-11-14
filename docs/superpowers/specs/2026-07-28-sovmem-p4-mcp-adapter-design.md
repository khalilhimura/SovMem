# SovMem P4: MCP Adapter Design

**Date:** 2026-07-28  
**Status:** Draft — not yet reviewed or approved  
**Phase:** P4 (MCP Adapter) — design gate  
**Authority:** `docs/sovmem_v0.3_execution_plan.md` §P4, `handover-p4.md`, frozen P0–P3 decisions  
**Normative MCP spec:** [Model Context Protocol 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28)

---

## 1. Scope and explicit non-scope

### In scope

- A single Rust `mod mcp` within the existing `sovmem` crate that implements a **stdio MCP (Model Context Protocol) server** at protocol revision `2026-07-28`.
- Four MCP tools: `memory_search`, `memory_get`, `memory_propose`, `memory_status`.
- Thin synchronous wrappers around existing P0–P3 library functions (query, store, intake, proposal).
- **No new async runtime.** The MCP server runs on a dedicated thread using **synchronous** `BufReader`/`BufWriter` on stdin/stdout. The `2026-07-28` protocol is stateless (no handshake, no session), so per-request blocking I/O is safe and simpler than async.
- **No MCP SDK dependency.** Protocol is implemented directly over JSON-RPC 2.0 using `serde_json` (already in `Cargo.toml`). The framing is trivial: one JSON-RPC message per newline-delimited line over stdin/stdout.
- Deterministic fake-client test suite that exercises every tool, error, and lifecycle condition.
- Redacted diagnostics on stderr. Protocol frames on stdout only (see §7).
- Single-threaded request processing: one request at a time, no concurrent library access.

### Explicit non-scope (P4 stop boundary)

| What | Why |
|------|-----|
| MCP `memory_verdict` tool | Verdicts are human CLI-only (A2, frozen). |
| MCP `memory_intake_seal` tool | Intake sealing is human-only (P3 v2 contract). |
| MCP `memory_write_canon` tool | Canon writes are CLI-only (A2, frozen). |
| MCP `memory_trust` tool | Trust is P5 (handover resolution #1). |
| MCP `memory_policy` tool | Policy mutation is a human git commit. |
| HTTP/SSE/WebSocket transport | stdio only (D6, frozen). |
| Network calls or model calls | Binary makes no network calls (A6, frozen). |
| Async outside `mcp/` | Deterministic core must remain sync (R6, frozen). |
| Embeddings, vectors, Qdrant | P2 baseline passed on lexical+graph only (D2, frozen). |
| Incremental indexing | Full rebuild only (frozen P2 decision). |
| Dependency addition without approval | Each new dep requires explicit pin and approval. |
| Frozen P2 corpus, queries, thresholds, baseline | Must not change. |
| Audy Vault mutation | Read-only for this phase. |
| Arbitrary filesystem access | `memory_get` resolves canonical IDs only, never paths. |

---

## 2. Trust boundaries and threat model

### Authority model

```
+-------------------------------------------------------------+
|                         MCP Client (agent)                    |
|  (Claude Code, OpenClaw, etc.)                               |
+----------------------------+--------------------------------+
                             | stdio stdin/stdout
                             v
+-------------------------------------------------------------+
|  sovmem mcp/ module                  | Deterministic core    |
|  (dedicated thread, sync I/O)        | (sync lib calls)      |
|  - JSON-RPC framing                 | - query_index()        |
|  - input validation / redaction     | - query_store()        |
|  - output serialization             | - read_sealed_pair()   |
|  - error mapping                    | - submit_proposal()    |
|  - cancellation / EOF               | - inspect_store()      |
|                                     | - verify_store()       |
+-------------------------------------+------------------------+
                                        |      ^
                                        v      |
                                   +----+------+----+
                                   |   Filesystem     |
                                   | (store, index)   |
                                   +------------------+
```

### Threat model

| Threat | Mitigation |
|--------|-----------|
| MCP client sends `memory_verdict` | Tool not registered; server returns protocol error `-32602` (unknown tool). |
| MCP client sends crafted `memory_propose` with intake-seal intent | `memory_propose` writes only to untrusted inbox; no seal or verdict path reachable from MCP. |
| MCP client sends forged evaluator or confidence block | Advisory fields passed through unchanged; PolicyEvaluator runs same CLI gate; no authority conveyed. |
| MCP client requests `memory_get` with path traversal | Tool resolves by canonical ID only; IDs validated against known namespace pattern; no path argument accepted. |
| MCP client sends oversized payload | Byte limit enforced before deserialization; diagnostic redacted. |
| MCP client sends malformed JSON-RPC | Framing error; diagnostic on stderr; connection continues. |
| MCP client sends concurrent requests | Sequential processing on single thread; second request waits for first to complete. |
| Race between MCP proposal and CLI seal | Existing P3 v2 lease protocol applies; MCP's `memory_propose` has no lease awareness. |
| MCP client sends fragmented input | JSON-RPC framing reads one line at a time; fragmented JSON is a parse error. |
| Stdout contamination from library code | Library must not `println!`; MCP module owns stdout writes exclusively. All diagnostics on stderr. |
| Stale or corrupt index / store | Tool returns structured error with redacted diagnostics. |
| Eavesdropping on same host | stdio only; localhost trust boundary. No encryption within scope. |

---

## 3. P3 v2 proposal/intake/verdict compatibility

The P3 v2 custody flow as implemented:

1. Agent submits proposal → `memory_propose` → untrusted inbox (`inbox/<submission_id>.json`) + raw candidate (`inbox/<submission_id>.md`).
2. **return** `submission_id` (not a staged `proposal_id`).
3. Human CLI: `sovmem intake seal <submission_id>` → PolicyEvaluator → staged pair (`staging/<proposal_id>.md` + `staging/<proposal_id>.meta.json`).
4. Human CLI: `sovmem verdict <proposal_id> ACCEPT --reason "..."` → canon write, re-index, ledger append.

**MCP may never call steps 3 or 4.**

`memory_propose` maps to `SubmitProposal` — the existing public-submit path. The proposal actor must pass `source`, `operation`, `intent`, `target_id`, `target_path`, and `candidate_path` — plus optional `confidence` and `evaluator` fields. These match the existing `cli/p3.rs::run_propose` calling convention.

The MCP tool must **not** accept `--profile` or `--store` as agent-controlled parameters; they are server startup configuration.

---

## 4. Module boundaries and async confinement

```
sovmem/src/
├── lib.rs             # + pub mod mcp;
├── cli.rs             # unchanged
├── cli/
│   ├── p2.rs          # unchanged
│   └── p3.rs          # unchanged
├── main.rs            # + --serve flag to start MCP server
├── profile/           # unchanged (all sync)
└── mcp/               # NEW — synchronous module on dedicated thread
    ├── mod.rs         # server loop, JSON-RPC dispatch, _meta validation
    ├── tools.rs       # tool definitions and handler implementations
    ├── redact.rs      # diagnostic redaction helpers
    └── client.rs      # (test) fake client for deterministic testing
```

- `mod mcp` runs on a **dedicated thread** with synchronous I/O on stdin/stdout.
- Library calls from `mcp/` into `profile/` are synchronous — no `spawn_blocking` needed since we're already on a worker thread.
- The MCP server is started by `main.rs` when the `--serve` flag is present. It spawns a thread for the server loop and blocks until shutdown.
- `cargo check --lib` (without features) does not compile `mcp/` unless enabled. MCP support is behind a default feature flag `mcp`.

---

## 5. Protocol revision and implementation approach

### Protocol revision

- **MCP spec revision:** `2026-07-28` — stateless; no `initialize`/`initialized` handshake.
- **Framing:** JSON-RPC 2.0 over stdio, one message per newline-delimited line.
- **Messages MUST NOT contain embedded newlines** (per stdio transport spec).

### Why no SDK

The `2026-07-28` MCP protocol is deliberately simple for a tools-only server:

1. **Stateless** — No session, no handshake, no `notifications/initialized`. Every request carries its protocol version in `_meta`.
2. **Single transport** — stdio only. No HTTP, SSE, or WebSocket.
3. **Tools only** — No resources, no prompts, no sampling, no roots, no logging.
4. **Simple framing** — Read a line from stdin, parse JSON-RPC, dispatch, write a line to stdout.
5. **`serde_json` is already a dependency** — No new crate needed.

The implementation uses `serde_json` for JSON-RPC framing, `std::io::BufReader`/`BufWriter` on stdin/stdout, and a dedicated thread. This keeps the crate dependency-free for MCP and avoids SDK churn.

### Fallback (tripwire)

If raw JSON-RPC implementation exceeds **one focused session** (≈2–3 hours), switch to a pinned MCP SDK. No SDK candidate is selected now because none is needed.

### Existing dependencies

All existing dependencies are unchanged. The lockfile is updated only for new additions, and there are none.

---

## 6. Versioned tool contracts

### 6.1 `memory_search`

Search the verified derived Tantivy index.

**Input:**
```json
{
  "query": "string (required, 1-200 chars)",
  "top_k": "number (optional, default 5, max 20)",
  "expand": "string (optional, \"none\" | \"one-hop\", default \"none\")"
}
```

**Output (structured content):**

The tool result wraps structured data in the MCP `structuredContent` field alongside human-readable text:

```json
{
  "resultType": "complete",
  "content": [
    {
      "type": "text",
      "text": "Found 3 results for query 'trust threshold'. Top result: ssa-0/04-Research/trust-threshold.md"
    }
  ],
  "structuredContent": {
    "results": [
      {
        "id": "ssa-0/04-Research/trust-threshold.md",
        "score": 12.45,
        "title": "Trust Threshold Analysis",
        "type": "judgment",
        "layer": "analysis",
        "tags": ["trust", "calibration", "threshold"],
        "snippet": "The trust threshold determines when a source's proposals receive reduced oversight..."
      }
    ],
    "total_hits": 3,
    "generation_id": "gen-a1b2c3d4"
  }
}
```

**Errors:**
| Code | Condition |
|------|-----------|
| `index.missing` | Index does not exist or is unreadable |
| `index.stale` | Index generation does not match current canon generation |
| `query.invalid` | Query is empty, too long, or contains invalid characters |
| `store.unreadable` | Store or generation manifest cannot be read |
| `server.internal` | Unexpected internal error (redacted) |

**Bindings:**
- Binds to the **disposable derived index** at the store path.
- Index is rebuilt from the verified current canonical generation.
- The tool reports `generation_id` so clients can detect staleness.

### 6.2 `memory_get`

Retrieve a single canonical document by its exact canonical ID.

**Input:**
```json
{
  "id": "string (required, canonical document ID)",
  "include_body": "boolean (optional, default false)"
}
```

**Output (success):**

The tool returns structured data with generation metadata:

```json
{
  "resultType": "complete",
  "content": [
    {
      "type": "text",
      "text": "Document: Trust Threshold Analysis\nType: judgment | Layer: analysis\nProvenance: tier1"
    }
  ],
  "structuredContent": {
    "id": "ssa-0/04-Research/trust-threshold.md",
    "title": "Trust Threshold Analysis",
    "type": "judgment",
    "layer": "analysis",
    "tags": ["trust", "calibration", "threshold"],
    "provenance": "tier1",
    "source": "src:vault:audy",
    "created": "2026-06-15",
    "updated": "2026-07-01",
    "body": null,
    "generation_id": "gen-a1b2c3d4"
  }
}
```

**Errors:**
| Code | Condition |
|------|-----------|
| `document.not_found` | Canonical ID does not exist in current generation |
| `id.invalid` | ID fails validation (malformed format, empty, path traversal) |
| `store.unreadable` | Store cannot be read |
| `server.internal` | Unexpected internal error (redacted) |

**Bindings:**
- Resolves only canonical document IDs from the **verified current canonical generation**.
- **No arbitrary path parameter.** The `id` is validated against the `namespace/<path>.md` pattern.
- Body limited to 4000 chars to prevent agent context overflow.

### 6.3 `memory_propose`

Submit an untrusted proposal to the inbox.

**Input:**
```json
{
  "source": "string (required, e.g. \"src:agent:claude-code\")",
  "operation": "string (required, \"Create\" | \"Replace\")",
  "intent": "string (required, \"Accept\" | \"Rewrite\")",
  "target_id": "string (required, canonical document ID to replace, or new path for Create)",
  "target_path": "string (required, relative path in vault, e.g. \"04-Research/topic.md\")",
  "candidate_body": "string (required, full Markdown body of proposed document)",
  "confidence": "number (optional, 0.0–1.0, pre-draft confidence estimate)",
  "evaluator": "object (optional, { agent, recommendation, reasons[] })"
}
```

**Output (success):**

```json
{
  "resultType": "complete",
  "content": [
    {
      "type": "text",
      "text": "Proposal submitted. Submission ID: sub_a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    }
  ],
  "structuredContent": {
    "submission_id": "sub_a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "state": "submitted",
    "warnings": []
  }
}
```

**Errors:**
| Code | Condition |
|------|-----------|
| `proposal.invalid_input` | Required fields missing or malformed |
| `proposal.size_exceeded` | Candidate body exceeds max size (100KB) |
| `proposal.quota_exceeded` | Source has exceeded per-source quota (checked at seal time, warned here) |
| `proposal.store_error` | Inbox write failed |
| `server.internal` | Unexpected internal error (redacted) |

**Bindings:**
- Writes to the **untrusted raw proposal inbox** only.
- Returns `submission_id` — **not** a staged `proposal_id`.
- Candidate body is written to `inbox/<submission_id>.md`.
- Submission metadata is written to `inbox/<submission_id>.json`.
- Policy evaluation (quota check, dedup) is advisory-only at this stage; definitive policy gate occurs at human `intake seal`.
- The `--profile` and `--store` paths are **server configuration**, not agent-controlled parameters.

### 6.4 `memory_status`

Read-only status of the SovMem store, index, and intake queue.

**Input:**
```json
{}
```

**Output:**

```json
{
  "resultType": "complete",
  "content": [
    {
      "type": "text",
      "text": "Store state: sealed | 62 documents | Index: current | Verdicts: 20"
    }
  ],
  "structuredContent": {
    "store": {
      "state": "sealed",
      "canonical_documents": 62,
      "canonical_generation": "gen-a1b2c3d4",
      "index_state": "current",
      "index_generation": "idx-e5f6a7b8",
      "level_4_verified": true
    },
    "intake": {
      "pending_submissions": 0,
      "staged_proposals": 1,
      "ruled_proposals": 20
    },
    "ledger": {
      "total_verdicts": 20,
      "last_verdict_at": "2026-07-28T12:00:00Z"
    },
    "server": {
      "version": "0.1.0",
      "uptime_seconds": 3600
    }
  }
}
```

**Errors:**
| Code | Condition |
|------|-----------|
| `store.unreadable` | Store cannot be inspected |
| `server.internal` | Unexpected internal error (redacted) |

**Bindings:**
- Read-only access to `inspect_store` and `read_intake`.
- No verdict, policy, or trust data is exposed beyond redacted aggregate counts.

---

## 7. Protocol lifecycle and JSON-RPC behavior

### Startup

```
sovmem serve --profile <path> --store <path> [--index <path>]
```

- Parses the profile and validates store accessibility.
- Launches the MCP server thread: reads JSON-RPC lines from stdin, dispatches, writes responses to stdout.
- All startup diagnostics go to stderr.
- Exit code 0 on clean shutdown. Exit code 1 on configuration error (printed to stderr before exit).

### Per-request metadata (`_meta`)

The `2026-07-28` protocol is **stateless** — there is no `initialize`/`notifications/initialized` handshake. Every request carries its protocol version and client capabilities in the `_meta` field:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {},
  "_meta": {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
    "io.modelcontextprotocol/clientCapabilities": {},
    "io.modelcontextprotocol/clientInfo": {
      "name": "claude-code",
      "version": "1.0.0"
    }
  }
}
```

If the client sends an unsupported protocol version, the server responds with:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32022,
    "message": "Unsupported protocol version",
    "data": {
      "supported": ["2026-07-28"],
      "requested": "1900-01-01"
    }
  }
}
```

### Server discovery

Clients **MAY** call `server/discover` before any other request to learn the server's supported protocol versions and capabilities:

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "server/discover",
  "params": {},
  "_meta": {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "resultType": "complete",
    "supportedVersions": ["2026-07-28"],
    "serverInfo": {
      "name": "sovmem",
      "version": "0.1.0"
    },
    "capabilities": {
      "tools": {}
    }
  }
}
```

### Tool list

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": {},
  "_meta": {
    "io.modelcontextprotocol/protocolVersion": "2026-07-28"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "resultType": "complete",
    "tools": [
      {
        "name": "memory_search",
        "description": "Search the SovMem knowledge base",
        "inputSchema": {
          "type": "object",
          "properties": { ... },
          "required": ["query"]
        }
      }
    ],
    "ttlMs": 300000,
    "cacheScope": "public"
  }
}
```

### Tool call lifecycle

1. Client sends `tools/call` request with protocol version in `_meta`.
2. Server validates `_meta` protocol version (rejects with `-32022` if unsupported).
3. Server validates tool name (rejects with `-32602` if unknown).
4. Server validates arguments against `inputSchema`.
5. Server executes the tool (synchronous library call on the MCP thread).
6. Server writes the JSON-RPC response to stdout.

**Error handling follows the MCP `2026-07-28` spec:**
- **Protocol errors** (unknown tool, malformed request, server error) → JSON-RPC error response (code `-32602` or `-32603`).
- **Tool execution errors** (business logic, input validation after dispatch) → result with `isError: true`.
- **All diagnostics** go to stderr as newline-delimited JSON (same format as CLI).

### Shutdown

- On EOF on stdin: flush pending writes, exit 0.
- On SIGTERM/SIGINT: flush pending writes, exit 0.
- On broken pipe during stdout write: exit 1.
- Client may send `notifications/cancelled` referencing a request ID. Server SHOULD stop processing that request if practical.
- Per MCP stdio spec: the client closes stdin, waits for the server to exit, then escalates to SIGTERM/SIGKILL if needed.

### Diagnostics

- Every diagnostic JSON line goes to stderr with the same format used by the CLI:
  `{"code":"mcp.<tool>.<error_code>","severity":"error|warning","message":"..."}`
- Redacted: never echo user input, file paths, or content in diagnostics.
- Protocol bytes are never echoed to stderr.
- Per the MCP stdio spec: stderr is for any logging — informational, debug, error. Clients SHOULD NOT assume stderr output indicates error conditions.

---

## 8. Redaction and stable errors

### Redaction rules

1. Never echo back user-supplied input (query strings, IDs, candidate bodies) in error messages.
2. Never echo filesystem paths in error messages.
3. Use generic codes and messages:
   - `"proposal.invalid_input"` instead of `"target_id ../../etc/passwd is invalid"`
   - `"store.unreadable"` instead of `"/Users/user/.sovmem/store is not readable"`
4. Diagnostic codes are stable and documented. They match the existing CLI diagnostic code pattern.

### Error mapping

Per the `2026-07-28` spec, error reporting uses two mechanisms:

1. **Protocol Errors** (JSON-RPC error codes) — for issues the LLM is unlikely to fix:
   - Unknown tool → `-32602` (Invalid Params)
   - Malformed request (fails `CallToolRequest` schema) → `-32602`
   - Unsupported protocol version → `-32022` (`UnsupportedProtocolVersionError`)
   - Server internal error → `-32603` (Internal error)

2. **Tool Execution Errors** (`isError: true` in result) — for actionable feedback the LLM can self-correct from:
   - Invalid input (empty query, malformed canonical ID, missing required field)
   - Business logic failures (index missing, store corrupted, proposal quota exceeded)
   - Resource not found (document not found by canonical ID)

| Condition | Error type | Code / isError |
|-----------|-----------|----------------|
| Unknown tool name | Protocol error | `-32602` |
| Missing/wrong protocol version | Protocol error | `-32022` |
| Malformed JSON-RPC frame | Protocol error | `-32602` |
| Internal panic or IO error | Protocol error | `-32603` |
| Empty or oversized query | Tool execution error | `isError: true`, code `query.invalid` |
| Index missing or stale | Tool execution error | `isError: true`, code `index.missing` or `index.stale` |
| Store unreadable or corrupted | Tool execution error | `isError: true`, code `store.unreadable` |
| Canonical ID not found | Tool execution error | `isError: true`, code `document.not_found` |
| Malformed ID (path traversal attempt) | Tool execution error | `isError: true`, code `id.invalid` |
| Proposal missing required field | Tool execution error | `isError: true`, code `proposal.invalid_input` |
| Proposal oversized | Tool execution error | `isError: true`, code `proposal.size_exceeded` |

Tool execution error responses look like:

```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "resultType": "complete",
    "content": [
      {
        "type": "text",
        "text": "Index is missing. Run `sovmem index` to rebuild."
      }
    ],
    "isError": true
  }
}
```

---

## 9. Concurrency, cancellation, EOF, crash, and restart behavior

### Concurrency

- The MCP server processes **one request at a time** on its dedicated thread.
- Requests are read and dispatched sequentially from stdin. No concurrent library access.
- Library functions that take leases (propose) are already protected by the P3 lease protocol.
- Read calls (`memory_search`, `memory_get`, `memory_status`) are thread-safe and hold no leases.

### Cancellation

- Client sends `notifications/cancelled` with the request ID to cancel.
- The server checks a cancellation flag between processing stages (read → validate → execute → respond).
- If a synchronous library call is in progress, cancellation is best-effort — the call completes, but the response may be discarded.
- Cancellation of `memory_propose` mid-write may leave a partial inbox file. No recovery action needed at the MCP level.

### EOF and crash

- EOF on stdin: server writes any pending in-flight response, then exits 0.
- Broken pipe on stdout: server exits 1 (no one listening).
- Internal panic: caught by `std::panic::catch_unwind` around the dispatch loop; server exits 1 with a redacted diagnostic on stderr.
- Process crash: stdio closure is visible to the client. The client restarts the server.

### Restart

- MCP server is stateless across restarts. All state lives in the filesystem store.
- Client reconnects by spawning a new `sovmem serve` subprocess. No handshake needed — the first request with valid `_meta` works.

---

## 10. Fixture-first TDD strategy

### Layer 1: Fake client (deterministic, no real MCP transport)

- A `fake_client.rs` module that calls tool handlers directly with JSON-RPC-like structs.
- Tests validate exact input/output schemas, error codes, and redaction.
- Tests are synchronous, no I/O, no separate thread.

### Layer 2: Deterministic stdio simulation

- A test that spawns the server binary with an in-memory store uses a helper that reads/writes JSON-RPC messages via a `BufReader`/`BufWriter` pair (simulating the process stdin/stdout without spawning a subprocess).
- This uses the same framing logic as production but over in-memory channels.
- Tests validate frame boundaries, backpressure, EOF, and shutdown.

### Layer 3: Integration with real store

- Tests that provision a P3 fixture store (`ProvisionedP3Fixture`), start the MCP server against it, and exercise each tool.
- These validate correct bindings between MCP tools and the store/index.
- They re-use the existing P3 fixture infrastructure.

### Specific fixture coverage (at minimum)

| Fixture | Layer | Verifies |
|---------|-------|----------|
| `mcp_server_discover` | 1 | `server/discover` returns supported versions, capabilities |
| `mcp_tools_list` | 1 | `tools/list` returns tool enumeration with JSON Schema |
| `mcp_unsupported_protocol_revision` | 1 | Protocol mismatch rejection (`-32022`) |
| `mcp_malformed_jsonrpc` | 2 | Graceful malformed input handling |
| `mcp_fragmented_input` | 2 | Incomplete frame timeout |
| `mcp_unknown_tool` | 1 | MethodNotFound |
| `mcp_unexpected_field` | 1 | Extra argument ignored or rejected |
| `mcp_oversized_request` | 1 | Size limit enforcement |
| `mcp_oversized_response` | 1 | Body truncation at tool level |
| `mcp_stdout_contamination` | 2 | No stray bytes on stdout |
| `mcp_stderr_redaction` | 2 | No user input or paths in stderr |
| `mcp_eof_shutdown` | 2 | Graceful EOF → exit 0 |
| `mcp_broken_pipe_shutdown` | 2 | Broken pipe → exit 1 |
| `mcp_cancellation` | 2 | Mid-request cancellation |
| `mcp_concurrent_search` | 1 | Serialized synchronous reads |
| `mcp_concurrent_propose` | 1 | One-at-a-time write guard |
| `memory_search_empty_query` | 1 | Invalid query rejection |
| `memory_search_stale_index` | 3 | Stale index diagnostic |
| `memory_search_missing_index` | 3 | Missing index diagnostic |
| `memory_search_normal_results` | 3 | Expected result shape |
| `memory_get_valid_id` | 3 | Correct document retrieval |
| `memory_get_invalid_id` | 1 | Malformed ID rejection |
| `memory_get_path_traversal` | 1 | `../` and `/` traversal blocked |
| `memory_get_symlink_id` | 1 | Symlink in ID rejected |
| `memory_get_nonexistent_id` | 3 | Not found error |
| `memory_get_with_body` | 3 | Body returned under limit |
| `memory_propose_valid` | 3 | Untrusted inbox submission |
| `memory_propose_duplicate` | 3 | Duplicate detection (advisory) |
| `memory_propose_quota_exceeded` | 3 | Quota advisory warning |
| `memory_propose_missing_field` | 1 | Required field validation |
| `memory_propose_oversized_candidate` | 1 | Size limit |
| `memory_propose_confidence_passthrough` | 3 | Confidence field preserved |
| `memory_propose_evaluator_passthrough` | 3 | Evaluator block preserved |
| `memory_propose_no_seal_access` | 3 | No intake seal path reachable |
| `memory_propose_no_verdict_access` | 3 | No verdict path reachable |
| `memory_status_empty_store` | 3 | Correct empty state |
| `memory_status_sealed_store` | 3 | Correct sealed state |
| `memory_status_corrupt_store` | 3 | Corrupt store diagnostic |
| `memory_status_index_missing` | 3 | Index missing diagnostic |
| `p3_regression_intact` | 3 | Existing P3 fixtures still pass |
| `p2_baseline_unchanged` | 3 | P2 retrieval baseline preserved |

---

## 11. Approved live-agent safe-clone ceremony

### Demo flow

1. **Agent connects** to `sovmem serve` via MCP.
2. **Agent calls** `memory_search("trust threshold")` → receives ranked canon results.
3. **Agent calls** `memory_get(id="ssa-0/04-Research/trust-threshold.md")` → receives full document.
4. **Agent proposes** a new judgment via `memory_propose` → receives `submission_id`.
5. **Human** runs `sovmem intake seal <submission_id>` in a separate terminal.
6. **Human** runs `sovmem verdict <proposal_id> ACCEPT --reason "Updated with new insight"`.
7. **Fresh agent session** calls `memory_get(id="...")` → retrieves the improved document and verdict reason.
8. P4 **stops here** — no `sovmem trust` command (that's P5).

### Harness recommendation

**Primary:** Claude Code CLI via the MCP server configuration in `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "sovmem": {
      "command": "sovmem",
      "args": ["serve", "--profile", "/path/to/profile.yaml", "--store", "/path/to/store"]
    }
  }
}
```

**Secondary:** A smoke-test shell script that sends JSON-RPC via `echo` to a running server process, to validate the flow without a full agent harness.

---

## 12. frozen P2 and P3 regression protection

- **No MCP tool** can alter the P2 corpus, golden queries, thresholds, or baseline.
- `memory_search` reads from the derived index only — it never writes to it.
- `memory_get` reads from the canonical store only — it never writes to it.
- `memory_propose` writes only to the untrusted inbox — it never touches staging, canon, or the index.
- All P3 CLI commands (`intake seal`, `verdict`, `recover`, etc.) remain unchanged and exclusively accessible via the CLI.
- The existing P3 lease protocol continues to protect concurrent access — MCP has no lease capability.

---

## 13. Audit-state transition

### Before P4 design approval

- `docs/sovmem-build-state.json`: P4 = 0%, overall ≈ 63%
- `docs/SOVMEM-BUILD-AUDIT.md`: stale — mentions P3 merge (handover note: do not reopen P3)

### After P4 design approval

- No change to build state — only design is committed.

### After P4 implementation + gate

- `docs/sovmem-build-state.json`: P4 = 100%, overall ≈ 78%
- `docs/SOVMEM-BUILD-AUDIT.md`: regenerated with P4 evidence
- `npm run audit:sovmem:check` passes at the reviewed head

---

## 14. Tripwires and stop conditions

| Tripwire | Action |
|----------|--------|
| Raw JSON-RPC implementation exceeds 1 session | Stop; evaluate SDK or simplify scope. |
| Any MCP tool path can reach intake sealing | Stop; redesign authority boundary. |
| Any MCP tool path can issue a verdict | Stop; design defect. |
| Any MCP tool path can write canon | Stop; design defect. |
| Synchronous I/O blocks cause unacceptable latency | Move to dedicated thread with timeout. |
| New dependency without explicit approval | Stop; revert. |
| Binary requires network or model access | Stop; violates A6. |
| Frozen P2 artifact would need alteration | Stop; regression. |
| P3 regression fixture fails | Stop; investigate before MCP changes. |
| Live harness cannot operate against safe clone | Stop; fix harness first. |
| P4/P5 demo conflict unresolved | Stop; resolve scope boundary. |

---

## 15. Future P5 compatibility

The design explicitly avoids steps that would conflict with P5:

- No `trust` command or trust data exposed through MCP.
- No policy mutation through MCP.
- No verdict history exposed through MCP beyond aggregate counts in `memory_status`.
- Ledger format unchanged — P5 trust computation reads from the same `verdicts.jsonl`.
- The `memory_status` output can be extended in P5 to include trust summary without breaking the P4 contract.

---

## 16. Implementation order (plan guidance)

This is the recommended order for the implementation plan:

1. **Module scaffold** — create `mcp/` module with `mod.rs`, `tools.rs`, `redact.rs`.
2. **`server/discover` and `tools/list`** — implement the discovery endpoint and tool enumeration with JSON Schema input schemas.
3. **JSON-RPC framing** — `BufReader`/`BufWriter` on stdin/stdout, newline-delimited JSON-RPC 2.0, `_meta` validation.
4. **`memory_status`** — simplest tool, no arguments, reads store state.
5. **`memory_search`** — wraps `query_store` / `query_index` with argument validation.
6. **`memory_get`** — wraps canon read with ID validation.
7. **`memory_propose`** — wraps `submit_proposal` with argument validation and size limits.
8. **Error handling** — protocol errors vs tool execution errors (`isError: true`), redaction rules.
9. **Shutdown and cancellation** — EOF, SIGTERM, `notifications/cancelled`, broken pipe.
10. **Fake client suite** — deterministic layer 1 tests.
11. **Stdio simulation suite** — layer 2 tests with in-memory channels.
12. **Integration suite** — layer 3 tests with provisioned P3 fixtures.
13. **Safe-clone ceremony** — manual proof using real store (after automated gate passes).
14. **Cleanup and audit update** — target cleanup, final review, audit regeneration.

---

*End of P4 MCP Adapter Design Specification.*
