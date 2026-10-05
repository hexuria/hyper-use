//! Mock environment: real caller flows against a scripted Chrome.
//!
//! Each test drives one [`Server`] the way JEV would, through `call_tool`,
//! over a `cdp` endpoint. A connector hands the server a
//! [`ReplayTransport`] built with [`ScriptBuilder`] instead of a websocket, so
//! the live-session paths run: session reuse, the stale flag, the snapshot
//! ring, eviction, and a dropped session. Every CDP call is logged, so a test
//! can prove a press did or did not happen.
//!
//! This is not a model of Chrome. The scripts emit only the response shapes
//! the extractors read, in the order observe and press call them. A live JEV
//! drive owns real pages, timing, and real Chrome behaviour.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use hyper_use_browser::script::{Control, HistorySpec, PageSpec, ScriptBuilder};
use hyper_use_browser::{CdpError, CdpTransport, ReplayTransport};
use hyper_use_mcp::{Server, ToolError, MAX_LIVE_SESSIONS};
use serde_json::{json, Value};

/// A replay transport that logs each method it is asked for.
struct Logged {
    inner: ReplayTransport,
    log: Rc<RefCell<Vec<String>>>,
}

impl CdpTransport for Logged {
    fn call(&mut self, method: &str, params_json: &str) -> Result<String, CdpError> {
        self.log.borrow_mut().push(method.to_owned());
        self.inner.call(method, params_json)
    }
}

/// Scripted Chrome for one or more endpoints. Each connect to an endpoint
/// takes the next queued script, so a reconnect after a dropped session gets
/// its own script.
#[derive(Clone, Default)]
struct MockChrome {
    scripts: Rc<RefCell<BTreeMap<String, VecDeque<ScriptBuilder>>>>,
    connects: Rc<RefCell<Vec<String>>>,
    log: Rc<RefCell<Vec<String>>>,
}

impl MockChrome {
    fn tab(&self, endpoint: &str, script: ScriptBuilder) -> &Self {
        self.scripts
            .borrow_mut()
            .entry(endpoint.to_owned())
            .or_default()
            .push_back(script);
        self
    }

    fn server(&self) -> Server {
        let mock = self.clone();
        Server::with_connector(move |url| {
            mock.connects.borrow_mut().push(url.to_owned());
            let script = mock
                .scripts
                .borrow_mut()
                .get_mut(url)
                .and_then(VecDeque::pop_front)
                .ok_or_else(|| CdpError::Transport {
                    message: format!("no mock tab at {url}"),
                })?;
            let inner = ReplayTransport::parse(&script.to_json())?;
            Ok(Box::new(Logged {
                inner,
                log: Rc::clone(&mock.log),
            }) as Box<dyn CdpTransport>)
        })
    }

    fn calls(&self, method: &str) -> usize {
        self.log.borrow().iter().filter(|m| *m == method).count()
    }

    fn observes(&self) -> usize {
        self.calls("Page.getLayoutMetrics")
    }

    fn presses(&self) -> usize {
        self.calls("DOM.resolveNode") + self.calls("Input.dispatchMouseEvent")
    }

    fn total_calls(&self) -> usize {
        self.log.borrow().len()
    }

    fn connects(&self) -> usize {
        self.connects.borrow().len()
    }
}

fn rect(x: f64, y: f64) -> (f64, f64, f64, f64) {
    (x, y, 80.0, 32.0)
}

fn sign_in_page() -> PageSpec {
    PageSpec::of(
        &[
            Control::button(10, 100, "Sign in", rect(600.0, 340.0)),
            Control::link(20, 200, "Forgot password", rect(600.0, 400.0)),
        ],
        "https://example.test/sign-in",
        "Sign in",
    )
}

fn account_page() -> PageSpec {
    PageSpec::of(
        &[Control::button(30, 300, "Welcome back", rect(600.0, 340.0))],
        "https://example.test/account",
        "Account",
    )
}

