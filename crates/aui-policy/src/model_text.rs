//! Model-backed [`TextResolver`] (feature `model-text`).
//!
//! The model only proposes the **payload** for a TYPE_TEXT / SELECT that Instinct
//! already chose from the finite [`aui_core::ActionSpace`]. It never picks
//! a target, never sees selectors, and never issues input: the agent still
//! gates, tickets, revalidates, and consumes exactly as with
//! [`DeterministicTextResolver`].
//!
//! Every reply is vetted before it can become a payload:
//!
//! 1. **Context binding.** The request carries [`TextContext::fingerprint`];
//!    the reply must echo it. A reply bound to any other context (a stale or
//!    reordered answer) is refused.
//! 2. **Shape.** Non-empty after trimming, at most `max_chars`, no control
//!    characters (no newlines / tabs / escapes smuggled into a field).
//! 3. **Grounding** ([`Grounding::Goal`], the default). The value must occur in
//!    the active goal clause (case-insensitive). The model may *extract* a
//!    value the deterministic patterns miss; it may not invent one. A bare echo
//!    of the field label is refused as well.
//!
//! When the model errors or its reply is refused, the resolver falls back to
//! [`DeterministicTextResolver`] (default) or abstains
//! ([`ModelTextResolver::without_fallback`]). An abstain surfaces as
//! [`TextError::Abstain`], which the agent reports as an abstained outcome with
//! nothing typed. CI uses scripted models only; a live LLM is optional and is
//! plugged in through [`TextModel`] (e.g. [`CommandTextModel`]).

use std::fmt;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::text::{
    DeterministicTextResolver, TextContext, TextError, TextResolution, TextResolver,
};

/// Default upper bound on a model-proposed value, in characters.
pub const DEFAULT_MAX_CHARS: usize = 256;

/// What the model is asked. Only the active goal clause and the already-chosen
/// target's label / role: no page dump, no other regions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextModelRequest {
    /// Active goal clause (one `then` clause, not the whole goal).
    pub goal: String,
    pub field_label: String,
    /// `"select"` for SELECT, otherwise the region role (e.g. `text_field`).
    pub field_role: String,
    /// [`TextContext::fingerprint`]. The reply must echo it unchanged.
    pub context_fingerprint: u64,
    pub max_chars: usize,
}

/// What the model answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextModelReply {
    pub text: String,
    /// Must equal [`TextModelRequest::context_fingerprint`].
    pub context_fingerprint: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextModelError {
    /// Transport / process / timeout failure. Never contains credentials.
    Unavailable(String),
    /// The model declined (it found no value for this field).
    Declined(String),
    /// The reply could not be parsed.
    Malformed(String),
}

impl fmt::Display for TextModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "model unavailable: {m}"),
            Self::Declined(m) => write!(f, "model declined: {m}"),
            Self::Malformed(m) => write!(f, "malformed model reply: {m}"),
        }
    }
}

impl std::error::Error for TextModelError {}

/// Injectable model client. Tests use scripted implementations; a live LLM
/// client is a consumer concern (see [`CommandTextModel`]).
pub trait TextModel {
    fn complete(&mut self, request: &TextModelRequest) -> Result<TextModelReply, TextModelError>;
}

impl<M: TextModel + ?Sized> TextModel for Box<M> {
    fn complete(&mut self, request: &TextModelRequest) -> Result<TextModelReply, TextModelError> {
        (**self).complete(request)
    }
}

/// How strictly a model value must be anchored in the goal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grounding {
    /// Value must occur (case-insensitive) in the active goal clause.
    #[default]
    Goal,
    /// Shape checks only. For generative fields; opt-in, never the default.
    ShapeOnly,
}

/// Why a model reply was not used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelRefusal {
    /// The model call itself failed or declined.
    Model(TextModelError),
    /// Reply echoed a different context fingerprint (stale / misrouted).
    StaleContext {
        expected: u64,
        got: u64,
    },
    Empty,
    TooLong {
        chars: usize,
        max: usize,
    },
    ControlCharacters,
    /// Value does not occur in the active goal clause.
    Ungrounded,
    /// Value is just the field label (the field reference, not a value).
    LabelEcho,
}

