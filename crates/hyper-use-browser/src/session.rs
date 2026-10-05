//! Browser session. Observation and press go through [`CdpTransport`], whether
//! that transport is a replay script or a live CDP websocket.
//!
//! Snapshots use `captured_at_ms = 0`. The ranker must not see a local clock.
//! `Page.getNavigationHistory` may set the page URL and title. That payload
//! has no time, so it does not change `captured_at_ms`.
//!
//! Press preference, and only for [`Action::Click`]:
//! 1. DOM semantic click by node id (`DOM.resolveNode` + `Runtime.callFunctionOn`)
//! 2. DOM semantic click by backend node id (same calls, `backendNodeId`)
//! 3. coordinate click (`Input.dispatchMouseEvent`)
//!
//! A focus is not a click, so there is no `DOM.focus` tier. A CDP `error`
//! result, or a click function that reports `exceptionDetails`, fails that
//! tier and the next tier runs. A missing script entry is not a tier failure:
//! it is [`CdpError::NoScriptedResponse`] and the press stops, so a short
//! fixture cannot silently become a click.
//!
//! Observe omits a node whose `DOM.getBoxModel` is a CDP `error` (Chrome says
//! "Could not compute box model." for `display:none`). Any other failure of
//! that call is fatal.
//!
//! After fusion, observe hit-tests each clickable region's center with
//! `DOM.getNodeForLocation`. When the node under the center is not the region
//! or a descendant of it (cookie banner, custom backdrop, toast), the region
//! is marked `occluded`. Dialog front-layer logic in `hyper-use-guard` still
//! applies on top of that.

use std::collections::BTreeMap;

use serde_json::json;

use hyper_use_core::{Action, InteractionManifold, InteractionRegion, Rect, RegionId};

use crate::error::{ActMechanism, BrowserError, CdpError};
use crate::extract::{self, content_rect, AxElement};
use crate::fusion::{self, NodeBinding, RawNode};
use crate::identity::IdentityMap;
use crate::page::PageState;
use crate::transport::CdpTransport;
use crate::verify::{self, Expectation};

pub const DOM_CLICK_FUNCTION: &str = "function(){this.click()}";

pub struct BrowserSession<T: CdpTransport> {
    transport: T,
    manifold: Option<InteractionManifold>,
    page: Option<PageState>,
    bindings: BTreeMap<RegionId, NodeBinding>,
    identity: IdentityMap,
    /// A press ran after the stored observation. The page may have changed,
    /// so the stored manifold and bindings must not be reused as `before`.
    stale: bool,
}

