//! Property tests for the executor boundary and abstention.

use hyper_use_agent::{execute_ticketed, AgentBuilder, AgentOutcome, Input, MockBrowser};
use hyper_use_core::{parse_fixture, Action, InteractionManifold, RegionId};
use hyper_use_guard::{gate, TicketLedger};
use hyper_use_policy::PuaPolicy;
use proptest::prelude::*;

fn page(
    n: usize,
    relabel: Option<(usize, &str)>,
    drop: Option<usize>,
    extra: bool,
    modal: bool,
) -> InteractionManifold {
    let mut src = String::from("viewport w=800 h=600\n");
    for i in 0..n {
        if drop == Some(i) {
            continue;
        }
        let label = match relabel {
            Some((j, l)) if j == i => l.to_owned(),
            _ => format!("Button {i}"),
        };
        src.push_str(&format!(
            "region id=b{i} role=button label=\"{label}\" x=10 y={} w=80 h=20 actions=click sources=dom,accessibility\n",
            10 + i * 30
        ));
    }
    if extra {
        src.push_str("region id=toast role=button label=\"Undo\" x=600 y=500 w=80 h=20 actions=click sources=dom,accessibility\n");
    }
    if modal {
        src.push_str("region id=dlg role=dialog label=\"Hold on\" x=300 y=200 w=200 h=150 actions=focus sources=dom,accessibility flags=modal\n");
    }
    parse_fixture(&src).unwrap()
}

proptest! {
    /// The executor dispatches only the ticket's target, and only when the
    /// target and world are unchanged since the ticket was issued.
    #[test]
    fn stale_never_executes_and_target_cannot_be_substituted(
        n in 2usize..6,
        pick in 0usize..6,
        mutation in 0u8..6,
        other in 0usize..6,
    ) {
        let pick = pick % n;
        let other = other % n;
        let before = page(n, None, None, false, false);
        let target = RegionId::try_new(format!("b{pick}")).unwrap();
        let ticket = gate(&before, &target, Action::Click, None, 0).unwrap();
        let after = match mutation {
            0 => before.clone(),
            1 => page(n, Some((pick, "Delete everything")), None, false, false),
            2 => page(n, None, Some(pick), false, false),
            3 => page(n, None, None, true, false),
            4 => page(n, None, None, false, true),
            _ => page(n, Some((other, "Renamed")), None, false, false),
        };
        let expect_exec = match mutation {
            0 => true,
            5 => other != pick,
            _ => false,
        };
        let mut browser = MockBrowser::new(after);
        let mut ledger = TicketLedger::new();
        let res = execute_ticketed(&mut browser, &mut ledger, &ticket, &Input::Click);
        prop_assert_eq!(res.is_ok(), expect_exec, "{:?}", res);
        if expect_exec {
            prop_assert_eq!(browser.press_log(), &[(target.clone(), Action::Click)][..]);
            // One-shot.
            prop_assert!(execute_ticketed(&mut browser, &mut ledger, &ticket, &Input::Click).is_err());
            prop_assert_eq!(browser.press_log().len(), 1);
        } else {
            prop_assert!(browser.press_log().is_empty());
        }
    }

    /// PUA abstention never becomes an input, whatever the twin count/order.
    #[test]
    fn abstain_never_executes(twins in 2usize..6, shift in 0usize..6) {
        let mut src = String::from("viewport w=800 h=600\n");
        for i in 0..twins {
            let k = (i + shift) % twins;
            src.push_str(&format!(
                "region id=t{k} role=button label=\"Archive\" x=10 y={} w=80 h=20 actions=click sources=dom,accessibility\n",
                10 + k * 30
            ));
        }
        let mut agent = AgentBuilder::new(MockBrowser::new(parse_fixture(&src).unwrap()), PuaPolicy::default())
            .max_steps(3)
            .build("Archive");
        let outcome = agent.run();
        prop_assert!(matches!(outcome, AgentOutcome::Abstained { .. }), "{:?}", outcome);
        prop_assert!(agent.browser_mut().press_log().is_empty());
    }
}