impl fmt::Display for ModelRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(e) => write!(f, "{e}"),
            Self::StaleContext { .. } => f.write_str("model reply bound to a stale context"),
            Self::Empty => f.write_str("model returned an empty value"),
            Self::TooLong { chars, max } => {
                write!(f, "model value too long ({chars} > {max} chars)")
            }
            Self::ControlCharacters => f.write_str("model value contains control characters"),
            Self::Ungrounded => f.write_str("model value is not grounded in the goal"),
            Self::LabelEcho => f.write_str("model value only echoes the field label"),
        }
    }
}

/// Which path produced the last resolution (for logs / tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextSource {
    Model,
    /// Model refused for the given reason; the fallback resolver answered.
    Fallback(ModelRefusal),
    /// Model refused and the fallback (if any) failed: abstained.
    Abstained(ModelRefusal),
}

/// [`TextResolver`] that asks a [`TextModel`] first and vets the reply.
pub struct ModelTextResolver<M, F = DeterministicTextResolver> {
    model: M,
    fallback: Option<F>,
    grounding: Grounding,
    max_chars: usize,
    model_calls: u32,
    last: Option<TextSource>,
}

impl<M: TextModel> ModelTextResolver<M, DeterministicTextResolver> {
    /// Model first, [`DeterministicTextResolver`] on any refusal, then abstain.
    pub fn new(model: M) -> Self {
        Self {
            model,
            fallback: Some(DeterministicTextResolver),
            grounding: Grounding::Goal,
            max_chars: DEFAULT_MAX_CHARS,
            model_calls: 0,
            last: None,
        }
    }
}

impl<M: TextModel, F: TextResolver> ModelTextResolver<M, F> {
    /// Replace the fallback resolver.
    pub fn with_fallback<F2: TextResolver>(self, fallback: F2) -> ModelTextResolver<M, F2> {
        ModelTextResolver {
            model: self.model,
            fallback: Some(fallback),
            grounding: self.grounding,
            max_chars: self.max_chars,
            model_calls: self.model_calls,
            last: self.last,
        }
    }

    /// No fallback: any model refusal abstains.
    pub fn without_fallback(mut self) -> Self {
        self.fallback = None;
        self
    }

    pub fn grounding(mut self, grounding: Grounding) -> Self {
        self.grounding = grounding;
        self
    }

    pub fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = n.max(1);
        self
    }

    pub fn model_calls(&self) -> u32 {
        self.model_calls
    }

    pub fn last_source(&self) -> Option<&TextSource> {
        self.last.as_ref()
    }

    pub fn model(&self) -> &M {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut M {
        &mut self.model
    }

    fn ask(&mut self, context: &TextContext) -> Result<String, ModelRefusal> {
        let request = TextModelRequest {
            goal: context.goal.as_str().to_owned(),
            field_label: context.field_label.clone(),
            field_role: context.field_role.clone(),
            context_fingerprint: context.fingerprint(),
            max_chars: self.max_chars,
        };
        self.model_calls += 1;
        let reply = self.model.complete(&request).map_err(ModelRefusal::Model)?;
        vet(&request, &reply, self.grounding)
    }
}

impl<M: TextModel, F: TextResolver> TextResolver for ModelTextResolver<M, F> {
    fn resolve(&mut self, context: &TextContext) -> Result<TextResolution, TextError> {
        let refusal = match self.ask(context) {
            Ok(text) => {
                self.last = Some(TextSource::Model);
                return Ok(TextResolution {
                    text,
                    context_fingerprint: context.fingerprint(),
                });
            }
            Err(refusal) => refusal,
        };
        let fallback_err = match self.fallback.as_mut().map(|f| f.resolve(context)) {
            Some(Ok(resolution)) => {
                self.last = Some(TextSource::Fallback(refusal));
                return Ok(resolution);
            }
            Some(Err(e)) => Some(e),
            None => None,
        };
        let reason = match fallback_err {
            Some(e) => format!("{refusal}; fallback: {e}"),
            None => refusal.to_string(),
        };
        self.last = Some(TextSource::Abstained(refusal));
        Err(TextError::Abstain(reason))
    }
}