fn call(server: &mut Server, tool: &str, arguments: Value) -> Value {
    server
        .call_tool(tool, &arguments)
        .unwrap_or_else(|err| panic!("{tool} {arguments}: {err:?}"))
}

fn id(value: &Value) -> &str {
    value["id"].as_str().unwrap()
}

/// observe -> locate -> inspect -> act -> diff -> verify on one live tab.
#[test]
fn find_and_click_a_labeled_control_closes_the_loop() {
    let tab = "ws://mock/find-and-click";
    let script = ScriptBuilder::new()
        .observe(&sign_in_page()) // observe
        .observe(&sign_in_page()) // locate
        .observe(&sign_in_page()) // inspect
        .dom_click(10) // act, reusing the fresh inspect snapshot
        .observe(&account_page()) // act observe_after
        .observe(&account_page()); // verify
    let scripted = script.len();
    let mock = MockChrome::default();
    mock.tab(tab, script);
    let mut server = mock.server();

    let observed = call(&mut server, "observe", json!({"cdp": tab}));
    assert_eq!(observed["snapshot"], 1);
    let labels: Vec<&str> = observed["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Sign in", "Forgot password"]);

    let located = call(
        &mut server,
        "locate",
        json!({"cdp": tab, "text": "Sign in", "role": "button"}),
    );
    assert_eq!(id(&located["target"]), "n100");
    assert_eq!(located["matcher"], "weighted");
    assert_eq!(located["benchmark"], false);
    assert!(located.get("executed").is_none());
    let top = located["candidates"][0]["confidence"].as_f64().unwrap();
    let runner = &located["candidates"][1];
    assert!(top >= 0.55, "{located}");

    let inspected = call(
        &mut server,
        "inspect",
        json!({"cdp": tab, "region": "n100"}),
    );
    assert_eq!(inspected["target"]["x"], 600.0);
    assert_eq!(inspected["target"]["role"], "button");
    let before_snapshot = inspected["snapshot"].clone();

    let acted = call(
        &mut server,
        "act",
        json!({
            "cdp": tab,
            "region": "n100",
            "confidence": top,
            "runner_up": {"id": runner["id"], "confidence": runner["confidence"]},
            "expect_text": "Welcome back"
        }),
    );
    assert_eq!(acted["executed"], true, "{acted}");
    assert_eq!(acted["verified"], true, "{acted}");
    assert_eq!(acted["mechanism"], "dom-semantic");
    assert_eq!(acted["fallback"], Value::Null);
    assert_eq!(acted["before_snapshot"], before_snapshot);
    assert_eq!(acted["after_snapshot"], 4);
    assert_eq!(
        acted["state_delta"],
        json!({
            "added": ["n300"],
            "removed": ["n100", "n200"],
            "changed": [],
            "moved": [],
            "text_changed": [],
            "focus_changed": false,
            "url_changed": true,
            "title_changed": true
        })
    );

    let diffed = call(
        &mut server,
        "diff",
        json!({"before_snapshot": before_snapshot, "after_snapshot": 4}),
    );
    assert_eq!(diffed["state_delta"]["added"], json!(["n300"]));
    assert_eq!(diffed["state_delta"]["removed"], json!(["n100", "n200"]));
    assert_eq!(diffed["state_delta"]["url_changed"], true);

    let verified = call(
        &mut server,
        "verify",
        json!({"cdp": tab, "expect_text": "Welcome back"}),
    );
    assert_eq!(verified["verified"], true);

    assert_eq!(mock.connects(), 1, "one socket for the whole flow");
    assert_eq!(mock.observes(), 5);
    assert_eq!(mock.calls("DOM.resolveNode"), 1);
    assert_eq!(mock.total_calls(), scripted, "every scripted call, no more");
    assert_eq!(server.live_sessions(), [tab.to_owned()]);
}

