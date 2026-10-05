//! Turn CDP JSON into raw nodes. This is the only parser of these result
//! shapes. Fusion does not parse CDP.

use serde_json::Value;

use hyper_use_core::{Action, Rect, Role};

use crate::error::{BrowserError, CdpError};

#[derive(Clone, Debug)]
pub(crate) struct DomElement {
    pub node_id: i64,
    pub backend_node_id: i64,
    pub role: Role,
    pub label: String,
    pub actions: Vec<Action>,
    pub disabled: bool,
    pub hidden: bool,
    /// `aria-modal="true"`. Only meaningful on a dialog.
    pub modal: bool,
    /// Backend ids of kept ancestors, nearest first.
    pub ancestors: Vec<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct AxElement {
    pub backend_dom_node_id: Option<i64>,
    pub role: Role,
    pub name: String,
    pub disabled: bool,
    pub focused: bool,
    /// The accessibility `modal` property. Chrome sets it on a dialog opened
    /// with `showModal()` or marked `aria-modal="true"`.
    pub modal: bool,
}

pub(crate) fn parse_viewport(result_json: &str) -> Result<hyper_use_core::Rect, BrowserError> {
    let value = parse_json(result_json)?;
    let css = value
        .get("cssLayoutViewport")
        .ok_or_else(|| BrowserError::BadViewport("missing cssLayoutViewport".into()))?;
    let width = number(css, "clientWidth")
        .ok_or_else(|| BrowserError::BadViewport("missing clientWidth".into()))?;
    let height = number(css, "clientHeight")
        .ok_or_else(|| BrowserError::BadViewport("missing clientHeight".into()))?;
    let x = number(css, "pageX").unwrap_or(0.0);
    let y = number(css, "pageY").unwrap_or(0.0);
    Rect::try_viewport(x, y, width, height)
        .map_err(|err| BrowserError::BadViewport(err.to_string()))
}

pub(crate) fn dom_elements(document_json: &str) -> Result<Vec<DomElement>, BrowserError> {
    let value = parse_json(document_json)?;
    let root = value.get("root").ok_or_else(|| {
        BrowserError::Cdp(CdpError::BadJson {
            message: "DOM.getDocument result has no root".into(),
        })
    })?;
    let mut out = Vec::new();
    let mut ancestors = Vec::new();
    walk_dom(root, &mut ancestors, &mut out);
    Ok(out)
}

pub(crate) fn ax_elements(tree_json: &str) -> Result<Vec<AxElement>, BrowserError> {
    let value = parse_json(tree_json)?;
    let nodes = value
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            BrowserError::Cdp(CdpError::BadJson {
                message: "Accessibility.getFullAXTree result has no nodes".into(),
            })
        })?;
    let mut out = Vec::new();
    for node in nodes {
        if node.get("ignored").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let role_value = node
            .get("role")
            .and_then(|role| role.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some(role) = ax_role(role_value) else {
            continue;
        };
        let name = node
            .get("name")
            .and_then(|name| name.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let backend = node.get("backendDOMNodeId").and_then(Value::as_i64);
        let disabled = ax_flag(node, "disabled");
        let focused = ax_flag(node, "focused");
        let modal = ax_flag(node, "modal");
        out.push(AxElement {
            backend_dom_node_id: backend,
            role,
            name,
            disabled,
            focused,
            modal,
        });
    }
    Ok(out)
}

/// `None` when the node has no box (it is omitted, not stored as zero size).
pub(crate) fn content_rect(box_json: &str) -> Result<Option<Rect>, BrowserError> {
    let value = parse_json(box_json)?;
    let Some(content) = value
        .get("model")
        .and_then(|model| model.get("content"))
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };
    if content.len() < 8 {
        return Ok(None);
    }
    let mut xs = [0.0; 4];
    let mut ys = [0.0; 4];
    for index in 0..4 {
        xs[index] = content[index * 2].as_f64().ok_or_else(|| {
            BrowserError::Cdp(CdpError::BadJson {
                message: "box model content is not numeric".into(),
            })
        })?;
        ys[index] = content[index * 2 + 1].as_f64().ok_or_else(|| {
            BrowserError::Cdp(CdpError::BadJson {
                message: "box model content is not numeric".into(),
            })
        })?;
    }
    let min_x = xs.into_iter().fold(f64::INFINITY, f64::min);
    let max_x = xs.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.into_iter().fold(f64::INFINITY, f64::min);
    let max_y = ys.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let rect = Rect::try_new(min_x, min_y, max_x - min_x, max_y - min_y).map_err(|err| {
        BrowserError::Cdp(CdpError::BadJson {
            message: err.to_string(),
        })
    })?;
    Ok(Some(rect))
}

