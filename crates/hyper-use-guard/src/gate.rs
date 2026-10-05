//! Hard browser-integrity gate for a target a policy **already chose**.
//!
//! This is the agent path's guard. It does not rank and has no confidence
//! threshold: the policy (PUA) decided *which* finite choice wins; the gate
//! answers only "may this exact region receive this exact action right now?"
//! Every check is a hard invalidity, never a score penalty:
//!
//! | check | reason |
//! |---|---|
//! | region not in the current observation | `missing-target` |
//! | region does not claim the action | `unsupported-action` |
//! | disabled | `disabled` |
//! | hidden / zero area | `hidden` |
//! | hit-test / stacking occluded | `occluded` |
//! | behind an open dialog | `front-layer` |
//! | outside the viewport | `offscreen` |
//!
//! Readonly is not observable in the manifold today; the browser input
//! function refuses readonly / non-editable nodes at execution time (fail
//! closed, nothing typed).
//!
//! On success the gate issues an [`ActionTicket`] bound to the action, the
//! target fingerprint, and the world fingerprint of this observation. The
//! executor must revalidate and consume it once ([`crate::revalidate`],
//! [`crate::TicketLedger`]).
//!
//! The ranked [`crate::guard`] entry point remains for the MCP / CLI preflight
//! surface, where a host proposes a label rather than a finite choice.

use hyper_use_core::{Action, InteractionManifold, InteractionRegion, RegionId};
use hyper_use_protocol::{ActionTicket, GuardCandidate, GuardReason};

use crate::ticket::issue_ticket;
use crate::world::{blocker, WorldSnapshot};

/// Gate `target` for `action` on `manifold` and issue a one-shot ticket.
pub fn gate(
    manifold: &InteractionManifold,
    target: &RegionId,
    action: Action,
    focused: Option<RegionId>,
    snapshot_id: u64,
) -> Result<ActionTicket, GuardReason> {
    let region = manifold.get(target).ok_or(GuardReason::MissingTarget)?;
    check(manifold, region, action)?;
    let candidate = GuardCandidate {
        id: region.id().clone(),
        role: region.role(),
        label: region.label().to_owned(),
        confidence: 1.0,
    };
    let world = WorldSnapshot::of(manifold, focused);
    Ok(issue_ticket(
        snapshot_id,
        action,
        &candidate,
        region,
        &world,
    ))
}

/// The hard checks alone (no ticket). Exposed for tests and evidence.
pub fn check(
    manifold: &InteractionManifold,
    region: &InteractionRegion,
    action: Action,
) -> Result<(), GuardReason> {
    if !supports(region, action) {
        return Err(GuardReason::UnsupportedAction);
    }
    let flags = region.flags();
    if flags.disabled() {
        return Err(GuardReason::Disabled);
    }
    if flags.hidden() || region.rect().is_zero_area() {
        return Err(GuardReason::Hidden);
    }
    if flags.occluded() {
        return Err(GuardReason::Occluded);
    }
    if blocker(manifold, region).is_some() {
        return Err(GuardReason::FrontLayer);
    }
    if flags.offscreen() {
        return Err(GuardReason::Offscreen);
    }
    Ok(())
}

fn supports(region: &InteractionRegion, action: Action) -> bool {
    let claims = region.actions();
    match action {
        Action::Click => claims.contains(&Action::Click) || claims.contains(&Action::Toggle),
        other => claims.contains(&other),
    }
}

#[cfg(test)]
mod tests {
    use hyper_use_core::parse_fixture;

    use super::*;
    use crate::{revalidate, TicketLedger};

    fn id(raw: &str) -> RegionId {
        RegionId::try_new(raw).unwrap()
    }

    const PAGE: &str = r#"
        viewport w=800 h=600
        region id=go role=button label="Go" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility
        region id=off role=button label="Off" x=10 y=10 w=80 h=24 actions=click sources=dom,accessibility flags=disabled
        region id=cov role=button label="Covered" x=10 y=50 w=80 h=24 actions=click sources=dom,accessibility flags=occluded
        region id=gone role=button label="Gone" x=10 y=90 w=80 h=24 actions=click sources=dom,accessibility flags=hidden
        region id=far role=button label="Far" x=10 y=900 w=80 h=24 actions=click sources=dom,accessibility flags=offscreen
        region id=name role=text_field label="Name" x=10 y=130 w=200 h=24 actions=click,type sources=dom,accessibility
    "#;

    #[test]
    fn viable_click_and_type_issue_bound_tickets() {
        let m = parse_fixture(PAGE).unwrap();
        let t = gate(&m, &id("go"), Action::Click, None, 7).unwrap();
        assert_eq!(t.action, Action::Click);
        assert_eq!(t.target_id, id("go"));
        assert_eq!(t.snapshot_id, 7);
        revalidate(&t, &m, None).unwrap();
        let t = gate(&m, &id("name"), Action::Type, None, 7).unwrap();
        assert_eq!(t.action, Action::Type);
    }

    #[test]
    fn hard_invalid_targets_never_get_tickets() {
        let m = parse_fixture(PAGE).unwrap();
        assert_eq!(
            gate(&m, &id("off"), Action::Click, None, 0),
            Err(GuardReason::Disabled)
        );
        assert_eq!(
            gate(&m, &id("cov"), Action::Click, None, 0),
            Err(GuardReason::Occluded)
        );
        assert_eq!(
            gate(&m, &id("gone"), Action::Click, None, 0),
            Err(GuardReason::Hidden)
        );
        assert_eq!(
            gate(&m, &id("far"), Action::Click, None, 0),
            Err(GuardReason::Offscreen)
        );
        assert_eq!(
            gate(&m, &id("nope"), Action::Click, None, 0),
            Err(GuardReason::MissingTarget)
        );
        assert_eq!(
            gate(&m, &id("go"), Action::Type, None, 0),
            Err(GuardReason::UnsupportedAction)
        );
        assert_eq!(
            gate(&m, &id("name"), Action::Select, None, 0),
            Err(GuardReason::UnsupportedAction)
        );
    }

    #[test]
    fn background_control_behind_modal_is_front_layer() {
        let m = parse_fixture(
            r#"
            viewport w=800 h=600
            region id=bg role=button label="Delete project" x=10 y=10 w=120 h=24 actions=click sources=dom,accessibility
            region id=dlg role=dialog label="Confirm" x=200 y=100 w=300 h=200 actions=focus sources=dom,accessibility flags=modal
            region id=ok role=button label="Delete" x=220 y=250 w=80 h=24 actions=click parent=dlg sources=dom,accessibility
            "#,
        )
        .unwrap();
        assert_eq!(
            gate(&m, &id("bg"), Action::Click, None, 0),
            Err(GuardReason::FrontLayer)
        );
        assert!(gate(&m, &id("ok"), Action::Click, None, 0).is_ok());
    }

    #[test]
    fn gate_ticket_is_one_shot_in_ledger() {
        let m = parse_fixture(PAGE).unwrap();
        let t = gate(&m, &id("go"), Action::Click, None, 0).unwrap();
        let mut ledger = TicketLedger::new();
        ledger.mark_consumed(t.ticket_id).unwrap();
        assert!(ledger.mark_consumed(t.ticket_id).is_err());
    }
}
