//! Default deterministic browser policy backed by Instinct.

use std::collections::BTreeMap;

use aui_core::{tokenize, ActionId, ActionKind, ActionSpace, ObservedAction};
use instinct_core::{
    arbitrate, Affinity, Answer, CandidateId, CandidateSet, Confidence, DriveIndex, Drives,
    Profile, Scores,
};

use crate::evidence::{missing_quoted_target, score_action, score_operation};
use crate::goal::AgentGoal;
use crate::text::{ground_select, label_literal, quoted_literals};
use crate::types::{
    BrowserPolicy, HistoryEntry, PolicyDecision, PolicyError, PolicyOutcome, RankedAction,
};

pub const HABITUATION_STEP: i16 = 250;
const FORM_FIRST_CAP: i16 = 800;
const MISSING_QUOTED_TARGET_CAP: i16 = 500;

/// Instinct-backed finite policy. Hard-invalid targets are already absent from
/// [`ActionSpace`]; this policy never applies occlusion/disabled as scores.
#[derive(Clone, Debug)]
pub struct InstinctPolicy {
    profile: Profile,
}

impl Default for InstinctPolicy {
    fn default() -> Self {
        Self {
            profile: Profile::Standard,
        }
    }
}

impl InstinctPolicy {
    pub fn new(profile: Profile) -> Self {
        Self { profile }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }
}

impl BrowserPolicy for InstinctPolicy {
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
        let missing_target = missing_quoted_target(goal_text, space);
        let form_first = if missing_target {
            None
        } else {
            named_unsatisfied_field(goal_text, space)
        };

        // --- Operation head -------------------------------------------------
        let kinds = offered_kinds(space);
        let mut op_scores = BTreeMap::new();
        let mut op_ids = BTreeMap::new();
        for kind in kinds.iter().copied() {
            let best = form_first
                .filter(|action| action.kind() == kind)
                .or_else(|| {
                    space
                        .targets_of(kind)
                        .max_by_key(|a| score_action(goal_text, a).get())
                });
            let id = if kind.is_control() {
                ActionId::try_new(kind.as_str())
                    .map_err(|e| PolicyError::Internal(e.to_string()))?
            } else {
                best.map(|action| action.id().clone())
                    .ok_or_else(|| PolicyError::Internal(format!("no target for {kind}")))?
            };
            let mut score = score_operation(goal_text, kind, best);
            if let Some(field) = form_first {
                if kind == field.kind() {
                    score = Confidence::MAX;
                } else {
                    score = cap_confidence(score, FORM_FIRST_CAP);
                }
            }
            if missing_target && !kind.is_control() {
                score = cap_confidence(score, MISSING_QUOTED_TARGET_CAP);
            }
            op_scores.insert(kind, score);
            op_ids.insert(kind, id);
        }

        let (op_answer, op_ranked) =
            choose_kinds(&kinds, &op_scores, &op_ids, history, self.profile)?;
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
            let confidence_millis = op_ranked
                .iter()
                .find(|ranked| ranked.kind == chosen_kind)
                .map(|ranked| ranked.confidence_millis)
                .ok_or_else(|| PolicyError::Internal("chosen control missing from ranks".into()))?;
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
                confidence_millis,
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
            .map(|action| {
                let mut score = score_action(goal_text, action);
                if let Some(field) = form_first {
                    if action.kind() == field.kind() {
                        if action.id() == field.id() {
                            score = Confidence::MAX;
                        } else {
                            score = cap_confidence(score, FORM_FIRST_CAP);
                        }
                    }
                }
                if missing_target {
                    score = cap_confidence(score, MISSING_QUOTED_TARGET_CAP);
                }
                (*action, score)
            })
            .collect();

        let (chosen, target_ranked) = choose_targets(&target_scores, history, self.profile)?;

        let Some(action) = chosen else {
            return Ok(PolicyOutcome::Abstain {
                reason: "target abstain".to_owned(),
                operation_ranked: op_ranked,
                target_ranked,
            });
        };