pub(crate) fn object_id(resolve_json: &str) -> Result<String, BrowserError> {
    let value = parse_json(resolve_json)?;
    value
        .get("object")
        .and_then(|object| object.get("objectId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(BrowserError::MissingObjectId)
}

/// `Runtime.callFunctionOn` reports a thrown click as `exceptionDetails`.
pub(crate) fn call_threw(call_json: &str) -> Result<bool, BrowserError> {
    let value = parse_json(call_json)?;
    Ok(value.get("exceptionDetails").is_some())
}

/// `ancestors` holds the backend ids of kept elements above `node`, outermost
/// first. Each element records them nearest first.
fn walk_dom(node: &Value, ancestors: &mut Vec<i64>, out: &mut Vec<DomElement>) {
    let node_type = node.get("nodeType").and_then(Value::as_i64).unwrap_or(1);
    let mut pushed = false;
    if node_type == 1 {
        if let Some(mut element) = element_from(node) {
            element.ancestors = ancestors.iter().rev().copied().collect();
            ancestors.push(element.backend_node_id);
            pushed = true;
            out.push(element);
        }
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            walk_dom(child, ancestors, out);
        }
    }
    if pushed {
        ancestors.pop();
    }
}

fn element_from(node: &Value) -> Option<DomElement> {
    let name = node.get("nodeName").and_then(Value::as_str).unwrap_or("");
    if matches!(
        name.to_ascii_uppercase().as_str(),
        "SCRIPT" | "STYLE" | "HEAD" | "HTML" | "BODY" | "#DOCUMENT"
    ) {
        return None;
    }
    let attributes = attr_map(node);
    let label = attributes
        .get("aria-label")
        .cloned()
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| direct_text(node));
    let role_attr = attributes.get("role").cloned();
    if !keep_element(name, role_attr.as_deref(), &label) {
        return None;
    }
    let role = dom_role(
        name,
        role_attr.as_deref(),
        attributes.get("type").map(String::as_str),
    );
    let node_id = node.get("nodeId").and_then(Value::as_i64)?;
    let backend_node_id = node
        .get("backendNodeId")
        .and_then(Value::as_i64)
        .unwrap_or(node_id);
    let disabled = attributes.contains_key("disabled")
        || attributes.get("aria-disabled").map(String::as_str) == Some("true");
    let hidden = attributes.contains_key("hidden")
        || attributes.get("aria-hidden").map(String::as_str) == Some("true");
    let modal = attributes.get("aria-modal").map(String::as_str) == Some("true");
    Some(DomElement {
        node_id,
        backend_node_id,
        role,
        label,
        actions: actions_for_role(role),
        disabled,
        hidden,
        modal,
        ancestors: Vec::new(),
    })
}

/// Unlabeled generic containers are not regions. A control tag, an explicit
/// role, or a non-empty label on a textual element is.
fn keep_element(name: &str, role_attr: Option<&str>, label: &str) -> bool {
    if role_attr.is_some() {
        return true;
    }
    let upper = name.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "BUTTON"
            | "A"
            | "INPUT"
            | "TEXTAREA"
            | "SELECT"
            | "NAV"
            | "IMG"
            | "H1"
            | "H2"
            | "H3"
            | "H4"
            | "H5"
            | "H6"
            | "DIALOG"
    ) {
        return true;
    }
    !label.is_empty() && matches!(upper.as_str(), "LABEL" | "SPAN" | "P" | "DIV")
}

