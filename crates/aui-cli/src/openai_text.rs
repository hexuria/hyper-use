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
const CALL_TIMEOUT: Duration = Duration::from_secs(20);

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
        check_base_url(base.trim_end_matches('/'))?;
        let model = std::env::var(MODEL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let reasoning = if std::env::var(REASONING_ENV).ok().as_deref() == Some("none") {
            json!({"reasoning": {"enabled": false}})
        } else if is_deepseek_api(&base) {
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
            // String on the wire: language models cannot reproduce a 19-digit
            // JSON number faithfully; `parse_command_reply` accepts the
            // decimal/hex string back.
            "context_fingerprint": request.context_fingerprint.to_string(),
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
        // One retry on retryable failures — the call is inside the agent
        // loop, so a transient blip must not cost a turn.
        let mut result = self.send_once(&body);
        let retryable = match &result {
            Ok(r) => retryable_status(r.status()),
            Err(err) => err.is_timeout() || err.is_connect(),
        };
        if retryable {
            result = self.send_once(&body);
        }
        let response: Value = result
            .and_then(|r| r.error_for_status())
            .map_err(|err| TextModelError::Unavailable(format!("text model http: {err}")))?
            .json()
            .map_err(|err| TextModelError::Malformed(format!("response not JSON: {err}")))?;
        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| TextModelError::Malformed("no assistant content".into()))?;
        parse_command_reply(content.as_bytes())
    }
}

impl OpenAiTextModel {
    fn send_once(&self, body: &Value) -> Result<reqwest::blocking::Response, reqwest::Error> {
        self.http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(body)
            .send()
    }
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// The API key rides the Authorization header — refuse cleartext endpoints
/// (`http://` is allowed only for localhost test servers).
fn check_base_url(base: &str) -> Result<(), String> {
    if base.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = base.strip_prefix("http://") {
        let authority = rest.split('/').next().unwrap_or_default();
        let host = match authority.strip_prefix('[') {
            Some(_) => &authority[..authority.find(']').map_or(authority.len(), |i| i + 1)],
            None => authority.split(':').next().unwrap_or_default(),
        };
        if matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Ok(());
        }
    }
    Err(format!(
        "text model: {BASE_URL_ENV} {base:?} must be https:// (http:// allowed only for localhost)"
    ))
}

/// `api.deepseek.com` exactly — a mere substring match would let
/// `?x=api.deepseek.com` on another host select the deepseek request shape.
fn is_deepseek_api(base: &str) -> bool {
    base.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|authority| authority.split(':').next())
        == Some("api.deepseek.com")
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

        // Cleartext endpoints are refused (the key rides Authorization);
        // localhost http is allowed for test servers.
        std::env::set_var(BASE_URL_ENV, "http://api.evil.example/v1");
        assert!(OpenAiTextModel::from_env().is_err());
        std::env::set_var(BASE_URL_ENV, "ftp://api.deepseek.com");
        assert!(OpenAiTextModel::from_env().is_err());
        std::env::set_var(BASE_URL_ENV, "http://127.0.0.1:9000");
        assert!(OpenAiTextModel::from_env().unwrap().is_some());
        std::env::remove_var(API_KEY_ENV);
        std::env::remove_var(BASE_URL_ENV);
        std::env::remove_var(REASONING_ENV);
    }

    #[test]
    fn base_url_guard_hosts() {
        for ok in [
            "https://api.deepseek.com/v1",
            "http://localhost",
            "http://localhost:8000",
            "http://127.0.0.1:3000/v1",
            "http://[::1]:8080",
        ] {
            assert!(check_base_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://api.deepseek.com/v1",
            "http://127.0.0.1.evil.test",
            "localhost:8000",
        ] {
            assert!(check_base_url(bad).is_err(), "{bad}");
        }
        assert!(is_deepseek_api("https://api.deepseek.com/v1"));
        assert!(is_deepseek_api("https://api.deepseek.com"));
        assert!(!is_deepseek_api("https://deepseek.com"));
        assert!(!is_deepseek_api("https://evil.test/?x=api.deepseek.com/"));
    }
}
