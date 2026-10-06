//! Cloudflare Workers AI Clef transport for `RemotePolicy` — feature `clef`.
//!
//! Same closed remote wire as `TypesafeTransport`: the System One body
//! (`state` + speculative `operation` / `<kind>_target` questions) built by
//! `build_questions`, POSTed to the Workers AI endpoint
//! `…/accounts/{id}/ai/run/@cf/cloudflare/{model}`. `@cf/cloudflare/clef`
//! (27B) and `@cf/cloudflare/clef-flash` (9B) are Clef decision models in
//! the Jev family: one forward pass scores every offered option; there is
//! no free-form text to parse.
//!
//! Answers are validated exactly like the jev leg (`choice` is an offered
//! key, the probabilities cover the offered set, sum ≈ 1, the choice is the
//! argmax) and mapped back into the closed reply: exactly
//! `{"choice":{"id","kind"}}` or `{"abstain"}`. The model can only ever pick
//! an offered id — anything malformed or off-menu is a hard error before it
//! becomes a `PolicyError`.
//!
//! Env: `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` (both required,
//! token needs Workers AI read). `CLEF_MODEL` overrides the model slug set by
//! `--policy clef` / `--policy clef-flash`. `CLEF_BASE_URL` overrides the
//! Workers AI endpoint template for a self-hosted Clef server speaking the
//! same request body.
//!
//! Timeout bounds: one 60 s attempt per call, no retry. The call only asks
//! for a decision and runs before any ticket is issued, so a timeout never
//! repeats page input — the agent sees an error, not a silent double-act.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{Map, Value};
use typesafe_sdk::ChoiceAnswer;

use aui_policy::RemoteTransport;

use crate::typesafe::{build_questions, compose_reply, BuiltQuestions};

const MODEL_ENV: &str = "CLEF_MODEL";
const BASE_URL_ENV: &str = "CLEF_BASE_URL";
const ACCOUNT_ENV: &str = "CLOUDFLARE_ACCOUNT_ID";
const API_KEY_ENV: &str = "CLOUDFLARE_API_TOKEN";
const DEFAULT_URL: &str = "https://api.cloudflare.com/client/v4/accounts";
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Cloudflare Workers AI Clef transport: the System One body over the
/// Workers AI `ai/run` endpoint.
pub struct ClefTransport {
    http: Client,
    url: String,
    api_key: String,
    model: String,
}

impl ClefTransport {
    /// `model` is the Clef slug (`clef` or `clef-flash`); `CLEF_MODEL`
    /// overrides it. `CLEF_BASE_URL` replaces the endpoint template — the
    /// account id, `/@cf/cloudflare/` path, and model slug are appended to it,
    /// so a self-hosted Clef with a different path layout needs its own URL.
    pub fn from_env(model: &str) -> Result<Self, String> {
        let account = required_env(ACCOUNT_ENV)?;
        let api_key = required_env(API_KEY_ENV)?;
        let model = std::env::var(MODEL_ENV)
            .ok()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| model.to_owned());
        let base = std::env::var(BASE_URL_ENV).unwrap_or_else(|_| DEFAULT_URL.to_owned());
        let http = Client::builder()
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(|err| format!("clef http: {err}"))?;
        Ok(Self {
            http,
            url: format!(
                "{}/{account}/ai/run/@cf/cloudflare/{model}",
                base.trim_end_matches('/')
            ),
            api_key,
            model,
        })
    }

    /// The model slug this transport calls (after `CLEF_MODEL` overrides).
    pub fn model(&self) -> &str {
        &self.model
    }
}

fn required_env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{name} is not set"))
}

/// A Clef `choice` question needs at least two criteria (Workers AI rejects
/// a single-option dictionary with 422). A head with exactly one offered
/// candidate is already decided, so it never reaches the wire: `auto` maps
/// head name to its sole candidate and `reply_from_answers` answers it
/// locally with probability 1.
struct ClefBody {
    body: Value,
    criteria_keys: Vec<(String, Vec<String>)>,
    /// head name -> sole candidate, answered locally.
    auto: BTreeMap<String, String>,
}

