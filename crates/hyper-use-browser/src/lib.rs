//! Browser surface for hyper-use.
//!
//! Phase 2 speaks Chrome DevTools Protocol through [`CdpTransport`].
//! [`ReplayTransport`] replays a recorded script. [`WebSocketTransport`] is
//! the same calls on a live `ws://` socket. There is no browser framework and
//! no computer-use vision path.
//!
//! The default HTTP endpoint is [`DEFAULT_CDP_HTTP`] (`http://127.0.0.1:9222`).
//! hyper-use does not launch Chrome. macOS and CUA stay outside this crate.

#![forbid(unsafe_code)]

mod error;
mod extract;
mod fusion;
mod identity;
mod page;
mod replay;
pub mod script;
mod session;
mod transport;
mod verify;
mod ws;

pub use error::{ActMechanism, BrowserError, CdpError};
pub use fusion::{MAX_CENTROID_PX, MIN_IOU, MIN_LABEL_JACCARD};
pub use page::{page_delta, PageDelta, PageState};
pub use replay::ReplayTransport;
pub use session::{BrowserSession, DOM_CLICK_FUNCTION};
pub use transport::CdpTransport;
pub use verify::{verify, verify_delta, Expectation, VerifyError};
pub use ws::{WebSocketTransport, DEFAULT_CDP_HTTP};

/// A session can be opened. This is not a claim that a browser is running.
pub const STATUS: &str = "browser CDP client ready; act requires a session";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrowserStub;

impl BrowserStub {
    pub const fn status(self) -> &'static str {
        STATUS
    }
}

#[cfg(test)]
mod proptest_parse {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn garbage_scripts_do_not_panic(raw in "\\PC{0,80}") {
            let _ = ReplayTransport::parse(&raw);
        }
    }
}

#[cfg(test)]
mod phase2 {
    use super::*;
    use hyper_use_core::{Action, LocateQuery, RegionId, Role, SourceMask};
    use hyper_use_observe::diff;
    use hyper_use_resonance::{default_matcher, RegionMatcher};

    #[test]
    fn locate_sign_in_on_cdp_snapshot_ranks_the_button_first() {
        let transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let mut session = BrowserSession::new(transport);
        let manifold = session.observe().unwrap().clone();
        assert_eq!(manifold.len(), 2);
        let sign_in = manifold.get_str("n100").unwrap();
        assert_eq!(sign_in.label(), "Sign in");
        assert!(sign_in.sources().contains(SourceMask::DOM));
        assert!(sign_in.sources().contains(SourceMask::ACCESSIBILITY));
        assert_eq!(sign_in.rect().x(), 400.0);
        let query = LocateQuery::new().text("Sign in").unwrap();
        let ranked = default_matcher().rank(&query, &manifold).unwrap();
        assert_eq!(ranked[0].id().as_str(), "n100");
        assert_eq!(ranked[0].rank(), 1);
        assert!(ranked[0].confidence() > ranked[1].confidence());
    }

