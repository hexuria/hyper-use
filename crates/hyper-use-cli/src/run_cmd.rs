//! `hyper-use run`: the owned agent loop from the command line.
//!
//! ```text
//! hyper-use run --goal <text> --cdp [url] [--url <page>] [--max-steps N]
//! hyper-use run --goal <text> --fixture <replay.cdp.json>   # full loop over a CDP replay
//! hyper-use run --goal <text> --fixture <page.manifold>     # predict only (dry run)
//! ```
//!
//! Live mode drives the attached Chrome page: observe → PUA → gate → ticket →
//! executor (revalidate + consume) → input → observe → verify, until DONE,
//! BLOCKED, abstain, or a bound. No LLM and no MCP are involved; PUA abstains
//! rather than guessing.

use hyper_use_agent::{Agent, AgentBuilder, AgentOutcome, BrowserRuntime, MockBrowser};
use hyper_use_browser::{BrowserSession, CdpTransport, ReplayTransport, WebSocketTransport};
use hyper_use_core::parse_fixture;
use hyper_use_policy::{BrowserPolicy, PuaPolicy, TextResolver};

use crate::CliError;

struct RunArgs {
    goal: String,
    cdp: Option<String>,
    url: Option<String>,
    fixture: Option<String>,
    max_steps: u32,
}

fn parse(args: &[String]) -> Result<RunArgs, CliError> {
    let mut goal = None;
    let mut cdp = None;
    let mut url = None;
    let mut fixture = None;
    let mut max_steps = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_owned())),
            _ => (arg, None),
        };
        let mut value = |flag: &'static str| -> Result<String, CliError> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            i += 1;
            match args.get(i) {
                Some(v) if !v.starts_with("--") => Ok(v.clone()),
                _ => Err(CliError::MissingValue(flag)),
            }
        };
        match name {
            "--goal" => set(&mut goal, "--goal", value("--goal")?)?,
            "--url" => set(&mut url, "--url", value("--url")?)?,
            "--fixture" => set(&mut fixture, "--fixture", value("--fixture")?)?,
            "--max-steps" => {
                let raw = value("--max-steps")?;
                let n: u32 = raw.parse().map_err(|_| CliError::BadNumber {
                    flag: "--max-steps",
                    value: raw.clone(),
                })?;
                set(&mut max_steps, "--max-steps", n)?;
            }
            "--cdp" => {
                if cdp.is_some() {
                    return Err(CliError::DuplicateFlag("--cdp"));
                }
                if let Some(v) = inline.clone() {
                    cdp = Some(v);
                } else if args.get(i + 1).is_some_and(|v| !v.starts_with("--")) {
                    i += 1;
                    cdp = Some(args[i].clone());
                } else {
                    cdp = Some(hyper_use_browser::DEFAULT_CDP_HTTP.to_owned());
                }
            }
            _ => return Err(CliError::UnknownFlag(arg.to_owned())),
        }
        i += 1;
    }
    let goal = goal.ok_or(CliError::MissingValue("--goal"))?;
    if goal.trim().is_empty() {
        return Err(CliError::EmptyText);
    }
    if cdp.is_some() && fixture.is_some() {
        return Err(CliError::DuplicateFlag("--cdp"));
    }
    if cdp.is_none() && fixture.is_none() {
        return Err(CliError::MissingSource);
    }
    if url.is_some() && cdp.is_none() {
        return Err(CliError::UnknownFlag("--url requires --cdp".into()));
    }
    Ok(RunArgs {
        goal,
        cdp,
        url,
        fixture,
        max_steps: max_steps.unwrap_or(20),
    })
}

fn set<T>(slot: &mut Option<T>, flag: &'static str, value: T) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::DuplicateFlag(flag));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn run_command(args: &[String]) -> Result<String, CliError> {
    let args = parse(args)?;
    if let Some(endpoint) = &args.cdp {
        let transport = WebSocketTransport::connect(endpoint)
            .map_err(|err| CliError::Browser(err.to_string()))?;
        let mut session = BrowserSession::new(transport);
        if let Some(url) = &args.url {
            session
                .navigate(url)
                .map_err(|err| CliError::Browser(err.to_string()))?;
            BrowserRuntime::settle(&mut session);
        }
        return drive(session, &args);
    }
    let path = args.fixture.as_deref().expect("checked in parse");
    let body = std::fs::read_to_string(path).map_err(|err| CliError::Io {
        path: path.to_owned(),
        message: err.to_string(),
    })?;
    if body.trim_start().starts_with('{') {
        let transport =
            ReplayTransport::parse(&body).map_err(|err| CliError::Browser(err.to_string()))?;
        return drive(BrowserSession::new(transport), &args);
    }
    // Static manifold: predict once. Executing against a page that cannot
    // change would only measure the mock.
    let manifold = parse_fixture(&body).map_err(|err| CliError::Fixture(err.to_string()))?;
    let mut agent = AgentBuilder::new(MockBrowser::new(manifold), PuaPolicy::default())
        .max_steps(1)
        .build(args.goal.clone());
    let mut out = String::from("mode dry-run (static manifold; nothing executed)\n");
    match agent.predict() {
        Ok(Some(p)) => {
            out.push_str(&format!(
                "predict {} {:?} confidence={} payload={}\n",
                p.decision.action_id,
                p.decision.target_label,
                p.decision.confidence_millis,
                p.payload
                    .as_deref()
                    .map_or("-".to_owned(), |t| format!("{t:?}"))
            ));
        }
        Ok(None) => out.push_str(&format!("predict terminal {:?}\n", agent.state())),
        Err(err) => out.push_str(&format!("predict refused: {err}\n")),
    }
    Ok(out)
}

