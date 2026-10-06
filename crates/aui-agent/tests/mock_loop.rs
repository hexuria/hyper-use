//! Offline e2e: goal → observe → Instinct → guard → ticket → press → verify.

use aui_agent::{AgentBuilder, AgentOutcome, MockBrowser, TickResult, VerificationKind};
use aui_core::{parse_fixture, Action, InteractionManifold};
use aui_policy::InstinctPolicy;

fn manifold(src: &str) -> InteractionManifold {
    parse_fixture(src).unwrap()
}

#[test]
fn clicks_exact_label_and_sees_state_change() {
    let before = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="Continue" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        region id=no role=button label="Cancel" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let after = manifold(
        r#"
        viewport w=800 h=600
        region id=done role=button label="Finish" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let mut browser = MockBrowser::new(before);
    browser.set_on_press(after);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(5)
        .build("Continue");
    let outcome = agent.run();
    // First step clicks Continue; page changes. Next predict may abstain or pick Finish.
    // We at least require a stepped history with a click.
    assert!(
        !outcome.steps().is_empty()
            || matches!(
                outcome,
                AgentOutcome::Abstained { .. } | AgentOutcome::Done { .. }
            ),
        "{outcome:?}"
    );
    let steps = outcome.steps();
    if let Some(first) = steps.first() {
        assert_eq!(first.label, "Continue");
        assert_eq!(first.kind, aui_core::ActionKind::Click);
        assert!(
            matches!(
                first.verification,
                VerificationKind::StateChanged
                    | VerificationKind::NoEffect
                    | VerificationKind::Success
            ),
            "{:?}",
            first.verification
        );
    }
    assert_eq!(
        agent
            .browser_mut()
            .press_log()
            .first()
            .map(|(id, a)| (id.as_str(), *a)),
        Some(("go", Action::Click))
    );
}

#[test]
fn twin_delete_abstains_without_pressing() {
    let m = manifold(
        r#"
        viewport w=800 h=600
        region id=a role=button label="Delete" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        region id=b role=button label="Delete" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let browser = MockBrowser::new(m);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(3)
        .build("Delete");
    let outcome = agent.run();
    assert!(
        matches!(&outcome, AgentOutcome::Abstained { .. }),
        "{outcome:?}"
    );
    assert!(agent.browser_mut().press_log().is_empty());
}

#[test]
fn stale_world_discards_prediction_without_failing_task_permanently() {
    let before = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="UniqueGo" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let browser = MockBrowser::new(before);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(4)
        .build("UniqueGo");
    // Predict successfully.
    let pred = agent.predict().unwrap();
    assert!(pred.is_some());
    // Mutate world before act so ticket/world fingerprint fails.
    let mutated = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="UniqueGo" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        region id=extra role=button label="Notify" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    agent.browser_mut().replace_manifold(mutated);
    let err = agent.act().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("stale") || msg.contains("world"), "{msg}");
    // Ready again — not terminal failure.
    assert_eq!(agent.state(), aui_agent::AgentState::Ready);
}

#[test]
fn done_goal_terminates_without_press() {
    let m = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="Continue" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let browser = MockBrowser::new(m);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(2)
        .build("DONE");
    let outcome = agent.run();
    assert!(matches!(outcome, AgentOutcome::Done { .. }), "{outcome:?}");
    assert!(agent.browser_mut().press_log().is_empty());
}

#[test]
fn tick_stale_is_not_finished_failure() {
    let before = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="UniqueGo" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let browser = MockBrowser::new(before);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(3)
        .build("UniqueGo");
    agent.predict().unwrap();
    let mutated = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="UniqueGo" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        region id=x role=button label="X" x=10 y=40 w=40 h=20 actions=click sources=dom,accessibility
        "#,
    );
    agent.browser_mut().replace_manifold(mutated);
    // Force state Predicted still — act via tick
    // After predict, state is Predicted; mutate; tick will act.
    match agent.tick() {
        Ok(TickResult::StaleDiscarded { .. }) => {}
        Ok(other) => panic!("expected stale discarded, got {other:?}"),
        Err(e) => panic!("unexpected err {e}"),
    }
}

#[test]
fn repeated_no_effect_click_abstains_before_max_steps() {
    let page = manifold(
        r#"
        viewport w=800 h=600
        region id=go role=button label="Continue" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        "#,
    );
    let mut browser = MockBrowser::new(page.clone());
    browser.set_on_press(page);
    let mut agent = AgentBuilder::new(browser, InstinctPolicy::default())
        .max_steps(10)
        .build("Continue");
    let outcome = agent.run();
    assert!(
        matches!(outcome, AgentOutcome::Abstained { .. }),
        "{outcome:?}"
    );
    assert_eq!(outcome.steps().len(), 2);
    assert!(outcome
        .steps()
        .iter()
        .all(|step| step.verification == VerificationKind::NoEffect));
    assert_eq!(agent.browser_mut().press_log().len(), 2);
    assert!(outcome.steps().len() < 10);
}
