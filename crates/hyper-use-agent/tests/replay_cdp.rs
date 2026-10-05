//! Mock-CDP e2e: the real `BrowserSession` over a `ReplayTransport` driven by
//! the agent. Every CDP call the loop makes must be scripted in order, so these
//! tests pin the exact wire sequence of observe → ticket → input → observe.

use hyper_use_agent::{AgentBuilder, AgentError, AgentOutcome, VerificationKind};
use hyper_use_browser::script::{Control, PageSpec, ScriptBuilder};
use hyper_use_browser::{
    BrowserSession, ReplayTransport, DOM_SELECT_FUNCTION, DOM_TYPE_FUNCTION,
    SCROLL_VIEWPORT_FRACTION,
};
use hyper_use_core::ActionKind;
use hyper_use_policy::PuaPolicy;
use serde_json::Value;

fn session(script: ScriptBuilder) -> BrowserSession<ReplayTransport> {
    BrowserSession::new(ReplayTransport::parse(&script.to_json()).unwrap())
}

fn search_page() -> PageSpec {
    PageSpec::of(
        &[
            Control::text_field(10, 100, "Search", (20.0, 20.0, 300.0, 28.0)),
            Control::button(11, 101, "Go", (340.0, 20.0, 60.0, 28.0)),
        ],
        "http://127.0.0.1/search",
        "Search",
    )
}

fn calls_of(log: &[(String, String)], method: &str) -> Vec<Value> {
    log.iter()
        .filter(|(m, _)| m == method)
        .map(|(_, p)| serde_json::from_str(p).unwrap())
        .collect()
}

