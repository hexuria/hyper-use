//! Argument parsing and fixture locate for the `ultra-instinct` binary.
//!
//! The library is the tested surface. The binary prints this module's stdout
//! and exits non-zero on [`CliError`].

#![forbid(unsafe_code)]

use aui_core::{Action, LocateQuery, Role, Zone};
use aui_resonance::{RegionMatcher, WeightedMatcher};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliError {
    UnknownCommand(String),
    MissingFixture,
    MissingValue(&'static str),
    UnknownFlag(String),
    DuplicateFlag(&'static str),
    UnknownRole(String),
    UnknownPosition(String),
    UnknownAction(String),
    BadDims(String),
    EmptyText,
    Io {
        path: String,
        message: String,
    },
    Fixture(String),
    Locate(String),
    ExpectedTextMissing {
        expected: String,
    },
    RegionStillPresent {
        id: String,
    },
    ConfidenceBelowThreshold {
        confidence_millis: i32,
        minimum_millis: i32,
    },
    AmbiguousTarget {
        top_millis: i32,
        runner_up_millis: i32,
        margin_millis: i32,
        minimum_margin_millis: i32,
    },
    RunnerUpNeedsConfidence,
    NonFiniteConfidence,
    DimsRequireHgra,
    MissingSource,
    Browser(String),
    /// Missing or invalid environment configuration (e.g. `TYPESAFE_API_KEY`).
    Config(String),
    UnknownMatcher(String),
    MissingRegion,
    MissingVerb,
    BadConfidence(String),
    MissingExpect,
    UnknownRegion(String),
    UnknownExecutor(String),
    UnsupportedAction(String),
    NotImplemented {
        executor: String,
    },
    BrowserUseIsReplay,
    BrowserUseRejected {
        message: String,
    },
    BrowserUseScript {
        message: String,
    },
    CuaIsReplay,
    CuaRejected {
        message: String,
    },
    CuaScript {
        message: String,
    },
    /// `mcp` is served by the binary, which owns stdin. This library call does not.
    McpIsStdio,
    /// `run` ended in a failed agent outcome; the transcript is attached.
    Agent(String),
    BadNumber {
        flag: &'static str,
        value: String,
    },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(name) => write!(f, "unknown command `{name}`"),
            Self::MissingFixture => f.write_str("locate requires --fixture <path>"),
            Self::MissingValue(flag) => write!(f, "missing value for {flag}"),
            Self::UnknownFlag(flag) => write!(f, "unknown flag `{flag}`"),
            Self::DuplicateFlag(flag) => write!(f, "duplicate flag {flag}"),
            Self::UnknownRole(role) => write!(f, "unknown role `{role}`"),
            Self::UnknownPosition(position) => write!(f, "unknown position `{position}`"),
            Self::UnknownAction(action) => write!(f, "unknown action `{action}`"),
            Self::BadDims(dims) => write!(f, "unsupported dims `{dims}`"),
            Self::EmptyText => {
                f.write_str("locate text must contain at least one alphanumeric token")
            }
            Self::Io { path, message } => write!(f, "cannot read {path}: {message}"),
            Self::Fixture(message) => write!(f, "fixture: {message}"),
            Self::Locate(message) => write!(f, "locate: {message}"),
            Self::ExpectedTextMissing { expected } => {
                write!(f, "expected text `{expected}` did not appear")
            }
            Self::RegionStillPresent { id } => write!(f, "region `{id}` is still present"),
            Self::ConfidenceBelowThreshold {
                confidence_millis,
                minimum_millis,
            } => write!(
                f,
                "confidence {confidence_millis} is below the act minimum {minimum_millis}"
            ),
            Self::AmbiguousTarget {
                top_millis,
                runner_up_millis,
                margin_millis,
                minimum_margin_millis,
            } => write!(
                f,
                "top {top_millis} and runner-up {runner_up_millis} differ by {margin_millis} millis, below the act margin {minimum_margin_millis}"
            ),
            Self::RunnerUpNeedsConfidence => f.write_str("--runner-up requires --confidence"),
            Self::NonFiniteConfidence => f.write_str("confidence must be finite"),
            Self::DimsRequireHgra => f.write_str("--dims is only valid with --matcher hgra"),
            Self::MissingSource => f.write_str("command requires --fixture <path> or --cdp [url]"),
            Self::Browser(message) => write!(f, "browser: {message}"),
            Self::Config(message) => write!(f, "config: {message}"),
            Self::UnknownMatcher(name) => write!(f, "unknown matcher `{name}`"),
            Self::MissingRegion => f.write_str("act requires a region id"),
            Self::MissingVerb => f.write_str("act requires a verb (`press`)"),
            Self::BadConfidence(value) => write!(f, "bad confidence `{value}`"),
            Self::MissingExpect => f.write_str("verify requires --expect-text or --expect-absent"),
            Self::UnknownRegion(id) => write!(f, "unknown region `{id}`"),
            Self::UnknownExecutor(name) => write!(f, "unknown executor `{name}`"),
            Self::UnsupportedAction(action) => {
                write!(f, "browser-use executor cannot perform `{action}`")
            }
            Self::NotImplemented { executor } => {
                write!(f, "{executor} executor is not implemented")
            }
            Self::BrowserUseIsReplay => {
                f.write_str("browser-use act uses a replay fixture, not a live CDP endpoint")
            }
            Self::BrowserUseRejected { message } => {
                write!(f, "browser-use rejected the semantic act: {message}")
            }
            Self::BrowserUseScript { message } => {
                write!(f, "invalid browser-use script: {message}")
            }
            Self::CuaIsReplay => {
                f.write_str("cua act uses a replay fixture, not a live CDP endpoint")
            }
            Self::CuaRejected { message } => {
                write!(f, "cua rejected the semantic act: {message}")
            }
            Self::CuaScript { message } => {
                write!(f, "invalid cua script: {message}")
            }
            Self::McpIsStdio => {
                f.write_str("mcp serves JSON-RPC on stdio; run the ultra-instinct binary")
            }
            Self::Agent(transcript) => write!(f, "agent run failed\n{transcript}"),
            Self::BadNumber { flag, value } => write!(f, "{flag} expects a number, got `{value}`"),
        }
    }
}

