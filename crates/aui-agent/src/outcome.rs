use aui_core::{ActionId, ActionKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationKind {
    Success,
    NoEffect,
    WrongEffect,
    Navigation,
    StateChanged,
    Unknown,
    Skipped,
}

impl VerificationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::NoEffect => "no-effect",
            Self::WrongEffect => "wrong-effect",
            Self::Navigation => "navigation",
            Self::StateChanged => "state-changed",
            Self::Unknown => "unknown",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRecord {
    pub step: u32,
    pub action_id: ActionId,
    pub kind: ActionKind,
    pub label: String,
    pub verification: VerificationKind,
    pub stale_retries: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentOutcome {
    Done {
        steps: Vec<StepRecord>,
        reason: String,
    },
    Blocked {
        steps: Vec<StepRecord>,
        reason: String,
    },
    Abstained {
        steps: Vec<StepRecord>,
        reason: String,
    },
    Failed {
        steps: Vec<StepRecord>,
        error: String,
    },
}

impl AgentOutcome {
    pub fn steps(&self) -> &[StepRecord] {
        match self {
            Self::Done { steps, .. }
            | Self::Blocked { steps, .. }
            | Self::Abstained { steps, .. }
            | Self::Failed { steps, .. } => steps,
        }
    }
}
