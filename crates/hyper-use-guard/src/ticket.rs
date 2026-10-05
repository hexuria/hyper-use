//! Issue and revalidate [`ActionTicket`]s.
//!
//! Hard-gate direction (not fully landed): treat occluded / front-layer /
//! disabled / hidden / wrong ancestry / stale ticket as *impossible* before
//! ranking, keep blocked candidates as evidence, and rank only the viable set.
//! Until that split lands, [`crate::guard`] still mixes some safety into
//! ranking (including `buried_better_label`); the ticket is still the lease
//! the executor must consume.

use std::sync::atomic::{AtomicU64, Ordering};

use hyper_use_core::{Action, InteractionManifold, InteractionRegion, RegionId};
use hyper_use_protocol::{ActionTicket, GuardCandidate, TicketInvalid};

use crate::world::WorldSnapshot;

static NEXT_TICKET_ID: AtomicU64 = AtomicU64::new(1);

/// Stable hash of the world variables compared across observations.
pub fn world_fingerprint(world: &WorldSnapshot) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    mix(
        &mut hash,
        world
            .focused()
            .map(|id| id.as_str().as_bytes())
            .unwrap_or(b"-"),
    );
    for entry in world.front_layer().entries() {
        mix(&mut hash, entry.id.as_str().as_bytes());
        mix(&mut hash, &[u8::from(entry.modal)]);
    }
    mix(&mut hash, b"|clickable|");
    for id in world.clickable() {
        mix(&mut hash, id.as_str().as_bytes());
    }
    mix(&mut hash, b"|occluded|");
    for id in world.occluded() {
        mix(&mut hash, id.as_str().as_bytes());
    }
    hash
}

fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

/// Build a ticket for an Allow decision.
pub fn issue_ticket(
    snapshot_id: u64,
    action: Action,
    target: &GuardCandidate,
    region: &InteractionRegion,
    world: &WorldSnapshot,
) -> ActionTicket {
    ActionTicket {
        ticket_id: NEXT_TICKET_ID.fetch_add(1, Ordering::Relaxed),
        snapshot_id,
        action,
        target_id: target.id.clone(),
        target_role: target.role,
        target_label: target.label.clone(),
        target_fingerprint: region.fingerprint().bits(),
        world_fingerprint: world_fingerprint(world),
    }
}

/// Revalidate a ticket against the world observed **now**.
///
/// Call this at the executor boundary immediately before clicking. On `Ok`
/// the host may press **exactly** `ticket.target_id` for `ticket.action`.
pub fn revalidate(
    ticket: &ActionTicket,
    manifold: &InteractionManifold,
    focused: Option<RegionId>,
) -> Result<(), TicketInvalid> {
    let now = WorldSnapshot::of(manifold, focused);
    if world_fingerprint(&now) != ticket.world_fingerprint {
        return Err(TicketInvalid::WorldChanged);
    }
    let Some(region) = manifold.get(&ticket.target_id) else {
        return Err(TicketInvalid::TargetGone);
    };
    if region.role() != ticket.target_role
        || region.label() != ticket.target_label
        || region.fingerprint().bits() != ticket.target_fingerprint
    {
        return Err(TicketInvalid::TargetChanged);
    }
    Ok(())
}

/// Host boundary: revalidate, then invoke `press` for the exact ticket target.
///
/// Hyper-Use MCP never calls this. Harnesses / invisible interceptors do.
pub fn consume_ticket<E>(
    ticket: &ActionTicket,
    manifold: &InteractionManifold,
    focused: Option<RegionId>,
    mut press: impl FnMut(&RegionId, Action) -> Result<(), E>,
) -> Result<(), ConsumeError<E>> {
    revalidate(ticket, manifold, focused).map_err(ConsumeError::Invalid)?;
    press(&ticket.target_id, ticket.action).map_err(ConsumeError::Press)
}

/// Failure consuming a ticket at the executor boundary.
#[derive(Debug)]
pub enum ConsumeError<E> {
    Invalid(TicketInvalid),
    Press(E),
}

impl<E: std::fmt::Display> std::fmt::Display for ConsumeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(err) => write!(f, "ticket invalid: {err}"),
            Self::Press(err) => write!(f, "press failed: {err}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ConsumeError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Invalid(err) => Some(err),
            Self::Press(err) => Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use hyper_use_core::{
        Action, InteractionManifold, InteractionRegion, LocateQuery, Rect, RegionFlags, RegionId,
        RegionParts, Role, SourceMask, UnitInterval,
    };
    use hyper_use_protocol::GuardReason;

    use super::*;
    use crate::{guard, GuardDecision, GuardRequest};

    fn button(id: &str, label: &str, y: f64) -> InteractionRegion {
        InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new(id).unwrap(),
            role: Role::Button,
            label: label.to_owned(),
            rect: Rect::try_new(10.0, y, 80.0, 24.0).unwrap(),
            actions: vec![Action::Click],
            parent: None,
            sources: SourceMask::DOM.union(SourceMask::ACCESSIBILITY),
            flags: RegionFlags::none(),
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap()
    }

    fn manifold(regions: Vec<InteractionRegion>) -> InteractionManifold {
        InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 800.0, 600.0).unwrap(),
            regions,
            0,
        )
        .unwrap()
    }

    #[test]
    fn allow_issues_a_ticket_that_revalidates_on_same_world() {
        let m = manifold(vec![button("ok", "Sign in", 100.0)]);
        let decision = guard(
            &m,
            &GuardRequest::click(LocateQuery::new().text("Sign in").unwrap()).snapshot_id(7),
        )
        .unwrap();
        let GuardDecision::Allow { ticket, target, .. } = decision else {
            panic!("expected allow, got {decision:?}");
        };
        assert_eq!(ticket.snapshot_id, 7);
        assert_eq!(ticket.target_id.as_str(), target.id.as_str());
        assert_eq!(ticket.action, Action::Click);
        revalidate(&ticket, &m, None).unwrap();
    }

    #[test]
    fn revalidate_fails_when_a_new_clickable_appears() {
        let before = manifold(vec![button("ok", "Sign in", 100.0)]);
        let decision = guard(
            &before,
            &GuardRequest::click(LocateQuery::new().text("Sign in").unwrap()),
        )
        .unwrap();
        let GuardDecision::Allow { ticket, .. } = decision else {
            panic!("expected allow");
        };
        let after = manifold(vec![
            button("ok", "Sign in", 100.0),
            button("extra", "Notify", 200.0),
        ]);
        assert_eq!(
            revalidate(&ticket, &after, None),
            Err(TicketInvalid::WorldChanged)
        );
    }

    #[test]
    fn consume_ticket_presses_only_after_revalidate() {
        let m = manifold(vec![button("ok", "Sign in", 100.0)]);
        let decision = guard(
            &m,
            &GuardRequest::click(LocateQuery::new().text("Sign in").unwrap()),
        )
        .unwrap();
        let GuardDecision::Allow { ticket, .. } = decision else {
            panic!("expected allow");
        };
        let mut pressed = None;
        consume_ticket(&ticket, &m, None, |id, action| {
            pressed = Some((id.clone(), action));
            Ok::<(), GuardReason>(())
        })
        .unwrap();
        assert_eq!(
            pressed,
            Some((RegionId::try_new("ok").unwrap(), Action::Click))
        );
    }
}
