//! The dojo policy (issue #49, work item 5): lessons inside the loop.
//!
//! Decision order, closed as a [`BrowserPolicy`] so the agent loop's
//! guard → ticket → revalidate → executor chain is untouched:
//!
//! 1. credit the pending remote choice (verified win → lesson, verified
//!    miss → loss) from `history` before deciding again;
//! 2. `InstinctPolicy` with this situation's trust evidence applied
//!    (±[`aui_policy::TRUST_CAP_MILLIS`]);
//! 3. a learned move replay: if `moves[key]` names a step for this clause
//!    whose action is still offered and not distrusted, choose it;
//! 4. remote escalation (armed only when a remote policy is given);
//!    a remote choice is remembered in `pending` until its step verdict
//!    shows up in `history`.
//!
//! On an identical rerun the learned move answers locally — zero remote
//! calls. Situation keys are `context_key` (near-free): the policy cannot
//! know `near` before deciding, so it never keys on it.

use std::collections::BTreeMap;

use aui_core::{ActionId, ActionKind, ActionSpace};
use aui_dojo::{
    context_key, label_bonus_map, site_line, trust_bonus, LessonStore, Move, MoveStep, Situation,
    Word,
};
use aui_policy::{
    label_names_target, AgentGoal, BrowserPolicy, HistoryEntry, InstinctPolicy, PolicyContext,
    PolicyDecision, PolicyError, PolicyOutcome,
};

/// A remote choice whose step verdict has not arrived yet: the situation
/// key it was decided in, the clause, the offered action id, and its
/// label for the trust entry.
type Pending = (String, String, String, String);

/// Instinct + lesson store + optional remote fallback.
pub struct DojoPolicy {
    local: InstinctPolicy,
    remote: Option<Box<dyn BrowserPolicy>>,
    store: LessonStore,
    ctx: PolicyContext,
    last: &'static str,
    pending: Option<Pending>,
    now_ms: u64,
    learned: usize,
}

impl DojoPolicy {
    pub fn new(remote: Option<Box<dyn BrowserPolicy>>, store: LessonStore) -> Self {
        Self {
            local: InstinctPolicy::default(),
            remote,
            store,
            ctx: PolicyContext::default(),
            last: "instinct",
            pending: None,
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            learned: 0,
        }
    }

    /// The learned lessons this policy grew during the run (wins and
    /// losses credited from remote choices).
    pub fn learned_count(&self) -> usize {
        self.learned
    }

    /// The store (load it, run, then save back through [`Self::into_store`]).
    pub fn store(&self) -> &LessonStore {
        &self.store
    }

    /// Take the store back for persisting at run end.
    pub fn into_store(self) -> LessonStore {
        self.store
    }

    /// Test knob: pin the clock the trust decay reads.
    #[cfg(test)]
    pub fn set_now_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// This decision's context key from the situation the agent pushed.
    fn key(&self, goal: &AgentGoal) -> String {
        let site = site_line(self.ctx.site_url.as_deref(), self.ctx.site_title.as_deref());
        let situation = Situation {
            front_layer: self.ctx.front_layer,
            roles: self.ctx.roles.clone(),
            near: Vec::new(),
        };
        context_key(site.as_ref(), &situation, goal.as_str())
    }

    /// Fold the pending remote choice's verdict into the store. Verified
    /// win → trust win + a Move + a Word; verified miss → trust loss.
    /// The pending entry stays until its action shows up in `history`.
    fn credit_pending(&mut self, history: &[HistoryEntry]) {
        let Some((key, clause, action_id, label)) = self.pending.take() else {
            return;
        };
        let Some(entry) = history
            .iter()
            .rev()
            .find(|h| h.action_id.as_str() == action_id)
        else {
            self.pending = Some((key, clause, action_id, label));
            return;
        };
        let won = match entry.kind {
            ActionKind::Click => {
                matches!(
                    entry.verification.as_str(),
                    "success" | "state-changed" | "navigation"
                ) && label_names_target(&clause, &label)
            }
            ActionKind::TypeText | ActionKind::Select => entry.verification == "success",
            _ => false,
        };
        let table = self.store.trust.entry(key.clone()).or_default();
        let rec = table.entry(label.clone()).or_default();
        if won {
            rec.wins += 1;
        } else {
            rec.losses += 1;
        }
        rec.last_seen_ms = rec.last_seen_ms.max(self.now_ms);
        self.learned += 1;
        if won {
            let steps = vec![MoveStep {
                clause: clause.clone(),
                action_id: action_id.clone(),
                label: label.clone(),
            }];
            let moves = self.store.moves.entry(key.clone()).or_default();
            if !moves.iter().any(|m| m.steps == steps) {
                moves.push(Move {
                    steps,
                    diaries: vec![],
                });
            }
            let words = self.store.words.entry(key).or_default();
            match words.iter_mut().find(|w| w.phrase == clause) {
                Some(w) => w.label = label,
                None => words.push(Word {
                    phrase: clause,
                    label,
                    diaries: vec![],
                }),
            }
        }
    }

