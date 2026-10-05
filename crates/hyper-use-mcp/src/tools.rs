//! The six tools. Each one is a single computer-capability call.
//!
//! `dispatch` rejects a goal, a navigate payload, and raw coordinates before
//! it reads a fixture. A scored act below the executor gate returns
//! `executed: false` and does not call `press`.
//!
//! Every observation is recorded in the server's snapshot ring and its id is
//! returned as `snapshot`. `act` observes before, presses, and (when asked, or
//! on a live session) observes after, diffs, and verifies in the same call.
//! `signals` reports a signature no-op or a loop. It does not retry.

use std::fs;
use std::path::Path;
use std::str::FromStr;

use hyper_use_browser::{
    page_delta, verify, verify_delta, BrowserSession, CdpTransport, Expectation, PageState,
    ReplayTransport, VerifyError,
};
use hyper_use_core::{
    parse_fixture, Action, InteractionManifold, LocateQuery, RegionId, Role, Zone,
};
use hyper_use_executor::{
    gate_confidence, select_act_executor, ActConfidence, ActionExecutor, ActionRequest,
    BrowserExecutor, BrowserUseError, BrowserUseExecutor, CuaError, CuaExecutor, ExecutorError,
    ExecutorKind, StubExecutor,
};
use hyper_use_hyper::Dims;
use hyper_use_observe::{
    diff,
    history::{SnapshotId, TemporalSignal},
    ManifoldDiff,
};
use hyper_use_protocol::{FallbackReason, MatcherConfidence, ProtocolError, StateDelta};
use hyper_use_resonance::{HgraMatcher, Match, RegionMatcher, ResonanceModel, WeightedMatcher};
use serde_json::{json, Value};

use crate::error::ToolError;
use crate::server::Server;

const PRODUCT: &str = "hyper-use";

enum Origin {
    Fixture(String),
    Cdp(String),
}

/// Run one tool against a fresh [`Server`]. Keeps no state between calls.
pub fn call_tool(name: &str, arguments: &Value) -> Result<Value, ToolError> {
    Server::new().call_tool(name, arguments)
}

pub(crate) fn dispatch(
    server: &mut Server,
    name: &str,
    arguments: &Value,
) -> Result<Value, ToolError> {
    let owned;
    let arguments = match arguments {
        Value::Null => {
            owned = Value::Object(serde_json::Map::new());
            &owned
        }
        Value::Object(_) => arguments,
        _ => {
            return Err(ToolError::InvalidArguments(
                "arguments must be an object".into(),
            ))
        }
    };
    if arguments.get("goal").is_some()
        || arguments.get("steps").is_some()
        || arguments.get("navigate").is_some()
    {
        return Err(ToolError::GoalNotAccepted);
    }
    if arguments.get("x").is_some()
        || arguments.get("y").is_some()
        || arguments.get("coordinates").is_some()
    {
        return Err(ToolError::CoordinatesNotAccepted);
    }
    match name {
        "navigate" => Err(ToolError::GoalNotAccepted),
        "observe" => observe(server, arguments),
        "locate" => locate(server, arguments),
        "inspect" => inspect(server, arguments),
        "act" => act(server, arguments),
        "diff" => diff_tool(server, arguments),
        "verify" => verify_tool(server, arguments),
        other => Err(ToolError::UnknownTool(other.to_owned())),
    }
}

fn observe(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let (snapshot, manifold, _page) = observe_origin(server, &resolve_origin(arguments)?)?;
    let regions: Vec<Value> = manifold
        .regions()
        .map(|region| {
            json!({
                "id": region.id().as_str(),
                "role": region.role().as_str(),
                "label": region.label(),
            })
        })
        .collect();
    let mut body = outcome(
        "observe",
        Value::Null,
        None,
        false,
        false,
        empty_delta(),
        None,
        None,
        None,
    );
    insert(&mut body, "regions", Value::Array(regions));
    insert(&mut body, "snapshot", json!(snapshot.get()));
    Ok(body)
}

fn locate(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let (snapshot, manifold, _page) = observe_origin(server, &resolve_origin(arguments)?)?;
    let query = build_query(arguments)?;
    let matcher_name = opt_str(arguments, "matcher")?.unwrap_or("weighted");
    if arguments.get("dims").is_some() && matcher_name != "hgra" {
        return Err(ToolError::DimsRequireHgra);
    }
    let ranked = rank(&manifold, &query, matcher_name, arguments)?;
    let top = ranked.first();
    let target = match top {
        Some(candidate) => target_of(&manifold, candidate.id()),
        None => Value::Null,
    };
    let confidence = top.map(Match::confidence);
    // Omit executed and verified. ComputerResult always sets both, so a locate
    // that carries executed: false looks like a refusal. This call does not press.
    let mut body = without_act_flags(outcome(
        "locate",
        target,
        None,
        false,
        false,
        empty_delta(),
        confidence,
        None,
        None,
    ));
    insert(&mut body, "matcher", json!(matcher_name));
    insert(&mut body, "benchmark", json!(false));
    insert(&mut body, "candidates", candidates_of(&manifold, &ranked));
    insert(&mut body, "snapshot", json!(snapshot.get()));
    Ok(body)
}

