//! Loop messages for hyper-use.
//!
//! The only legal order is observe, locate, inspect when the locate result is
//! ambiguous, act, then verify. This crate names that order. It does not run
//! it. Coordinates are not a message: an act names a [`RegionId`].

#![forbid(unsafe_code)]

use hyper_use_core::{Action, LocateQuery, RegionId};

/// Phases of one hyper-use turn. Declaration order is the loop order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoopPhase {
    Observe,
    Locate,
    Inspect,
    Act,
    Verify,
}

pub const LOOP_ORDER: [LoopPhase; 5] = [
    LoopPhase::Observe,
    LoopPhase::Locate,
    LoopPhase::Inspect,
    LoopPhase::Act,
    LoopPhase::Verify,
];

impl LoopPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Locate => "locate",
            Self::Inspect => "inspect",
            Self::Act => "act",
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::Role;

    #[test]
    fn loop_order_is_observe_locate_inspect_act_verify() {
        let names: Vec<_> = LOOP_ORDER.iter().map(|phase| phase.as_str()).collect();
        assert_eq!(names, ["observe", "locate", "inspect", "act", "verify"]);
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
    }
}
