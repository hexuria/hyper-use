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
//! After fusion, observe builds a stacking map from each kept node's computed
//! style (`CSS.getComputedStyleForNode`) and marks clickable regions whose
//! center sits under a higher-painting kept region. It then hit-tests each
//! clickable center with `DOM.getNodeForLocation`. When the node under the
//! center is not the region or a descendant of it (cookie banner, custom
//! backdrop, toast outside the kept set), the region is marked `occluded`.
//! Dialog front-layer logic in `aui-guard` still applies on top of that.
//! Old CDP fixtures without `CSS.enable` skip the stacking pass.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use aui_core::{Action, InteractionManifold, InteractionRegion, Rect, RegionId};

use crate::compact;
use aui_cdp::{CdpError, CdpTransport};

use crate::error::{ActMechanism, BrowserError};
use crate::extract::{self, content_rect, AxElement};
use crate::fusion::{self, NodeBinding, RawNode};
use crate::identity::IdentityMap;
use crate::page::PageState;
use crate::verify::{self, Expectation};

pub const DOM_CLICK_FUNCTION: &str = "function(){this.click()}";

/// Set the value of an editable element the way a user edit would, then fire
/// `input` + `change`. Throws (and so changes nothing) when the element is
/// disabled, readonly, or not editable. The text arrives as a CDP call
/// argument, never spliced into the function source.
pub const DOM_TYPE_FUNCTION: &str = "function(v){if(this.disabled)throw new Error('disabled');if(this.readOnly)throw new Error('readonly');if(typeof v!=='string')throw new Error('text must be a string');this.focus();if(this.isContentEditable){this.textContent=v;}else{var d=null;for(var p=Object.getPrototypeOf(this);p&&!d;p=Object.getPrototypeOf(p)){d=Object.getOwnPropertyDescriptor(p,'value');}if(!d||!d.set)throw new Error('not editable');d.set.call(this,v);}this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));return true;}";

/// Choose exactly one `<option>` of a `<select>` by value, label, or trimmed
/// text (exact first, then case-insensitive). Zero or several matches throw,
/// so nothing is selected. Fires `input` + `change`.
pub const DOM_SELECT_FUNCTION: &str = "function(v){if(this.disabled)throw new Error('disabled');if(this.tagName!=='SELECT')throw new Error('not a select');var o=Array.prototype.slice.call(this.options);var m=o.filter(function(x){return x.value===v||x.label===v||x.text.trim()===v;});if(m.length===0){var l=String(v).trim().toLowerCase();m=o.filter(function(x){return x.value.toLowerCase()===l||x.label.trim().toLowerCase()===l||x.text.trim().toLowerCase()===l;});}if(m.length!==1)throw new Error('option matches: '+m.length);if(m[0].disabled)throw new Error('option disabled');this.value=m[0].value;this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));return m[0].value;}";

/// Read `[value, selectedOptionText]` of a field; `null` for non-fields.
pub const DOM_READ_VALUE_FUNCTION: &str = "function(){var t='';if(this.tagName==='SELECT'&&this.selectedIndex>=0){t=this.options[this.selectedIndex].text.trim();}if(this.isContentEditable){return [this.textContent,t];}if(this.value===undefined||this.value===null){return null;}return [String(this.value),t];}";

/// Page scroll direction for [`BrowserSession::scroll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Fraction of the viewport height one page scroll moves.
pub const SCROLL_VIEWPORT_FRACTION: f64 = 0.8;