/// Apply context-binding, shape, and grounding rules to one reply.
pub fn vet(
    request: &TextModelRequest,
    reply: &TextModelReply,
    grounding: Grounding,
) -> Result<String, ModelRefusal> {
    if reply.context_fingerprint != request.context_fingerprint {
        return Err(ModelRefusal::StaleContext {
            expected: request.context_fingerprint,
            got: reply.context_fingerprint,
        });
    }
    let text = strip_matching_quotes(reply.text.trim()).trim();
    if text.is_empty() {
        return Err(ModelRefusal::Empty);
    }
    if text.chars().any(char::is_control) {
        return Err(ModelRefusal::ControlCharacters);
    }
    let chars = text.chars().count();
    if chars > request.max_chars {
        return Err(ModelRefusal::TooLong {
            chars,
            max: request.max_chars,
        });
    }
    if grounding == Grounding::Goal {
        let goal = request.goal.to_lowercase();
        let value = text.to_lowercase();
        if !goal.contains(&value) {
            return Err(ModelRefusal::Ungrounded);
        }
        let label = request.field_label.trim().to_lowercase();
        if !label.is_empty() && value == label && goal.matches(&label).count() <= 1 {
            return Err(ModelRefusal::LabelEcho);
        }
    }
    Ok(text.to_owned())
}

fn strip_matching_quotes(s: &str) -> &str {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Scripted model for tests and offline demos: returns queued replies in
/// order, then [`TextModelError::Unavailable`]. Records every request.
#[derive(Clone, Debug, Default)]
pub struct ScriptedTextModel {
    replies: std::collections::VecDeque<ScriptedReply>,
    requests: Vec<TextModelRequest>,
}

#[derive(Clone, Debug)]
enum ScriptedReply {
    /// Echo the request fingerprint with this text.
    Text(String),
    /// Reply with an explicit (possibly stale) fingerprint.
    Bound(String, u64),
    Error(TextModelError),
}

impl ScriptedTextModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a well-bound reply (echoes whatever fingerprint is asked).
    pub fn reply(mut self, text: impl Into<String>) -> Self {
        self.replies.push_back(ScriptedReply::Text(text.into()));
        self
    }

    /// Queue a reply bound to a fixed fingerprint (simulates a stale answer).
    pub fn reply_bound(mut self, text: impl Into<String>, fingerprint: u64) -> Self {
        self.replies
            .push_back(ScriptedReply::Bound(text.into(), fingerprint));
        self
    }

    pub fn fail(mut self, error: TextModelError) -> Self {
        self.replies.push_back(ScriptedReply::Error(error));
        self
    }

    pub fn requests(&self) -> &[TextModelRequest] {
        &self.requests
    }
}

impl TextModel for ScriptedTextModel {
    fn complete(&mut self, request: &TextModelRequest) -> Result<TextModelReply, TextModelError> {
        self.requests.push(request.clone());
        match self.replies.pop_front() {
            Some(ScriptedReply::Text(text)) => Ok(TextModelReply {
                text,
                context_fingerprint: request.context_fingerprint,
            }),
            Some(ScriptedReply::Bound(text, context_fingerprint)) => Ok(TextModelReply {
                text,
                context_fingerprint,
            }),
            Some(ScriptedReply::Error(e)) => Err(e),
            None => Err(TextModelError::Unavailable("script exhausted".into())),
        }
    }
}

/// Live-model adapter with no HTTP or credential handling of its own: runs a
/// user-supplied program, writes one JSON request line on stdin, and reads one
/// JSON reply from stdout.
///
/// Request: `{"goal":…,"field_label":…,"field_role":…,"context_fingerprint":N,"max_chars":N}`
///
/// Reply: `{"text":"…","context_fingerprint":N}` (echo the request value —
/// a JSON number or a decimal/hex string) or `{"declined":"reason"}`.
///
/// The program owns its API keys (environment / keychain); ultra-instinct never
/// reads, logs, or forwards them. Stderr is discarded so a chatty client
/// cannot leak secrets into ultra-instinct output.
#[derive(Clone, Debug)]
pub struct CommandTextModel {
    program: String,
    args: Vec<String>,
}

impl CommandTextModel {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }
}

impl TextModel for CommandTextModel {
    fn complete(&mut self, request: &TextModelRequest) -> Result<TextModelReply, TextModelError> {
        let line = serde_json::json!({
            "goal": request.goal,
            "field_label": request.field_label,
            "field_role": request.field_role,
            "context_fingerprint": request.context_fingerprint,
            "max_chars": request.max_chars,
        })
        .to_string();
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| TextModelError::Unavailable(format!("spawn failed: {}", e.kind())))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(line.as_bytes())
                .and_then(|()| stdin.write_all(b"\n"))
                .map_err(|e| TextModelError::Unavailable(format!("write failed: {}", e.kind())))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| TextModelError::Unavailable(format!("wait failed: {}", e.kind())))?;
        if !output.status.success() {
            return Err(TextModelError::Unavailable(format!(
                "model command exited with {}",
                output.status
            )));
        }
        parse_command_reply(&output.stdout)
    }
}

