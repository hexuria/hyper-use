//! Build a [`crate::ReplayTransport`] script in code.
//!
//! This is mock support for tests: it writes the CDP calls that
//! [`crate::BrowserSession::observe`] and [`crate::BrowserSession::press`]
//! make, in the order they make them. Steps carry no `params`, so the replay
//! does not check them. It does not talk to Chrome. It is not a model of
//! Chrome; it only emits the response shapes the extractors read.
//!
//! DOM nodes use tags that observe always keeps (`BUTTON`, `A`, `INPUT`,
//! `NAV`, `H1`), so the getBoxModel order is the document order.

use std::collections::BTreeMap;

use serde_json::{json, Value};

/// One DOM element. `rect` of `None` is a getBoxModel protocol error, which
/// observe treats as "no box" and omits.
#[derive(Clone, Debug)]
pub struct DomSpec {
    pub node_id: i64,
    pub backend: i64,
    pub tag: &'static str,
    pub label: String,
    pub rect: Option<(f64, f64, f64, f64)>,
    pub children: Vec<DomSpec>,
    /// Extra attributes, in order (for example `role`, `aria-label`,
    /// `aria-modal`). Empty for the plain kept tags.
    pub attributes: Vec<(String, String)>,
}

impl DomSpec {
    pub fn button(node_id: i64, backend: i64, label: &str, rect: (f64, f64, f64, f64)) -> Self {
        Self {
            node_id,
            backend,
            tag: "BUTTON",
            label: label.to_owned(),
            rect: Some(rect),
            children: Vec::new(),
            attributes: Vec::new(),
        }
    }

    /// A container `DIV` with an explicit `role` and `aria-label`, so observe
    /// keeps it as a region and its children get it as their parent.
    pub fn container(
        node_id: i64,
        backend: i64,
        role: &str,
        label: &str,
        rect: (f64, f64, f64, f64),
    ) -> Self {
        Self {
            node_id,
            backend,
            tag: "DIV",
            label: String::new(),
            rect: Some(rect),
            children: Vec::new(),
            attributes: vec![
                ("role".to_owned(), role.to_owned()),
                ("aria-label".to_owned(), label.to_owned()),
            ],
        }
    }

    /// The same element with another kept tag (`A`, `INPUT`, `NAV`, `H1`).
    pub fn with_tag(mut self, tag: &'static str) -> Self {
        self.tag = tag;
        self
    }

    /// Add one attribute.
    pub fn with_attr(mut self, name: &str, value: &str) -> Self {
        self.attributes.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Nest `children` under this element.
    pub fn with_children(mut self, children: Vec<DomSpec>) -> Self {
        self.children = children;
        self
    }
}

/// One accessibility node. `rect` is read with getBoxModel by backend id.
#[derive(Clone, Debug)]
pub struct AxSpec {
    pub backend: Option<i64>,
    pub role: &'static str,
    pub name: String,
    pub rect: Option<(f64, f64, f64, f64)>,
    pub focused: bool,
    /// The accessibility `modal` property.
    pub modal: bool,
}

impl AxSpec {
    pub fn new(backend: i64, role: &'static str, name: &str, rect: (f64, f64, f64, f64)) -> Self {
        Self {
            backend: Some(backend),
            role,
            name: name.to_owned(),
            rect: Some(rect),
            focused: false,
            modal: false,
        }
    }

    /// Mark this node as the focused one.
    pub fn focused(mut self) -> Self {
        self.focused = true;
        self
    }

    /// Mark this node as modal (a dialog opened with `showModal()`).
    pub fn modal(mut self) -> Self {
        self.modal = true;
        self
    }
}

/// One control seen by both DOM and accessibility, as Chrome reports a real
/// button or link: one DOM element and one AX node with the same backend id
/// and the same box.
#[derive(Clone, Debug)]
pub struct Control {
    pub node_id: i64,
    pub backend: i64,
    pub tag: &'static str,
    pub role: &'static str,
    pub label: String,
    pub rect: (f64, f64, f64, f64),
    pub focused: bool,
}

impl Control {
    pub fn button(node_id: i64, backend: i64, label: &str, rect: (f64, f64, f64, f64)) -> Self {
        Self {
            node_id,
            backend,
            tag: "BUTTON",
            role: "button",
            label: label.to_owned(),
            rect,
            focused: false,
        }
    }

