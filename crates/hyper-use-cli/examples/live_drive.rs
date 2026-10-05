//! Manual, opt-in live drive: JEV (System One) chooses hyper-use MCP calls
//! against a throwaway local Chrome. Not CI, not a benchmark.
//!
//! JEV answers named choice questions about a JSON state. It does not run a
//! tool loop of its own, so this harness is the loop: each step it asks JEV
//! which tool to call, then asks for that tool's arguments as choices built
//! from the task text and the last observation, runs the call on a
//! `hyper-use mcp` child, and appends the reply to the state.
//!
//! Caller-side rules the harness enforces (the server enforces the rest):
//! - act either forwards the last locate's top and runner-up confidence, or
//!   names a region that was already inspected (the MCP contract for an
//!   ungated act);
//! - expectation text can only be a string quoted in the task.
//!
//! Each task gets a fresh tab (`PUT /json/new`) and a fresh `hyper-use mcp`
//! process on that tab's page websocket. After the task the harness reads the
//! page's real URL, title, and snackbar text through its own CDP connection, so the
//! transcript has ground truth next to what hyper-use reported.
//!
//! Run (see examples/live-drive/README.md):
//! `HYPER_USE_JEV=1 TYPESAFE_API_KEY=... cargo run --release -p hyper-use-cli
//!  --features jev --example live_drive -- --bin target/release/hyper-use`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use hyper_use_browser::{CdpTransport, WebSocketTransport};
use serde_json::{json, Value};
use typesafe_sdk::{JsonContent, Question};

const MAX_STEPS: usize = 14;

struct Task {
    id: &'static str,
    /// Page the fresh tab opens, relative to the site root.
    start: &'static str,
    text: &'static str,
    /// What the page should show afterwards. Recorded, not scored.
    expect: &'static str,
}

const TASKS: [Task; 8] = [
    Task {
        id: "t1-compose-send",
        start: "index.html",
        text: r#"Open "Compose", then click the "Send" button in the compose window and verify "Message sent" appears."#,
        expect: "snackbar Message sent",
    },
    Task {
        id: "t2-nav-settings",
        start: "index.html",
        text: r#"Open "Settings" from the left navigation and confirm the page changed."#,
        expect: "title Settings - Acme Mail",
    },
    Task {
        id: "t3-reveal-cc",
        start: "index.html",
        text: r#"Open "Compose", click "Add Cc" to reveal the "Cc recipients" field, and verify it appeared."#,
        expect: "cc field visible",
    },
    Task {
        id: "t4-enabled-save",
        start: "settings.html",
        text: r#"Click the enabled "Save" button (the Signature one), not the disabled Save, and verify "Settings saved" appears."#,
        expect: "snackbar Settings saved",
    },
    Task {
        id: "t5-help-link",
        start: "index.html",
        text: r#"Open "Help" from the top bar and verify the URL changed."#,
        expect: "title Help - Acme Mail",
    },
    Task {
        id: "t6-thread-archive",
        start: "index.html",
        text: r#"Open the "Q3 launch checklist" message, then click the visible "Archive" button in its toolbar and verify "Conversation archived" appears."#,
        expect: "snackbar Conversation archived",
    },
    Task {
        id: "t7-thread-reply",
        start: "index.html",
        text: r#"Open the "Q3 launch checklist" message, then click "Send" in the quick reply box and verify "Reply sent" appears."#,
        expect: "snackbar Reply sent, url #thread/q3",
    },
    Task {
        id: "t8-twin-send",
        start: "index.html",
        text: r#"Open the "Q3 launch checklist" message, open "Compose", then click "Send"."#,
        expect: "unspecified (twin probe: compose Send vs quick-reply Send both visible)",
    },
];

struct Args {
    bin: PathBuf,
    site: String,
    cdp: String,
    out: PathBuf,
    only: Option<String>,
    screenshot_only: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        bin: PathBuf::from("target/release/hyper-use"),
        site: "http://127.0.0.1:8765".into(),
        cdp: "http://127.0.0.1:9333".into(),
        out: PathBuf::from("/tmp/hyper-use-live"),
        only: None,
        screenshot_only: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--bin" => args.bin = PathBuf::from(value()),
            "--site" => args.site = value(),
            "--cdp" => args.cdp = value(),
            "--out" => args.out = PathBuf::from(value()),
            "--only" => args.only = Some(value()),
            "--screenshot-only" => args.screenshot_only = true,
            other => panic!("unknown flag {other}"),
        }
    }
    for url in [&args.site, &args.cdp] {
        assert!(
            url.starts_with("http://127.0.0.1:"),
            "{url}: only 127.0.0.1 is allowed"
        );
    }
    args
}

