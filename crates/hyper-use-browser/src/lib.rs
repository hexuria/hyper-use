//! Browser surface for hyper-use.
//!
//! Phase 1 is a stub. There is no Chrome DevTools Protocol client, no socket,
//! and no coordinate fallback. A later phase will report regions into an
//! interaction manifold instead of guessing pixels.

#![forbid(unsafe_code)]

/// Stable status string. Callers must not treat this as a live browser.
pub const STATUS: &str = "not implemented: browser CDP is a later phase";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrowserStub;

impl BrowserStub {
    pub const fn status(self) -> &'static str {
        STATUS
    }
}
