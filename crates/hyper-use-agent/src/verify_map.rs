use hyper_use_browser::PageDelta;
use hyper_use_core::InteractionManifold;
use hyper_use_observe::diff;

use crate::outcome::VerificationKind;
use crate::runtime::FieldValue;

/// Classify post-action observation into a verification kind.
///
/// URL change → navigation; manifold delta → state-changed; title / focus
/// only → state-changed; nothing → no-effect.
pub fn classify_delta(
    before_manifold: &InteractionManifold,
    after_manifold: &InteractionManifold,
    page: Option<&PageDelta>,
) -> VerificationKind {
    if let Some(p) = page {
        if p.url_changed() {
            return VerificationKind::Navigation;
        }
    }
    let delta = diff(before_manifold, after_manifold);
    if delta.is_empty() {
        if page.is_some_and(|p| p.title_changed() || p.focus_changed()) {
            return VerificationKind::StateChanged;
        }
        return VerificationKind::NoEffect;
    }
    VerificationKind::StateChanged
}

/// TYPE_TEXT / SELECT postcondition from the field's value read back after
/// input. `None` when the runtime could not read it (fall back to the diff).
pub fn classify_value(expected: &str, value: Option<&FieldValue>) -> Option<VerificationKind> {
    let value = value?;
    Some(if value.matches(expected) {
        VerificationKind::Success
    } else {
        VerificationKind::WrongEffect
    })
}