// ---------- Chrome HTTP endpoint and ground truth ----------

fn http(method: &str, base: &str, path: &str) -> String {
    let host = base.trim_start_matches("http://");
    let mut stream = TcpStream::connect(host).expect("connect to Chrome");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    // Chrome's devtools HTTP server may keep the socket open, so read by
    // Content-Length instead of to EOF.
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        raw.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&raw);
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            });
            if length.is_some_and(|length| body.len() >= length) {
                break;
            }
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    text.split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default()
}

struct Tab {
    id: String,
    ws: String,
}

fn open_tab(cdp: &str, url: &str) -> Tab {
    let body = http("PUT", cdp, &format!("/json/new?{url}"));
    let value: Value = serde_json::from_str(&body).expect("json/new");
    let tab = Tab {
        id: value["id"].as_str().unwrap().to_owned(),
        ws: value["webSocketDebuggerUrl"].as_str().unwrap().to_owned(),
    };
    // Let the page load before the first observe.
    std::thread::sleep(Duration::from_millis(1200));
    tab
}

fn close_tab(cdp: &str, tab: &Tab) {
    let _ = http("GET", cdp, &format!("/json/close/{}", tab.id));
}

const TRUTH_JS: &str = "JSON.stringify({url: location.href, title: document.title, \
    snackbar: (document.querySelector('#snackbar:not(.hidden)') || {getAttribute: () => ''}).getAttribute('aria-label') || '', \
    thread_open: !!document.querySelector('#thread-view:not(.hidden)'), compose_open: !!document.querySelector('#compose:not(.hidden)'), \
    cc_visible: !!document.querySelector('#cc-line:not(.hidden)'), \
    focused: (document.activeElement || {}).id || ''})";

fn ground_truth(tab: &Tab) -> Value {
    let mut socket = match WebSocketTransport::connect(&tab.ws) {
        Ok(socket) => socket,
        Err(err) => return json!({"error": err.to_string()}),
    };
    let params = json!({"expression": TRUTH_JS, "returnByValue": true}).to_string();
    match socket.call("Runtime.evaluate", &params) {
        Ok(raw) => {
            let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
            value["result"]["value"]
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(value)
        }
        Err(err) => json!({"error": err.to_string()}),
    }
}