fn rank(
    manifold: &InteractionManifold,
    query: &LocateQuery,
    matcher_name: &str,
    arguments: &Value,
) -> Result<Vec<Match>, ToolError> {
    match matcher_name {
        "weighted" => WeightedMatcher::default()
            .rank(query, manifold)
            .map_err(|err| ToolError::Ranker(err.to_string())),
        "hgra" => {
            let dims = match opt_u64(arguments, "dims")? {
                None => Dims::DEFAULT,
                Some(width) => {
                    let width = usize::try_from(width)
                        .map_err(|_| ToolError::BadDims(width.to_string()))?;
                    Dims::try_from_usize(width)
                        .map_err(|_| ToolError::BadDims(width.to_string()))?
                }
            };
            HgraMatcher::new(dims, ResonanceModel::V1)
                .rank(query, manifold)
                .map_err(|err| ToolError::Ranker(err.to_string()))
        }
        other => Err(ToolError::UnknownMatcher(other.to_owned())),
    }
}

fn inspect(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let (snapshot, manifold, _page) = observe_origin(server, &resolve_origin(arguments)?)?;
    let region = require_region(arguments)?;
    let found = manifold
        .get(&region)
        .ok_or_else(|| ToolError::UnknownRegion(region.to_string()))?;
    let target = json!({
        "id": found.id().as_str(),
        "role": found.role().as_str(),
        "label": found.label(),
        "x": found.rect().x(),
        "y": found.rect().y(),
        "width": found.rect().width(),
        "height": found.rect().height(),
    });
    // Same omission as locate. Inspect reads one region and does not press.
    let mut body = without_act_flags(outcome(
        "inspect",
        target,
        None,
        false,
        false,
        empty_delta(),
        None,
        None,
        None,
    ));
    insert(&mut body, "snapshot", json!(snapshot.get()));
    Ok(body)
}

fn act(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let requested = parse_requested_executor(arguments)?;
    let available = match requested {
        Some(kind) => vec![kind],
        None => vec![ExecutorKind::Browser],
    };
    let selected = select_act_executor(requested, &available).map_err(|err| match err {
        ExecutorError::Unavailable(kind) => ToolError::NotImplemented {
            executor: kind.as_str().to_owned(),
        },
        other => ToolError::Browser(other.to_string()),
    })?;
    match selected {
        ExecutorKind::BrowserUse => act_browser_use(arguments),
        ExecutorKind::Cua => act_cua(arguments),
        ExecutorKind::Macos => act_stub(selected, arguments),
        ExecutorKind::Browser => act_browser(server, arguments),
        other => Err(ToolError::NotImplemented {
            executor: other.as_str().to_owned(),
        }),
    }
}

fn act_browser(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let origin = resolve_origin(arguments)?;
    let region = require_region(arguments)?;
    let _action = parse_press_action(arguments)?;
    let score = parse_act_score(arguments, &region)?;
    let query = act_query(arguments)?;
    if query.is_some() && score.is_some() {
        return Err(ToolError::ConfidenceWithQuery);
    }
    let expectation = parse_expectation(arguments)?;
    let observe_after = match arguments.get("observe_after") {
        None | Some(Value::Null) => expectation.is_some() || matches!(origin, Origin::Cdp(_)),
        Some(Value::Bool(flag)) => *flag,
        Some(_) => {
            return Err(ToolError::InvalidArguments(
                "observe_after must be a boolean".into(),
            ))
        }
    };
    if expectation.is_some() && !observe_after {
        return Err(ToolError::ExpectNeedsObserveAfter);
    }
    let plan = ActPlan {
        key: origin_key(&origin),
        region,
        score,
        query,
        expectation,
        observe_after,
        matcher: opt_str(arguments, "matcher")?
            .unwrap_or("weighted")
            .to_owned(),
        arguments: arguments.clone(),
    };
    match &origin {
        Origin::Fixture(path) => {
            let body = read_cdp_script(path)?;
            let transport =
                ReplayTransport::parse(&body).map_err(|err| ToolError::Browser(err.to_string()))?;
            run_act(server, BrowserSession::new(transport), &plan).0
        }
        Origin::Cdp(url) => {
            let session = server.take_session(url)?;
            let (result, session, healthy) = run_act(server, session, &plan);
            if healthy {
                server.keep_session(url, session);
            }
            result
        }
    }
}

struct ActPlan {
    key: String,
    region: RegionId,
    score: Option<ActScore>,
    query: Option<LocateQuery>,
    expectation: Option<Expectation>,
    observe_after: bool,
    matcher: String,
    arguments: Value,
}

