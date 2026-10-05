//! Integer evidence for browser candidates. No float thresholds.

use hyper_use_core::{tokenize, ActionKind, ObservedAction, Role};
use pua_core::Confidence;
use pua_lexicon::overlap;
use pua_text::{normalize, NormalizeConfig};

/// Score how well `action` matches `goal`. Returns PUA confidence millis 0..=1000.
///
/// Label evidence is the better of the whole goal and its *target phrase*
/// (goal minus quoted payload, leading operation verb, and the preposition
/// after them): in `Type "rust" into Search` the verb is operation evidence
/// and `"rust"` is the TextResolver's payload; only `Search` names a target.
pub fn score_action(goal: &str, action: &ObservedAction) -> Confidence {
    let whole = score_action_text(goal, action);
    match target_phrase(goal) {
        Some(phrase) if phrase != goal => {
            let p = score_action_text(&phrase, action);
            if p.get() > whole.get() {
                p
            } else {
                whole
            }
        }
        _ => whole,
    }
}

/// Operation verbs stripped from the front of a goal for target evidence.
const LEADING_VERBS: &[&str] = &[
    "click", "press", "tap", "type", "enter", "fill", "input", "select", "choose", "pick", "open",
];

/// Connectives dropped right after a stripped verb or quoted payload.
const CONNECTIVES: &[&str] = &["into", "in", "on", "to", "the", "a", "an", "from", "as"];

/// The goal with quoted payloads, a leading operation verb, and the
/// connectives that followed them removed. `None` when nothing is left.
pub fn target_phrase(goal: &str) -> Option<String> {
    // Drop quoted segments ("…" or '…'), marking where they were.
    let mut unquoted = String::with_capacity(goal.len());
    let mut chars = goal.chars();
    while let Some(c) = chars.next() {
        if c == '"' || c == '\u{201c}' {
            let close = if c == '"' { '"' } else { '\u{201d}' };
            for d in chars.by_ref() {
                if d == close {
                    break;
                }
            }
            unquoted.push_str(" \u{0} ");
        } else {
            unquoted.push(c);
        }
    }
    let mut words: Vec<&str> = unquoted.split_whitespace().collect();
    let mut stripped = false;
    if let Some(first) = words.first() {
        if LEADING_VERBS.iter().any(|v| first.eq_ignore_ascii_case(v)) {
            words.remove(0);
            stripped = true;
        }
    }
    let mut out = Vec::new();
    let mut after_marker = stripped;
    for w in words {
        if w == "\u{0}" {
            after_marker = true;
            continue;
        }
        if after_marker && CONNECTIVES.iter().any(|c| w.eq_ignore_ascii_case(c)) {
            continue;
        }
        after_marker = false;
        out.push(w);
    }
    let phrase = out.join(" ");
    if phrase.is_empty() {
        None
    } else {
        Some(phrase)
    }
}

fn score_action_text(goal: &str, action: &ObservedAction) -> Confidence {
    let label = action.label();
    let kind = action.kind();

    if eq_fold(goal, label) {
        return Confidence::MAX;
    }
    if kind.is_control() && eq_fold(goal, kind.as_str()) {
        return Confidence::MAX;
    }
    // Control verbs ("done", "wait", "scroll down") pick the control itself.
    // Operation verbs on target-bound kinds ("type", "select") are operation
    // evidence (see `score_operation`), not a cap on label evidence.
    if kind.is_control() && control_keyword_hit(goal, kind) {
        return Confidence::saturating(920);
    }

    let mut score = Confidence::ZERO;

    if let Ok(ov) = lexical_overlap(goal, label) {
        score = score.saturating_add(ov);
    }

    let g = fold(goal);
    let l = fold(label);
    if !l.is_empty() && g.contains(&l) {
        score = score.saturating_add(Confidence::saturating(250));
    } else if !g.is_empty() && l.contains(&g) {
        score = score.saturating_add(Confidence::saturating(200));
    }

    let goal_tokens = tokenize(goal);
    let label_tokens = tokenize(label);
    if !goal_tokens.is_empty() {
        let hits = goal_tokens
            .iter()
            .filter(|t| label_tokens.iter().any(|lt| lt == *t))
            .count();
        if hits == goal_tokens.len() && hits > 0 {
            score = score.saturating_add(Confidence::saturating(300));
        } else if hits > 0 {
            let part = i32::try_from(hits.saturating_mul(150) / goal_tokens.len()).unwrap_or(0);
            score = score.saturating_add(Confidence::saturating(part));
        }
    }

    if let Some(role) = action.role() {
        if role_mentioned(goal, role) {
            score = score.saturating_add(Confidence::saturating(80));
        }
    }

    if kind == ActionKind::Done && done_language(goal) {
        score = score.saturating_add(Confidence::saturating(100));
    }

    score
}

