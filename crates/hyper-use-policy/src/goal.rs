//! Agent goal: consumer-owned natural-language intent.

use std::fmt;

/// What the agent is trying to accomplish. Opaque to PUA except as evidence text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentGoal {
    text: String,
}

impl AgentGoal {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into().trim().to_owned(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

impl fmt::Display for AgentGoal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
