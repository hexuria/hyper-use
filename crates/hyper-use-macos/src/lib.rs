//! macOS accessibility surface for hyper-use.
//!
//! Phase 1 is a stub. There is no AXUIElement binding and no permission prompt.
//! The macOS host application itself is also not built in this workspace.

#![forbid(unsafe_code)]

/// Stable status string. Callers must not treat this as a live accessibility tree.
pub const STATUS: &str = "not implemented: macOS accessibility is a later phase";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MacosStub;

impl MacosStub {
    pub const fn status(self) -> &'static str {
        STATUS
    }
}
