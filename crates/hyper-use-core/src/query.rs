use crate::error::CoreError;
use crate::text::tokenize;
use crate::vocab::{Action, Role, Zone};

/// A structured locate request. Absent fields are unconstrained.
///
/// Text, when present, must contain at least one alphanumeric token. The same
/// query value is pure data: ranking code is not allowed to consult a clock
/// or a random source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocateQuery {
    text: Option<String>,
    role: Option<Role>,
    position: Option<Zone>,
    action: Option<Action>,
}

impl LocateQuery {
    pub const fn new() -> Self {
        Self {
            text: None,
            role: None,
            position: None,
            action: None,
        }
    }

    pub fn text(mut self, text: impl Into<String>) -> Result<Self, CoreError> {
        let text = text.into();
        if tokenize(&text).is_empty() {
            return Err(CoreError::EmptyQueryText);
        }
        self.text = Some(text);
        Ok(self)
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = Some(role);
        self
    }

    pub fn position(mut self, position: Zone) -> Self {
        self.position = Some(position);
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.action = Some(action);
        self
    }

    pub fn text_ref(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub const fn role_ref(&self) -> Option<Role> {
        self.role
    }

    pub const fn position_ref(&self) -> Option<Zone> {
        self.position
    }

    pub const fn action_ref(&self) -> Option<Action> {
        self.action
    }
}

impl Default for LocateQuery {
    fn default() -> Self {
        Self::new()
    }
}
