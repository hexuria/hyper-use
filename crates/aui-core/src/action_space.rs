//! Finite action space derived from an [`InteractionManifold`].
//!
//! Phase 1 of the agent-runtime pivot: build the set of operations and targets
//! a policy may choose from. No ranking, no Instinct, no executor.
//!
//! Architectural invariants (from `browser-use/jev-ultrafast`, MIT):
//! - Every observation yields a dynamic finite action space.
//! - Policies may only choose offered operations / target ids.
//! - Target-bound actions refer to observed [`RegionId`]s, never CSS/JS.
//! - Terminal controls (`WAIT`, `DONE`, `BLOCKED`) and page scrolls have no target.
//!
//! Per-region [`crate::Action`] claims (click/type/select/…) are mapped into
//! agent-level [`ActionKind`]s. Hard-invalid regions (disabled, hidden,
//! occluded, offscreen, zero-area) are excluded from the offered target set;
//! they remain in the manifold for evidence / guard later.

use std::collections::BTreeMap;
use std::fmt;

use crate::element_state::ElementState;
use crate::error::CoreError;
use crate::id::RegionId;
use crate::manifold::InteractionManifold;
use crate::region::InteractionRegion;
use crate::vocab::{Action, Role};

/// Agent-level operation. Distinct from per-region [`Action`] capability claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ActionKind {
    Click,
    TypeText,
    Select,
    ScrollUp,
    ScrollDown,
    Wait,
    Done,
    Blocked,
}

impl ActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Click => "CLICK",
            Self::TypeText => "TYPE_TEXT",
            Self::Select => "SELECT",
            Self::ScrollUp => "SCROLL_UP",
            Self::ScrollDown => "SCROLL_DOWN",
            Self::Wait => "WAIT",
            Self::Done => "DONE",
            Self::Blocked => "BLOCKED",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "CLICK" | "click" => Self::Click,
            "TYPE_TEXT" | "type_text" | "type" => Self::TypeText,
            "SELECT" | "select" => Self::Select,
            "SCROLL_UP" | "scroll_up" => Self::ScrollUp,
            "SCROLL_DOWN" | "scroll_down" => Self::ScrollDown,
            "WAIT" | "wait" => Self::Wait,
            "DONE" | "done" => Self::Done,
            "BLOCKED" | "blocked" => Self::Blocked,
            _ => return None,
        })
    }

    /// True when this kind never binds a manifold region.
    pub const fn is_control(self) -> bool {
        matches!(
            self,
            Self::ScrollUp | Self::ScrollDown | Self::Wait | Self::Done | Self::Blocked
        )
    }
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stable id of one offered action inside an [`ActionSpace`].
///
/// Target-bound forms: `{KIND}:{region_id}` (e.g. `CLICK:nav-settings`).
/// Controls use the kind name alone (`WAIT`, `DONE`, …).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(String);

impl ActionId {
    pub fn try_new(raw: impl AsRef<str>) -> Result<Self, CoreError> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(CoreError::EmptyId);
        }
        if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(CoreError::InvalidId);
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn for_target(kind: ActionKind, region: &RegionId) -> Self {
        Self(format!("{}:{}", kind.as_str(), region.as_str()))
    }

    fn for_control(kind: ActionKind) -> Self {
        Self(kind.as_str().to_owned())
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ActionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// One offered action bound to an observed region, or a terminal/page control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedAction {
    id: ActionId,
    kind: ActionKind,
    /// Present for target-bound kinds; absent for controls.
    target: Option<RegionId>,
    label: String,
    role: Option<Role>,
    /// Region fingerprint bits at observation time (0 for controls).
    target_fingerprint: u64,
    state: ElementState,
}

impl ObservedAction {
    /// Build one offered action directly. `from_manifold` uses this for every
    /// action it emits; the replay arena uses it to rebuild a recorded menu —
    /// the caller owns which actions belong on the menu.
    pub fn new(
        id: ActionId,
        kind: ActionKind,
        target: Option<RegionId>,
        label: impl Into<String>,
        role: Option<Role>,
        target_fingerprint: u64,
        state: ElementState,
    ) -> Self {
        Self {
            id,
            kind,
            target,
            label: label.into(),
            role,
            target_fingerprint,
            state,
        }
    }