        let confidence_millis = target_ranked
            .iter()
            .find(|ranked| ranked.id == *action.id())
            .map(|ranked| ranked.confidence_millis)
            .ok_or_else(|| PolicyError::Internal("chosen target missing from ranks".into()))?;

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
            confidence_millis,
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

fn named_unsatisfied_field<'a>(goal: &str, space: &'a ActionSpace) -> Option<&'a ObservedAction> {
    let mut candidates = Vec::new();
    let type_actions: Vec<_> = space.targets_of(ActionKind::TypeText).collect();
    let select_actions: Vec<_> = space.targets_of(ActionKind::Select).collect();

    for action in type_actions.iter().copied() {
        let paired_select = select_actions
            .iter()
            .copied()
            .find(|select| select.target() == action.target());
        if paired_select.is_some_and(|select| !select.state().options.is_empty()) {
            continue;
        }

        let mut positions = Vec::new();
        if let Some((position, literal)) = label_literal(goal, action.label(), &[]) {
            if !value_matches_literal(action.state().value.as_deref(), &literal) {
                positions.push(position);
            }
        }
        if let Some(position) = explicit_field_position(goal, action.label(), action.kind()) {
            positions.push(position);
        }
        if let Some(position) = positions.into_iter().min() {
            candidates.push((position, action));
        }
    }

    for action in select_actions.iter().copied() {
        let paired_type = type_actions
            .iter()
            .copied()
            .any(|typed| typed.target() == action.target());
        if paired_type && action.state().options.is_empty() {
            continue;
        }

        let mut positions = Vec::new();
        if let Some((position, literal)) = label_literal(goal, action.label(), &[]) {
            if !value_matches_literal(action.state().selected.as_deref(), &literal) {
                positions.push(position);
            }
        }
        if let Some(position) = explicit_field_position(goal, action.label(), action.kind()) {
            positions.push(position);
        }
        if !action.state().options.is_empty() {
            if let Ok(option) = ground_select(
                goal,
                None,
                &action.state().options,
                action.state().selected.as_deref(),
            ) {
                if !value_matches_literal(action.state().selected.as_deref(), &option) {
                    if let Some(position) = phrase_position(goal, &option) {
                        positions.push(position);
                    }
                }
            }
        }
        if let Some(position) = positions.into_iter().min() {
            candidates.push((position, action));
        }
    }

    let earliest_position = candidates.iter().map(|(position, _)| *position).min()?;
    let mut earliest = candidates
        .into_iter()
        .filter(|(position, _)| *position == earliest_position);
    let (_, candidate) = earliest.next()?;
    if earliest.next().is_some() {
        None
    } else {
        Some(candidate)
    }
}

fn explicit_field_position(goal: &str, label: &str, kind: ActionKind) -> Option<usize> {
    if score_operation(goal, kind, None).get() == 0 {
        return None;
    }

    let quoted = quoted_literals(goal);
    let mut offset = 0;
    while offset < goal.len() {
        let position = offset + phrase_position(&goal[offset..], label)?;
        if !position_is_quoted(goal, position, &quoted) {
            let prefix = tokenize(&goal[..position]);
            let field_preposition = prefix
                .last()
                .is_some_and(|word| matches!(word.as_str(), "in" | "into"))
                || (prefix.len() >= 2
                    && matches!(prefix[prefix.len() - 2].as_str(), "in" | "into")
                    && matches!(prefix.last().map(String::as_str), Some("the" | "a" | "an")));
            if field_preposition {
                return Some(position);
            }
        }
        offset = goal[position..]
            .char_indices()
            .nth(1)
            .map_or(goal.len(), |(relative, _)| position + relative);
    }
    None
}

fn position_is_quoted(goal: &str, position: usize, quoted: &[(usize, String)]) -> bool {
    quoted.iter().any(|(opening_quote, literal)| {
        let Some(opening) = goal
            .get(*opening_quote..)
            .and_then(|suffix| suffix.chars().next())
        else {
            return false;
        };
        let content_start = opening_quote + opening.len_utf8();
        position >= content_start && position < content_start + literal.len()
    })
}