fn twins_page() -> PageSpec {
    PageSpec::of(
        &[
            Control::button(10, 100, "Send", rect(40.0, 600.0)),
            Control::button(20, 200, "Send", rect(1160.0, 600.0)),
        ],
        "https://example.test/compose",
        "Compose",
    )
}

/// Two identical "Send" buttons: the margin refuses, then a position hint
/// separates them and the act presses the right one.
#[test]
fn twin_controls_refuse_on_margin_until_the_caller_disambiguates() {
    let tab = "ws://mock/twins";
    let sent = PageSpec::of(
        &[Control::button(10, 100, "Send", rect(40.0, 600.0))],
        "https://example.test/compose",
        "Compose",
    );
    let script = ScriptBuilder::new()
        .observe(&twins_page()) // locate, no hint
        .observe(&twins_page()) // locate, position right
        .dom_click(20)
        .observe(&sent);
    let mock = MockChrome::default();
    mock.tab(tab, script);
    let mut server = mock.server();

    let located = call(&mut server, "locate", json!({"cdp": tab, "text": "Send"}));
    let top = &located["candidates"][0];
    let runner = &located["candidates"][1];
    let refused = call(
        &mut server,
        "act",
        json!({
            "cdp": tab,
            "region": top["id"],
            "confidence": top["confidence"],
            "runner_up": {"id": runner["id"], "confidence": runner["confidence"]}
        }),
    );
    assert_eq!(refused["executed"], false, "{refused}");
    assert_eq!(refused["fallback"], "ambiguous");
    assert_eq!(refused["mechanism"], Value::Null);
    assert!(refused["margin_millis"].as_i64().unwrap() < 50, "{refused}");
    assert_eq!(refused["runner_up"]["label"], "Send");
    assert_eq!(refused["after_snapshot"], Value::Null);
    assert_eq!(mock.presses(), 0, "an ambiguous act never presses");

    let hinted = call(
        &mut server,
        "locate",
        json!({"cdp": tab, "text": "Send", "position": "right"}),
    );
    assert_eq!(id(&hinted["target"]), "n200", "{hinted}");
    let top = hinted["candidates"][0]["confidence"].as_f64().unwrap();
    let second = hinted["candidates"][1]["confidence"].as_f64().unwrap();
    assert!(top - second >= 0.05, "{hinted}");
    let acted = call(
        &mut server,
        "act",
        json!({
            "cdp": tab,
            "region": "n200",
            "text": "Send",
            "position": "right"
        }),
    );
    assert_eq!(acted["executed"], true, "{acted}");
    assert_eq!(acted["state_delta"]["removed"], json!(["n200"]));
    assert_eq!(mock.calls("DOM.resolveNode"), 1);
}

/// A press that reveals a panel and moves focus, on the same URL.
#[test]
fn act_with_observe_after_reports_region_and_focus_delta_without_url_change() {
    let tab = "ws://mock/disclosure";
    let closed = PageSpec::of(
        &[Control::button(10, 100, "Show details", rect(100.0, 100.0))],
        "https://example.test/order/7",
        "Order 7",
    );
    let open = PageSpec::of(
        &[
            Control::button(10, 100, "Show details", rect(100.0, 100.0)),
            Control::text_field(40, 400, "Gift note", rect(100.0, 160.0)).focused(),
        ],
        "https://example.test/order/7",
        "Order 7",
    );
    let mock = MockChrome::default();
    mock.tab(
        tab,
        ScriptBuilder::new()
            .observe(&closed)
            .dom_click(10)
            .observe(&open),
    );
    let mut server = mock.server();
    let acted = call(&mut server, "act", json!({"cdp": tab, "region": "n100"}));
    assert_eq!(acted["executed"], true);
    assert_eq!(acted["verified"], false, "no expectation was given");
    assert_eq!(acted["fallback"], Value::Null, "{acted}");
    assert_eq!(
        acted["state_delta"],
        json!({
            "added": ["n400"],
            "removed": [],
            "changed": [],
            "moved": [],
            "text_changed": [],
            "focus_changed": true,
            "url_changed": false,
            "title_changed": false
        })
    );
    assert_eq!(acted["signals"], json!([]));
}