    pub fn id(&self) -> &ActionId {
        &self.id
    }
    pub fn kind(&self) -> ActionKind {
        self.kind
    }
    pub fn target(&self) -> Option<&RegionId> {
        self.target.as_ref()
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn role(&self) -> Option<Role> {
        self.role
    }
    pub fn target_fingerprint(&self) -> u64 {
        self.target_fingerprint
    }
    pub fn state(&self) -> &ElementState {
        &self.state
    }
}

/// Finite set of operations/targets a policy may choose from for one observation.
///
/// Stored in deterministic `ActionId` order. Construction never invents
/// selectors or coordinates — only manifold region ids and fixed controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSpace {
    captured_at_ms: u64,
    actions: BTreeMap<ActionId, ObservedAction>,
}

impl ActionSpace {
    /// Build an action space from the current manifold.
    ///
    /// Hard-invalid regions are skipped for target-bound actions. Page-level
    /// controls (`SCROLL_*`, `WAIT`, `DONE`, `BLOCKED`) are always offered.
    pub fn from_manifold(manifold: &InteractionManifold) -> Self {
        let mut actions = BTreeMap::new();

        for region in manifold.regions() {
            if !region_is_viable_target(region) {
                continue;
            }
            for claim in region.actions() {
                let Some(kind) = map_region_claim(*claim) else {
                    continue;
                };
                // Readonly controls stay clickable (focus/select text) but are
                // not offered as TYPE_TEXT / SELECT targets.
                if region.flags().readonly()
                    && matches!(kind, ActionKind::TypeText | ActionKind::Select)
                {
                    continue;
                }
                let observed = target_action(kind, region);
                actions.insert(observed.id.clone(), observed);
            }
        }

        for kind in [
            ActionKind::ScrollUp,
            ActionKind::ScrollDown,
            ActionKind::Wait,
            ActionKind::Done,
            ActionKind::Blocked,
        ] {
            let observed = control_action(kind);
            actions.insert(observed.id.clone(), observed);
        }

        Self {
            captured_at_ms: manifold.captured_at_ms(),
            actions,
        }
    }

    /// Build a space from already-offered actions — e.g. the replay arena
    /// rebuilding a recorded menu. Viability filtering happened upstream;
    /// this only orders deterministically. `captured_at_ms` is 0: a recorded
    /// menu has no observation timestamp.
    pub fn from_actions(actions: Vec<ObservedAction>) -> Self {
        Self {
            captured_at_ms: 0,
            actions: actions
                .into_iter()
                .map(|action| (action.id.clone(), action))
                .collect(),
        }
    }

