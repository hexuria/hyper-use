//! Execution policy for hyper-use.
//!
//! Policy order is fixed: browser, then macOS accessibility, then computer-use
//! pixels. The first available backend wins. Availability is an input; this
//! crate does not probe the operating system.
//!
//! [`BrowserExecutor`] performs a browser press when it holds a CDP session.
//! macOS and CUA still return [`ExecutorError::NotImplemented`]. A scored
//! confidence below [`MIN_ACT_CONFIDENCE_MILLIS`] returns
//! [`ExecutorError::ConfidenceBelowThreshold`] and does not click.
//! [`ActConfidence::Inspected`] is the operator naming a region; the gate
//! does not apply. There is no CUA call anywhere in this crate.

#![forbid(unsafe_code)]

use std::fmt;

use hyper_use_browser::{ActMechanism, BrowserError, BrowserSession, CdpTransport};
use hyper_use_core::{Action, RegionId};
use hyper_use_protocol::{ComputerResult, FallbackReason, MatcherConfidence};

/// Preference order when more than one backend can perform an action.
pub const DEFAULT_POLICY_ORDER: [ExecutorKind; 3] = [
    ExecutorKind::Browser,
    ExecutorKind::Macos,
    ExecutorKind::Cua,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExecutorKind {
    Browser,
    Macos,
    Cua,
}

impl ExecutorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Macos => "macos",
            Self::Cua => "cua",
        }
    }

    pub const fn status(self) -> &'static str {
        match self {
            Self::Browser => hyper_use_browser::STATUS,
            Self::Macos => hyper_use_macos::STATUS,
            Self::Cua => hyper_use_cua::STATUS,
        }
    }
}

impl fmt::Display for ExecutorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Act gate for a scored matcher total. 550 means 0.55. A weighted text miss
/// tops out at 0.50 (geometry and actionability, semantic 0), so it is refused.
/// This is not a probability and is not calibrated across matchers.
pub const MIN_ACT_CONFIDENCE_MILLIS: i32 = 550;