    pub fn link(node_id: i64, backend: i64, label: &str, rect: (f64, f64, f64, f64)) -> Self {
        Self {
            tag: "A",
            role: "link",
            ..Self::button(node_id, backend, label, rect)
        }
    }

    pub fn text_field(node_id: i64, backend: i64, label: &str, rect: (f64, f64, f64, f64)) -> Self {
        Self {
            tag: "INPUT",
            role: "textbox",
            ..Self::button(node_id, backend, label, rect)
        }
    }

    pub fn focused(mut self) -> Self {
        self.focused = true;
        self
    }
}

/// The `Page.getNavigationHistory` step.
#[derive(Clone, Debug)]
pub enum HistorySpec {
    Entry {
        url: String,
        title: String,
    },
    /// A CDP error result. Page state is unknown.
    ProtocolError,
    /// A result with no entries. Page state is unknown.
    NoEntries,
}

/// One observation of a page.
#[derive(Clone, Debug)]
pub struct PageSpec {
    pub width: f64,
    pub height: f64,
    pub dom: Vec<DomSpec>,
    pub ax: Vec<AxSpec>,
    pub history: HistorySpec,
    /// `DOM.getNodeForLocation` override keyed by the clickable region's
    /// backend id. Absent keys return the region's own backend (clear hit).
    /// Use this to script a cookie banner or custom backdrop covering a
    /// control: map the buried control's backend to the overlay's backend.
    pub hit_overrides: BTreeMap<i64, i64>,
    /// Optional computed-style overrides keyed by DOM `nodeId`. Absent keys
    /// get a default static stacking style. Enables stacking-map fixtures
    /// without relying on hit-test overrides.
    pub style_overrides: BTreeMap<i64, Vec<(String, String)>>,
}

impl PageSpec {
    /// A page of controls that both DOM and accessibility report.
    pub fn of(controls: &[Control], url: &str, title: &str) -> Self {
        let dom = controls
            .iter()
            .map(|control| {
                DomSpec::button(
                    control.node_id,
                    control.backend,
                    &control.label,
                    control.rect,
                )
                .with_tag(control.tag)
            })
            .collect();
        let ax = controls
            .iter()
            .map(|control| {
                let node = AxSpec::new(control.backend, control.role, &control.label, control.rect);
                if control.focused {
                    node.focused()
                } else {
                    node
                }
            })
            .collect();
        Self::new(dom, ax, url, title)
    }

    /// Replace the history step.
    pub fn with_history(mut self, history: HistorySpec) -> Self {
        self.history = history;
        self
    }

    pub fn new(dom: Vec<DomSpec>, ax: Vec<AxSpec>, url: &str, title: &str) -> Self {
        Self {
            width: 1280.0,
            height: 720.0,
            dom,
            ax,
            history: HistorySpec::Entry {
                url: url.to_owned(),
                title: title.to_owned(),
            },
            hit_overrides: BTreeMap::new(),
            style_overrides: BTreeMap::new(),
        }
    }

    /// Cover `target_backend`'s center with `hit_backend` (hit-test overlay).
    pub fn cover(mut self, target_backend: i64, hit_backend: i64) -> Self {
        self.hit_overrides.insert(target_backend, hit_backend);
        self
    }

