//! `browser_*` tools — the browser-use-compatible host surface.
//!
//! These tools drive a live CDP endpoint the way browser-use's MCP server
//! does, under the same names. The difference is under the hood: target-bound
//! input (`browser_click`, `browser_type`) still goes through gate → ticket →
//! `aui_agent::execute_ticketed`, so a stale page refuses instead of clicking
//! the wrong element. Page-level calls (navigate, scroll, screenshot, exec,
//! tabs) are plain CDP like browser-use's.
//!
//! Element indexes follow browser-use convention: interactive regions in
//! reading order (top-to-bottom, left-to-right), 1-based, recomputed from the
//! current observation on every call. Coordinate clicks are deliberately not
//! offered — the surface acts on region identity, never on guessed pixels.

use serde_json::{json, Value};

use aui_agent::{execute_ticketed, Input};
use aui_cdp::{activate_target, close_target, create_target, page_targets, DEFAULT_CDP_HTTP};
use aui_core::{Action, ActionSpace, InteractionManifold, InteractionRegion, RegionId};
use aui_guard::gate;

use crate::error::ToolError;
use crate::server::Server;

/// Longest a `browser_wait` sleeps, whatever the argument says.
const MAX_WAIT_SECS: u64 = 60;

/// All `browser_*` tool names this module serves.
pub const BROWSER_TOOLS: [&str; 14] = [
    "browser_navigate",
    "browser_new_tab",
    "browser_go_back",
    "browser_wait",
    "browser_get_state",
    "browser_get_html",
    "browser_get_text",
    "browser_screenshot",
    "browser_scroll",
    "browser_click",
    "browser_type",
    "browser_list_tabs",
    "browser_switch_tab",
    "browser_close_tab",
];

/// Run one `browser_*` tool against this server's state.
pub(crate) fn call(server: &mut Server, name: &str, arguments: &Value) -> Result<Value, ToolError> {
    match name {
        "browser_navigate" => navigate(server, arguments),
        "browser_new_tab" => new_tab(server, arguments),
        "browser_go_back" => go_back(server, arguments),
        "browser_wait" => wait(arguments),
        "browser_get_state" => get_state(server, arguments),
        "browser_get_html" => get_html(server, arguments),
        "browser_get_text" => get_text(server, arguments),
        "browser_screenshot" => screenshot(server, arguments),
        "browser_scroll" => scroll(server, arguments),
        "browser_click" => click(server, arguments),
        "browser_type" => type_text(server, arguments),
        "browser_list_tabs" => list_tabs(arguments),
        "browser_switch_tab" => switch_tab(server, arguments),
        "browser_close_tab" => close_tab(server, arguments),
        other => Err(ToolError::UnknownTool(other.to_owned())),
    }
}

/// `cdp` argument or the documented default endpoint.
fn endpoint_of(arguments: &Value) -> Result<String, ToolError> {
    Ok(opt_str(arguments, "cdp")?
        .unwrap_or(DEFAULT_CDP_HTTP)
        .to_owned())
}

fn opt_str<'a>(arguments: &'a Value, key: &str) -> Result<Option<&'a str>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(other) => Err(ToolError::InvalidArguments(format!(
            "`{key}` must be a string, got {other}"
        ))),
    }
}

fn req_str<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    opt_str(arguments, key)?
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing required `{key}`")))
}

fn opt_usize(arguments: &Value, key: &str) -> Result<Option<usize>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|v| usize::try_from(v).unwrap_or(usize::MAX))
            .map(Some)
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!("`{key}` must be a non-negative integer"))
            }),
        Some(other) => Err(ToolError::InvalidArguments(format!(
            "`{key}` must be a non-negative integer, got {other}"
        ))),
    }
}

fn opt_f64(arguments: &Value, key: &str) -> Result<Option<f64>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| ToolError::InvalidArguments(format!("`{key}` must be finite"))),
        Some(other) => Err(ToolError::InvalidArguments(format!(
            "`{key}` must be a number, got {other}"
        ))),
    }
}

fn opt_bool(arguments: &Value, key: &str) -> Result<Option<bool>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(ToolError::InvalidArguments(format!(
            "`{key}` must be a boolean, got {other}"
        ))),
    }
}

fn browser_err(err: impl ToString) -> ToolError {
    ToolError::Browser(err.to_string())
}

/// The page-websocket key a `cdp` endpoint currently drives.
fn session_key(server: &Server, endpoint: &str) -> String {
    server
        .current_tab(endpoint)
        .map(str::to_owned)
        .unwrap_or_else(|| endpoint.to_owned())
}