#[test]
fn type_text_over_cdp_sends_argument_not_source_and_verifies_value() {
    let page = search_page();
    let script = ScriptBuilder::new()
        .observe(&page) // predict
        .observe(&page) // executor revalidation
        .dom_input(10) // DOM.resolveNode + Runtime.callFunctionOn(type)
        .observe(&page) // after
        .dom_read_value(10, "rust ownership", "") // value read-back
        .observe(&page); // next predict → policy: satisfied → DONE
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(4)
        .build(r#"Type "rust ownership" into Search"#);
    let outcome = agent.run();
    assert!(matches!(outcome, AgentOutcome::Done { .. }), "{outcome:?}");
    let step = &outcome.steps()[0];
    assert_eq!(step.kind, ActionKind::TypeText);
    assert_eq!(step.verification, VerificationKind::Success);

    let transport = agent.browser_mut().transport();
    assert_eq!(transport.remaining(), 0, "every scripted call consumed");
    let fns = calls_of(transport.logged_calls(), "Runtime.callFunctionOn");
    let typed = &fns[0];
    assert_eq!(typed["functionDeclaration"], DOM_TYPE_FUNCTION);
    assert_eq!(typed["arguments"][0]["value"], "rust ownership");
    assert!(!DOM_TYPE_FUNCTION.contains("rust"));
    // Bound to the observed node, never coordinates.
    let resolves = calls_of(transport.logged_calls(), "DOM.resolveNode");
    assert_eq!(resolves[0]["nodeId"], 10);
    assert!(calls_of(transport.logged_calls(), "Input.dispatchMouseEvent").is_empty());
}

#[test]
fn readonly_field_rejection_over_cdp_types_nothing() {
    let page = search_page();
    let script = ScriptBuilder::new()
        .observe(&page)
        .observe(&page)
        .dom_input_rejected(10, "readonly");
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(2)
        .build(r#"Type "x" into Search"#);
    agent.predict().unwrap();
    let err = agent.act().unwrap_err();
    assert!(
        matches!(err, AgentError::InputRejected(ref m) if m.contains("readonly")),
        "{err}"
    );
}

#[test]
fn stale_rerender_over_cdp_never_reaches_input() {
    let page = search_page();
    // Nearby peer (not a far banner): target-scoped world must go stale.
    let changed = PageSpec::of(
        &[
            Control::text_field(10, 100, "Search", (20.0, 20.0, 300.0, 28.0)),
            Control::button(11, 101, "Go", (340.0, 20.0, 60.0, 28.0)),
            Control::button(12, 102, "Accept cookies", (420.0, 20.0, 160.0, 28.0)),
        ],
        "http://127.0.0.1/search",
        "Search",
    );
    let script = ScriptBuilder::new().observe(&page).observe(&changed);
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(2)
        .build("Go");
    agent.predict().unwrap().expect("click Go");
    let err = agent.act().unwrap_err();
    assert!(matches!(err, AgentError::Stale(_)), "{err}");
    let transport = agent.browser_mut().transport();
    assert!(calls_of(transport.logged_calls(), "Runtime.callFunctionOn").is_empty());
    assert!(calls_of(transport.logged_calls(), "Input.dispatchMouseEvent").is_empty());
}

#[test]
fn covered_button_over_cdp_is_not_offered_or_pressed() {
    // The search field's node sits over "Go"'s center (hit-test override),
    // e.g. a cookie banner. Observation marks Go occluded.
    let page = search_page().cover(101, 100);
    let mut s = session(ScriptBuilder::new().observe(&page));
    let manifold = hyper_use_agent::BrowserRuntime::observe(&mut s)
        .unwrap()
        .clone();
    let go = manifold
        .regions()
        .find(|r| r.label() == "Go")
        .expect("Go observed");
    assert!(go.flags().occluded(), "hit-test must mark Go occluded");
    let space = hyper_use_agent::Agent::<BrowserSession<ReplayTransport>, PuaPolicy>::action_space(
        &manifold,
    );
    assert!(space
        .targets_of(ActionKind::Click)
        .all(|a| a.label() != "Go"));
    // And the gate refuses it outright if anything ever proposed it.
    assert_eq!(
        hyper_use_guard::gate(&manifold, go.id(), hyper_use_core::Action::Click, None, 0),
        Err(hyper_use_protocol::GuardReason::Occluded)
    );
}

#[test]
fn select_over_cdp_uses_select_function_and_verifies_option_text() {
    let select = Control {
        tag: "SELECT",
        role: "combobox",
        ..Control::button(20, 200, "Cabin class", (20.0, 80.0, 200.0, 28.0))
    };
    let page = PageSpec::of(
        &[
            select,
            Control::button(21, 201, "Find flights", (20.0, 140.0, 140.0, 28.0)),
        ],
        "http://127.0.0.1/travel",
        "Travel",
    );
    let script = ScriptBuilder::new()
        .observe(&page)
        .observe(&page)
        .dom_input(20)
        .observe(&page)
        .dom_read_value(20, "business", "Business");
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(2)
        .build(r#"Select "Business" in Cabin class"#);
    let p = agent.predict().unwrap().expect("select").clone();
    assert_eq!(p.decision.kind, ActionKind::Select, "{p:?}");
    let rec = agent.act().unwrap();
    assert_eq!(rec.verification, VerificationKind::Success);
    let transport = agent.browser_mut().transport();
    let fns = calls_of(transport.logged_calls(), "Runtime.callFunctionOn");
    assert_eq!(fns[0]["functionDeclaration"], DOM_SELECT_FUNCTION);
    assert_eq!(fns[0]["arguments"][0]["value"], "Business");
}

#[test]
fn scroll_over_cdp_is_one_wheel_event_at_viewport_center() {
    let page = search_page();
    let script = ScriptBuilder::new().observe(&page).scroll().observe(&page);
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(2)
        .build("scroll down");
    agent.predict().unwrap();
    let rec = agent.act().unwrap();
    assert_eq!(rec.kind, ActionKind::ScrollDown);
    let transport = agent.browser_mut().transport();
    let wheels = calls_of(transport.logged_calls(), "Input.dispatchMouseEvent");
    assert_eq!(wheels.len(), 1);
    assert_eq!(wheels[0]["type"], "mouseWheel");
    assert_eq!(wheels[0]["x"], 640.0);
    assert_eq!(wheels[0]["y"], 360.0);
    assert_eq!(
        wheels[0]["deltaY"],
        (720.0 * SCROLL_VIEWPORT_FRACTION).round()
    );
}

#[test]
fn autocomplete_type_then_option_click_over_cdp() {
    let empty = PageSpec::of(
        &[Control::combobox(
            10,
            100,
            "City",
            (10.0, 10.0, 200.0, 28.0),
        )],
        "http://127.0.0.1/auto",
        "Auto",
    );
    let open = PageSpec::of(
        &[
            Control::combobox(10, 100, "City", (10.0, 10.0, 200.0, 28.0)),
            Control::option(11, 110, "Manila", (10.0, 40.0, 200.0, 28.0)),
        ],
        "http://127.0.0.1/auto",
        "Auto",
    );
    // Post-click page keeps Manila (history → DONE) and adds a marker so
    // verify sees state-changed rather than no-effect.
    let picked = PageSpec::of(
        &[
            Control::combobox(10, 100, "City", (10.0, 10.0, 200.0, 28.0)),
            Control::option(11, 110, "Manila", (10.0, 40.0, 200.0, 28.0)),
            Control::button(12, 120, "picked", (10.0, 80.0, 80.0, 24.0)),
        ],
        "http://127.0.0.1/auto",
        "Auto",
    );
    let script = ScriptBuilder::new()
        .observe(&empty) // predict TYPE
        .observe(&empty) // executor revalidate TYPE
        .dom_input(10)
        .observe(&open) // after TYPE (popup open)
        .dom_read_value(10, "man", "")
        .observe(&open) // TYPE clause → DONE
        .observe(&open) // predict CLICK Manila
        .observe(&open) // executor revalidate CLICK
        .dom_click(11)
        .observe(&picked) // after CLICK (state-changed)
        .observe(&picked); // CLICK clause → DONE
    let mut agent = AgentBuilder::new(session(script), PuaPolicy::default())
        .max_steps(6)
        .build(r#"Type "man" into City then click Manila"#);
    let outcome = agent.run();
    assert!(matches!(outcome, AgentOutcome::Done { .. }), "{outcome:?}");
    let steps = outcome.steps();
    assert_eq!(steps.len(), 2, "{steps:?}");
    assert_eq!(steps[0].kind, ActionKind::TypeText);
    assert_eq!(steps[1].kind, ActionKind::Click);
    assert_eq!(steps[1].label, "Manila");
    let transport = agent.browser_mut().transport();
    assert_eq!(transport.remaining(), 0, "every scripted call consumed");
}
