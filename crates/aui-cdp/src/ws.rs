//! Blocking CDP websocket. Plain `ws://` only. `wss://` is refused: this
//! crate does not pull a TLS stack. Chrome's local debugging port is `ws`.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::{Message, WebSocket};

use crate::{CdpError, CdpTransport};

/// Documented default. Pass `--cdp` with no value to use it. A live Chrome
/// must already be listening; ultra-instinct does not launch a browser.
pub const DEFAULT_CDP_HTTP: &str = "http://127.0.0.1:9222";

/// Maximum time to wait for a CDP response after sending a call.
const CALL_DEADLINE: Duration = Duration::from_secs(30);

type Socket = WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

#[derive(Clone)]
struct OwnedTarget {
    http: String,
    target_id: String,
}

pub struct WebSocketTransport {
    socket: Socket,
    next_id: i64,
    owned: Option<OwnedTarget>,
}

impl WebSocketTransport {
    pub fn connect(endpoint: &str) -> Result<Self, CdpError> {
        let ws_url = resolve_websocket_url(endpoint)?;
        Ok(Self {
            socket: connect_socket(&ws_url)?,
            next_id: 1,
            owned: None,
        })
    }

    fn open_tab_inner(endpoint: &str) -> Result<Self, CdpError> {
        let http = http_endpoint(endpoint)?.to_owned();
        let version = http_get(&format!("{http}/json/version"))?;
        let browser_url = version_websocket_url(&version)?;
        let mut browser = Self {
            socket: connect_socket(&browser_url)?,
            next_id: 1,
            owned: None,
        };
        let result = browser.call(
            "Target.createTarget",
            &json!({"url": "about:blank", "background": true}).to_string(),
        )?;
        let target_id = created_target_id(&result)?;
        drop(browser);

        let owned = OwnedTarget {
            http: http.clone(),
            target_id: target_id.clone(),
        };
        let result: Result<Self, CdpError> = (|| {
            let page_url = page_socket_url(&http, &target_id)?;
            let mut transport = Self {
                socket: connect_socket(&page_url)?,
                next_id: 1,
                owned: None,
            };
            transport.call(
                "Emulation.setFocusEmulationEnabled",
                &json!({"enabled": true}).to_string(),
            )?;
            transport.owned = Some(owned.clone());
            Ok(transport)
        })();
        if result.is_err() {
            close_owned_target(&owned);
        }
        result
    }

    pub fn target_id(&self) -> Option<&str> {
        self.owned.as_ref().map(|target| target.target_id.as_str())
    }
}

pub fn open_tab(endpoint: &str) -> Result<WebSocketTransport, CdpError> {
    WebSocketTransport::open_tab_inner(endpoint)
}

impl Drop for WebSocketTransport {
    fn drop(&mut self) {
        if let Some(target) = self.owned.take() {
            close_owned_target(&target);
        }
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

fn connect_socket(ws_url: &str) -> Result<Socket, CdpError> {
    if ws_url.starts_with("wss://") {
        return Err(CdpError::Transport {
            message: "wss is not supported; use a local ws:// debugging port".into(),
        });
    }
    if !ws_url.starts_with("ws://") {
        return Err(CdpError::Transport {
            message: format!("websocket URL `{ws_url}` must use ws://"),
        });
    }
    let (socket, _response) = tungstenite::connect(ws_url).map_err(|err| CdpError::Transport {
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
    Ok(socket)
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
    first_page_websocket_url(&http_get(&format!("{http}/json/list"))?)
}

/// First `"type": "page"` target's websocket URL in a `/json/list` body.
fn first_page_websocket_url(body: &str) -> Result<String, CdpError> {
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

fn http_endpoint(endpoint: &str) -> Result<&str, CdpError> {
    let http = endpoint.trim_end_matches('/');
    if !http.starts_with("http://") || http.len() == "http://".len() {
        return Err(CdpError::Transport {
            message: "open_tab needs the http:// debugging endpoint".into(),
        });
    }
    Ok(http)
}

fn version_websocket_url(body: &str) -> Result<String, CdpError> {
    let value: Value = serde_json::from_str(body).map_err(|err| CdpError::BadJson {
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

fn created_target_id(body: &str) -> Result<String, CdpError> {
    let value: Value = serde_json::from_str(body).map_err(|err| CdpError::BadJson {
        message: err.to_string(),
    })?;
    value
        .get("targetId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| CdpError::Transport {
            message: "Target.createTarget result has no targetId".into(),
        })
}

fn page_socket_url(endpoint: &str, target_id: &str) -> Result<String, CdpError> {
    let http = http_endpoint(endpoint)?;
    let host = http
        .strip_prefix("http://")
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    if host.is_empty() {
        return Err(CdpError::Transport {
            message: "open_tab needs an http:// debugging endpoint".into(),
        });
    }
    Ok(format!("ws://{host}/devtools/page/{target_id}"))
}

fn close_owned_target(target: &OwnedTarget) {
    let result = (|| {
        let version = http_get(&format!("{}/json/version", target.http))?;
        let browser_url = version_websocket_url(&version)?;
        let socket = connect_socket(&browser_url)?;
        let mut browser = WebSocketTransport {
            socket,
            next_id: 1,
            owned: None,
        };
        browser.call(
            "Target.closeTarget",
            &json!({"targetId": target.target_id}).to_string(),
        )?;
        Ok::<(), CdpError>(())
    })();
    let _ = result;
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
            first_page_websocket_url(body),
            Ok("ws://127.0.0.1:9333/devtools/page/A".to_owned())
        );
    }

    #[test]
    fn json_list_without_a_page_target_is_a_transport_error() {
        let body = r#"[{"type": "service_worker", "webSocketDebuggerUrl": "ws://x/sw"}, {"type": "page"}]"#;
        assert_eq!(
            first_page_websocket_url(body),
            Err(CdpError::Transport {
                message: "json/list has no page target with a webSocketDebuggerUrl".into(),
            })
        );
        assert!(matches!(
            first_page_websocket_url("{}"),
            Err(CdpError::Transport { .. })
        ));
        assert!(matches!(
            first_page_websocket_url("not json"),
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

    #[test]
    fn json_version_resolves_browser_websocket_url() {
        assert_eq!(
            version_websocket_url(
                r#"{"Browser":"Chrome","webSocketDebuggerUrl":"ws://127.0.0.1:9333/devtools/browser/BROWSER"}"#
            ),
            Ok("ws://127.0.0.1:9333/devtools/browser/BROWSER".to_owned())
        );
    }

    #[test]
    fn json_version_without_websocket_url_is_a_transport_error() {
        assert_eq!(
            version_websocket_url(r#"{"Browser":"Chrome"}"#),
            Err(CdpError::Transport {
                message: "json/version has no webSocketDebuggerUrl".into(),
            })
        );
    }

    #[test]
    fn page_socket_url_uses_http_endpoint_host_and_target_id() {
        assert_eq!(
            page_socket_url("http://127.0.0.1:9333", "TARGET"),
            Ok("ws://127.0.0.1:9333/devtools/page/TARGET".to_owned())
        );
    }

    #[test]
    fn open_tab_refuses_non_http_endpoints_without_a_socket() {
        for endpoint in [
            "ws://127.0.0.1:9333/devtools/page/TARGET",
            "ftp://127.0.0.1",
        ] {
            let error = match open_tab(endpoint) {
                Err(error) => error,
                Ok(_) => panic!("open_tab must refuse non-http endpoints"),
            };
            assert_eq!(
                error,
                CdpError::Transport {
                    message: "open_tab needs the http:// debugging endpoint".into(),
                }
            );
        }
    }
}
