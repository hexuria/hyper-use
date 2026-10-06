//! Integer evidence for browser candidates. No float thresholds.

use std::collections::BTreeSet;

use aui_core::{tokenize, ActionKind, ActionSpace, ObservedAction, Role};
use instinct_core::Confidence;

use crate::text::quoted_literals;

/// Score how well `action` matches `goal`. Returns Instinct confidence millis 0..=1000.
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

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "at", "for", "with", "and", "or", "from", "into",
    "by", "as", "is", "are", "be", "it", "its", "this", "that", "these", "those", "then", "so",
    "all", "any", "only", "already", "use", "page", "site", "browser", "without", "changing",
    "anything", "stop", "say", "give", "up", "make", "sure", "me", "my", "your", "please",
];

const CONTEXT_PREPOSITIONS: &[&str] = &["in", "on", "at", "for", "with", "from"];

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

    let label_tokens = tokenize(label);
    let filtered_label_tokens: Vec<_> = label_tokens
        .iter()
        .filter(|token| !is_stopword(token))
        .cloned()
        .collect();
    let content_label_tokens = if filtered_label_tokens.is_empty() {
        &label_tokens
    } else {
        &filtered_label_tokens
    };
    let goal_tokens: BTreeSet<_> = instruction_tokens(goal)
        .into_iter()
        .filter(|token| !is_stopword(token))
        .collect();
    let unique_label_tokens: BTreeSet<_> =
        content_label_tokens.iter().map(String::as_str).collect();
    let hits = unique_label_tokens
        .iter()
        .filter(|token| goal_tokens.contains(**token))
        .count();
    let mut content_score = if content_label_tokens.is_empty() {
        0
    } else {
        let value = 400_u128 * hits as u128 / content_label_tokens.len() as u128
            + 50_u128 * hits.min(4) as u128;
        i32::try_from(value).unwrap_or(600)
    };

    if let Some(role) = action.role() {
        if role_mentioned(goal, role) {
            content_score += 80;
        }
    }
    let mut score = content_score.min(600);

    if content_label_tokens.first().is_some_and(|first| {
        content_label_tokens
            .iter()
            .all(|token| goal_tokens.contains(token))
            && clause_heads(goal).iter().any(|head| head == first)
    }) {
        score = score.max(950);
    }

    if quoted_literals(goal).iter().any(|(_, literal)| {
        let literal_tokens = tokenize(literal);
        !literal_tokens.is_empty()
            && label_tokens
                .windows(literal_tokens.len())
                .any(|window| window == literal_tokens.as_slice())
    }) {
        score = score.max(800);
    }

    if kind == ActionKind::Done && done_language(goal) {
        score += 100;
    }

    Confidence::saturating(score)
}

pub(crate) fn missing_quoted_target(goal: &str, space: &ActionSpace) -> bool {
    let target_labels = [ActionKind::Click, ActionKind::TypeText, ActionKind::Select]
        .into_iter()
        .flat_map(|kind| space.targets_of(kind));

    let labels: Vec<_> = target_labels
        .map(|action| tokenize(action.label()))
        .collect();
    quoted_literals(goal).iter().any(|(position, literal)| {
        if !quoted_target_kind(goal, *position, literal) {
            return false;
        }
        let literal_tokens = tokenize(literal);
        !literal_tokens.is_empty()
            && !labels.iter().any(|label| {
                label
                    .windows(literal_tokens.len())
                    .any(|window| window == literal_tokens.as_slice())
            })
    })
}

fn quoted_target_kind(goal: &str, opening_quote: usize, literal: &str) -> bool {
    let Some(opening) = goal[opening_quote..].chars().next() else {
        return false;
    };
    let closing = if opening == '"' { '"' } else { '”' };
    let suffix_start = opening_quote + opening.len_utf8() + literal.len() + closing.len_utf8();
    goal.get(suffix_start..)
        .and_then(|suffix| tokenize(suffix).into_iter().next())
        .is_some_and(|word| {
            matches!(
                word.as_str(),
                "button" | "link" | "tab" | "checkbox" | "option" | "field" | "menu"
            )
        })
}