/// Borrow-free take/keep: the session for `key`, reconnected when absent.
fn take(server: &mut Server, key: &str) -> Result<crate::server::LiveSession, ToolError> {
    server.take_session(key)
}

fn keep(server: &mut Server, key: &str, session: crate::server::LiveSession) {
    server.keep_session(key, session);
}

fn navigate(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let url = req_str(arguments, "url")?;
    if opt_bool(arguments, "new_tab")? == Some(true) {
        return new_tab_at(server, &endpoint, url);
    }
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let result = session.navigate(url);
    let outcome = result.map(|_| {
        let _ = session.ready_state();
        json!({"navigated": url})
    });
    keep(server, &key, session);
    outcome.map_err(browser_err)
}

fn new_tab(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let url = opt_str(arguments, "url")?.unwrap_or("about:blank");
    new_tab_at(server, &endpoint, url)
}

/// Create the target, point this endpoint's session at it, keep it.
fn new_tab_at(server: &mut Server, endpoint: &str, url: &str) -> Result<Value, ToolError> {
    let target = create_target(endpoint, url, true).map_err(browser_err)?;
    server.set_current_tab(endpoint, &target.ws_url);
    let mut session = take(server, &target.ws_url)?;
    let _ = session.ready_state();
    keep(server, &target.ws_url, session);
    Ok(json!({
        "tab_id": target.target_id,
        "url": url,
        "new_tab": true,
    }))
}

fn go_back(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = session.go_back().map(|_| {
        let _ = session.ready_state();
        json!({"went_back": true})
    });
    keep(server, &key, session);
    outcome.map_err(browser_err)
}

fn wait(arguments: &Value) -> Result<Value, ToolError> {
    let seconds = opt_f64(arguments, "seconds")?.unwrap_or(3.0);
    let seconds = seconds.clamp(0.0, MAX_WAIT_SECS as f64);
    std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
    Ok(json!({"waited": seconds}))
}

/// Interactive regions in browser-use reading order: top-to-bottom rows,
/// left-to-right within a row (ties by id). Viability is the action-space
/// rule, so the same index always maps the same way for every tool.
fn interactive_index(manifold: &InteractionManifold) -> Vec<RegionId> {
    let space = ActionSpace::from_manifold(manifold);
    let mut seen = Vec::<RegionId>::new();
    for action in space.actions() {
        let Some(target) = action.target() else {
            continue;
        };
        if !seen.contains(target) {
            seen.push(target.clone());
        }
    }
    let mut regions: Vec<&InteractionRegion> =
        seen.iter().filter_map(|id| manifold.get(id)).collect();
    regions.sort_by(|a, b| {
        let ra = a.rect();
        let rb = b.rect();
        let row_a = (ra.y() / 20.0).floor() as i64;
        let row_b = (rb.y() / 20.0).floor() as i64;
        (row_a, ra.x() as i64, a.id().as_str()).cmp(&(row_b, rb.x() as i64, b.id().as_str()))
    });
    regions.into_iter().map(|r| r.id().clone()).collect()
}

fn element_json(index: usize, region: &InteractionRegion) -> Value {
    let rect = region.rect();
    json!({
        "index": index,
        "region": region.id().as_str(),
        "role": region.role().to_string(),
        "label": region.label(),
        "actions": region.actions().iter().map(|a| a.to_string()).collect::<Vec<_>>(),
        "rect": {"x": rect.x(), "y": rect.y(), "w": rect.width(), "h": rect.height()},
    })
}

fn get_state(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = (|| {
        let page = session.page().cloned();
        let manifold = session.observe().map_err(browser_err)?;
        let order = interactive_index(manifold);
        let elements: Vec<Value> = order
            .iter()
            .enumerate()
            .filter_map(|(i, id)| manifold.get(id).map(|r| element_json(i + 1, r)))
            .collect();
        Ok::<_, ToolError>(json!({
            "url": page.as_ref().and_then(|p| p.url().map(str::to_owned)),
            "title": page.as_ref().and_then(|p| p.title().map(str::to_owned)),
            "interactive_elements": elements.len(),
            "elements": elements,
        }))
    })();
    keep(server, &key, session);
    outcome
}

fn get_html(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = session
        .outer_html()
        .map(|html| json!({"html": html}))
        .map_err(browser_err);
    keep(server, &key, session);
    outcome
}

fn get_text(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = session
        .page_text()
        .map(|text| json!({"text": text}))
        .map_err(browser_err);
    keep(server, &key, session);
    outcome
}