/// Observe (or reuse the session's latest observation), gate, press, and
/// optionally observe again, diff, and verify. Returns the session and whether
/// its transport is still usable.
fn run_act<T: CdpTransport>(
    server: &mut Server,
    mut session: BrowserSession<T>,
    plan: &ActPlan,
) -> (Result<Value, ToolError>, BrowserSession<T>, bool) {
    let reuse = session
        .manifold()
        .is_some()
        .then(|| server.latest_for(&plan.key))
        .flatten();
    let (before_id, before, before_page) = match reuse {
        Some(id) => {
            let manifold = session.manifold().expect("checked above").clone();
            let page = session.page().cloned().unwrap_or_else(PageState::blank);
            (id, manifold, page)
        }
        None => match session.observe() {
            Ok(manifold) => {
                let manifold = manifold.clone();
                let page = session.page().cloned().unwrap_or_else(PageState::blank);
                (
                    server.record(&plan.key, manifold.clone(), page.clone()),
                    manifold,
                    page,
                )
            }
            Err(err) => return (Err(ToolError::Browser(err.to_string())), session, false),
        },
    };
    if before.get(&plan.region).is_none() {
        return (
            Err(ToolError::UnknownRegion(plan.region.to_string())),
            session,
            true,
        );
    }
    let score = match ranked_score(&before, plan) {
        Ok(score) => score,
        Err(err) => return (Err(err), session, true),
    };
    if let Some(score) = &score {
        if let Err(err) = gate_confidence(score.confidence()) {
            let refused = refusal(err, target_of(&before, &plan.region), score, Some(&before)).map(
                |mut body| {
                    insert(&mut body, "before_snapshot", json!(before_id.get()));
                    insert(&mut body, "after_snapshot", Value::Null);
                    insert(&mut body, "signals", json!([]));
                    body
                },
            );
            return (refused, session, true);
        }
    }
    let mut request = ActionRequest::new(plan.region.clone(), Action::Click);
    if let Some(score) = &score {
        request = score.apply(request);
    }
    let mut executor = BrowserExecutor::new(session);
    let executed = executor.execute(&request);
    let mut session = executor.into_session();
    let receipt = match executed {
        Ok(receipt) => receipt,
        Err(err) => return (Err(map_executor(err)), session, false),
    };
    let mut verified = false;
    let mut fallback = None;
    let mut verify_error = None;
    let mut delta = empty_delta();
    let mut after_id: Option<SnapshotId> = None;
    if plan.observe_after {
        let after = match session.observe() {
            Ok(manifold) => manifold.clone(),
            Err(err) => return (Err(ToolError::Browser(err.to_string())), session, false),
        };
        let after_page = session.page().cloned().unwrap_or_else(PageState::blank);
        after_id = Some(server.record(&plan.key, after.clone(), after_page.clone()));
        let regions = diff(&before, &after);
        let pages = page_delta(&before_page, &after_page);
        delta = delta_json(&regions, &pages);
        if let Some(expectation) = &plan.expectation {
            match verify(&after, expectation) {
                Ok(()) => verified = true,
                Err(err) => {
                    fallback = Some(FallbackReason::VerifyFailed.as_str());
                    verify_error = Some(verify_tool_error(err).to_value());
                }
            }
        } else if let Err(VerifyError::NoEffect) =
            verify_delta(&regions, &pages, &Expectation::url_changed())
        {
            fallback = Some(FallbackReason::NoEffect.as_str());
        }
    }
    let mut body = outcome(
        "act",
        target_of(&before, &plan.region),
        Some(Action::Click.as_str()),
        true,
        verified,
        delta,
        score.as_ref().map(|s| s.top),
        fallback,
        Some("browser"),
    );
    insert(&mut body, "mechanism", json!(receipt.mechanism().as_str()));
    insert(&mut body, "before_snapshot", json!(before_id.get()));
    insert(
        &mut body,
        "after_snapshot",
        after_id.map_or(Value::Null, |id| json!(id.get())),
    );
    if let Some(error) = verify_error {
        insert(&mut body, "verify_error", error);
    }
    let signals = match after_id {
        Some(after) => match server.temporal_signals(before_id, after) {
            Ok(signals) => signals,
            Err(err) => return (Err(ToolError::Browser(err.to_string())), session, true),
        },
        None => Vec::new(),
    };
    insert(&mut body, "signals", signals_json(&signals));
    (Ok(body), session, true)
}

/// With locate fields, rank `before` with the same matcher locate uses and
/// require `region` to be first. Otherwise the caller's score.
fn ranked_score(
    before: &InteractionManifold,
    plan: &ActPlan,
) -> Result<Option<ActScore>, ToolError> {
    let Some(query) = &plan.query else {
        return Ok(plan.score.as_ref().map(|score| ActScore {
            top: score.top,
            runner_up: score.runner_up.clone(),
        }));
    };
    let ranked = rank(before, query, &plan.matcher, &plan.arguments)?;
    let Some(top) = ranked.first() else {
        return Err(ToolError::UnknownRegion(plan.region.to_string()));
    };
    if top.id() != &plan.region {
        return Err(ToolError::TargetNotTop {
            region: plan.region.to_string(),
            top: top.id().to_string(),
        });
    }
    Ok(Some(ActScore {
        top: top.confidence(),
        runner_up: ranked
            .get(1)
            .map(|second| (second.id().clone(), second.confidence())),
    }))
}

