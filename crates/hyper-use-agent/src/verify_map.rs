use hyper_use_browser::PageDelta;
use hyper_use_core::InteractionManifold;
use hyper_use_observe::diff;

use crate::outcome::VerificationKind;

/// Classify post-action observation into a verification kind.
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
