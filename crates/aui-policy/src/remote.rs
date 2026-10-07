//! Optional remote (Jev / System-One / any host model) fallback policy.
//!
//! Feature `remote`. No network stack and no TypeSafe/Jev dependency: the
//! consumer supplies a [`RemoteTransport`] that carries one JSON request to
//! its model and returns one JSON reply. Ultra-Instinct owns the wire contract and
//! enforces the same finite [`ActionSpace`] as Instinct:
//!
//! Request (one object):
//!
//! ```json
//! {"goal": "...",
//!  "actions": [{"id": "TYPE_TEXT:n4", "kind": "TYPE_TEXT", "label": "Name",
//!               "state": {"value": "Ana"}}, ...],
//!  "history": [{"step": 1, "id": "TYPE_TEXT:n4", "kind": "TYPE_TEXT",
//!               "label": "Name", "verification": "success"}]}
//! ```
//!
//! Region-bound actions include nonempty observed control state as additive
//! evidence. The reply remains closed to the offered id and kind.
//!
//! Reply (exactly one of):
//!
//! ```json
//! {"choice": {"id": "CLICK:n12", "kind": "CLICK"}}
//! {"abstain": "why"}
//! ```
//!
//! Anything else is a hard [`PolicyError`]: an id not offered, a kind that does
//! not match the offered id, extra selector / coordinate / script fields, or
//! malformed JSON. A reply can never carry a CSS selector, coordinates, shell,
//! or JavaScript into execution because only offered ids are accepted.
//!
//! [`UnconfiguredRemote`] is the stub: it always abstains, so wiring an
//! `EscalatingPolicy<InstinctPolicy, UnconfiguredRemote>` changes nothing until a
//! real transport is plugged in.

use serde_json::{json, Map, Value};

use aui_core::{ActionKind, ActionSpace};

use crate::goal::AgentGoal;
use crate::types::{BrowserPolicy, HistoryEntry, PolicyDecision, PolicyError, PolicyOutcome};

/// Carries one request to a remote model and returns its raw reply.
pub trait RemoteTransport {
    fn call(&mut self, request_json: &str) -> Result<String, String>;
}

/// Remote policy over a consumer transport. See the module docs for the wire.
pub struct RemotePolicy<T> {
    transport: T,
    calls: u32,
    name: &'static str,
}

impl<T: RemoteTransport> RemotePolicy<T> {
    pub fn new(transport: T) -> Self {
        Self::named("remote", transport)
    }

    /// A remote policy whose decisions are recorded under `name` in the diary.
    pub fn named(name: &'static str, transport: T) -> Self {
        Self {
            transport,
            calls: 0,
            name,
        }
    }

