//! JEV-facing task and result types.
//!
//! An agent loop that already exists outside this repository owns goals,
//! delegation, and the journal. hyper-use only accepts one locate or one act.
//! There is no navigate intent and no method that plans a multi-step workflow.

use std::fmt;

use hyper_use_core::{tokenize, Action, LocateQuery, RegionId};

/// Why an act was not performed. This is data for a host journal, not a second
/// executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackReason {
    /// Matcher total was below the act threshold. Nothing was clicked.
    LowConfidence,
    /// The chosen backend cannot run this action.
    NotImplemented,
    /// The postcondition did not hold.
    VerifyFailed,
    /// The top two candidates were too close. Nothing was clicked.
    Ambiguous,
}

impl FallbackReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowConfidence => "low-confidence",
            Self::NotImplemented => "not-implemented",
            Self::VerifyFailed => "verify-failed",
            Self::Ambiguous => "ambiguous",
        }
    }
}

impl fmt::Display for FallbackReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProtocolError {
    NonFiniteConfidence,
    EmptyExpectedText,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteConfidence => f.write_str("confidence must be finite"),
            Self::EmptyExpectedText => {
                f.write_str("expected text must contain at least one alphanumeric token")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Finite matcher total. Not a probability. Negative values are allowed
/// because penalties can drive a score below zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatcherConfidence(f64);

impl MatcherConfidence {
    pub fn try_new(value: f64) -> Result<Self, ProtocolError> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(ProtocolError::NonFiniteConfidence)
        }
    }

    pub const fn get(self) -> f64 {
        self.0
    }
}

/// What the host wants hyper-use to resolve. A locate or a single act.
/// A workflow goal is not representable.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Intent {
    Locate(LocateQuery),
    Act { region_id: RegionId, action: Action },
}

/// Optional host limits. Absence means the product default applies at act time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    min_confidence: Option<f64>,
}

impl Constraints {
    pub const fn none() -> Self {
        Self {
            min_confidence: None,
        }
    }

    pub fn min_confidence(value: f64) -> Result<Self, ProtocolError> {
        if !value.is_finite() {
            return Err(ProtocolError::NonFiniteConfidence);
        }
        Ok(Self {
            min_confidence: Some(value),
        })
    }

    pub const fn min_confidence_value(self) -> Option<f64> {
        self.min_confidence
    }
}

/// Postcondition the host asked verify to check. Both fields may be set.
/// Neither field plans how to get there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedOutcome {
    text: Option<String>,
    absent_region: Option<RegionId>,
}

impl ExpectedOutcome {
    pub fn text_present(text: impl Into<String>) -> Result<Self, ProtocolError> {
        let text = text.into();
        if tokenize(&text).is_empty() {
            return Err(ProtocolError::EmptyExpectedText);
        }
        Ok(Self {
            text: Some(text),
            absent_region: None,
        })
    }

    pub fn region_absent(id: RegionId) -> Self {
        Self {
            text: None,
            absent_region: Some(id),
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn absent_region(&self) -> Option<&RegionId> {
        self.absent_region.as_ref()
    }
}

/// One computer-use request under an existing JEV loop.
///
/// `intent` is a locate or an act, not a goal such as "sign the user in".
/// This type has no planner and no navigate constructor.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputerTask {
    intent: Intent,
    constraints: Constraints,
    expected_outcome: ExpectedOutcome,
}

impl ComputerTask {
    pub fn locate(
        query: LocateQuery,
        constraints: Constraints,
        expected_outcome: ExpectedOutcome,
    ) -> Self {
        Self {
            intent: Intent::Locate(query),
            constraints,
            expected_outcome,
        }
    }

    pub fn act(
        region_id: RegionId,
        action: Action,
        constraints: Constraints,
        expected_outcome: ExpectedOutcome,
    ) -> Self {
        Self {
            intent: Intent::Act { region_id, action },
            constraints,
            expected_outcome,
        }
    }

    pub fn intent(&self) -> &Intent {
        &self.intent
    }

    pub const fn constraints(&self) -> Constraints {
        self.constraints
    }

    pub fn expected_outcome(&self) -> &ExpectedOutcome {
        &self.expected_outcome
    }
}

/// Id-level change between two observations. Not a second diff algorithm:
/// a host fills this from `hyper-use-observe::diff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDelta {
    added: Vec<RegionId>,
    removed: Vec<RegionId>,
    changed: Vec<RegionId>,
}

impl StateDelta {
    pub fn new(
        mut added: Vec<RegionId>,
        mut removed: Vec<RegionId>,
        mut changed: Vec<RegionId>,
    ) -> Self {
        added.sort();
        removed.sort();
        changed.sort();
        added.dedup();
        removed.dedup();
        changed.dedup();
        Self {
            added,
            removed,
            changed,
        }
    }

    pub fn empty() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
        }
    }

    pub fn added(&self) -> &[RegionId] {
        &self.added
    }

    pub fn removed(&self) -> &[RegionId] {
        &self.removed
    }

    pub fn changed(&self) -> &[RegionId] {
        &self.changed
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Which backend a host should record. This is a label, not the policy in
/// `hyper-use-executor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportedExecutor {
    Browser,
    Macos,
    Cua,
}

impl ReportedExecutor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Macos => "macos",
            Self::Cua => "cua",
        }
    }
}

/// What hyper-use hands back. `executed == false` means no click was sent.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputerResult {
    target: Option<RegionId>,
    action: Option<Action>,
    executor: Option<ReportedExecutor>,
    executed: bool,
    verified: bool,
    state_delta: StateDelta,
    confidence: MatcherConfidence,
    fallback: Option<FallbackReason>,
}

impl ComputerResult {
    pub fn recorded(
        target: RegionId,
        action: Action,
        executor: ReportedExecutor,
        verified: bool,
        state_delta: StateDelta,
        confidence: MatcherConfidence,
    ) -> Self {
        Self {
            target: Some(target),
            action: Some(action),
            executor: Some(executor),
            executed: true,
            verified,
            state_delta,
            confidence,
            fallback: None,
        }
    }

    /// Refusal record. `executed` is false. Callers must not also click.
    pub fn refused(confidence: MatcherConfidence, fallback: FallbackReason) -> Self {
        Self {
            target: None,
            action: None,
            executor: None,
            executed: false,
            verified: false,
            state_delta: StateDelta::empty(),
            confidence,
            fallback: Some(fallback),
        }
    }

    pub fn target(&self) -> Option<&RegionId> {
        self.target.as_ref()
    }

    pub const fn action(&self) -> Option<Action> {
        self.action
    }

    pub const fn executor(&self) -> Option<ReportedExecutor> {
        self.executor
    }

    pub const fn executed(&self) -> bool {
        self.executed
    }

    pub const fn verified(&self) -> bool {
        self.verified
    }

    pub fn state_delta(&self) -> &StateDelta {
        &self.state_delta
    }

    pub const fn confidence(&self) -> MatcherConfidence {
        self.confidence
    }

    pub const fn fallback(&self) -> Option<FallbackReason> {
        self.fallback
    }
}
