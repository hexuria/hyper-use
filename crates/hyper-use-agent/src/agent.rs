//! Agent state machine: Ready ↔ Predicted → Done/Blocked.
//!
//! One tick:
//!
//! ```text
//! observe → ActionSpace (front layer applied) → policy (Instinct first)
//!   → DONE / BLOCKED?            terminal, no input
//!   → TYPE_TEXT / SELECT payload  TextResolver (never Instinct)
//!   → hard gate on the decided observation → ActionTicket
//!   → executor: ledger → fresh observe → revalidate → gate → consume → input
//!   → settle → observe → diff / value check → history
//! ```
//!
//! A stale ticket discards the prediction and returns to Ready (observe and
//! decide again). It is not a failed task, but consecutive stale discards are
//! bounded.
//!
//! Multi-step goals (`"type X then click Go"`) are split on `then` / `and then`
//! ([`hyper_use_policy::split_sequential_clauses`]). Each clause is one Instinct
//! single-intent; when Instinct chooses DONE the agent advances to the next clause
//! instead of finishing. See that function's docs for the connective limits.

use hyper_use_browser::ScrollDirection;
use hyper_use_core::{Action, ActionKind, ActionSpace, InteractionManifold, RegionId};
use hyper_use_guard::{gate, with_front_layer, TicketLedger};
use hyper_use_policy::{
    split_sequential_clauses, AgentGoal, BrowserPolicy, DeterministicTextResolver, HistoryEntry,
    PolicyDecision, PolicyOutcome, TextContext, TextError, TextResolver,
};
#[cfg(feature = "model-text")]
use hyper_use_policy::{ModelTextResolver, TextModel};

use crate::error::AgentError;
use crate::executor::{execute_ticketed, ExecError};
use crate::outcome::{AgentOutcome, StepRecord, VerificationKind};
use crate::runtime::{BrowserRuntime, Input};
use crate::verify_map::{classify_delta, classify_value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Ready,
    Predicted,
    Done,
    Blocked,
}

/// A policy choice waiting for ticketed execution.
///
/// There is exactly **one** staleness barrier between this prediction and page
/// input: the [`hyper_use_protocol::ActionTicket`] issued by
/// [`gate()`](fn@hyper_use_guard::gate) on [`Self::manifold`] and revalidated by
/// [`crate::execute_ticketed`] against a fresh observation (target-scoped world
/// fingerprint + target role / label / fingerprint). A prediction carries no
/// fingerprint of its own, so nothing here implies a second pre-ticket check
/// (ADR 0005 §R2).
#[derive(Clone, Debug)]
pub struct Predicted {
    pub decision: PolicyDecision,
    /// TYPE_TEXT text or SELECT option, from the [`TextResolver`]. Its context
    /// (goal clause, target label / role / fingerprint) is checked once at
    /// resolution time; target drift after that is caught by the ticket.
    pub payload: Option<String>,
    pub space_captured_at_ms: u64,
    /// The observation the decision was made on (ticket is issued against it).
    pub manifold: InteractionManifold,
    pub focused: Option<RegionId>,
}

pub struct Agent<B, P, T = DeterministicTextResolver> {
    browser: B,
    policy: P,
    text: T,
    goal: AgentGoal,
    /// Full goal split on `then` / `and then`. [`Self::goal`] is the active clause.
    clauses: Vec<String>,
    clause_index: usize,
    state: AgentState,
    predicted: Option<Predicted>,
    history: Vec<StepRecord>,
    policy_history: Vec<HistoryEntry>,
    ledger: TicketLedger,
    max_steps: u32,
    max_policy_calls: u32,
    max_consecutive_no_effect: u32,
    max_consecutive_stale: u32,
    steps_taken: u32,
    policy_calls: u32,
    consecutive_no_effect: u32,
    consecutive_stale: u32,
    stale_total: u32,
}

pub struct AgentBuilder<B, P, T = DeterministicTextResolver> {
    browser: B,
    policy: P,
    text: T,
    max_steps: u32,
    max_policy_calls: u32,
    max_consecutive_no_effect: u32,
    max_consecutive_stale: u32,
}