    /// Remote calls made (escalations), for eval accounting.
    pub fn calls(&self) -> u32 {
        self.calls
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

/// Build the request object for `space` / `goal` / `history`.
///
/// Each action carries `id`, `kind`, `label`, and — for region-bound
/// actions — `role` and nonempty observed `state`. History includes the
/// executed action's `kind` and `label`. Extra fields are additive for
/// transports; a consumer may ignore them.
pub fn request_json(space: &ActionSpace, goal: &AgentGoal, history: &[HistoryEntry]) -> String {
    let actions: Vec<Value> = space
        .actions()
        .map(|a| {
            let mut action =
                json!({"id": a.id().as_str(), "kind": a.kind().as_str(), "label": a.label()});
            if let Some(role) = a.role() {
                action["role"] = json!(role.as_str());
            }
            let observed = a.state();
            if !observed.is_empty() {
                let mut state = Map::new();
                if let Some(value) = &observed.value {
                    state.insert("value".to_owned(), json!(value));
                }
                if let Some(checked) = observed.checked {
                    state.insert("checked".to_owned(), json!(checked));
                }
                if let Some(expanded) = observed.expanded {
                    state.insert("expanded".to_owned(), json!(expanded));
                }
                if let Some(selected) = &observed.selected {
                    state.insert("selected".to_owned(), json!(selected));
                }
                if !observed.options.is_empty() {
                    state.insert("options".to_owned(), json!(&observed.options));
                }
                action["state"] = Value::Object(state);
            }
            action
        })
        .collect();
    let history: Vec<Value> = history
        .iter()
        .map(|h| {
            json!({
                "step": h.step,
                "id": h.action_id.as_str(),
                "kind": h.kind.as_str(),
                "label": h.label,
                "verification": h.verification
            })
        })
        .collect();
    json!({"goal": goal.as_str(), "actions": actions, "history": history}).to_string()
}

/// Parse and validate a reply against the offered space.
pub fn parse_reply(space: &ActionSpace, reply: &str) -> Result<PolicyOutcome, PolicyError> {
    let bad = |m: &str| PolicyError::Internal(format!("remote reply: {m}"));
    let value: Value = serde_json::from_str(reply).map_err(|e| bad(&e.to_string()))?;
    let obj = value.as_object().ok_or_else(|| bad("not an object"))?;
    if obj.len() != 1 {
        return Err(bad("expected exactly one of `choice` or `abstain`"));
    }
    if let Some(reason) = obj.get("abstain") {
        let reason = reason
            .as_str()
            .ok_or_else(|| bad("abstain must be a string"))?;
        return Ok(PolicyOutcome::Abstain {
            reason: format!("remote: {reason}"),
            operation_ranked: Vec::new(),
            target_ranked: Vec::new(),
        });
    }
    let choice = obj
        .get("choice")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("expected `choice` object"))?;
    if choice.keys().any(|k| k != "id" && k != "kind") {
        return Err(bad("choice may only carry `id` and `kind`"));
    }
    let id = choice
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("choice.id missing"))?;
    let kind = choice
        .get("kind")
        .and_then(Value::as_str)
        .and_then(ActionKind::parse)
        .ok_or_else(|| bad("choice.kind missing or unknown"))?;
    let offered = space
        .get_str(id)
        .ok_or_else(|| bad(&format!("off-menu id `{id}`")))?;
    if offered.kind() != kind {
        return Err(bad(&format!("id `{id}` is {} not {kind}", offered.kind())));
    }
    Ok(PolicyOutcome::Choice(PolicyDecision {
        action_id: offered.id().clone(),
        kind,
        target_label: offered.label().to_owned(),
        // Remote confidence is not comparable to Instinct millis; never reported as such.
        confidence_millis: 0,
        operation_ranked: Vec::new(),
        target_ranked: Vec::new(),
    }))
}

impl<T: RemoteTransport> BrowserPolicy for RemotePolicy<T> {
    fn decide(
        &mut self,
        space: &ActionSpace,
        goal: &AgentGoal,
        history: &[HistoryEntry],
    ) -> Result<PolicyOutcome, PolicyError> {
        if goal.is_empty() {
            return Err(PolicyError::EmptyGoal);
        }
        self.calls += 1;
        let reply = self
            .transport
            .call(&request_json(space, goal, history))
            .map_err(|e| PolicyError::Internal(format!("remote transport: {e}")))?;
        parse_reply(space, &reply)
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

/// Stub fallback: always abstains. The product builds and runs without a remote.
#[derive(Clone, Debug, Default)]
pub struct UnconfiguredRemote;

impl BrowserPolicy for UnconfiguredRemote {
    fn name(&self) -> &'static str {
        "unconfigured-remote"
    }

    fn decide(
        &mut self,
        _space: &ActionSpace,
        _goal: &AgentGoal,
        _history: &[HistoryEntry],
    ) -> Result<PolicyOutcome, PolicyError> {
        Ok(PolicyOutcome::Abstain {
            reason: "remote policy not configured".to_owned(),
            operation_ranked: Vec::new(),
            target_ranked: Vec::new(),
        })
    }
}

/// Test / replay transport: returns scripted replies in order and records requests.
#[derive(Clone, Debug, Default)]
pub struct ScriptedRemote {
    replies: std::collections::VecDeque<String>,
    requests: Vec<String>,
}

impl ScriptedRemote {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(replies: I) -> Self {
        Self {
            replies: replies.into_iter().map(Into::into).collect(),
            requests: Vec::new(),
        }
    }

