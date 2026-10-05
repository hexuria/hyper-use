//! stdio MCP server for hyper-use.
//!
//! Product tools: observe, guard, verify. Locate, inspect, and diff remain as
//! transitional helpers. `act` is accepted as a deprecated alias of `guard` and
//! never clicks. There is no navigate tool.

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

pub const TOOL_OBSERVE: &str = "observe";
pub const TOOL_LOCATE: &str = "locate";
pub const TOOL_INSPECT: &str = "inspect";
pub const TOOL_GUARD: &str = "guard";
/// Deprecated alias of [`TOOL_GUARD`]. Never clicks. Kept only because the
/// historical bench arms (A5/A6, `bench/arms/`) call it over MCP; see ADR 0003.
#[deprecated(since = "0.1.0", note = "use TOOL_GUARD; `act` never clicks")]
pub const TOOL_ACT: &str = "act";
pub const TOOL_DIFF: &str = "diff";
pub const TOOL_VERIFY: &str = "verify";

#[allow(deprecated)]
pub const TOOLS: [&str; 7] = [
    TOOL_OBSERVE,
    TOOL_LOCATE,
    TOOL_INSPECT,
    TOOL_GUARD,
    TOOL_ACT,
    TOOL_DIFF,
    TOOL_VERIFY,
];

pub const PRODUCT_TOOLS: [&str; 3] = [TOOL_OBSERVE, TOOL_GUARD, TOOL_VERIFY];

/// Tool name for a legacy protocol phase.
pub fn tool_for_phase(phase: hyper_use_protocol::LoopPhase) -> &'static str {
    use hyper_use_protocol::LoopPhase;
    match phase {
        LoopPhase::Observe => TOOL_OBSERVE,
        LoopPhase::Locate => TOOL_LOCATE,
        LoopPhase::Inspect => TOOL_INSPECT,
        LoopPhase::Act => TOOL_GUARD,
        LoopPhase::Diff => TOOL_DIFF,
        LoopPhase::Verify => TOOL_VERIFY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_protocol::{FirewallPhase, LoopPhase, FIREWALL_ORDER, LOOP_ORDER};

    #[test]
    fn product_tools_are_observe_guard_verify() {
        assert_eq!(PRODUCT_TOOLS, ["observe", "guard", "verify"]);
        for name in PRODUCT_TOOLS {
            assert!(TOOLS.contains(&name));
        }
        assert!(!PRODUCT_TOOLS.contains(&"navigate"));
        assert!(!PRODUCT_TOOLS.contains(&"act"));
    }

    #[test]
    fn firewall_phases_match_product_tools() {
        let names: Vec<_> = FIREWALL_ORDER.iter().map(|p| p.as_str()).collect();
        assert_eq!(names, PRODUCT_TOOLS);
        assert_eq!(tool_for_phase(LoopPhase::Act), TOOL_GUARD);
        assert_eq!(tool_for_phase(LoopPhase::Observe), TOOL_OBSERVE);
        let _ = LOOP_ORDER;
        let _ = FirewallPhase::Guard;
    }
}
