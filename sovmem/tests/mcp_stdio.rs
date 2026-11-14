use sovmem::mcp::{handle_request, read_frame, write_frame, Request, Response};
use std::io::{BufRead, BufReader, BufWriter, Write};

/// Simulate reading a JSON-RPC request from a byte buffer.
fn read_frame_from_bytes(data: &[u8]) -> serde_json::Value {
    let mut cursor = std::io::Cursor::new(data);
    let mut reader = BufReader::new(&mut cursor);
    let mut line = String::new();
    reader.read_line(&mut line).expect("read line");
    serde_json::from_str(&line).expect("parse JSON")
}

#[test]
fn reads_newline_delimited_json_rpc() {
    let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"tools/list\",\"id\":1}\n";
    let value = read_frame_from_bytes(input);
    assert_eq!(value["method"], "tools/list");
    assert_eq!(value["id"], 1);
}

#[test]
fn eof_returns_error() {
    let input = b"";
    let mut reader = BufReader::new(&input[..]);
    let result = read_frame(&mut reader);
    assert!(result.is_err());
}

#[test]
fn malformed_json_does_not_crash_handler() {
    let input = b"{\"jsonrpc\":\"2.0\",\"method\":\n"; // truncated JSON
    let mut reader = BufReader::new(&input[..]);
    let line = read_frame(&mut reader).unwrap();
    let req: Result<Request, _> = serde_json::from_str(&line);
    assert!(req.is_err());
}

#[test]
fn broken_pipe_on_write_returns_error() {
    struct BrokenWriter;
    impl Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "broken pipe",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = BufWriter::new(BrokenWriter);
    let resp = Response::success(Some(serde_json::json!(1)), serde_json::json!({}));
    assert!(write_frame(&mut writer, &resp).is_err());
}

#[test]
fn multiple_requests_processed_sequentially() {
    let json1 = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\",\"params\":{},\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\"}}\n";
    let json2 = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"server/discover\",\"params\":{},\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\"}}\n";
    let mut combined: Vec<u8> = Vec::new();
    combined.extend_from_slice(json1);
    combined.extend_from_slice(json2);

    let mut reader = BufReader::new(&combined[..]);
    let mut output: Vec<u8> = Vec::new();

    for _ in 0..2 {
        let line = read_frame(&mut reader).unwrap();
        let req: Request = serde_json::from_str(&line).unwrap();
        let profile = std::path::Path::new("");
        let store = std::path::Path::new("");
        let resp = handle_request(&req, profile, store);
        let json = serde_json::to_string(&resp).unwrap();
        writeln!(&mut output, "{json}").unwrap();
    }

    let output_str = String::from_utf8(output).unwrap();
    assert!(output_str.contains("\"id\":1"));
    assert!(output_str.contains("\"id\":2"));
}