    /// Set computed style for a DOM `nodeId` (stacking-map fixtures).
    pub fn style(mut self, node_id: i64, pairs: Vec<(&str, &str)>) -> Self {
        self.style_overrides.insert(
            node_id,
            pairs
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        );
        self
    }
}

/// A sequence of observations and presses, in call order.
#[derive(Clone, Debug, Default)]
pub struct ScriptBuilder {
    calls: Vec<Value>,
}

impl ScriptBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The calls of one `observe`.
    pub fn observe(mut self, page: &PageSpec) -> Self {
        self.calls.push(result(
            "Page.getLayoutMetrics",
            json!({"cssLayoutViewport": {
                "clientWidth": page.width, "clientHeight": page.height, "pageX": 0, "pageY": 0
            }}),
        ));
        let children: Vec<Value> = page.dom.iter().map(dom_json).collect();
        self.calls.push(result(
            "DOM.getDocument",
            json!({"root": {
                "nodeId": 1, "backendNodeId": 1, "nodeType": 9, "nodeName": "#document",
                "children": children
            }}),
        ));
        let nodes: Vec<Value> = page.ax.iter().enumerate().map(ax_json).collect();
        self.calls.push(result(
            "Accessibility.getFullAXTree",
            json!({"nodes": nodes}),
        ));
        let mut flat = Vec::new();
        for node in &page.dom {
            flatten(node, &mut flat);
        }
        for node in flat {
            self.calls.push(box_call(node.rect));
        }
        for node in &page.ax {
            // Match `extract::ax_role`: skipped roles never request a box.
            if node.backend.is_some() && ax_role_kept(node.role) {
                self.calls.push(box_call(node.rect));
            }
        }
        self.calls.push(match &page.history {
            HistorySpec::Entry { url, title } => result(
                "Page.getNavigationHistory",
                json!({"currentIndex": 0, "entries": [{"id": 1, "url": url, "title": title}]}),
            ),
            HistorySpec::ProtocolError => error("Page.getNavigationHistory", "history failed"),
            HistorySpec::NoEntries => result("Page.getNavigationHistory", json!({})),
        });
        // Stacking map: CSS.enable + computed style per kept DOM node, in
        // RegionId order (same as BrowserSession::apply_stacking_occlusion).
        self.calls.push(result("CSS.enable", json!({})));
        for (node_id, pairs) in style_targets(page) {
            let computed = pairs
                .iter()
                .map(|(name, value)| json!({"name": name, "value": value}))
                .collect::<Vec<_>>();
            self.calls.push(result(
                "CSS.getComputedStyleForNode",
                json!({"computedStyle": computed}),
            ));
            let _ = node_id; // params are not checked by ReplayTransport unless set
        }
        // Hit-test each clickable fused region in RegionId order (same order
        // BrowserSession::apply_hit_test_occlusion walks the manifold).
        for (_id, backend, _x, _y) in clickable_targets(page) {
            let hit = page.hit_overrides.get(&backend).copied().unwrap_or(backend);
            self.calls.push(result(
                "DOM.getNodeForLocation",
                json!({"backendNodeId": hit}),
            ));
        }
        self
    }

    /// A DOM click by node id that succeeds.
    pub fn dom_click(mut self, node_id: i64) -> Self {
        let object = format!("obj-{node_id}");
        self.calls.push(result(
            "DOM.resolveNode",
            json!({"object": {"type": "object", "objectId": object}}),
        ));
        self.calls.push(result(
            "Runtime.callFunctionOn",
            json!({"result": {"type": "undefined"}}),
        ));
        self
    }

    /// Both DOM click tiers fail with a protocol error (the node is gone), and
    /// the coordinate click is scripted.
    pub fn stale_node_then_coordinate_click(mut self) -> Self {
        self.calls
            .push(error("DOM.resolveNode", "No node with given id found"));
        self.calls
            .push(error("DOM.resolveNode", "No node with given id found"));
        self.calls
            .push(result("Input.dispatchMouseEvent", json!({})));
        self.calls
            .push(result("Input.dispatchMouseEvent", json!({})));
        self
    }

    /// Number of scripted CDP calls so far.
    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    pub fn to_json(&self) -> String {
        json!({"calls": self.calls}).to_string()
    }
}

fn result(method: &str, value: Value) -> Value {
    json!({"method": method, "result": value})
}

fn error(method: &str, message: &str) -> Value {
    json!({"method": method, "error": message})
}

fn box_call(rect: Option<(f64, f64, f64, f64)>) -> Value {
    match rect {
        Some((x, y, w, h)) => result(
            "DOM.getBoxModel",
            json!({"model": {"content": [x, y, x + w, y, x + w, y + h, x, y + h]}}),
        ),
        None => error("DOM.getBoxModel", "Could not compute box model."),
    }
}

fn flatten<'a>(node: &'a DomSpec, out: &mut Vec<&'a DomSpec>) {
    out.push(node);
    for child in &node.children {
        flatten(child, out);
    }
}

