//! Semantic act transport for a Browser Use executor.
//!
//! hyper-use has already located a region. This crate hands that region to a
//! transport as an id, a role, a label, and an action. It does not take a goal,
//! a URL, or a coordinate, and it does not decide to navigate.
//!
//! [`ReplayTransport`] is the only transport. There is no live Browser Use
//! process. A scripted rejection is a [`BrowserUseError`], not a panic.

#![forbid(unsafe_code)]

use std::fmt;

use hyper_use_core::{Action, RegionId, Role};
use serde_json::{Map, Value};

/// Stable status string. This is not a live Browser Use agent.
pub const STATUS: &str =
    "browser-use semantic executor ready; sends region id, role, and label. It does not navigate";

const KIND: &str = "browser-use-replay";

const REQUEST_KEYS: [&str; 4] = ["region_id", "role", "label", "action"];

const FORBIDDEN_KEYS: [&str; 12] = [
    "goal",
    "steps",
    "navigate",
    "task",
    "url",
    "x",
    "y",
    "coordinates",
    "screenshot",
    "tokens",
    "latency",
    "retries",
];

/// One semantic act. The fields are the whole request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticRequest {
    region_id: RegionId,
    role: Role,
    label: String,
    action: Action,
}

impl SemanticRequest {
    pub fn new(
        region_id: RegionId,
        role: Role,
        label: impl Into<String>,
        action: Action,
    ) -> Result<Self, BrowserUseError> {
        let label = label.into();
        if label.is_empty() {
            return Err(BrowserUseError::BadScript {
                message: "label must not be empty".into(),
            });
        }
        Ok(Self {
            region_id,
            role,
            label,
            action,
        })
    }

    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }

    pub const fn role(&self) -> Role {
        self.role
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub const fn action(&self) -> Action {
        self.action
    }

    /// JSON object with exactly `region_id`, `role`, `label`, and `action`.
    pub fn to_wire(&self) -> String {
        serde_json::json!({
            "region_id": self.region_id.as_str(),
            "role": self.role.as_str(),
            "label": self.label,
            "action": self.action.as_str(),
        })
        .to_string()
    }
}

/// What a transport returns after it accepts a request. No coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportReceipt {
    region_id: RegionId,
    action: Action,
}

impl TransportReceipt {
    pub fn new(region_id: RegionId, action: Action) -> Self {
        Self { region_id, action }
    }

    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }

    pub const fn action(&self) -> Action {
        self.action
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrowserUseError {
    BadScript { message: String },
    ParamsMismatch { message: String },
    Rejected { message: String },
    UnknownRegion(String),
    UnsupportedAction(String),
}

impl fmt::Display for BrowserUseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadScript { message } => write!(f, "invalid browser-use script: {message}"),
            Self::ParamsMismatch { message } => {
                write!(
                    f,
                    "browser-use request does not match the script: {message}"
                )
            }
            Self::Rejected { message } => {
                write!(f, "browser-use rejected the semantic act: {message}")
            }
            Self::UnknownRegion(id) => write!(f, "unknown region `{id}`"),
            Self::UnsupportedAction(action) => {
                write!(f, "browser-use executor cannot perform `{action}`")
            }
        }
    }
}

impl std::error::Error for BrowserUseError {}

pub trait BrowserUseTransport {
    fn submit(&mut self, request: &SemanticRequest) -> Result<TransportReceipt, BrowserUseError>;
}

#[derive(Clone, Debug)]
struct Scripted {
    expected: SemanticRequest,
    rejected: Option<String>,
}

/// Fixture peer. Tests and `--executor browser-use` use this. It does not
/// start a process.
#[derive(Clone, Debug)]
pub struct ReplayTransport {
    scripted: Scripted,
    submitted: Vec<SemanticRequest>,
}

