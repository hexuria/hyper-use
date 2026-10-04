use std::fmt;

/// Failure talking to a CDP endpoint or a replay script.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CdpError {
    /// The script has no further response for this method.
    NoScriptedResponse {
        method: String,
    },
    /// The script listed this method, but the JSON params differ.
    ParamsMismatch {
        method: String,
    },
    /// CDP returned an `error` object. Callers may try the next act tier.
    Protocol {
        message: String,
    },
    BadJson {
        message: String,
    },
    /// The script itself is not usable.
    BadScript {
        message: String,
    },
    Transport {
        message: String,
    },
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScriptedResponse { method } => {
                write!(f, "no scripted CDP response for `{method}`")
            }
            Self::ParamsMismatch { method } => {
                write!(f, "scripted CDP params do not match the call to `{method}`")
            }
            Self::Protocol { message } => write!(f, "CDP error: {message}"),
            Self::BadJson { message } => write!(f, "invalid CDP JSON: {message}"),
            Self::BadScript { message } => write!(f, "invalid CDP script: {message}"),
            Self::Transport { message } => write!(f, "CDP transport: {message}"),
        }
    }
}

impl std::error::Error for CdpError {}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrowserError {
    Cdp(CdpError),
    NotObserved,
    UnknownRegion(String),
    UnsupportedAction(String),
    MissingObjectId,
    BadViewport(String),
    DuplicateRegion(String),
    Verify(crate::verify::VerifyError),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cdp(err) => write!(f, "{err}"),
            Self::NotObserved => f.write_str("browser session has no observation yet"),
            Self::UnknownRegion(id) => write!(f, "unknown region `{id}`"),
            Self::UnsupportedAction(action) => {
                write!(f, "browser session cannot perform `{action}`")
            }
            Self::MissingObjectId => f.write_str("DOM.resolveNode returned no objectId"),
            Self::BadViewport(message) => write!(f, "viewport: {message}"),
            Self::DuplicateRegion(id) => write!(f, "duplicate fused region `{id}`"),
            Self::Verify(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BrowserError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cdp(err) => Some(err),
            Self::Verify(err) => Some(err),
            _ => None,
        }
    }
}

impl From<CdpError> for BrowserError {
    fn from(value: CdpError) -> Self {
        Self::Cdp(value)
    }
}

/// How a press was sent. Declaration order is the preference order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActMechanism {
    /// `Runtime.callFunctionOn` of `function(){this.click()}`.
    DomSemantic,
    /// `DOM.focus` on the node id. No pointer event.
    CdpElement,
    /// `Input.dispatchMouseEvent` at the region center.
    Coordinate,
}

impl ActMechanism {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DomSemantic => "dom-semantic",
            Self::CdpElement => "cdp-element",
            Self::Coordinate => "coordinate",
        }
    }
}

impl fmt::Display for ActMechanism {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