    pub fn requests(&self) -> &[String] {
        &self.requests
    }
}

impl RemoteTransport for ScriptedRemote {
    fn call(&mut self, request_json: &str) -> Result<String, String> {
        self.requests.push(request_json.to_owned());
        self.replies
            .pop_front()
            .ok_or_else(|| "no scripted reply".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escalate::EscalatingPolicy;
    use crate::instinct_policy::InstinctPolicy;
    use aui_core::{parse_fixture, ActionId, ElementState};

    fn twins() -> ActionSpace {
        ActionSpace::from_manifold(
            &parse_fixture(
                r#"
                viewport w=800 h=600
                region id=a role=button label="Delete" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
                region id=b role=button label="Delete" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
                "#,
            )
            .unwrap(),
        )
    }

    fn stateful_space() -> ActionSpace {
        let mut manifold = parse_fixture(
            r#"
            viewport w=800 h=600
            region id=name role=text_field label="Name" x=10 y=10 w=160 h=24 actions=type sources=dom
            region id=submit role=button label="Submit" x=10 y=50 w=80 h=24 actions=click sources=dom
            "#,
        )
        .unwrap();
        let name = manifold
            .get_str("name")
            .unwrap()
            .clone()
            .with_state(ElementState {
                value: Some("Ana".to_owned()),
                checked: Some(false),
                expanded: Some(true),
                selected: Some("UTC".to_owned()),
                options: vec!["UTC".to_owned()],
            });
        manifold.replace(name);
        ActionSpace::from_manifold(&manifold)
    }

    #[test]
    fn unconfigured_remote_keeps_instinct_abstain() {
        let mut p = EscalatingPolicy::new(InstinctPolicy::default(), Some(UnconfiguredRemote));
        let out = p.decide(&twins(), &AgentGoal::new("Delete"), &[]).unwrap();
        assert!(matches!(out, PolicyOutcome::Abstain { .. }), "{out:?}");
    }

    #[test]
    fn remote_choice_runs_only_after_instinct_abstains_and_must_be_offered() {
        let remote = RemotePolicy::new(ScriptedRemote::new([
            r#"{"choice":{"id":"CLICK:b","kind":"CLICK"}}"#,
        ]));
        let mut p = EscalatingPolicy::new(InstinctPolicy::default(), Some(remote));
        let out = p.decide(&twins(), &AgentGoal::new("Delete"), &[]).unwrap();
        assert_eq!(out.as_choice().unwrap().action_id.as_str(), "CLICK:b");
        let remote = p.fallback.as_ref().unwrap();
        assert_eq!(remote.calls(), 1);
        let req: Value = serde_json::from_str(&remote.transport().requests()[0]).unwrap();
        assert_eq!(req["goal"], "Delete");
        assert!(req["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == "CLICK:a"));
    }

    #[test]
    fn request_includes_state_only_for_nonempty_actions_and_history_details() {
        let space = stateful_space();
        let history = [HistoryEntry {
            step: 1,
            action_id: ActionId::try_new("TYPE_TEXT:name").unwrap(),
            kind: ActionKind::TypeText,
            label: "Name".to_owned(),
            verification: "success".to_owned(),
        }];
        let request: Value = serde_json::from_str(&request_json(
            &space,
            &AgentGoal::new("Fill the name"),
            &history,
        ))
        .unwrap();
        let actions = request["actions"].as_array().unwrap();
        let name = actions
            .iter()
            .find(|action| action["id"] == "TYPE_TEXT:name")
            .unwrap();
        assert_eq!(
            name["state"],
            json!({
                "value":"Ana",
                "checked":false,
                "expanded":true,
                "selected":"UTC",
                "options":["UTC"]
            })
        );
        let submit = actions
            .iter()
            .find(|action| action["id"] == "CLICK:submit")
            .unwrap();
        assert!(submit.get("state").is_none());
        let done = actions
            .iter()
            .find(|action| action["id"] == "DONE")
            .unwrap();
        assert!(done.get("state").is_none());
        assert_eq!(request["history"][0]["kind"], "TYPE_TEXT");
        assert_eq!(request["history"][0]["label"], "Name");
    }

    #[test]
    fn off_menu_selector_coordinates_and_kind_mismatch_are_hard_errors() {
        let space = twins();
        for reply in [
            r#"{"choice":{"id":"CLICK:ghost","kind":"CLICK"}}"#,
            r#"{"choice":{"id":"CLICK:a","kind":"TYPE_TEXT"}}"#,
            r##"{"choice":{"id":"CLICK:a","kind":"CLICK","selector":"#delete"}}"##,
            r#"{"choice":{"id":"CLICK:a","kind":"CLICK","x":10,"y":20}}"#,
            r#"{"choice":{"id":"CLICK:a","kind":"CLICK"},"abstain":"x"}"#,
            r#"{"js":"document.body.remove()"}"#,
            r#"not json"#,
        ] {
            assert!(parse_reply(&space, reply).is_err(), "{reply}");
        }
        assert!(matches!(
            parse_reply(&space, r#"{"abstain":"unsure"}"#).unwrap(),
            PolicyOutcome::Abstain { .. }
        ));
    }
}