impl std::error::Error for CliError {}

pub fn usage() -> &'static str {
    "ultra-instinct run|observe|locate|inspect|guard|verify|diff|mcp\nrun --goal <text> (--cdp [url] [--url <page>] | --fixture <replay.cdp.json|page.manifold>) [--max-steps N] [--text-model-cmd <program>]\n  owned agent loop: observe -> Instinct -> gate -> ticket -> execute -> verify (no LLM, no MCP)\nmcp serves newline-delimited JSON-RPC on stdin.\nguard [--fixture <path> | --cdp [url]] --target <label> [--role button] [--proposed <id>] [--json]\nact is a deprecated alias of guard and never clicks.\nlocate [--fixture <path>] [text] [--role ...] [--matcher weighted] [--json]\nverify (--expect-text <text> | --expect-absent <id>) [--fixture <path>]\nDefault matcher: weighted. HGRA is experimental.\n"
}

/// Run one invocation. `args` does not include the program name.
/// The returned string is the full stdout body.
pub fn execute(args: &[String]) -> Result<String, CliError> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(usage().to_owned());
    }
    match args[0].as_str() {
        "locate" => locate_command(&args[1..]),
        "observe" => crate::session_cmd::observe_command(&args[1..]),
        "act" => crate::session_cmd::act_command(&args[1..]),
        "guard" => crate::session_cmd::guard_command(&args[1..]),
        "verify" => crate::session_cmd::verify_command(&args[1..]),
        "diff" => crate::session_cmd::diff_command(&args[1..]),
        "inspect" => crate::session_cmd::inspect_command(&args[1..]),
        "run" => crate::run_cmd::run_command(&args[1..]),
        "mcp" => Err(CliError::McpIsStdio),
        other => Err(CliError::UnknownCommand(other.to_owned())),
    }
}