/// Clef request body for one wire request, keeping `criteria_keys` for
/// answer validation.
fn body_for(request: &Value, model: &str) -> Result<ClefBody, String> {
    let BuiltQuestions {
        state,
        questions,
        criteria_keys,
    } = build_questions(request)?;
    let mut map = Map::new();
    let mut auto = BTreeMap::new();
    for ((name, question), (_, keys)) in questions.into_iter().zip(criteria_keys.iter()) {
        if name != "operation" && keys.len() == 1 {
            auto.insert(name, keys[0].clone());
            continue;
        }
        let value = serde_json::to_value(&question)
            .map_err(|err| format!("clef question `{name}`: {err}"))?;
        map.insert(name, value);
    }
    Ok(ClefBody {
        body: serde_json::json!({"model": model, "state": state, "questions": Value::Object(map)}),
        criteria_keys,
        auto,
    })
}

/// Map `result.answers` of a Clef response into the closed reply. Heads in
/// `auto` were decided locally (sole candidate) and answer themselves.
fn reply_from_answers(
    answers: &Value,
    criteria_keys: &[(String, Vec<String>)],
    auto: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut answers: BTreeMap<String, ChoiceAnswer> = answers
        .as_object()
        .ok_or("clef response: `answers` is not an object")?
        .iter()
        .map(|(head, value)| {
            let choice = value["choice"]
                .as_str()
                .ok_or_else(|| format!("clef answer `{head}`: `choice` missing"))?;
            let confidence = value["confidence"].as_f64().unwrap_or(0.0);
            let probabilities = value["probabilities"]
                .as_object()
                .ok_or_else(|| format!("clef answer `{head}`: `probabilities` missing"))?
                .iter()
                .map(|(key, value)| {
                    value.as_f64().map(|p| (key.clone(), p)).ok_or_else(|| {
                        format!("clef answer `{head}`: probability `{key}` is not a number")
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((
                head.clone(),
                ChoiceAnswer::new(choice, confidence, probabilities),
            ))
        })
        .collect::<Result<_, String>>()?;
    for (head, candidate) in auto {
        answers.insert(
            head.clone(),
            ChoiceAnswer::new(candidate, 1.0, [(candidate.clone(), 1.0)]),
        );
    }
    compose_reply(criteria_keys, |head| {
        answers
            .get(head)
            .ok_or_else(|| format!("clef answer: missing `{head}`"))
    })
}

impl RemoteTransport for ClefTransport {
    fn call(&mut self, request_json: &str) -> Result<String, String> {
        let request: Value =
            serde_json::from_str(request_json).map_err(|err| format!("clef request: {err}"))?;
        let ClefBody {
            body,
            criteria_keys,
            auto,
        } = body_for(&request, &self.model)?;
        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|err| format!("clef call: {err}"))?;
        let status = response.status();
        let text = response.text().map_err(|err| format!("clef call: {err}"))?;
        let response: Value = serde_json::from_str(&text).map_err(|err| {
            format!(
                "clef response ({status}): {err}: {}",
                &text[..text.len().min(300)]
            )
        })?;
        if !status.is_success() {
            return Err(format!(
                "clef http {status}: {}",
                &text[..text.len().min(300)]
            ));
        }
        if response["success"].as_bool() == Some(false) {
            return Err(format!("clef api: {}", response["errors"]));
        }
        reply_from_answers(&response["result"]["answers"], &criteria_keys, &auto)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = r#"{
        "goal": "Send the message",
        "actions": [
            {"id": "CLICK:n12", "kind": "CLICK", "label": "Send", "role": "button"},
            {"id": "CLICK:n30", "kind": "CLICK", "label": "Cancel", "role": "button"},
            {"id": "TYPE_TEXT:n44", "kind": "TYPE_TEXT", "label": "Message", "role": "textbox"},
            {"id": "SCROLL_DOWN", "kind": "SCROLL_DOWN", "label": "Scroll the page down"},
            {"id": "DONE", "kind": "DONE", "label": "Every requirement is visibly satisfied"}
        ],
        "history": [{"step": 1, "id": "CLICK:n1", "verification": "success"}]
    }"#;

    fn request() -> Value {
        serde_json::from_str(REQUEST).unwrap()
    }

    fn answers(value: Value) -> Value {
        serde_json::json!({"answers": value})
    }

    fn keys_and_auto(request: &Value) -> ClefBody {
        body_for(request, "clef-flash").unwrap()
    }

    #[test]
    fn body_keeps_the_systemone_shape() {
        let ClefBody { body, .. } = body_for(&request(), "clef-flash").unwrap();
        assert_eq!(body["model"], "clef-flash");
        assert_eq!(body["state"]["goal"], "Send the message");
        assert!(body["state"]["actions"].is_array());
        assert!(body["state"]["recent_actions"].is_array());
        let questions = body["questions"].as_object().unwrap();
        let names: Vec<&str> = questions.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "operation",
                "click_target",
                "type_text_target",
                "select_target"
            ]
            .into_iter()
            .filter(|name| questions.contains_key(*name))
            .collect::<Vec<_>>()
        );
        let operation = &questions["operation"];
        assert_eq!(operation["type"], "choice");
        assert_eq!(operation["instructions"]["goal"], "Send the message");
        let click = &questions["click_target"];
        assert!(click["instructions"]["rules"].is_array());
        let criteria = click["criteria"].as_object().unwrap();
        assert!(criteria.contains_key("n12") && criteria.contains_key("n30"));
    }

    #[test]
    fn valid_choice_maps_to_closed_reply() {
        let ClefBody {
            criteria_keys,
            auto,
            ..
        } = keys_and_auto(&request());
        let value = answers(serde_json::json!({
            "operation": {
                "type": "choice",
                "choice": "CLICK",
                "confidence": 0.9,
                "probabilities": {"CLICK": 0.9, "TYPE_TEXT": 0.05, "SCROLL_DOWN": 0.03, "DONE": 0.02}
            },
            "click_target": {
                "type": "choice",
                "choice": "n12",
                "confidence": 0.8,
                "probabilities": {"n12": 0.8, "n30": 0.2}
            }
        }));
        let reply = reply_from_answers(&value["answers"], &criteria_keys, &auto).unwrap();
        assert_eq!(reply, r#"{"choice":{"id":"CLICK:n12","kind":"CLICK"}}"#);
    }

    #[test]
    fn control_choice_needs_no_target() {
        let ClefBody {
            criteria_keys,
            auto,
            ..
        } = keys_and_auto(&request());
        let value = answers(serde_json::json!({
            "operation": {
                "type": "choice",
                "choice": "DONE",
                "confidence": 0.9,
                "probabilities": {"CLICK": 0.05, "TYPE_TEXT": 0.03, "SCROLL_DOWN": 0.02, "DONE": 0.9}
            }
        }));
        let reply = reply_from_answers(&value["answers"], &criteria_keys, &auto).unwrap();
        assert_eq!(reply, r#"{"choice":{"id":"DONE","kind":"DONE"}}"#);
    }

    #[test]
    fn off_menu_choice_is_a_hard_error() {
        let ClefBody {
            criteria_keys,
            auto,
            ..
        } = keys_and_auto(&request());
        let value = answers(serde_json::json!({
            "operation": {
                "type": "choice",
                "choice": "JUMP",
                "confidence": 0.9,
                "probabilities": {"CLICK": 0.1, "TYPE_TEXT": 0.0, "SCROLL_DOWN": 0.0, "DONE": 0.9}
            }
        }));
        let err = reply_from_answers(&value["answers"], &criteria_keys, &auto).unwrap_err();
        assert!(err.contains("off-menu"), "{err}");
    }

    #[test]
    fn missing_target_head_is_a_hard_error() {
        let ClefBody {
            criteria_keys,
            auto,
            ..
        } = keys_and_auto(&request());
        let value = answers(serde_json::json!({
            "operation": {
                "type": "choice",
                "choice": "CLICK",
                "confidence": 0.9,
                "probabilities": {"CLICK": 0.9, "TYPE_TEXT": 0.05, "SCROLL_DOWN": 0.03, "DONE": 0.02}
            }
        }));
        let err = reply_from_answers(&value["answers"], &criteria_keys, &auto).unwrap_err();
        assert!(err.contains("click_target"), "{err}");
    }
}
