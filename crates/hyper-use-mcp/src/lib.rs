//! Tool names a future MCP server would expose.
//!
//! Phase 1 does not bind a transport, allocate a port, or speak JSON-RPC.
//! Names match the protocol loop and are prefixed with `hyper-use` so they
//! cannot be confused with another product.

#![forbid(unsafe_code)]

/// `hyper-use.observe`
pub const TOOL_OBSERVE: &str = "hyper-use.observe";
/// `hyper-use.locate`
pub const TOOL_LOCATE: &str = "hyper-use.locate";
/// `hyper-use.inspect`
pub const TOOL_INSPECT: &str = "hyper-use.inspect";
/// `hyper-use.act`
pub const TOOL_ACT: &str = "hyper-use.act";
/// `hyper-use.diff`
pub const TOOL_DIFF: &str = "hyper-use.diff";
/// `hyper-use.verify`
pub const TOOL_VERIFY: &str = "hyper-use.verify";

pub const TOOLS: [&str; 6] = [
    TOOL_OBSERVE,
    TOOL_LOCATE,
    TOOL_INSPECT,
    TOOL_ACT,
    TOOL_DIFF,
    TOOL_VERIFY,
];

/// Tool name for a protocol phase. Returns `None` only if a future phase is
/// added to the protocol without a matching tool. The five current phases match.
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
    fn tool_names_are_unique_and_follow_the_loop() {
        let mut names = TOOLS.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        for name in TOOLS {
            assert!(name.starts_with("hyper-use."));
            assert!(!name.contains("hgra"));
        }
        for phase in LOOP_ORDER {
            assert!(tool_for_phase(phase).ends_with(phase.as_str()));
        }
        assert_eq!(tool_for_phase(LoopPhase::Locate), TOOL_LOCATE);
    }
}
