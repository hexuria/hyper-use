//! `ultra-instinct replay --diary <dir>` — the offline arena: re-score every
//! recorded decision with the offline policy and report where it agrees,
//! where it would abstain, and where it would regress.
//!
//! Deterministic by construction: the replayed policy is `InstinctPolicy`
//! (the only offline arm), the menu is the recorded `offered` list, the goal
//! is the recorded clause, and files are read in sorted order. Remote arms
//! (`jev`, `clef-*`) cannot replay — the diary records what *they* chose,
//! and replay asks what *Instinct* would have done in the same situation.
//! That comparison is exactly the dojo's progress metric.

use std::path::{Path, PathBuf};

use aui_core::{ActionId, ActionKind};
use aui_dojo::{action_space, read_diary, DecisionLine, DiaryLine};
use aui_policy::{AgentGoal, BrowserPolicy, HistoryEntry, InstinctPolicy, PolicyOutcome};

use crate::CliError;

#[derive(Default)]
struct ReplayArgs {
    diary: String,
    lessons: Option<String>,
}

#[derive(Default)]
struct Report {
    decisions: u32,
    agree: u32,
    would_abstain: u32,
    regress: u32,
    /// Decisions the policy could not re-score (empty rebuilt menu …).
    unscored: u32,
}

impl Report {
    fn add(&mut self, other: &Report) {
        self.decisions += other.decisions;
        self.agree += other.agree;
        self.would_abstain += other.would_abstain;
        self.regress += other.regress;
        self.unscored += other.unscored;
    }

    fn render(&self) -> String {
        format!(
            "{} decisions — agree {}, would-abstain {}, regress {}{}",
            self.decisions,
            self.agree,
            self.would_abstain,
            self.regress,
            if self.unscored > 0 {
                format!(", unscored {}", self.unscored)
            } else {
                String::new()
            },
        )
    }
}

/// How a replayed verdict compares to the recorded one.
enum Verdict {
    /// Same verdict: chose the same action, or both abstained.
    Agree,
    /// The recorded run chose; the replay abstains.
    WouldAbstain,
    /// Any other divergence: a different action, or a choice where the
    /// recorded run abstained — a behaviour change that could regress.
    Regress,
}

impl Verdict {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Agree => "agree",
            Self::WouldAbstain => "would-abstain",
            Self::Regress => "regress",
        }
    }
}

pub fn replay_command(args: &[String]) -> Result<String, CliError> {
    let parsed = parse_args(args)?;
    let dir = Path::new(&parsed.diary);
    if !dir.is_dir() {
        return Err(CliError::Io {
            path: parsed.diary.clone(),
            message: "not a diary directory".to_owned(),
        });
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| CliError::Io {
            path: parsed.diary.clone(),
            message: e.to_string(),
        })?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().and_then(|e| e.to_str()) == Some("jsonl")).then_some(path)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(CliError::Io {
            path: parsed.diary.clone(),
            message: "no .jsonl diaries".to_owned(),
        });
    }

    let mut out = String::new();
    if let Some(store) = &parsed.lessons {
        out.push_str(&format!(
            "lessons {store}: ignored (the lesson store lands in work item 3)\n"
        ));
    }
    let mut total = Report::default();
    for file in &files {
        let report = replay_file(file, &mut out)?;
        out.push_str(&format!("{}: {}\n", file.display(), report.render()));
        total.add(&report);
    }
    out.push_str(&format!("total: {}\n", total.render()));
    Ok(out)
}