/// Locate fields on `act`: text, role, position. `action` is the press verb
/// here, so it is not a locate field.
fn act_query(arguments: &Value) -> Result<Option<LocateQuery>, ToolError> {
    let text = opt_str(arguments, "text")?;
    let role = opt_str(arguments, "role")?;
    let position = opt_str(arguments, "position")?;
    if text.is_none() && role.is_none() && position.is_none() {
        return Ok(None);
    }
    let mut query = LocateQuery::new();
    if let Some(text) = text {
        query = query.text(text).map_err(|_| ToolError::EmptyText)?;
    }
    if let Some(role) = role {
        let parsed = Role::parse(&role.to_ascii_lowercase())
            .ok_or_else(|| ToolError::UnknownRole(role.to_owned()))?;
        query = query.role(parsed);
    }
    if let Some(position) = position {
        let parsed = Zone::parse(&position.to_ascii_lowercase())
            .ok_or_else(|| ToolError::UnknownPosition(position.to_owned()))?;
        query = query.position(parsed);
    }
    Ok(Some(query))
}

fn parse_expectation(arguments: &Value) -> Result<Option<Expectation>, ToolError> {
    let expect_text = opt_str(arguments, "expect_text")?;
    let expect_absent = opt_str(arguments, "expect_absent")?;
    match (expect_text, expect_absent) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(ToolError::BothExpectations),
        (Some(text), None) => Expectation::text_present(text)
            .map(Some)
            .map_err(|_| ToolError::EmptyText),
        (None, Some(id)) => RegionId::try_new(id)
            .map(|id| Some(Expectation::region_absent(id)))
            .map_err(|_| ToolError::UnknownRegion(id.to_owned())),
    }
}

fn verify_tool_error(err: VerifyError) -> ToolError {
    match err {
        VerifyError::ExpectedTextMissing { expected } => {
            ToolError::ExpectedTextMissing { expected }
        }
        VerifyError::RegionStillPresent { id } => ToolError::RegionStillPresent { id },
        VerifyError::EmptyExpectation => ToolError::EmptyText,
        other => ToolError::Browser(other.to_string()),
    }
}

fn delta_json(delta: &ManifoldDiff, page: &hyper_use_browser::PageDelta) -> Value {
    let changed: Vec<hyper_use_core::RegionId> = delta
        .changed()
        .iter()
        .map(|change| change.id().clone())
        .collect();
    let state = StateDelta::new(delta.added().to_vec(), delta.removed().to_vec(), changed)
        .with_moved(delta.moved().cloned().collect())
        .with_text_changed(delta.relabeled().cloned().collect())
        .with_focus_changed(page.focus_changed())
        .with_url_changed(page.url_changed());
    json!({
        "added": id_strings(state.added()),
        "removed": id_strings(state.removed()),
        "changed": id_strings(state.changed()),
        "moved": id_strings(state.moved()),
        "text_changed": id_strings(state.text_changed()),
        "focus_changed": state.focus_changed(),
        "url_changed": state.url_changed(),
    })
}

/// Confidence the caller passed for an act. `None` means inspected.
struct ActScore {
    top: f64,
    runner_up: Option<(RegionId, f64)>,
}

impl ActScore {
    fn confidence(&self) -> ActConfidence {
        match &self.runner_up {
            None => ActConfidence::Scored(self.top),
            Some((_, runner_up)) => ActConfidence::Ranked {
                top: self.top,
                runner_up: *runner_up,
            },
        }
    }

    fn apply(&self, request: ActionRequest) -> ActionRequest {
        match &self.runner_up {
            None => request.scored(self.top),
            Some((_, runner_up)) => request.ranked(self.top, *runner_up),
        }
    }
}

fn parse_act_score(arguments: &Value, region: &RegionId) -> Result<Option<ActScore>, ToolError> {
    let top = parse_confidence(arguments)?;
    let runner_up = match arguments.get("runner_up") {
        None | Some(Value::Null) => None,
        Some(Value::Object(object)) => {
            let id = object
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::BadRunnerUp("id must be a string".into()))?;
            let id = RegionId::try_new(id)
                .map_err(|_| ToolError::BadRunnerUp(format!("bad region id `{id}`")))?;
            if &id == region {
                return Err(ToolError::RunnerUpIsTarget);
            }
            let raw = object
                .get("confidence")
                .ok_or_else(|| ToolError::BadRunnerUp("confidence must be a number".into()))?;
            let confidence = raw
                .as_f64()
                .ok_or_else(|| ToolError::BadRunnerUp("confidence must be a number".into()))?;
            Some((id, unit_confidence(confidence, &raw.to_string())?))
        }
        Some(_) => return Err(ToolError::BadRunnerUp("runner_up must be an object".into())),
    };
    match (top, runner_up) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ToolError::RunnerUpNeedsConfidence),
        (Some(top), runner_up) => Ok(Some(ActScore { top, runner_up })),
    }
}