/// Screenshots at each width, then one with Compose open. Uses a device
/// metrics override on this tab only.
fn screenshots(tab: &Tab, out: &Path) -> Result<Vec<PathBuf>, String> {
    let mut socket = WebSocketTransport::connect(&tab.ws).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    let mut shot = |socket: &mut WebSocketTransport, name: &str| -> Result<(), String> {
        let raw = socket
            .call("Page.captureScreenshot", r#"{"format":"png"}"#)
            .map_err(|e| e.to_string())?;
        let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let data = value["data"].as_str().ok_or("no data")?;
        let path = out.join(name);
        std::fs::write(&path, base64_decode(data)?).map_err(|e| e.to_string())?;
        paths.push(path);
        Ok(())
    };
    for width in [1280, 1440] {
        let metrics =
            json!({"width": width, "height": 860, "deviceScaleFactor": 1, "mobile": false});
        socket
            .call("Emulation.setDeviceMetricsOverride", &metrics.to_string())
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(400));
        shot(&mut socket, &format!("screenshot-inbox-{width}.png"))?;
        for (hash, name) in [("#thread/q3", "thread"), ("#inbox", "")] {
            let go = json!({"expression": format!("location.hash = '{hash}'; true")});
            socket
                .call("Runtime.evaluate", &go.to_string())
                .map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(300));
            if !name.is_empty() {
                shot(&mut socket, &format!("screenshot-{name}-{width}.png"))?;
            }
        }
    }
    let open = json!({"expression": "location.hash = '#thread/q3'; document.getElementById('compose-open').click(); document.getElementById('add-cc').click(); true"});
    socket
        .call("Runtime.evaluate", &open.to_string())
        .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_millis(300));
    shot(&mut socket, "screenshot-compose-1440.png")?;
    for page in ["settings.html", "help.html"] {
        let nav = json!({"expression": format!("location.href = '{page}'; true")});
        socket
            .call("Runtime.evaluate", &nav.to_string())
            .map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(900));
        shot(
            &mut socket,
            &format!("screenshot-{}-1440.png", page.trim_end_matches(".html")),
        )?;
    }
    socket
        .call("Emulation.clearDeviceMetricsOverride", "{}")
        .map_err(|e| e.to_string())?;
    Ok(paths)
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for byte in input.bytes() {
        let v = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            other => return Err(format!("bad base64 byte {other}")),
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

// ---------- MCP child ----------

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn spawn(bin: &Path) -> Self {
        let mut child = Command::new(bin)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn hyper-use mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|_| json!({"raw": reply}))
    }

    fn tool(&mut self, name: &str, arguments: &Value) -> (bool, Value) {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or("null");
        (
            result["isError"] == true,
            serde_json::from_str(text).unwrap_or(Value::Null),
        )
    }

    fn close(mut self) {
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

// ---------- JEV ----------

struct Jev {
    client: typesafe_sdk::blocking::Client,
}

struct Pick {
    choice: String,
    confidence: f64,
    latency_ms: u128,
}

impl Jev {
    fn ask(
        &self,
        state: &Value,
        name: &str,
        prompt: String,
        options: &[(String, String)],
    ) -> Result<Pick, String> {
        let criteria = options
            .iter()
            .map(|(label, about)| (label.clone(), Some(JsonContent::from(about.clone()))));
        let started = Instant::now();
        let response = self
            .client
            .system_one(state.clone(), [(name, Question::choice(prompt, criteria))])
            .map_err(|err| err.to_string())?;
        let answer = response.choice(name).map_err(|err| err.to_string())?;
        Ok(Pick {
            choice: answer.choice.clone(),
            confidence: answer.confidence,
            latency_ms: started.elapsed().as_millis(),
        })
    }
}

// ---------- the loop ----------

fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn summarize(tool: &str, is_error: bool, body: &Value) -> Value {
    if is_error {
        return json!({"error": body});
    }
    match tool {
        "observe" => {
            json!({"snapshot": body["snapshot"], "regions": body["regions"].as_array().map_or(0, Vec::len)})
        }
        "locate" => json!({
            "target": body["target"]["id"],
            "candidates": body["candidates"].as_array().map(|c| c.iter().take(3).map(|x| json!({"id": x["id"], "label": x["label"], "confidence": x["confidence"]})).collect::<Vec<_>>()),
        }),
        "inspect" => body["target"].clone(),
        "act" => json!({
            "executed": body["executed"], "verified": body["verified"], "fallback": body["fallback"],
            "margin_millis": body["margin_millis"], "state_delta": body["state_delta"],
            "verify_error": body["verify_error"], "signals": body["signals"],
            "before_snapshot": body["before_snapshot"], "after_snapshot": body["after_snapshot"],
        }),
        "verify" | "diff" => {
            json!({"verified": body["verified"], "state_delta": body["state_delta"]})
        }
        _ => body.clone(),
    }
}

struct Log {
    lines: Vec<Value>,
    started: Instant,
}

impl Log {
    fn push(&mut self, mut event: Value) {
        event["t_ms"] = json!(self.started.elapsed().as_millis());
        self.lines.push(event);
    }
}

fn run_task(args: &Args, jev: &Jev, task: &Task, tools: &Value) -> Value {
    let started = Instant::now();
    let mut log = Log {
        lines: Vec::new(),
        started,
    };
    let tab = open_tab(&args.cdp, &format!("{}/{}", args.site, task.start));
    let mut mcp = Mcp::spawn(&args.bin);
    let init = mcp.request(
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "jev-live-drive", "version": "0"}}),
    );
    log.push(json!({"kind": "mcp-init", "server": init["result"]["serverInfo"]}));

    let mentions = quoted(task.text);
    let mut regions: Vec<Value> = Vec::new();
    let mut inspected: Vec<String> = Vec::new();
    let mut last_locate: Option<Value> = None;
    let mut last_act: Option<Value> = None;
    let mut history: Vec<Value> = Vec::new();
    let (mut tool_calls, mut jev_calls) = (0usize, 0usize);
    let mut outcome = "max-steps".to_owned();

    for step in 1..=MAX_STEPS {
        let state = json!({
            "role": "You drive hyper-use, a browser coprocessor, through MCP tools to do one task. Choose the next call. hyper-use never navigates and never guesses coordinates.",
            "task": task.text,
            "tools": tools,
            "guidance": [
                "observe lists the page regions; locate ranks regions for a text, role, and position query",
                "act presses one region; it refuses below confidence 0.55 or when the runner-up is within 0.05 (fallback ambiguous)",
                "act on a cdp session observes again and returns state_delta; expect_text makes it verify",
                "a refusal is not a click: change the query (role or position) and try again",
                "choose done when the task's check has passed, or give up when it cannot pass"
            ],
            "page_regions": regions,
            "last_locate": last_locate,
            "history": history,
            "step": step,
        });
        let mut tool_options: Vec<(String, String)> = vec![
            ("observe".into(), "list the regions on the page".into()),
            (
                "locate".into(),
                "rank regions for a text/role/position query".into(),
            ),
        ];
        if !regions.is_empty() {
            tool_options.push((
                "inspect".into(),
                "read one region's role, label, and box".into(),
            ));
        }
        if last_locate.is_some() || !inspected.is_empty() {
            tool_options.push(("act".into(), "press a located or inspected region".into()));
        }
        tool_options.push((
            "verify".into(),
            "check that a quoted text is on the page now".into(),
        ));
        if last_act.is_some() {
            tool_options.push((
                "diff".into(),
                "diff the last act's before and after snapshots".into(),
            ));
        }
        tool_options.push(("done".into(), "stop: the task's check passed".into()));
        tool_options.push((
            "give_up".into(),
            "stop: the task cannot be completed".into(),
        ));

        let pick = match jev.ask(
            &state,
            "next_tool",
            "Which call should be made next for this task?".into(),
            &tool_options,
        ) {
            Ok(pick) => pick,
            Err(err) => {
                log.push(json!({"kind": "jev-error", "step": step, "error": err}));
                outcome = "jev-error".into();
                break;
            }
        };
        jev_calls += 1;
        log.push(json!({"kind": "jev", "step": step, "question": "next_tool", "options": tool_options.iter().map(|o| &o.0).collect::<Vec<_>>(), "choice": pick.choice, "confidence": pick.confidence, "latency_ms": pick.latency_ms}));
        let tool = pick.choice.clone();
        if tool == "done" || tool == "give_up" {
            outcome = tool;
            break;
        }

        // Arguments, as choices.
        let mut ask_arg = |name: &str,
                           prompt: &str,
                           options: Vec<(String, String)>|
         -> Option<String> {
            if options.is_empty() {
                return None;
            }
            match jev.ask(
                &state,
                name,
                format!("{prompt} (for the `{tool}` call)"),
                &options,
            ) {
                Ok(pick) => {
                    jev_calls += 1;
                    log.push(json!({"kind": "jev", "step": step, "question": name, "options": options.iter().map(|o| &o.0).collect::<Vec<_>>(), "choice": pick.choice, "confidence": pick.confidence, "latency_ms": pick.latency_ms}));
                    Some(pick.choice)
                }
                Err(err) => {
                    log.push(
                        json!({"kind": "jev-error", "step": step, "question": name, "error": err}),
                    );
                    None
                }
            }
        };
        let region_options = |filter: &dyn Fn(&str) -> bool| -> Vec<(String, String)> {
            regions
                .iter()
                .filter(|r| filter(r["id"].as_str().unwrap_or("")))
                .map(|r| {
                    (
                        r["id"].as_str().unwrap_or("").to_owned(),
                        format!(
                            "{} \"{}\"",
                            r["role"].as_str().unwrap_or(""),
                            r["label"].as_str().unwrap_or("")
                        ),
                    )
                })
                .collect()
        };
        let mut texts: Vec<String> = mentions.clone();
        for region in &regions {
            if let Some(label) = region["label"].as_str() {
                if !label.is_empty() && !texts.iter().any(|t| t == label) {
                    texts.push(label.to_owned());
                }
            }
        }
        let text_options: Vec<(String, String)> = texts
            .iter()
            .map(|t| (t.clone(), format!("search for \"{t}\"")))
            .collect();
        let expect_options: Vec<(String, String)> = mentions
            .iter()
            .map(|t| (t.clone(), format!("expect \"{t}\" on the page after")))
            .collect();

        let mut arguments = json!({"cdp": tab.ws});
        let mut skip = None;
        match tool.as_str() {
            "observe" => {}
            "locate" => {
                match ask_arg("text", "Which text should locate search for?", text_options) {
                    Some(text) => arguments["text"] = json!(text),
                    None => skip = Some("no locate text"),
                }
                let roles = [
                    "any",
                    "button",
                    "link",
                    "text_field",
                    "heading",
                    "navigation",
                ];
                if let Some(role) = ask_arg(
                    "role",
                    "Which role should the target have?",
                    roles
                        .iter()
                        .map(|r| (r.to_string(), format!("role {r}")))
                        .collect(),
                ) {
                    if role != "any" {
                        arguments["role"] = json!(role);
                    }
                }
                let zones = ["none", "left", "right", "top", "bottom", "center"];
                if let Some(zone) = ask_arg(
                    "position",
                    "Where on the page is the target?",
                    zones
                        .iter()
                        .map(|z| (z.to_string(), format!("position {z}")))
                        .collect(),
                ) {
                    if zone != "none" {
                        arguments["position"] = json!(zone);
                    }
                }
            }
            "inspect" => match ask_arg(
                "region",
                "Which region should be inspected?",
                region_options(&|_| true),
            ) {
                Some(id) => arguments["region"] = json!(id),
                None => skip = Some("no regions known"),
            },
            "act" => {
                let mut targets: Vec<(String, String)> = Vec::new();
                if let Some(located) = &last_locate {
                    targets.push((
                        "last_locate_top".into(),
                        format!(
                            "press locate's top candidate {} with its confidence and runner-up",
                            located["candidates"][0]["id"]
                        ),
                    ));
                }
                targets.extend(
                    region_options(&|id| inspected.iter().any(|i| i == id))
                        .into_iter()
                        .map(|(id, about)| {
                            (
                                id,
                                format!("press inspected region {about} without a score"),
                            )
                        }),
                );
                match ask_arg("target", "Which region should be pressed?", targets) {
                    Some(choice) if choice == "last_locate_top" => {
                        let located = last_locate.as_ref().unwrap();
                        let top = &located["candidates"][0];
                        arguments["region"] = top["id"].clone();
                        arguments["confidence"] = top["confidence"].clone();
                        if let Some(second) = located["candidates"].get(1) {
                            arguments["runner_up"] =
                                json!({"id": second["id"], "confidence": second["confidence"]});
                        }
                    }
                    Some(id) => arguments["region"] = json!(id),
                    None => skip = Some("no act target"),
                }
                let mut options = vec![("none".to_owned(), "no expectation".to_owned())];
                options.extend(expect_options);
                if let Some(expect) = ask_arg(
                    "expect_text",
                    "Which text should be expected after the press?",
                    options,
                ) {
                    if expect != "none" {
                        arguments["expect_text"] = json!(expect);
                    }
                }
            }
            "verify" => match ask_arg(
                "expect_text",
                "Which text should be verified on the page?",
                expect_options,
            ) {
                Some(text) => arguments["expect_text"] = json!(text),
                None => skip = Some("no quoted text to verify"),
            },
            "diff" => {
                let act = last_act.as_ref().unwrap();
                arguments = json!({"before_snapshot": act["before_snapshot"], "after_snapshot": act["after_snapshot"]});
            }
            other => {
                skip = Some(if other.is_empty() {
                    "empty choice"
                } else {
                    "unknown choice"
                })
            }
        }
        if let Some(reason) = skip {
            log.push(json!({"kind": "skipped", "step": step, "tool": tool, "reason": reason}));
            history.push(json!({"step": step, "tool": tool, "skipped": reason}));
            continue;
        }

        let called = Instant::now();
        let (is_error, body) = mcp.tool(&tool, &arguments);
        tool_calls += 1;
        let mut shown_args = arguments.clone();
        if shown_args.get("cdp").is_some() {
            shown_args["cdp"] = json!("<tab>");
        }
        log.push(json!({"kind": "mcp", "step": step, "tool": tool, "arguments": shown_args, "is_error": is_error, "latency_ms": called.elapsed().as_millis(), "body": body}));
        match tool.as_str() {
            "observe" if !is_error => {
                regions = body["regions"].as_array().cloned().unwrap_or_default();
            }
            "locate" if !is_error => {
                last_locate = Some(
                    json!({"query": shown_args, "candidates": body["candidates"].as_array().map(|c| c.iter().take(3).cloned().collect::<Vec<_>>())}),
                )
            }
            "inspect" if !is_error => {
                inspected.push(arguments["region"].as_str().unwrap_or("").to_owned())
            }
            "act" if !is_error => {
                if body["executed"] == true {
                    // The page may have changed: old regions and locate are stale.
                    regions.clear();
                    inspected.clear();
                    last_locate = None;
                }
                if body["after_snapshot"].is_u64() {
                    last_act = Some(body.clone());
                }
            }
            _ => {}
        }
        history.push(json!({"step": step, "tool": tool, "arguments": shown_args, "result": summarize(&tool, is_error, &body)}));
    }

    let truth = ground_truth(&tab);
    let mcp_events: Vec<&Value> = log.lines.iter().filter(|e| e["kind"] == "mcp").collect();
    let count = |pred: &dyn Fn(&Value) -> bool| mcp_events.iter().filter(|e| pred(e)).count();
    let summary = json!({
        "task": task.id,
        "text": task.text,
        "expected_page": task.expect,
        "jev_outcome": outcome,
        "tool_calls": tool_calls,
        "jev_calls": jev_calls,
        "wall_ms": started.elapsed().as_millis(),
        "acts_executed": count(&|e| e["tool"] == "act" && e["body"]["executed"] == true),
        "verified_true": count(&|e| (e["tool"] == "act" || e["tool"] == "verify") && e["body"]["verified"] == true),
        "refused_ambiguous": count(&|e| e["body"]["fallback"] == "ambiguous"),
        "refused_low_confidence": count(&|e| e["body"]["fallback"] == "low-confidence"),
        "no_effect": count(&|e| e["body"]["fallback"] == "no-effect"),
        "tool_errors": count(&|e| e["is_error"] == true),
        "error_variants": mcp_events.iter().filter(|e| e["is_error"] == true).map(|e| e["body"]["variant"].clone()).collect::<Vec<_>>(),
        "calls": mcp_events.iter().map(|e| e["tool"].clone()).collect::<Vec<_>>(),
        "page_after": truth,
    });
    log.push(json!({"kind": "summary", "summary": summary}));
    mcp.close();
    close_tab(&args.cdp, &tab);

    let dir = args.out.join("transcripts");
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = std::fs::File::create(dir.join(format!("{}.jsonl", task.id))).unwrap();
    for line in &log.lines {
        writeln!(file, "{line}").unwrap();
    }
    summary
}

