//! observe, act, verify, diff, and inspect. Locate stays in the parent module.
//!
//! `--cdp` with no value uses [`hyper_use_browser::DEFAULT_CDP_HTTP`].
//! A fixture whose first non-whitespace byte is `{` is a CDP replay script.
//! Anything else is a manifold fixture. Live Chrome is only contacted when
//! `--cdp` is set.

use std::fs;
use std::path::Path;
use std::str::FromStr;

use hyper_use_browser::{
    BrowserSession, Expectation, ReplayTransport, WebSocketTransport, DEFAULT_CDP_HTTP,
};
use hyper_use_core::{parse_fixture, InteractionManifold, RegionId};
use hyper_use_executor::{
    select_act_executor, ActionExecutor, ActionRequest, BrowserExecutor, BrowserUseError,
    BrowserUseExecutor, CuaError, CuaExecutor, ExecutorError, ExecutorKind, StubExecutor,
};
use hyper_use_observe::diff;

use crate::{set_once, CliError};

pub(crate) fn load_source(
    fixture: Option<&str>,
    cdp: Option<&str>,
) -> Result<InteractionManifold, CliError> {
    if fixture.is_some() && cdp.is_some() {
        return Err(CliError::DuplicateFlag("--cdp"));
    }
    if let Some(url) = cdp {
        let mut session = BrowserSession::new(connect_live(url)?);
        return session
            .observe()
            .cloned()
            .map_err(|err| CliError::Browser(err.to_string()));
    }
    let Some(path) = fixture else {
        return Err(CliError::MissingSource);
    };
    load_fixture(path)
}

pub(crate) fn observe_command(args: &[String]) -> Result<String, CliError> {
    let source = parse_source(args)?;
    let manifold = load_source(source.fixture.as_deref(), source.cdp.as_deref())?;
    Ok(render_regions(&manifold))
}

pub(crate) fn inspect_command(args: &[String]) -> Result<String, CliError> {
    let mut region: Option<String> = None;
    let mut rest = Vec::new();
    for arg in args {
        if region.is_none() && !arg.starts_with("--") {
            region = Some(arg.clone());
        } else {
            rest.push(arg.clone());
        }
    }
    let region = region.ok_or(CliError::MissingRegion)?;
    let source = parse_source(&rest)?;
    let manifold = load_source(source.fixture.as_deref(), source.cdp.as_deref())?;
    let found = manifold
        .get_str(&region)
        .ok_or_else(|| CliError::UnknownRegion(region.clone()))?;
    Ok(format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        found.id(),
        found.role(),
        found.label(),
        found.rect().x(),
        found.rect().y(),
        found.rect().width(),
        found.rect().height()
    ))
}

