//! Guard and verify messages for the action-firewall product.
//!
//! Hyper-Use does not click. A host proposes a target; the runtime returns a
//! [`GuardDecision`]-shaped payload. After the host acts, verify reports the
//! postcondition. JEV `ComputerTask` types in [`crate::contract`] are legacy
//! and are not the product surface.

use std::fmt;

use hyper_use_core::{RegionId, Role};

use crate::MatcherConfidence;

/// Why guard refused or escalated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GuardReason {
    LowConfidence,
    Ambiguous,
    MissingTarget,
    Disabled,
    Hidden,
    Occluded,
    Offscreen,
    ProposedNotTop,
    NoEffect,
    WrongEffect,
    /// The target sits behind a dialog: outside a modal dialog, or under a
    /// non-modal dialog's box. A script click would bypass the dialog.
    FrontLayer,
    /// The front layer (open dialogs) differs from the observation the host
    /// decided on. The host must observe again before it acts.
    WorldChanged,
}

impl GuardReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LowConfidence => "low-confidence",
            Self::Ambiguous => "ambiguous",
            Self::MissingTarget => "missing-target",
            Self::Disabled => "disabled",
            Self::Hidden => "hidden",
            Self::Occluded => "occluded",
            Self::Offscreen => "offscreen",
            Self::ProposedNotTop => "proposed-not-top",
            Self::NoEffect => "no-effect",
            Self::WrongEffect => "wrong-effect",
            Self::FrontLayer => "front-layer",
            Self::WorldChanged => "world-changed",
        }
    }
}

impl fmt::Display for GuardReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One ranked candidate returned with a decision.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardCandidate {
    pub id: RegionId,
    pub role: Role,
    pub label: String,
    pub confidence: f64,
}

/// Observables that justified allow / refuse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardEvidence {
    pub visible: bool,
    pub enabled: bool,
    pub occluded: bool,
    pub hidden: bool,
    pub offscreen: bool,
    pub role: Role,
}

/// Firewall decision. Never implies Hyper-Use clicked.
///
/// [`GuardDecision::Allow`] carries an [`crate::ActionTicket`]. The host must
/// revalidate that ticket against a fresh observation before clicking the
/// exact target. Hyper-Use itself never presses.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum GuardDecision {
    Allow {
        target: GuardCandidate,
        confidence: MatcherConfidence,
        margin: Option<MatcherConfidence>,
        evidence: GuardEvidence,
        ticket: crate::ActionTicket,
    },
    Refuse {
        reason: GuardReason,
        candidates: Vec<GuardCandidate>,
    },
    Escalate {
        reason: GuardReason,
        candidates: Vec<GuardCandidate>,
    },
}

impl GuardDecision {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Allow { .. } => "allow",
            Self::Refuse { .. } => "refuse",
            Self::Escalate { .. } => "escalate",
        }
    }
}

/// Product loop phases. Declaration order is the loop order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FirewallPhase {
    Observe,
    Guard,
    Verify,
}

pub const FIREWALL_ORDER: [FirewallPhase; 3] = [
    FirewallPhase::Observe,
    FirewallPhase::Guard,
    FirewallPhase::Verify,
];

impl FirewallPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Guard => "guard",
            Self::Verify => "verify",
        }
    }
}