fn locate_command(args: &[String]) -> Result<String, CliError> {
    let mut fixture: Option<String> = None;
    let mut text: Option<String> = None;
    let mut role: Option<String> = None;
    let mut position: Option<String> = None;
    let mut action: Option<String> = None;
    let mut dims: Option<String> = None;
    let mut matcher: Option<String> = None;
    let mut cdp: Option<String> = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--fixture=") {
            set_once("fixture", &mut fixture, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--text=") {
            set_once("text", &mut text, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--role=") {
            set_once("role", &mut role, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--position=") {
            set_once("position", &mut position, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--action=") {
            set_once("action", &mut action, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--dims=") {
            set_once("dims", &mut dims, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--matcher=") {
            set_once("matcher", &mut matcher, value.to_owned())?;
        } else if let Some(value) = arg.strip_prefix("--cdp=") {
            set_once("cdp", &mut cdp, value.to_owned())?;
        } else if arg == "--cdp" {
            if cdp.is_some() {
                return Err(CliError::DuplicateFlag("--cdp"));
            }
            let next = args.get(index + 1);
            if next.is_some_and(|value| !value.starts_with("--")) {
                cdp = Some(next.unwrap().clone());
                index += 1;
            } else {
                cdp = Some(aui_browser::DEFAULT_CDP_HTTP.to_owned());
            }
        } else if arg == "--json" {
            if json {
                return Err(CliError::DuplicateFlag("--json"));
            }
            json = true;
        } else if let Some(flag) = arg.strip_prefix("--") {
            let name = flag.split('=').next().unwrap_or(flag);
            let slot = match name {
                "fixture" => Some(&mut fixture),
                "text" => Some(&mut text),
                "role" => Some(&mut role),
                "position" => Some(&mut position),
                "action" => Some(&mut action),
                "dims" => Some(&mut dims),
                "matcher" => Some(&mut matcher),
                _ => None,
            };
            let Some(slot) = slot else {
                return Err(CliError::UnknownFlag(arg.clone()));
            };
            index += 1;
            let Some(value) = args.get(index) else {
                return Err(CliError::MissingValue(flag_name(name)));
            };
            if value.starts_with("--") {
                return Err(CliError::MissingValue(flag_name(name)));
            }
            set_once(name, slot, value.clone())?;
        } else {
            set_once("text", &mut text, arg.clone())?;
        }
        index += 1;
    }
    if fixture.is_none() && cdp.is_none() {
        return Err(CliError::MissingFixture);
    }
    if fixture.is_some() && cdp.is_some() {
        return Err(CliError::DuplicateFlag("--cdp"));
    }
    let query = build_query(text, role, position, action)?;
    let matcher_name = matcher.unwrap_or_else(|| "weighted".to_owned());
    let manifold = session_cmd::load_source(fixture.as_deref(), cdp.as_deref())?;
    if dims.is_some() {
        return Err(CliError::DimsRequireHgra);
    }
    let ranked = match matcher_name.as_str() {
        "weighted" => WeightedMatcher::default()
            .rank(&query, &manifold)
            .map_err(|err| CliError::Locate(err.to_string()))?,
        "hgra" => {
            return Err(CliError::UnknownMatcher(
                "hgra is an experiment; enable aui-resonance feature `hgra`".into(),
            ))
        }
        other => return Err(CliError::UnknownMatcher(other.to_owned())),
    };
    if json {
        Ok(render_json(&manifold, &matcher_name, &ranked))
    } else {
        Ok(render_text(&manifold, &ranked))
    }
}

pub(crate) fn set_once(
    name: &str,
    slot: &mut Option<String>,
    value: String,
) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::DuplicateFlag(flag_name(name)));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn flag_name(name: &str) -> &'static str {
    match name {
        "fixture" => "--fixture",
        "text" => "--text",
        "role" => "--role",
        "position" => "--position",
        "action" => "--action",
        "dims" => "--dims",
        "json" => "--json",
        "matcher" => "--matcher",
        "cdp" => "--cdp",
        "confidence" => "--confidence",
        "runner-up" => "--runner-up",
        "expect-text" => "--expect-text",
        "expect-absent" => "--expect-absent",
        "before" => "--before",
        "after" => "--after",
        _ => "--flag",
    }
}

fn build_query(
    text: Option<String>,
    role: Option<String>,
    position: Option<String>,
    action: Option<String>,
) -> Result<LocateQuery, CliError> {
    let mut query = LocateQuery::new();
    if let Some(text) = text {
        query = query.text(text).map_err(|_| CliError::EmptyText)?;
    }
    if let Some(role) = role {
        let parsed = Role::parse(&role.to_ascii_lowercase()).ok_or(CliError::UnknownRole(role))?;
        query = query.role(parsed);
    }
    if let Some(position) = position {
        let parsed = Zone::parse(&position.to_ascii_lowercase())
            .ok_or(CliError::UnknownPosition(position))?;
        query = query.position(parsed);
    }
    if let Some(action) = action {
        let parsed =
            Action::parse(&action.to_ascii_lowercase()).ok_or(CliError::UnknownAction(action))?;
        query = query.action(parsed);
    }
    Ok(query)
}

fn render_text(
    manifold: &aui_core::InteractionManifold,
    ranked: &[aui_resonance::Match],
) -> String {
    let mut out = String::new();
    for candidate in ranked {
        let region = manifold.get(candidate.id());
        let label = region.map(|region| region.label()).unwrap_or("");
        let role = region
            .map(|region| region.role().as_str())
            .unwrap_or("unknown");
        out.push_str(&format!(
            "{}\t{}\t{:.6}\t{}\t{}\n",
            candidate.rank(),
            candidate.id(),
            candidate.confidence(),
            role,
            label
        ));
    }
    out
}

fn render_json(
    manifold: &aui_core::InteractionManifold,
    matcher: &str,
    ranked: &[aui_resonance::Match],
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"product\": \"ultra-instinct\",\n");
    out.push_str(&format!("  \"matcher\": \"{matcher}\",\n"));
    out.push_str("  \"candidates\": [\n");
    for (index, candidate) in ranked.iter().enumerate() {
        let region = manifold.get(candidate.id());
        let label = region.map(|region| region.label()).unwrap_or("");
        let role = region
            .map(|region| region.role().as_str())
            .unwrap_or("unknown");
        let comma = if index + 1 == ranked.len() { "" } else { "," };
        out.push_str("    {\n");
        out.push_str(&format!("      \"rank\": {},\n", candidate.rank()));
        out.push_str(&format!(
            "      \"id\": \"{}\",\n",
            json_escape(candidate.id().as_str())
        ));
        out.push_str(&format!("      \"role\": \"{role}\",\n"));
        out.push_str(&format!("      \"label\": \"{}\",\n", json_escape(label)));
        out.push_str(&format!(
            "      \"confidence\": {:.6}\n",
            candidate.confidence()
        ));
        out.push_str(&format!("    }}{comma}\n"));
    }
    out.push_str("  ]\n}\n");
    out
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out
}

mod compare;
pub(crate) mod run_cmd;
pub(crate) mod session_cmd;

#[cfg(feature = "jev")]
pub mod typesafe;

#[cfg(feature = "jev")]
pub use compare::fixture_compare_live;
pub use compare::{eval_corpus, fixture_compare, CompareError, CorpusReport, FixtureReport};

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    fn fixture() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sidebar.manifold")
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn missing_fixture_is_a_specific_error() {
        let err = execute(&args(&["locate", "--text", "Settings"])).unwrap_err();
        assert_eq!(err, CliError::MissingFixture);
        assert_eq!(err.to_string(), "locate requires --fixture <path>");
    }

    #[test]
    fn unknown_role_and_command_fail() {
        let err = execute(&args(&[
            "locate",
            "--fixture",
            "unused",
            "--role",
            "spaceship",
        ]))
        .unwrap_err();
        assert_eq!(err, CliError::UnknownRole("spaceship".into()));
        let err = execute(&args(&["fly"])).unwrap_err();
        assert_eq!(err, CliError::UnknownCommand("fly".into()));
        let err = execute(&args(&["mcp"])).unwrap_err();
        assert_eq!(err, CliError::McpIsStdio);
        assert_eq!(
            err.to_string(),
            "mcp serves JSON-RPC on stdio; run the ultra-instinct binary"
        );
        let err = execute(&args(&["locate", "--fixture"])).unwrap_err();
        assert_eq!(err, CliError::MissingValue("--fixture"));
    }

    #[test]
    fn locate_json_ranks_sidebar_settings_first() {
        let path = fixture();
        let stdout = execute(&args(&[
            "locate",
            "--fixture",
            &path,
            "--text",
            "Settings",
            "--role",
            "button",
            "--position",
            "left",
            "--json",
        ]))
        .unwrap();
        let nav = stdout.find("\"id\": \"nav-settings\"").unwrap();
        let main = stdout.find("\"id\": \"main-settings\"").unwrap();
        assert!(nav < main, "{stdout}");
        assert!(stdout.contains("\"product\": \"ultra-instinct\""));
        assert!(stdout.contains("\"rank\": 1"));
        assert!(!stdout.contains("hgra"));
    }

    fn cdp_fixture(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn locate_sign_in_on_cdp_fixture_ranks_the_button_first() {
        let path = cdp_fixture("sign-in.cdp.json");
        let stdout = execute(&args(&["locate", "Sign in", "--fixture", &path])).unwrap();
        let first = stdout.lines().next().unwrap();
        assert!(first.starts_with("1\tn100\t"), "{stdout}");
        assert!(first.contains("Sign in"));
        let json = execute(&args(&["locate", "Sign in", "--fixture", &path, "--json"])).unwrap();
        assert!(json.contains("\"matcher\": \"weighted\""));
        assert!(json.find("\"id\": \"n100\"").unwrap() < json.find("\"id\": \"n200\"").unwrap());
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn act_refuses_a_ranked_target_inside_the_margin() {
        // sign-in.cdp.json has no press steps, so a press would fail.
        let path = cdp_fixture("sign-in.cdp.json");
        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &path,
            "--confidence",
            "1.0",
            "--runner-up=0.98",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::AmbiguousTarget {
                top_millis: 1000,
                runner_up_millis: 980,
                margin_millis: 20,
                minimum_margin_millis: 50,
            }
        );
        let press = cdp_fixture("sign-in-press.cdp.json");
        let stdout = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &press,
            "--confidence",
            "1.0",
            "--runner-up",
            "0.5",
        ]))
        .unwrap();
        assert_eq!(stdout, "n100\tpress\tdom-semantic\texecuted\n");
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn runner_up_needs_confidence() {
        let path = cdp_fixture("sign-in-press.cdp.json");
        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &path,
            "--runner-up",
            "0.5",
        ]))
        .unwrap_err();
        assert_eq!(err, CliError::RunnerUpNeedsConfidence);
        assert_eq!(err.to_string(), "--runner-up requires --confidence");
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn act_press_records_dom_semantic_and_low_confidence_is_exact() {
        let path = cdp_fixture("sign-in-press.cdp.json");
        let stdout = execute(&args(&["act", "n100", "press", "--fixture", &path])).unwrap();
        assert_eq!(stdout, "n100\tpress\tdom-semantic\texecuted\n");
        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &path,
            "--confidence",
            "0.49",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        assert_eq!(
            err.to_string(),
            "confidence 490 is below the act minimum 550"
        );
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn browser_use_act_sends_semantics_and_low_confidence_does_not_execute() {
        let path = cdp_fixture("sign-in.browser-use.json");
        let stdout = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &path,
            "--executor",
            "browser-use",
            "--confidence",
            "0.55",
        ]))
        .unwrap();
        assert_eq!(stdout, "n100\tpress\tbrowser-use-semantic\texecuted\n");
        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &path,
            "--executor",
            "browser-use",
            "--confidence",
            "0.49",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        let rejected = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp_fixture("sign-in-reject.browser-use.json"),
            "--executor",
            "browser-use",
        ]))
        .unwrap_err();
        assert_eq!(
            rejected,
            CliError::BrowserUseRejected {
                message: "control refused the semantic act".into(),
            }
        );
        assert_eq!(
            rejected.to_string(),
            "browser-use rejected the semantic act: control refused the semantic act"
        );
        let macos = execute(&args(&["act", "n100", "press", "--executor", "macos"])).unwrap_err();
        assert_eq!(
            macos,
            CliError::NotImplemented {
                executor: "macos".into(),
            }
        );
        assert_eq!(macos.to_string(), "macos executor is not implemented");
        let cua = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp_fixture("sign-in.cua.json"),
            "--executor",
            "cua",
            "--confidence",
            "0.55",
        ]))
        .unwrap();
        assert_eq!(cua, "n100\tpress\tcua-semantic\texecuted\n");
        let low = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp_fixture("sign-in.cua.json"),
            "--executor",
            "cua",
            "--confidence",
            "0.49",
        ]))
        .unwrap_err();
        assert_eq!(
            low,
            CliError::ConfidenceBelowThreshold {
                confidence_millis: 490,
                minimum_millis: 550,
            }
        );
        let rejected = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp_fixture("sign-in-reject.cua.json"),
            "--executor",
            "cua",
        ]))
        .unwrap_err();
        assert_eq!(
            rejected,
            CliError::CuaRejected {
                message: "control refused the semantic act".into(),
            }
        );
        assert_eq!(
            rejected.to_string(),
            "cua rejected the semantic act: control refused the semantic act"
        );
        let replay = execute(&args(&[
            "act",
            "n100",
            "press",
            "--cdp",
            "--executor",
            "cua",
        ]))
        .unwrap_err();
        assert_eq!(replay, CliError::CuaIsReplay);
        assert_eq!(
            replay.to_string(),
            "cua act uses a replay fixture, not a live CDP endpoint"
        );
        let unknown = execute(&args(&[
            "act",
            "n100",
            "press",
            "--executor",
            "navigate",
            "--fixture",
            &path,
        ]))
        .unwrap_err();
        assert_eq!(unknown, CliError::UnknownExecutor("navigate".into()));
    }

    #[test]
    fn verify_missing_text_is_exact_and_welcome_succeeds() {
        let err = execute(&args(&[
            "verify",
            "--fixture",
            &cdp_fixture("sign-in.cdp.json"),
            "--expect-text",
            "Welcome",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::ExpectedTextMissing {
                expected: "Welcome".into(),
            }
        );
        assert_eq!(err.to_string(), "expected text `Welcome` did not appear");
        let stdout = execute(&args(&[
            "verify",
            "--fixture",
            &cdp_fixture("welcome.cdp.json"),
            "--expect-text",
            "Welcome",
        ]))
        .unwrap();
        assert_eq!(stdout, "verified\n");
    }

    #[test]
    fn diff_reports_removed_sign_in() {
        let stdout = execute(&args(&[
            "diff",
            "--before",
            &cdp_fixture("sign-in.cdp.json"),
            "--after",
            &cdp_fixture("welcome.cdp.json"),
        ]))
        .unwrap();
        assert!(stdout.contains("removed\tn100\n"), "{stdout}");
        assert!(stdout.contains("added\tn300\n"), "{stdout}");
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn hgra_matcher_still_ranks_sidebar_settings_first() {
        let path = fixture();
        let stdout = execute(&args(&[
            "locate",
            "--fixture",
            &path,
            "--text",
            "Settings",
            "--role",
            "button",
            "--position",
            "left",
            "--matcher",
            "hgra",
        ]))
        .unwrap();
        assert!(
            stdout.lines().next().unwrap().contains("nav-settings"),
            "{stdout}"
        );
    }

    #[test]
    fn empty_text_is_rejected() {
        let err =
            execute(&args(&["locate", "--fixture", &fixture(), "--text", "..."])).unwrap_err();
        assert_eq!(err, CliError::EmptyText);
        assert_eq!(
            err.to_string(),
            "locate text must contain at least one alphanumeric token"
        );
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn flag_parser_returns_exact_variants() {
        let err = execute(&args(&["locate", "--nope"])).unwrap_err();
        assert_eq!(err, CliError::UnknownFlag("--nope".into()));
        assert_eq!(err.to_string(), "unknown flag `--nope`");

        let err = execute(&args(&["locate", "--json", "--json"])).unwrap_err();
        assert_eq!(err, CliError::DuplicateFlag("--json"));
        assert_eq!(err.to_string(), "duplicate flag --json");

        let path = fixture();
        let err = execute(&args(&["locate", "--fixture", &path, "--matcher", "nope"])).unwrap_err();
        assert_eq!(err, CliError::UnknownMatcher("nope".into()));
        assert_eq!(err.to_string(), "unknown matcher `nope`");

        let err = execute(&args(&[
            "locate",
            "--fixture",
            &path,
            "--matcher",
            "hgra",
            "--dims",
            "7",
        ]))
        .unwrap_err();
        assert_eq!(err, CliError::BadDims("7".into()));
        assert_eq!(err.to_string(), "unsupported dims `7`");

        let err = execute(&args(&["act"])).unwrap_err();
        assert_eq!(err, CliError::MissingRegion);
        assert_eq!(err.to_string(), "act requires a region id");

        let err = execute(&args(&["act", "n100"])).unwrap_err();
        assert_eq!(err, CliError::MissingVerb);
        assert_eq!(err.to_string(), "act requires a verb (`press`)");
    }

    #[ignore = "actuation/HGRA removed in action-firewall pivot"]
    #[test]
    fn replay_scripts_and_verify_return_exact_variants() {
        let cdp = cdp_fixture("sign-in.cdp.json");
        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp,
            "--executor",
            "browser-use",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::BrowserUseScript {
                message: "unexpected field `calls`".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid browser-use script: unexpected field `calls`"
        );

        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp,
            "--executor",
            "cua",
        ]))
        .unwrap_err();
        assert_eq!(
            err,
            CliError::CuaScript {
                message: "unexpected field `calls`".into(),
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid cua script: unexpected field `calls`"
        );

        let err = execute(&args(&[
            "act",
            "n100",
            "press",
            "--fixture",
            &cdp_fixture("sign-in.browser-use.json"),
            "--executor",
            "browser-use",
            "--confidence",
            "NaN",
        ]))
        .unwrap_err();
        assert_eq!(err, CliError::NonFiniteConfidence);
        assert_eq!(err.to_string(), "confidence must be finite");

        let err = execute(&args(&[
            "verify",
            "--fixture",
            &cdp,
            "--expect-absent",
            "n100",
        ]))
        .unwrap_err();
        assert_eq!(err, CliError::RegionStillPresent { id: "n100".into() });
        assert_eq!(err.to_string(), "region `n100` is still present");
    }
}