impl<B, P> AgentBuilder<B, P, DeterministicTextResolver> {
    pub fn new(browser: B, policy: P) -> Self {
        Self {
            browser,
            policy,
            text: DeterministicTextResolver,
            max_steps: 60,
            max_policy_calls: 120,
            max_consecutive_no_effect: 3,
            max_consecutive_stale: 5,
        }
    }
}

impl<B, P, T> AgentBuilder<B, P, T> {
    pub fn text_resolver<T2>(self, text: T2) -> AgentBuilder<B, P, T2> {
        AgentBuilder {
            browser: self.browser,
            policy: self.policy,
            text,
            max_steps: self.max_steps,
            max_policy_calls: self.max_policy_calls,
            max_consecutive_no_effect: self.max_consecutive_no_effect,
            max_consecutive_stale: self.max_consecutive_stale,
        }
    }

    /// Use a model for TYPE_TEXT / SELECT payloads (feature `model-text`).
    ///
    /// The model only fills the payload of an action Instinct already chose; the
    /// reply is context-bound, shape-checked, and grounded in the goal clause
    /// before it is used, then gated / ticketed like any payload. On model
    /// failure or a refused reply it falls back to
    /// [`DeterministicTextResolver`], and abstains when that also fails. For
    /// other fallback / grounding settings build a [`ModelTextResolver`] and
    /// pass it to [`Self::text_resolver`].
    #[cfg(feature = "model-text")]
    pub fn model_text<M: TextModel>(self, model: M) -> AgentBuilder<B, P, ModelTextResolver<M>> {
        self.text_resolver(ModelTextResolver::new(model))
    }

    pub fn max_steps(mut self, n: u32) -> Self {
        self.max_steps = n;
        self
    }

    pub fn max_policy_calls(mut self, n: u32) -> Self {
        self.max_policy_calls = n;
        self
    }

    pub fn max_consecutive_no_effect(mut self, n: u32) -> Self {
        self.max_consecutive_no_effect = n.max(1);
        self
    }

    pub fn max_consecutive_stale(mut self, n: u32) -> Self {
        self.max_consecutive_stale = n.max(1);
        self
    }

    pub fn build(self, goal: impl Into<String>) -> Agent<B, P, T> {
        let raw = goal.into();
        let clauses = split_sequential_clauses(&raw);
        let active = clauses.first().cloned().unwrap_or_default();
        Agent {
            browser: self.browser,
            policy: self.policy,
            text: self.text,
            goal: AgentGoal::new(active),
            clauses,
            clause_index: 0,
            state: AgentState::Ready,
            predicted: None,
            history: Vec::new(),
            policy_history: Vec::new(),
            ledger: TicketLedger::new(),
            max_steps: self.max_steps,
            max_policy_calls: self.max_policy_calls,
            max_consecutive_no_effect: self.max_consecutive_no_effect,
            max_consecutive_stale: self.max_consecutive_stale,
            steps_taken: 0,
            policy_calls: 0,
            consecutive_no_effect: 0,
            consecutive_stale: 0,
            stale_total: 0,
        }
    }
}

/// Region capability a target-bound agent operation needs.
pub fn region_action(kind: ActionKind) -> Option<Action> {
    match kind {
        ActionKind::Click => Some(Action::Click),
        ActionKind::TypeText => Some(Action::Type),
        ActionKind::Select => Some(Action::Select),
        _ => None,
    }
}

