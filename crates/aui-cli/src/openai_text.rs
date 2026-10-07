//! OpenAI-compatible text model for TYPE_TEXT / SELECT payloads — feature
//! `model-text`, enabled by env when `--text-model-cmd` is absent.
//!
//! Wire parity with jev-ultrafast's text helper: `POST
//! $TEXT_MODEL_BASE_URL/chat/completions` with a Bearer `TEXT_MODEL_API_KEY`,
//! model `TEXT_MODEL`, `response_format: json_object`, `max_tokens` 1024,
//! and jev's reasoning knobs (`TEXT_MODEL_REASONING=none` disables; a
//! DeepSeek base gets `thinking: disabled`, others `reasoning: low`).
//!
//! The reply contract is ours, not jev's: the assistant content must be a
//! single JSON object — `{"text", "context_fingerprint"}` echoing the
//! request's fingerprint, or `{"declined": "<reason>"}`. The resolver re-vets
//! the echo and goal grounding, so a model that invents a value abstains the
//! same way a declined call does.

use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{json, Value};

use aui_policy::{
    parse_command_reply, TextModel, TextModelError, TextModelReply, TextModelRequest,
};

const API_KEY_ENV: &str = "TEXT_MODEL_API_KEY";
const BASE_URL_ENV: &str = "TEXT_MODEL_BASE_URL";
const MODEL_ENV: &str = "TEXT_MODEL";
const REASONING_ENV: &str = "TEXT_MODEL_REASONING";
const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";
const DEFAULT_MODEL: &str = "deepseek-chat";
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// The reply contract the model must satisfy (`parse_command_reply` shape).
const SYSTEM: &str = "You fill one browser field for a web agent. Reply with exactly one JSON \
object: {\"context_fingerprint\": <the request's context_fingerprint, echoed unchanged>, \"text\": \
\"<the value to type>\"} — or {\"declined\": \"<why>\"} when the goal supplies no fitting value. \
Never invent a value the goal does not contain or clearly imply.";

/// OpenAI-compatible `TextModel` configured entirely by jev-ultrafast's
/// `TEXT_MODEL_*` environment.
pub struct OpenAiTextModel {
    http: Client,
    url: String,
    api_key: String,
    model: String,
    reasoning: Value,
}

impl OpenAiTextModel {
    /// `Some` when `TEXT_MODEL_API_KEY` is set; `None` leaves `run` on the
    /// deterministic resolver (same as jev: no key means no model call).
    /// `TEXT_MODEL_BASE_URL` / `TEXT_MODEL` fall back to jev's defaults.
    pub fn from_env() -> Result<Option<Self>, String> {
        let api_key = std::env::var(API_KEY_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty());
        let Some(api_key) = api_key else {
            return Ok(None);
        };
        let base = std::env::var(BASE_URL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());
        let model = std::env::var(MODEL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let reasoning = if std::env::var(REASONING_ENV).ok().as_deref() == Some("none") {
            json!({"reasoning": {"enabled": false}})
        } else if base.contains("api.deepseek.com/") {
            json!({"thinking": {"type": "disabled"}})
        } else {
            json!({"reasoning": {"effort": "low"}})
        };
        let http = Client::builder()
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(|err| format!("text model http: {err}"))?;
        Ok(Some(Self {
            http,
            url: format!("{}/chat/completions", base.trim_end_matches('/')),
            api_key,
            model,
            reasoning,
        }))
    }
}

impl TextModel for OpenAiTextModel {
    fn complete(&mut self, request: &TextModelRequest) -> Result<TextModelReply, TextModelError> {
        let prompt = json!({
            "goal": request.goal,
            "field_label": request.field_label,
            "field_role": request.field_role,
            "context_fingerprint": request.context_fingerprint,
            "max_chars": request.max_chars,
        });
        let mut body = json!({
            "model": self.model,
            "max_tokens": 1024,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": prompt.to_string()},
            ],
        });
        if let Some(map) = body.as_object_mut() {
            for (key, value) in self.reasoning.as_object().into_iter().flatten() {
                map.insert(key.clone(), value.clone());
            }
        }
        let response: Value = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|err| TextModelError::Unavailable(format!("request failed: {err}")))?
            .error_for_status()
            .map_err(|err| TextModelError::Unavailable(format!("http error: {err}")))?
            .json()
            .map_err(|err| TextModelError::Malformed(format!("response not JSON: {err}")))?;
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| TextModelError::Malformed("no assistant content".into()))?;
        parse_command_reply(content.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test covers every env arm: sibling tests could otherwise race on
    /// the shared process environment.
    #[test]
    fn env_parity_with_jev() {
        std::env::remove_var(API_KEY_ENV);
        assert!(OpenAiTextModel::from_env().unwrap().is_none());

        // Mirrors jev-ultrafast field_text(): deepseek gets thinking.disabled,
        // others get reasoning.effort low; "none" turns it off entirely.
        std::env::set_var(API_KEY_ENV, "k");
        std::env::set_var(BASE_URL_ENV, "https://api.deepseek.com/v1");
        std::env::remove_var(REASONING_ENV);
        let m = OpenAiTextModel::from_env().unwrap().unwrap();
        assert_eq!(m.reasoning, json!({"thinking": {"type": "disabled"}}));
        std::env::set_var(BASE_URL_ENV, "https://example.test/v1");
        let m = OpenAiTextModel::from_env().unwrap().unwrap();
        assert_eq!(m.reasoning, json!({"reasoning": {"effort": "low"}}));
        std::env::set_var(REASONING_ENV, "none");
        let m = OpenAiTextModel::from_env().unwrap().unwrap();
        assert_eq!(m.reasoning, json!({"reasoning": {"enabled": false}}));
        assert_eq!(m.url, "https://example.test/v1/chat/completions");
        std::env::remove_var(API_KEY_ENV);
        std::env::remove_var(BASE_URL_ENV);
        std::env::remove_var(REASONING_ENV);
    }
}
