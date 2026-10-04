//! Browser session. Observation and press go through [`CdpTransport`], whether
//! that transport is a replay script or a live CDP websocket.
//!
//! Snapshots use `captured_at_ms = 0`. The ranker must not see a local clock.
//!
//! Press preference, and only for [`Action::Click`]:
//! 1. DOM semantic click (`DOM.resolveNode` + `Runtime.callFunctionOn`)
//! 2. CDP element action (`DOM.focus`)
//! 3. coordinate click (`Input.dispatchMouseEvent`)
//!
//! A CDP `error` result fails that tier and the next tier runs. A missing
//! script entry is not a tier failure: it is [`CdpError::NoScriptedResponse`]
//! and the press stops, so a short fixture cannot silently become a click.

use std::collections::BTreeMap;

use serde_json::json;

use hyper_use_core::{Action, InteractionManifold, RegionId};

use crate::error::{ActMechanism, BrowserError, CdpError};
use crate::extract::{self, content_rect};
use crate::fusion::{self, NodeBinding, RawNode};
use crate::transport::CdpTransport;
use crate::verify::{self, Expectation};

pub const DOM_CLICK_FUNCTION: &str = "function(){this.click()}";

pub struct BrowserSession<T: CdpTransport> {
    transport: T,
    manifold: Option<InteractionManifold>,
    bindings: BTreeMap<RegionId, NodeBinding>,
}

impl<T: CdpTransport> BrowserSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            manifold: None,
            bindings: BTreeMap::new(),
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

    pub fn observe(&mut self) -> Result<&InteractionManifold, BrowserError> {
        let layout = self.call("Page.getLayoutMetrics", &json!({}).to_string())?;
        let viewport = extract::parse_viewport(&layout)?;
        let document = self.call(
            "DOM.getDocument",
            &json!({"depth": -1, "pierce": false}).to_string(),
        )?;
        let elements = extract::dom_elements(&document)?;
        let ax_tree = self.call("Accessibility.getFullAXTree", &json!({}).to_string())?;
        let ax_nodes = extract::ax_elements(&ax_tree)?;

        let mut dom_raw = Vec::new();
        for element in &elements {
            let boxed = self.call(
                "DOM.getBoxModel",
                &json!({"nodeId": element.node_id}).to_string(),
            )?;
            if let Some(rect) = content_rect(&boxed)? {
                dom_raw.push(RawNode::from_dom(element, rect));
            }
        }
        let mut ax_raw = Vec::new();
        for element in &ax_nodes {
            let Some(backend) = element.backend_dom_node_id else {
                continue;
            };
            let boxed = self.call(
                "DOM.getBoxModel",
                &json!({"backendNodeId": backend}).to_string(),
            )?;
            if let Some(rect) = content_rect(&boxed)? {
                ax_raw.push(RawNode::from_ax(element, rect));
            }
        }
        let (manifold, bindings) = fusion::fuse(viewport, &dom_raw, &ax_raw)?;
        self.bindings = bindings;
        self.manifold = Some(manifold);
        Ok(self.manifold.as_ref().expect("observation just stored"))
    }

    /// Click `id`. Other actions are refused. The session must already have
    /// been observed, or this returns [`BrowserError::NotObserved`].
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
        if let Some(node_id) = binding.dom_node_id {
            if self.try_dom_semantic(node_id)? {
                return Ok(ActMechanism::DomSemantic);
            }
            if self.try_dom_focus(node_id)? {
                return Ok(ActMechanism::CdpElement);
            }
        } else if let Some(backend) = binding.backend_node_id {
            if self.try_backend_semantic(backend)? {
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

    fn try_dom_semantic(&mut self, node_id: i64) -> Result<bool, BrowserError> {
        let resolved = match self.call("DOM.resolveNode", &json!({"nodeId": node_id}).to_string()) {
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
            Ok(_) => Ok(true),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(false),
            Err(other) => Err(other),
        }
    }

    fn try_dom_focus(&mut self, node_id: i64) -> Result<bool, BrowserError> {
        match self.call("DOM.focus", &json!({"nodeId": node_id}).to_string()) {
            Ok(_) => Ok(true),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(false),
            Err(other) => Err(other),
        }
    }

    fn try_backend_semantic(&mut self, backend: i64) -> Result<bool, BrowserError> {
        let resolved = match self.call(
            "DOM.resolveNode",
            &json!({"backendNodeId": backend}).to_string(),
        ) {
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
            Ok(_) => Ok(true),
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
