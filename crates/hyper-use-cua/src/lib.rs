//! Computer-use surface for hyper-use.
//!
//! Phase 1 is a stub. There is no screenshot loop and no synthesized pointer.
//! CUA remains the last resort in the executor policy, behind structured
//! browser and accessibility backends that do not exist yet.

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