pub(crate) fn act_command(args: &[String]) -> Result<String, CliError> {
    let mut region: Option<String> = None;
    let mut verb: Option<String> = None;
    let mut confidence: Option<String> = None;
    let mut runner_up: Option<String> = None;
    let mut fixture: Option<String> = None;
    let mut cdp: Option<String> = None;
    let mut executor: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--fixture=") {
            set_once("fixture", &mut fixture, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--cdp=") {
            set_once("cdp", &mut cdp, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--confidence=") {
            set_once("confidence", &mut confidence, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--executor=") {
            set_once("executor", &mut executor, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--runner-up=") {
            set_once("runner-up", &mut runner_up, value.to_owned())?;
        } else if arg == "--cdp" {
            if cdp.is_some() {
                return Err(CliError::DuplicateFlag("--cdp"));
            }
            let next = args.get(index + 1);
            if next.is_some_and(|value| !value.starts_with("--")) {
                cdp = Some(next.unwrap().clone());
                index += 1;
            } else {
                cdp = Some(DEFAULT_CDP_HTTP.to_owned());
            }
        } else if arg == "--fixture"
            || arg == "--confidence"
            || arg == "--executor"
            || arg == "--runner-up"
        {
            index += 1;
            let flag = match arg.as_str() {
                "--fixture" => "--fixture",
                "--confidence" => "--confidence",
                "--executor" => "--executor",
                "--runner-up" => "--runner-up",
                _ => "--flag",
            };
            let Some(value) = args.get(index) else {
                return Err(CliError::MissingValue(flag));
            };
            if value.starts_with("--") {
                return Err(CliError::MissingValue(flag));
            }
            match arg.as_str() {
                "--fixture" => set_once("fixture", &mut fixture, value.clone())?,
                "--confidence" => set_once("confidence", &mut confidence, value.clone())?,
                "--executor" => set_once("executor", &mut executor, value.clone())?,
                "--runner-up" => set_once("runner-up", &mut runner_up, value.clone())?,
                _ => unreachable!("flag matched"),
            }
        } else if arg.starts_with("--") {
            return Err(CliError::UnknownFlag(arg.clone()));
        } else if region.is_none() {
            region = Some(arg.clone());
        } else if verb.is_none() {
            verb = Some(arg.clone());
        } else {
            return Err(CliError::UnknownFlag(arg.clone()));
        }
        index += 1;
    }
    let region = region.ok_or(CliError::MissingRegion)?;
    let verb = verb.ok_or(CliError::MissingVerb)?;
    if verb != "press" && verb != "click" {
        return Err(CliError::UnknownAction(verb));
    }
    let id = RegionId::try_new(&region).map_err(|_| CliError::UnknownRegion(region.clone()))?;
    let mut request = ActionRequest::new(id, hyper_use_core::Action::Click);
    let parse_score =
        |raw: &str| f64::from_str(raw).map_err(|_| CliError::BadConfidence(raw.to_owned()));
    match (confidence.as_deref(), runner_up.as_deref()) {
        (None, None) => {}
        (None, Some(_)) => return Err(CliError::RunnerUpNeedsConfidence),
        (Some(top), None) => request = request.scored(parse_score(top)?),
        (Some(top), Some(second)) => {
            request = request.ranked(parse_score(top)?, parse_score(second)?);
        }
    }
    let requested = match executor.as_deref() {
        None => None,
        Some(name) => Some(
            ExecutorKind::parse(name).ok_or_else(|| CliError::UnknownExecutor(name.to_owned()))?,
        ),
    };
    let available = match requested {
        Some(kind) => vec![kind],
        None => vec![ExecutorKind::Browser],
    };
    let selected = select_act_executor(requested, &available).map_err(|err| match err {
        ExecutorError::Unavailable(kind) => CliError::NotImplemented {
            executor: kind.as_str().to_owned(),
        },
        other => CliError::Browser(other.to_string()),
    })?;
    match selected {
        ExecutorKind::BrowserUse => {
            if cdp.is_some() {
                return Err(CliError::BrowserUseIsReplay);
            }
            let Some(path) = fixture else {
                return Err(CliError::MissingSource);
            };
            let body = read_path(&path)?;
            let mut backend = BrowserUseExecutor::from_replay(&body).map_err(|err| match err {
                BrowserUseError::BadScript { message } => CliError::BrowserUseScript { message },
                other => CliError::BrowserUseScript {
                    message: other.to_string(),
                },
            })?;
            finish_act(&mut backend, &request)
        }
        ExecutorKind::Cua => {
            if cdp.is_some() {
                return Err(CliError::CuaIsReplay);
            }
            let Some(path) = fixture else {
                return Err(CliError::MissingSource);
            };
            let body = read_path(&path)?;
            let mut backend = CuaExecutor::from_replay(&body).map_err(|err| match err {
                CuaError::BadScript { message } => CliError::CuaScript { message },
                other => CliError::CuaScript {
                    message: other.to_string(),
                },
            })?;
            finish_act(&mut backend, &request)
        }
        ExecutorKind::Macos => {
            let mut backend = StubExecutor::new(selected);
            finish_act(&mut backend, &request)
        }
        ExecutorKind::Browser => {
            if let Some(url) = cdp.as_deref() {
                let mut backend = BrowserExecutor::new(BrowserSession::new(connect_live(url)?));
                return finish_act(&mut backend, &request);
            }
            let Some(path) = fixture else {
                return Err(CliError::MissingSource);
            };
            let body = read_path(&path)?;
            if !body.trim_start().starts_with('{') {
                return Err(CliError::Browser(
                    "act against a browser session needs a CDP fixture or --cdp".into(),
                ));
            }
            let transport =
                ReplayTransport::parse(&body).map_err(|err| CliError::Browser(err.to_string()))?;
            let mut backend = BrowserExecutor::new(BrowserSession::new(transport));
            finish_act(&mut backend, &request)
        }
        other => Err(CliError::NotImplemented {
            executor: other.as_str().to_owned(),
        }),
    }
}

fn finish_act(
    executor: &mut impl ActionExecutor,
    request: &ActionRequest,
) -> Result<String, CliError> {
    match executor.execute(request) {
        Ok(receipt) => Ok(format!(
            "{}\tpress\t{}\texecuted\n",
            receipt.region_id(),
            receipt.mechanism()
        )),
        Err(err) => map_executor(err),
    }
}

pub(crate) fn verify_command(args: &[String]) -> Result<String, CliError> {
    let mut expect_text: Option<String> = None;
    let mut expect_absent: Option<String> = None;
    let mut fixture: Option<String> = None;
    let mut cdp: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--expect-text=") {
            set_once("expect-text", &mut expect_text, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--expect-absent=") {
            set_once("expect-absent", &mut expect_absent, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--fixture=") {
            set_once("fixture", &mut fixture, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--cdp=") {
            set_once("cdp", &mut cdp, value.to_owned())?;
        } else if arg == "--expect-text"
            || arg == "--expect-absent"
            || arg == "--fixture"
            || arg == "--cdp"
        {
            let flag = match arg.as_str() {
                "--expect-text" => "--expect-text",
                "--expect-absent" => "--expect-absent",
                "--fixture" => "--fixture",
                "--cdp" => "--cdp",
                _ => "--flag",
            };
            if arg == "--cdp" {
                let next = args.get(index + 1);
                if next.is_some_and(|value| !value.starts_with("--")) {
                    set_once("cdp", &mut cdp, next.unwrap().clone())?;
                    index += 1;
                } else {
                    set_once("cdp", &mut cdp, DEFAULT_CDP_HTTP.to_owned())?;
                }
            } else {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(CliError::MissingValue(flag));
                };
                if value.starts_with("--") {
                    return Err(CliError::MissingValue(flag));
                }
                let slot = match arg.as_str() {
                    "--expect-text" => &mut expect_text,
                    "--expect-absent" => &mut expect_absent,
                    "--fixture" => &mut fixture,
                    _ => unreachable!("flag matched"),
                };
                let name = match arg.as_str() {
                    "--expect-text" => "expect-text",
                    "--expect-absent" => "expect-absent",
                    "--fixture" => "fixture",
                    _ => "flag",
                };
                set_once(name, slot, value.clone())?;
            }
        } else {
            return Err(CliError::UnknownFlag(arg.clone()));
        }
        index += 1;
    }
    let manifold = load_source(fixture.as_deref(), cdp.as_deref())?;
    if let Some(text) = expect_text {
        let expectation = Expectation::text_present(text).map_err(|err| match err {
            hyper_use_browser::VerifyError::EmptyExpectation => CliError::EmptyText,
            other => CliError::Browser(other.to_string()),
        })?;
        return match hyper_use_browser::verify(&manifold, &expectation) {
            Ok(()) => Ok("verified\n".to_owned()),
            Err(hyper_use_browser::VerifyError::ExpectedTextMissing { expected }) => {
                Err(CliError::ExpectedTextMissing { expected })
            }
            Err(other) => Err(CliError::Browser(other.to_string())),
        };
    }
    if let Some(id) = expect_absent {
        let region = RegionId::try_new(&id).map_err(|_| CliError::UnknownRegion(id))?;
        return match hyper_use_browser::verify(&manifold, &Expectation::region_absent(region)) {
            Ok(()) => Ok("verified\n".to_owned()),
            Err(hyper_use_browser::VerifyError::RegionStillPresent { id }) => {
                Err(CliError::RegionStillPresent { id })
            }
            Err(other) => Err(CliError::Browser(other.to_string())),
        };
    }
    Err(CliError::MissingExpect)
}

pub(crate) fn diff_command(args: &[String]) -> Result<String, CliError> {
    let mut before: Option<String> = None;
    let mut after: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--before=") {
            set_once("before", &mut before, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--after=") {
            set_once("after", &mut after, value.to_owned())?;
        } else if arg == "--before" || arg == "--after" {
            let flag = if arg == "--before" {
                "--before"
            } else {
                "--after"
            };
            let name = if arg == "--before" { "before" } else { "after" };
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(CliError::MissingValue(flag));
            };
            if value.starts_with("--") {
                return Err(CliError::MissingValue(flag));
            }
            let slot = if arg == "--before" {
                &mut before
            } else {
                &mut after
            };
            set_once(name, slot, value.clone())?;
        } else {
            return Err(CliError::UnknownFlag(arg.clone()));
        }
        index += 1;
    }
    let before = load_fixture(&before.ok_or(CliError::MissingValue("--before"))?)?;
    let after = load_fixture(&after.ok_or(CliError::MissingValue("--after"))?)?;
    let delta = diff(&before, &after);
    let mut out = String::new();
    for id in delta.added() {
        out.push_str(&format!("added\t{id}\n"));
    }
    for id in delta.removed() {
        out.push_str(&format!("removed\t{id}\n"));
    }
    for change in delta.changed() {
        let fields: Vec<_> = change
            .fields()
            .iter()
            .map(|field| format!("{field:?}"))
            .collect();
        out.push_str(&format!("changed\t{}\t{}\n", change.id(), fields.join(",")));
    }
    if out.is_empty() {
        out.push_str("unchanged\n");
    }
    Ok(out)
}

