//! Execution policy for hyper-use.
//!
//! Phase 1 defines the trait and the order in which a later runtime should
//! try backends. Every built-in backend returns [`ExecutorError::NotImplemented`].
//! This crate does not click, type, or move the pointer.
//!
//! Policy order is fixed: browser, then macOS accessibility, then computer-use
//! pixels. The first available backend wins. Availability is an input; this
//! crate does not probe the operating system.

#![forbid(unsafe_code)]

use std::fmt;

use hyper_use_core::{Action, RegionId};

/// Preference order when more than one backend can perform an action.
pub const DEFAULT_POLICY_ORDER: [ExecutorKind; 3] = [
    ExecutorKind::Browser,
    ExecutorKind::Macos,
    ExecutorKind::Cua,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExecutorKind {
    Browser,
    Macos,
    Cua,
}

impl ExecutorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Macos => "macos",
            Self::Cua => "cua",
        }
    }

    pub const fn status(self) -> &'static str {
        match self {
            Self::Browser => hyper_use_browser::STATUS,
            Self::Macos => hyper_use_macos::STATUS,
            Self::Cua => hyper_use_cua::STATUS,
        }
    }
}

impl fmt::Display for ExecutorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRequest {
    region_id: RegionId,
    action: Action,
}

impl ActionRequest {
    pub fn new(region_id: RegionId, action: Action) -> Self {
        Self { region_id, action }
    }

    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }

    pub fn action(&self) -> Action {
        self.action
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionReceipt {
    kind: ExecutorKind,
    region_id: RegionId,
    action: Action,
}

impl ActionReceipt {
    pub fn kind(&self) -> ExecutorKind {
        self.kind
    }
    pub fn region_id(&self) -> &RegionId {
        &self.region_id
    }
    pub fn action(&self) -> Action {
        self.action
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutorError {
    /// The backend exists only as a Phase 1 policy slot.
    NotImplemented(ExecutorKind),
    /// No entry in the policy order was present in the available set.
    NoneAvailable,
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented(kind) => {
                write!(f, "{kind} executor is not implemented in phase 1")
            }
            Self::NoneAvailable => f.write_str("no executor is available"),
        }
    }
}

impl std::error::Error for ExecutorError {}

pub trait ActionExecutor {
    fn kind(&self) -> ExecutorKind;

    fn execute(&self, request: &ActionRequest) -> Result<ActionReceipt, ExecutorError>;
}

/// Backend that always refuses. Used until a real browser, AX, or CUA driver exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StubExecutor {
    kind: ExecutorKind,
}

impl StubExecutor {
    pub const fn new(kind: ExecutorKind) -> Self {
        Self { kind }
    }
}

impl ActionExecutor for StubExecutor {
    fn kind(&self) -> ExecutorKind {
        self.kind
    }

    fn execute(&self, _request: &ActionRequest) -> Result<ActionReceipt, ExecutorError> {
        Err(ExecutorError::NotImplemented(self.kind))
    }
}

/// First kind in [`DEFAULT_POLICY_ORDER`] that is also in `available`.
pub fn select_executor(available: &[ExecutorKind]) -> Result<ExecutorKind, ExecutorError> {
    for kind in DEFAULT_POLICY_ORDER {
        if available.contains(&kind) {
            return Ok(kind);
        }
    }
    Err(ExecutorError::NoneAvailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_prefers_browser_then_macos_then_cua() {
        assert_eq!(
            DEFAULT_POLICY_ORDER,
            [ExecutorKind::Browser, ExecutorKind::Macos, ExecutorKind::Cua]
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Browser]).unwrap(),
            ExecutorKind::Browser
        );
        assert_eq!(
            select_executor(&[ExecutorKind::Cua, ExecutorKind::Macos]).unwrap(),
            ExecutorKind::Macos
        );
        assert_eq!(select_executor(&[ExecutorKind::Cua]).unwrap(), ExecutorKind::Cua);
        let err = select_executor(&[]).unwrap_err();
        assert_eq!(err, ExecutorError::NoneAvailable);
        assert_eq!(err.to_string(), "no executor is available");
    }

    #[test]
    fn stubs_refuse_to_act() {
        let request = ActionRequest::new(RegionId::try_new("nav-settings").unwrap(), Action::Click);
        for kind in DEFAULT_POLICY_ORDER {
            let executor = StubExecutor::new(kind);
            let err = executor.execute(&request).unwrap_err();
            assert_eq!(err, ExecutorError::NotImplemented(kind));
            assert!(err.to_string().contains("not implemented"));
            assert!(kind.status().contains("not implemented") || kind.status().contains("later phase"));
        }
        assert_eq!(hyper_use_browser::BrowserStub.status(), hyper_use_browser::STATUS);
        assert_eq!(hyper_use_macos::MacosStub.status(), hyper_use_macos::STATUS);
        assert_eq!(hyper_use_cua::CuaStub.status(), hyper_use_cua::STATUS);
    }
}