fn screenshot(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = session
        .screenshot_png()
        .map(|png| json!({"format": "png", "base64": png}))
        .map_err(browser_err);
    keep(server, &key, session);
    outcome
}

fn scroll(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let down = opt_bool(arguments, "down")?.unwrap_or(true);
    let pages = opt_f64(arguments, "pages")?.unwrap_or(1.0);
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    // scroll_pages needs a stored observation for the viewport height.
    let outcome = (|| {
        if session.manifold().is_none() {
            session.observe().map_err(browser_err)?;
        }
        session
            .scroll_pages(down, pages)
            .map(|_| json!({"scrolled": if down { "down" } else { "up" }, "pages": pages}))
            .map_err(browser_err)
    })();
    keep(server, &key, session);
    outcome
}

/// One gated, ticketed input against `index`. The executor re-observes,
/// revalidates the target-scoped world, re-gates, consumes, then dispatches —
/// a page that drifted between get_state and this call refuses, it does not
/// click the wrong element.
fn gated_input(
    server: &mut Server,
    arguments: &Value,
    input_kind: &str,
) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let index = opt_usize(arguments, "index")?
        .ok_or_else(|| ToolError::InvalidArguments("missing required `index`".into()))?;
    let input = match input_kind {
        "click" => Input::Click,
        "type" => Input::Type(req_str(arguments, "text")?.to_owned()),
        other => return Err(ToolError::UnknownTool(other.to_owned())),
    };
    let action = match input {
        Input::Click | Input::PointerClick => Action::Click,
        Input::Type(_) => Action::Type,
        Input::Select(_) => Action::Select,
    };
    let key = session_key(server, &endpoint);
    let mut session = take(server, &key)?;
    let outcome = (|| {
        let focused = session.page().and_then(|p| p.focused().cloned());
        let manifold = session.observe().map_err(browser_err)?;
        let order = interactive_index(manifold);
        let region_id = order.get(index.wrapping_sub(1)).cloned().ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "no interactive element at index {index} ({} indexed)",
                order.len()
            ))
        })?;
        let ticket = gate(
            manifold,
            &region_id,
            action,
            focused,
            manifold.captured_at_ms(),
        )
        .map_err(|reason| ToolError::TicketInvalid(reason.to_string()))?;
        let mut ledger = server.take_ledger(&key);
        let executed = execute_ticketed(&mut session, &mut ledger, &ticket, &input)
            .map_err(|err| ToolError::TicketInvalid(err.to_string()));
        server.keep_ledger(&key, ledger);
        executed.map(|done| {
            json!({
                "acted": input_kind,
                "index": index,
                "region": done.target.as_str(),
            })
        })
    })();
    keep(server, &key, session);
    outcome
}

fn click(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    gated_input(server, arguments, "click")
}

fn type_text(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    gated_input(server, arguments, "type")
}

fn list_tabs(arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let tabs: Vec<Value> = page_targets(&endpoint)
        .map_err(browser_err)?
        .iter()
        .map(|t| {
            json!({
                "tab_id": t.target_id,
                "url": t.url,
                "title": t.title,
            })
        })
        .collect();
    Ok(json!({"tabs": tabs}))
}

fn switch_tab(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let tab_id = req_str(arguments, "tab_id")?;
    let target = page_targets(&endpoint)
        .map_err(browser_err)?
        .into_iter()
        .find(|t| t.target_id == tab_id || t.target_id.starts_with(tab_id))
        .ok_or_else(|| ToolError::InvalidArguments(format!("no tab `{tab_id}`")))?;
    activate_target(&endpoint, &target.target_id).map_err(browser_err)?;
    server.set_current_tab(&endpoint, &target.ws_url);
    Ok(json!({"active_tab": target.target_id, "url": target.url}))
}

fn close_tab(server: &mut Server, arguments: &Value) -> Result<Value, ToolError> {
    let endpoint = endpoint_of(arguments)?;
    let tab_id = req_str(arguments, "tab_id")?;
    let targets = page_targets(&endpoint).map_err(browser_err)?;
    let target = targets
        .iter()
        .find(|t| t.target_id == tab_id || t.target_id.starts_with(tab_id))
        .ok_or_else(|| ToolError::InvalidArguments(format!("no tab `{tab_id}`")))?;
    let closed = close_target(&endpoint, &target.target_id).map_err(browser_err)?;
    // If the closed tab was bound, fall back to any remaining page target.
    if let Some(bound) = server.current_tab(&endpoint).map(str::to_owned) {
        if bound == target.ws_url {
            server.drop_session(&bound);
            match page_targets(&endpoint)
                .map_err(browser_err)?
                .first()
                .map(|t| t.ws_url.clone())
            {
                Some(next) => server.set_current_tab(&endpoint, &next),
                None => server.clear_current_tab(&endpoint),
            }
        }
    }
    Ok(json!({"closed": closed, "tab_id": target.target_id}))
}