fn clause_heads(goal: &str) -> Vec<String> {
    let masked_goal = mask_quoted_literals(goal);
    let mut heads = Vec::new();
    for clause in instruction_clauses(&masked_goal) {
        let mut part = Vec::new();
        for token in clause {
            if matches!(token.as_str(), "and" | "then") {
                push_clause_head(&mut part, &mut heads);
            } else {
                part.push(token);
            }
        }
        push_clause_head(&mut part, &mut heads);
    }
    heads
}

fn mask_quoted_literals(goal: &str) -> String {
    let mut masked = String::with_capacity(goal.len());
    let mut cursor = 0;
    for (opening_quote, literal) in quoted_literals(goal) {
        let Some(opening) = goal[opening_quote..].chars().next() else {
            continue;
        };
        let closing = if opening == '"' { '"' } else { '”' };
        let closing_end = opening_quote + opening.len_utf8() + literal.len() + closing.len_utf8();
        masked.push_str(&goal[cursor..opening_quote]);
        masked.push('\0');
        cursor = closing_end;
    }
    masked.push_str(&goal[cursor..]);
    masked
}

fn push_clause_head(tokens: &mut Vec<String>, heads: &mut Vec<String>) {
    if tokens
        .first()
        .is_some_and(|token| CONTEXT_PREPOSITIONS.contains(&token.as_str()))
    {
        tokens.clear();
        return;
    }
    if tokens
        .first()
        .is_some_and(|token| LEADING_VERBS.contains(&token.as_str()))
    {
        tokens.remove(0);
    }
    if let Some(head) = tokens.iter().find(|token| !is_stopword(token)) {
        heads.push(head.clone());
    }
    tokens.clear();
}

fn is_stopword(token: &str) -> bool {
    STOPWORDS.contains(&token)
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

fn eq_fold(a: &str, b: &str) -> bool {
    fold(a) == fold(b)
}

fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn instruction_tokens(goal: &str) -> Vec<String> {
    instruction_clauses(goal).into_iter().flatten().collect()
}

fn instruction_clauses(goal: &str) -> Vec<Vec<String>> {
    let mut clauses = Vec::new();
    for clause in goal.split(['.', ';', '!', '?', ',', '\n']) {
        let clause_tokens = tokenize(clause);
        if matches!(
            clause_tokens.first().map(String::as_str),
            Some("if" | "unless" | "otherwise")
        ) {
            continue;
        }
        clauses.push(clause_tokens);
    }
    clauses
}

fn matches_instruction_tokens(tokens: &[String], keys: &[&[&str]]) -> bool {
    keys.iter().any(|key| {
        tokens.windows(key.len()).any(|window| {
            window
                .iter()
                .zip(key.iter())
                .all(|(token, expected)| token.as_str() == *expected)
        })
    })
}

fn control_keyword_hit(goal: &str, kind: ActionKind) -> bool {
    let keys: &[&[&str]] = match kind {
        ActionKind::Done => &[&["done"], &["finished"], &["complete"], &["completed"]],
        ActionKind::Blocked => &[&["blocked"], &["stuck"], &["impossible"]],
        ActionKind::Wait => &[&["wait"], &["pause"], &["loading"]],
        ActionKind::ScrollDown => &[
            &["scroll", "down"],
            &["scroll", "page"],
            &["page", "down"],
            &["scrolldown"],
            &["pagedown"],
        ],
        ActionKind::ScrollUp => &[
            &["scroll", "up"],
            &["page", "up"],
            &["scrollup"],
            &["pageup"],
        ],
        ActionKind::Click => &[&["click"], &["press"], &["tap"]],
        ActionKind::TypeText => &[&["type"], &["enter"], &["fill"], &["input"]],
        ActionKind::Select => &[&["select"], &["choose"], &["pick"]],
        _ => &[],
    };
    matches_instruction_tokens(&instruction_tokens(goal), keys)
}

/// Score ceiling for a target-bound operation the goal did not name when it
/// named another one. Below Instinct Standard's min confidence.
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
        Role::TextField | Role::ComboBox => "text",
        Role::Checkbox => "checkbox",
        Role::MenuItem | Role::Option => "menu",
        Role::Tab => "tab",
        Role::Dialog => "dialog",
        Role::Navigation => "nav",
        Role::ListBox => "list",
        _ => return false,
    };
    g.contains(key)
}