    /// A learned move for this situation whose step covers this clause,
    /// is still offered, and isn't distrusted — the local answer that
    /// makes an identical rerun need zero remote calls.
    fn learned_choice(
        &self,
        space: &ActionSpace,
        key: &str,
        goal: &AgentGoal,
        abstain: &PolicyOutcome,
    ) -> Option<PolicyDecision> {
        let moves = self.store.moves.get(key)?;
        let trust_table = self.store.trust.get(key);
        // Higher belts replay first; ties keep store order (distilled
        // lessons sit earlier than in-run learned ones).
        let mut ranked: Vec<&aui_dojo::Move> = moves.iter().collect();
        ranked.sort_by_key(|mv| std::cmp::Reverse(aui_dojo::move_belt(&self.store, key, mv)));
        let mut best: Option<&MoveStep> = None;
        for mv in ranked {
            for step in &mv.steps {
                if step.clause != goal.as_str() {
                    continue;
                }
                let Ok(id) = ActionId::try_new(step.action_id.as_str()) else {
                    continue;
                };
                if space.get(&id).is_none() {
                    continue;
                }
                if let Some(trust) = trust_table.and_then(|t| t.get(&step.label)) {
                    if trust_bonus(trust, self.now_ms) < 0 {
                        continue;
                    }
                }
                best = Some(step);
                break;
            }
            if best.is_some() {
                break;
            }
        }
        let step = best?;
        let id = ActionId::try_new(step.action_id.as_str()).ok()?;
        let action = space.get(&id)?;
        // Report the evidence Instinct saw for this action (the abstain's
        // own ranked list) — the lesson supplies the choice, not a score.
        let confidence = match abstain {
            PolicyOutcome::Abstain { target_ranked, .. } => target_ranked
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.confidence_millis)
                .unwrap_or(0),
            PolicyOutcome::Choice(_) => 0,
        };
        Some(PolicyDecision {
            action_id: id,
            kind: action.kind(),
            target_label: step.label.clone(),
            confidence_millis: confidence,
            operation_ranked: Vec::new(),
            target_ranked: Vec::new(),
        })
    }
}

impl BrowserPolicy for DojoPolicy {
    fn name(&self) -> &'static str {
        "dojo"
    }

    fn decision_source(&self) -> &'static str {
        self.last
    }

    fn set_situation(&mut self, ctx: &PolicyContext) {
        self.ctx = ctx.clone();
    }

    fn decide(
        &mut self,
        space: &ActionSpace,
        goal: &AgentGoal,
        history: &[HistoryEntry],
    ) -> Result<PolicyOutcome, PolicyError> {
        self.credit_pending(history);
        let key = self.key(goal);

        // Trust evidence into Instinct, capped — item 4's only channel.
        let adjustments: BTreeMap<String, i16> = self
            .store
            .trust
            .get(&key)
            .map(|table| label_bonus_map(table, self.now_ms))
            .unwrap_or_default();
        self.local.set_evidence_adjustments(adjustments);
        let first = self.local.decide(space, goal, history)?;
        self.last = self.local.decision_source();
        if first.as_choice().is_some() {
            return Ok(first);
        }

        if let Some(choice) = self.learned_choice(space, &key, goal, &first) {
            self.last = "dojo";
            return Ok(PolicyOutcome::Choice(choice));
        }

        if let Some(remote) = self.remote.as_mut() {
            let outcome = remote.decide(space, goal, history)?;
            self.last = remote.decision_source();
            if let Some(choice) = outcome.as_choice() {
                self.pending = Some((
                    key,
                    goal.as_str().to_owned(),
                    choice.action_id.as_str().to_owned(),
                    choice.target_label.clone(),
                ));
            }
            return Ok(outcome);
        }

        Ok(first)
    }
}
