use std::fmt;

#[derive(Debug)]
pub enum AgentError {
    Browser(String),
    Policy(String),
    Guard(String),
    Ticket(String),
    Text(String),
    InvalidState(&'static str),
    MaxSteps,
    MaxPolicyCalls,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Browser(m) => write!(f, "browser: {m}"),
            Self::Policy(m) => write!(f, "policy: {m}"),
            Self::Guard(m) => write!(f, "guard: {m}"),
            Self::Ticket(m) => write!(f, "ticket: {m}"),
            Self::Text(m) => write!(f, "text: {m}"),
            Self::InvalidState(m) => write!(f, "invalid state: {m}"),
            Self::MaxSteps => f.write_str("max steps exceeded"),
            Self::MaxPolicyCalls => f.write_str("max policy calls exceeded"),
        }
    }
}

impl std::error::Error for AgentError {}
