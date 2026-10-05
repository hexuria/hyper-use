//! Integer evidence for browser candidates. No float thresholds.

use hyper_use_core::{tokenize, ActionKind, ObservedAction, Role};
use pua_core::Confidence;
use pua_lexicon::overlap;
use pua_text::{normalize, NormalizeConfig};

/// Score how well `action` matches `goal`. Returns PUA confidence millis 0..=1000.
pub fn score_action(goal: &str, action: &ObservedAction) -> Confidence {
    let label = action.label();
    let kind = action.kind();

    if eq_fold(goal, label) {
        return Confidence::MAX;
    }
    if kind.is_control() && eq_fold(goal, kind.as_str()) {
        return Confidence::MAX;
    }
    if control_keyword_hit(goal, kind) {
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
    fn fold_strips_case_and_space() {
        assert!(eq_fold("Sign In", "sign in"));
        assert!(!eq_fold("Sign In", "Sign Out"));
    }
}