/// A gate refusal as a successful tool result with `executed: false`.
fn refusal(
    err: ExecutorError,
    target: Value,
    score: &ActScore,
    manifold: Option<&InteractionManifold>,
) -> Result<Value, ToolError> {
    match err {
        ExecutorError::ConfidenceBelowThreshold { .. } => Ok(refused_target(target, score.top)),
        ExecutorError::AmbiguousTarget { margin_millis, .. } => {
            let mut body = refused_with(target, score.top, "ambiguous");
            if let Some((id, confidence)) = &score.runner_up {
                let mut runner = match manifold {
                    Some(manifold) if manifold.get(id).is_some() => target_of(manifold, id),
                    _ => json!({"id": id.as_str()}),
                };
                insert(&mut runner, "confidence", json!(confidence));
                insert(&mut body, "runner_up", runner);
            }
            insert(&mut body, "margin_millis", json!(margin_millis));
            Ok(body)
        }
        ExecutorError::NonFiniteConfidence => Err(ToolError::NonFiniteConfidence),
        other => Err(map_executor(other)),
    }
}

fn diff_tool(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let before_path = opt_str(arguments, "before")?;
    let after_path = opt_str(arguments, "after")?;
    let before_id = opt_snapshot(arguments, "before_snapshot")?;
    let after_id = opt_snapshot(arguments, "after_snapshot")?;
    let by_path = before_path.is_some() || after_path.is_some();
    let by_id = before_id.is_some() || after_id.is_some();
    let (delta, pages) = match (by_path, by_id) {
        (true, true) => return Err(ToolError::MixedDiffSources),
        (false, true) => {
            let before_id = before_id.ok_or(ToolError::MissingBefore)?;
            let after_id = after_id.ok_or(ToolError::MissingAfter)?;
            let (before_m, before_p, after_m, after_p) = {
                let before = server.snapshot(before_id)?;
                let after = server.snapshot(after_id)?;
                (
                    before.manifold.clone(),
                    before.page.clone(),
                    after.manifold.clone(),
                    after.page.clone(),
                )
            };
            (diff(&before_m, &after_m), page_delta(&before_p, &after_p))
        }
        _ => {
            let (before_m, before_p) =
                load_observation(before_path.ok_or(ToolError::MissingBefore)?)?;
            let (after_m, after_p) = load_observation(after_path.ok_or(ToolError::MissingAfter)?)?;
            (diff(&before_m, &after_m), page_delta(&before_p, &after_p))
        }
    };
    Ok(outcome(
        "diff",
        Value::Null,
        None,
        false,
        false,
        delta_json(&delta, &pages),
        None,
        None,
        None,
    ))
}

fn opt_snapshot(arguments: &Value, key: &str) -> Result<Option<u64>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| ToolError::BadSnapshot(number.to_string())),
        Some(other) => Err(ToolError::BadSnapshot(other.to_string())),
    }
}

fn verify_tool(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let (_snapshot, manifold, _page) = observe_origin(server, &resolve_origin(arguments)?)?;
    let expect_text = opt_str(arguments, "expect_text")?;
    let expect_absent = opt_str(arguments, "expect_absent")?;
    match (expect_text, expect_absent) {
        (None, None) => Err(ToolError::MissingExpect),
        (Some(_), Some(_)) => Err(ToolError::BothExpectations),
        (Some(text), None) => verify_text(&manifold, text),
        (None, Some(id)) => verify_absent(&manifold, id),
    }
}

fn verify_text(manifold: &InteractionManifold, text: &str) -> Result<Value, ToolError> {
    let expectation = Expectation::text_present(text).map_err(|err| match err {
        VerifyError::EmptyExpectation => ToolError::EmptyText,
        other => ToolError::Browser(other.to_string()),
    })?;
    match verify(manifold, &expectation) {
        Ok(()) => Ok(verified_ok()),
        Err(VerifyError::ExpectedTextMissing { expected }) => {
            Err(ToolError::ExpectedTextMissing { expected })
        }
        Err(other) => Err(ToolError::Browser(other.to_string())),
    }
}

fn verify_absent(manifold: &InteractionManifold, id: &str) -> Result<Value, ToolError> {
    let region = RegionId::try_new(id).map_err(|_| ToolError::UnknownRegion(id.to_owned()))?;
    match verify(manifold, &Expectation::region_absent(region)) {
        Ok(()) => Ok(verified_ok()),
        Err(VerifyError::RegionStillPresent { id }) => Err(ToolError::RegionStillPresent { id }),
        Err(other) => Err(ToolError::Browser(other.to_string())),
    }
}

fn verified_ok() -> Value {
    outcome(
        "verify",
        Value::Null,
        None,
        false,
        true,
        empty_delta(),
        None,
        None,
        None,
    )
}

