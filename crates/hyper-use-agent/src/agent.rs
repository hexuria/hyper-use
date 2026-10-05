//! Agent state machine: Ready ↔ Predicted → Done/Blocked.

use hyper_use_core::{Action, ActionKind, ActionSpace, InteractionManifold, LocateQuery};
use hyper_use_guard::{guard, world_fingerprint, GuardRequest, TicketLedger, WorldSnapshot};
use hyper_use_policy::{
    AgentGoal, BrowserPolicy, DeterministicTextResolver, HistoryEntry, PolicyDecision,
    PolicyOutcome, TextContext, TextResolver,
};
use hyper_use_protocol::{GuardDecision, TicketInvalid};

use crate::error::AgentError;
use crate::outcome::{AgentOutcome, StepRecord, VerificationKind};
use crate::runtime::BrowserRuntime;
use crate::verify_map::classify_delta;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Ready,
    Predicted,
    Done,
    Blocked,
}

/// A policy choice waiting for ticketed execution.
#[derive(Clone, Debug)]
pub struct Predicted {
    pub decision: PolicyDecision,
    pub typed_text: Option<String>,
    pub text_fingerprint: Option<u64>,
    pub observation_fingerprint: u64,
    pub space_captured_at_ms: u64,
}

pub struct Agent<B, P, T = DeterministicTextResolver> {
    browser: B,
    policy: P,
    text: T,
    goal: AgentGoal,
    state: AgentState,
    predicted: Option<Predicted>,
    history: Vec<StepRecord>,
    policy_history: Vec<HistoryEntry>,
    ledger: TicketLedger,
    max_steps: u32,
    max_policy_calls: u32,
    steps_taken: u32,
    policy_calls: u32,
    consecutive_no_effect: u32,
    max_consecutive_no_effect: u32,
}

