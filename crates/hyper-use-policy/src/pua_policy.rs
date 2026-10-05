//! Default deterministic browser policy backed by PUA.

use std::collections::BTreeMap;

use hyper_use_core::{ActionId, ActionKind, ActionSpace, ObservedAction};
use pua_core::{decide, Answer, CandidateId, CandidateSet, Confidence, Profile, Scores};

use crate::evidence::{score_action, score_operation};
use crate::goal::AgentGoal;
use crate::types::{
    BrowserPolicy, HistoryEntry, PolicyDecision, PolicyError, PolicyOutcome, RankedAction,
};

/// PUA-backed finite policy. Hard-invalid targets are already absent from
/// [`ActionSpace`]; this policy never applies occlusion/disabled as scores.
#[derive(Clone, Debug)]
pub struct PuaPolicy {
    profile: Profile,
}

impl Default for PuaPolicy {
    fn default() -> Self {
        Self {
            profile: Profile::Standard,
        }
    }
}

impl PuaPolicy {
    pub fn new(profile: Profile) -> Self {
        Self { profile }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }
}

impl BrowserPolicy for PuaPolicy {
    fn decide(
        &mut self,
        space: &ActionSpace,
        goal: &AgentGoal,
        history: &[HistoryEntry],
    ) -> Result<PolicyOutcome, PolicyError> {
        if goal.is_empty() {
            return Err(PolicyError::EmptyGoal);
        }
        if space.is_empty() {
            return Err(PolicyError::EmptyActionSpace);
        }

        let goal_text = goal.as_str();

        // --- Operation head -------------------------------------------------
        let kinds = offered_kinds(space);
        let op_scores: BTreeMap<ActionKind, Confidence> = kinds
            .iter()
            .copied()
            .map(|kind| {
                let best = space
                    .targets_of(kind)
                    .max_by_key(|a| score_action(goal_text, a).get());
                (kind, score_operation(goal_text, kind, best))
            })
            .collect();

        let (op_answer, op_ranked) = choose_kinds(&kinds, &op_scores, self.profile)?;
        let Some(chosen_kind) = op_answer else {
            return Ok(PolicyOutcome::Abstain {
                reason: "operation abstain".to_owned(),
                operation_ranked: op_ranked,
                target_ranked: Vec::new(),
            });
        };

        if chosen_kind.is_control() {
            let id = ActionId::try_new(chosen_kind.as_str())
                .map_err(|e| PolicyError::Internal(e.to_string()))?;
            let action = space.get(&id).ok_or_else(|| {
                PolicyError::Internal(format!("control `{chosen_kind}` missing from space"))
            })?;
            let conf = op_scores
                .get(&chosen_kind)
                .copied()
                .unwrap_or(Confidence::ZERO);
            // A page control (scroll / wait) that just ran with a verified
            // effect satisfied a single-intent goal: DONE, not a loop.
            if !chosen_kind_is_terminal(chosen_kind) && satisfied_by_history(history, action.id()) {
                if let Some(done) = space.get_str(ActionKind::Done.as_str()) {
                    return Ok(PolicyOutcome::Choice(PolicyDecision {
                        action_id: done.id().clone(),
                        kind: ActionKind::Done,
                        target_label: done.label().to_owned(),
                        confidence_millis: Confidence::MAX.get(),
                        operation_ranked: op_ranked,
                        target_ranked: Vec::new(),
                    }));
                }
            }
            return Ok(PolicyOutcome::Choice(PolicyDecision {
                action_id: action.id().clone(),
                kind: chosen_kind,
                target_label: action.label().to_owned(),
                confidence_millis: conf.get(),
                operation_ranked: op_ranked,
                target_ranked: Vec::new(),
            }));
        }

        // --- Target head (same observation; only this kind's targets) -------
        let targets: Vec<&ObservedAction> = space.targets_of(chosen_kind).collect();
        if targets.is_empty() {
            return Ok(PolicyOutcome::Abstain {
                reason: format!("no viable targets for {chosen_kind}"),
                operation_ranked: op_ranked,
                target_ranked: Vec::new(),
            });
        }

        let target_scores: Vec<(&ObservedAction, Confidence)> = targets
            .iter()
            .map(|a| (*a, score_action(goal_text, a)))
            .collect();

        let (chosen, target_ranked) = choose_targets(&target_scores, self.profile)?;

        let Some(action) = chosen else {
            return Ok(PolicyOutcome::Abstain {
                reason: "target abstain".to_owned(),
                operation_ranked: op_ranked,
                target_ranked,
            });
        };

        let conf = target_scores
            .iter()
            .find(|(a, _)| a.id() == action.id())
            .map(|(_, c)| *c)
            .unwrap_or(Confidence::ZERO);

        // Repeated-action avoidance: the winner is exactly the action that
        // just executed with a verified effect. The goal's single intent is
        // satisfied; repeating it would double-submit. Choose DONE instead.
        if satisfied_by_history(history, action.id()) {
            if let Some(done) = space.get_str(ActionKind::Done.as_str()) {
                return Ok(PolicyOutcome::Choice(PolicyDecision {
                    action_id: done.id().clone(),
                    kind: ActionKind::Done,
                    target_label: done.label().to_owned(),
                    confidence_millis: Confidence::MAX.get(),
                    operation_ranked: op_ranked,
                    target_ranked,
                }));
            }
        }

        Ok(PolicyOutcome::Choice(PolicyDecision {
            action_id: action.id().clone(),
            kind: action.kind(),
            target_label: action.label().to_owned(),
            confidence_millis: conf.get(),
            operation_ranked: op_ranked,
            target_ranked,
        }))
    }
}