/// Score an operation kind given the goal and the best target (if any) for that kind.
pub fn score_operation(
    goal: &str,
    kind: ActionKind,
    best_target: Option<&ObservedAction>,
) -> Confidence {
    let mut score = Confidence::ZERO;
    if control_keyword_hit(goal, kind) {
        score = score.saturating_add(Confidence::saturating(920));
    }
    if eq_fold(goal, kind.as_str()) {
        return Confidence::MAX;
    }
    if let Some(target) = best_target {
        let t = score_action(goal, target);
        if t.get() > score.get() {
            score = t;
        }
        // The goal names a different target-bound operation ("type … into
        // Search" offers CLICK and TYPE_TEXT on the same field): an unnamed
        // operation cannot outrank the named one on label evidence alone.
        let named = named_target_operations(goal);
        if !named.is_empty() && !named.contains(&kind) {
            score = Confidence::saturating(i32::from(score.get().min(UNNAMED_OPERATION_CAP)));
        }
    } else if kind.is_control() {
        // Weak prior so controls remain choosable when goal is vague.
        score = score.saturating_add(Confidence::saturating(50));
    }
    score
}

fn lexical_overlap(goal: &str, candidate: &str) -> Result<Confidence, ()> {
    let cfg = NormalizeConfig::default();
    let q = normalize(goal, cfg).map_err(|_| ())?;
    let c = normalize(candidate, cfg).map_err(|_| ())?;
    overlap(&q, &c).map_err(|_| ())
}

fn eq_fold(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn control_keyword_hit(goal: &str, kind: ActionKind) -> bool {
    let g = fold(goal);
    let keys: &[&str] = match kind {
        ActionKind::Done => &["done", "finished", "complete", "completed"],
        ActionKind::Blocked => &["blocked", "stuck", "impossible"],
        ActionKind::Wait => &["wait", "pause", "loading"],
        ActionKind::ScrollDown => &["scrolldown", "scrollpage", "pagedown"],
        ActionKind::ScrollUp => &["scrollup", "pageup"],
        ActionKind::Click => &["click", "press", "tap"],
        ActionKind::TypeText => &["type", "enter", "fill", "input"],
        ActionKind::Select => &["select", "choose", "pick"],
        _ => &[],
    };
    keys.iter().any(|k| g.contains(k))
}

/// Score ceiling for a target-bound operation the goal did not name when it
/// named another one. Below PUA Standard's min confidence.
const UNNAMED_OPERATION_CAP: i16 = 500;

/// Target-bound operations whose verb appears in the goal.
fn named_target_operations(goal: &str) -> Vec<ActionKind> {
    [ActionKind::Click, ActionKind::TypeText, ActionKind::Select]
        .into_iter()
        .filter(|kind| control_keyword_hit(goal, *kind))
        .collect()
}

fn role_mentioned(goal: &str, role: Role) -> bool {
    let g = fold(goal);
    let key = match role {
        Role::Button => "button",
        Role::Link => "link",
        Role::TextField => "text",
        Role::Checkbox => "checkbox",
        Role::MenuItem => "menu",
        Role::Tab => "tab",
        Role::Dialog => "dialog",
        Role::Navigation => "nav",
        _ => return false,
    };
    g.contains(key)
}

fn done_language(goal: &str) -> bool {
    let g = fold(goal);
    ["done", "finish", "complete", "success", "submitted"]
        .iter()
        .any(|k| g.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_phrase_strips_verb_payload_and_connectives() {
        assert_eq!(target_phrase("Click Go").as_deref(), Some("Go"));
        assert_eq!(
            target_phrase(r#"Type "rust ownership" into Search"#).as_deref(),
            Some("Search")
        );
        assert_eq!(
            target_phrase(r#"Select "Business" in Cabin class"#).as_deref(),
            Some("Cabin class")
        );
        // No verb: connectives inside a label are kept.
        assert_eq!(target_phrase("Sign in").as_deref(), Some("Sign in"));
        assert_eq!(target_phrase("click").as_deref(), None);
    }

    #[test]
    fn fold_strips_case_and_space() {
        assert!(eq_fold("Sign In", "sign in"));
        assert!(!eq_fold("Sign In", "Sign Out"));
    }
}
