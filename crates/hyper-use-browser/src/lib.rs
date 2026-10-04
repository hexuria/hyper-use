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
mod replay;
mod session;
mod transport;
mod verify;
mod ws;

pub use error::{ActMechanism, BrowserError, CdpError};
pub use fusion::{MAX_CENTROID_PX, MIN_IOU, MIN_LABEL_JACCARD};
pub use replay::ReplayTransport;
pub use session::{BrowserSession, DOM_CLICK_FUNCTION};
pub use transport::CdpTransport;
pub use verify::{verify, Expectation, VerifyError};
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
    use hyper_use_core::{Action, LocateQuery, RegionId, SourceMask};
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

    #[test]
    fn protocol_error_falls_through_to_element_focus_not_coordinates() {
        let mut transport =
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in.cdp.json")).unwrap();
        transport
            .append(
                r#"{"calls":[
                    {"method":"DOM.resolveNode","params":{"nodeId":10},"error":"node gone"},
                    {"method":"DOM.focus","params":{"nodeId":10},"result":{}}
                ]}"#,
            )
            .unwrap();
        let mut session = BrowserSession::new(transport);
        session.observe().unwrap();
        let mechanism = session
            .press(&RegionId::try_new("n100").unwrap(), Action::Click)
            .unwrap();
        assert_eq!(mechanism, ActMechanism::CdpElement);
        assert!(session
            .transport()
            .logged_methods()
            .iter()
            .all(|method| method != "Input.dispatchMouseEvent"));
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
}