fn dom_json(node: &DomSpec) -> Value {
    let mut children = Vec::new();
    if !node.label.is_empty() {
        children.push(json!({
            "nodeId": node.node_id * 1000 + 1,
            "backendNodeId": node.backend * 1000 + 1,
            "nodeType": 3,
            "nodeName": "#text",
            "nodeValue": node.label,
        }));
    }
    children.extend(node.children.iter().map(dom_json));
    let attributes: Vec<&str> = node
        .attributes
        .iter()
        .flat_map(|(name, value)| [name.as_str(), value.as_str()])
        .collect();
    json!({
        "nodeId": node.node_id,
        "backendNodeId": node.backend,
        "nodeType": 1,
        "nodeName": node.tag,
        "attributes": attributes,
        "children": children,
    })
}

fn ax_json((index, node): (usize, &AxSpec)) -> Value {
    let mut value = json!({
        "nodeId": format!("ax{index}"),
        "ignored": false,
        "role": {"value": node.role},
        "name": {"value": node.name},
    });
    if let Some(backend) = node.backend {
        value["backendDOMNodeId"] = json!(backend);
    }
    let mut properties = Vec::new();
    if node.focused {
        properties.push(json!({"name": "focused", "value": {"type": "boolean", "value": true}}));
    }
    if node.modal {
        properties.push(json!({"name": "modal", "value": {"type": "boolean", "value": true}}));
    }
    if !properties.is_empty() {
        value["properties"] = json!(properties);
    }
    value
}

/// Kept DOM nodes that receive a stacking style, in RegionId (`n{backend}`) order.
fn style_targets(page: &PageSpec) -> Vec<(i64, Vec<(String, String)>)> {
    let mut flat = Vec::new();
    for node in &page.dom {
        flatten(node, &mut flat);
    }
    let mut targets: Vec<(String, i64, i64)> = Vec::new();
    for node in flat {
        if node.rect.is_none() {
            continue;
        }
        // Only nodes observe keeps as regions get styles. Keep rule matches
        // extract::keep_element for the tags ScriptBuilder emits.
        if !dom_node_kept(node) {
            continue;
        }
        targets.push((format!("n{}", node.backend), node.node_id, node.backend));
    }
    targets.sort_by(|left, right| left.0.cmp(&right.0));
    targets
        .into_iter()
        .map(|(_id, node_id, _backend)| {
            let pairs = page
                .style_overrides
                .get(&node_id)
                .cloned()
                .unwrap_or_else(default_stacking_style);
            (node_id, pairs)
        })
        .collect()
}

fn default_stacking_style() -> Vec<(String, String)> {
    vec![
        ("z-index".into(), "auto".into()),
        ("position".into(), "static".into()),
        ("opacity".into(), "1".into()),
        ("transform".into(), "none".into()),
        ("filter".into(), "none".into()),
        ("isolation".into(), "auto".into()),
        ("mix-blend-mode".into(), "normal".into()),
        ("will-change".into(), "auto".into()),
        ("pointer-events".into(), "auto".into()),
    ]
}

fn dom_node_kept(node: &DomSpec) -> bool {
    let role_attr = node
        .attributes
        .iter()
        .find(|(name, _)| name == "role")
        .map(|(_, value)| value.as_str());
    let label = node.label.trim();
    if role_attr.is_some() {
        return true;
    }
    let tag = node.tag.to_ascii_uppercase();
    matches!(
        tag.as_str(),
        "BUTTON"
            | "A"
            | "INPUT"
            | "TEXTAREA"
            | "SELECT"
            | "NAV"
            | "H1"
            | "H2"
            | "H3"
            | "H4"
            | "H5"
            | "H6"
    ) || (!label.is_empty() && matches!(tag.as_str(), "LABEL" | "SPAN" | "P" | "DIV"))
}

/// Clickable fused regions in `n{backend}` / `ax{backend}` id order.
///
/// Mirrors the session's hit-test targets closely enough for scripted pages:
/// DOM-kept clickable controls, then AX-only clickable leftovers.
fn clickable_targets(page: &PageSpec) -> Vec<(String, i64, f64, f64)> {
    let mut flat = Vec::new();
    for node in &page.dom {
        flatten(node, &mut flat);
    }
    let mut dom_backends = std::collections::BTreeSet::new();
    let mut targets: Vec<(String, i64, f64, f64)> = Vec::new();
    for node in flat {
        let Some(rect) = node.rect else {
            continue;
        };
        if !dom_role_is_clickable(node) {
            continue;
        }
        dom_backends.insert(node.backend);
        let (x, y, w, h) = rect;
        targets.push((
            format!("n{}", node.backend),
            node.backend,
            x + w / 2.0,
            y + h / 2.0,
        ));
    }
    for node in &page.ax {
        let Some(backend) = node.backend else {
            continue;
        };
        if dom_backends.contains(&backend) {
            continue;
        }
        if !ax_role_is_clickable(node.role) {
            continue;
        }
        let Some(rect) = node.rect else {
            continue;
        };
        let (x, y, w, h) = rect;
        targets.push((format!("ax{backend}"), backend, x + w / 2.0, y + h / 2.0));
    }
    targets.sort_by(|left, right| left.0.cmp(&right.0));
    targets
}