fn replay_file(path: &Path, out: &mut String) -> Result<Report, CliError> {
    let lines = read_diary(path).map_err(|e| CliError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    // One policy per run: mirrors the live call sequence (instinct-side
    // state such as habituation still flows through recorded history).
    let mut policy = InstinctPolicy::default();
    let mut report = Report::default();
    for line in &lines {
        let DiaryLine::Decision(decision) = line else {
            continue;
        };
        report.decisions += 1;
        match score_decision(&mut policy, decision) {
            Some((verdict, replayed)) => {
                match verdict {
                    Verdict::Agree => report.agree += 1,
                    Verdict::WouldAbstain => report.would_abstain += 1,
                    Verdict::Regress => report.regress += 1,
                }
                if !matches!(verdict, Verdict::Agree) {
                    out.push_str(&format!(
                        "  seq {} {} \"{}\": recorded {} -> replay {}\n",
                        decision.seq,
                        verdict.as_str(),
                        decision.clause,
                        describe_choice(&decision.choice),
                        describe_outcome(&replayed),
                    ));
                }
            }
            None => {
                report.unscored += 1;
                out.push_str(&format!(
                    "  seq {} unscored \"{}\": rebuilt menu was undecidable\n",
                    decision.seq, decision.clause,
                ));
            }
        }
    }
    Ok(report)
}

/// Re-score one recorded decision. `None` when the policy hard-fails on the
/// rebuilt menu (e.g. every offered line was malformed).
fn score_decision(
    policy: &mut InstinctPolicy,
    decision: &DecisionLine,
) -> Option<(Verdict, PolicyOutcome)> {
    let space = action_space(decision);
    let goal = AgentGoal::new(decision.clause.clone());
    let history = history_entries(decision);
    let outcome = policy.decide(&space, &goal, &history).ok()?;
    let verdict = compare(decision, &outcome);
    Some((verdict, outcome))
}

fn history_entries(decision: &DecisionLine) -> Vec<HistoryEntry> {
    decision
        .history
        .iter()
        .filter_map(|entry| {
            Some(HistoryEntry {
                step: entry.step,
                action_id: ActionId::try_new(&entry.action_id).ok()?,
                kind: ActionKind::parse(&entry.kind)?,
                label: entry.label.clone(),
                verification: entry.verification.clone(),
            })
        })
        .collect()
}

fn compare(decision: &DecisionLine, replayed: &PolicyOutcome) -> Verdict {
    match (&decision.choice, replayed) {
        (Some(recorded), PolicyOutcome::Choice(d))
            if recorded.action_id == d.action_id.as_str() =>
        {
            Verdict::Agree
        }
        (None, PolicyOutcome::Abstain { .. }) => Verdict::Agree,
        (Some(_), PolicyOutcome::Abstain { .. }) => Verdict::WouldAbstain,
        _ => Verdict::Regress,
    }
}

fn describe_choice(choice: &Option<aui_dojo::ChoiceLine>) -> String {
    match choice {
        Some(c) => format!("{} ({})", c.action_id, c.kind),
        None => "abstain".to_owned(),
    }
}

fn describe_outcome(outcome: &PolicyOutcome) -> String {
    match outcome {
        PolicyOutcome::Choice(d) => format!("{} ({})", d.action_id.as_str(), d.kind.as_str()),
        PolicyOutcome::Abstain { reason, .. } => format!("abstain ({reason})"),
    }
}

fn parse_args(args: &[String]) -> Result<ReplayArgs, CliError> {
    let mut parsed = ReplayArgs::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let (name, value) = match arg.split_once('=') {
            Some((flag, v)) => (flag, Some(v.to_owned())),
            None => (arg.as_str(), None),
        };
        match name {
            "--diary" => {
                index += 1;
                let value = value
                    .or_else(|| args.get(index).cloned())
                    .ok_or(CliError::MissingValue("--diary"))?;
                if !parsed.diary.is_empty() {
                    return Err(CliError::DuplicateFlag("--diary"));
                }
                parsed.diary = value;
            }
            "--lessons" => {
                index += 1;
                let value = value
                    .or_else(|| args.get(index).cloned())
                    .ok_or(CliError::MissingValue("--lessons"))?;
                if parsed.lessons.is_some() {
                    return Err(CliError::DuplicateFlag("--lessons"));
                }
                parsed.lessons = Some(value);
            }
            _ => return Err(CliError::UnknownFlag(arg.clone())),
        }
        index += 1;
    }
    if parsed.diary.is_empty() {
        return Err(CliError::MissingValue("--diary"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_requires_diary() {
        let err = replay_command(&[]).unwrap_err();
        assert!(matches!(err, CliError::MissingValue("--diary")), "{err:?}");
    }

    #[test]
    fn replay_rejects_unknown_flag() {
        let err = replay_command(&[
            "--diary".to_owned(),
            "/tmp".to_owned(),
            "--bogus".to_owned(),
        ])
        .unwrap_err();
        assert!(matches!(err, CliError::UnknownFlag(_)), "{err:?}");
    }

    #[test]
    fn replay_errors_on_empty_dir() {
        let dir = std::env::temp_dir().join(format!("aui-replay-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = replay_command(&["--diary".to_owned(), dir.display().to_string()]).unwrap_err();
        assert!(matches!(err, CliError::Io { .. }), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