    #[test]
    fn press_before_observe_is_not_observed() {
        let transport = ReplayTransport::parse(r#"{"calls":[]}"#).unwrap();
        let mut session = BrowserSession::new(transport);
        let err = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap_err();
        assert_eq!(err, BrowserError::NotObserved);
        assert_eq!(err.to_string(), "browser session has no observation yet");
        assert!(session.transport().logged_methods().is_empty());
    }

    #[test]
    fn bad_viewport_and_missing_object_id_are_exact() {
        let transport = ReplayTransport::parse(
            r#"{"calls":[{"method":"Page.getLayoutMetrics","params":{},"result":{}}]}"#,
        )
        .unwrap();
        let mut session = BrowserSession::new(transport);
        let err = session.observe().unwrap_err();
        assert_eq!(
            err,
            BrowserError::BadViewport("missing cssLayoutViewport".into())
        );
        assert_eq!(err.to_string(), "viewport: missing cssLayoutViewport");

        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport
            .append(
                r#"{"calls":[{"method":"DOM.resolveNode","params":{"nodeId":10},"result":{"object":{"type":"object"}}}]}"#,
            )
            .unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let err = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap_err();
        assert_eq!(err, BrowserError::MissingObjectId);
        assert_eq!(err.to_string(), "DOM.resolveNode returned no objectId");
        assert!(session
            .transport()
            .logged_methods()
            .iter()
            .all(
                |method| method != "Runtime.callFunctionOn" && method != "Input.dispatchMouseEvent"
            ));
    }

    #[test]
    fn press_uses_dom_semantic_click_when_a_dom_node_id_exists() {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport
            .append(include_str!("../../../fixtures/press-only.cdp.json"))
            .unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let id = RegionId::try_new("n100").unwrap();
        let mechanism = session.press(&id, Action::Click).unwrap();
        assert_eq!(mechanism, ActMechanism::DomSemantic);
        let methods = session.transport().logged_methods();
        assert!(methods
            .iter()
            .any(|method| method == "Runtime.callFunctionOn"));
        assert!(methods
            .iter()
            .all(|method| method != "Input.dispatchMouseEvent"));
        let err = session.press(&id, Action::Type).unwrap_err();
        assert_eq!(err, BrowserError::UnsupportedAction("type".into()));
    }

    fn sign_in_then(script: &str) -> BrowserSession<ReplayTransport> {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport.append(script).unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        session
    }

    #[test]
    fn node_id_failure_retries_by_backend_id_before_coordinates() {
        let mut session = sign_in_then(
            r#"{"calls":[
                {"method":"DOM.resolveNode","params":{"nodeId":10},"error":"node gone"},
                {"method":"DOM.resolveNode","params":{"backendNodeId":100},"result":{"object":{"objectId":"obj-100"}}},
                {"method":"Runtime.callFunctionOn","params":{"functionDeclaration":"function(){this.click()}","objectId":"obj-100","returnByValue":true},"result":{"result":{"type":"undefined"}}}
            ]}"#,
        );
        let mechanism = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        assert_eq!(mechanism, ActMechanism::DomSemantic);
        let methods = session.transport().logged_methods();
        assert!(methods.iter().all(|method| method != "DOM.focus"));
        assert!(methods
            .iter()
            .all(|method| method != "Input.dispatchMouseEvent"));
    }

    #[test]
    fn both_semantic_tiers_fail_then_coordinates() {
        let mut session = sign_in_then(
            r#"{"calls":[
                {"method":"DOM.resolveNode","params":{"nodeId":10},"error":"node gone"},
                {"method":"DOM.resolveNode","params":{"backendNodeId":100},"error":"node gone"},
                {"method":"Input.dispatchMouseEvent","params":{"type":"mousePressed","x":440.0,"y":316.0,"button":"left","clickCount":1},"result":{}},
                {"method":"Input.dispatchMouseEvent","params":{"type":"mouseReleased","x":440.0,"y":316.0,"button":"left","clickCount":1},"result":{}}
            ]}"#,
        );
        let mechanism = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        assert_eq!(mechanism, ActMechanism::Coordinate);
        assert_eq!(mechanism.as_str(), "coordinate");
        assert!(session
            .transport()
            .logged_methods()
            .iter()
            .all(|method| method != "DOM.focus"));
    }

    #[test]
    fn click_exception_is_a_tier_failure() {
        let mut session = sign_in_then(
            r#"{"calls":[
                {"method":"DOM.resolveNode","params":{"nodeId":10},"result":{"object":{"objectId":"obj-10"}}},
                {"method":"Runtime.callFunctionOn","params":{"functionDeclaration":"function(){this.click()}","objectId":"obj-10","returnByValue":true},"result":{"result":{"type":"object"},"exceptionDetails":{"text":"this.click is not a function"}}},
                {"method":"DOM.resolveNode","params":{"backendNodeId":100},"result":{"object":{"objectId":"obj-100"}}},
                {"method":"Runtime.callFunctionOn","params":{"functionDeclaration":"function(){this.click()}","objectId":"obj-100","returnByValue":true},"result":{"result":{"type":"undefined"}}}
            ]}"#,
        );
        let mechanism = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        assert_eq!(mechanism, ActMechanism::DomSemantic);
        let resolves = session
            .transport()
            .logged_methods()
            .iter()
            .filter(|method| *method == "DOM.resolveNode")
            .count();
        assert_eq!(resolves, 2);
    }

    /// sign-in.cdp.json with the Sign in button re-rendered as node 90 /
    /// backend 900 at `x`.
    fn rerendered_sign_in(x: f64) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let calls = value["calls"].as_array_mut().unwrap();
        let button = &mut calls[1]["result"]["root"]["children"][0];
        button["nodeId"] = 90.into();
        button["backendNodeId"] = 900.into();
        calls[2]["result"]["nodes"][0]["backendDOMNodeId"] = 900.into();
        let quad = serde_json::json!([x, 300, x + 80.0, 300, x + 80.0, 332, x, 332]);
        calls[3]["params"] = serde_json::json!({"nodeId": 90});
        calls[3]["result"]["model"]["content"] = quad.clone();
        calls[5]["params"] = serde_json::json!({"backendNodeId": 900});
        calls[5]["result"]["model"]["content"] = quad;
        value.to_string()
    }

    #[test]
    fn rerendered_button_keeps_its_id_with_a_new_backend_node() {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport.append(&rerendered_sign_in(400.0)).unwrap();
        transport
            .append(
                r#"{"calls":[
                    {"method":"DOM.resolveNode","params":{"nodeId":90},"result":{"object":{"objectId":"obj-90"}}},
                    {"method":"Runtime.callFunctionOn","params":{"functionDeclaration":"function(){this.click()}","objectId":"obj-90","returnByValue":true},"result":{"result":{"type":"undefined"}}}
                ]}"#,
            )
            .unwrap();
        let mut session = BrowserSession::new(transport);
        let before = session.observe().unwrap().clone();
        let after = session.observe().unwrap().clone();
        let ids: Vec<_> = after.ids().map(RegionId::as_str).collect();
        assert_eq!(ids, ["n100", "n200"]);
        assert!(diff(&before, &after).is_empty());
        let mechanism = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        assert_eq!(mechanism, ActMechanism::DomSemantic);
    }

    #[test]
    fn distant_same_label_does_not_inherit_identity() {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport.append(&rerendered_sign_in(1100.0)).unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let after = session.observe().unwrap();
        let ids: Vec<_> = after.ids().map(RegionId::as_str).collect();
        assert_eq!(ids, ["n200", "n900"]);
    }

    #[test]
    fn observe_records_the_nearest_dom_parent() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let calls = value["calls"].as_array_mut().unwrap();
        let buttons = calls[1]["result"]["root"]["children"].take();
        calls[1]["result"]["root"]["children"] = serde_json::json!([{
            "nodeId": 5,
            "backendNodeId": 50,
            "nodeType": 1,
            "nodeName": "NAV",
            "attributes": ["aria-label", "Account"],
            "children": buttons
        }]);
        calls.insert(
            3,
            serde_json::json!({
                "method": "DOM.getBoxModel",
                "params": {"nodeId": 5},
                "result": {"model": {"content": [380, 280, 500, 280, 500, 400, 380, 400]}}
            }),
        );
        let transport = ReplayTransport::parse(&value.to_string()).unwrap();
        let mut session = BrowserSession::new(transport);
        let manifold = session.observe().unwrap();
        assert_eq!(manifold.len(), 3);
        let parent = |id: &str| {
            manifold
                .get_str(id)
                .unwrap()
                .parent()
                .map(|parent| parent.to_string())
        };
        assert_eq!(parent("n100").as_deref(), Some("n50"));
        assert_eq!(parent("n200").as_deref(), Some("n50"));
        assert_eq!(parent("n50"), None);
    }

    #[test]
    fn single_observation_ids_are_unchanged() {
        let transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let mut session = BrowserSession::new(transport);
        let ids: Vec<_> = session
            .observe()
            .unwrap()
            .ids()
            .map(|id| id.to_string())
            .collect();
        assert_eq!(ids, ["n100", "n200"]);
    }

    #[test]
    fn observe_omits_a_node_whose_box_model_is_a_protocol_error() {
        let transport =
            ReplayTransport::parse(include_str!("../../../fixtures/hidden-node.cdp.json")).unwrap();
        let mut session = BrowserSession::new(transport);
        let manifold = session.observe().unwrap();
        assert_eq!(manifold.len(), 2);
        assert!(manifold.get_str("n300").is_none());
        let sign_in = manifold.get_str("n100").unwrap();
        assert!(sign_in.sources().contains(SourceMask::ACCESSIBILITY));
    }

    #[test]
    fn observe_still_fails_when_the_box_model_step_is_missing() {
        let full = include_str!("../../../fixtures/sign-in.cdp.json");
        let mut value: serde_json::Value = serde_json::from_str(full).unwrap();
        value["calls"].as_array_mut().unwrap().truncate(4);
        let transport = ReplayTransport::parse(&value.to_string()).unwrap();
        let mut session = BrowserSession::new(transport);
        let err = session.observe().unwrap_err();
        assert_eq!(
            err,
            BrowserError::Cdp(CdpError::NoScriptedResponse {
                method: "DOM.getBoxModel".into()
            })
        );
    }

    #[test]
    fn box_model_params_mismatch_is_still_fatal() {
        let full = include_str!("../../../fixtures/sign-in.cdp.json");
        let mut value: serde_json::Value = serde_json::from_str(full).unwrap();
        value["calls"][3]["params"] = serde_json::json!({"nodeId": 99});
        let transport = ReplayTransport::parse(&value.to_string()).unwrap();
        let mut session = BrowserSession::new(transport);
        let err = session.observe().unwrap_err();
        assert_eq!(
            err,
            BrowserError::Cdp(CdpError::ParamsMismatch {
                method: "DOM.getBoxModel".into()
            })
        );
    }

    #[test]
    fn refresh_then_verify_sees_welcome_and_diffs_the_old_button() {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport
            .append(include_str!("../../../fixtures/press-only.cdp.json"))
            .unwrap();
        transport
            .append(include_str!("../../../fixtures/welcome.cdp.json"))
            .unwrap();
        let mut session = BrowserSession::new(transport);
        let before = session.observe().unwrap().clone();
        session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        let after = session.observe().unwrap().clone();
        let delta = diff(&before, &after);
        assert!(delta.removed().iter().any(|id| id.as_str() == "n100"));
        assert!(delta.added().iter().any(|id| id.as_str() == "n300"));
        session
            .verify(&Expectation::text_present("Welcome").unwrap())
            .unwrap();
        let err = verify(&before, &Expectation::text_present("Welcome").unwrap()).unwrap_err();
        assert_eq!(
            err,
            VerifyError::ExpectedTextMissing {
                expected: "Welcome".into()
            }
        );
    }

    #[test]
    #[ignore = "read-only attach to a local Chrome; set HYPER_USE_CDP=http://127.0.0.1:PORT"]
    fn live_browser_get_version_reads_a_cdp_socket() {
        let endpoint =
            std::env::var("HYPER_USE_CDP").unwrap_or_else(|_| DEFAULT_CDP_HTTP.to_owned());
        let mut socket = WebSocketTransport::connect(&endpoint).expect("connect");
        let version = socket.call("Browser.getVersion", "{}").expect("version");
        assert!(
            version.contains("Chrome") || version.contains("protocolVersion"),
            "{version}"
        );
    }

    fn history_url(script: &str, url: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(script).unwrap();
        let history = value["calls"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|call| call["method"] == "Page.getNavigationHistory")
            .unwrap();
        history["result"]["entries"][0]["url"] = serde_json::json!(url);
        value.to_string()
    }

    #[test]
    fn url_change_is_reported_without_a_region_change() {
        let base = include_str!("../../../fixtures/sign-in.cdp.json");
        let mut transport =
            ReplayTransport::parse(&history_url(base, "https://example.test/sign-in")).unwrap();
        transport
            .append(&history_url(base, "https://example.test/account"))
            .unwrap();
        let mut session = BrowserSession::new(transport);
        let before = session.observe().unwrap().clone();
        let before_page = session.page().unwrap().clone();
        let after = session.observe().unwrap().clone();
        let after_page = session.page().unwrap().clone();
        assert!(diff(&before, &after).is_empty());
        assert_eq!(before.captured_at_ms(), 0);
        assert_eq!(after.captured_at_ms(), 0);
        let pages = page_delta(&before_page, &after_page);
        assert!(pages.url_changed());
        assert!(!pages.focus_changed());
        assert_eq!(after_page.url(), "https://example.test/account");
        verify_delta(&diff(&before, &after), &pages, &Expectation::url_changed()).unwrap();
    }

    fn focus_sign_in(script: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(script).unwrap();
        value["calls"][2]["result"]["nodes"][0]["properties"] = serde_json::json!([{
            "name": "focused",
            "value": {"type": "boolean", "value": true}
        }]);
        value.to_string()
    }

    fn with_focused_email(script: &str) -> String {
        let mut value: serde_json::Value = serde_json::from_str(script).unwrap();
        let calls = value["calls"].as_array_mut().unwrap();
        calls[1]["result"]["root"]["children"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "nodeId": 30,
                "backendNodeId": 300,
                "nodeType": 1,
                "nodeName": "INPUT",
                "attributes": ["type", "email", "aria-label", "Email"],
                "children": []
            }));
        calls[2]["result"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "nodeId": "ax30",
                "backendDOMNodeId": 300,
                "ignored": false,
                "role": {"value": "textbox"},
                "name": {"value": "Email"},
                "properties": [{
                    "name": "focused",
                    "value": {"type": "boolean", "value": true}
                }]
            }));
        let node_20 = calls
            .iter()
            .position(|call| {
                call["method"] == "DOM.getBoxModel"
                    && call["params"].get("nodeId").and_then(|id| id.as_i64()) == Some(20)
            })
            .unwrap();
        calls.insert(
            node_20 + 1,
            serde_json::json!({
                "method": "DOM.getBoxModel",
                "params": {"nodeId": 30},
                "result": {"model": {"content": [400, 360, 560, 360, 560, 392, 400, 392]}}
            }),
        );
        let history_at = calls
            .iter()
            .position(|call| call["method"] == "Page.getNavigationHistory")
            .unwrap();
        calls.insert(
            history_at,
            serde_json::json!({
                "method": "DOM.getBoxModel",
                "params": {"backendNodeId": 300},
                "result": {"model": {"content": [400, 360, 560, 360, 560, 392, 400, 392]}}
            }),
        );
        value.to_string()
    }

    #[test]
    fn focus_moves_to_the_text_field() {
        let base = include_str!("../../../fixtures/sign-in.cdp.json");
        let mut transport = ReplayTransport::parse(&focus_sign_in(base)).unwrap();
        transport.append(&with_focused_email(base)).unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let before_page = session.page().unwrap().clone();
        assert_eq!(
            before_page.focused().map(hyper_use_core::RegionId::as_str),
            Some("n100")
        );
        let after = session.observe().unwrap();
        let email = after.get_str("n300").unwrap();
        assert_eq!(email.role(), Role::TextField);
        assert_eq!(email.label(), "Email");
        let after_page = session.page().unwrap();
        assert_eq!(
            after_page.focused().map(hyper_use_core::RegionId::as_str),
            Some("n300")
        );
        let pages = page_delta(&before_page, after_page);
        assert!(pages.focus_changed());
        assert!(!pages.url_changed());
    }

    #[test]
    fn navigation_history_protocol_error_omits_url_and_title() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let history = value["calls"].as_array_mut().unwrap().last_mut().unwrap();
        assert_eq!(history["method"], "Page.getNavigationHistory");
        history.as_object_mut().unwrap().remove("result");
        history["error"] = serde_json::json!("Inspector not attached");
        let transport = ReplayTransport::parse(&value.to_string()).unwrap();
        let mut session = BrowserSession::new(transport);
        let manifold = session.observe().unwrap();
        assert_eq!(manifold.captured_at_ms(), 0);
        let page = session.page().unwrap();
        assert_eq!(page.url(), "");
        assert_eq!(page.title(), "");
        assert!(page.focused().is_none());
    }

    #[test]
    fn missing_navigation_history_step_is_fatal() {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let removed = value["calls"].as_array_mut().unwrap().pop().unwrap();
        assert_eq!(removed["method"], "Page.getNavigationHistory");
        let transport = ReplayTransport::parse(&value.to_string()).unwrap();
        let mut session = BrowserSession::new(transport);
        let err = session.observe().unwrap_err();
        assert_eq!(
            err,
            BrowserError::Cdp(CdpError::NoScriptedResponse {
                method: "Page.getNavigationHistory".into(),
            })
        );
        assert_eq!(
            err.to_string(),
            "no scripted CDP response for `Page.getNavigationHistory`"
        );
    }
}

