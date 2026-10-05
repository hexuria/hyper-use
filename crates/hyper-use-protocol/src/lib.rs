//! Loop messages for hyper-use.
//!
//! Product operations are observe, guard, verify. Hyper-Use does not click and
//! does not plan navigation. Locate / inspect / diff remain internal helpers.
//!
//! [`GuardDecision`] is the product contract. Legacy [`ComputerTask`] /
//! [`ComputerResult`] types remain for transitional hosts and are not the
//! recommended surface.

#![forbid(unsafe_code)]

use hyper_use_core::{Action, LocateQuery, RegionId};

mod contract;
mod guard;

pub use contract::{
    ComputerResult, ComputerTask, Constraints, ExpectedOutcome, FallbackReason, Intent,
    MatcherConfidence, ProtocolError, ReportedExecutor, StateDelta,
};
pub use guard::{
    FirewallPhase, GuardCandidate, GuardDecision, GuardEvidence, GuardReason, FIREWALL_ORDER,
};

/// Legacy six-phase loop. Prefer [`FirewallPhase`] / [`FIREWALL_ORDER`].
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
    Inspect {
        region_id: RegionId,
    },
    /// Legacy. Prefer guard; Hyper-Use does not click on the product path.
    Act {
        region_id: RegionId,
        action: Action,
    },
    Guard {
        query: LocateQuery,
    },
    Verify {
        region_id: RegionId,
    },
    Diff,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::Role;

    #[test]
    fn firewall_order_is_observe_guard_verify() {
        let names: Vec<_> = FIREWALL_ORDER.iter().map(|p| p.as_str()).collect();
        assert_eq!(names, ["observe", "guard", "verify"]);
    }

    #[test]
    fn legacy_loop_order_still_names_six_verbs() {
        let names: Vec<_> = LOOP_ORDER.iter().map(|phase| phase.as_str()).collect();
        assert_eq!(
            names,
            ["observe", "locate", "inspect", "act", "diff", "verify"]
        );
    }

    #[test]
    fn guard_reasons_have_stable_names() {
        assert_eq!(GuardReason::LowConfidence.as_str(), "low-confidence");
        assert_eq!(GuardReason::Ambiguous.as_str(), "ambiguous");
        assert_eq!(GuardReason::MissingTarget.as_str(), "missing-target");
        assert_eq!(GuardReason::Disabled.as_str(), "disabled");
        assert_eq!(GuardReason::FrontLayer.as_str(), "front-layer");
        assert_eq!(GuardReason::WorldChanged.as_str(), "world-changed");
    }

    #[test]
    fn computer_task_is_locate_or_act_and_refusal_does_not_claim_execution() {
        let task = ComputerTask::locate(
            LocateQuery::new().text("Sign in").unwrap(),
            Constraints::none(),
            ExpectedOutcome::text_present("Welcome").unwrap(),
        );
        assert!(matches!(task.intent(), Intent::Locate(_)));
        let refused = ComputerResult::refused(
            MatcherConfidence::try_new(0.49).unwrap(),
            FallbackReason::LowConfidence,
        );
        assert!(!refused.executed());
        assert_eq!(refused.fallback(), Some(FallbackReason::LowConfidence));
    }

    #[test]
    fn unit_confidence_rejects_outside_zero_to_one_exactly() {
        assert_eq!(MatcherConfidence::try_unit(0.0).unwrap().get(), 0.0);
        assert_eq!(
            MatcherConfidence::try_unit(1.000_001).unwrap_err(),
            ProtocolError::ConfidenceOutOfRange
        );
        assert_eq!(MatcherConfidence::try_new(-0.2).unwrap().get(), -0.2);
    }

    #[test]
    fn request_guard_carries_a_query() {
        let request = Request::Guard {
            query: LocateQuery::new()
                .text("Settings")
                .unwrap()
                .role(Role::Button),
        };
        assert!(matches!(request, Request::Guard { .. }));
    }
}