/// After an act with observe_after false, the next act must observe again
/// instead of reusing the pre-press snapshot as its before.
#[test]
fn act_without_observe_after_then_next_act_does_not_reuse_stale_before() {
    let tab = "ws://mock/stale";
    let done = PageSpec::of(
        &[Control::link(50, 500, "Sign out", rect(1100.0, 20.0))],
        "https://example.test/account",
        "Account",
    );
    let script = ScriptBuilder::new()
        .observe(&sign_in_page()) // act 1 before
        .dom_click(10) // act 1 press, no observe after
        .observe(&account_page()) // act 2 must observe again
        .dom_click(30)
        .observe(&done); // act 2 observe_after
    let scripted = script.len();
    let mock = MockChrome::default();
    mock.tab(tab, script);
    let mut server = mock.server();

    let first = call(
        &mut server,
        "act",
        json!({"cdp": tab, "region": "n100", "observe_after": false}),
    );
    assert_eq!(first["executed"], true);
    assert_eq!(first["before_snapshot"], 1);
    assert_eq!(first["after_snapshot"], Value::Null);
    assert_eq!(mock.observes(), 1);

    // n300 exists only on the page after the first press. Reusing the stale
    // snapshot would fail with UnknownRegion.
    let second = call(&mut server, "act", json!({"cdp": tab, "region": "n300"}));
    assert_eq!(second["executed"], true, "{second}");
    assert_eq!(second["before_snapshot"], 2);
    assert_eq!(second["after_snapshot"], 3);
    assert_eq!(second["target"]["label"], "Welcome back");
    assert_eq!(second["state_delta"]["removed"], json!(["n300"]));
    assert_eq!(second["state_delta"]["added"], json!(["n500"]));
    assert_eq!(mock.observes(), 3);
    assert_eq!(mock.total_calls(), scripted);
}

/// 0.5496 used to round to 550 millis and click. The raw gate refuses and the
/// session survives for the next call.
#[test]
fn low_confidence_refuses_without_pressing_and_keeps_the_session() {
    let tab = "ws://mock/low";
    let script = ScriptBuilder::new()
        .observe(&sign_in_page())
        .dom_click(10)
        .observe(&account_page());
    let mock = MockChrome::default();
    mock.tab(tab, script);
    let mut server = mock.server();

    let refused = call(
        &mut server,
        "act",
        json!({"cdp": tab, "region": "n100", "confidence": 0.5496}),
    );
    assert_eq!(refused["executed"], false);
    assert_eq!(refused["fallback"], "low-confidence");
    assert_eq!(refused["mechanism"], Value::Null);
    assert_eq!(refused["after_snapshot"], Value::Null);
    assert_eq!(mock.presses(), 0);
    assert_eq!(server.live_sessions(), [tab.to_owned()]);

    // The refusal did not press, so the snapshot is still fresh and reused.
    let acted = call(
        &mut server,
        "act",
        json!({"cdp": tab, "region": "n100", "confidence": 0.9}),
    );
    assert_eq!(acted["executed"], true, "{acted}");
    assert_eq!(acted["before_snapshot"], 1);
    assert_eq!(mock.observes(), 2);
    assert_eq!(mock.connects(), 1);
}