impl ReplayTransport {
    pub fn parse(text: &str) -> Result<Self, BrowserUseError> {
        let root: Value = serde_json::from_str(text).map_err(|err| BrowserUseError::BadScript {
            message: err.to_string(),
        })?;
        let object = root.as_object().ok_or_else(|| BrowserUseError::BadScript {
            message: "script must be a JSON object".into(),
        })?;
        reject_forbidden(object)?;
        require_only(object, &["kind", "request", "result"])?;
        let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| {
            BrowserUseError::BadScript {
                message: "script kind must be browser-use-replay".into(),
            }
        })?;
        if kind != KIND {
            return Err(BrowserUseError::BadScript {
                message: "script kind must be browser-use-replay".into(),
            });
        }
        let request = object
            .get("request")
            .and_then(Value::as_object)
            .ok_or_else(|| BrowserUseError::BadScript {
                message: "request must be an object".into(),
            })?;
        reject_forbidden(request)?;
        require_only(request, &REQUEST_KEYS)?;
        let expected = parse_request(request)?;
        let result = object
            .get("result")
            .and_then(Value::as_object)
            .ok_or_else(|| BrowserUseError::BadScript {
                message: "result must be an object".into(),
            })?;
        reject_forbidden(result)?;
        let accepted = result
            .get("accepted")
            .and_then(Value::as_bool)
            .ok_or_else(|| BrowserUseError::BadScript {
                message: "result.accepted must be a bool".into(),
            })?;
        let rejected = if accepted {
            require_only(result, &["accepted"])?;
            None
        } else {
            require_only(result, &["accepted", "message"])?;
            let message = result
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| !message.is_empty())
                .ok_or_else(|| BrowserUseError::BadScript {
                    message: "a rejected result needs a message".into(),
                })?;
            Some(message.to_owned())
        };
        Ok(Self {
            scripted: Scripted { expected, rejected },
            submitted: Vec::new(),
        })
    }

    pub fn expected(&self) -> &SemanticRequest {
        &self.scripted.expected
    }

    pub fn submitted(&self) -> &[SemanticRequest] {
        &self.submitted
    }
}

impl BrowserUseTransport for ReplayTransport {
    fn submit(&mut self, request: &SemanticRequest) -> Result<TransportReceipt, BrowserUseError> {
        self.submitted.push(request.clone());
        if request != &self.scripted.expected {
            return Err(BrowserUseError::ParamsMismatch {
                message: format!(
                    "expected {} `{}` and got {} `{}`",
                    self.scripted.expected.region_id(),
                    self.scripted.expected.label(),
                    request.region_id(),
                    request.label()
                ),
            });
        }
        if let Some(message) = &self.scripted.rejected {
            return Err(BrowserUseError::Rejected {
                message: message.clone(),
            });
        }
        Ok(TransportReceipt::new(
            request.region_id().clone(),
            request.action(),
        ))
    }
}

fn parse_request(object: &Map<String, Value>) -> Result<SemanticRequest, BrowserUseError> {
    let region_id = object
        .get("region_id")
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserUseError::BadScript {
            message: "request.region_id must be a string".into(),
        })?;
    let region_id = RegionId::try_new(region_id).map_err(|_| BrowserUseError::BadScript {
        message: "request.region_id is empty or has whitespace".into(),
    })?;
    let role =
        object
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserUseError::BadScript {
                message: "request.role must be a string".into(),
            })?;
    let role = Role::parse(role).ok_or_else(|| BrowserUseError::BadScript {
        message: format!("unknown role `{role}`"),
    })?;
    let label =
        object
            .get("label")
            .and_then(Value::as_str)
            .ok_or_else(|| BrowserUseError::BadScript {
                message: "request.label must be a string".into(),
            })?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| BrowserUseError::BadScript {
            message: "request.action must be a string".into(),
        })?;
    let action = Action::parse(action).ok_or_else(|| BrowserUseError::BadScript {
        message: format!("unknown action `{action}`"),
    })?;
    SemanticRequest::new(region_id, role, label, action)
}

fn require_only(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), BrowserUseError> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(BrowserUseError::BadScript {
                message: format!("unexpected field `{key}`"),
            });
        }
    }
    for key in allowed {
        if !object.contains_key(*key) {
            return Err(BrowserUseError::BadScript {
                message: format!("missing field `{key}`"),
            });
        }
    }
    Ok(())
}