fn dom_role_is_clickable(node: &DomSpec) -> bool {
    let role_attr = node
        .attributes
        .iter()
        .find(|(name, _)| name == "role")
        .map(|(_, value)| value.as_str());
    let role = match role_attr {
        Some("button") | Some("link") | Some("menuitem") | Some("tab") | Some("slider") => {
            return true;
        }
        Some("textbox") | Some("searchbox") | Some("checkbox") => return true,
        Some("dialog") | Some("alertdialog") | Some("row") | Some("navigation") => return false,
        _ => node.tag.to_ascii_uppercase(),
    };
    matches!(
        role.as_str(),
        "BUTTON" | "A" | "INPUT" | "TEXTAREA" | "SELECT"
    )
}

fn ax_role_kept(role: &str) -> bool {
    !matches!(
        role.to_ascii_lowercase().as_str(),
        "statictext"
            | "inlinetextbox"
            | "none"
            | "generic"
            | "rootwebarea"
            | "genericcontainer"
            | "inline"
            | ""
    )
}

fn ax_role_is_clickable(role: &str) -> bool {
    matches!(
        role.to_ascii_lowercase().as_str(),
        "button"
            | "link"
            | "textbox"
            | "searchbox"
            | "textfield"
            | "checkbox"
            | "menuitem"
            | "tab"
            | "slider"
    )
}

#[cfg(test)]
mod hit_script_tests {
    use super::*;
    #[test]
    fn one_button_page_scripts_one_hit_test() {
        let page = PageSpec::of(
            &[Control::button(
                10,
                100,
                "Sign in",
                (100.0, 200.0, 80.0, 32.0),
            )],
            "http://x",
            "X",
        );
        let json = ScriptBuilder::new().observe(&page).to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let hits: Vec<_> = value["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["method"] == "DOM.getNodeForLocation")
            .collect();
        eprintln!("calls={}", value["calls"].as_array().unwrap().len());
        eprintln!("hits={hits:?}");
        assert_eq!(hits.len(), 1, "{json}");
        assert_eq!(hits[0]["result"]["backendNodeId"], 100);
    }
}

#[cfg(test)]
mod overlay_script_tests {
    use super::*;
    #[test]
    fn cookie_backdrop_script_shape() {
        let backdrop = DomSpec::container(
            50,
            500,
            "generic",
            "Cookie consent",
            (0.0, 0.0, 1440.0, 900.0),
        );
        let save = DomSpec::button(10, 100, "Save", (1200.0, 780.0, 100.0, 36.0));
        let accept = DomSpec::button(51, 510, "Accept all", (1200.0, 40.0, 120.0, 36.0));
        let mut page = PageSpec::new(
            vec![save, backdrop.with_children(vec![accept])],
            vec![
                AxSpec::new(100, "button", "Save", (1200.0, 780.0, 100.0, 36.0)),
                AxSpec::new(500, "generic", "Cookie consent", (0.0, 0.0, 1440.0, 900.0)),
                AxSpec::new(510, "button", "Accept all", (1200.0, 40.0, 120.0, 36.0)),
            ],
            "http://127.0.0.1/docs",
            "Docs",
        );
        page = page.cover(100, 500);
        let json = ScriptBuilder::new().observe(&page).to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let methods: Vec<_> = value["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["method"].as_str().unwrap())
            .collect();
        assert!(methods.contains(&"DOM.getNodeForLocation"));
        assert_eq!(clickable_targets(&page).len(), 2);
        let hits: Vec<_> = value["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["method"] == "DOM.getNodeForLocation")
            .map(|c| c["result"]["backendNodeId"].as_i64().unwrap())
            .collect();
        assert_eq!(hits, vec![500, 510]); // Save covered by backdrop 500
    }
}