fn value_matches_literal(value: Option<&str>, literal: &str) -> bool {
    value.is_some_and(|value| value.trim().to_lowercase() == literal.trim().to_lowercase())
}

fn phrase_position(goal: &str, phrase: &str) -> Option<usize> {
    let phrase = phrase.trim().to_lowercase();
    if phrase.is_empty() {
        return None;
    }
    for (start, _) in goal.char_indices() {
        if !phrase_boundary_before(goal, start) {
            continue;
        }
        let mut normalized = String::new();
        for (offset, character) in goal[start..].char_indices() {
            normalized.extend(character.to_lowercase());
            let end = start + offset + character.len_utf8();
            if normalized == phrase {
                if phrase_boundary_after(goal, end) {
                    return Some(start);
                }
                break;
            }
            if !phrase.starts_with(&normalized) {
                break;
            }
        }
    }
    None
}

fn phrase_boundary_before(text: &str, position: usize) -> bool {
    text[..position]
        .chars()
        .next_back()
        .is_none_or(|character| !character.is_alphanumeric())
}

fn phrase_boundary_after(text: &str, position: usize) -> bool {
    text[position..]
        .chars()
        .next()
        .is_none_or(|character| !character.is_alphanumeric())
}

fn cap_confidence(score: Confidence, ceiling: i16) -> Confidence {
    Confidence::saturating(i32::from(score.get().min(ceiling)))
}

fn habituation_weight(id: &ActionId, history: &[HistoryEntry]) -> Confidence {
    let mut weight = 1000_i16;
    for entry in history.iter().rev() {
        if &entry.action_id != id
            || !matches!(entry.verification.as_str(), "no-effect" | "wrong-effect")
        {
            break;
        }
        weight = weight.saturating_sub(HABITUATION_STEP);
        if weight == 0 {
            break;
        }
    }
    Confidence::saturating(i32::from(weight))
}

fn damped_urge(evidence: Confidence, weight: Confidence) -> Confidence {
    Confidence::saturating(i32::from(evidence.get()) * i32::from(weight.get()) / 1000)
}