/// Tool spec fragments shared by every `browser_*` tool.
pub(crate) fn spec(name: &str) -> (String, serde_json::Map<String, Value>, Vec<&'static str>) {
    let cdp = || {
        (
            "cdp",
            json!({"type": "string", "description": format!("CDP debugging endpoint; default {DEFAULT_CDP_HTTP}")}),
        )
    };
    let mut props = serde_json::Map::new();
    props.insert(cdp().0.to_owned(), cdp().1);
    let mut required = Vec::new();
    let description = match name {
        "browser_navigate" => {
            props.insert("url".into(), json!({"type": "string"}));
            props.insert("new_tab".into(), json!({"type": "boolean"}));
            required.push("url");
            "Navigate the current tab to url; new_tab=true opens it in a new tab."
        }
        "browser_new_tab" => {
            props.insert("url".into(), json!({"type": "string"}));
            "Open a new tab (default about:blank) and make it the current one."
        }
        "browser_go_back" => "Go back one history entry.",
        "browser_wait" => {
            props.insert("seconds".into(), json!({"type": "number"}));
            "Wait seconds (default 3, capped at 60)."
        }
        "browser_get_state" => {
            "Page url, title, and interactive elements as 1-based indexes in reading order — the numbers browser_click/browser_type take."
        }
        "browser_get_html" => "document.documentElement.outerHTML of the current tab.",
        "browser_get_text" => "document.body.innerText — visible page text.",
        "browser_screenshot" => "PNG screenshot of the viewport, base64.",
        "browser_scroll" => {
            props.insert("down".into(), json!({"type": "boolean"}));
            props.insert("pages".into(), json!({"type": "number"}));
            "Scroll pages viewport heights (default 1.0) down or up."
        }
        "browser_click" => {
            props.insert("index".into(), json!({"type": "integer"}));
            required.push("index");
            "Left-click the interactive element at a browser_get_state index, through the ticketed executor (re-observes and refuses on drift)."
        }
        "browser_type" => {
            props.insert("index".into(), json!({"type": "integer"}));
            props.insert("text".into(), json!({"type": "string"}));
            required.extend(["index", "text"]);
            "Set the text of the interactive element at a browser_get_state index, through the ticketed executor."
        }
        "browser_list_tabs" => "List open page tabs (id, url, title).",
        "browser_switch_tab" => {
            props.insert("tab_id".into(), json!({"type": "string"}));
            required.push("tab_id");
            "Activate a tab by id and drive it in later calls."
        }
        "browser_close_tab" => {
            props.insert("tab_id".into(), json!({"type": "string"}));
            required.push("tab_id");
            "Close a tab by id; falls back to another tab if it was current."
        }
        other => return (other.to_owned(), props, required),
    };
    (description.to_owned(), props, required)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: &str, x: f64, y: f64, actions: &[Action]) -> InteractionRegion {
        use aui_core::{Rect, RegionFlags, RegionParts, Role, SourceMask};
        InteractionRegion::try_new(RegionParts {
            id: RegionId::try_new(id).unwrap(),
            role: Role::Button,
            label: id.to_owned(),
            rect: Rect::try_new(x, y, 50.0, 10.0).unwrap(),
            actions: actions.to_vec(),
            parent: None,
            sources: SourceMask::DOM.union(SourceMask::ACCESSIBILITY),
            flags: RegionFlags::default(),
            temporal_stability: aui_core::UnitInterval::ONE,
        })
        .unwrap()
    }

    #[test]
    fn index_is_reading_order_and_stable() {
        use aui_core::{InteractionManifold, Rect};
        let manifold = InteractionManifold::try_new(
            Rect::try_new(0.0, 0.0, 800.0, 600.0).unwrap(),
            vec![
                region("b2", 10.0, 40.0, &[Action::Click]),
                region("a1", 10.0, 10.0, &[Action::Click]),
                region("a2", 200.0, 12.0, &[Action::Click]),
            ],
            0,
        )
        .unwrap();
        let order = interactive_index(&manifold);
        let names: Vec<&str> = order.iter().map(RegionId::as_str).collect();
        assert_eq!(names, ["a1", "a2", "b2"]);
        // Deterministic: same manifold → same order.
        assert_eq!(interactive_index(&manifold), order);
    }
}