struct Source {
    fixture: Option<String>,
    cdp: Option<String>,
}

fn parse_source(args: &[String]) -> Result<Source, CliError> {
    let mut fixture = None;
    let mut cdp = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--fixture=") {
            set_once("fixture", &mut fixture, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--cdp=") {
            set_once("cdp", &mut cdp, value.to_owned())?;
        } else if arg == "--fixture" {
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(CliError::MissingValue("--fixture"));
            };
            if value.starts_with("--") {
                return Err(CliError::MissingValue("--fixture"));
            }
            set_once("fixture", &mut fixture, value.clone())?;
        } else if arg == "--cdp" {
            if cdp.is_some() {
                return Err(CliError::DuplicateFlag("--cdp"));
            }
            let next = args.get(index + 1);
            if next.is_some_and(|value| !value.starts_with("--")) {
                cdp = Some(next.unwrap().clone());
                index += 1;
            } else {
                cdp = Some(DEFAULT_CDP_HTTP.to_owned());
            }
        } else {
            return Err(CliError::UnknownFlag(arg.clone()));
        }
        index += 1;
    }
    if fixture.is_none() && cdp.is_none() {
        return Err(CliError::MissingSource);
    }
    Ok(Source { fixture, cdp })
}

fn load_fixture(path: &str) -> Result<InteractionManifold, CliError> {
    let body = read_path(path)?;
    if body.trim_start().starts_with('{') {
        let transport =
            ReplayTransport::parse(&body).map_err(|err| CliError::Browser(err.to_string()))?;
        let mut session = BrowserSession::new(transport);
        session
            .observe()
            .cloned()
            .map_err(|err| CliError::Browser(err.to_string()))
    } else {
        parse_fixture(&body).map_err(|err| CliError::Fixture(err.to_string()))
    }
}

