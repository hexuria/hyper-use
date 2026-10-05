//! Execution policy for hyper-use.
//!
//! Policy order is fixed: browser, then macOS accessibility. The first
//! available backend in that order wins. Availability is an input; this crate
//! does not probe the operating system.
//!
//! [`BrowserExecutor`] performs a browser press when it holds a CDP session.
//! [`BrowserUseExecutor`] and [`CuaExecutor`] each hand an already located
//! region to a replay transport as a region id, role, label, and action.
//! Neither is in [`DEFAULT_POLICY_ORDER`]: a missing CDP session does not
//! delegate to either. macOS still returns [`ExecutorError::NotImplemented`].
//! A pixel CUA driver does not exist; [`hyper_use_cua::CuaStub`] says so.
//! A scored confidence below [`MIN_ACT_CONFIDENCE_MILLIS`] returns
//! [`ExecutorError::ConfidenceBelowThreshold`] and does not click, and it does
//! not call the Browser Use or CUA transport. A ranked confidence whose top
//! and runner-up are closer than [`MIN_ACT_MARGIN_MILLIS`] returns
//! [`ExecutorError::AmbiguousTarget`] and does not act either.
//! [`ActConfidence::Inspected`] is the operator naming a region; the gate does
//! not apply.

#![forbid(unsafe_code)]

use std::fmt;

use hyper_use_browser::{ActMechanism, BrowserError, BrowserSession, CdpTransport};
pub use hyper_use_browser_use::BrowserUseError;
use hyper_use_browser_use::{
    BrowserUseTransport, ReplayTransport, SemanticRequest, STATUS as BROWSER_USE_STATUS,
};
use hyper_use_core::{Action, RegionId};
pub use hyper_use_cua::CuaError;
use hyper_use_cua::{
    CuaTransport, ReplayTransport as CuaReplayTransport, SemanticRequest as CuaSemanticRequest,
    STATUS as CUA_STATUS,
};
use hyper_use_protocol::{ComputerResult, FallbackReason, MatcherConfidence};

/// Preference order when more than one backend can perform an action.
pub const DEFAULT_POLICY_ORDER: [ExecutorKind; 2] = [ExecutorKind::Browser, ExecutorKind::Macos];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExecutorKind {
    Browser,
    Macos,
    /// Opt-in semantic handoff. Not a member of [`DEFAULT_POLICY_ORDER`].
    Cua,
    /// Opt-in. Not a member of [`DEFAULT_POLICY_ORDER`].
    BrowserUse,
}

impl ExecutorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Macos => "macos",
            Self::Cua => "cua",
            Self::BrowserUse => "browser-use",
        }
    }

    pub const fn status(self) -> &'static str {
        match self {
            Self::Browser => hyper_use_browser::STATUS,
            Self::Macos => hyper_use_macos::STATUS,
            Self::Cua => CUA_STATUS,
            Self::BrowserUse => BROWSER_USE_STATUS,
        }
    }
}

impl fmt::Display for ExecutorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ExecutorKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "browser" => Some(Self::Browser),
            "macos" => Some(Self::Macos),
            "cua" => Some(Self::Cua),
            "browser-use" => Some(Self::BrowserUse),
            _ => None,
        }
    }
}

/// Act gate for a scored matcher total. 550 means 0.55. A weighted text miss
/// tops out at 0.50 (geometry and actionability, semantic 0), so it is refused.
/// This is not a probability and is not calibrated across matchers.
pub const MIN_ACT_CONFIDENCE_MILLIS: i32 = 550;

/// Minimum gap between the top candidate and the runner-up, in millis of the
/// same matcher's total. 50 means 0.05. Not calibrated across matchers.
pub const MIN_ACT_MARGIN_MILLIS: i32 = 50;