/// A caller-supplied runner-up 0.02 behind the top is AmbiguousTarget.
#[test]
fn caller_runner_up_inside_the_margin_is_ambiguous_target() {
    let tab = "ws://mock/ambiguous";
    let mock = MockChrome::default();
    mock.tab(tab, ScriptBuilder::new().observe(&sign_in_page()));
    let mut server = mock.server();
    let refused = call(
        &mut server,
        "act",
        json!({
            "cdp": tab,
            "region": "n100",
            "confidence": 0.9,
            "runner_up": {"id": "n200", "confidence": 0.88}
        }),
    );
    assert_eq!(refused["executed"], false);
    assert_eq!(refused["fallback"], "ambiguous");
    assert_eq!(refused["margin_millis"], 20);
    assert_eq!(refused["runner_up"]["id"], "n200");
    assert_eq!(refused["runner_up"]["label"], "Forgot password");
    assert_eq!(refused["runner_up"]["confidence"], 0.88);
    assert_eq!(mock.presses(), 0);

    // Out-of-range caller confidence is an exact error, not a score.
    let err = server
        .call_tool(
            "act",
            &json!({
                "cdp": tab,
                "region": "n100",
                "confidence": 0.9,
                "runner_up": {"id": "n200", "confidence": -0.2}
            }),
        )
        .unwrap_err();
    assert_eq!(err, ToolError::ConfidenceOutOfRange("-0.2".into()));
}

/// The page changes between two observes (another actor, a timer). Diff by
/// snapshot id reports it. An evicted snapshot is an exact error.
#[test]
fn session_ring_observe_then_diff_and_eviction() {
    let tab = "ws://mock/ring";
    let mut script = ScriptBuilder::new()
        .observe(&sign_in_page())
        .observe(&account_page());
    for _ in 0..16 {
        script = script.observe(&account_page());
    }
    let mock = MockChrome::default();
    mock.tab(tab, script);
    let mut server = mock.server();
    assert_eq!(
        call(&mut server, "observe", json!({"cdp": tab}))["snapshot"],
        1
    );
    assert_eq!(
        call(&mut server, "observe", json!({"cdp": tab}))["snapshot"],
        2
    );
    let diffed = call(
        &mut server,
        "diff",
        json!({"before_snapshot": 1, "after_snapshot": 2}),
    );
    assert_eq!(diffed["state_delta"]["added"], json!(["n300"]));
    assert_eq!(diffed["state_delta"]["removed"], json!(["n100", "n200"]));
    assert_eq!(diffed["state_delta"]["url_changed"], true);
    assert_eq!(diffed["state_delta"]["title_changed"], true);

    for _ in 0..16 {
        call(&mut server, "observe", json!({"cdp": tab}));
    }
    let err = server
        .call_tool("diff", &json!({"before_snapshot": 1, "after_snapshot": 18}))
        .unwrap_err();
    assert_eq!(err, ToolError::SnapshotEvicted { id: 1, oldest: 3 });
}

/// The press lands but nothing changes: NoEffect and a no-op signal, as data.
#[test]
fn press_with_no_change_is_no_effect_and_no_op() {
    let tab = "ws://mock/dead-button";
    let mock = MockChrome::default();
    mock.tab(
        tab,
        ScriptBuilder::new()
            .observe(&sign_in_page())
            .dom_click(10)
            .observe(&sign_in_page()),
    );
    let mut server = mock.server();
    let acted = call(&mut server, "act", json!({"cdp": tab, "region": "n100"}));
    assert_eq!(acted["executed"], true);
    assert_eq!(acted["verified"], false);
    assert_eq!(acted["fallback"], "no-effect");
    assert_eq!(acted["signals"], json!([{"kind": "no-op"}]));
    assert_eq!(acted["state_delta"]["url_changed"], false);
    assert_eq!(acted["state_delta"]["added"], json!([]));
    assert_eq!(mock.presses(), 1, "signals do not retry");
}

