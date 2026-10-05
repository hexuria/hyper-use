//! Action firewall decisions for hyper-use.
//!
//! Hyper-Use does not click. A host proposes a target; this crate ranks,
//! gates, and returns [`GuardDecision`]. Browser Use (or another executor)
//! performs the trusted action only after [`GuardDecision::Allow`].

#![forbid(unsafe_code)]

use std::fmt;

use hyper_use_core::{Action, InteractionManifold, InteractionRegion, LocateQuery, RegionId};
use hyper_use_protocol::MatcherConfidence;
use hyper_use_resonance::{default_matcher, Match, RegionMatcher, RegionState, TEXT_MISS_CAP};

pub use hyper_use_protocol::{GuardCandidate, GuardDecision, GuardEvidence, GuardReason};

/// Raw confidence below this never allows. Not a probability.
pub const MIN_ALLOW_CONFIDENCE: f64 = 0.55;

/// Minimum raw gap between top and runner-up. Below this → ambiguous.
pub const MIN_ALLOW_MARGIN: f64 = 0.05;

/// Tolerance on the margin only (`0.6 - 0.55` is not exact in f64).
pub const MARGIN_EPSILON: f64 = 1e-9;

pub const MIN_ALLOW_CONFIDENCE_MILLIS: i32 = 550;
pub const MIN_ALLOW_MARGIN_MILLIS: i32 = 50;

const _: () = assert!(TEXT_MISS_CAP < MIN_ALLOW_CONFIDENCE);

/// What the host asked to do. Click is the only action the guard evaluates today.
#[derive(Clone, Debug, PartialEq)]
pub struct GuardRequest {
    action: Action,
    query: LocateQuery,
    /// Optional host-proposed region id. When set, it must be the top match.
    proposed: Option<RegionId>,
}

impl GuardRequest {
    pub fn click(query: LocateQuery) -> Self {
        Self {
            action: Action::Click,
            query,
            proposed: None,
        }
    }

    pub fn proposed(mut self, id: RegionId) -> Self {
        self.proposed = Some(id);
        self
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn query(&self) -> &LocateQuery {
        &self.query
    }

    pub fn proposed_id(&self) -> Option<&RegionId> {
        self.proposed.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GuardError {
    Rank(hyper_use_resonance::ResonanceError),
    Confidence(hyper_use_protocol::ProtocolError),
    UnsupportedAction(Action),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rank(err) => write!(f, "{err}"),
            Self::Confidence(err) => write!(f, "{err}"),
            Self::UnsupportedAction(action) => {
                write!(f, "guard does not evaluate action `{action}`")
            }
        }
    }
}

impl std::error::Error for GuardError {}

impl From<hyper_use_resonance::ResonanceError> for GuardError {
    fn from(value: hyper_use_resonance::ResonanceError) -> Self {
        Self::Rank(value)
    }
}

impl From<hyper_use_protocol::ProtocolError> for GuardError {
    fn from(value: hyper_use_protocol::ProtocolError) -> Self {
        Self::Confidence(value)
    }
}

/// Rank with the default matcher and decide allow / refuse / escalate.
pub fn guard(
    manifold: &InteractionManifold,
    request: &GuardRequest,
) -> Result<GuardDecision, GuardError> {
    guard_with(manifold, request, &default_matcher())
}

/// Rank with a caller-supplied matcher and decide.
pub fn guard_with<M: RegionMatcher>(
    manifold: &InteractionManifold,
    request: &GuardRequest,
    matcher: &M,
) -> Result<GuardDecision, GuardError> {
    if request.action != Action::Click {
        return Err(GuardError::UnsupportedAction(request.action));
    }
    let ranked = matcher.rank(request.query(), manifold)?;
    decide(manifold, request, &ranked)
}

fn decide(
    manifold: &InteractionManifold,
    request: &GuardRequest,
    ranked: &[Match],
) -> Result<GuardDecision, GuardError> {
    let candidates: Vec<GuardCandidate> = ranked
        .iter()
        .take(5)
        .filter_map(|m| candidate_from(manifold, m))
        .collect();

    if ranked.is_empty() || candidates.is_empty() {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::MissingTarget,
            candidates,
        });
    }

    let top = &ranked[0];
    let top_conf = top.confidence();

    if let Some(proposed) = request.proposed_id() {
        if top.id() != proposed {
            return Ok(GuardDecision::Refuse {
                reason: GuardReason::ProposedNotTop,
                candidates,
            });
        }
    }

    let top_region = manifold
        .regions()
        .find(|r| r.id() == top.id())
        .expect("ranked id comes from manifold");

    let evidence = evidence_of(manifold.viewport(), top_region);
    if !evidence.enabled {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::Disabled,
            candidates,
        });
    }
    if evidence.hidden {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::Hidden,
            candidates,
        });
    }
    if evidence.occluded {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::Occluded,
            candidates,
        });
    }
    if evidence.offscreen {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::Offscreen,
            candidates,
        });
    }

    if top_conf < MIN_ALLOW_CONFIDENCE {
        return Ok(GuardDecision::Refuse {
            reason: GuardReason::LowConfidence,
            candidates,
        });
    }

    let runner_up = ranked.get(1).map(|m| m.confidence());
    if let Some(second) = runner_up {
        let margin = top_conf - second;
        if !margin.is_finite() || margin < MIN_ALLOW_MARGIN - MARGIN_EPSILON {
            return Ok(GuardDecision::Escalate {
                reason: GuardReason::Ambiguous,
                candidates,
            });
        }
        return Ok(GuardDecision::Allow {
            target: candidates[0].clone(),
            confidence: MatcherConfidence::try_new(top_conf)?,
            margin: Some(MatcherConfidence::try_new(margin)?),
            evidence,
        });
    }

    Ok(GuardDecision::Allow {
        target: candidates[0].clone(),
        confidence: MatcherConfidence::try_new(top_conf)?,
        margin: None,
        evidence,
    })
}

