//! Live Chrome smoke for the owned loop. Ignored by default (not in CI).
//!
//! ```text
//! chrome --headless=new --remote-debugging-port=9222 --user-data-dir=/tmp/hu-smoke
//! ULTRA_INSTINCT_CDP=http://127.0.0.1:9222 cargo test -p aui-agent --test live_smoke -- --ignored --nocapture
//! ```
//!
//! Navigates the first page target to an inline `data:` page (no server) and
//! runs TYPE_TEXT → SELECT → CLICK → SCROLL goals through observe → Instinct →
//! gate → ticket → executor → verify, then checks the page DOM result.

use aui_agent::{AgentBuilder, AgentOutcome, BrowserRuntime, VerificationKind};
use aui_browser::{BrowserSession, WebSocketTransport};
use aui_core::ActionKind;
use aui_policy::InstinctPolicy;

const PAGE: &str = "<!doctype html><title>HU live smoke</title><h1>Flight search</h1>\
<input id=q aria-label=Search style=width:300px>\
<select id=cabin aria-label='Cabin class'><option value=economy>Economy</option><option value=business>Business</option></select>\
<button id=go onclick=\"document.getElementById('out').textContent='Searched '+q.value+' in '+cabin.value\">Go</button>\
<p id=out></p><div style=height:3000px></div><button>Bottom</button>";

fn data_url() -> String {
    let mut out = String::from("data:text/html,");
    for b in PAGE.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn step(
    session: BrowserSession<WebSocketTransport>,
    goal: &str,
    kind: ActionKind,
    verification: VerificationKind,
) -> BrowserSession<WebSocketTransport> {
    let mut agent = AgentBuilder::new(session, InstinctPolicy::default())
        .max_steps(4)
        .build(goal);
    let outcome = agent.run();
    eprintln!("{goal}: {outcome:?}");
    assert!(
        matches!(outcome, AgentOutcome::Done { .. }),
        "{goal}: {outcome:?}"
    );
    let first = &outcome.steps()[0];
    assert_eq!(first.kind, kind, "{goal}");
    assert_eq!(first.verification, verification, "{goal}");
    agent.into_browser()
}

#[test]
#[ignore = "needs a live Chrome with --remote-debugging-port (ULTRA_INSTINCT_CDP)"]
fn owned_loop_drives_type_select_click_scroll_on_live_chrome() {
    let endpoint =
        std::env::var("ULTRA_INSTINCT_CDP").unwrap_or_else(|_| "http://127.0.0.1:9222".to_owned());
    let mut session = BrowserSession::new(WebSocketTransport::connect(&endpoint).unwrap());
    session.navigate(&data_url()).unwrap();
    session.settle();

    let session = step(
        session,
        r#"Type "rust ownership" into Search"#,
        ActionKind::TypeText,
        VerificationKind::Success,
    );
    let session = step(
        session,
        r#"Select "Business" in Cabin class"#,
        ActionKind::Select,
        VerificationKind::Success,
    );
    let mut session = step(
        session,
        "Click Go",
        ActionKind::Click,
        VerificationKind::StateChanged,
    );
    let m = BrowserRuntime::observe(&mut session).unwrap().clone();
    assert!(
        m.regions()
            .any(|r| r.label() == "Searched rust ownership in business"),
        "page result missing"
    );
    let _ = step(
        session,
        "scroll down",
        ActionKind::ScrollDown,
        VerificationKind::StateChanged,
    );
}

const HARDER: &str = r#"<!doctype html><title>HU harder</title>
<div id=host></div>
<script>
const host = document.getElementById('host');
const root = host.attachShadow({mode:'open'});
root.innerHTML = '<button aria-label="Shadow Ping">Shadow Ping</button>';
</script>
<input role=combobox aria-label=City id=city style=width:200px>
<div role=listbox aria-label=Suggestions>
  <div role=option aria-label=Manila>Manila</div>
</div>
<iframe id=frame srcdoc="<button aria-label=Frame Hi>Frame Hi</button>"></iframe>
"#;

fn harder_data_url() -> String {
    let mut out = String::from("data:text/html,");
    for b in HARDER.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[test]
#[ignore = "needs a live Chrome with --remote-debugging-port (ULTRA_INSTINCT_CDP)"]
fn harder_page_types_observe_shadow_iframe_combobox_on_live_chrome() {
    let endpoint =
        std::env::var("ULTRA_INSTINCT_CDP").unwrap_or_else(|_| "http://127.0.0.1:9222".to_owned());
    let mut session = BrowserSession::new(WebSocketTransport::connect(&endpoint).unwrap());
    session.navigate(&harder_data_url()).unwrap();
    session.settle();
    let m = BrowserRuntime::observe(&mut session).unwrap().clone();
    let labels: Vec<_> = m.regions().map(|r| r.label().to_owned()).collect();
    eprintln!("labels: {labels:?}");
    assert!(
        labels.iter().any(|l| l == "Shadow Ping"),
        "open shadow button missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "City"),
        "combobox missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "Manila"),
        "option missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l == "Frame Hi"),
        "same-origin iframe button missing: {labels:?}"
    );
}