impl<B, P, T> Agent<B, P, T>
where
    B: BrowserRuntime,
    P: BrowserPolicy,
    T: TextResolver,
{
    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn history(&self) -> &[StepRecord] {
        &self.history
    }

    pub fn goal(&self) -> &AgentGoal {
        &self.goal
    }

    /// Sequential clauses the agent will run (length 1 when the goal has no `then`).
    pub fn clauses(&self) -> &[String] {
        &self.clauses
    }

    pub fn clause_index(&self) -> usize {
        self.clause_index
    }

    /// Advance to the next `then` clause, if any. Returns true when advanced.
    fn advance_clause(&mut self) -> bool {
        let next = self.clause_index + 1;
        if next >= self.clauses.len() {
            return false;
        }
        self.clause_index = next;
        self.goal = AgentGoal::new(self.clauses[next].clone());
        self.state = AgentState::Ready;
        self.predicted = None;
        self.consecutive_no_effect = 0;
        self.consecutive_stale = 0;
        true
    }

    /// The payload resolver (e.g. to inspect a model resolver's last source).
    pub fn text_resolver(&self) -> &T {
        &self.text
    }

    pub fn browser_mut(&mut self) -> &mut B {
        &mut self.browser
    }

    /// Hand the browser back (e.g. to run the next goal on the same page).
    pub fn into_browser(self) -> B {
        self.browser
    }

    pub fn policy_calls(&self) -> u32 {
        self.policy_calls
    }

    /// Stale predictions discarded so far (ticket revalidation caught a change).
    pub fn stale_discards(&self) -> u32 {
        self.stale_total
    }

    pub fn observe(&mut self) -> Result<&InteractionManifold, AgentError> {
        self.browser.observe()
    }

    /// The finite action space for an observation: front layer applied, so a
    /// control behind an open dialog is never offered.
    pub fn action_space(manifold: &InteractionManifold) -> ActionSpace {
        ActionSpace::from_manifold(&with_front_layer(manifold))
    }

    /// Policy decides on a fresh observation. Any previous prediction is discarded.
    pub fn predict(&mut self) -> Result<Option<&Predicted>, AgentError> {
        if !matches!(self.state, AgentState::Ready | AgentState::Predicted) {
            return Err(AgentError::InvalidState("predict from terminal state"));
        }
        if self.policy_calls >= self.max_policy_calls {
            return Err(AgentError::MaxPolicyCalls);
        }
        self.predicted = None;
        self.state = AgentState::Ready;

        let manifold = self.browser.observe()?.clone();
        let focused = self.browser.focused();
        let space = Self::action_space(&manifold);
        self.policy_calls += 1;
        let outcome = self
            .policy
            .decide(&space, &self.goal, &self.policy_history)
            .map_err(|e| AgentError::Policy(e.to_string()))?;

        let decision = match outcome {
            PolicyOutcome::Abstain { reason, .. } => return Err(AgentError::Abstain(reason)),
            PolicyOutcome::Choice(decision) => decision,
        };
        // Off-menu guard: whatever the policy (local or remote) returned must
        // be an offered action of the matching kind.
        let offered = space.get(&decision.action_id).ok_or_else(|| {
            AgentError::Policy(format!("off-menu action `{}`", decision.action_id))
        })?;
        if offered.kind() != decision.kind {
            return Err(AgentError::Policy(format!(
                "action `{}` is {} not {}",
                decision.action_id,
                offered.kind(),
                decision.kind
            )));
        }
        match decision.kind {
            ActionKind::Done => {
                self.state = AgentState::Done;
                return Ok(None);
            }
            ActionKind::Blocked => {
                self.state = AgentState::Blocked;
                return Ok(None);
            }
            _ => {}
        }

        let mut payload = None;
        if matches!(decision.kind, ActionKind::TypeText | ActionKind::Select) {
            let ctx = TextContext {
                goal: self.goal.clone(),
                field_label: offered.label().to_owned(),
                field_role: if decision.kind == ActionKind::Select {
                    "select".to_owned()
                } else {
                    offered
                        .role()
                        .map(|r| r.as_str().to_owned())
                        .unwrap_or_default()
                },
                context_fingerprint: offered.target_fingerprint(),
            };
            let resolution = self.text.resolve(&ctx).map_err(|e| match e {
                // A resolver that declines (model refused, no fallback value)
                // abstains like Instinct: nothing typed, never a guessed value.
                TextError::Abstain(reason) => AgentError::Abstain(format!("text: {reason}")),
                other => AgentError::Text(other.to_string()),
            })?;
            if resolution.context_fingerprint != ctx.fingerprint() {
                return Err(AgentError::Text(
                    "resolution belongs to a different context".into(),
                ));
            }
            // The target is revalidated after resolution by the executor
            // (fresh observe + ticket), so resolver latency cannot go stale
            // unnoticed. The ticket binds the same target role / label /
            // fingerprint the text context was built from.
            payload = Some(resolution.text);
        }

        self.predicted = Some(Predicted {
            decision,
            payload,
            space_captured_at_ms: space.captured_at_ms(),
            manifold,
            focused,
        });
        self.state = AgentState::Predicted;
        Ok(self.predicted.as_ref())
    }

    /// Gate → ticket → executor (revalidate/consume/input) → verify.
    ///
    /// A stale ticket discards the prediction and returns
    /// [`AgentError::Stale`] with the agent back in Ready.
    pub fn act(&mut self) -> Result<StepRecord, AgentError> {
        if self.state != AgentState::Predicted {
            return Err(AgentError::InvalidState("act requires Predicted"));
        }
        if self.steps_taken >= self.max_steps {
            return Err(AgentError::MaxSteps);
        }
        let predicted = self
            .predicted
            .take()
            .ok_or(AgentError::InvalidState("missing prediction"))?;
        self.state = AgentState::Ready;

        let record = match self.try_act_once(&predicted) {
            Ok(record) => record,
            Err(AgentError::Stale(msg)) => {
                self.consecutive_stale += 1;
                self.stale_total += 1;
                return Err(AgentError::Stale(msg));
            }
            Err(e) => return Err(e),
        };
        self.consecutive_stale = 0;
        self.steps_taken += 1;
        self.history.push(record.clone());
        self.policy_history.push(HistoryEntry {
            step: record.step,
            action_id: record.action_id.clone(),
            kind: record.kind,
            label: record.label.clone(),
            verification: record.verification.as_str().to_owned(),
        });
        if matches!(
            record.verification,
            VerificationKind::NoEffect | VerificationKind::WrongEffect
        ) {
            self.consecutive_no_effect += 1;
            if self.consecutive_no_effect >= self.max_consecutive_no_effect {
                self.state = AgentState::Blocked;
            }
        } else {
            self.consecutive_no_effect = 0;
        }
        Ok(record)
    }

    fn try_act_once(&mut self, predicted: &Predicted) -> Result<StepRecord, AgentError> {
        let kind = predicted.decision.kind;
        let step = self.steps_taken + 1;
        let stale_retries = self.consecutive_stale;
        let record = |verification| StepRecord {
            step,
            action_id: predicted.decision.action_id.clone(),
            kind,
            label: predicted.decision.target_label.clone(),
            verification,
            stale_retries,
        };

        // Page-level controls: no target, no ticket.
        let scroll = match kind {
            ActionKind::ScrollUp => Some(ScrollDirection::Up),
            ActionKind::ScrollDown => Some(ScrollDirection::Down),
            _ => None,
        };
        if scroll.is_some() || kind == ActionKind::Wait {
            let before = predicted.manifold.clone();
            let before_page = self.browser.page().cloned();
            if let Some(direction) = scroll {
                self.browser.scroll(direction)?;
            }
            self.browser.settle();
            let after = self.browser.observe()?.clone();
            let after_page = self.browser.page().cloned();
            let page_d = match (before_page.as_ref(), after_page.as_ref()) {
                (Some(b), Some(a)) => Some(self.browser.page_delta_between(b, a)),
                _ => None,
            };
            return Ok(record(classify_delta(&before, &after, page_d.as_ref())));
        }

        let action = region_action(kind)
            .ok_or(AgentError::InvalidState("non-executable kind reached act"))?;
        let offered = Self::action_space(&predicted.manifold)
            .get(&predicted.decision.action_id)
            .cloned()
            .ok_or_else(|| AgentError::Policy("prediction not in its own action space".into()))?;
        let target = offered
            .target()
            .cloned()
            .ok_or_else(|| AgentError::Guard("target-bound action missing region id".into()))?;

        let ticket = gate(
            &predicted.manifold,
            &target,
            action,
            predicted.focused.clone(),
            predicted.space_captured_at_ms,
        )
        .map_err(|reason| AgentError::Guard(format!("refuse: {reason}")))?;

        let input = match kind {
            ActionKind::Click => Input::Click,
            ActionKind::TypeText => Input::Type(
                predicted
                    .payload
                    .clone()
                    .ok_or_else(|| AgentError::Text("TYPE_TEXT without resolved text".into()))?,
            ),
            ActionKind::Select => Input::Select(
                predicted
                    .payload
                    .clone()
                    .ok_or_else(|| AgentError::Text("SELECT without resolved option".into()))?,
            ),
            _ => unreachable!("region_action filtered kinds"),
        };

        let executed = match execute_ticketed(&mut self.browser, &mut self.ledger, &ticket, &input)
        {
            Ok(executed) => executed,
            Err(err) if err.is_stale() => return Err(AgentError::Stale(err.to_string())),
            Err(ExecError::Rejected(msg)) => return Err(AgentError::InputRejected(msg)),
            Err(ExecError::Gate(reason)) => {
                return Err(AgentError::Guard(format!("refuse at executor: {reason}")))
            }
            Err(other) => return Err(AgentError::Ticket(other.to_string())),
        };

        self.browser.settle();
        let after = self.browser.observe()?.clone();
        let after_page = self.browser.page().cloned();
        let page_d = match (executed.before_page.as_ref(), after_page.as_ref()) {
            (Some(b), Some(a)) => Some(self.browser.page_delta_between(b, a)),
            _ => None,
        };
        let verification = match input.payload() {
            Some(expected) => {
                let value = self.browser.read_value(&executed.target);
                classify_value(expected, value.as_ref())
                    .unwrap_or_else(|| classify_delta(&executed.before, &after, page_d.as_ref()))
            }
            None => classify_delta(&executed.before, &after, page_d.as_ref()),
        };
        Ok(record(verification))
    }

    /// One predict+act cycle.
    pub fn tick(&mut self) -> Result<TickResult, AgentError> {
        match self.state {
            AgentState::Done => return Ok(TickResult::Finished(self.finish_done("done"))),
            AgentState::Blocked => {
                return Ok(TickResult::Finished(self.finish_blocked("blocked".into())))
            }
            AgentState::Ready | AgentState::Predicted => {}
        }

        if self.state == AgentState::Ready {
            match self.predict() {
                Ok(None) => {
                    if self.state == AgentState::Blocked {
                        return Ok(TickResult::Finished(
                            self.finish_blocked("policy chose BLOCKED".into()),
                        ));
                    }
                    // Single-intent clause satisfied. Multi-step: advance.
                    if self.advance_clause() {
                        return Ok(TickResult::ClauseAdvanced {
                            next_clause: self.goal.as_str().to_owned(),
                        });
                    }
                    return Ok(TickResult::Finished(self.finish_done("policy chose DONE")));
                }
                Ok(Some(_)) => {}
                Err(AgentError::Abstain(reason)) => {
                    return Ok(TickResult::Finished(AgentOutcome::Abstained {
                        steps: self.history.clone(),
                        reason: format!("abstain: {reason}"),
                    }));
                }
                Err(e) => return Err(e),
            }
        }

        match self.act() {
            Ok(record) => {
                if self.state == AgentState::Blocked {
                    return Ok(TickResult::Finished(self.finish_blocked(format!(
                        "{} consecutive no-effect/wrong-effect actions (last: {})",
                        self.consecutive_no_effect, record.label
                    ))));
                }
                Ok(TickResult::Stepped(record))
            }
            Err(AgentError::Stale(reason)) => {
                if self.consecutive_stale >= self.max_consecutive_stale {
                    return Err(AgentError::TooManyStale(self.consecutive_stale));
                }
                Ok(TickResult::StaleDiscarded { reason })
            }
            Err(e) => Err(e),
        }
    }

    pub fn run(&mut self) -> AgentOutcome {
        loop {
            if self.steps_taken >= self.max_steps {
                return AgentOutcome::Failed {
                    steps: self.history.clone(),
                    error: AgentError::MaxSteps.to_string(),
                };
            }
            match self.tick() {
                Ok(TickResult::Finished(outcome)) => return outcome,
                Ok(TickResult::Stepped(_))
                | Ok(TickResult::StaleDiscarded { .. })
                | Ok(TickResult::ClauseAdvanced { .. }) => continue,
                Err(e) => {
                    return AgentOutcome::Failed {
                        steps: self.history.clone(),
                        error: e.to_string(),
                    };
                }
            }
        }
    }

    fn finish_done(&self, reason: &str) -> AgentOutcome {
        AgentOutcome::Done {
            steps: self.history.clone(),
            reason: reason.to_owned(),
        }
    }

    fn finish_blocked(&self, reason: String) -> AgentOutcome {
        AgentOutcome::Blocked {
            steps: self.history.clone(),
            reason,
        }
    }
}

#[derive(Clone, Debug)]
pub enum TickResult {
    Stepped(StepRecord),
    StaleDiscarded {
        reason: String,
    },
    /// Multi-step goal moved to the next `then` clause; keep ticking.
    ClauseAdvanced {
        next_clause: String,
    },
    Finished(AgentOutcome),
}