/// Where the confidence came from. These variants cannot be combined.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum ActConfidence {
    /// The operator named the region. The confidence gate does not apply.
    Inspected,
    /// A matcher total. Compared with [`MIN_ACT_CONFIDENCE_MILLIS`].
    Scored(f64),
    /// The top total and the runner-up total from one ranking. The top is
    /// compared with [`MIN_ACT_CONFIDENCE_MILLIS`], and the gap with
    /// [`MIN_ACT_MARGIN_MILLIS`].
    Ranked { top: f64, runner_up: f64 },
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

    /// Top and runner-up totals from the same ranking.
    pub fn ranked(mut self, top: f64, runner_up: f64) -> Self {
        self.confidence = ActConfidence::Ranked { top, runner_up };
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

/// How an act was carried out. Browser Use is semantic, not a CDP mechanism.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutedVia {
    Browser(ActMechanism),
    /// Region id, role, label, and action. No coordinates and no goal.
    BrowserUseSemantic,
    /// Region id, role, label, and action. No coordinates, no goal, no fusion.
    CuaSemantic,
}

impl ExecutedVia {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser(mechanism) => mechanism.as_str(),
            Self::BrowserUseSemantic => "browser-use-semantic",
            Self::CuaSemantic => "cua-semantic",
        }
    }
}

impl fmt::Display for ExecutedVia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionReceipt {
    kind: ExecutorKind,
    region_id: RegionId,
    action: Action,
    mechanism: ExecutedVia,
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
    pub const fn mechanism(&self) -> ExecutedVia {
        self.mechanism
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutorError {
    /// The backend has no session. macOS is always this.
    /// A browser stub is this; [`BrowserExecutor`] is not.
    /// A Browser Use stub is this; [`BrowserUseExecutor`] is not.
    /// A CUA stub is this; [`CuaExecutor`] is not.
    NotImplemented(ExecutorKind),
    /// No entry in the policy order was present in the available set.
    NoneAvailable,
    /// The act path named a backend that was not available.
    Unavailable(ExecutorKind),
    /// Matcher total is below [`MIN_ACT_CONFIDENCE_MILLIS`]. No click was sent.
    ConfidenceBelowThreshold {
        confidence_millis: i32,
        minimum_millis: i32,
    },
    /// The top and runner-up totals differ by less than
    /// [`MIN_ACT_MARGIN_MILLIS`]. No click was sent.
    AmbiguousTarget {
        top_millis: i32,
        runner_up_millis: i32,
        margin_millis: i32,
        minimum_margin_millis: i32,
    },
    NonFiniteConfidence,
    Browser(BrowserError),
    BrowserUse(BrowserUseError),
    Cua(CuaError),
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented(kind) => {
                write!(f, "{kind} executor is not implemented")
            }
            Self::NoneAvailable => f.write_str("no executor is available"),
            Self::Unavailable(kind) => {
                write!(f, "{kind} executor was requested but is not available")
            }
            Self::ConfidenceBelowThreshold {
                confidence_millis,
                minimum_millis,
            } => write!(
                f,
                "confidence {confidence_millis} is below the act minimum {minimum_millis}"
            ),
            Self::AmbiguousTarget {
                top_millis,
                runner_up_millis,
                margin_millis,
                minimum_margin_millis,
            } => write!(
                f,
                "top {top_millis} and runner-up {runner_up_millis} differ by {margin_millis} millis, below the act margin {minimum_margin_millis}"
            ),
            Self::NonFiniteConfidence => f.write_str("confidence must be finite"),
            Self::Browser(err) => write!(f, "{err}"),
            Self::BrowserUse(err) => write!(f, "{err}"),
            Self::Cua(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

pub trait ActionExecutor {
    fn kind(&self) -> ExecutorKind;

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError>;
}

/// Backend that always refuses. Used for macOS, and for a named backend with no session.
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

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        gate_confidence(request.confidence())?;
        Err(ExecutorError::NotImplemented(self.kind))
    }
}

/// Browser backend. macOS, Browser Use, and CUA are not constructed here.
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
        gate_confidence(request.confidence())?;
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
            mechanism: ExecutedVia::Browser(mechanism),
        })
    }
}

/// Browser Use backend. The target is the region hyper-use already resolved.
/// Submitting it is not navigation and not a goal.
pub struct BrowserUseExecutor<T: BrowserUseTransport> {
    transport: T,
    target: SemanticRequest,
}