fn dom_role(name: &str, role_attr: Option<&str>, input_type: Option<&str>) -> Role {
    if let Some(role) = role_attr {
        if let Some(parsed) = Role::parse(&role.to_ascii_lowercase().replace('-', "_")) {
            return parsed;
        }
        if role.eq_ignore_ascii_case("textbox") {
            return Role::TextField;
        }
    }
    match name.to_ascii_uppercase().as_str() {
        "BUTTON" => Role::Button,
        "A" => Role::Link,
        "TEXTAREA" => Role::TextField,
        "SELECT" => Role::Generic,
        "NAV" => Role::Navigation,
        "IMG" => Role::Image,
        "H1" | "H2" | "H3" | "H4" | "H5" | "H6" => Role::Heading,
        "DIALOG" => Role::Dialog,
        "INPUT" => match input_type.unwrap_or("").to_ascii_lowercase().as_str() {
            "checkbox" => Role::Checkbox,
            "button" | "submit" => Role::Button,
            _ => Role::TextField,
        },
        _ => Role::Generic,
    }
}

pub(crate) fn actions_for_role(role: Role) -> Vec<Action> {
    match role {
        Role::TextField => vec![Action::Click, Action::Focus, Action::Type],
        Role::Checkbox => vec![Action::Click, Action::Focus, Action::Toggle],
        Role::Button | Role::Link | Role::MenuItem | Role::Tab => {
            vec![Action::Click, Action::Focus]
        }
        Role::Slider => vec![Action::Click, Action::Focus],
        _ => vec![Action::Focus],
    }
}

/// Roles that are not controls. `None` means skip the accessibility node.
fn ax_role(value: &str) -> Option<Role> {
    match value.to_ascii_lowercase().as_str() {
        "button" => Some(Role::Button),
        "link" => Some(Role::Link),
        "textbox" | "searchbox" | "textfield" => Some(Role::TextField),
        "checkbox" => Some(Role::Checkbox),
        "menuitem" => Some(Role::MenuItem),
        "navigation" => Some(Role::Navigation),
        "image" => Some(Role::Image),
        "heading" => Some(Role::Heading),
        "tab" => Some(Role::Tab),
        "slider" => Some(Role::Slider),
        "statictext" | "inlinetextbox" | "none" | "generic" | "rootwebarea"
        | "genericcontainer" | "inline" | "" => None,
        other => Role::parse(&other.replace('-', "_")).or(Some(Role::Generic)),
    }
}

fn ax_flag(node: &Value, name: &str) -> bool {
    let Some(properties) = node.get("properties").and_then(Value::as_array) else {
        return false;
    };
    properties.iter().any(|property| {
        property.get("name").and_then(Value::as_str) == Some(name)
            && property
                .get("value")
                .and_then(|value| value.get("value"))
                .and_then(Value::as_bool)
                == Some(true)
    })
}

/// URL and title of the current history entry. `None` when there is no
/// current entry, so the page state is unknown.
/// This does not read a timestamp: `Page.getNavigationHistory` has none.
pub(crate) fn navigation_entry(
    history_json: &str,
) -> Result<Option<(String, String)>, BrowserError> {
    let value = parse_json(history_json)?;
    let Some(entries) = value.get("entries").and_then(Value::as_array) else {
        return Ok(None);
    };
    let index = value
        .get("currentIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let Some(entry) = usize::try_from(index)
        .ok()
        .and_then(|index| entries.get(index))
    else {
        return Ok(None);
    };
    let url = entry
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let title = entry
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Ok(Some((url, title)))
}

fn direct_text(node: &Value) -> String {
    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return String::new();
    };
    let mut parts = Vec::new();
    for child in children {
        if child.get("nodeType").and_then(Value::as_i64) == Some(3) {
            if let Some(text) = child.get("nodeValue").and_then(Value::as_str) {
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_owned());
                }
            }
        }
    }
    parts.join(" ")
}

fn attr_map(node: &Value) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let Some(attributes) = node.get("attributes").and_then(Value::as_array) else {
        return map;
    };
    let mut index = 0;
    while index + 1 < attributes.len() {
        if let (Some(key), Some(value)) =
            (attributes[index].as_str(), attributes[index + 1].as_str())
        {
            map.insert(key.to_owned(), value.to_owned());
        }
        index += 2;
    }
    map
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn parse_json(text: &str) -> Result<Value, BrowserError> {
    serde_json::from_str(text).map_err(|err| {
        BrowserError::Cdp(CdpError::BadJson {
            message: err.to_string(),
        })
    })
}