#[cfg(test)]
mod exact_errors {
    use super::script::{DomSpec, PageSpec, ScriptBuilder};
    use super::*;
    use hyper_use_core::{Action, RegionId};

    #[test]
    fn a_scripted_cdp_error_is_the_exact_protocol_error() {
        let mut transport =
            ReplayTransport::parse(r#"{"calls":[{"method":"DOM.resolveNode","error":"boom"}]}"#)
                .unwrap();
        assert_eq!(
            transport.call("DOM.resolveNode", "{}"),
            Err(CdpError::Protocol {
                message: "boom".into()
            })
        );
    }

    #[test]
    fn session_verify_wraps_the_exact_verify_error() {
        let transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let err = session
            .verify(&Expectation::text_present("Welcome").unwrap())
            .unwrap_err();
        assert_eq!(
            err,
            BrowserError::Verify(VerifyError::ExpectedTextMissing {
                expected: "Welcome".into()
            })
        );
    }

    #[test]
    fn press_on_an_unknown_region_is_exact() {
        let transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let err = session
            .press(&RegionId::try_new("nope").unwrap(), Action::Click)
            .unwrap_err();
        assert_eq!(err, BrowserError::UnknownRegion("nope".into()));
    }

    #[test]
    fn two_dom_nodes_with_one_backend_id_are_the_exact_duplicate_error() {
        let page = PageSpec::new(
            vec![
                DomSpec::button(10, 100, "Sign in", (400.0, 300.0, 80.0, 32.0)),
                DomSpec::button(20, 100, "Cancel", (400.0, 360.0, 80.0, 32.0)),
            ],
            Vec::new(),
            "https://example.test/",
            "Example",
        );
        let script = ScriptBuilder::new().observe(&page).to_json();
        let mut session = BrowserSession::new(ReplayTransport::parse(&script).unwrap());
        let err = session.observe().unwrap_err();
        assert_eq!(
            err,
            BrowserError::DuplicateRegion("duplicate region id `n100`".into())
        );
    }
}
