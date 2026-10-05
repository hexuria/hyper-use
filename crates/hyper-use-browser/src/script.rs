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
        }
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
            if node.backend.is_some() {
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
