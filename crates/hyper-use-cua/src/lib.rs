//! Computer-use surface for hyper-use.
//!
//! There is no screenshot loop and no synthesized pointer.
//! CUA remains the last resort in the executor policy. The browser CDP
//! client exists; this backend still returns not-implemented. hyper-use
//! does not call CUA on a low-confidence act.

#![forbid(unsafe_code)]

/// Stable status string. Callers must not treat this as a live pointer driver.
pub const STATUS: &str = "not implemented: computer-use actuation is a later phase";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CuaStub;

impl CuaStub {
    pub const fn status(self) -> &'static str {
        STATUS
    }
}