fn act_browser_use(arguments: &Value) -> Result<Value, ToolError> {
    if opt_str(arguments, "cdp")?.is_some() {
        return Err(ToolError::BrowserUseIsReplay);
    }
    let region = require_region(arguments)?;
    let _action = parse_press_action(arguments)?;
    let score = parse_act_score(arguments, &region)?;
    let path = opt_str(arguments, "fixture")?.ok_or(ToolError::MissingFixture)?;
    let body = read_path(path)?;
    let mut executor = BrowserUseExecutor::from_replay(&body).map_err(map_browser_use_script)?;
    if executor.target().region_id() != &region {
        return Err(ToolError::UnknownRegion(region.to_string()));
    }
    let mut request = ActionRequest::new(region.clone(), Action::Click);
    if let Some(score) = &score {
        request = score.apply(request);
    }
    let target = json!({
        "id": executor.target().region_id().as_str(),
        "role": executor.target().role().as_str(),
        "label": executor.target().label(),
    });
    match executor.execute(&request) {
        Ok(receipt) => {
            let mut body = outcome(
                "act",
                target,
                Some(Action::Click.as_str()),
                true,
                false,
                empty_delta(),
                score.as_ref().map(|s| s.top),
                None,
                Some(receipt.kind().as_str()),
            );
            insert(&mut body, "mechanism", json!(receipt.mechanism().as_str()));
            Ok(body)
        }
        Err(err) => match &score {
            Some(score) => refusal(err, target, score, None),
            None => Err(map_executor(err)),
        },
    }
}

fn act_stub(kind: ExecutorKind, arguments: &Value) -> Result<Value, ToolError> {
    let region = require_region(arguments)?;
    let _action = parse_press_action(arguments)?;
    let score = parse_act_score(arguments, &region)?;
    let mut request = ActionRequest::new(region.clone(), Action::Click);
    if let Some(score) = &score {
        request = score.apply(request);
    }
    let mut stub = StubExecutor::new(kind);
    match stub.execute(&request) {
        Ok(_) => Err(ToolError::Browser(
            "unimplemented executor returned a receipt".into(),
        )),
        Err(err) => match &score {
            Some(score) => refusal(err, json!({"id": region.as_str()}), score, None),
            None => Err(map_executor(err)),
        },
    }
}

fn act_cua(arguments: &Value) -> Result<Value, ToolError> {
    if opt_str(arguments, "cdp")?.is_some() {
        return Err(ToolError::CuaIsReplay);
    }
    let region = require_region(arguments)?;
    let _action = parse_press_action(arguments)?;
    let score = parse_act_score(arguments, &region)?;
    let path = opt_str(arguments, "fixture")?.ok_or(ToolError::MissingFixture)?;
    let body = read_path(path)?;
    let mut executor = CuaExecutor::from_replay(&body).map_err(map_cua_script)?;
    if executor.target().region_id() != &region {
        return Err(ToolError::UnknownRegion(region.to_string()));
    }
    let mut request = ActionRequest::new(region.clone(), Action::Click);
    if let Some(score) = &score {
        request = score.apply(request);
    }
    let target = json!({
        "id": executor.target().region_id().as_str(),
        "role": executor.target().role().as_str(),
        "label": executor.target().label(),
    });
    match executor.execute(&request) {
        Ok(receipt) => {
            let mut body = outcome(
                "act",
                target,
                Some(Action::Click.as_str()),
                true,
                false,
                empty_delta(),
                score.as_ref().map(|s| s.top),
                None,
                Some(receipt.kind().as_str()),
            );
            insert(&mut body, "mechanism", json!(receipt.mechanism().as_str()));
            Ok(body)
        }
        Err(err) => match &score {
            Some(score) => refusal(err, target, score, None),
            None => Err(map_executor(err)),
        },
    }
}

fn map_browser_use_script(err: BrowserUseError) -> ToolError {
    match err {
        BrowserUseError::BadScript { message } => ToolError::BrowserUseScript { message },
        other => ToolError::BrowserUseScript {
            message: other.to_string(),
        },
    }
}

fn map_cua_script(err: CuaError) -> ToolError {
    match err {
        CuaError::BadScript { message } => ToolError::CuaScript { message },
        other => ToolError::CuaScript {
            message: other.to_string(),
        },
    }
}

fn refused_target(target: Value, score: f64) -> Value {
    refused_with(target, score, "low-confidence")
}

fn refused_with(target: Value, score: f64, fallback: &str) -> Value {
    let mut body = outcome(
        "act",
        target,
        Some(Action::Click.as_str()),
        false,
        false,
        empty_delta(),
        Some(score),
        Some(fallback),
        None,
    );
    insert(&mut body, "mechanism", Value::Null);
    body
}

fn parse_requested_executor(arguments: &Value) -> Result<Option<ExecutorKind>, ToolError> {
    match opt_str(arguments, "executor")? {
        None => Ok(None),
        Some(name) => ExecutorKind::parse(name)
            .map(Some)
            .ok_or_else(|| ToolError::UnknownExecutor(name.to_owned())),
    }
}

