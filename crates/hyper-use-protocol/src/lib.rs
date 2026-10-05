//! Loop messages for hyper-use.
//!
//! The only legal operations are observe, locate, inspect when the locate
//! result is ambiguous, act, diff, then verify. This crate names that order.
//! It does not run it, and it does not plan a navigation. Coordinates are not
//! a message: an act names a [`RegionId`].
//!
//! [`ComputerTask`] and [`ComputerResult`] are the contract an existing JEV
//! loop would hand across. Nothing here executes a goal or calls a browser.

#![forbid(unsafe_code)]

use hyper_use_core::{Action, LocateQuery, RegionId};

/// Phases of one hyper-use turn. Declaration order is the loop order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoopPhase {
    Observe,
    Locate,
    Inspect,
    Act,
    Diff,
    Verify,
}

pub const LOOP_ORDER: [LoopPhase; 6] = [
    LoopPhase::Observe,
    LoopPhase::Locate,
    LoopPhase::Inspect,
    LoopPhase::Act,
    LoopPhase::Diff,
    LoopPhase::Verify,
];

impl LoopPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Locate => "locate",
            Self::Inspect => "inspect",
            Self::Act => "act",
            Self::Diff => "diff",
            Self::Verify => "verify",
        }
    }
}

/// A request a host or MCP tool can hand to a future runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    Observe,
    Locate(LocateQuery),
    /// Inspect is required when locate does not separate a single target.
    Inspect {
        region_id: RegionId,
    },
    Act {
        region_id: RegionId,
        action: Action,
    },
    Verify {
        region_id: RegionId,
    },
    /// Diff the host's previous observation against the current one.
    /// The manifolds stay with the host; this message does not embed them.
    Diff,
}

mod contract;

pub use contract::{
    ComputerResult, ComputerTask, Constraints, ExpectedOutcome, FallbackReason, Intent,
    MatcherConfidence, ProtocolError, ReportedExecutor, StateDelta,
};

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::Role;

    #[test]
    fn loop_order_is_observe_locate_inspect_act_verify() {
        let names: Vec<_> = LOOP_ORDER.iter().map(|phase| phase.as_str()).collect();
        assert_eq!(
            names,
            ["observe", "locate", "inspect", "act", "diff", "verify"]
        );
    }

    #[test]
    fn act_names_a_region_not_a_coordinate() {
        let request = Request::Act {
            region_id: RegionId::try_new("nav-settings").unwrap(),
            action: Action::Click,
        };
        match request {
            Request::Act { region_id, action } => {
                assert_eq!(region_id.as_str(), "nav-settings");
                assert_eq!(action, Action::Click);
            }
            _ => panic!("expected act"),
        }
        let locate = Request::Locate(
            LocateQuery::new()
                .text("Settings")
                .unwrap()
                .role(Role::Button),
        );
        assert!(matches!(locate, Request::Locate(_)));
        assert!(matches!(Request::Diff, Request::Diff));
    }

    #[test]
    fn computer_task_is_locate_or_act_and_refusal_does_not_claim_execution() {
        let task = ComputerTask::locate(
            LocateQuery::new().text("Sign in").unwrap(),
            Constraints::none(),
            ExpectedOutcome::text_present("Welcome").unwrap(),
        );
        assert!(matches!(task.intent(), Intent::Locate(_)));
        assert_eq!(task.expected_outcome().text(), Some("Welcome"));
        let acted = ComputerTask::act(
            RegionId::try_new("n100").unwrap(),
            Action::Click,
            Constraints::min_confidence(0.55).unwrap(),
            ExpectedOutcome::region_absent(RegionId::try_new("n100").unwrap()),
        );
        assert!(matches!(acted.intent(), Intent::Act { .. }));
        assert_eq!(acted.constraints().min_confidence_value(), Some(0.55));

        let err = Constraints::min_confidence(f64::NAN).unwrap_err();
        assert_eq!(err, ProtocolError::NonFiniteConfidence);
        assert_eq!(err.to_string(), "confidence must be finite");
        let err = ExpectedOutcome::text_present("...").unwrap_err();
        assert_eq!(err, ProtocolError::EmptyExpectedText);
        assert_eq!(
            err.to_string(),
            "expected text must contain at least one alphanumeric token"
        );

        let refused = ComputerResult::refused(
            MatcherConfidence::try_new(0.49).unwrap(),
            FallbackReason::LowConfidence,
        );
        assert!(!refused.executed());
        assert!(!refused.verified());
        assert_eq!(refused.fallback(), Some(FallbackReason::LowConfidence));
        assert!(refused.target().is_none());
        assert!(refused.action().is_none());
        assert_eq!(refused.confidence().get(), 0.49);
        assert!(refused.state_delta().is_empty());
    }

    #[test]
    fn fallback_reasons_have_stable_names() {
        assert_eq!(FallbackReason::LowConfidence.as_str(), "low-confidence");
        assert_eq!(FallbackReason::NotImplemented.as_str(), "not-implemented");
        assert_eq!(FallbackReason::VerifyFailed.as_str(), "verify-failed");
        assert_eq!(FallbackReason::Ambiguous.as_str(), "ambiguous");
        assert_eq!(FallbackReason::Ambiguous.to_string(), "ambiguous");
        assert_eq!(FallbackReason::NoEffect.as_str(), "no-effect");
        assert_eq!(FallbackReason::NoEffect.to_string(), "no-effect");
    }
}