impl<T: BrowserUseTransport> BrowserUseExecutor<T> {
    pub fn new(transport: T, target: SemanticRequest) -> Self {
        Self { transport, target }
    }

    pub fn target(&self) -> &SemanticRequest {
        &self.target
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }
}

impl BrowserUseExecutor<ReplayTransport> {
    pub fn from_replay(script: &str) -> Result<Self, BrowserUseError> {
        let transport = ReplayTransport::parse(script)?;
        let target = transport.expected().clone();
        Ok(Self { transport, target })
    }
}

impl<T: BrowserUseTransport> ActionExecutor for BrowserUseExecutor<T> {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::BrowserUse
    }

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        if request.region_id() != self.target.region_id() {
            return Err(ExecutorError::BrowserUse(BrowserUseError::UnknownRegion(
                request.region_id().to_string(),
            )));
        }
        if request.action() != self.target.action() {
            return Err(ExecutorError::BrowserUse(
                BrowserUseError::UnsupportedAction(request.action().as_str().to_owned()),
            ));
        }
        gate_confidence(request.confidence())?;
        let receipt = self
            .transport
            .submit(&self.target)
            .map_err(ExecutorError::BrowserUse)?;
        if receipt.region_id() != self.target.region_id()
            || receipt.action() != self.target.action()
        {
            return Err(ExecutorError::BrowserUse(BrowserUseError::ParamsMismatch {
                message: "browser-use receipt does not match the semantic request".into(),
            }));
        }
        Ok(ActionReceipt {
            kind: ExecutorKind::BrowserUse,
            region_id: request.region_id().clone(),
            action: request.action(),
            mechanism: ExecutedVia::BrowserUseSemantic,
        })
    }
}

/// CUA backend. The target is the region hyper-use already resolved.
/// Submitting it is not navigation, not fusion, and not a goal.
pub struct CuaExecutor<T: CuaTransport> {
    transport: T,
    target: CuaSemanticRequest,
}

impl<T: CuaTransport> CuaExecutor<T> {
    pub fn new(transport: T, target: CuaSemanticRequest) -> Self {
        Self { transport, target }
    }

    pub fn target(&self) -> &CuaSemanticRequest {
        &self.target
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }
}

impl CuaExecutor<CuaReplayTransport> {
    pub fn from_replay(script: &str) -> Result<Self, CuaError> {
        let transport = CuaReplayTransport::parse(script)?;
        let target = transport.expected().clone();
        Ok(Self { transport, target })
    }
}

impl<T: CuaTransport> ActionExecutor for CuaExecutor<T> {
    fn kind(&self) -> ExecutorKind {
        ExecutorKind::Cua
    }