fn map_executor(err: ExecutorError) -> ToolError {
    match err {
        ExecutorError::NonFiniteConfidence => ToolError::NonFiniteConfidence,
        ExecutorError::NotImplemented(kind) => ToolError::NotImplemented {
            executor: kind.as_str().to_owned(),
        },
        ExecutorError::Browser(hyper_use_browser::BrowserError::UnknownRegion(id)) => {
            ToolError::UnknownRegion(id)
        }
        ExecutorError::Browser(hyper_use_browser::BrowserError::UnsupportedAction(action)) => {
            ToolError::UnsupportedAction(action)
        }
        ExecutorError::BrowserUse(err) => match err {
            BrowserUseError::UnknownRegion(id) => ToolError::UnknownRegion(id),
            BrowserUseError::UnsupportedAction(action) => ToolError::UnsupportedAction(action),
            BrowserUseError::Rejected { message } => ToolError::BrowserUseRejected { message },
            BrowserUseError::BadScript { message }
            | BrowserUseError::ParamsMismatch { message } => {
                ToolError::BrowserUseScript { message }
            }
            other => ToolError::BrowserUseScript {
                message: other.to_string(),
            },
        },
        ExecutorError::Cua(err) => match err {
            CuaError::UnknownRegion(id) => ToolError::UnknownRegion(id),
            CuaError::UnsupportedAction(action) => ToolError::UnsupportedAction(action),
            CuaError::Rejected { message } => ToolError::CuaRejected { message },
            CuaError::BadScript { message } | CuaError::ParamsMismatch { message } => {
                ToolError::CuaScript { message }
            }
            other => ToolError::CuaScript {
                message: other.to_string(),
            },
        },
        ExecutorError::ConfidenceBelowThreshold { .. } => ToolError::Browser(err.to_string()),
        other => ToolError::Browser(other.to_string()),
    }
}

fn resolve_origin(arguments: &Value) -> Result<Origin, ToolError> {
    let fixture = opt_str(arguments, "fixture")?;
    let cdp = opt_str(arguments, "cdp")?;
    match (fixture, cdp) {
        (Some(_), Some(_)) => Err(ToolError::DuplicateSource),
        (None, None) => Err(ToolError::MissingFixture),
        (Some(path), None) => Ok(Origin::Fixture(path.to_owned())),
        (None, Some(url)) => Ok(Origin::Cdp(url.to_owned())),
    }
}

fn origin_key(origin: &Origin) -> String {
    match origin {
        Origin::Fixture(path) => format!("fixture:{path}"),
        Origin::Cdp(url) => format!("cdp:{url}"),
    }
}

/// Observe `origin` and record it in the server's ring. A live session is kept
/// for the next call; a failed live call drops it.
fn observe_origin(
    server: &mut Server,
    origin: &Origin,
) -> Result<(SnapshotId, InteractionManifold, PageState), ToolError> {
    let (manifold, page) = match origin {
        Origin::Fixture(path) => load_observation(path)?,
        Origin::Cdp(url) => {
            let mut session = server.take_session(url)?;
            let manifold = session
                .observe()
                .cloned()
                .map_err(|err| ToolError::Browser(err.to_string()))?;
            let page = session.page().cloned().unwrap_or_else(PageState::blank);
            server.keep_session(url, session);
            (manifold, page)
        }
    };
    let id = server.record(&origin_key(origin), manifold.clone(), page.clone());
    Ok((id, manifold, page))
}

fn load_observation(path: &str) -> Result<(InteractionManifold, PageState), ToolError> {
    let body = read_path(path)?;
    if body.trim_start().starts_with('{') {
        let transport =
            ReplayTransport::parse(&body).map_err(|err| ToolError::Browser(err.to_string()))?;
        let mut session = BrowserSession::new(transport);
        let manifold = session
            .observe()
            .cloned()
            .map_err(|err| ToolError::Browser(err.to_string()))?;
        let page = session.page().cloned().unwrap_or_else(PageState::blank);
        Ok((manifold, page))
    } else {
        parse_fixture(&body)
            .map(|manifold| (manifold, PageState::blank()))
            .map_err(|err| ToolError::Fixture(err.to_string()))
    }
}

fn read_cdp_script(path: &str) -> Result<String, ToolError> {
    let body = read_path(path)?;
    if !body.trim_start().starts_with('{') {
        return Err(ToolError::ActNeedsCdp);
    }
    Ok(body)
}

fn read_path(path: &str) -> Result<String, ToolError> {
    fs::read_to_string(Path::new(path)).map_err(|err| ToolError::Io {
        path: path.to_owned(),
        message: err.to_string(),
    })
}

fn require_region(arguments: &Value) -> Result<RegionId, ToolError> {
    let raw = opt_str(arguments, "region")?.ok_or(ToolError::MissingRegion)?;
    RegionId::try_new(raw).map_err(|_| ToolError::UnknownRegion(raw.to_owned()))
}

fn parse_press_action(arguments: &Value) -> Result<Action, ToolError> {
    let raw = opt_str(arguments, "action")?.unwrap_or("press");
    match raw.to_ascii_lowercase().as_str() {
        "press" | "click" => Ok(Action::Click),
        other => match Action::parse(other) {
            Some(Action::Click) => Ok(Action::Click),
            Some(action) => Err(ToolError::UnsupportedAction(action.as_str().to_owned())),
            None => Err(ToolError::UnknownAction(raw.to_owned())),
        },
    }
}

