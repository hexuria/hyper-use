//! Blocking CDP websocket. Plain `ws://` only. `wss://` is refused: this
//! crate does not pull a TLS stack. Chrome's local debugging port is `ws`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

use crate::error::CdpError;
use crate::transport::CdpTransport;

/// Documented default. Pass `--cdp` with no value to use it. A live Chrome
/// must already be listening; hyper-use does not launch a browser.
pub const DEFAULT_CDP_HTTP: &str = "http://127.0.0.1:9222";

pub struct WebSocketTransport {
    socket: WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    next_id: i64,
}

impl WebSocketTransport {
    pub fn connect(endpoint: &str) -> Result<Self, CdpError> {
        let ws_url = resolve_websocket_url(endpoint)?;
        if ws_url.starts_with("wss://") {
            return Err(CdpError::Transport {
                message: "wss is not supported; use a local ws:// debugging port".into(),
            });
        }
        let (socket, _response) =
            tungstenite::connect(&ws_url).map_err(|err| CdpError::Transport {
                message: err.to_string(),
            })?;
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|err| CdpError::Transport {
                    message: err.to_string(),
                })?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|err| CdpError::Transport {
                    message: err.to_string(),
                })?;
        }
        Ok(Self { socket, next_id: 1 })
    }
}

impl CdpTransport for WebSocketTransport {
    fn call(&mut self, method: &str, params_json: &str) -> Result<String, CdpError> {
        let params: Value = serde_json::from_str(params_json).map_err(|err| CdpError::BadJson {
            message: err.to_string(),
        })?;
        let id = self.next_id;
        self.next_id += 1;
        let envelope = json!({"id": id, "method": method, "params": params});
        self.socket
            .send(Message::Text(envelope.to_string().into()))
            .map_err(|err| CdpError::Transport {
                message: err.to_string(),
            })?;
        for _ in 0..64 {
            let message = self.socket.read().map_err(|err| CdpError::Transport {
                message: err.to_string(),
            })?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value =
                serde_json::from_str(text.as_ref()).map_err(|err| CdpError::BadJson {
                    message: err.to_string(),
                })?;
            if value.get("id").and_then(Value::as_i64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("CDP error")
                    .to_owned();
                return Err(CdpError::Protocol { message });
            }
            let result = value.get("result").cloned().unwrap_or(Value::Null);
            return Ok(result.to_string());
        }
        Err(CdpError::Transport {
            message: format!("no CDP result for `{method}` after 64 messages"),
        })
    }
}

fn resolve_websocket_url(endpoint: &str) -> Result<String, CdpError> {
    if endpoint.starts_with("ws://") || endpoint.starts_with("wss://") {
        return Ok(endpoint.to_owned());
    }
    let http = endpoint.trim_end_matches('/');
    if !http.starts_with("http://") {
        return Err(CdpError::Transport {
            message: format!("CDP endpoint `{endpoint}` must be http:// or ws://"),
        });
    }
    let body = http_get(&format!("{http}/json/version"))?;
    let value: Value = serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
        message: err.to_string(),
    })?;
    value
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| CdpError::Transport {
            message: "json/version has no webSocketDebuggerUrl".into(),
        })
}

fn http_get(url: &str) -> Result<String, CdpError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| CdpError::Transport {
            message: format!("unsupported url `{url}`"),
        })?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path}");
    let mut stream = TcpStream::connect(host).map_err(|err| CdpError::Transport {
        message: err.to_string(),
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| CdpError::Transport {
            message: err.to_string(),
        })?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|err| CdpError::Transport {
            message: err.to_string(),
        })?;
    // Chrome often leaves this socket open. Reading to EOF hits the timeout,
    // and Linux reports that as EAGAIN. Stop at Content-Length instead.
    let (headers, mut body) = read_http(&mut stream)?;
    let headers_lower = headers.to_ascii_lowercase();
    if headers_lower.contains("transfer-encoding: chunked") {
        return decode_chunked(&String::from_utf8_lossy(&body));
    }
    if let Some(length) = content_length(&headers_lower) {
        while body.len() < length {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).map_err(|err| CdpError::Transport {
                message: err.to_string(),
            })?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        body.truncate(length);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn read_http(stream: &mut TcpStream) -> Result<(String, Vec<u8>), CdpError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).map_err(|err| CdpError::Transport {
            message: err.to_string(),
        })?;
        if n == 0 {
            return Err(CdpError::Transport {
                message: "HTTP response ended before headers".into(),
            });
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(split) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buf[..split]).into_owned();
            let body = buf[split + 4..].to_vec();
            return Ok((headers, body));
        }
        if buf.len() > 65_536 {
            return Err(CdpError::Transport {
                message: "HTTP headers are too large".into(),
            });
        }
    }
}

fn content_length(headers_lower: &str) -> Option<usize> {
    for line in headers_lower.lines() {
        let Some(value) = line.strip_prefix("content-length:") else {
            continue;
        };
        return value.trim().parse().ok();
    }
    None
}

fn decode_chunked(body: &str) -> Result<String, CdpError> {
    let mut rest = body;
    let mut out = String::new();
    loop {
        let (size_line, after) = rest.split_once("\r\n").ok_or_else(|| CdpError::Transport {
            message: "truncated chunked body".into(),
        })?;
        let size =
            usize::from_str_radix(size_line.trim(), 16).map_err(|err| CdpError::Transport {
                message: err.to_string(),
            })?;
        if size == 0 {
            return Ok(out);
        }
        if after.len() < size {
            return Err(CdpError::Transport {
                message: "short HTTP chunk".into(),
            });
        }
        out.push_str(&after[..size]);
        rest = &after[size..];
        if let Some(stripped) = rest.strip_prefix("\r\n") {
            rest = stripped;
        }
    }
}
