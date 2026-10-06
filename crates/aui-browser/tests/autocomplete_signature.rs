use aui_browser::script::ScriptBuilder;
use aui_browser::{BrowserSession, ReplayTransport};
use serde_json::json;

fn session(script: &str) -> BrowserSession<ReplayTransport> {
    BrowserSession::new(ReplayTransport::parse(script).unwrap())
}

#[test]
fn autocomplete_signature_reads_a_string() {
    let script = ScriptBuilder::new()
        .evaluate_value(json!("2:Manila\u{001f}Makati"))
        .to_json();
    let mut session = session(&script);
    assert_eq!(
        session.autocomplete_options_signature().unwrap(),
        Some("2:Manila\u{001f}Makati".to_owned())
    );
}

#[test]
fn autocomplete_signature_ignores_non_string_values() {
    let script = ScriptBuilder::new().evaluate_value(json!(7)).to_json();
    let mut session = session(&script);
    assert_eq!(session.autocomplete_options_signature().unwrap(), None);
}

#[test]
fn autocomplete_signature_ignores_protocol_errors() {
    let mut session = session(
        r#"{"calls":[{"method":"Runtime.evaluate","error":"execution context was destroyed"}]}"#,
    );
    assert_eq!(session.autocomplete_options_signature().unwrap(), None);
}
