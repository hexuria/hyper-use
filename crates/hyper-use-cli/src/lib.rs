//! Argument parsing and fixture locate for the `hyper-use` binary.
//!
//! The library is the tested surface. The binary prints this module's stdout
//! and exits non-zero on [`CliError`].

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::str::FromStr;

use hyper_use_core::{parse_fixture, Action, LocateQuery, Role, Zone};
use hyper_use_hyper::{Dims, Encoder};
use hyper_use_resonance::{locate_with, ResonanceModel};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CliError {
    MissingCommand,
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
    Io { path: String, message: String },
    Fixture(String),
    Locate(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCommand => f.write_str("missing command (expected `locate`)"),
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
        }
    }
}

impl std::error::Error for CliError {}

pub fn usage() -> &'static str {
    "hyper-use locate --fixture <path> [--text <label>] [--role <role>] [--position left|right|top|bottom|center] [--action <action>] [--dims 512|1024|2048|4096] [--json]\n"
}

/// Run one invocation. `args` does not include the program name.
/// The returned string is the full stdout body.
pub fn execute(args: &[String]) -> Result<String, CliError> {
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(usage().to_owned());
    }
    match args[0].as_str() {
        "locate" => locate_command(&args[1..]),
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
            return Err(CliError::UnknownFlag(arg.clone()));
        }
        index += 1;
    }
    let fixture = fixture.ok_or(CliError::MissingFixture)?;
    let query = build_query(text, role, position, action)?;
    let dims = match dims {
        None => Dims::DEFAULT,
        Some(raw) => {
            let parsed = usize::from_str(&raw).map_err(|_| CliError::BadDims(raw.clone()))?;
            Dims::try_from_usize(parsed).map_err(|_| CliError::BadDims(raw))?
        }
    };
    let body = fs::read_to_string(Path::new(&fixture)).map_err(|err| CliError::Io {
        path: fixture.clone(),
        message: err.to_string(),
    })?;
    let manifold = parse_fixture(&body).map_err(|err| CliError::Fixture(err.to_string()))?;
    let ranked = locate_with(&manifold, &query, &Encoder::new(dims), ResonanceModel::V1)
        .map_err(|err| CliError::Locate(err.to_string()))?;
    if json {
        Ok(render_json(&manifold, &ranked))
    } else {
        Ok(render_text(&manifold, &ranked))
    }
}

fn set_once(name: &str, slot: &mut Option<String>, value: String) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::DuplicateFlag(flag_name(name)));
    }
    *slot = Some(value);
    Ok(())
}

fn flag_name(name: &str) -> &'static str {
    match name {
        "fixture" => "--fixture",
        "text" => "--text",
        "role" => "--role",
        "position" => "--position",
        "action" => "--action",
        "dims" => "--dims",
        "json" => "--json",
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
    manifold: &hyper_use_core::InteractionManifold,
    ranked: &[hyper_use_resonance::RankedCandidate],
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
            candidate.score().total(),
            role,
            label
        ));
    }
    out
}

fn render_json(
    manifold: &hyper_use_core::InteractionManifold,
    ranked: &[hyper_use_resonance::RankedCandidate],
) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"product\": \"hyper-use\",\n");
    out.push_str(&format!(
        "  \"weights_version\": {},\n",
        ResonanceModel::V1.version()
    ));
    out.push_str("  \"candidates\": [\n");
    for (index, candidate) in ranked.iter().enumerate() {
        let region = manifold.get(candidate.id());
        let label = region.map(|region| region.label()).unwrap_or("");
        let role = region
            .map(|region| region.role().as_str())
            .unwrap_or("unknown");
        let score = candidate.score();
        let comma = if index + 1 == ranked.len() { "" } else { "," };
        out.push_str("    {\n");
        out.push_str(&format!("      \"rank\": {},\n", candidate.rank()));
        out.push_str(&format!(
            "      \"id\": \"{}\",\n",
            json_escape(candidate.id().as_str())
        ));
        out.push_str(&format!("      \"role\": \"{role}\",\n"));
        out.push_str(&format!("      \"label\": \"{}\",\n", json_escape(label)));
        out.push_str(&format!("      \"score\": {:.6},\n", score.total()));
        out.push_str(&format!(
            "      \"hypervector\": {:.6},\n",
            score.hypervector()
        ));
        out.push_str(&format!("      \"semantic\": {:.6},\n", score.semantic()));
        out.push_str(&format!(
            "      \"source_agreement\": {:.6},\n",
            score.source_agreement()
        ));
        out.push_str(&format!("      \"geometric\": {:.6},\n", score.geometric()));
        out.push_str(&format!(
            "      \"actionability\": {:.6},\n",
            score.actionability()
        ));
        out.push_str(&format!(
            "      \"temporal_stability\": {:.6},\n",
            score.temporal_stability()
        ));
        out.push_str(&format!(
            "      \"contextual_consistency\": {:.6},\n",
            score.contextual_consistency()
        ));
        out.push_str(&format!("      \"penalty\": {:.6}\n", score.penalty()));
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(stdout.contains("\"product\": \"hyper-use\""));
        assert!(stdout.contains("\"rank\": 1"));
        assert!(!stdout.contains("hgra"));
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
}