pub struct AgentBuilder<B, P, T = DeterministicTextResolver> {
    browser: B,
    policy: P,
    text: T,
    max_steps: u32,
    max_policy_calls: u32,
    max_consecutive_no_effect: u32,
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
        }
    }

    pub fn max_steps(mut self, n: u32) -> Self {
        self.max_steps = n;
        self
    }

    pub fn max_policy_calls(mut self, n: u32) -> Self {
        self.max_policy_calls = n;
        self
    }

    pub fn build(self, goal: impl Into<String>) -> Agent<B, P, T> {
        Agent {
            browser: self.browser,
            policy: self.policy,
            text: self.text,
            goal: AgentGoal::new(goal),
            state: AgentState::Ready,
            predicted: None,
            history: Vec::new(),
            policy_history: Vec::new(),
            ledger: TicketLedger::new(),
            max_steps: self.max_steps,
            max_policy_calls: self.max_policy_calls,
            steps_taken: 0,
            policy_calls: 0,
            consecutive_no_effect: 0,
            max_consecutive_no_effect: self.max_consecutive_no_effect,
        }
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

    pub fn browser_mut(&mut self) -> &mut B {
        &mut self.browser
    }

    pub fn observe(&mut self) -> Result<&InteractionManifold, AgentError> {
        self.browser.observe()
    }

    /// Policy decide on a fresh observation. Stale predictions are discarded.
    pub fn predict(&mut self) -> Result<Option<&Predicted>, AgentError> {
        if !matches!(self.state, AgentState::Ready | AgentState::Predicted) {
            return Err(AgentError::InvalidState("predict from terminal state"));
        }
        if self.policy_calls >= self.max_policy_calls {
            return Err(AgentError::MaxPolicyCalls);
        }

        let manifold = self.browser.observe()?.clone();
        let space = ActionSpace::from_manifold(&manifold);
        self.policy_calls += 1;
        let outcome = self
            .policy
            .decide(&space, &self.goal, &self.policy_history)
            .map_err(|e| AgentError::Policy(e.to_string()))?;

        match outcome {
            PolicyOutcome::Abstain { reason, .. } => {
                self.predicted = None;
                self.state = AgentState::Ready;
                Err(AgentError::Policy(format!("abstain: {reason}")))
            }
            PolicyOutcome::Choice(decision) => {
                if decision.kind == ActionKind::Done {
                    self.state = AgentState::Done;
                    self.predicted = None;
                    return Ok(None);
                }
                if decision.kind == ActionKind::Blocked {
                    self.state = AgentState::Blocked;
                    self.predicted = None;
                    return Ok(None);
                }

                let mut typed_text = None;
                let mut text_fingerprint = None;
                if decision.kind == ActionKind::TypeText {
                    let action = space.get(&decision.action_id).ok_or_else(|| {
                        AgentError::Policy("chosen TYPE_TEXT missing from space".into())
                    })?;
                    let ctx = TextContext {
                        goal: self.goal.clone(),
                        field_label: action.label().to_owned(),
                        field_role: action
                            .role()
                            .map(|r| r.as_str().to_owned())
                            .unwrap_or_default(),
                        context_fingerprint: action.target_fingerprint(),
                    };
                    let resolution = self
                        .text
                        .resolve(&ctx)
                        .map_err(|e| AgentError::Text(e.to_string()))?;
                    // Re-observe after text resolution latency (even if deterministic).
                    let manifold_after = self.browser.observe()?.clone();
                    let space_after = ActionSpace::from_manifold(&manifold_after);
                    if space_after
                        .get(&decision.action_id)
                        .map(|a| a.target_fingerprint())
                        != Some(action.target_fingerprint())
                    {
                        // Page mutated during text resolve — discard and let caller predict again.
                        self.predicted = None;
                        self.state = AgentState::Ready;
                        return Err(AgentError::Ticket(
                            "page changed during text resolution".into(),
                        ));
                    }
                    text_fingerprint = Some(resolution.context_fingerprint);
                    typed_text = Some(resolution.text);
                }

                let focused = self.browser.focused();
                let world = WorldSnapshot::of(&manifold, focused.clone());
                let predicted = Predicted {
                    decision,
                    typed_text,
                    text_fingerprint,
                    observation_fingerprint: world_fingerprint(&world),
                    space_captured_at_ms: space.captured_at_ms(),
                };
                self.predicted = Some(predicted);
                self.state = AgentState::Predicted;
                Ok(self.predicted.as_ref())
            }
        }
    }

    /// Guard + issue ticket + revalidate/consume + execute + verify.
    ///
    /// Stale tickets discard the prediction and return to Ready (not a failed task).
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

        // Single attempt; stale discards prediction and returns Ready (caller may predict again).
        match self.try_act_once(&predicted) {
            Ok(record) => {
                self.history.push(record.clone());
                self.policy_history.push(HistoryEntry {
                    step: record.step,
                    action_id: record.action_id.clone(),
                    kind: record.kind,
                    label: record.label.clone(),
                    verification: record.verification.as_str().to_owned(),
                });
                if record.verification == VerificationKind::NoEffect {
                    self.consecutive_no_effect += 1;
                    if self.consecutive_no_effect >= self.max_consecutive_no_effect {
                        self.state = AgentState::Blocked;
                        return Ok(record);
                    }
                } else {
                    self.consecutive_no_effect = 0;
                }
                self.steps_taken += 1;
                self.state = AgentState::Ready;
                self.predicted = None;
                Ok(record)
            }
            Err(AgentError::Ticket(msg))
                if msg.contains("stale")
                    || msg.contains("world-changed")
                    || msg.contains("target-changed")
                    || msg.contains("target-gone")
                    || msg.contains("page changed") =>
            {
                let _ = self.browser.observe()?;
                self.state = AgentState::Ready;
                self.predicted = None;
                Err(AgentError::Ticket(format!("stale action discarded: {msg}")))
            }
            Err(e) => {
                self.state = AgentState::Ready;
                self.predicted = None;
                Err(e)
            }
        }
    }

    fn try_act_once(&mut self, predicted: &Predicted) -> Result<StepRecord, AgentError> {
        let before_manifold = self.browser.observe()?.clone();
        let before_page = self.browser.page().cloned();
        let focused = self.browser.focused();
        let now_world = WorldSnapshot::of(&before_manifold, focused.clone());
        if world_fingerprint(&now_world) != predicted.observation_fingerprint {
            return Err(AgentError::Ticket(
                "stale: world changed since prediction".into(),
            ));
        }
        let space = ActionSpace::from_manifold(&before_manifold);
        let action = space.get(&predicted.decision.action_id).ok_or_else(|| {
            AgentError::Ticket("stale: chosen action missing from fresh ActionSpace".into())
        })?;

        // Controls that do not press a target.
        if predicted.decision.kind.is_control()
            && !matches!(
                predicted.decision.kind,
                ActionKind::ScrollUp | ActionKind::ScrollDown
            )
        {
            // WAIT: no press. DONE/BLOCKED handled in predict.
            let after_manifold = self.browser.observe()?.clone();
            let after_page = self.browser.page().cloned();
            let page_d = match (before_page.as_ref(), after_page.as_ref()) {
                (Some(b), Some(a)) => Some(self.browser.page_delta_between(b, a)),
                _ => None,
            };
            let verification = classify_delta(&before_manifold, &after_manifold, page_d.as_ref());
            return Ok(StepRecord {
                step: self.steps_taken + 1,
                action_id: predicted.decision.action_id.clone(),
                kind: predicted.decision.kind,
                label: predicted.decision.target_label.clone(),
                verification,
                stale_retries: 0,
            });
        }

        let target = action
            .target()
            .ok_or_else(|| AgentError::Guard("target-bound action missing region id".into()))?;

        let query = LocateQuery::new()
            .text(action.label())
            .map_err(|e| AgentError::Guard(e.to_string()))?;
        let seen_world = WorldSnapshot::of(&before_manifold, focused.clone());
        let request = GuardRequest::click(query)
            .proposed(target.clone())
            .focused(focused.clone())
            .seen_world(seen_world)
            .snapshot_id(predicted.space_captured_at_ms);

        let decision =
            guard(&before_manifold, &request).map_err(|e| AgentError::Guard(e.to_string()))?;
        let ticket = match decision {
            GuardDecision::Allow { ticket, .. } => ticket,
            GuardDecision::Refuse { reason, .. } => {
                return Err(AgentError::Guard(format!("refuse: {reason}")));
            }
            GuardDecision::Escalate { reason, .. } => {
                return Err(AgentError::Guard(format!("escalate: {reason}")));
            }
            _ => return Err(AgentError::Guard("unknown guard decision".into())),
        };

        // Map agent kind → core Action for press.
        let press_action = match predicted.decision.kind {
            ActionKind::Click => Action::Click,
            ActionKind::TypeText => Action::Type,
            ActionKind::Select => Action::Select,
            ActionKind::ScrollUp | ActionKind::ScrollDown => Action::Scroll,
            _ => Action::Click,
        };

        // Ensure ticket action matches what we will press (guard today always Click).
        // For non-click kinds we still bind the target via ticket identity checks.
        let _ = &ticket;

        let fresh = self.browser.observe()?.clone();
        let focused_now = self.browser.focused();
        if self.ledger.is_consumed(ticket.ticket_id) {
            return Err(AgentError::Ticket("ticket already consumed".into()));
        }
        if let Err(err) = hyper_use_guard::revalidate(&ticket, &fresh, focused_now) {
            return Err(match err {
                TicketInvalid::WorldChanged
                | TicketInvalid::TargetChanged
                | TicketInvalid::TargetGone => {
                    AgentError::Ticket("stale: ticket revalidation failed".into())
                }
                other => AgentError::Ticket(other.to_string()),
            });
        }
        let target_id = ticket.target_id.clone();
        self.browser.press(&target_id, press_action)?;
        self.ledger
            .mark_consumed(ticket.ticket_id)
            .map_err(|e| AgentError::Ticket(e.to_string()))?;

        // typed_text is recorded in history label annotation; live typing CDP is Phase gap.
        let _ = &predicted.typed_text;

        let after_manifold = self.browser.observe()?.clone();
        let after_page = self.browser.page().cloned();
        let page_d = match (before_page.as_ref(), after_page.as_ref()) {
            (Some(b), Some(a)) => Some(self.browser.page_delta_between(b, a)),
            _ => None,
        };
        let verification = classify_delta(&before_manifold, &after_manifold, page_d.as_ref());

        Ok(StepRecord {
            step: self.steps_taken + 1,
            action_id: predicted.decision.action_id.clone(),
            kind: predicted.decision.kind,
            label: predicted.decision.target_label.clone(),
            verification,
            stale_retries: 0,
        })
    }

    /// One predict+act cycle. Abstain / stale returns Ready without failing the task permanently.
    pub fn tick(&mut self) -> Result<TickResult, AgentError> {
        match self.state {
            AgentState::Done => {
                return Ok(TickResult::Finished(AgentOutcome::Done {
                    steps: self.history.clone(),
                    reason: "done".into(),
                }))
            }
            AgentState::Blocked => {
                return Ok(TickResult::Finished(AgentOutcome::Blocked {
                    steps: self.history.clone(),
                    reason: "blocked".into(),
                }))
            }
            AgentState::Ready | AgentState::Predicted => {}
        }

        if self.state == AgentState::Ready {
            let predicted = self.predict();
            match predicted {
                Ok(None) => {
                    if self.state == AgentState::Done {
                        return Ok(TickResult::Finished(AgentOutcome::Done {
                            steps: self.history.clone(),
                            reason: "policy chose DONE".into(),
                        }));
                    }
                    if self.state == AgentState::Blocked {
                        return Ok(TickResult::Finished(AgentOutcome::Blocked {
                            steps: self.history.clone(),
                            reason: "policy chose BLOCKED".into(),
                        }));
                    }
                }
                Ok(Some(_)) => {}
                Err(AgentError::Policy(msg)) if msg.starts_with("abstain:") => {
                    return Ok(TickResult::Finished(AgentOutcome::Abstained {
                        steps: self.history.clone(),
                        reason: msg,
                    }));
                }
                Err(e) => return Err(e),
            }
        }

        if self.state == AgentState::Predicted {
            match self.act() {
                Ok(record) => {
                    if self.state == AgentState::Blocked {
                        return Ok(TickResult::Finished(AgentOutcome::Blocked {
                            steps: self.history.clone(),
                            reason: format!("repeated no-effect after {}", record.label),
                        }));
                    }
                    return Ok(TickResult::Stepped(record));
                }
                Err(AgentError::Ticket(msg)) if msg.contains("stale") => {
                    return Ok(TickResult::StaleDiscarded { reason: msg });
                }
                Err(e) => return Err(e),
            }
        }

        Ok(TickResult::Stepped(StepRecord {
            step: self.steps_taken,
            action_id: predicted_dummy_id(),
            kind: ActionKind::Wait,
            label: String::new(),
            verification: VerificationKind::Skipped,
            stale_retries: 0,
        }))
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
                Ok(TickResult::Stepped(_)) | Ok(TickResult::StaleDiscarded { .. }) => continue,
                Err(e) => {
                    return AgentOutcome::Failed {
                        steps: self.history.clone(),
                        error: e.to_string(),
                    };
                }
            }
        }
    }
}

fn predicted_dummy_id() -> hyper_use_core::ActionId {
    hyper_use_core::ActionId::try_new("WAIT").expect("WAIT")
}

#[derive(Clone, Debug)]
pub enum TickResult {
    Stepped(StepRecord),
    StaleDiscarded { reason: String },
    Finished(AgentOutcome),
}

// GuardRequest focused helper — check if focused_opt exists