fn reject_forbidden(object: &Map<String, Value>) -> Result<(), BrowserUseError> {
    for key in object.keys() {
        if FORBIDDEN_KEYS.contains(&key.as_str()) {
            return Err(BrowserUseError::BadScript {
                message: format!("browser-use replay does not accept `{key}`"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn accept_script() -> ReplayTransport {
        ReplayTransport::parse(include_str!("../../../fixtures/sign-in.browser-use.json")).unwrap()
    }

    #[test]
    fn replay_accepts_the_semantic_request_and_records_it() {
        let mut transport = accept_script();
        let request = transport.expected().clone();
        assert!(transport.submitted().is_empty());
        let receipt = transport.submit(&request).unwrap();
        assert_eq!(receipt.region_id().as_str(), "n100");
        assert_eq!(receipt.action(), Action::Click);
        assert_eq!(transport.submitted(), std::slice::from_ref(&request));
        let wire: Value = serde_json::from_str(&request.to_wire()).unwrap();
        let mut keys: Vec<_> = wire.as_object().unwrap().keys().cloned().collect();
        keys.sort_unstable();
        assert_eq!(keys, ["action", "label", "region_id", "role"]);
        assert_eq!(wire["label"], "Sign in");
        assert_eq!(wire["role"], "button");
        assert!(wire.get("x").is_none());
        assert!(wire.get("goal").is_none());
        assert!(STATUS.contains("does not navigate"));
    }

    #[test]
    fn rejection_is_exact_and_still_records_the_request() {
        let mut transport = ReplayTransport::parse(include_str!(
            "../../../fixtures/sign-in-reject.browser-use.json"
        ))
        .unwrap();
        let request = transport.expected().clone();
        let err = transport.submit(&request).unwrap_err();
        assert_eq!(
            err,
            BrowserUseError::Rejected {
                message: "control refused the semantic act".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "browser-use rejected the semantic act: control refused the semantic act"
        );
        assert_eq!(transport.submitted().len(), 1);
    }

    #[test]
    fn a_different_label_is_a_params_mismatch_after_the_call() {
        let mut transport = accept_script();
        let other = SemanticRequest::new(
            RegionId::try_new("n100").unwrap(),
            Role::Button,
            "Cancel",
            Action::Click,
        )
        .unwrap();
        let err = transport.submit(&other).unwrap_err();
        assert_eq!(
            err,
            BrowserUseError::ParamsMismatch {
                message: "expected n100 `Sign in` and got n100 `Cancel`".into(),
            }
        );
        assert_eq!(transport.submitted(), &[other]);
    }

    #[test]
    fn a_goal_field_is_a_script_error() {
        let err = ReplayTransport::parse(
            r#"{"kind":"browser-use-replay","goal":"sign in","request":{},"result":{"accepted":true}}"#,
        )
        .unwrap_err();
        assert_eq!(
            err,
            BrowserUseError::BadScript {
                message: "browser-use replay does not accept `goal`".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid browser-use script: browser-use replay does not accept `goal`"
        );
    }

    #[test]
    fn empty_label_and_unknown_role_are_script_errors() {
        let empty = SemanticRequest::new(
            RegionId::try_new("n100").unwrap(),
            Role::Button,
            "",
            Action::Click,
        )
        .unwrap_err();
        assert_eq!(
            empty,
            BrowserUseError::BadScript {
                message: "label must not be empty".into(),
            }
        );
        let err = ReplayTransport::parse(
            r#"{"kind":"browser-use-replay","request":{"region_id":"n100","role":"robot","label":"Sign in","action":"click"},"result":{"accepted":true}}"#,
        )
        .unwrap_err();
        assert_eq!(
            err,
            BrowserUseError::BadScript {
                message: "unknown role `robot`".into(),
            }
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]
        #[test]
        fn wire_keys_stay_semantic(label in "[A-Za-z][A-Za-z0-9 ]{0,24}") {
            let request = SemanticRequest::new(
                RegionId::try_new("n100").unwrap(),
                Role::Button,
                label,
                Action::Click,
            )
            .unwrap();
            let wire: Value = serde_json::from_str(&request.to_wire()).unwrap();
            let object = wire.as_object().unwrap();
            let mut keys: Vec<_> = object.keys().cloned().collect();
            keys.sort_unstable();
            assert_eq!(keys, ["action", "label", "region_id", "role"]);
            for forbidden in FORBIDDEN_KEYS {
                assert!(!object.contains_key(forbidden));
            }
        }
    }
}
