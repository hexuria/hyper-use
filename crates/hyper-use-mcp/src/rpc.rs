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
        "instructions": "hyper-use is an action firewall: observe, guard, verify. It resolves what an agent is about to interact with, refuses ambiguous or unsafe actions, and verifies state change. It does not click. Browser Use (or another host) acts after Allow. Never guess coordinates. There is no navigate tool."
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
            "Read a CDP fixture or an optional live CDP endpoint into regions. Returns id, role, label, state (availability enabled or disabled; visibility visible, occluded, offscreen, or hidden), and a snapshot id for diff. Does not click and does not choose the next capability.",
            source_props(),
            Vec::<&str>::new(),
        ),
        "locate" => (
            "Rank regions for one query. Default matcher is weighted. matcher hgra selects the hyperdimensional ranker and is not a benchmark and not a measured win. Returns the top target id, role, label, and confidence, and candidates with state. signals carries repeated_query when the same query already ran on the same page state, with top and runner_up ids and a suggested_position that separates each; it is data and does not refuse or retry. Does not click.",
            locate_props(),
            Vec::new(),
        ),
        "inspect" => (
            "Return one region by id, including its state and rectangle. The rectangle is descriptive. Do not click those coordinates. Act on the region id.",
            {
                let mut props = source_props();
                props.insert("region".into(), json!({"type": "string"}));
                props
            },
            vec!["region"],
        ),
        "guard" => (
            "Decide allow / refuse / escalate for a proposed click. Never clicks. Pass target (or text), optional role/position, optional proposed region id. Returns decision, evidence, candidates.",
            {
                let mut props = locate_props();
                props.insert(
                    "target".into(),
                    json!({
                        "type": "string",
                        "description": "Visible label to resolve (alias of text)."
                    }),
                );
                props.insert(
                    "proposed".into(),
                    json!({
                        "type": "string",
                        "description": "Optional region id that must be the top match."
                    }),
                );
                props.insert(
                    "region".into(),
                    json!({
                        "type": "string",
                        "description": "Deprecated alias of proposed."
                    }),
                );
                props
            },
            Vec::new(),
        ),
        "act" => (
            "Deprecated alias of guard. Returns the same decision and never clicks.",
            {
                let mut props = locate_props();
                props.insert(
                    "target".into(),
                    json!({
                        "type": "string",
                        "description": "Visible label to resolve (alias of text)."
                    }),
                );
                props.insert(
                    "proposed".into(),
                    json!({
                        "type": "string",
                        "description": "Optional region id that must be the top match."
                    }),
                );
                props.insert(
                    "region".into(),
                    json!({
                        "type": "string",
                        "description": "Deprecated alias of proposed."
                    }),
                );
                props
            },
            Vec::new(),
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