fn candidate_from(manifold: &InteractionManifold, m: &Match) -> Option<GuardCandidate> {
    let region = manifold.regions().find(|r| r.id() == m.id())?;
    Some(GuardCandidate {
        id: region.id().clone(),
        role: region.role(),
        label: region.label().to_owned(),
        confidence: m.confidence(),
    })
}

fn evidence_of(viewport: hyper_use_core::Rect, region: &InteractionRegion) -> GuardEvidence {
    let state = RegionState::of(viewport, region);
    use hyper_use_resonance::{Availability, Visibility};
    GuardEvidence {
        visible: state.visibility() == Visibility::Visible,
        enabled: state.availability() == Availability::Enabled,
        occluded: state.visibility() == Visibility::Occluded,
        hidden: state.visibility() == Visibility::Hidden,
        offscreen: state.visibility() == Visibility::Offscreen,
        role: region.role(),
    }
}

/// Display helper: raw confidence as truncated millis.
pub fn display_millis(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    let scaled = (value * 1000.0).trunc();
    if scaled > f64::from(i32::MAX) {
        i32::MAX
    } else if scaled < f64::from(i32::MIN) {
        i32::MIN
    } else {
        scaled as i32
    }
}

pub fn margin_millis(top: f64, runner_up: f64) -> i32 {
    display_millis(top - runner_up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::{Action, Rect, RegionFlags, RegionParts, Role, SourceMask, UnitInterval};

    fn region(id: &str, label: &str, x: f64, flags: &str) -> InteractionRegion {
        InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new(id).unwrap(),
            role: Role::Button,
            label: label.into(),
            rect: Rect::try_new(x, 10.0, 80.0, 24.0).unwrap(),
            actions: vec![Action::Click, Action::Focus],
            parent: None,
            sources: SourceMask::DOM.union(SourceMask::ACCESSIBILITY),
            flags: RegionFlags::parse_list(flags).unwrap(),
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap()
    }

    fn manifold(regions: Vec<InteractionRegion>) -> InteractionManifold {
        InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 800.0, 600.0).unwrap(),
            regions,
            0,
        )
        .unwrap()
    }

    #[test]
    fn allows_a_clear_sign_in_button() {
        let m = manifold(vec![region("n1", "Sign in", 100.0, "")]);
        let req = GuardRequest::click(
            LocateQuery::new()
                .text("Sign in")
                .unwrap()
                .role(Role::Button),
        );
        let decision = guard(&m, &req).unwrap();
        assert!(
            matches!(decision, GuardDecision::Allow { .. }),
            "{decision:?}"
        );
    }

    #[test]
    fn refuses_low_confidence_text_miss() {
        let m = manifold(vec![region("n1", "Cancel", 100.0, "")]);
        let req = GuardRequest::click(
            LocateQuery::new()
                .text("Sign in")
                .unwrap()
                .role(Role::Button),
        );
        let decision = guard(&m, &req).unwrap();
        match decision {
            GuardDecision::Refuse {
                reason: GuardReason::LowConfidence | GuardReason::MissingTarget,
                ..
            } => {}
            other => panic!("expected refuse low/missing, got {other:?}"),
        }
    }

    #[test]
    fn escalates_identical_twin_buttons() {
        let m = manifold(vec![
            region("n1", "Send", 100.0, ""),
            region("n2", "Send", 400.0, ""),
        ]);
        let req = GuardRequest::click(LocateQuery::new().text("Send").unwrap().role(Role::Button));
        let decision = guard(&m, &req).unwrap();
        match decision {
            GuardDecision::Escalate {
                reason: GuardReason::Ambiguous,
                candidates,
            } => assert!(candidates.len() >= 2),
            other => panic!("expected escalate ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn refuses_disabled_target() {
        let m = manifold(vec![region("n1", "Save", 100.0, "disabled")]);
        let req = GuardRequest::click(LocateQuery::new().text("Save").unwrap().role(Role::Button));
        let decision = guard(&m, &req).unwrap();
        match decision {
            GuardDecision::Refuse {
                reason: GuardReason::Disabled,
                ..
            } => {}
            other => panic!("expected refuse disabled, got {other:?}"),
        }
    }

    #[test]
    fn refuses_when_proposed_is_not_top() {
        let m = manifold(vec![
            region("n1", "Sign in", 100.0, ""),
            region("n2", "Cancel", 400.0, ""),
        ]);
        let req = GuardRequest::click(
            LocateQuery::new()
                .text("Sign in")
                .unwrap()
                .role(Role::Button),
        )
        .proposed(RegionId::try_new("n2").unwrap());
        let decision = guard(&m, &req).unwrap();
        match decision {
            GuardDecision::Refuse {
                reason: GuardReason::ProposedNotTop,
                ..
            } => {}
            other => panic!("expected proposed-not-top, got {other:?}"),
        }
    }
}
