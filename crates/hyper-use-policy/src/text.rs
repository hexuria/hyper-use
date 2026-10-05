//! TextResolver: TYPE_TEXT payload generation is *not* PUA.

use std::fmt;

use crate::goal::AgentGoal;

/// Context for resolving typed text. Fingerprint so stale resolves are dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextContext {
    pub goal: AgentGoal,
    pub field_label: String,
    pub field_role: String,
    /// Observation / action-space fingerprint bits the resolve was based on.
    pub context_fingerprint: u64,
}

impl TextContext {
    pub fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for b in self.goal.as_str().as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for b in self.field_label.as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= self.context_fingerprint;
        hash = hash.wrapping_mul(0x100000001b3);
        hash
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextResolution {
    pub text: String,
    pub context_fingerprint: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    Ambiguous,
    Missing,
    Invalid(String),
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ambiguous => f.write_str("text resolution ambiguous"),
            Self::Missing => f.write_str("no text value found in goal/context"),
            Self::Invalid(msg) => write!(f, "invalid text: {msg}"),
        }
    }
}

impl std::error::Error for TextError {}

pub trait TextResolver {
    fn resolve(&mut self, context: &TextContext) -> Result<TextResolution, TextError>;
}

/// Pulls an obvious quoted or `into <field>:` value from the goal. No model.
#[derive(Clone, Debug, Default)]
pub struct DeterministicTextResolver;

impl TextResolver for DeterministicTextResolver {
    fn resolve(&mut self, context: &TextContext) -> Result<TextResolution, TextError> {
        let goal = context.goal.as_str();
        if let Some(quoted) = first_quoted(goal) {
            if quoted.is_empty() {
                return Err(TextError::Missing);
            }
            return Ok(TextResolution {
                text: quoted,
                context_fingerprint: context.fingerprint(),
            });
        }
        // Pattern: type <value> into <field>
        if let Some(v) = type_into_pattern(goal, &context.field_label) {
            return Ok(TextResolution {
                text: v,
                context_fingerprint: context.fingerprint(),
            });
        }
        // Pattern: fill <field> with <value>
        if let Some(v) = fill_with_pattern(goal, &context.field_label) {
            return Ok(TextResolution {
                text: v,
                context_fingerprint: context.fingerprint(),
            });
        }
        Err(TextError::Missing)
    }
}

fn first_quoted(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                return Some(s[start..i].to_owned());
            }
            return None;
        }
        i += 1;
    }
    None
}

fn type_into_pattern(goal: &str, field: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    let field_l = field.to_ascii_lowercase();
    // "type VALUE into FIELD"
    let type_idx = lower.find("type ")?;
    let into_idx = lower.find(" into ")?;
    if into_idx <= type_idx {
        return None;
    }
    let value = goal[type_idx + 5..into_idx].trim();
    let after = &lower[into_idx + 6..];
    if !after.contains(&field_l) && !field_l.is_empty() {
        // Still allow if field label empty / generic.
        if !field.is_empty() {
            return None;
        }
    }
    if value.is_empty() {
        return None;
    }
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_owned())
}

fn fill_with_pattern(goal: &str, field: &str) -> Option<String> {
    let lower = goal.to_ascii_lowercase();
    let field_l = field.to_ascii_lowercase();
    if !field_l.is_empty() && !lower.contains(&field_l) {
        return None;
    }
    let with_idx = lower.find(" with ")?;
    let value = goal[with_idx + 6..].trim();
    if value.is_empty() {
        return None;
    }
    Some(value.trim_matches(|c| c == '"' || c == '\'').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_value() {
        let mut r = DeterministicTextResolver;
        let ctx = TextContext {
            goal: AgentGoal::new(r#"Type "Zürich" into City"#),
            field_label: "City".into(),
            field_role: "text_field".into(),
            context_fingerprint: 1,
        };
        let res = r.resolve(&ctx).unwrap();
        assert_eq!(res.text, "Zürich");
    }

    #[test]
    fn type_into_pattern_works() {
        let mut r = DeterministicTextResolver;
        let ctx = TextContext {
            goal: AgentGoal::new("type hello world into Search"),
            field_label: "Search".into(),
            field_role: "text_field".into(),
            context_fingerprint: 2,
        };
        assert_eq!(r.resolve(&ctx).unwrap().text, "hello world");
    }

    #[test]
    fn missing_when_no_value() {
        let mut r = DeterministicTextResolver;
        let ctx = TextContext {
            goal: AgentGoal::new("click Submit"),
            field_label: "Email".into(),
            field_role: "text_field".into(),
            context_fingerprint: 3,
        };
        assert_eq!(r.resolve(&ctx), Err(TextError::Missing));
    }
}