    pub fn captured_at_ms(&self) -> u64 {
        self.captured_at_ms
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn get(&self, id: &ActionId) -> Option<&ObservedAction> {
        self.actions.get(id)
    }

    pub fn get_str(&self, id: &str) -> Option<&ObservedAction> {
        let id = ActionId::try_new(id).ok()?;
        self.actions.get(&id)
    }

    pub fn actions(&self) -> impl Iterator<Item = &ObservedAction> {
        self.actions.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = &ActionId> {
        self.actions.keys()
    }

    /// Target-bound actions of one kind, in id order.
    pub fn targets_of(&self, kind: ActionKind) -> impl Iterator<Item = &ObservedAction> {
        self.actions
            .values()
            .filter(move |a| a.kind == kind && a.target.is_some())
    }

    pub fn contains_kind(&self, kind: ActionKind) -> bool {
        self.actions.values().any(|a| a.kind == kind)
    }

    /// This space minus every action of `kind` (same capture time).
    pub fn without_kind(&self, kind: ActionKind) -> Self {
        Self {
            captured_at_ms: self.captured_at_ms,
            actions: self
                .actions
                .iter()
                .filter(|(_, a)| a.kind != kind)
                .map(|(id, a)| (id.clone(), a.clone()))
                .collect(),
        }
    }
}

fn region_is_viable_target(region: &InteractionRegion) -> bool {
    let flags = region.flags();
    if flags.disabled() || flags.hidden() || flags.occluded() || flags.offscreen() {
        return false;
    }
    if region.rect().is_zero_area() {
        return false;
    }
    true
}

fn map_region_claim(claim: Action) -> Option<ActionKind> {
    match claim {
        Action::Click | Action::Toggle => Some(ActionKind::Click),
        Action::Type => Some(ActionKind::TypeText),
        Action::Select => Some(ActionKind::Select),
        // Focus/Hover/Scroll on a region are not agent operations; page scroll
        // is offered as SCROLL_UP / SCROLL_DOWN controls.
        Action::Focus | Action::Hover | Action::Scroll => None,
    }
}

fn target_action(kind: ActionKind, region: &InteractionRegion) -> ObservedAction {
    ObservedAction {
        id: ActionId::for_target(kind, region.id()),
        kind,
        target: Some(region.id().clone()),
        label: region.label().to_owned(),
        role: Some(region.role()),
        target_fingerprint: region.fingerprint().bits(),
        state: region.state().clone(),
    }
}

fn control_action(kind: ActionKind) -> ObservedAction {
    let label = match kind {
        ActionKind::ScrollUp => "Scroll the page up".to_owned(),
        ActionKind::ScrollDown => "Scroll the page down".to_owned(),
        ActionKind::Wait => "Wait briefly for the page to settle".to_owned(),
        ActionKind::Done => "Every requirement is visibly satisfied".to_owned(),
        ActionKind::Blocked => "No supported operation can progress".to_owned(),
        _ => kind.as_str().to_owned(),
    };
    ObservedAction {
        id: ActionId::for_control(kind),
        kind,
        target: None,
        label,
        role: None,
        target_fingerprint: 0,
        state: ElementState::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_fixture;
    use crate::{
        InteractionManifold, Rect, RegionFlags, RegionParts, Role, SourceMask, UnitInterval,
    };

    #[test]
    fn action_kind_round_trips() {
        for kind in [
            ActionKind::Click,
            ActionKind::TypeText,
            ActionKind::Select,
            ActionKind::ScrollUp,
            ActionKind::ScrollDown,
            ActionKind::Wait,
            ActionKind::Done,
            ActionKind::Blocked,
        ] {
            assert_eq!(ActionKind::parse(kind.as_str()), Some(kind));
        }
        assert!(!ActionKind::Click.is_control());
        assert!(ActionKind::Wait.is_control());
    }

    #[test]
    fn sidebar_offers_stable_click_ids_and_excludes_invalid() {
        let m = parse_fixture(include_str!("../../../fixtures/sidebar.manifold")).unwrap();
        let space = ActionSpace::from_manifold(&m);

        // Stable click ids for viable nav buttons.
        for id in [
            "CLICK:nav-help",
            "CLICK:nav-settings",
            "CLICK:nav-profile",
            "CLICK:main-settings",
        ] {
            let action = space.get_str(id).unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(action.kind(), ActionKind::Click);
            assert!(action.target().is_some());
        }

        // Hard-invalid / non-viable must not appear.
        for id in [
            "CLICK:nav-disabled",
            "CLICK:nav-hidden",
            "CLICK:offscreen-undo",
            "CLICK:zero-ghost",
        ] {
            assert!(space.get_str(id).is_none(), "should exclude {id}");
        }

        // Controls always present.
        for id in ["SCROLL_UP", "SCROLL_DOWN", "WAIT", "DONE", "BLOCKED"] {
            assert!(space.get_str(id).is_some(), "missing control {id}");
            assert!(space.get_str(id).unwrap().target().is_none());
        }

        // Deterministic id order.
        let ids: Vec<_> = space.ids().map(ActionId::as_str).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn twin_suspend_rows_expose_both_click_targets() {
        let m =
            parse_fixture(include_str!("../../../fixtures/twin-suspend-rows.manifold")).unwrap();
        let space = ActionSpace::from_manifold(&m);

        let alpha = space.get_str("CLICK:alpha-suspend").unwrap();
        let beta = space.get_str("CLICK:beta-suspend").unwrap();
        assert_eq!(alpha.label(), "Suspend");
        assert_eq!(beta.label(), "Suspend");
        assert_ne!(alpha.target(), beta.target());
        assert_ne!(alpha.target_fingerprint(), beta.target_fingerprint());

        // Editable fields become TYPE_TEXT (and Click if claimed).
        assert!(space.get_str("TYPE_TEXT:alpha-host").is_some());
        assert!(space.get_str("TYPE_TEXT:beta-host").is_some());
        assert!(space.get_str("CLICK:alpha-host").is_some());
    }

    #[test]
    fn modal_confirm_still_lists_buried_page_buttons() {
        // Phase 1: ActionSpace lists what the manifold claims. Front-layer /
        // occlusion refusal is a guard/hard-gate concern (Phase 3), not
        // ActionSpace filtering — except explicit occluded flags.
        let m = parse_fixture(include_str!("../../../fixtures/modal-confirm.manifold")).unwrap();
        let space = ActionSpace::from_manifold(&m);

        assert!(space.get_str("CLICK:page-delete").is_some());
        assert!(space.get_str("CLICK:confirm-delete").is_some());
        assert!(space.get_str("CLICK:confirm-cancel").is_some());
        assert!(space.get_str("CLICK:page-cancel").is_some());
    }

    #[test]
    fn send_buttons_all_three_click_targets_stable() {
        let m = parse_fixture(include_str!("../../../fixtures/send-buttons.manifold")).unwrap();
        let space = ActionSpace::from_manifold(&m);
        for id in ["CLICK:a-feedback", "CLICK:b-device", "CLICK:z-send"] {
            let a = space.get_str(id).unwrap();
            assert_eq!(a.kind(), ActionKind::Click);
        }
        let clicks: Vec<_> = space
            .targets_of(ActionKind::Click)
            .map(|a| a.id().as_str())
            .collect();
        assert_eq!(
            clicks,
            ["CLICK:a-feedback", "CLICK:b-device", "CLICK:z-send"]
        );
    }

    #[test]
    fn empty_manifold_still_offers_controls() {
        let m = parse_fixture("viewport w=100 h=100\n").unwrap();
        let space = ActionSpace::from_manifold(&m);
        assert_eq!(space.len(), 5);
        assert!(space.contains_kind(ActionKind::Done));
        assert!(!space.contains_kind(ActionKind::Click));
    }

    #[test]
    fn target_actions_copy_state_and_controls_have_none() {
        let state = ElementState {
            value: Some("Ana".to_owned()),
            selected: Some("UTC".to_owned()),
            options: vec!["UTC".to_owned()],
            ..ElementState::default()
        };
        let region = InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new("field").unwrap(),
            role: Role::ComboBox,
            label: "Name".to_owned(),
            rect: Rect::try_new(10.0, 10.0, 80.0, 24.0).unwrap(),
            actions: vec![Action::Type, Action::Select],
            parent: None,
            sources: SourceMask::DOM,
            flags: RegionFlags::none(),
            temporal_stability: UnitInterval::ONE,
        })
        .unwrap()
        .with_state(state.clone());
        let manifold = InteractionManifold::try_new(
            Rect::try_viewport(0.0, 0.0, 100.0, 100.0).unwrap(),
            vec![region],
            0,
        )
        .unwrap();
        let space = ActionSpace::from_manifold(&manifold);

        assert_eq!(space.get_str("TYPE_TEXT:field").unwrap().state(), &state);
        assert_eq!(space.get_str("SELECT:field").unwrap().state(), &state);
        assert!(space
            .actions()
            .filter(|action| action.kind().is_control())
            .all(|action| action.state().is_empty()));
    }
}
