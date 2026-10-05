//! Browser policy for Hyper-Use.
//!
//! PUA owns **HOW** a finite choice is made (`CandidateSet` + `Scores` + `decide`).
//! This crate owns **WHAT** browser evidence means. Hard invalidity (disabled,
//! hidden, occluded, …) is filtered **before** PUA sees candidates — never as
//! score penalties.
//!
//! Pin: `hexuria/pua` @ `fe3f1fd3818feb452fae1771ff2171b8598f86e6`.

#![forbid(unsafe_code)]

mod escalate;
mod evidence;
mod goal;
mod pua_policy;
#[cfg(feature = "remote")]
mod remote;
mod text;
mod types;

pub use escalate::EscalatingPolicy;
pub use goal::AgentGoal;
pub use pua_policy::PuaPolicy;
#[cfg(feature = "remote")]
pub use remote::{
    parse_reply, request_json, RemotePolicy, RemoteTransport, ScriptedRemote, UnconfiguredRemote,
};
pub use text::{DeterministicTextResolver, TextContext, TextError, TextResolution, TextResolver};
pub use types::{
    BrowserPolicy, HistoryEntry, PolicyDecision, PolicyError, PolicyOutcome, RankedAction,
};

/// Documented PUA git rev this crate is pinned to.
pub const PUA_GIT_REV: &str = "fe3f1fd3818feb452fae1771ff2171b8598f86e6";
