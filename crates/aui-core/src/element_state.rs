/// Observed control state. Evidence only: it is NOT part of the region
/// fingerprint, so a value change never makes a ticket stale.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElementState {
    /// INPUT (except type password / hidden / file / checkbox / radio) or TEXTAREA value, ≤ 200 chars.
    pub value: Option<String>,
    /// checkbox / radio `.checked`; otherwise `aria-checked` "true"/"false" ("mixed" → None).
    pub checked: Option<bool>,
    /// `aria-expanded` "true"/"false".
    pub expanded: Option<bool>,
    /// SELECT: labels of the selected options, trimmed, joined with ", ".
    pub selected: Option<String>,
    /// SELECT: enabled option labels in document order (skip disabled options and options in a disabled optgroup), trimmed, each ≤ 80 chars, at most 50.
    pub options: Vec<String>,
}

impl ElementState {
    pub fn is_empty(&self) -> bool {
        self.value.is_none()
            && self.checked.is_none()
            && self.expanded.is_none()
            && self.selected.is_none()
            && self.options.is_empty()
    }
}