/// Parse a [`CommandTextModel`] reply. Public for adapter authors' tests.
pub fn parse_command_reply(stdout: &[u8]) -> Result<TextModelReply, TextModelError> {
    let value: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|_| TextModelError::Malformed("reply is not JSON".into()))?;
    if let Some(reason) = value.get("declined").and_then(|v| v.as_str()) {
        return Err(TextModelError::Declined(reason.to_owned()));
    }
    let text = value
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TextModelError::Malformed("missing string `text`".into()))?;
    let context_fingerprint = value
        .get("context_fingerprint")
        .and_then(reply_fingerprint)
        .ok_or_else(|| {
            TextModelError::Malformed(
                "missing `context_fingerprint` (u64 number or decimal/hex string)".into(),
            )
        })?;
    Ok(TextModelReply {
        text: text.to_owned(),
        context_fingerprint,
    })
}

/// A fingerprint echoed on the wire may be a JSON number or a decimal/hex
/// string. Language models routinely corrupt a 19-digit integer, so adapters
/// send the fingerprint as a string and accept it back in either form.
fn reply_fingerprint(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value.as_str().and_then(|s| {
            s.parse::<u64>().ok().or_else(|| {
                s.strip_prefix("0x")
                    .and_then(|h| u64::from_str_radix(h, 16).ok())
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::AgentGoal;

    fn ctx(goal: &str, label: &str, role: &str) -> TextContext {
        TextContext {
            goal: AgentGoal::new(goal),
            field_label: label.into(),
            field_role: role.into(),
            typed: Vec::new(),
            context_fingerprint: 7,
        }
    }

    #[test]
    fn model_extracts_value_deterministic_patterns_miss() {
        let c = ctx(
            "search for rust ownership in Search",
            "Search",
            "text_field",
        );
        assert_eq!(
            DeterministicTextResolver.resolve(&c),
            Err(TextError::Missing)
        );
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().reply("rust ownership"));
        let res = r.resolve(&c).unwrap();
        assert_eq!(res.text, "rust ownership");
        assert_eq!(res.context_fingerprint, c.fingerprint());
        assert_eq!(r.last_source(), Some(&TextSource::Model));
        let req = &r.model_mut().requests()[0];
        assert_eq!(req.context_fingerprint, c.fingerprint());
        assert_eq!(req.field_label, "Search");
    }

    #[test]
    fn select_value_is_grounded_and_returned_trimmed() {
        let c = ctx(
            "choose business class in Cabin class",
            "Cabin class",
            "select",
        );
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().reply("  \"Business\" "))
            .without_fallback();
        assert_eq!(r.resolve(&c).unwrap().text, "Business");
    }

    #[test]
    fn invented_value_is_refused_and_abstains_without_fallback() {
        let c = ctx(
            "search for rust ownership in Search",
            "Search",
            "text_field",
        );
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().reply("python generics"))
            .without_fallback();
        let err = r.resolve(&c).unwrap_err();
        assert!(
            matches!(err, TextError::Abstain(ref m) if m.contains("not grounded")),
            "{err}"
        );
        assert_eq!(
            r.last_source(),
            Some(&TextSource::Abstained(ModelRefusal::Ungrounded))
        );
    }

    #[test]
    fn junk_shapes_are_refused() {
        let req = TextModelRequest {
            goal: "type ab into Search".into(),
            field_label: "Search".into(),
            field_role: "text_field".into(),
            context_fingerprint: 1,
            max_chars: 4,
        };
        let reply = |t: &str| TextModelReply {
            text: t.into(),
            context_fingerprint: 1,
        };
        assert_eq!(
            vet(&req, &reply("  "), Grounding::Goal),
            Err(ModelRefusal::Empty)
        );
        assert_eq!(
            vet(&req, &reply("\"\""), Grounding::Goal),
            Err(ModelRefusal::Empty)
        );
        assert_eq!(
            vet(&req, &reply("a\nb"), Grounding::ShapeOnly),
            Err(ModelRefusal::ControlCharacters)
        );
        assert_eq!(
            vet(&req, &reply("abcdef"), Grounding::ShapeOnly),
            Err(ModelRefusal::TooLong { chars: 6, max: 4 })
        );
        assert_eq!(
            vet(&req, &reply("Search"), Grounding::ShapeOnly),
            Err(ModelRefusal::TooLong { chars: 6, max: 4 })
        );
        let req = TextModelRequest {
            max_chars: 64,
            ..req
        };
        assert_eq!(
            vet(&req, &reply("Search"), Grounding::Goal),
            Err(ModelRefusal::LabelEcho)
        );
        assert_eq!(vet(&req, &reply("ab"), Grounding::Goal), Ok("ab".into()));
        assert_eq!(
            vet(&req, &reply("zz"), Grounding::ShapeOnly),
            Ok("zz".into())
        );
    }

    #[test]
    fn stale_fingerprint_is_refused() {
        let c = ctx("search for rust in Search", "Search", "text_field");
        let stale = c.fingerprint() ^ 1;
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().reply_bound("rust", stale))
            .without_fallback();
        let err = r.resolve(&c).unwrap_err();
        assert!(
            matches!(err, TextError::Abstain(ref m) if m.contains("stale")),
            "{err}"
        );
    }

    #[test]
    fn model_failure_falls_back_to_deterministic() {
        let c = ctx(r#"Type "rust" into Search"#, "Search", "text_field");
        let err = TextModelError::Unavailable("offline".into());
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().fail(err.clone()));
        assert_eq!(r.resolve(&c).unwrap().text, "rust");
        assert_eq!(
            r.last_source(),
            Some(&TextSource::Fallback(ModelRefusal::Model(err)))
        );
    }

    #[test]
    fn model_and_fallback_both_fail_abstains() {
        let c = ctx("type into Search", "Search", "text_field");
        let mut r = ModelTextResolver::new(ScriptedTextModel::new().reply("Search"));
        let err = r.resolve(&c).unwrap_err();
        assert!(
            matches!(err, TextError::Abstain(ref m) if m.contains("label") && m.contains("fallback")),
            "{err}"
        );
    }

    #[test]
    fn command_reply_parsing() {
        assert_eq!(
            parse_command_reply(br#"{"text":"rust","context_fingerprint":18446744073709551615}"#),
            Ok(TextModelReply {
                text: "rust".into(),
                context_fingerprint: u64::MAX
            })
        );
        // LLM adapters echo the fingerprint as a string — decimal or hex.
        assert_eq!(
            parse_command_reply(br#"{"text":"rust","context_fingerprint":"18446744073709551615"}"#),
            Ok(TextModelReply {
                text: "rust".into(),
                context_fingerprint: u64::MAX
            })
        );
        assert_eq!(
            parse_command_reply(br#"{"text":"rust","context_fingerprint":"0x10"}"#),
            Ok(TextModelReply {
                text: "rust".into(),
                context_fingerprint: 16
            })
        );
        assert!(matches!(
            parse_command_reply(br#"{"text":"rust","context_fingerprint":"not-a-number"}"#),
            Err(TextModelError::Malformed(_))
        ));
        assert!(matches!(
            parse_command_reply(br#"{"declined":"no value"}"#),
            Err(TextModelError::Declined(_))
        ));
        assert!(matches!(
            parse_command_reply(br#"{"text":"rust"}"#),
            Err(TextModelError::Malformed(_))
        ));
        assert!(matches!(
            parse_command_reply(b"sure! here you go"),
            Err(TextModelError::Malformed(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn command_model_round_trip_echoes_fingerprint() {
        // Reads the request line and echoes its fingerprint; no network.
        let script = r#"read -r line; fp=$(printf '%s' "$line" | sed 's/.*"context_fingerprint":\([0-9]*\).*/\1/'); printf '{"text":"rust","context_fingerprint":%s}\n' "$fp""#;
        let model = CommandTextModel::new("sh").arg("-c").arg(script);
        let c = ctx("search for rust in Search", "Search", "text_field");
        let mut r = ModelTextResolver::new(model).without_fallback();
        assert_eq!(r.resolve(&c).unwrap().text, "rust");
    }

    #[cfg(unix)]
    #[test]
    fn command_model_failure_is_unavailable() {
        let model = CommandTextModel::new("sh").arg("-c").arg("exit 3");
        let c = ctx("search for rust in Search", "Search", "text_field");
        let mut r = ModelTextResolver::new(model).without_fallback();
        let err = r.resolve(&c).unwrap_err();
        assert!(
            matches!(err, TextError::Abstain(ref m) if m.contains("unavailable")),
            "{err}"
        );
    }
}