fn read_path(path: &str) -> Result<String, CliError> {
    fs::read_to_string(Path::new(path)).map_err(|err| CliError::Io {
        path: path.to_owned(),
        message: err.to_string(),
    })
}

fn connect_live(url: &str) -> Result<WebSocketTransport, CliError> {
    WebSocketTransport::connect(url).map_err(|err| CliError::Browser(err.to_string()))
}

fn render_regions(manifold: &InteractionManifold) -> String {
    let mut out = String::new();
    for region in manifold.regions() {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            region.id(),
            region.role(),
            region.label(),
            region.rect().x(),
            region.rect().y(),
            region.rect().width(),
            region.rect().height()
        ));
    }
    out
}

fn map_executor(err: ExecutorError) -> Result<String, CliError> {
    match err {
        ExecutorError::ConfidenceBelowThreshold {
            confidence_millis,
            minimum_millis,
        } => Err(CliError::ConfidenceBelowThreshold {
            confidence_millis,
            minimum_millis,
        }),
        ExecutorError::AmbiguousTarget {
            top_millis,
            runner_up_millis,
            margin_millis,
            minimum_margin_millis,
        } => Err(CliError::AmbiguousTarget {
            top_millis,
            runner_up_millis,
            margin_millis,
            minimum_margin_millis,
        }),
        ExecutorError::NonFiniteConfidence => Err(CliError::NonFiniteConfidence),
        ExecutorError::NotImplemented(kind) => Err(CliError::NotImplemented {
            executor: kind.as_str().to_owned(),
        }),
        ExecutorError::BrowserUse(err) => match err {
            BrowserUseError::UnknownRegion(id) => Err(CliError::UnknownRegion(id)),
            BrowserUseError::UnsupportedAction(action) => Err(CliError::UnsupportedAction(action)),
            BrowserUseError::Rejected { message } => Err(CliError::BrowserUseRejected { message }),
            BrowserUseError::BadScript { message }
            | BrowserUseError::ParamsMismatch { message } => {
                Err(CliError::BrowserUseScript { message })
            }
            other => Err(CliError::BrowserUseScript {
                message: other.to_string(),
            }),
        },
        ExecutorError::Cua(err) => match err {
            CuaError::UnknownRegion(id) => Err(CliError::UnknownRegion(id)),
            CuaError::UnsupportedAction(action) => Err(CliError::UnsupportedAction(action)),
            CuaError::Rejected { message } => Err(CliError::CuaRejected { message }),
            CuaError::BadScript { message } | CuaError::ParamsMismatch { message } => {
                Err(CliError::CuaScript { message })
            }
            other => Err(CliError::CuaScript {
                message: other.to_string(),
            }),
        },
        other => Err(CliError::Browser(other.to_string())),
    }
}