    fn execute(&mut self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        if request.region_id() != self.target.region_id() {
            return Err(ExecutorError::Cua(CuaError::UnknownRegion(
                request.region_id().to_string(),
            )));
        }
        if request.action() != self.target.action() {
            return Err(ExecutorError::Cua(CuaError::UnsupportedAction(
                request.action().as_str().to_owned(),
            )));
        }
        gate_confidence(request.confidence())?;
        let receipt = self
            .transport
            .submit(&self.target)
            .map_err(ExecutorError::Cua)?;
        if receipt.region_id() != self.target.region_id()
            || receipt.action() != self.target.action()
        {
            return Err(ExecutorError::Cua(CuaError::ParamsMismatch {
                message: "cua receipt does not match the semantic request".into(),
            }));
        }
        Ok(ActionReceipt {
            kind: ExecutorKind::Cua,
            region_id: request.region_id().clone(),
            action: request.action(),
            mechanism: ExecutedVia::CuaSemantic,
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

/// Refuse a ranked act whose top is low or whose runner-up is too close.
/// The threshold is checked first, so a low top is `ConfidenceBelowThreshold`
/// even when it is also ambiguous. A runner-up above the top also refuses.
pub fn gate_ranked_confidence(top: f64, runner_up: f64) -> Result<(), ExecutorError> {
    if !runner_up.is_finite() {
        return Err(ExecutorError::NonFiniteConfidence);
    }
    gate_scored_confidence(top)?;
    let top_millis = (top * 1000.0).round() as i32;
    let runner_up_millis = (runner_up * 1000.0).round() as i32;
    let margin_millis = top_millis - runner_up_millis;
    if margin_millis < MIN_ACT_MARGIN_MILLIS {
        return Err(ExecutorError::AmbiguousTarget {
            top_millis,
            runner_up_millis,
            margin_millis,
            minimum_margin_millis: MIN_ACT_MARGIN_MILLIS,
        });
    }
    Ok(())
}

/// The single act gate. `Inspected` is not gated.
pub fn gate_confidence(confidence: ActConfidence) -> Result<(), ExecutorError> {
    match confidence {
        ActConfidence::Inspected => Ok(()),
        ActConfidence::Scored(score) => gate_scored_confidence(score),
        ActConfidence::Ranked { top, runner_up } => gate_ranked_confidence(top, runner_up),
    }
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
            Self::AmbiguousTarget { top_millis, .. } => {
                let confidence =
                    MatcherConfidence::try_new(f64::from(*top_millis) / 1000.0).ok()?;
                Some(ComputerResult::refused(
                    confidence,
                    FallbackReason::Ambiguous,
                ))
            }
            Self::NotImplemented(_) => Some(ComputerResult::refused(
                MatcherConfidence::try_new(0.0).expect("zero is finite"),
                FallbackReason::NotImplemented,
            )),
            Self::NonFiniteConfidence
            | Self::NoneAvailable
            | Self::Unavailable(_)
            | Self::Browser(_)
            | Self::BrowserUse(_)
            | Self::Cua(_) => None,
        }
    }
}

/// First kind in [`DEFAULT_POLICY_ORDER`] that is also in `available`.
/// [`ExecutorKind::BrowserUse`] and [`ExecutorKind::Cua`] are not in that
/// order, so neither is selected here.
pub fn select_executor(available: &[ExecutorKind]) -> Result<ExecutorKind, ExecutorError> {
    for kind in DEFAULT_POLICY_ORDER {
        if available.contains(&kind) {
            return Ok(kind);
        }
    }
    Err(ExecutorError::NoneAvailable)
}

/// The named backend, if the caller listed it. This does not fall through.
pub fn select_requested(
    requested: ExecutorKind,
    available: &[ExecutorKind],
) -> Result<ExecutorKind, ExecutorError> {
    if available.contains(&requested) {
        Ok(requested)
    } else {
        Err(ExecutorError::Unavailable(requested))
    }
}

/// `None` uses [`select_executor`] and ignores Browser Use and CUA. `Some`
/// uses [`select_requested`] and does not substitute another backend.
pub fn select_act_executor(
    requested: Option<ExecutorKind>,
    available: &[ExecutorKind],
) -> Result<ExecutorKind, ExecutorError> {
    match requested {
        Some(kind) => select_requested(kind, available),
        None => {
            let policy: Vec<ExecutorKind> = available
                .iter()
                .copied()
                .filter(|kind| *kind != ExecutorKind::BrowserUse && *kind != ExecutorKind::Cua)
                .collect();
            select_executor(&policy)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_prefers_browser_then_macos_and_does_not_select_cua() {
        assert_eq!(
            DEFAULT_POLICY_ORDER,
            [ExecutorKind::Browser, ExecutorKind::Macos]
        );
        assert!(!DEFAULT_POLICY_ORDER.contains(&ExecutorKind::Cua));
        assert!(!DEFAULT_POLICY_ORDER.contains(&ExecutorKind::BrowserUse));
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Browser]).unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Macos]).unwrap(),
            ExecutorKind::Macos
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua]).unwrap_err(),
            ExecutorError::NoneAvailable
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::BrowserUse]).unwrap_err(),
            ExecutorError::NoneAvailable
        );
        assert_eq!(
            select_act_executor(
                None,
                &[
                    ExecutorKind::Browser,
                    ExecutorKind::Cua,
                    ExecutorKind::BrowserUse
                ]
            )
            .unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_act_executor(None, &[ExecutorKind::Cua]).unwrap_err(),
            ExecutorError::NoneAvailable
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
        assert!(ExecutorKind::Cua.status().contains("does not navigate"));
        assert!(!ExecutorKind::Cua.status().contains("fusion"));
        assert_eq!(
            hyper_use_browser::BrowserStub.status(),
            hyper_use_browser::STATUS
        );
        assert_eq!(hyper_use_macos::MacosStub.status(), hyper_use_macos::STATUS);
        assert_eq!(hyper_use_cua::CuaStub.status(), hyper_use_cua::PIXEL_STATUS);
        assert!(hyper_use_cua::CuaStub.status().contains("later phase"));
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
            ExecutedVia::Browser(hyper_use_browser::ActMechanism::DomSemantic)
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

    #[test]
    fn browser_use_is_opt_in_and_not_a_fallback() {
        assert_eq!(
            select_executor(&[ExecutorKind::BrowserUse, ExecutorKind::Browser]).unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_executor(&[ExecutorKind::BrowserUse]).unwrap_err(),
            ExecutorError::NoneAvailable
        );
        assert_eq!(
            select_act_executor(None, &[ExecutorKind::Browser, ExecutorKind::BrowserUse]).unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_act_executor(Some(ExecutorKind::BrowserUse), &[ExecutorKind::BrowserUse])
                .unwrap(),
            ExecutorKind::BrowserUse
        );
        let err = select_requested(ExecutorKind::BrowserUse, &[ExecutorKind::Browser]).unwrap_err();
        assert_eq!(err, ExecutorError::Unavailable(ExecutorKind::BrowserUse));
        assert_eq!(
            err.to_string(),
            "browser-use executor was requested but is not available"
        );
        assert_eq!(
            ExecutorKind::parse("browser-use"),
            Some(ExecutorKind::BrowserUse)
        );
        assert_eq!(ExecutorKind::parse("browser"), Some(ExecutorKind::Browser));
        assert!(ExecutorKind::parse("navigate").is_none());
        assert!(ExecutorKind::BrowserUse
            .status()
            .contains("does not navigate"));
    }

    #[test]
    fn browser_use_stub_does_not_panic() {
        let request = ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click);
        let mut stub = StubExecutor::new(ExecutorKind::BrowserUse);
        let err = stub.execute(&request).unwrap_err();
        assert_eq!(err, ExecutorError::NotImplemented(ExecutorKind::BrowserUse));
        assert_eq!(err.to_string(), "browser-use executor is not implemented");
        let scored = request.scored(0.49);
        let err = stub.execute(&scored).unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
    }

    #[test]
    fn browser_use_low_confidence_does_not_submit_and_high_confidence_records_a_receipt() {
        let mut low = BrowserUseExecutor::from_replay(include_str!(
            "../../../fixtures/sign-in.browser-use.json"
        ))
        .unwrap();
        let err = low
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.49),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        assert!(low.transport().submitted().is_empty());

        let mut unknown = BrowserUseExecutor::from_replay(include_str!(
            "../../../fixtures/sign-in.browser-use.json"
        ))
        .unwrap();
        let err = unknown
            .execute(&ActionRequest::new(
                RegionId::try_new("n200").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::BrowserUse(BrowserUseError::UnknownRegion("n200".into()))
        );
        assert!(unknown.transport().submitted().is_empty());

        let mut high = BrowserUseExecutor::from_replay(include_str!(
            "../../../fixtures/sign-in.browser-use.json"
        ))
        .unwrap();
        let receipt = high
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.55),
            )
            .unwrap();
        assert_eq!(receipt.kind(), ExecutorKind::BrowserUse);
        assert_eq!(receipt.mechanism(), ExecutedVia::BrowserUseSemantic);
        assert_eq!(receipt.mechanism().as_str(), "browser-use-semantic");
        assert_eq!(high.transport().submitted().len(), 1);
        let sent = &high.transport().submitted()[0];
        assert_eq!(sent.region_id().as_str(), "n100");
        assert_eq!(sent.role(), hyper_use_core::Role::Button);
        assert_eq!(sent.label(), "Sign in");
        assert_eq!(sent.action(), Action::Click);
        assert!(!sent.to_wire().contains("\"x\""));
        assert!(!sent.to_wire().contains("goal"));
    }

    #[test]
    fn browser_use_rejection_and_receipt_mismatch_are_typed() {
        let mut rejected = BrowserUseExecutor::from_replay(include_str!(
            "../../../fixtures/sign-in-reject.browser-use.json"
        ))
        .unwrap();
        let err = rejected
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::BrowserUse(BrowserUseError::Rejected {
                message: "control refused the semantic act".into(),
            })
        );
        assert_eq!(rejected.transport().submitted().len(), 1);
        assert!(err.to_string().contains("control refused the semantic act"));

        let target = SemanticRequest::new(
            RegionId::try_new("n100").unwrap(),
            hyper_use_core::Role::Button,
            "Sign in",
            Action::Click,
        )
        .unwrap();
        let mut lying = BrowserUseExecutor::new(LieTransport { calls: 0 }, target);
        let err = lying
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(lying.transport().calls, 1);
        assert_eq!(
            err,
            ExecutorError::BrowserUse(BrowserUseError::ParamsMismatch {
                message: "browser-use receipt does not match the semantic request".into(),
            })
        );
    }

    struct LieTransport {
        calls: usize,
    }

    impl BrowserUseTransport for LieTransport {
        fn submit(
            &mut self,
            request: &SemanticRequest,
        ) -> Result<hyper_use_browser_use::TransportReceipt, BrowserUseError> {
            self.calls += 1;
            let _ = request;
            Ok(hyper_use_browser_use::TransportReceipt::new(
                RegionId::try_new("other").unwrap(),
                Action::Click,
            ))
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(16))]
        #[test]
        fn scored_below_the_gate_does_not_submit(millis in 0i32..550) {
            let confidence = f64::from(millis) / 1000.0;
            let mut executor = BrowserUseExecutor::from_replay(include_str!(
                "../../../fixtures/sign-in.browser-use.json"
            ))
            .unwrap();
            let err = executor
                .execute(
                    &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click)
                        .scored(confidence),
                )
                .unwrap_err();
            assert_eq!(
                err,
                ExecutorError::ConfidenceBelowThreshold {
                    confidence_millis: millis,
                    minimum_millis: 550,
                }
            );
            assert!(executor.transport().submitted().is_empty());
        }
    }

    #[test]
    fn cua_is_opt_in_and_not_a_fallback() {
        assert_eq!(
            select_requested(ExecutorKind::Cua, &[ExecutorKind::Cua]).unwrap(),
            ExecutorKind::Cua
        );
        let err = select_requested(ExecutorKind::Cua, &[ExecutorKind::Browser]).unwrap_err();
        assert_eq!(err, ExecutorError::Unavailable(ExecutorKind::Cua));
        assert_eq!(
            err.to_string(),
            "cua executor was requested but is not available"
        );
        assert_eq!(ExecutorKind::parse("cua"), Some(ExecutorKind::Cua));
        assert!(ExecutorKind::Cua.status().contains("does not navigate"));
    }

    #[test]
    fn cua_stub_does_not_panic() {
        let request = ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click);
        let mut stub = StubExecutor::new(ExecutorKind::Cua);
        let err = stub.execute(&request).unwrap_err();
        assert_eq!(err, ExecutorError::NotImplemented(ExecutorKind::Cua));
        assert_eq!(err.to_string(), "cua executor is not implemented");
        let scored = request.scored(0.49);
        let err = stub.execute(&scored).unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
    }

    #[test]
    fn cua_low_confidence_does_not_submit_and_high_confidence_records_a_receipt() {
        let mut low =
            CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in.cua.json")).unwrap();
        let err = low
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.49),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        assert!(low.transport().submitted().is_empty());

        let mut unknown =
            CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in.cua.json")).unwrap();
        let err = unknown
            .execute(&ActionRequest::new(
                RegionId::try_new("n200").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::Cua(CuaError::UnknownRegion("n200".into()))
        );
        assert!(unknown.transport().submitted().is_empty());

        let mut wrong = CuaExecutor::from_replay(
            r#"{"kind":"cua-replay","request":{"region_id":"n100","role":"button","label":"Sign in","action":"type"},"result":{"accepted":true}}"#,
        )
        .unwrap();
        let err = wrong
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.49),
            )
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::Cua(CuaError::UnsupportedAction("click".into()))
        );
        assert!(wrong.transport().submitted().is_empty());

        let mut high =
            CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in.cua.json")).unwrap();
        let receipt = high
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).scored(0.55),
            )
            .unwrap();
        assert_eq!(receipt.kind(), ExecutorKind::Cua);
        assert_eq!(receipt.mechanism(), ExecutedVia::CuaSemantic);
        assert_eq!(receipt.mechanism().as_str(), "cua-semantic");
        assert_eq!(high.transport().submitted().len(), 1);
        let sent = &high.transport().submitted()[0];
        assert_eq!(sent.region_id().as_str(), "n100");
        assert_eq!(sent.role(), hyper_use_core::Role::Button);
        assert_eq!(sent.label(), "Sign in");
        assert_eq!(sent.action(), Action::Click);
        assert!(!sent.to_wire().contains("\"x\""));
        assert!(!sent.to_wire().contains("goal"));
    }

    #[test]
    fn cua_rejection_receipt_mismatch_and_non_finite_are_typed() {
        let mut rejected =
            CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in-reject.cua.json"))
                .unwrap();
        let err = rejected
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(
            err,
            ExecutorError::Cua(CuaError::Rejected {
                message: "control refused the semantic act".into(),
            })
        );
        assert_eq!(rejected.transport().submitted().len(), 1);
        assert_eq!(
            err.to_string(),
            "cua rejected the semantic act: control refused the semantic act"
        );

        let target = CuaSemanticRequest::new(
            RegionId::try_new("n100").unwrap(),
            hyper_use_core::Role::Button,
            "Sign in",
            Action::Click,
        )
        .unwrap();
        let mut lying = CuaExecutor::new(CuaLieTransport { calls: 0 }, target);
        let err = lying
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap_err();
        assert_eq!(lying.transport().calls, 1);
        assert_eq!(
            err,
            ExecutorError::Cua(CuaError::ParamsMismatch {
                message: "cua receipt does not match the semantic request".into(),
            })
        );

        let mut non_finite =
            CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in.cua.json")).unwrap();
        let err = non_finite
            .execute(
                &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click)
                    .scored(f64::NAN),
            )
            .unwrap_err();
        assert_eq!(err, ExecutorError::NonFiniteConfidence);
        assert!(non_finite.transport().submitted().is_empty());
    }

    struct CuaLieTransport {
        calls: usize,
    }

    impl CuaTransport for CuaLieTransport {
        fn submit(
            &mut self,
            request: &CuaSemanticRequest,
        ) -> Result<hyper_use_cua::TransportReceipt, CuaError> {
            self.calls += 1;
            let _ = request;
            Ok(hyper_use_cua::TransportReceipt::new(
                RegionId::try_new("other").unwrap(),
                Action::Click,
            ))
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(16))]
        #[test]
        fn cua_scored_below_the_gate_does_not_submit(millis in 0i32..550) {
            let confidence = f64::from(millis) / 1000.0;
            let mut executor =
                CuaExecutor::from_replay(include_str!("../../../fixtures/sign-in.cua.json"))
                    .unwrap();
            let err = executor
                .execute(
                    &ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click)
                        .scored(confidence),
                )
                .unwrap_err();
            assert_eq!(
                err,
                ExecutorError::ConfidenceBelowThreshold {
                    confidence_millis: millis,
                    minimum_millis: 550,
                }
            );
            assert!(executor.transport().submitted().is_empty());
        }
    }

    #[test]
    fn ambiguous_ranked_act_does_not_click_and_refusal_is_typed() {
        let mut executor = BrowserExecutor::new(BrowserSession::new(
            hyper_use_browser::ReplayTransport::parse(include_str!(
                "../../../fixtures/sign-in-press.cdp.json"
            ))
            .unwrap(),
        ));
        let request =
            ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click).ranked(1.0, 0.98);
        let err = executor.execute(&request).unwrap_err();
        assert_eq!(
            err,
            ExecutorError::AmbiguousTarget {
                top_millis: 1000,
                runner_up_millis: 980,
                margin_millis: 20,
                minimum_margin_millis: 50,
            }
        );
        assert_eq!(
            err.to_string(),
            "top 1000 and runner-up 980 differ by 20 millis, below the act margin 50"
        );
        assert!(executor.session().transport().logged_methods().is_empty());
        let refused = err.refusal_result().unwrap();
        assert!(!refused.executed());
        assert_eq!(
            refused.fallback(),
            Some(hyper_use_protocol::FallbackReason::Ambiguous)
        );
        assert_eq!(refused.confidence().get(), 1.0);
    }

    #[test]
    fn low_confidence_wins_over_ambiguity() {
        assert_eq!(
            gate_ranked_confidence(0.50, 0.49).unwrap_err(),
            ExecutorError::ConfidenceBelowThreshold {
                confidence_millis: 500,
                minimum_millis: 550,
            }
        );
        assert_eq!(
            gate_ranked_confidence(0.9, 0.95).unwrap_err(),
            ExecutorError::AmbiguousTarget {
                top_millis: 900,
                runner_up_millis: 950,
                margin_millis: -50,
                minimum_margin_millis: 50,
            }
        );
        assert_eq!(
            gate_ranked_confidence(0.9, f64::NAN).unwrap_err(),
            ExecutorError::NonFiniteConfidence
        );
        assert!(gate_ranked_confidence(1.0, 0.875).is_ok());
        assert!(gate_ranked_confidence(1.0, 0.95).is_ok());
    }

    #[test]
    fn inspected_act_ignores_the_margin() {
        assert!(gate_confidence(ActConfidence::Inspected).is_ok());
        let mut executor = BrowserExecutor::new(BrowserSession::new(
            hyper_use_browser::ReplayTransport::parse(include_str!(
                "../../../fixtures/sign-in-press.cdp.json"
            ))
            .unwrap(),
        ));
        let receipt = executor
            .execute(&ActionRequest::new(
                RegionId::try_new("n100").unwrap(),
                Action::Click,
            ))
            .unwrap();
        assert_eq!(receipt.region_id().as_str(), "n100");
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(16))]
        #[test]
        fn ranked_within_margin_does_not_submit(top in 550i32..=1000, gap in 0i32..50) {
            let top_f = f64::from(top) / 1000.0;
            let runner_f = f64::from(top - gap) / 1000.0;
            let request = ActionRequest::new(RegionId::try_new("n100").unwrap(), Action::Click)
                .ranked(top_f, runner_f);
            let mut browser_use = BrowserUseExecutor::from_replay(include_str!(
                "../../../fixtures/sign-in.browser-use.json"
            ))
            .unwrap();
            let is_ambiguous = matches!(
                browser_use.execute(&request).unwrap_err(),
                ExecutorError::AmbiguousTarget { .. }
            );
            proptest::prop_assert!(is_ambiguous);
            proptest::prop_assert!(browser_use.transport().submitted().is_empty());
            let mut cua = CuaExecutor::from_replay(include_str!(
                "../../../fixtures/sign-in.cua.json"
            ))
            .unwrap();
            let is_ambiguous = matches!(
                cua.execute(&request).unwrap_err(),
                ExecutorError::AmbiguousTarget { .. }
            );
            proptest::prop_assert!(is_ambiguous);
            proptest::prop_assert!(cua.transport().submitted().is_empty());
        }

        #[test]
        fn ranked_outside_margin_passes_the_gate(top in 600i32..=1000, gap in 50i32..=600) {
            let runner = (top - gap).max(0);
            proptest::prop_assume!(top - runner >= 50);
            let result = gate_ranked_confidence(
                f64::from(top) / 1000.0,
                f64::from(runner) / 1000.0,
            );
            proptest::prop_assert_eq!(result, Ok(()));
        }
    }
}