fn parse_confidence(arguments: &Value) -> Result<Option<f64>, ToolError> {
    match arguments.get("confidence") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let value = number
                .as_f64()
                .ok_or_else(|| ToolError::BadConfidence(number.to_string()))?;
            unit_confidence(value, &number.to_string()).map(Some)
        }
        Some(Value::String(text)) => {
            let value = f64::from_str(text).map_err(|_| ToolError::BadConfidence(text.clone()))?;
            unit_confidence(value, text).map(Some)
        }
        Some(other) => Err(ToolError::BadConfidence(other.to_string())),
    }
}

/// A caller confidence must be finite and in `[0, 1]`. `raw` is the text the
/// caller sent, for the error.
fn unit_confidence(value: f64, raw: &str) -> Result<f64, ToolError> {
    match MatcherConfidence::try_unit(value) {
        Ok(confidence) => Ok(confidence.get()),
        Err(ProtocolError::ConfidenceOutOfRange) => {
            Err(ToolError::ConfidenceOutOfRange(raw.to_owned()))
        }
        Err(_) => Err(ToolError::NonFiniteConfidence),
    }
}

fn build_query(arguments: &Value) -> Result<LocateQuery, ToolError> {
    let mut query = LocateQuery::new();
    if let Some(text) = opt_str(arguments, "text")? {
        query = query.text(text).map_err(|_| ToolError::EmptyText)?;
    }
    if let Some(role) = opt_str(arguments, "role")? {
        let parsed = Role::parse(&role.to_ascii_lowercase())
            .ok_or_else(|| ToolError::UnknownRole(role.to_owned()))?;
        query = query.role(parsed);
    }
    if let Some(position) = opt_str(arguments, "position")? {
        let parsed = Zone::parse(&position.to_ascii_lowercase())
            .ok_or_else(|| ToolError::UnknownPosition(position.to_owned()))?;
        query = query.position(parsed);
    }
    if let Some(action) = opt_str(arguments, "action")? {
        let parsed = Action::parse(&action.to_ascii_lowercase())
            .ok_or_else(|| ToolError::UnknownAction(action.to_owned()))?;
        query = query.action(parsed);
    }
    Ok(query)
}

fn opt_str<'a>(arguments: &'a Value, key: &str) -> Result<Option<&'a str>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(ToolError::InvalidArguments(format!(
            "{key} must be a string"
        ))),
    }
}

fn opt_u64(arguments: &Value, key: &str) -> Result<Option<u64>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| ToolError::BadDims(number.to_string())),
        Some(other) => Err(ToolError::BadDims(other.to_string())),
    }
}

#[allow(clippy::too_many_arguments)]
fn outcome(
    tool: &str,
    target: Value,
    action: Option<&str>,
    executed: bool,
    verified: bool,
    state_delta: Value,
    confidence: Option<f64>,
    fallback: Option<&str>,
    executor: Option<&str>,
) -> Value {
    json!({
        "product": PRODUCT,
        "tool": tool,
        "target": target,
        "action": action,
        "executed": executed,
        "verified": verified,
        "state_delta": state_delta,
        "confidence": confidence,
        "fallback": fallback,
        "executor": executor,
    })
}

fn without_act_flags(mut body: Value) -> Value {
    if let Some(object) = body.as_object_mut() {
        object.remove("executed");
        object.remove("verified");
    }
    body
}

fn insert(body: &mut Value, key: &str, value: Value) {
    body.as_object_mut()
        .expect("tool result is a JSON object")
        .insert(key.to_owned(), value);
}

fn signals_json(signals: &[TemporalSignal]) -> Value {
    Value::Array(
        signals
            .iter()
            .map(|signal| match signal {
                TemporalSignal::NoOp => json!({"kind": "no-op"}),
                TemporalSignal::LoopDetected { matches } => json!({
                    "kind": "loop-detected",
                    "matches": matches.iter().map(|id| id.get()).collect::<Vec<_>>(),
                }),
            })
            .collect(),
    )
}

fn empty_delta() -> Value {
    json!({"added": [], "removed": [], "changed": []})
}

fn target_of(manifold: &InteractionManifold, id: &RegionId) -> Value {
    match manifold.get(id) {
        Some(region) => json!({
            "id": region.id().as_str(),
            "role": region.role().as_str(),
            "label": region.label(),
        }),
        None => json!({"id": id.as_str()}),
    }
}

fn candidates_of(manifold: &InteractionManifold, ranked: &[Match]) -> Value {
    let rows: Vec<Value> = ranked
        .iter()
        .map(|candidate| {
            let region = manifold.get(candidate.id());
            json!({
                "rank": candidate.rank(),
                "id": candidate.id().as_str(),
                "role": region.map(|region| region.role().as_str()).unwrap_or("unknown"),
                "label": region.map(|region| region.label()).unwrap_or(""),
                "confidence": candidate.confidence(),
            })
        })
        .collect();
    Value::Array(rows)
}

fn id_strings(ids: &[RegionId]) -> Vec<&str> {
    ids.iter().map(RegionId::as_str).collect()
}