fn drive<T: CdpTransport>(session: BrowserSession<T>, args: &RunArgs) -> Result<String, CliError> {
    let mut agent = AgentBuilder::new(session, PuaPolicy::default())
        .max_steps(args.max_steps)
        .build(args.goal.clone());
    let outcome = agent.run();
    render(&agent, &outcome)
}

fn render<B, P, T>(agent: &Agent<B, P, T>, outcome: &AgentOutcome) -> Result<String, CliError>
where
    B: BrowserRuntime,
    P: BrowserPolicy,
    T: TextResolver,
{
    let mut out = String::new();
    for step in outcome.steps() {
        out.push_str(&format!(
            "step {} {} {:?} -> {}\n",
            step.step,
            step.action_id,
            step.label,
            step.verification.as_str()
        ));
    }
    let (kind, why) = match outcome {
        AgentOutcome::Done { reason, .. } => ("done", reason.as_str()),
        AgentOutcome::Blocked { reason, .. } => ("blocked", reason.as_str()),
        AgentOutcome::Abstained { reason, .. } => ("abstained", reason.as_str()),
        AgentOutcome::Failed { error, .. } => ("failed", error.as_str()),
    };
    out.push_str(&format!(
        "outcome {kind}: {why}\npolicy_calls {} stale_discards {}\n",
        agent.policy_calls(),
        agent.stale_discards()
    ));
    if matches!(outcome, AgentOutcome::Failed { .. }) {
        return Err(CliError::Agent(out));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn requires_goal_and_source() {
        assert_eq!(
            run_command(&a(&["--cdp"])).unwrap_err(),
            CliError::MissingValue("--goal")
        );
        assert_eq!(
            run_command(&a(&["--goal", "Go"])).unwrap_err(),
            CliError::MissingSource
        );
        assert!(matches!(
            run_command(&a(&["--goal", "Go", "--fixture", "x", "--url", "http://a"])).unwrap_err(),
            CliError::UnknownFlag(_)
        ));
    }

    #[test]
    fn replay_script_runs_full_loop() {
        use hyper_use_browser::script::{Control, PageSpec, ScriptBuilder};
        let page = PageSpec::of(
            &[Control::text_field(
                10,
                100,
                "Search",
                (20.0, 20.0, 300.0, 28.0),
            )],
            "http://127.0.0.1/",
            "Search",
        );
        let script = ScriptBuilder::new()
            .observe(&page)
            .observe(&page)
            .dom_input(10)
            .observe(&page)
            .dom_read_value(10, "rust", "")
            .observe(&page);
        let dir = std::env::temp_dir().join(format!("hu-run-replay-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("search.cdp.json");
        std::fs::write(&path, script.to_json()).unwrap();
        let out = run_command(&a(&[
            "--goal",
            r#"Type "rust" into Search"#,
            "--fixture",
            path.to_str().unwrap(),
        ]))
        .unwrap();
        assert!(out.contains("TYPE_TEXT:"), "{out}");
        assert!(out.contains("-> success"), "{out}");
        assert!(out.contains("outcome done"), "{out}");
    }

    #[test]
    fn static_manifold_is_dry_run_predict() {
        let dir = std::env::temp_dir().join(format!("hu-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page.manifold");
        std::fs::write(
            &path,
            "viewport w=800 h=600\nregion id=go role=button label=\"Continue\" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility\n",
        )
        .unwrap();
        let out = run_command(&a(&[
            "--goal",
            "Continue",
            "--fixture",
            path.to_str().unwrap(),
        ]))
        .unwrap();
        assert!(out.contains("dry-run"), "{out}");
        assert!(out.contains("CLICK:go"), "{out}");
    }
}