fn done_language(goal: &str) -> bool {
    matches_instruction_tokens(
        &instruction_tokens(goal),
        &[
            &["done"],
            &["finish"],
            &["finished"],
            &["complete"],
            &["completed"],
            &["success"],
            &["successful"],
            &["submitted"],
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aui_core::parse_fixture;

    fn action_from_fixture(fixture: &str, action_id: &str) -> ObservedAction {
        let manifold = parse_fixture(fixture).unwrap();
        ActionSpace::from_manifold(&manifold)
            .get_str(action_id)
            .unwrap()
            .clone()
    }

    #[test]
    fn inbox_content_evidence_stays_below_the_selection_threshold() {
        let action = action_from_fixture(
            r#"
            viewport w=800 h=600
            region id=inbox role=link label="Inbox" x=10 y=10 w=100 h=24 actions=click
            "#,
            "CLICK:inbox",
        );
        let goal = "Print all messages in the inbox with the \"Print all\" button. The page is already open in the browser. Use only this site. If the task cannot be done on this site, stop and say so (give up) without changing anything.";
        assert!(score_action(goal, &action).get() <= 600);
    }

    #[test]
    fn clause_head_verb_scores_950() {
        let action = action_from_fixture(
            r#"
            viewport w=800 h=600
            region id=save role=button label="Save" x=10 y=10 w=80 h=24 actions=click
            "#,
            "CLICK:save",
        );
        assert_eq!(
            score_action("In Settings, save the signature.", &action).get(),
            950
        );
    }

    #[test]
    fn quoted_name_scores_800() {
        let action = action_from_fixture(
            r#"
            viewport w=800 h=600
            region id=q3 role=link label="Q3 launch checklist, from Maya Reyes" x=10 y=10 w=280 h=24 actions=click
            "#,
            "CLICK:q3",
        );
        assert_eq!(
            score_action(
                "Open the email \"Q3 launch checklist\" from Maya Reyes.",
                &action
            )
            .get(),
            800
        );
    }

    #[test]
    fn content_tier_caps_role_mention_at_600() {
        let action = action_from_fixture(
            r#"
            viewport w=800 h=600
            region id=settings role=button label="User settings billing" x=10 y=10 w=180 h=24 actions=click
            "#,
            "CLICK:settings",
        );
        assert_eq!(
            score_action("Update the user settings billing button.", &action).get(),
            600
        );
    }

    #[test]
    fn clause_heads_skip_quoted_tokens_and_leading_verbs() {
        assert_eq!(clause_heads(r#"Type "save" into Notes"#), ["notes"]);
    }

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

    #[test]
    fn control_keywords_match_whole_token_sequences() {
        assert!(!control_keyword_hit(
            "Open the city center map",
            ActionKind::TypeText
        ));
        assert!(!control_keyword_hit("Express checkout", ActionKind::Click));
        assert!(control_keyword_hit(
            "Type \"man\" into City then click Manila",
            ActionKind::TypeText
        ));
        assert!(control_keyword_hit(
            "Type \"man\" into City then click Manila",
            ActionKind::Click
        ));
        assert!(control_keyword_hit(
            "Click Save. Say done when finished",
            ActionKind::Done
        ));
    }

    #[test]
    fn scroll_keywords_match_adjacent_tokens_and_compact_forms() {
        assert!(control_keyword_hit("scroll down", ActionKind::ScrollDown));
        assert!(control_keyword_hit(
            "Scroll  Down please",
            ActionKind::ScrollDown
        ));
        assert!(control_keyword_hit("page up", ActionKind::ScrollUp));
        assert!(control_keyword_hit("scrolldown", ActionKind::ScrollDown));
        assert!(control_keyword_hit("pagedown", ActionKind::ScrollDown));
        assert!(control_keyword_hit("pageup", ActionKind::ScrollUp));
        assert!(control_keyword_hit("scrollup", ActionKind::ScrollUp));
    }

    #[test]
    fn conditional_clauses_do_not_trigger_control_or_done_language() {
        let goal = "Star the email \"Weekly sync\". If the task cannot be done on this site, stop and say so (give up) without changing anything.";
        assert!(!control_keyword_hit(goal, ActionKind::Done));
        assert!(!done_language(goal));
        assert!(!control_keyword_hit(
            "If it is loading, click Retry",
            ActionKind::Wait
        ));
        assert!(control_keyword_hit(
            "If it is loading, click Retry",
            ActionKind::Click
        ));
    }
}
