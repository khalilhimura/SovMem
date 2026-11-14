use serde_json::Value;

/// Ensure error messages never echo user input or filesystem paths.
pub fn redacted_error(code: &'static str, message: &'static str) -> Value {
    serde_json::json!({
        "code": code,
        "severity": "error",
        "message": message
    })
}