fn chosen_kind_is_terminal(kind: ActionKind) -> bool {
    matches!(kind, ActionKind::Done | ActionKind::Blocked)
}

/// The last executed step was `id` and verification saw a real effect.
fn satisfied_by_history(history: &[HistoryEntry], id: &ActionId) -> bool {
    history.last().is_some_and(|entry| {
        &entry.action_id == id
            && matches!(
                entry.verification.as_str(),
                "success" | "state-changed" | "navigation"
            )
    })
}

fn offered_kinds(space: &ActionSpace) -> Vec<ActionKind> {
    let mut kinds = Vec::new();
    for kind in [
        ActionKind::Click,
        ActionKind::TypeText,
        ActionKind::Select,
        ActionKind::ScrollUp,
        ActionKind::ScrollDown,
        ActionKind::Wait,
        ActionKind::Done,
        ActionKind::Blocked,
    ] {
        if space.contains_kind(kind) {
            // Target-bound kinds need at least one target; controls always ok.
            if kind.is_control() || space.targets_of(kind).next().is_some() {
                kinds.push(kind);
            }
        }
    }
    kinds
}

/// Returns `(Some(kind), ranked)` on choice, `(None, ranked)` on abstain.
fn choose_kinds(
    kinds: &[ActionKind],
    scores: &BTreeMap<ActionKind, Confidence>,
    profile: Profile,
) -> Result<(Option<ActionKind>, Vec<RankedAction>), PolicyError> {
    if kinds.is_empty() {
        return Ok((None, Vec::new()));
    }
    if kinds.len() == 1 {
        let kind = kinds[0];
        let conf = scores.get(&kind).copied().unwrap_or(Confidence::ZERO);
        let ranked = vec![ranked_kind(kind, conf)];
        if conf.get() < profile.thresholds().min_confidence.get() {
            return Ok((None, ranked));
        }
        return Ok((Some(kind), ranked));
    }

    let ids: Result<Vec<_>, _> = kinds
        .iter()
        .map(|k| CandidateId::new(k.as_str()).map_err(|e| PolicyError::Internal(e.to_string())))
        .collect();
    let set = CandidateSet::new(ids?).map_err(|e| PolicyError::Internal(e.to_string()))?;
    let question = set
        .question("operation")
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let mut pua_scores = Scores::new(&question);
    for kind in kinds {
        let Some(idx) = set.index_of(kind.as_str()) else {
            continue;
        };
        let conf = scores.get(kind).copied().unwrap_or(Confidence::ZERO);
        pua_scores
            .set(idx, conf)
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
    }
    let answer = decide(&pua_scores, profile);
    let ranked = ranked_from_answer_kinds(&set, &answer, scores);
    match answer {
        Answer::Choice { option, .. } => {
            let id = set
                .id(option)
                .ok_or_else(|| PolicyError::Internal("missing chosen kind".into()))?;
            let kind = ActionKind::parse(id.as_str())
                .ok_or_else(|| PolicyError::Internal(format!("bad kind {}", id.as_str())))?;
            Ok((Some(kind), ranked))
        }
        Answer::Abstain { .. } => Ok((None, ranked)),
        other => Err(PolicyError::Internal(format!(
            "unexpected operation answer {other:?}"
        ))),
    }
}

