//! Hyper-Use agent loop: observe → ActionSpace → policy → guard → ticket →
//! execute → observe → verify → history.
//!
//! The LLM (if any) does not orchestrate these steps over MCP. This crate owns
//! the loop. MCP remains an optional adapter elsewhere.

#![forbid(unsafe_code)]

mod agent;
mod error;
mod outcome;
mod runtime;
mod verify_map;

pub use agent::{Agent, AgentBuilder, AgentState, Predicted, TickResult};
pub use error::AgentError;
pub use outcome::{AgentOutcome, StepRecord, VerificationKind};
pub use runtime::{BrowserRuntime, MockBrowser};
pub use verify_map::classify_delta;
