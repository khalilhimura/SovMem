#[cfg(test)]
pub mod client;
pub mod redact;
pub mod tools;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::thread;

use crate::mcp::tools::{all_tools, ToolResult};

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
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl Response {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: &str, data: Option<Value>) -> Self {
        Response {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data,
            }),
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

/// Dispatch a parsed JSON-RPC request to the appropriate handler.
pub fn handle_request(req: &Request, profile_path: &Path, store_root: &Path) -> Response {
    // Handle notifications (they have no id and expect no response)
    if req.id.is_none() {
        if req.method == "notifications/cancelled" {
            // Cancellation is best-effort; the server continues processing
            return Response {
                jsonrpc: "2.0",
                id: None,
                result: None,
                error: None,
            };
        }
        return Response {
            jsonrpc: "2.0",
            id: None,
            result: None,
            error: None,
        };
    }

    // Validate protocol version if present
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
        "tools/call" => handle_tools_call(req, profile_path, store_root),
        "initialize" => handle_initialize(req),
        "notifications/initialized" => {
            // Standard MCP initialized notification; no response needed
            Response {
                jsonrpc: "2.0",
                id: None,
                result: None,
                error: None,
            }
        }
        _ => Response::error(
            req.id.clone(),
            -32602,
            &format!("Unknown method: {}", req.method),
            None,
        ),
    }
}

fn handle_discover(req: &Request) -> Response {
    Response::success(
        req.id.clone(),
        json!({
            "resultType": "complete",
            "supportedVersions": ["2026-07-28"],
            "serverInfo": {
                "name": "sovmem",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "tools": {}
            }
        }),
    )
}

/// Handle the standard MCP `initialize` lifecycle method.
/// This enables compatibility with standard MCP clients that begin the
/// lifecycle with `initialize` rather than the custom `server/discover`.
fn handle_initialize(req: &Request) -> Response {
    Response::success(
        req.id.clone(),
        json!({
            "protocolVersion": "2026-07-28",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "sovmem",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(req: &Request) -> Response {
    let tools: Vec<Value> = all_tools()
        .into_iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();

    Response::success(
        req.id.clone(),
        json!({
            "resultType": "complete",
            "tools": tools,
            "ttlMs": 300_000,
            "cacheScope": "public"
        }),
    )
}

fn handle_tools_call(req: &Request, profile_path: &Path, store_root: &Path) -> Response {
    let tool_name = req
        .params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let empty = json!({});
    let arguments = req.params.get("arguments").unwrap_or(&empty);

    match tool_name {
        "memory_search" => {
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let top_k = arguments.get("top_k").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let expand = arguments
                .get("expand")
                .and_then(|v| v.as_str())
                .unwrap_or("none");
            let pb_profile = profile_path.to_path_buf();
            let pb_store = store_root.to_path_buf();
            let result = tools::handle_memory_search(&pb_profile, &pb_store, query, top_k, expand);
            tool_result_to_response(req, result)
        }
        "memory_get" => {
            let id = arguments.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let include_body = arguments
                .get("include_body")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let pb_profile = profile_path.to_path_buf();
            let pb_store = store_root.to_path_buf();
            let result = tools::handle_memory_get(&pb_profile, &pb_store, id, include_body);
            tool_result_to_response(req, result)
        }
        "memory_propose" => {
            let source = arguments
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let operation = arguments
                .get("operation")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let intent = arguments
                .get("intent")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target_id = arguments
                .get("target_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target_path = arguments
                .get("target_path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let candidate_body = arguments
                .get("candidate_body")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let confidence = arguments.get("confidence").and_then(|v| v.as_f64());
            let evaluator = arguments.get("evaluator").cloned();
            let pb_profile = profile_path.to_path_buf();
            let pb_store = store_root.to_path_buf();
            let result = tools::handle_memory_propose(
                &pb_profile,
                &pb_store,
                source,
                operation,
                intent,
                target_id,
                target_path,
                candidate_body,
                confidence,
                evaluator,
            );
            tool_result_to_response(req, result)
        }
        "memory_status" => {
            let pb_profile = profile_path.to_path_buf();
            let pb_store = store_root.to_path_buf();
            let result = tools::handle_memory_status(&pb_profile, &pb_store);
            tool_result_to_response(req, result)
        }
        _ => Response::error(
            req.id.clone(),
            -32602,
            &format!("Unknown tool: {tool_name}"),
            None,
        ),
    }
}

#[allow(dead_code)]
fn tool_result_to_response(req: &Request, result: ToolResult) -> Response {
    match result {
        ToolResult::Success { text, structured } => Response::success(
            req.id.clone(),
            json!({
                "resultType": "complete",
                "content": [{ "type": "text", "text": text }],
                "structuredContent": structured
            }),
        ),
        ToolResult::Error { code: _, message } => Response::success(
            req.id.clone(),
            json!({
                "resultType": "complete",
                "content": [{ "type": "text", "text": message }],
                "isError": true
            }),
        ),
    }
}

pub fn run_server(profile_path: PathBuf, store_root: PathBuf) -> i32 {
    eprintln!(
        "{}",
        serde_json::json!({
            "code": "mcp.server.starting",
            "severity": "info",
            "message": "MCP server starting"
        })
    );
    let handle = thread::spawn(move || server_loop(&profile_path, &store_root));
    match handle.join() {
        Ok(code) => code,
        Err(_) => {
            eprintln!(
                "{}",
                serde_json::json!({
                    "code": "mcp.server.panic",
                    "severity": "error",
                    "message": "MCP server thread panicked"
                })
            );
            1
        }
    }
}

fn server_loop(profile_path: &Path, store_root: &Path) -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());

    loop {
        let line = match read_frame(&mut reader) {
            Ok(line) => line,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return 0,
            Err(_) => return 1,
        };

        // Parse and handle in a catch_unwind to prevent panics from taking down the server
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || match serde_json::from_str::<Request>(&line) {
                    Ok(req) => handle_request(&req, profile_path, store_root),
                    Err(_) => Response::error(None, -32602, "Malformed JSON-RPC request", None),
                },
            ));

        let response = match result {
            Ok(response) => response,
            Err(_) => {
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "code": "mcp.server.internal",
                        "severity": "error",
                        "message": "Internal server error"
                    })
                );
                return 1;
            }
        };

        if let Err(_e) = write_frame(&mut writer, &response) {
            return 1;
        }
    }
}
