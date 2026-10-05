//! stdio MCP server for hyper-use.
//!
//! Tool names are the six verbs: observe, locate, inspect, act, diff, verify.
//! There is no navigate tool and no argument that carries a multi-step goal.
//! Transport is newline-delimited JSON-RPC 2.0 on stdin and stdout. A
//! notification (no `id`) gets no response. Batches are rejected.
//!
//! [`serve_stdio`] owns one [`Server`], which keeps live CDP sessions and a
//! ring of recent snapshots between calls. The free [`handle_line`] and
//! [`call_tool`] use a fresh `Server` each time, so they keep no state.
//!
//! The ranker crates do not depend on this crate. `serde_json` is used here
//! to parse JSON-RPC. It is not a public type in the ranker API.

#![forbid(unsafe_code)]

mod error;
mod repeat;
mod rpc;
mod server;
mod tools;

pub use error::ToolError;
pub use repeat::{REPEAT_THRESHOLD, REPEAT_WINDOW};
pub use rpc::{handle_line, serve_stdio};
pub use server::{CdpConnector, Server, MAX_LIVE_SESSIONS};
pub use tools::call_tool;

/// `observe`
pub const TOOL_OBSERVE: &str = "observe";
/// `locate`
pub const TOOL_LOCATE: &str = "locate";
/// `inspect`
pub const TOOL_INSPECT: &str = "inspect";
/// `act`
pub const TOOL_ACT: &str = "act";
/// `diff`
pub const TOOL_DIFF: &str = "diff";
/// `verify`
pub const TOOL_VERIFY: &str = "verify";

pub const TOOLS: [&str; 6] = [
    TOOL_OBSERVE,
    TOOL_LOCATE,
    TOOL_INSPECT,
    TOOL_ACT,
    TOOL_DIFF,
    TOOL_VERIFY,
];

/// Tool name for a protocol phase.
pub fn tool_for_phase(phase: hyper_use_protocol::LoopPhase) -> &'static str {
    use hyper_use_protocol::LoopPhase;
    match phase {
        LoopPhase::Observe => TOOL_OBSERVE,
        LoopPhase::Locate => TOOL_LOCATE,
        LoopPhase::Inspect => TOOL_INSPECT,
        LoopPhase::Act => TOOL_ACT,
        LoopPhase::Diff => TOOL_DIFF,
        LoopPhase::Verify => TOOL_VERIFY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_protocol::{LoopPhase, LOOP_ORDER};

    #[test]
    fn tool_names_are_the_six_verbs() {
        let mut names = TOOLS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        assert_eq!(
            TOOLS,
            ["observe", "locate", "inspect", "act", "diff", "verify"]
        );
        for name in TOOLS {
            assert!(!name.contains('.'), "{name}");
            assert!(!name.contains("hgra"), "{name}");
            assert_ne!(name, "navigate");
        }
        for phase in LOOP_ORDER {
            assert_eq!(tool_for_phase(phase), phase.as_str());
        }
        assert_eq!(tool_for_phase(LoopPhase::Locate), TOOL_LOCATE);
    }
}
