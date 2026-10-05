//! Browser runtime surface the agent drives. Live CDP or an in-memory mock.

use hyper_use_browser::{
    page_delta, BrowserError, BrowserSession, CdpTransport, PageDelta, PageState,
};
use hyper_use_core::{Action, InteractionManifold, RegionId};

use crate::error::AgentError;

pub trait BrowserRuntime {
    fn observe(&mut self) -> Result<&InteractionManifold, AgentError>;
    fn focused(&self) -> Option<RegionId>;
    fn page(&self) -> Option<&PageState>;
    fn press(&mut self, id: &RegionId, action: Action) -> Result<(), AgentError>;
    fn page_delta_between(&self, before: &PageState, after: &PageState) -> PageDelta {
        page_delta(before, after)
    }
}

impl<T: CdpTransport> BrowserRuntime for BrowserSession<T> {
    fn observe(&mut self) -> Result<&InteractionManifold, AgentError> {
        BrowserSession::observe(self).map_err(|e: BrowserError| AgentError::Browser(e.to_string()))
    }

    fn focused(&self) -> Option<RegionId> {
        BrowserSession::page(self).and_then(|p| p.focused().cloned())
    }

    fn page(&self) -> Option<&PageState> {
        BrowserSession::page(self)
    }

    fn press(&mut self, id: &RegionId, action: Action) -> Result<(), AgentError> {
        BrowserSession::press(self, id, action)
            .map(|_| ())
            .map_err(|e| AgentError::Browser(e.to_string()))
    }
}

/// In-memory browser for offline agent tests. Mutators simulate UI changes.
#[derive(Clone, Debug)]
pub struct MockBrowser {
    manifold: InteractionManifold,
    focused: Option<RegionId>,
    page: PageState,
    /// When set, the next `press` replaces the manifold with this snapshot.
    on_press: Option<InteractionManifold>,
    press_log: Vec<(RegionId, Action)>,
}

impl MockBrowser {
    pub fn new(manifold: InteractionManifold) -> Self {
        Self {
            manifold,
            focused: None,
            page: PageState::blank(),
            on_press: None,
            press_log: Vec::new(),
        }
    }

    pub fn with_page(mut self, page: PageState) -> Self {
        self.page = page;
        self
    }

    pub fn set_on_press(&mut self, next: InteractionManifold) {
        self.on_press = Some(next);
    }

    pub fn press_log(&self) -> &[(RegionId, Action)] {
        &self.press_log
    }

    pub fn replace_manifold(&mut self, manifold: InteractionManifold) {
        self.manifold = manifold;
    }
}

impl BrowserRuntime for MockBrowser {
    fn observe(&mut self) -> Result<&InteractionManifold, AgentError> {
        Ok(&self.manifold)
    }

    fn focused(&self) -> Option<RegionId> {
        self.focused.clone()
    }

    fn page(&self) -> Option<&PageState> {
        Some(&self.page)
    }

    fn press(&mut self, id: &RegionId, action: Action) -> Result<(), AgentError> {
        self.press_log.push((id.clone(), action));
        if let Some(next) = self.on_press.take() {
            self.manifold = next;
        }
        Ok(())
    }
}