/// Where the confidence came from. These variants cannot be combined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ActConfidence {
    /// The operator named the region. The confidence gate does not apply.
    Inspected,
    /// A matcher total. Compared with [`MIN_ACT_CONFIDENCE_MILLIS`].
    Scored(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionRequest {
    region_id: RegionId,
    action: Action,
    confidence: ActConfidence,
}

impl ActionRequest {
    /// Inspected target. Does not apply the confidence gate.
    pub fn new(region_id: RegionId, action: Action) -> Self {
        Self {
            region_id,
            action,
            confidence: ActConfidence::Inspected,
        }
    }

    pub fn scored(mut self, confidence: f64) -> Self {
        self.confidence = ActConfidence::Scored(confidence);
        self
    }

    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub const fn confidence(&self) -> ActConfidence {
        self.confidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionReceipt {
    kind: ExecutorKind,
    region_id: RegionId,
    action: Action,
    mechanism: ActMechanism,
}

impl ActionReceipt {
    pub fn kind(&self) -> ExecutorKind {
        self.kind
    }
    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }
    pub fn action(&self) -> Action {
        self.action
    }
    pub const fn mechanism(&self) -> ActMechanism {
        self.mechanism
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutorError {
    /// The backend has no session. macOS and CUA are always this.
    /// A browser stub is this; [`BrowserExecutor`] is not.
    NotImplemented(ExecutorKind),
    /// No entry in the policy order was present in the available set.
    NoneAvailable,
    /// Matcher total is below [`MIN_ACT_CONFIDENCE_MILLIS`]. No click was sent.
    ConfidenceBelowThreshold {
        confidence_millis: i32,
        minimum_millis: i32,
    },
    NonFiniteConfidence,
    Browser(BrowserError),
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented(kind) => {
                write!(f, "{kind} executor is not implemented")
            }
            Self::NoneAvailable => f.write_str("no executor is available"),
            Self::ConfidenceBelowThreshold {
                confidence_millis,
                minimum_millis,
            } => write!(
                f,
                "confidence {confidence_millis} is below the act minimum {minimum_millis}"
            ),
            Self::NonFiniteConfidence => f.write_str("confidence must be finite"),
            Self::Browser(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

pub trait ActionExecutor {
    fn kind(&self) -> ExecutorKind;

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError>;
}

/// Backend that always refuses. Used until a real browser, AX, or CUA driver exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StubExecutor {
    kind: ExecutorKind,
}

impl StubExecutor {
    pub const fn new(kind: ExecutorKind) -> Self {
        Self { kind }
    }
}

impl ActionExecutor for StubExecutor {
    fn kind(&self) -> ExecutorKind {
        self.kind
    }

    fn execute(&mut self, _request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        Err(ExecutorError::NotImplemented(self.kind))
    }
}

/// Browser backend. macOS and CUA are not constructed here.
pub struct BrowserExecutor<T: CdpTransport> {
    session: BrowserSession<T>,
}

impl<T: CdpTransport> BrowserExecutor<T> {
    pub fn new(session: BrowserSession<T>) -> Self {
        Self { session }
    }

    pub fn session(&self) -> &BrowserSession<T> {
        &self.session
    }
}

impl<T: CdpTransport> ActionExecutor for BrowserExecutor<T> {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Browser
    }

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        if let ActConfidence::Scored(confidence) = request.confidence() {
            gate_scored_confidence(confidence)?;
        }
        if self.session.manifold().is_none() {
            self.session.observe().map_err(ExecutorError::Browser)?;
        }
        let mechanism = self
            .session
            .press(request.region_id(), request.action())
            .map_err(ExecutorError::Browser)?;
        Ok(ActionReceipt {
            kind: ExecutorKind::Browser,
            region_id: request.region_id().clone(),
            action: request.action(),
            mechanism,
        })
    }
}

/// Refuse a scored act that cannot clear the threshold. Does not click.
pub fn gate_scored_confidence(confidence: f64) -> Result<(), ExecutorError> {
    if !confidence.is_finite() {
        return Err(ExecutorError::NonFiniteConfidence);
    }
    let confidence_millis = (confidence * 1000.0).round() as i32;
    if confidence_millis < MIN_ACT_CONFIDENCE_MILLIS {
        return Err(ExecutorError::ConfidenceBelowThreshold {
            confidence_millis,
            minimum_millis: MIN_ACT_CONFIDENCE_MILLIS,
        });
    }
    Ok(())
}

impl ExecutorError {
    /// Journal shape for a refusal. `executed` is false. This does not act.
    pub fn refusal_result(&self) -> Option<ComputerResult> {
        match self {
            Self::ConfidenceBelowThreshold {
                confidence_millis, ..
            } => {
                let confidence =
                    MatcherConfidence::try_new(f64::from(*confidence_millis) / 1000.0).ok()?;
                Some(ComputerResult::refused(
                    confidence,
                    FallbackReason::LowConfidence,
                ))
            }
            Self::NotImplemented(_) => Some(ComputerResult::refused(
                MatcherConfidence::try_new(0.0).expect("zero is finite"),
                FallbackReason::NotImplemented,
            )),
            Self::NonFiniteConfidence | Self::NoneAvailable | Self::Browser(_) => None,
        }
    }
}

/// First kind in [`DEFAULT_POLICY_ORDER`] that is also in `available`.
pub fn select_executor(available: &[ExecutorKind]) -> Result<ExecutorKind, ExecutorError> {
    for kind in DEFAULT_POLICY_ORDER {
        if available.contains(&kind) {
            return Ok(kind);
        }
    }
    Err(ExecutorError::NoneAvailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_prefers_browser_then_macos_then_cua() {
        assert_eq!(
            DEFAULT_POLICY_ORDER,
            [
                ExecutorKind::Browser,
                ExecutorKind::Macos,
                ExecutorKind::Cua
            ]
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Browser]).unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Macos]).unwrap(),
            ExecutorKind::Macos
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua]).unwrap(),
            ExecutorKind::Cua
        );
        let err = select_executor(&[]).unwrap_err();
        assert_eq!(err, ExecutorError::NoneAvailable);
        assert_eq!(err.to_string(), "no executor is available");
    }

    #[test]
    fn stubs_refuse_to_act() {
        let request = ActionRequest::new(RegionId::try_new("nav-settings").unwrap(), Action::Click);
        for kind in DEFAULT_POLICY_ORDER {
            let mut executor = StubExecutor::new(kind);
            let err = executor.execute(&request).unwrap_err();
            assert_eq!(err, ExecutorError::NotImplemented(kind));
            assert!(err.to_string().contains("not implemented"));
            let refused = err.refusal_result().unwrap();
            assert!(!refused.executed());
            assert_eq!(
                refused.fallback(),
                Some(hyper_use_protocol::FallbackReason::NotImplemented)
            );
        }
        assert!(ExecutorKind::Browser.status().contains("CDP"));
        assert!(ExecutorKind::Macos.status().contains("later phase"));
        assert!(ExecutorKind::Cua.status().contains("later phase"));
        assert_eq!(
            hyper_use_browser::BrowserStub.status(),
            hyper_use_browser::STATUS
        );
        assert_eq!(hyper_use_macos::MacosStub.status(), hyper_use_macos::STATUS);
        assert_eq!(hyper_use_cua::CuaStub.status(), hyper_use_cua::STATUS);
    }

    #[test]
    fn low_confidence_does_not_click_and_dom_press_does() {
        use hyper_use_browser::ReplayTransport;
        let err = gate_scored_confidence(0.49).unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        assert_eq!(
            err.to_string(),
            "confidence 490 is below the act minimum 550"
        );
        let refused = err.refusal_result().unwrap();
        assert!(!refused.executed());
        assert_eq!(
            refused.fallback(),
            Some(hyper_use_protocol::FallbackReason::LowConfidence)
        );
        assert_eq!(
            gate_scored_confidence(f64::NAN).unwrap_err(),
            ExecutorError::NonFiniteConfidence
        );
        assert!(gate_scored_confidence(0.55).is_ok());
        assert!(gate_scored_confidence(1.0).is_ok());

        let mut low = BrowserExecutor::new(BrowserSession::new(
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in-press.cdp.json"))
                .unwrap(),
        ));
        let request =
            ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.49);
        let err = low.execute(&request).unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        assert!(low.session().transport().logged_methods().is_empty());

        let mut high = BrowserExecutor::new(BrowserSession::new(
            ReplayTransport::parse(include_str!("../../../fixtures/sign-in-press.cdp.json"))
                .unwrap(),
        ));
        let receipt = high
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap();
        assert_eq!(
            receipt.mechanism(),
            hyper_use_browser::ActMechanism::DomSemantic
        );
        assert_eq!(receipt.kind(), ExecutorKind::Browser);
        assert!(high
            .session()
            .transport()
            .logged_methods()
            .iter()
            .any(|method| method == "Runtime.callFunctionOn"));
        assert!(high
            .session()
            .transport()
            .logged_methods()
            .iter()
            .all(|method| method != "Input.dispatchMouseEvent"));
    }
}