fn main() {
    let args = parse_args();
    std::fs::create_dir_all(&args.out).unwrap();
    if args.screenshot_only {
        let tab = open_tab(&args.cdp, &format!("{}/index.html", args.site));
        match screenshots(&tab, &args.out) {
            Ok(paths) => {
                for path in paths {
                    println!("screenshot {}", path.display());
                }
            }
            Err(err) => println!("screenshot failed: {err}"),
        }
        close_tab(&args.cdp, &tab);
        return;
    }
    assert_eq!(
        std::env::var("HYPER_USE_JEV").ok().as_deref(),
        Some("1"),
        "set HYPER_USE_JEV=1 to call System One"
    );
    let jev = Jev {
        client: typesafe_sdk::blocking::Client::from_env().expect("TYPESAFE_API_KEY"),
    };
    let mut probe = Mcp::spawn(&args.bin);
    probe.request("initialize", json!({"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "jev-live-drive", "version": "0"}}));
    let listed = probe.request("tools/list", json!({}));
    probe.close();
    let tools: Value = listed["result"]["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .map(|t| json!({"name": t["name"], "description": t["description"]}))
                .collect()
        })
        .unwrap_or(Value::Null);

    let mut summaries = Vec::new();
    for task in TASKS
        .iter()
        .filter(|t| args.only.as_deref().is_none_or(|only| t.id == only))
    {
        let summary = run_task(&args, &jev, task, &tools);
        println!("{summary}");
        summaries.push(summary);
    }
    std::fs::write(
        args.out.join("summary.json"),
        serde_json::to_string_pretty(&summaries).unwrap(),
    )
    .unwrap();
}
