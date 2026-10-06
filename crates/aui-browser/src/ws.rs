//! Blocking CDP websocket. Plain `ws://` only. `wss://` is refused: this
//! crate does not pull a TLS stack. Chrome's local debugging port is `ws`.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

use crate::error::CdpError;
use crate::transport::CdpTransport;

/// Documented default. Pass `--cdp` with no value to use it. A live Chrome
/// must already be listening; ultra-instinct does not launch a browser.
pub const DEFAULT_CDP_HTTP: &str = "http://127.0.0.1:9222";

/// Maximum time to wait for a CDP response after sending a call.
const CALL_DEADLINE: Duration = Duration::from_secs(30);

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
        let deadline = Instant::now() + CALL_DEADLINE;
        loop {
            if Instant::now() >= deadline {
                return Err(call_deadline_error(method));
            }
            let message = match self.socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if Instant::now() >= deadline {
                        return Err(call_deadline_error(method));
                    }
                    continue;
                }
                Err(err) => {
                    return Err(CdpError::Transport {
                        message: err.to_string(),
                    });
                }
            };
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
    }
}

fn call_deadline_error(method: &str) -> CdpError {
    CdpError::Transport {
        message: format!(
            "no CDP result for `{method}` within {} s",
            CALL_DEADLINE.as_secs()
        ),
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
    // `/json/version` names the browser target, which has no `Page` domain.
    // The tools need a page, so pick the first page target from `/json/list`.
    page_websocket_url(&http_get(&format!("{http}/json/list"))?)
}

/// First `"type": "page"` target's websocket URL in a `/json/list` body.
fn page_websocket_url(body: &str) -> Result<String, CdpError> {
    let value: Value = serde_json::from_str(body).map_err(|err| CdpError::BadJson {
        message: err.to_string(),
    })?;
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|target| target.get("type").and_then(Value::as_str) == Some("page"))
        .find_map(|target| target.get("webSocketDebuggerUrl").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| CdpError::Transport {
            message: "json/list has no page target with a webSocketDebuggerUrl".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_endpoint_resolves_to_the_first_page_target_not_the_browser() {
        // Shape of Chrome's `/json/list`: service workers and iframes can come
        // before the page, and the browser target is not listed at all.
        let body = r#"[
            {"type": "service_worker", "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/page/SW"},
            {"type": "page", "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/page/A"},
            {"type": "page", "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/page/B"}
        ]"#;
        assert_eq!(
            page_websocket_url(body),
            Ok("ws://127.0.0.1:9333/devtools/page/A".to_owned())
        );
    }

    #[test]
    fn json_list_without_a_page_target_is_a_transport_error() {
        let body = r#"[{"type": "service_worker", "webSocketDebuggerUrl": "ws://x/sw"}, {"type": "page"}]"#;
        assert_eq!(
            page_websocket_url(body),
            Err(CdpError::Transport {
                message: "json/list has no page target with a webSocketDebuggerUrl".into(),
            })
        );
        assert!(matches!(
            page_websocket_url("{}"),
            Err(CdpError::Transport { .. })
        ));
        assert!(matches!(
            page_websocket_url("not json"),
            Err(CdpError::BadJson { .. })
        ));
    }

    #[test]
    fn refused_endpoints_are_transport_errors_without_a_socket() {
        let err = match WebSocketTransport::connect("wss://127.0.0.1/devtools/browser") {
            Err(err) => err,
            Ok(_) => panic!("wss must not open a socket"),
        };
        assert_eq!(
            err,
            CdpError::Transport {
                message: "wss is not supported; use a local ws:// debugging port".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "CDP transport: wss is not supported; use a local ws:// debugging port"
        );

        let err = match WebSocketTransport::connect("ftp://127.0.0.1/json") {
            Err(err) => err,
            Ok(_) => panic!("ftp must not open a socket"),
        };
        assert_eq!(
            err,
            CdpError::Transport {
                message: "CDP endpoint `ftp://127.0.0.1/json` must be http:// or ws://".into(),
            }
        );
    }
}
