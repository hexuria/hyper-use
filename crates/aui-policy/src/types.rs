//! Policy trait and decision types.

use std::fmt;

use aui_core::{ActionId, ActionKind, ActionSpace};

use crate::goal::AgentGoal;

/// One ranked candidate the policy considered (for trails / escalation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankedAction {
    pub id: ActionId,
    pub kind: ActionKind,
    pub label: String,
    /// Instinct confidence millis 0..=1000 (not a probability).
    pub confidence_millis: i16,
}

/// Successful finite choice from a policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyDecision {
    pub action_id: ActionId,
    pub kind: ActionKind,
    pub target_label: String,
    pub confidence_millis: i16,
    pub operation_ranked: Vec<RankedAction>,
    pub target_ranked: Vec<RankedAction>,
}

impl PolicyDecision {
    pub fn is_terminal(&self) -> bool {
        matches!(self.kind, ActionKind::Done | ActionKind::Blocked)
    }
}

/// Policy result: choose, abstain, or hard error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyOutcome {
    Choice(PolicyDecision),
    Abstain {
        reason: String,
        operation_ranked: Vec<RankedAction>,
        target_ranked: Vec<RankedAction>,
    },
}

impl PolicyOutcome {
    pub fn as_choice(&self) -> Option<&PolicyDecision> {
        match self {
            Self::Choice(d) => Some(d),
            Self::Abstain { .. } => None,
        }
    }
}

/// Errors that prevent a decision attempt (not abstention).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    EmptyGoal,
    EmptyActionSpace,
    Internal(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGoal => f.write_str("agent goal is empty"),
            Self::EmptyActionSpace => f.write_str("action space has no candidates"),
            Self::Internal(msg) => write!(f, "policy internal: {msg}"),
        }
    }
}

impl std::error::Error for PolicyError {}

/// Past executed / observed steps (consumer-owned; policy may use for repetition).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub step: u32,
    pub action_id: ActionId,
    pub kind: ActionKind,
    pub label: String,
    pub verification: String,
}

/// Policy over a finite [`ActionSpace`].
pub trait BrowserPolicy {
    fn decide(
        &mut self,
        space: &ActionSpace,
        goal: &AgentGoal,
        history: &[HistoryEntry],
    ) -> Result<PolicyOutcome, PolicyError>;

    /// Stable name recorded in the battle diary (`instinct`, `jev`,
    /// `clef-flash`, …). Default keeps old test policies honest.
    fn name(&self) -> &'static str {
        "policy"
    }

    /// Which arm produced the most recent decision. Escalating policies
    /// override this to report the arm that answered, not the wrapper.
    fn decision_source(&self) -> &'static str {
        self.name()
    }

    /// The situation the next `decide` runs in (site, front layer, roles
    /// — the decision-time half of what the dojo journals; `near` is a
    /// post-decision artifact and is not context). Default no-op: only
    /// policies that consult situational knowledge (the dojo) need it.
    fn set_situation(&mut self, _ctx: &PolicyContext) {}
}

/// Where a decision happens, as far as the caller can know before the
/// policy answers. Plain owned data — no browser types.
#[derive(Clone, Debug, Default)]
pub struct PolicyContext {
    pub site_url: Option<String>,
    pub site_title: Option<String>,
    pub front_layer: bool,
    /// Roles present on the page, sorted.
    pub roles: Vec<String>,
}