impl<T: CdpTransport> BrowserSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            manifold: None,
            page: None,
            bindings: BTreeMap::new(),
            identity: IdentityMap::default(),
            stale: false,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn manifold(&self) -> Option<&InteractionManifold> {
        self.manifold.as_ref()
    }

    pub fn page(&self) -> Option<&PageState> {
        self.page.as_ref()
    }

    /// The stored observation, only if no press ran after it. A caller that
    /// reuses an observation as an act's `before` must use this, not
    /// [`Self::manifold`].
    pub fn fresh_manifold(&self) -> Option<&InteractionManifold> {
        if self.stale {
            None
        } else {
            self.manifold.as_ref()
        }
    }

    /// A press ran after the last observation.
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    pub fn observe(&mut self) -> Result<&InteractionManifold, BrowserError> {
        let layout = self.call("Page.getLayoutMetrics", &json!({}).to_string())?;
        let viewport = extract::parse_viewport(&layout)?;
        let document = self.call(
            "DOM.getDocument",
            &json!({"depth": -1, "pierce": false}).to_string(),
        )?;
        let dom = extract::dom_document(&document)?;
        let ax_tree = self.call("Accessibility.getFullAXTree", &json!({}).to_string())?;
        let ax_nodes = extract::ax_elements(&ax_tree)?;

        let mut dom_raw = Vec::new();
        for element in &dom.elements {
            let params = json!({"nodeId": element.node_id}).to_string();
            if let Some(rect) = self.box_rect(&params)? {
                dom_raw.push(RawNode::from_dom(element, rect));
            }
        }
        let mut ax_raw = Vec::new();
        for element in &ax_nodes {
            let Some(backend) = element.backend_dom_node_id else {
                continue;
            };
            let params = json!({"backendNodeId": backend}).to_string();
            if let Some(rect) = self.box_rect(&params)? {
                ax_raw.push(RawNode::from_ax(element, rect));
            }
        }
        let (fused, fused_bindings) = fusion::fuse(viewport, &dom_raw, &ax_raw)?;
        let (mut manifold, bindings) =
            self.identity
                .assign(self.manifold.as_ref(), fused, fused_bindings)?;
        let focused = focused_region(&ax_nodes, &bindings);
        let page = self.read_page(focused)?;
        // Hit-tests run after history so scripted CDP fixtures can append
        // `DOM.getNodeForLocation` after `Page.getNavigationHistory`.
        self.apply_hit_test_occlusion(&mut manifold, &bindings, &dom.parent_of)?;
        self.bindings = bindings;
        self.page = Some(page);
        self.manifold = Some(manifold);
        self.stale = false;
        Ok(self.manifold.as_ref().expect("observation just stored"))
    }

    /// Mark clickable regions whose center is covered by another node.
    ///
    /// `DOM.getNodeForLocation` at the region's center must land on the region
    /// itself or a descendant. Anything else (cookie banner, custom backdrop,
    /// toast) means a pointer click would not reach this control.
    fn apply_hit_test_occlusion(
        &mut self,
        manifold: &mut InteractionManifold,
        bindings: &BTreeMap<RegionId, NodeBinding>,
        parent_of: &BTreeMap<i64, i64>,
    ) -> Result<(), BrowserError> {
        let targets: Vec<(RegionId, i64, f64, f64)> = manifold
            .regions()
            .filter(|region| region.actions().contains(&Action::Click))
            .filter_map(|region| {
                let binding = bindings.get(region.id())?;
                let backend = binding.backend_node_id?;
                Some((
                    region.id().clone(),
                    backend,
                    binding.center_x,
                    binding.center_y,
                ))
            })
            .collect();
        let mut buried = Vec::new();
        for (id, backend, x, y) in targets {
            let params = json!({"x": x, "y": y}).to_string();
            let body = match self.call("DOM.getNodeForLocation", &params) {
                Ok(body) => body,
                Err(BrowserError::Cdp(CdpError::Protocol { .. })) => continue,
                Err(other) => return Err(other),
            };
            let Some(hit) = extract::location_backend(&body)? else {
                continue;
            };
            if !owns_hit(hit, backend, parent_of) {
                buried.push(id);
            }
        }
        for id in buried {
            let region = manifold
                .get(&id)
                .expect("id taken from this manifold")
                .clone();
            if region.flags().occluded() {
                continue;
            }
            let mut parts = region.to_parts();
            parts.flags.set_occluded(true);
            let updated =
                InteractionRegion::try_new(parts).expect("rebuilding a valid region cannot fail");
            manifold.replace(updated);
        }
        Ok(())
    }

    /// Low-level CDP click used only by browser fixture tests.
    ///
    /// **Not the product path.** Hyper-Use is an action firewall: hosts click
    /// after [`hyper_use_guard::guard`] returns Allow. Do not call this from
    /// MCP or CLI.
    #[doc(hidden)]
    pub fn press(&mut self, id: &RegionId, action: Action) -> Result<ActMechanism, BrowserError> {
        if action != Action::Click {
            return Err(BrowserError::UnsupportedAction(action.to_string()));
        }
        if self.manifold.is_none() {
            return Err(BrowserError::NotObserved);
        }
        let binding = self
            .bindings
            .get(id)
            .cloned()
            .ok_or_else(|| BrowserError::UnknownRegion(id.to_string()))?;
        // From here CDP click calls may reach the page, even if one fails.
        self.stale = true;
        if let Some(node_id) = binding.dom_node_id {
            if self.try_semantic_click(json!({"nodeId": node_id}))? {
                return Ok(ActMechanism::DomSemantic);
            }
        }
        if let Some(backend) = binding.backend_node_id {
            if self.try_semantic_click(json!({"backendNodeId": backend}))? {
                return Ok(ActMechanism::DomSemantic);
            }
        }
        self.coordinate_click(binding.center_x, binding.center_y)?;
        Ok(ActMechanism::Coordinate)
    }

    pub fn verify(&self, expectation: &Expectation) -> Result<(), BrowserError> {
        let manifold = self.manifold.as_ref().ok_or(BrowserError::NotObserved)?;
        verify::verify(manifold, expectation).map_err(BrowserError::Verify)
    }

    /// A protocol error or an empty history leaves URL and title unknown.
    /// Every other failure aborts.
    fn read_page(&mut self, focused: Option<RegionId>) -> Result<PageState, BrowserError> {
        match self.call("Page.getNavigationHistory", &json!({}).to_string()) {
            Ok(body) => Ok(match extract::navigation_entry(&body)? {
                Some((url, title)) => PageState::new(url, title, focused),
                None => PageState::unknown(focused),
            }),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(PageState::unknown(focused)),
            Err(other) => Err(other),
        }
    }

    /// `Ok(None)` when CDP reports an error for this node's box.
    fn box_rect(&mut self, params_json: &str) -> Result<Option<Rect>, BrowserError> {
        match self.call("DOM.getBoxModel", params_json) {
            Ok(body) => content_rect(&body),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// `node` is `{"nodeId": n}` or `{"backendNodeId": n}`.
    fn try_semantic_click(&mut self, node: serde_json::Value) -> Result<bool, BrowserError> {
        let resolved = match self.call("DOM.resolveNode", &node.to_string()) {
            Ok(body) => body,
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => return Ok(false),
            Err(other) => return Err(other),
        };
        let object_id = extract::object_id(&resolved)?;
        let params = json!({
            "functionDeclaration": DOM_CLICK_FUNCTION,
            "objectId": object_id,
            "returnByValue": true
        })
        .to_string();
        match self.call("Runtime.callFunctionOn", &params) {
            Ok(body) => Ok(!extract::call_threw(&body)?),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(false),
            Err(other) => Err(other),
        }
    }

    fn coordinate_click(&mut self, x: f64, y: f64) -> Result<(), BrowserError> {
        for kind in ["mousePressed", "mouseReleased"] {
            let params = json!({
                "type": kind,
                "x": x,
                "y": y,
                "button": "left",
                "clickCount": 1
            })
            .to_string();
            self.call("Input.dispatchMouseEvent", &params)?;
        }
        Ok(())
    }

    fn call(&mut self, method: &str, params_json: &str) -> Result<String, BrowserError> {
        self.transport
            .call(method, params_json)
            .map_err(BrowserError::from)
    }
}

fn owns_hit(hit: i64, target: i64, parent_of: &BTreeMap<i64, i64>) -> bool {
    let mut current = hit;
    loop {
        if current == target {
            return true;
        }
        match parent_of.get(&current) {
            Some(&parent) => current = parent,
            None => return false,
        }
    }
}

fn focused_region(
    ax_nodes: &[AxElement],
    bindings: &BTreeMap<RegionId, NodeBinding>,
) -> Option<RegionId> {
    let backends: Vec<i64> = ax_nodes
        .iter()
        .filter(|element| element.focused)
        .filter_map(|element| element.backend_dom_node_id)
        .collect();
    if backends.is_empty() {
        return None;
    }
    bindings
        .iter()
        .filter(|(_, binding)| {
            binding
                .backend_node_id
                .is_some_and(|id| backends.contains(&id))
        })
        .map(|(id, _)| id.clone())
        .min()
}
