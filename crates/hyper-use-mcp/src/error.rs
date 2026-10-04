//! Exact failures a tool call can return.
//!
//! [`ToolError::to_value`] is the JSON a client matches on. Tests compare the
//! enum and that object. A low-confidence act is not one of these: it is a
//! successful tool result with `executed: false`.

use std::fmt;

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolError {
    MissingFixture,
    MissingBefore,
    MissingAfter,
    DuplicateSource,
    MissingToolName,
    UnknownTool(String),
    GoalNotAccepted,
    CoordinatesNotAccepted,
    UnknownMatcher(String),
    EmptyText,
    UnknownRegion(String),
    UnknownRole(String),
    UnknownPosition(String),
    UnknownAction(String),
    UnsupportedAction(String),
    NonFiniteConfidence,
    ExpectedTextMissing {
        expected: String,
    },
    RegionStillPresent {
        id: String,
    },
    DimsRequireHgra,
    BadDims(String),
    BadConfidence(String),
    MissingRegion,
    MissingExpect,
    BothExpectations,
    ActNeedsCdp,
    InvalidArguments(String),
    Io {
        path: String,
        message: String,
    },
    Fixture(String),
    Browser(String),
    /// `locate_with` failed. The default weighted and V1 hyper matchers do not
    /// return this on a manifold this crate just parsed. Kept so a future
    /// encoder error is not turned into a click.
    Ranker(String),
}

impl ToolError {
    pub const fn variant(&self) -> &'static str {
        match self {
            Self::MissingFixture => "MissingFixture",
            Self::MissingBefore => "MissingBefore",
            Self::MissingAfter => "MissingAfter",
            Self::DuplicateSource => "DuplicateSource",
            Self::MissingToolName => "MissingToolName",
            Self::UnknownTool(_) => "UnknownTool",
            Self::GoalNotAccepted => "GoalNotAccepted",
            Self::CoordinatesNotAccepted => "CoordinatesNotAccepted",
            Self::UnknownMatcher(_) => "UnknownMatcher",
            Self::EmptyText => "EmptyText",
            Self::UnknownRegion(_) => "UnknownRegion",
            Self::UnknownRole(_) => "UnknownRole",
            Self::UnknownPosition(_) => "UnknownPosition",
            Self::UnknownAction(_) => "UnknownAction",
            Self::UnsupportedAction(_) => "UnsupportedAction",
            Self::NonFiniteConfidence => "NonFiniteConfidence",
            Self::ExpectedTextMissing { .. } => "ExpectedTextMissing",
            Self::RegionStillPresent { .. } => "RegionStillPresent",
            Self::DimsRequireHgra => "DimsRequireHgra",
            Self::BadDims(_) => "BadDims",
            Self::BadConfidence(_) => "BadConfidence",
            Self::MissingRegion => "MissingRegion",
            Self::MissingExpect => "MissingExpect",
            Self::BothExpectations => "BothExpectations",
            Self::ActNeedsCdp => "ActNeedsCdp",
            Self::InvalidArguments(_) => "InvalidArguments",
            Self::Io { .. } => "Io",
            Self::Fixture(_) => "Fixture",
            Self::Browser(_) => "Browser",
            Self::Ranker(_) => "Ranker",
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::UnknownTool(name) => json!({"variant": "UnknownTool", "name": name}),
            Self::UnknownMatcher(name) => json!({"variant": "UnknownMatcher", "name": name}),
            Self::UnknownRegion(id) => json!({"variant": "UnknownRegion", "id": id}),
            Self::UnknownRole(role) => json!({"variant": "UnknownRole", "role": role}),
            Self::UnknownPosition(position) => {
                json!({"variant": "UnknownPosition", "position": position})
            }
            Self::UnknownAction(action) => json!({"variant": "UnknownAction", "action": action}),
            Self::UnsupportedAction(action) => {
                json!({"variant": "UnsupportedAction", "action": action})
            }
            Self::ExpectedTextMissing { expected } => {
                json!({"variant": "ExpectedTextMissing", "expected": expected})
            }
            Self::RegionStillPresent { id } => json!({"variant": "RegionStillPresent", "id": id}),
            Self::BadDims(dims) => json!({"variant": "BadDims", "dims": dims}),
            Self::BadConfidence(value) => json!({"variant": "BadConfidence", "value": value}),
            Self::InvalidArguments(message) => {
                json!({"variant": "InvalidArguments", "message": message})
            }
            Self::Io { path, message } => {
                json!({"variant": "Io", "path": path, "message": message})
            }
            Self::Fixture(message) => json!({"variant": "Fixture", "message": message}),
            Self::Browser(message) => json!({"variant": "Browser", "message": message}),
            Self::Ranker(message) => json!({"variant": "Ranker", "message": message}),
            other => json!({"variant": other.variant()}),
        }
    }

    pub fn to_json_string(&self) -> String {
        self.to_value().to_string()
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFixture => f.write_str("tool requires fixture or cdp"),
            Self::MissingBefore => f.write_str("diff requires before"),
            Self::MissingAfter => f.write_str("diff requires after"),
            Self::DuplicateSource => f.write_str("pass fixture or cdp, not both"),
            Self::MissingToolName => f.write_str("tools/call requires a name"),
            Self::UnknownTool(name) => write!(f, "unknown tool `{name}`"),
            Self::GoalNotAccepted => f.write_str("hyper-use does not accept a goal or navigate"),
            Self::CoordinatesNotAccepted => {
                f.write_str("hyper-use does not accept coordinates; act on a region id")
            }
            Self::UnknownMatcher(name) => write!(f, "unknown matcher `{name}`"),
            Self::EmptyText => f.write_str("text must contain at least one alphanumeric token"),
            Self::UnknownRegion(id) => write!(f, "unknown region `{id}`"),
            Self::UnknownRole(role) => write!(f, "unknown role `{role}`"),
            Self::UnknownPosition(position) => write!(f, "unknown position `{position}`"),
            Self::UnknownAction(action) => write!(f, "unknown action `{action}`"),
            Self::UnsupportedAction(action) => {
                write!(f, "browser session cannot perform `{action}`")
            }
            Self::NonFiniteConfidence => f.write_str("confidence must be finite"),
            Self::ExpectedTextMissing { expected } => {
                write!(f, "expected text `{expected}` did not appear")
            }
            Self::RegionStillPresent { id } => write!(f, "region `{id}` is still present"),
            Self::DimsRequireHgra => f.write_str("dims is only valid with matcher hgra"),
            Self::BadDims(dims) => write!(f, "unsupported dims `{dims}`"),
            Self::BadConfidence(value) => write!(f, "bad confidence `{value}`"),
            Self::MissingRegion => f.write_str("act and inspect require region"),
            Self::MissingExpect => f.write_str("verify requires expect_text or expect_absent"),
            Self::BothExpectations => {
                f.write_str("verify accepts expect_text or expect_absent, not both")
            }
            Self::ActNeedsCdp => {
                f.write_str("act needs a CDP fixture or cdp; a manifold fixture has no DOM node")
            }
            Self::InvalidArguments(message) => write!(f, "invalid arguments: {message}"),
            Self::Io { path, message } => write!(f, "cannot read {path}: {message}"),
            Self::Fixture(message) => write!(f, "fixture: {message}"),
            Self::Browser(message) => write!(f, "browser: {message}"),
            Self::Ranker(message) => write!(f, "locate: {message}"),
        }
    }
}

impl std::error::Error for ToolError {}
