//! Newline-delimited JSON-RPC 2.0.
//!
//! stdout carries only protocol messages. This module does not speak
//! `Content-Length` framing and does not accept JSON-RPC batches. Those are
//! documented absences, not silent successes.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::error::ToolError;
use crate::server::Server;
use crate::TOOLS;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Read stdin until EOF. Write one JSON line per response. Notifications and
/// blank lines produce nothing.
pub fn serve_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut server = Server::new();
    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(response) = server.handle_line(&line) {
            stdout.write_all(response.as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// One inbound line against a fresh [`Server`]. `None` means the client must
/// not be answered. Use [`Server::handle_line`] to keep state between lines.
pub fn handle_line(line: &str) -> Option<String> {
    Server::new().handle_line(line)
}

pub(crate) fn handle_line_with(server: &mut Server, line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            return Some(rpc_error(
                Value::Null,
                -32700,
                "parse error",
                json!({"variant": "ParseError"}),
            ))
        }
    };
    if !value.is_object() {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "invalid request",
            json!({"variant": "InvalidRequest"}),
        ));
    }
    dispatch_object(server, &value)
}

fn dispatch_object(server: &mut Server, value: &Value) -> Option<String> {
    let id = value.get("id").cloned();
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "invalid request",
            json!({"variant": "InvalidRequest"}),
        ));
    }
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Some(rpc_error(
            id.unwrap_or(Value::Null),
            -32600,
            "invalid request",
            json!({"variant": "InvalidRequest"}),
        ));
    };
    let id = id?;
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => rpc_ok(id, initialize_result()),
        "tools/list" => rpc_ok(id, tools_list()),
        "tools/call" => match tools_call(server, &params) {
            Ok(result) => rpc_ok(id, result),
            Err(err) => rpc_error(id, -32602, &err.to_string(), err.to_value()),
        },
        "ping" => rpc_ok(id, json!({})),
        other => rpc_error(
            id,
            -32601,
            "method not found",
            json!({"variant": "MethodNotFound", "method": other}),
        ),
    })
}

fn tools_call(server: &mut Server, params: &Value) -> Result<Value, ToolError> {
    if !params.is_object() && !params.is_null() {
        return Err(ToolError::InvalidArguments(
            "tools/call params must be an object".into(),
        ));
    }
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or(ToolError::MissingToolName)?;
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    match server.call_tool(name, &arguments) {
        Ok(body) => Ok(tool_result(body.to_string(), false)),
        Err(err) => Ok(tool_result(err.to_json_string(), true)),
    }
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "hyper-use", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "hyper-use resolves one computer target and can act on its region id. It does not choose the next agent capability. JEV does. Never guess coordinates. A confidence below 0.55 is a result with executed false, not a click. There is no navigate tool. Selecting matcher hgra is not a benchmark."
    })
}

fn tools_list() -> Value {
    json!({
        "tools": TOOLS.iter().copied().map(tool_spec).collect::<Vec<_>>(),
    })
}