const AUTOCOMPLETE_OPTIONS_SIGNATURE_FUNCTION: &str = r#"function(){if(document.readyState!=='complete')return null;const root=this.getRootNode?this.getRootNode():document;const byId=id=>(root.getElementById?root.getElementById(id):null)||document.getElementById(id);const ids=((this.getAttribute('aria-controls')||'')+' '+(this.getAttribute('aria-owns')||'')).split(/\s+/).filter(Boolean);const owned=ids.map(byId).filter(Boolean);const pool=owned.length?owned.flatMap(el=>[...(el.matches('[role=option]')?[el]:[]),...el.querySelectorAll('[role=option]')]):Array.from(document.querySelectorAll('[role=option]'));const options=pool.filter(el=>el.getClientRects().length>0&&getComputedStyle(el).visibility!=='hidden');return(owned.length?'o':'d')+options.length+':'+options.slice(0,20).map(el=>(el.textContent||'').trim().slice(0,80)).join('\u001f');}"#;

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

    /// One `Runtime.evaluate` running the compact walk (`compact.rs`):
    /// tags every reachable element `data-hu-k` and returns its rect,
    /// computed style, and hit-test result in one reply. `None` when the
    /// transport cannot run it (a replay script that never scripted the
    /// step, a protocol error, or a walk that threw — e.g. a page that
    /// replaced DOM globals), which leaves the per-node fallback.
    fn compact_eval(&mut self) -> Result<Option<compact::CompactSnapshot>, BrowserError> {
        let params = json!({
            "expression": compact::COMPACT_JS,
            "returnByValue": true
        })
        .to_string();
        match self.call("Runtime.evaluate", &params) {
            Ok(body) => {
                if extract::call_threw(&body)? {
                    return Ok(None);
                }
                Ok(Some(compact::parse(&body)?))
            }
            Err(BrowserError::Cdp(CdpError::NoScriptedResponse { .. }))
            | Err(BrowserError::Cdp(CdpError::Protocol { .. })) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Fuse DOM + accessibility into a fresh manifold.
    ///
    /// Cost: one `Runtime.evaluate` running the compact walk plus four
    /// constant calls (`Page.getLayoutMetrics`, `DOM.getDocument`,
    /// `Accessibility.getFullAXTree`, `Page.getNavigationHistory`),
    /// independent of page size. Elements the walk could not tag —
    /// cross-origin iframe content, closed shadow roots, or every element
    /// when the eval is unavailable — keep the per-node fallback
    /// (`DOM.getBoxModel`, `CSS.getComputedStyleForNode`,
    /// `DOM.getNodeForLocation`). Replay fixtures that never scripted the
    /// eval take the per-node path throughout.
    pub fn observe(&mut self) -> Result<&InteractionManifold, BrowserError> {
        // The compact eval must run before `DOM.getDocument` so the injected
        // `data-hu-k` attributes arrive inside the document tree.
        let compact = self.compact_eval()?;
        let layout = self.call("Page.getLayoutMetrics", &json!({}).to_string())?;
        let viewport = extract::parse_viewport(&layout)?;
        let document = self.call(
            "DOM.getDocument",
            &json!({"depth": -1, "pierce": true}).to_string(),
        )?;
        let dom = extract::dom_document(&document)?;
        let ax_tree = self.call("Accessibility.getFullAXTree", &json!({}).to_string())?;
        let ax_nodes = extract::ax_elements(&ax_tree)?;

        let mut dom_raw = Vec::new();
        for element in &dom.elements {
            let rect = match element
                .hu_k
                .and_then(|k| compact.as_ref().and_then(|c| c.node(k)))
            {
                // Tagged: the blob is authoritative (`None` = omit, like a
                // getBoxModel protocol error).
                Some(node) => node.rect,
                // Untagged (cross-origin iframe content, closed shadow,
                // or no compact eval): per-node fallback.
                None => {
                    let params = json!({"nodeId": element.node_id}).to_string();
                    self.box_rect(&params)?
                }
            };
            if let Some(rect) = rect {
                dom_raw.push(RawNode::from_dom(element, rect));
            }
        }
        let mut ax_raw = Vec::new();
        for element in &ax_nodes {
            let Some(backend) = element.backend_dom_node_id else {
                continue;
            };
            let rect = match dom
                .hu_k_of_backend
                .get(&backend)
                .and_then(|k| compact.as_ref().and_then(|c| c.node(*k)))
            {
                Some(node) => node.rect,
                None => {
                    let params = json!({"backendNodeId": backend}).to_string();
                    self.box_rect(&params)?
                }
            };
            if let Some(rect) = rect {
                ax_raw.push(RawNode::from_ax(element, rect));
            }
        }
        let (fused, fused_bindings) = fusion::fuse(viewport, &dom_raw, &ax_raw)?;
        let (mut manifold, bindings) =
            self.identity
                .assign(self.manifold.as_ref(), fused, fused_bindings)?;
        let focused = focused_region(&ax_nodes, &bindings);
        let page = self.read_page(focused)?;
        // Document order among kept DOM elements (walk order). Used as the
        // paint-order tiebreak when z-index ties.
        let mut dom_order = BTreeMap::new();
        for (index, element) in dom.elements.iter().enumerate() {
            dom_order.insert(element.backend_node_id, index as u32);
        }
        // Stacking runs before hit-test. Fixtures that never scripted CSS
        // skip it (`NoScriptedResponse` on CSS.enable) and keep hit-test only.
        self.apply_stacking_occlusion(
            &mut manifold,
            &bindings,
            &dom_order,
            compact.as_ref(),
            &dom,
        )?;
        // Hit-tests run after history so scripted CDP fixtures can append
        // `DOM.getNodeForLocation` after `Page.getNavigationHistory`.
        self.apply_hit_test_occlusion(&mut manifold, &bindings, &dom, compact.as_ref())?;
        attach_element_state(&mut manifold, &bindings, &dom, compact.as_ref());
        self.bindings = bindings;
        self.page = Some(page);
        self.manifold = Some(manifold);
        self.stale = false;
        Ok(self.manifold.as_ref().expect("observation just stored"))
    }

    /// Mark clickable regions buried under a higher-painting kept region.
    ///
    /// Uses `CSS.enable` + `CSS.getComputedStyleForNode`. When the next
    /// scripted CDP step is not `CSS.enable` (older fixtures), this returns
    /// without changing the manifold so hit-test still runs.
    fn apply_stacking_occlusion(
        &mut self,
        manifold: &mut InteractionManifold,
        bindings: &BTreeMap<RegionId, NodeBinding>,
        dom_order: &BTreeMap<i64, u32>,
        compact: Option<&compact::CompactSnapshot>,
        dom: &extract::DomDocument,
    ) -> Result<(), BrowserError> {
        let mut styles = BTreeMap::new();
        // Stable RegionId order so ScriptBuilder can emit matching CSS calls.
        let targets: Vec<(RegionId, i64, Option<i64>)> = manifold
            .regions()
            .filter_map(|region| {
                let binding = bindings.get(region.id())?;
                let node_id = binding.dom_node_id?;
                Some((region.id().clone(), node_id, binding.backend_node_id))
            })
            .collect();
        // Compact evidence covers the tagged nodes; only untagged targets
        // still need `CSS.getComputedStyleForNode`.
        let mut pending = Vec::new();
        for (id, node_id, backend) in targets {
            let order = backend
                .and_then(|b| dom_order.get(&b).copied())
                .unwrap_or(u32::MAX);
            let covered = backend
                .and_then(|b| dom.hu_k_of_backend.get(&b))
                .and_then(|k| compact.and_then(|c| c.node(*k)));
            match covered.and_then(|node| node.style.as_ref()) {
                Some(style) => {
                    styles.insert(id, (crate::stacking::style_from_computed(style), order));
                }
                None => pending.push((id, node_id, order)),
            }
        }
        let hit_owned = compact_hit_owned(manifold, bindings, dom, compact);
        if pending.is_empty() && compact.is_some() {
            crate::stacking::apply_stacking_occlusion_except(manifold, &styles, &hit_owned);
            return Ok(());
        }
        match self.call("CSS.enable", &json!({}).to_string()) {
            Ok(_) => {}
            Err(BrowserError::Cdp(CdpError::NoScriptedResponse { .. })) => return Ok(()),
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => return Ok(()),
            Err(other) => return Err(other),
        }
        for (id, node_id, order) in pending {
            let params = json!({"nodeId": node_id}).to_string();
            let body = match self.call("CSS.getComputedStyleForNode", &params) {
                Ok(body) => body,
                Err(BrowserError::Cdp(CdpError::Protocol { .. })) => continue,
                Err(BrowserError::Cdp(CdpError::NoScriptedResponse { .. })) => {
                    // Partial scripts: stop stacking; already-fetched styles still apply.
                    break;
                }
                Err(other) => return Err(other),
            };
            let pairs = extract::computed_style_pairs(&body)?;
            let style = crate::stacking::style_from_computed(&pairs);
            styles.insert(id, (style, order));
        }
        crate::stacking::apply_stacking_occlusion_except(manifold, &styles, &hit_owned);
        Ok(())
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
        dom: &extract::DomDocument,
        compact: Option<&compact::CompactSnapshot>,
    ) -> Result<(), BrowserError> {
        let parent_of = &dom.parent_of;
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
            let evidence = dom
                .hu_k_of_backend
                .get(&backend)
                .and_then(|k| compact.and_then(|c| c.node(*k)))
                .map_or(compact::Hit::NotCollected, |node| node.hit);
            let resolved = match evidence {
                compact::Hit::Nothing => Some(None),
                // The blob's hit k resolves through the same document's
                // `data-hu-k` attributes; an ambiguous or unknown k falls back.
                compact::Hit::Key(k) => dom.backend_of_hu_k.get(&k).map(|b| Some(*b)),
                compact::Hit::NotCollected => None,
            };
            let hit = match resolved {
                Some(hit) => hit,
                // Untagged, not collected, or unresolvable: per-node fallback.
                None => {
                    let params = json!({"x": x.round() as i64, "y": y.round() as i64}).to_string();
                    let body = match self.call("DOM.getNodeForLocation", &params) {
                        Ok(body) => body,
                        Err(BrowserError::Cdp(CdpError::Protocol { .. })) => continue,
                        Err(other) => return Err(other),
                    };
                    extract::location_backend(&body)?
                }
            };
            let Some(hit) = hit else {
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
    /// **Not the product path.** Ultra-Instinct is an action firewall: hosts click
    /// after [`aui_guard::guard`] returns Allow. Do not call this from
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
        for node in binding
            .dom_node_id
            .map(|_| NodeBinding {
                backend_node_id: None,
                ..binding.clone()
            })
            .into_iter()
            .chain(binding.backend_node_id.map(|_| NodeBinding {
                dom_node_id: None,
                ..binding.clone()
            }))
        {
            if self.try_semantic_click(&node)? {
                return Ok(ActMechanism::DomSemantic);
            }
        }
        self.coordinate_click(binding.center_x, binding.center_y)?;
        Ok(ActMechanism::Coordinate)
    }

    /// Click the observed region `id` with a trusted pointer event at its
    /// center (`Input.dispatchMouseEvent`), skipping the semantic tier.
    ///
    /// Some controls ignore script-dispatched clicks (a video player's skip
    /// button checks `isTrusted`). The agent uses this only after a semantic
    /// click on the same target verified no effect, and only through the
    /// executor, after the gate has checked that nothing covers the center.
    #[doc(hidden)]
    pub fn pointer_click(&mut self, id: &RegionId) -> Result<ActMechanism, BrowserError> {
        let binding = self.binding_for(id)?;
        self.stale = true;
        let move_params = json!({
            "type": "mouseMoved",
            "x": binding.center_x,
            "y": binding.center_y
        })
        .to_string();
        self.call("Input.dispatchMouseEvent", &move_params)?;
        self.coordinate_click(binding.center_x, binding.center_y)?;
        Ok(ActMechanism::Coordinate)
    }

    /// Set `text` as the value of the observed editable region `id`.
    ///
    /// Low-level input used by the agent executor **after** ticket
    /// revalidation (`aui-agent`). There is no coordinate tier: when
    /// the observed node cannot be resolved, or the page rejects the edit
    /// (disabled / readonly / not editable), nothing is typed and an error
    /// returns. Fail closed rather than typing into whatever has focus.
    #[doc(hidden)]
    pub fn type_text(&mut self, id: &RegionId, text: &str) -> Result<ActMechanism, BrowserError> {
        let binding = self.binding_for(id)?;
        self.stale = true;
        self.semantic_input(&binding, DOM_TYPE_FUNCTION, text)?;
        Ok(ActMechanism::DomSemantic)
    }

    /// Select exactly one option of the observed `<select>` region `id`.
    ///
    /// Same boundary and fail-closed rules as [`Self::type_text`]. Zero or
    /// several matching options select nothing.
    #[doc(hidden)]
    pub fn select_option(
        &mut self,
        id: &RegionId,
        option: &str,
    ) -> Result<ActMechanism, BrowserError> {
        let binding = self.binding_for(id)?;
        self.stale = true;
        self.semantic_input(&binding, DOM_SELECT_FUNCTION, option)?;
        Ok(ActMechanism::DomSemantic)
    }

    /// Scroll the page by [`SCROLL_VIEWPORT_FRACTION`] of the viewport with a
    /// trusted wheel event at the viewport center. Page-level primitive: no
    /// target, no model-supplied coordinates.
    pub fn scroll(&mut self, direction: ScrollDirection) -> Result<(), BrowserError> {
        let viewport = self
            .manifold
            .as_ref()
            .ok_or(BrowserError::NotObserved)?
            .viewport();
        let height = viewport.height();
        let delta = (height * SCROLL_VIEWPORT_FRACTION).round();
        let delta_y = match direction {
            ScrollDirection::Up => -delta,
            ScrollDirection::Down => delta,
        };
        self.stale = true;
        let params = json!({
            "type": "mouseWheel",
            "x": (viewport.width() / 2.0).round(),
            "y": (height / 2.0).round(),
            "deltaX": 0,
            "deltaY": delta_y
        })
        .to_string();
        self.call("Input.dispatchMouseEvent", &params)?;
        Ok(())
    }

    /// Navigate the page. The stored observation becomes stale. Callers
    /// observe again before deciding (the agent loop always does).
    pub fn navigate(&mut self, url: &str) -> Result<(), BrowserError> {
        self.stale = true;
        let body = self.call("Page.navigate", &json!({"url": url}).to_string())?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(err) = value.get("errorText").and_then(serde_json::Value::as_str) {
                return Err(BrowserError::Navigation(err.to_owned()));
            }
        }
        Ok(())
    }

    /// Ask the page for `document.readyState`. `None` when CDP cannot answer.
    pub fn ready_state(&mut self) -> Result<Option<String>, BrowserError> {
        let params =
            json!({"expression": "document.readyState", "returnByValue": true}).to_string();
        let body = match self.call("Runtime.evaluate", &params) {
            Ok(body) => body,
            Err(BrowserError::Cdp(CdpError::Protocol { .. })) => return Ok(None),
            Err(other) => return Err(other),
        };
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
                message: err.to_string(),
            })?;
        Ok(value
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned))
    }

    /// `document.documentElement.outerHTML`. The host surface's get_html.
    pub fn outer_html(&mut self) -> Result<String, BrowserError> {
        self.eval_string("document.documentElement.outerHTML")
    }

    /// `document.body.innerText` — visible page text, no markup.
    pub fn page_text(&mut self) -> Result<String, BrowserError> {
        self.eval_string("document.body ? document.body.innerText : ''")
    }

    /// PNG screenshot of the viewport, base64 (`Page.captureScreenshot`).
    pub fn screenshot_png(&mut self) -> Result<String, BrowserError> {
        let body = self.call(
            "Page.captureScreenshot",
            &json!({"format": "png"}).to_string(),
        )?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
                message: err.to_string(),
            })?;
        value
            .get("data")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                BrowserError::Cdp(CdpError::Protocol {
                    message: "captureScreenshot returned no data".into(),
                })
            })
    }

    /// Back one navigation-history entry. Errors when there is no entry
    /// behind the current one. The stored observation becomes stale.
    pub fn go_back(&mut self) -> Result<(), BrowserError> {
        let body = self.call("Page.getNavigationHistory", "{}")?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
                message: err.to_string(),
            })?;
        let current = value
            .get("currentIndex")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let entries = value
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let previous = entries
            .iter()
            .find(|entry| entry.get("id").and_then(serde_json::Value::as_i64) == Some(current - 1))
            .or_else(|| {
                entries.iter().rfind(|entry| {
                    entry
                        .get("id")
                        .and_then(serde_json::Value::as_i64)
                        .is_some_and(|id| id < current)
                })
            })
            .ok_or_else(|| BrowserError::Navigation("no earlier history entry".into()))?;
        let entry_id = previous
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(current - 1);
        self.stale = true;
        self.call(
            "Page.navigateToHistoryEntry",
            &json!({"entryId": entry_id}).to_string(),
        )?;
        Ok(())
    }

    /// Evaluate JavaScript `expression` in the page (`Runtime.evaluate`,
    /// `returnByValue` + `awaitPromise`). Returns the result value; a thrown
    /// exception is an error. Marks the observation stale — arbitrary script
    /// may mutate the page. The host surface's javascript_exec.
    pub fn evaluate(&mut self, expression: &str) -> Result<serde_json::Value, BrowserError> {
        let params = json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true
        })
        .to_string();
        let body = self.call("Runtime.evaluate", &params)?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
                message: err.to_string(),
            })?;
        if value.get("exceptionDetails").is_some() {
            return Err(BrowserError::InputRejected(thrown_message(&body)));
        }
        self.stale = true;
        Ok(value
            .get("result")
            .and_then(|r| r.get("value").cloned())
            .unwrap_or(serde_json::Value::Null))
    }

    /// Scroll `pages` viewport heights (0.5 = half page, 10 ≈ to the end)
    /// down or up. The host surface's scroll tool.
    pub fn scroll_pages(&mut self, down: bool, pages: f64) -> Result<(), BrowserError> {
        let viewport = self
            .manifold
            .as_ref()
            .ok_or(BrowserError::NotObserved)?
            .viewport();
        let height = viewport.height();
        let delta = (height * SCROLL_VIEWPORT_FRACTION * pages.max(0.05)).round();
        let delta_y = if down { delta } else { -delta };
        self.stale = true;
        let params = json!({
            "type": "mouseWheel",
            "x": (viewport.width() / 2.0).round(),
            "y": (height / 2.0).round(),
            "deltaX": 0,
            "deltaY": delta_y
        })
        .to_string();
        self.call("Input.dispatchMouseEvent", &params)?;
        Ok(())
    }

    fn eval_string(&mut self, expression: &str) -> Result<String, BrowserError> {
        let params = json!({"expression": expression, "returnByValue": true}).to_string();
        let body = self.call("Runtime.evaluate", &params)?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
                message: err.to_string(),
            })?;
        if value.get("exceptionDetails").is_some() {
            return Err(BrowserError::InputRejected(thrown_message(&body)));
        }
        Ok(value
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned())
    }

    /// Format `<o|d><count>:<texts joined by U+001F>`; `o` is the owned popup, `d` document-wide.
    pub fn autocomplete_options_signature(
        &mut self,
        id: &RegionId,
    ) -> Result<Option<String>, BrowserError> {
        let Some(binding) = self.binding_for(id).ok() else {
            return Ok(None);
        };
        match self.call_function_on_node(&binding, AUTOCOMPLETE_OPTIONS_SIGNATURE_FUNCTION) {
            Ok(value) => Ok(value.and_then(|value| value.as_str().map(str::to_owned))),
            Err(BrowserError::TargetUnresolved | BrowserError::Cdp(CdpError::Protocol { .. })) => {
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }

    /// Read `(value, selected text)` without mutation; `None` if absent.
    pub fn field_value(&mut self, id: &RegionId) -> Result<Option<(String, String)>, BrowserError> {
        let binding = self.binding_for(id)?;
        let Some(value) = self.call_function_on_node(&binding, DOM_READ_VALUE_FUNCTION)? else {
            return Ok(None);
        };
        let Some(pair) = value.as_array() else {
            return Ok(None);
        };
        let get = |i: usize| {
            pair.get(i)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Some((get(0), get(1))))
    }

    fn binding_for(&self, id: &RegionId) -> Result<NodeBinding, BrowserError> {
        if self.manifold.is_none() {
            return Err(BrowserError::NotObserved);
        }
        self.bindings
            .get(id)
            .cloned()
            .ok_or_else(|| BrowserError::UnknownRegion(id.to_string()))
    }

    fn call_function_on_node(
        &mut self,
        binding: &NodeBinding,
        function: &str,
    ) -> Result<Option<Value>, BrowserError> {
        let node = match (binding.dom_node_id, binding.backend_node_id) {
            (Some(node_id), _) => json!({"nodeId": node_id}),
            (None, Some(backend_node_id)) => json!({"backendNodeId": backend_node_id}),
            (None, None) => return Err(BrowserError::TargetUnresolved),
        };
        let object_id = extract::object_id(&self.call("DOM.resolveNode", &node.to_string())?)?;
        let params =
            json!({"functionDeclaration":function,"objectId":object_id,"returnByValue":true})
                .to_string();
        let body = self.call("Runtime.callFunctionOn", &params)?;
        if extract::call_threw(&body)? {
            return Ok(None);
        }
        let value: Value = serde_json::from_str(&body).map_err(|err| CdpError::BadJson {
            message: err.to_string(),
        })?;
        Ok(Some(value["result"]["value"].clone()))
    }

    /// Resolve the bound node (node id, then backend id) and call `function`
    /// with one string argument. A thrown function is a page refusal and
    /// stops immediately; only an unresolvable node moves to the next tier.
    fn semantic_input(
        &mut self,
        binding: &NodeBinding,
        function: &str,
        value: &str,
    ) -> Result<(), BrowserError> {
        for node in binding
            .dom_node_id
            .map(|node_id| json!({"nodeId": node_id}))
            .into_iter()
            .chain(
                binding
                    .backend_node_id
                    .map(|backend| json!({"backendNodeId": backend})),
            )
        {
            let resolved = match self.call("DOM.resolveNode", &node.to_string()) {
                Ok(body) => body,
                Err(BrowserError::Cdp(CdpError::Protocol { .. })) => continue,
                Err(other) => return Err(other),
            };
            let object_id = extract::object_id(&resolved)?;
            let params = json!({
                "functionDeclaration": function,
                "objectId": object_id,
                "arguments": [{"value": value}],
                "returnByValue": true
            })
            .to_string();
            let body = self.call("Runtime.callFunctionOn", &params)?;
            if extract::call_threw(&body)? {
                return Err(BrowserError::InputRejected(thrown_message(&body)));
            }
            return Ok(());
        }
        Err(BrowserError::TargetUnresolved)
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

    fn try_semantic_click(&mut self, binding: &NodeBinding) -> Result<bool, BrowserError> {
        match self.call_function_on_node(binding, DOM_CLICK_FUNCTION) {
            Ok(value) => Ok(value.is_some()),
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

/// Best-effort message of a thrown `Runtime.callFunctionOn`.
fn thrown_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return "page threw".to_owned();
    };
    let details = value.get("exceptionDetails");
    details
        .and_then(|d| d.get("exception"))
        .and_then(|e| e.get("description"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            details
                .and_then(|d| d.get("text"))
                .and_then(serde_json::Value::as_str)
        })
        .map(|m| m.lines().next().unwrap_or(m).to_owned())
        .unwrap_or_else(|| "page threw".to_owned())
}

/// Regions whose compact hit test (no extra CDP calls) landed on the region
/// or a descendant. Not-collected or unresolvable hits are not evidence.
fn compact_hit_owned(
    manifold: &InteractionManifold,
    bindings: &BTreeMap<RegionId, NodeBinding>,
    dom: &extract::DomDocument,
    compact: Option<&compact::CompactSnapshot>,
) -> BTreeSet<RegionId> {
    let Some(compact) = compact else {
        return BTreeSet::new();
    };
    manifold
        .regions()
        .filter(|region| region.actions().contains(&Action::Click))
        .filter(|region| {
            let Some(backend) = bindings.get(region.id()).and_then(|b| b.backend_node_id) else {
                return false;
            };
            let hit = dom
                .hu_k_of_backend
                .get(&backend)
                .and_then(|k| compact.node(*k))
                .map(|node| node.hit);
            match hit {
                Some(compact::Hit::Key(k)) => dom
                    .backend_of_hu_k
                    .get(&k)
                    .is_some_and(|hit| owns_hit(*hit, backend, &dom.parent_of)),
                _ => false,
            }
        })
        .map(|region| region.id().clone())
        .collect()
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

fn attach_element_state(
    manifold: &mut InteractionManifold,
    bindings: &BTreeMap<RegionId, NodeBinding>,
    dom: &extract::DomDocument,
    compact: Option<&compact::CompactSnapshot>,
) {
    let Some(compact) = compact else {
        return;
    };
    let updated: Vec<_> = manifold
        .regions()
        .filter_map(|region| {
            let backend = bindings.get(region.id())?.backend_node_id?;
            let key = dom.hu_k_of_backend.get(&backend)?;
            let state = compact.node(*key)?.state.clone();
            (!state.is_empty()).then(|| region.clone().with_state(state))
        })
        .collect();
    for region in updated {
        manifold.replace(region);
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