/// History failure is unknown page state: no false url/title change and no
/// false NoEffect, in either direction.
#[test]
fn page_history_failure_is_unknown_not_a_false_delta() {
    let tab = "ws://mock/history";
    let unknown = sign_in_page().with_history(HistorySpec::ProtocolError);
    let empty = account_page().with_history(HistorySpec::NoEntries);
    let mock = MockChrome::default();
    mock.tab(
        tab,
        ScriptBuilder::new()
            .observe(&sign_in_page()) // act 1 before: known
            .dom_click(10)
            .observe(&unknown) // act 1 after: same regions, history failed
            .dom_click(10) // act 2 reuses the fresh after
            .observe(&empty), // act 2 after: new regions, no entries
    );
    let mut server = mock.server();

    let first = call(&mut server, "act", json!({"cdp": tab, "region": "n100"}));
    assert_eq!(first["executed"], true);
    assert_eq!(first["state_delta"]["url_changed"], false);
    assert_eq!(first["state_delta"]["title_changed"], false);
    assert_eq!(first["state_delta"]["added"], json!([]));
    assert_eq!(
        first["fallback"],
        Value::Null,
        "unknown page state is not evidence of no effect: {first}"
    );
    // The signature still matches, and that is reported as data.
    assert_eq!(first["signals"], json!([{"kind": "no-op"}]));

    let second = call(&mut server, "act", json!({"cdp": tab, "region": "n100"}));
    assert_eq!(second["before_snapshot"], 2);
    assert_eq!(second["state_delta"]["added"], json!(["n300"]));
    assert_eq!(second["state_delta"]["url_changed"], false);
    assert_eq!(second["state_delta"]["title_changed"], false);

    let diffed = call(
        &mut server,
        "diff",
        json!({"before_snapshot": 1, "after_snapshot": 3}),
    );
    assert_eq!(diffed["state_delta"]["url_changed"], false);
}

/// Connect failure, a transport that dies mid-act, reconnect, eviction at
/// the session cap, and macOS still NotImplemented.
#[test]
fn session_lifecycle_failures_are_typed_and_drop_the_session() {
    let mock = MockChrome::default();
    let mut server = mock.server();

    let err = server
        .call_tool("observe", &json!({"cdp": "ws://mock/absent"}))
        .unwrap_err();
    assert_eq!(
        err,
        ToolError::Browser("CDP transport: no mock tab at ws://mock/absent".into())
    );
    assert!(server.live_sessions().is_empty());

    // The first script ends before the press: the act fails, the session is
    // dropped, and the next call reconnects to a fresh tab.
    let tab = "ws://mock/flaky";
    mock.tab(tab, ScriptBuilder::new().observe(&sign_in_page()));
    mock.tab(
        tab,
        ScriptBuilder::new()
            .observe(&sign_in_page())
            .dom_click(10)
            .observe(&account_page()),
    );
    let err = server
        .call_tool("act", &json!({"cdp": tab, "region": "n100"}))
        .unwrap_err();
    assert!(matches!(err, ToolError::Browser(_)), "{err:?}");
    assert!(server.live_sessions().is_empty(), "a failed act drops it");
    let acted = call(&mut server, "act", json!({"cdp": tab, "region": "n100"}));
    assert_eq!(acted["executed"], true);
    assert_eq!(
        mock.connects.borrow().iter().filter(|u| *u == tab).count(),
        2
    );

    // Cap: a fifth endpoint evicts the least recently used.
    let tabs: Vec<String> = (0..=MAX_LIVE_SESSIONS)
        .map(|n| format!("ws://mock/cap-{n}"))
        .collect();
    for endpoint in &tabs {
        mock.tab(endpoint, ScriptBuilder::new().observe(&sign_in_page()));
    }
    for endpoint in &tabs {
        call(&mut server, "observe", json!({"cdp": endpoint}));
    }
    assert_eq!(server.live_sessions().len(), MAX_LIVE_SESSIONS);
    assert!(!server.live_sessions().contains(&tabs[0]));
    assert_eq!(server.live_sessions().last(), tabs.last());

    let err = server
        .call_tool(
            "act",
            &json!({"cdp": tab, "region": "n100", "executor": "macos"}),
        )
        .unwrap_err();
    assert_eq!(
        err,
        ToolError::NotImplemented {
            executor: "macos".into()
        }
    );
}