/// Returns `(Some(kind), ranked)` on choice, `(None, ranked)` on abstain.
fn choose_kinds(
    kinds: &[ActionKind],
    scores: &BTreeMap<ActionKind, Confidence>,
    action_ids: &BTreeMap<ActionKind, ActionId>,
    history: &[HistoryEntry],
    profile: Profile,
) -> Result<(Option<ActionKind>, Vec<RankedAction>), PolicyError> {
    if kinds.is_empty() {
        return Ok((None, Vec::new()));
    }
    if kinds.len() == 1 {
        let kind = kinds[0];
        let evidence = scores.get(&kind).copied().unwrap_or(Confidence::ZERO);
        let action_id = action_ids
            .get(&kind)
            .ok_or_else(|| PolicyError::Internal(format!("missing action id for {kind}")))?;
        let urge = damped_urge(evidence, habituation_weight(action_id, history));
        let ranked = vec![ranked_kind(kind, urge)?];
        if urge < profile.thresholds().min_confidence {
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
    let drives = Drives::new(&[("goal", Confidence::MAX)])
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let mut scores_by_option = Scores::new(&question);
    let mut affinity = Affinity::new(&question, &drives);
    for kind in kinds {
        let idx = set
            .index_of(kind.as_str())
            .ok_or_else(|| PolicyError::Internal(format!("missing operation option for {kind}")))?;
        let evidence = scores.get(kind).copied().unwrap_or(Confidence::ZERO);
        let action_id = action_ids
            .get(kind)
            .ok_or_else(|| PolicyError::Internal(format!("missing action id for {kind}")))?;
        scores_by_option
            .set(idx, evidence)
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
        affinity
            .set(
                DriveIndex::new(0),
                idx,
                habituation_weight(action_id, history),
            )
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
    }
    let arbitration = arbitrate(&scores_by_option, &drives, &affinity, None, profile)
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let ranked = ranked_from_answer_kinds(&set, arbitration.answer(), arbitration.urges())?;
    match arbitration.answer() {
        Answer::Choice { option, .. } => {
            let id = set
                .id(*option)
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
    history: &[HistoryEntry],
    profile: Profile,
) -> Result<(Option<&'a ObservedAction>, Vec<RankedAction>), PolicyError> {
    if scored.is_empty() {
        return Ok((None, Vec::new()));
    }
    if scored.len() == 1 {
        let (action, conf) = scored[0];
        let urge = damped_urge(conf, habituation_weight(action.id(), history));
        let ranked = vec![ranked_action(action, urge)];
        if urge < profile.thresholds().min_confidence {
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
    let drives = Drives::new(&[("goal", Confidence::MAX)])
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let mut scores_by_option = Scores::new(&question);
    let mut affinity = Affinity::new(&question, &drives);
    for (action, conf) in scored {
        let idx = set
            .index_of(action.id().as_str())
            .ok_or_else(|| PolicyError::Internal("missing target option".into()))?;
        scores_by_option
            .set(idx, *conf)
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
        affinity
            .set(
                DriveIndex::new(0),
                idx,
                habituation_weight(action.id(), history),
            )
            .map_err(|e| PolicyError::Internal(e.to_string()))?;
    }
    let arbitration = arbitrate(&scores_by_option, &drives, &affinity, None, profile)
        .map_err(|e| PolicyError::Internal(e.to_string()))?;
    let ranked =
        ranked_from_answer_targets(&set, arbitration.answer(), arbitration.urges(), scored)?;
    match arbitration.answer() {
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

fn ranked_kind(kind: ActionKind, conf: Confidence) -> Result<RankedAction, PolicyError> {
    Ok(RankedAction {
        id: ActionId::try_new(kind.as_str()).map_err(|e| PolicyError::Internal(e.to_string()))?,
        kind,
        label: kind.as_str().to_owned(),
        confidence_millis: conf.get(),
    })
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
    urges: &Scores<'_>,
) -> Result<Vec<RankedAction>, PolicyError> {
    let entries = match answer {
        Answer::Choice { ranked, .. } | Answer::Abstain { ranked, .. } => ranked.entries(),
        _ => return Ok(Vec::new()),
    };
    entries
        .iter()
        .map(|(idx, _)| {
            let id = set
                .id(*idx)
                .ok_or_else(|| PolicyError::Internal("missing ranked operation".into()))?;
            let kind = ActionKind::parse(id.as_str())
                .ok_or_else(|| PolicyError::Internal(format!("bad kind {}", id.as_str())))?;
            let urge = urges
                .get(*idx)
                .ok_or_else(|| PolicyError::Internal("missing operation urge".into()))?;
            ranked_kind(kind, urge)
        })
        .collect()
}

fn ranked_from_answer_targets(
    set: &CandidateSet,
    answer: &Answer,
    urges: &Scores<'_>,
    scored: &[(&ObservedAction, Confidence)],
) -> Result<Vec<RankedAction>, PolicyError> {
    let entries = match answer {
        Answer::Choice { ranked, .. } | Answer::Abstain { ranked, .. } => ranked.entries(),
        _ => return Ok(Vec::new()),
    };
    entries
        .iter()
        .map(|(idx, _)| {
            let id = set
                .id(*idx)
                .ok_or_else(|| PolicyError::Internal("missing ranked target".into()))?;
            let (action, _) = scored
                .iter()
                .find(|(a, _)| a.id().as_str() == id.as_str())
                .ok_or_else(|| PolicyError::Internal("ranked target not in space".into()))?;
            let urge = urges
                .get(*idx)
                .ok_or_else(|| PolicyError::Internal("missing target urge".into()))?;
            Ok(ranked_action(action, urge))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aui_core::{parse_fixture, ElementState, InteractionManifold};

    fn space_from(fixture: &str) -> ActionSpace {
        let m = parse_fixture(fixture).unwrap();
        ActionSpace::from_manifold(&m)
    }

    const BENCH_SUFFIX: &str = " The page is already open in the browser. Use only this site. If the task cannot be done on this site, stop and say so (give up) without changing anything.";

    fn space_with_state(fixture: &str, id: &str, state: ElementState) -> ActionSpace {
        let manifold = parse_fixture(fixture).unwrap();
        let regions = manifold
            .regions()
            .map(|region| {
                if region.id().as_str() == id {
                    region.clone().with_state(state.clone())
                } else {
                    region.clone()
                }
            })
            .collect();
        let manifold =
            InteractionManifold::try_new(manifold.viewport(), regions, manifold.captured_at_ms())
                .unwrap();
        ActionSpace::from_manifold(&manifold)
    }

    #[test]
    fn missing_quoted_button_target_abstains() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=inbox role=link label="Inbox" x=10 y=10 w=100 h=24 actions=click sources=dom,accessibility
            region id=select-all role=checkbox label="Select all" x=10 y=50 w=100 h=24 actions=click sources=dom,accessibility
            region id=compose role=button label="Compose" x=10 y=90 w=100 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal =
            format!("Print all messages in the inbox with the \"Print all\" button.{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        assert!(
            matches!(outcome, PolicyOutcome::Abstain { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn clause_head_save_beats_settings_link() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=save role=button label="Save" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=settings role=link label="Settings" x=10 y=50 w=100 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal = format!("In Settings, save the signature.{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Save");
    }

    #[test]
    fn explicit_text_field_reference_selects_text_target() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=search role=text_field label="Search" x=10 y=10 w=300 h=24 actions=click,type sources=dom,accessibility
            region id=go role=button label="Go" x=320 y=10 w=60 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal = format!("type rust ownership in the Search box{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::TypeText);
        assert_eq!(choice.target_label, "Search");
    }

    #[test]
    fn explicit_select_field_reference_selects_select_target() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=cabin role=combobox label="Cabin class" x=10 y=10 w=200 h=24 actions=select sources=dom,accessibility
            region id=find role=button label="Find flights" x=10 y=50 w=120 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal = format!("select business in Cabin class{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Select);
        assert_eq!(choice.target_label, "Cabin class");
    }

    #[test]
    fn clause_head_does_not_choose_an_already_checked_reminder() {
        let fixture = r#"
            viewport w=800 h=600
            region id=email role=checkbox label="Email reminders" x=10 y=10 w=180 h=24 actions=click sources=dom,accessibility
            region id=sms role=checkbox label="SMS reminders" x=10 y=50 w=180 h=24 actions=click sources=dom,accessibility
            "#;
        let space = space_with_state(
            fixture,
            "email",
            ElementState {
                checked: Some(true),
                ..ElementState::default()
            },
        );
        let goal = format!("Make sure email reminders are turned on.{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        assert!(
            matches!(outcome, PolicyOutcome::Abstain { .. }),
            "{outcome:?}"
        );
        assert!(outcome.as_choice().is_none());
    }

    #[test]
    fn named_unsatisfied_timezone_is_selected_before_save() {
        let fixture = r#"
            viewport w=800 h=600
            region id=timezone role=combobox label="Time zone" x=10 y=10 w=200 h=24 actions=select,type sources=dom,accessibility
            region id=save role=button label="Save" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            region id=settings role=link label="Settings" x=10 y=90 w=100 h=24 actions=click sources=dom,accessibility
            "#;
        let goal =
            format!("Change the time zone to \"Asia/Manila\" and save the settings.{BENCH_SUFFIX}");
        let unsatisfied = space_with_state(
            fixture,
            "timezone",
            ElementState {
                selected: Some("America/Los_Angeles".to_owned()),
                options: vec!["America/Los_Angeles".to_owned(), "Asia/Manila".to_owned()],
                ..ElementState::default()
            },
        );
        let outcome = InstinctPolicy::default()
            .decide(&unsatisfied, &AgentGoal::new(&goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Select);
        assert_eq!(choice.target_label, "Time zone");

        let satisfied = space_with_state(
            fixture,
            "timezone",
            ElementState {
                selected: Some("Asia/Manila".to_owned()),
                options: vec!["America/Los_Angeles".to_owned(), "Asia/Manila".to_owned()],
                ..ElementState::default()
            },
        );
        let outcome = InstinctPolicy::default()
            .decide(&satisfied, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Save");
    }

    #[test]
    fn email_reply_selects_named_link_then_fills_then_sends() {
        let list_page = space_from(
            r#"
            viewport w=800 h=600
            region id=q3 role=link label="Q3 launch checklist, from Maya Reyes" x=10 y=10 w=280 h=24 actions=click sources=dom,accessibility
            region id=sprint role=link label="Sprint retro, from Maya Reyes" x=10 y=50 w=240 h=24 actions=click sources=dom,accessibility
            region id=search role=text_field label="Search mail" x=10 y=90 w=200 h=24 actions=type sources=dom,accessibility
            "#,
        );
        let goal = format!(
            "Open the email \"Q3 launch checklist\" from Maya Reyes and send the quick reply \"On it, thanks\".{BENCH_SUFFIX}"
        );
        let outcome = InstinctPolicy::default()
            .decide(&list_page, &AgentGoal::new(&goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Q3 launch checklist, from Maya Reyes");

        let email_fixture = r#"
            viewport w=800 h=600
            region id=reply role=text_field label="Reply to Maya Reyes" x=10 y=10 w=300 h=80 actions=type sources=dom,accessibility
            region id=send role=button label="Send" x=10 y=110 w=80 h=24 actions=click sources=dom,accessibility
            "#;
        let empty_reply = space_with_state(email_fixture, "reply", ElementState::default());
        let outcome = InstinctPolicy::default()
            .decide(&empty_reply, &AgentGoal::new(&goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::TypeText);
        assert_eq!(choice.target_label, "Reply to Maya Reyes");

        let filled_reply = space_with_state(
            email_fixture,
            "reply",
            ElementState {
                value: Some("On it, thanks".to_owned()),
                ..ElementState::default()
            },
        );
        let outcome = InstinctPolicy::default()
            .decide(&filled_reply, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Send");
    }

    #[test]
    fn clause_head_archive_beats_named_email() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=archive role=button label="Archive" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=q3 role=link label="Q3 launch checklist, from Maya Reyes" x=10 y=50 w=280 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal = format!("Archive the open email \"Q3 launch checklist\".{BENCH_SUFFIX}");
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        let choice = outcome.as_choice().expect("expected choice");
        assert_eq!(choice.kind, ActionKind::Click);
        assert_eq!(choice.target_label, "Archive");
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
        let mut policy = InstinctPolicy::default();
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
        let mut policy = InstinctPolicy::default();
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
        let mut policy = InstinctPolicy::default();
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
        let mut policy = InstinctPolicy::new(Profile::Fast);
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
        let mut policy = InstinctPolicy::default();
        let outcome = policy.decide(&space, &AgentGoal::new("DONE"), &[]).unwrap();
        let choice = outcome.as_choice().expect("choice");
        assert_eq!(choice.kind, ActionKind::Done);
    }

    #[test]
    fn conditional_give_up_suffix_does_not_choose_done() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=weekly role=button label="Weekly sync moved to 2 PM" x=10 y=10 w=240 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let goal = r#"Star the email "Weekly sync". If the task cannot be done on this site, stop and say so (give up) without changing anything."#;
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new(goal), &[])
            .unwrap();
        assert!(
            !matches!(
                outcome.as_choice(),
                Some(choice) if choice.kind == ActionKind::Done
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn empty_goal_is_error_not_silent_top() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=ok role=button label="OK" x=10 y=10 w=40 h=20 actions=click sources=dom,accessibility
            "#,
        );
        let mut policy = InstinctPolicy::default();
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
        let mut policy = InstinctPolicy::default();
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

    fn history_entry(action_id: &ActionId, kind: ActionKind, verification: &str) -> HistoryEntry {
        HistoryEntry {
            step: 1,
            action_id: action_id.clone(),
            kind,
            label: String::new(),
            verification: verification.to_owned(),
        }
    }

    #[test]
    fn no_failed_streak_matches_direct_decide() {
        let kinds = [ActionKind::Click, ActionKind::Wait];
        let operation_scores = BTreeMap::from([
            (ActionKind::Click, Confidence::new(900).unwrap()),
            (ActionKind::Wait, Confidence::new(100).unwrap()),
        ]);
        let operation_ids = BTreeMap::from([
            (ActionKind::Click, ActionId::try_new("CLICK:save").unwrap()),
            (ActionKind::Wait, ActionId::try_new("WAIT").unwrap()),
        ]);
        let operation_set = CandidateSet::new(
            kinds
                .iter()
                .map(|kind| CandidateId::new(kind.as_str()).unwrap()),
        )
        .unwrap();
        let operation_question = operation_set.question("operation").unwrap();
        let mut direct_operation_scores = Scores::new(&operation_question);
        for kind in kinds {
            direct_operation_scores
                .set(
                    operation_set.index_of(kind.as_str()).unwrap(),
                    operation_scores[&kind],
                )
                .unwrap();
        }
        let direct_operation_answer =
            instinct_core::decide(&direct_operation_scores, Profile::Standard);
        let (operation_choice, operation_ranked) = choose_kinds(
            &kinds,
            &operation_scores,
            &operation_ids,
            &[],
            Profile::Standard,
        )
        .unwrap();
        let direct_operation_choice = match direct_operation_answer {
            Answer::Choice { option, .. } => operation_set
                .id(option)
                .and_then(|id| ActionKind::parse(id.as_str())),
            Answer::Abstain { .. } => None,
            other => panic!("unexpected operation answer {other:?}"),
        };
        assert_eq!(operation_choice, direct_operation_choice);
        assert_eq!(
            operation_ranked
                .iter()
                .map(|ranked| (ranked.kind, ranked.confidence_millis))
                .collect::<Vec<_>>(),
            vec![(ActionKind::Click, 900), (ActionKind::Wait, 100)]
        );

        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=save role=button label="Save" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=cancel role=button label="Cancel" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let scored: Vec<_> = space
            .targets_of(ActionKind::Click)
            .map(|action| (action, score_action("Save", action)))
            .collect();
        let target_set = CandidateSet::new(
            scored
                .iter()
                .map(|(action, _)| CandidateId::new(action.id().as_str()).unwrap()),
        )
        .unwrap();
        let target_question = target_set.question("target").unwrap();
        let mut direct_target_scores = Scores::new(&target_question);
        for (action, evidence) in &scored {
            direct_target_scores
                .set(
                    target_set.index_of(action.id().as_str()).unwrap(),
                    *evidence,
                )
                .unwrap();
        }
        let direct_target_answer = instinct_core::decide(&direct_target_scores, Profile::Standard);
        let (target_choice, target_ranked) =
            choose_targets(&scored, &[], Profile::Standard).unwrap();
        let direct_target_id = match direct_target_answer {
            Answer::Choice { option, .. } => target_set.id(option).map(|id| id.as_str()),
            Answer::Abstain { .. } => None,
            other => panic!("unexpected target answer {other:?}"),
        };
        assert_eq!(
            target_choice.map(|action| action.id().as_str()),
            direct_target_id
        );
        assert!(target_ranked.iter().all(|ranked| {
            let option = target_set.index_of(ranked.id.as_str()).unwrap();
            direct_target_scores.get(option).unwrap().get() == ranked.confidence_millis
        }));
    }

    #[test]
    fn one_no_effect_dampens_urge_by_a_quarter() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=go role=button label="Go" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let action = space.targets_of(ActionKind::Click).next().unwrap();
        let scored = [(action, Confidence::MAX)];
        let history = [history_entry(action.id(), ActionKind::Click, "no-effect")];
        let (chosen, ranked) = choose_targets(&scored, &history, Profile::Standard).unwrap();
        assert_eq!(chosen.map(|a| a.id()), Some(action.id()));
        assert_eq!(ranked[0].confidence_millis, 750);
    }

    #[test]
    fn four_trailing_no_effects_abstain_on_the_only_target() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=go role=button label="Go" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let action = space.targets_of(ActionKind::Click).next().unwrap();
        let scored = [(action, Confidence::MAX)];
        let history: Vec<_> = (0..4)
            .map(|_| history_entry(action.id(), ActionKind::Click, "no-effect"))
            .collect();
        let (chosen, ranked) = choose_targets(&scored, &history, Profile::Standard).unwrap();
        assert!(chosen.is_none());
        assert_eq!(ranked[0].confidence_millis, 0);
    }

    #[test]
    fn habituation_shifts_to_the_next_supported_target() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=a role=button label="Save" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            region id=b role=button label="Save" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let mut targets = space.targets_of(ActionKind::Click);
        let target_a = targets
            .find(|action| action.id().as_str() == "CLICK:a")
            .unwrap();
        let target_b = targets
            .find(|action| action.id().as_str() == "CLICK:b")
            .unwrap();
        let score_a = score_action("Save", target_a);
        let score_b = score_action("Save", target_b);
        assert_eq!(score_a.get(), 1000);
        assert_eq!(score_b.get(), 1000);
        let history = [
            history_entry(target_a.id(), ActionKind::Click, "no-effect"),
            history_entry(target_a.id(), ActionKind::Click, "no-effect"),
        ];
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new("Save"), &history)
            .unwrap();
        let choice = outcome.as_choice().expect("expected next target");
        assert_eq!(choice.action_id.as_str(), "CLICK:b");
        assert_eq!(choice.confidence_millis, 1000);
        assert_eq!(choice.operation_ranked[0].confidence_millis, 1000);
    }

    #[test]
    fn failure_streak_resets_on_success_or_another_id() {
        let id_a = ActionId::try_new("CLICK:a").unwrap();
        let id_b = ActionId::try_new("CLICK:b").unwrap();
        let success = [
            history_entry(&id_a, ActionKind::Click, "no-effect"),
            history_entry(&id_a, ActionKind::Click, "success"),
        ];
        let other_id = [
            history_entry(&id_a, ActionKind::Click, "no-effect"),
            history_entry(&id_b, ActionKind::Click, "no-effect"),
        ];
        assert_eq!(habituation_weight(&id_a, &success), Confidence::MAX);
        assert_eq!(habituation_weight(&id_a, &other_id), Confidence::MAX);
    }

    #[test]
    fn wrong_effect_counts_like_no_effect() {
        let id = ActionId::try_new("CLICK:a").unwrap();
        let no_effect = [history_entry(&id, ActionKind::Click, "no-effect")];
        let wrong_effect = [history_entry(&id, ActionKind::Click, "wrong-effect")];
        assert_eq!(
            habituation_weight(&id, &wrong_effect),
            habituation_weight(&id, &no_effect)
        );
        assert_eq!(habituation_weight(&id, &wrong_effect).get(), 750);
    }

    #[test]
    fn four_failed_scrolls_do_not_choose_scroll_down_again() {
        let space = space_from(
            r#"
            viewport w=800 h=600
            region id=go role=button label="Go" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
            "#,
        );
        let scroll_down = ActionId::try_new(ActionKind::ScrollDown.as_str()).unwrap();
        let history: Vec<_> = (0..4)
            .map(|_| history_entry(&scroll_down, ActionKind::ScrollDown, "no-effect"))
            .collect();
        let outcome = InstinctPolicy::default()
            .decide(&space, &AgentGoal::new("scroll down"), &history)
            .unwrap();
        assert!(
            !matches!(
                outcome,
                PolicyOutcome::Choice(PolicyDecision {
                    ref action_id,
                    kind: ActionKind::ScrollDown,
                    ..
                }) if *action_id == scroll_down
            ),
            "{outcome:?}"
        );
    }
}