fn choose_targets<'a>(
    scored: &[(&'a ObservedAction, Confidence)],
    profile: Profile,
) -> Result<(Option<&'a ObservedAction>, Vec<RankedAction>), PolicyError> {
    if scored.is_empty() {
        return Ok((None, Vec::new()));
    }
    if scored.len() == 1 {
        let (action, conf) = scored[0];
        let ranked = vec![ranked_action(action, conf)];
        if conf.get() < profile.thresholds().min_confidence.get() {
            return Ok((None, ranked));
        }
        return Ok((Some(action), ranked));
    }

    let ids: Result<Vec<_>, _> = scored
        .iter()
        .map(|(a, _)| {
            CandidateId::new(a.id().as_str()).map_err(|e| PolicyError::Internal(e.to_string()))
        })
        .collect();
    let set = CandidateSet::new(ids?).map_err(|e| PolicyError::Internal(e.to_string()))?;
    let question = set
        .question("target")
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let mut pua_scores = Scores::new(&question);
    for (action, conf) in scored {
        let Some(idx) = set.index_of(action.id().as_str()) else {
            continue;
        };
        pua_scores
            .set(idx, *conf)
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
    }
    let answer = decide(&pua_scores, profile);
    let ranked = ranked_from_answer_targets(&set, &answer, scored);
    match &answer {
        Answer::Choice { option, .. } => {
            let id = set
                .id(*option)
                .ok_or_else(|| PolicyError::Internal("missing chosen target".into()))?;
            let action = scored
                .iter()
                .map(|(a, _)| *a)
                .find(|a| a.id().as_str() == id.as_str())
                .ok_or_else(|| PolicyError::Internal("chosen target not in space".into()))?;
            Ok((Some(action), ranked))
        }
        Answer::Abstain { .. } => Ok((None, ranked)),
        other => Err(PolicyError::Internal(format!(
            "unexpected target answer {other:?}"
        ))),
    }
}

fn ranked_kind(kind: ActionKind, conf: Confidence) -> RankedAction {
    RankedAction {
        id: ActionId::try_new(kind.as_str())
            .unwrap_or_else(|_| ActionId::try_new("WAIT").expect("WAIT")),
        kind,
        label: kind.as_str().to_owned(),
        confidence_millis: conf.get(),
    }
}

fn ranked_action(action: &ObservedAction, conf: Confidence) -> RankedAction {
    RankedAction {
        id: action.id().clone(),
        kind: action.kind(),
        label: action.label().to_owned(),
        confidence_millis: conf.get(),
    }
}

fn ranked_from_answer_kinds(
    set: &CandidateSet,
    answer: &Answer,
    scores: &BTreeMap<ActionKind, Confidence>,
) -> Vec<RankedAction> {
    let entries = match answer {
        Answer::Choice { ranked, .. } | Answer::Abstain { ranked, .. } => ranked.entries(),
        _ => return Vec::new(),
    };
    entries
        .iter()
        .filter_map(|(idx, ranked_conf)| {
            let id = set.id(*idx)?;
            let kind = ActionKind::parse(id.as_str())?;
            let conf = scores.get(&kind).copied().unwrap_or(*ranked_conf);
            Some(ranked_kind(kind, conf))
        })
        .collect()
}

fn ranked_from_answer_targets(
    set: &CandidateSet,
    answer: &Answer,
    scored: &[(&ObservedAction, Confidence)],
) -> Vec<RankedAction> {
    let entries = match answer {
        Answer::Choice { ranked, .. } | Answer::Abstain { ranked, .. } => ranked.entries(),
        _ => return Vec::new(),
    };
    entries
        .iter()
        .filter_map(|(idx, _conf)| {
            let id = set.id(*idx)?;
            let (action, c) = scored
                .iter()
                .find(|(a, _)| a.id().as_str() == id.as_str())?;
            Some(ranked_action(action, *c))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::parse_fixture;

    fn space_from(fixture: &str) -> ActionSpace {
        let m = parse_fixture(fixture).unwrap();
        ActionSpace::from_manifold(&m)
    }

    #[test]
    fn exact_label_wins_click() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=signin role=button label="Sign in" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=cancel role=button label="Cancel" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::default();
        let outcome = policy
            .decide(&space, &AgentGoal::new("Sign in"), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Sign in");
        assert_eq!(choice.action_id.as_str(), "CLICK:signin");
        assert!(choice.confidence_millis >= 750);
    }

    #[test]
    fn verified_repeat_of_last_action_becomes_done() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=q role=text_field label="Search" x=10 y=10 w=200 h=24 actions=click,type sources=dom,accessibility
            "#,
        );
        let goal = AgentGoal::new(r#"Type "rust" into Search"#);
        let mut policy = PuaPolicy::default();
        let first = policy.decide(&space, &goal, &[]).unwrap();
        let first = first
            .as_choice()
            .unwrap_or_else(|| panic!("{first:?}"))
            .clone();
        assert_eq!(first.kind, ActionKind::TypeText);
        let entry = |verification: &str| HistoryEntry {
            step: 1,
            action_id: first.action_id.clone(),
            kind: first.kind,
            label: first.target_label.clone(),
            verification: verification.to_owned(),
        };
        let done = policy.decide(&space, &goal, &[entry("success")]).unwrap();
        assert_eq!(done.as_choice().unwrap().kind, ActionKind::Done);
        // No verified effect: not satisfied, repeat stays the choice.
        let again = policy.decide(&space, &goal, &[entry("no-effect")]).unwrap();
        assert_eq!(again.as_choice().unwrap().kind, ActionKind::TypeText);
    }

    #[test]
    fn exact_tie_of_twin_labels_abstains() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=a role=button label="Delete" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=b role=button label="Delete" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::default();
        let outcome = policy
            .decide(&space, &AgentGoal::new("Delete"), &[])
            .unwrap();
        match outcome {
            PolicyOutcome::Abstain {
                reason,
                target_ranked,
                ..
            } => {
                assert!(
                    reason.contains("abstain") || reason.contains("target"),
                    "{reason}"
                );
                assert!(target_ranked.len() >= 2, "{target_ranked:?}");
                // Exact tie: both at MAX → margin 0 → abstain.
                assert_eq!(
                    target_ranked[0].confidence_millis,
                    target_ranked[1].confidence_millis
                );
            }
            PolicyOutcome::Choice(c) => panic!("expected abstain, got {c:?}"),
        }
    }

    #[test]
    fn low_margin_near_twins_abstains_under_fast_profile() {
        // Fast profile needs margin 200. Overlap-only near-ties should abstain.
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=a role=button label="Send feedback" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=b role=button label="Send to device" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::new(Profile::Fast);
        let outcome = policy.decide(&space, &AgentGoal::new("Send"), &[]).unwrap();
        assert!(
            matches!(outcome, PolicyOutcome::Abstain { .. }),
            "expected abstain for ambiguous Send*, got {outcome:?}"
        );
    }

    #[test]
    fn done_control_when_goal_is_done() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=ok role=button label="OK" x=10 y=10 w=40 h=20 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::default();
        let outcome = policy.decide(&space, &AgentGoal::new("DONE"), &[]).unwrap();
        let choice = outcome.as_choice().expect("choice");
        assert_eq!(choice.kind, ActionKind::Done);
    }

    #[test]
    fn empty_goal_is_error_not_silent_top() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=ok role=button label="OK" x=10 y=10 w=40 h=20 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::default();
        assert_eq!(
            policy.decide(&space, &AgentGoal::new("  "), &[]),
            Err(PolicyError::EmptyGoal)
        );
    }

    #[test]
    fn candidate_order_does_not_change_choice() {
        // Same labels different region id insertion order in fixture — ActionSpace is BTree ordered.
        let a = space_from(
            r#"
            viewport w=800 h=600
            region id=z role=button label="UniqueSave" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=a role=button label="Cancel" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let b = space_from(
            r#"
            viewport w=800 h=600
            region id=a role=button label="Cancel" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            region id=z role=button label="UniqueSave" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = PuaPolicy::default();
        let ca = policy
            .decide(&a, &AgentGoal::new("UniqueSave"), &[])
            .unwrap()
            .as_choice()
            .unwrap()
            .action_id
            .clone();
        let cb = policy
            .decide(&b, &AgentGoal::new("UniqueSave"), &[])
            .unwrap()
            .as_choice()
            .unwrap()
            .action_id
            .clone();
        assert_eq!(ca.as_str(), "CLICK:z");
        assert_eq!(ca, cb);
    }
}
