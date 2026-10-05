//! Page-level facts from one observation.
//!
//! `url` and `title` come from `Page.getNavigationHistory`. `focused` is the
//! stable id of the accessibility node whose `focused` property is true.
//! The history payload has no timestamp, so this does not set `captured_at_ms`.

use hyper_use_core::RegionId;

/// URL, title, and focused region after one observation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageState {
    url: String,
    title: String,
    focused: Option<RegionId>,
}

impl PageState {
    pub fn new(
        url: impl Into<String>,
        title: impl Into<String>,
        focused: Option<RegionId>,
    ) -> Self {
        Self {
            url: url.into(),
            title: title.into(),
            focused,
        }
    }

    /// No URL, title, or focus. Used when the source is not a CDP page.
    pub fn blank() -> Self {
        Self {
            url: String::new(),
            title: String::new(),
            focused: None,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn focused(&self) -> Option<&RegionId> {
        self.focused.as_ref()
    }
}

/// What changed between two [`PageState`] values. Not a region diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageDelta {
    url_changed: bool,
    title_changed: bool,
    focus_changed: bool,
}

impl PageDelta {
    pub const fn url_changed(self) -> bool {
        self.url_changed
    }

    pub const fn title_changed(self) -> bool {
        self.title_changed
    }

    pub const fn focus_changed(self) -> bool {
        self.focus_changed
    }

    pub const fn is_unchanged(self) -> bool {
        !self.url_changed && !self.title_changed && !self.focus_changed
    }
}

pub fn page_delta(before: &PageState, after: &PageState) -> PageDelta {
    PageDelta {
        url_changed: before.url != after.url,
        title_changed: before.title != after.title,
        focus_changed: before.focused != after.focused,
    }
}