fn tool_spec(name: &str) -> Value {
    let (description, properties, required) = match name {
        "observe" => (
            "Read a CDP fixture or an optional live CDP endpoint into regions. Returns id, role, label, and a snapshot id for diff. Does not click and does not choose the next capability.",
            source_props(),
            Vec::<&str>::new(),
        ),
        "locate" => (
            "Rank regions for one query. Default matcher is weighted. matcher hgra selects the hyperdimensional ranker and is not a benchmark and not a measured win. Returns the top target id, role, label, and confidence. Does not click.",
            locate_props(),
            Vec::new(),
        ),
        "inspect" => (
            "Return one region by id, including its rectangle. The rectangle is descriptive. Do not click those coordinates. Act on the region id.",
            {
                let mut props = source_props();
                props.insert("region".into(), json!({"type": "string"}));
                props
            },
            vec!["region"],
        ),
        "act" => (
            "Press one region id. The default executor is the CDP browser press, which prefers a DOM click over coordinates. executor browser-use and executor cua each hand the located region id, role, and label to a replay transport. Neither is in the default policy order, neither changes the page, and neither is a fusion benchmark. A confidence below 0.55 returns executed false and does not click. Pass the locate top confidence as confidence and candidates[1] as runner_up; a gap below 0.05 returns executed false with fallback ambiguous. Omit confidence only when the region was already inspected. Or pass the locate text, role, and position instead of confidence: act ranks its own observation, derives confidence and runner_up, and refuses with TargetNotTop if region is not first. With expect_text or expect_absent, or on a cdp session, act observes again and returns state_delta, verified, before_snapshot, and after_snapshot. Does not take x or y.",
            {
                let mut props = source_props();
                props.insert("region".into(), json!({"type": "string"}));
                props.insert(
                    "action".into(),
                    json!({"type": "string", "enum": ["press", "click"]}),
                );
                props.insert("confidence".into(), json!({"type": "number"}));
                props.insert(
                    "runner_up".into(),
                    json!({
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "confidence": {"type": "number"}
                        },
                        "required": ["id", "confidence"],
                        "description": "The second locate candidate. Requires confidence."
                    }),
                );
                props.insert("text".into(), json!({"type": "string"}));
                props.insert("role".into(), json!({"type": "string"}));
                props.insert("position".into(), json!({"type": "string"}));
                props.insert(
                    "matcher".into(),
                    json!({"type": "string", "enum": ["weighted", "hgra"]}),
                );
                props.insert("expect_text".into(), json!({"type": "string"}));
                props.insert("expect_absent".into(), json!({"type": "string"}));
                props.insert(
                    "observe_after".into(),
                    json!({"type": "boolean", "description": "Observe after the press and diff. Defaults to true with an expectation or a cdp session."}),
                );
                props.insert(
                    "executor".into(),
                    json!({
                        "type": "string",
                        "enum": ["browser", "browser-use", "macos", "cua"],
                        "description": "Default browser is the CDP press. browser-use and cua are opt-in semantic replays and are not fallbacks. macos is not implemented."
                    }),
                );
                props
            },
            vec!["region"],
        ),
        "diff" => (
            "Id-level difference of two observations, given as before/after fixture paths or as before_snapshot/after_snapshot ids returned by earlier calls on this server. Returns state_delta added, removed, changed, moved, text_changed, focus_changed, and url_changed. Does not click.",
            {
                let mut props = serde_json::Map::new();
                props.insert("before".into(), json!({"type": "string"}));
                props.insert("after".into(), json!({"type": "string"}));
                props.insert("before_snapshot".into(), json!({"type": "integer"}));
                props.insert("after_snapshot".into(), json!({"type": "integer"}));
                props
            },
            Vec::new(),
        ),
        "verify" => (
            "Check one postcondition: expect_text appeared, or expect_absent is gone. Does not click and does not plan how to get there.",
            {
                let mut props = source_props();
                props.insert("expect_text".into(), json!({"type": "string"}));
                props.insert("expect_absent".into(), json!({"type": "string"}));
                props
            },
            Vec::new(),
        ),
        _ => ("unsupported", serde_json::Map::new(), Vec::new()),
    };
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn source_props() -> serde_json::Map<String, Value> {
    let mut props = serde_json::Map::new();
    props.insert(
        "fixture".into(),
        json!({"type": "string", "description": "Path to a CDP replay script or a manifold fixture"}),
    );
    props.insert(
        "cdp".into(),
        json!({"type": "string", "description": "Optional live CDP HTTP endpoint. Not required for tests."}),
    );
    props
}

fn locate_props() -> serde_json::Map<String, Value> {
    let mut props = source_props();
    props.insert("text".into(), json!({"type": "string"}));
    props.insert("role".into(), json!({"type": "string"}));
    props.insert("position".into(), json!({"type": "string"}));
    props.insert("action".into(), json!({"type": "string"}));
    props.insert(
        "matcher".into(),
        json!({
            "type": "string",
            "enum": ["weighted", "hgra"],
            "description": "Default weighted. hgra is not a benchmark."
        }),
    );
    props.insert(
        "dims".into(),
        json!({"type": "integer", "enum": [512, 1024, 2048, 4096]}),
    );
    props
}

fn rpc_ok(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn rpc_error(id: Value, code: i64, message: &str, data: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message, "data": data}
    })
    .to_string()
}
