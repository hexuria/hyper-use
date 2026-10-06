//! Compact snapshot: one `Runtime.evaluate` collects the per-element
//! evidence `observe` otherwise gathers node by node (`DOM.getBoxModel`,
//! `CSS.getComputedStyleForNode`, `DOM.getNodeForLocation`).
//!
//! [`COMPACT_JS`] walks the same flattened tree `DOM.getDocument`
//! (`pierce: true`) exposes — element children, then open shadow roots,
//! then same-origin `contentDocument` — tags every element with a
//! `data-hu-k` attribute, and returns, per tag: the content-box rect in
//! top-viewport coordinates, the computed-style pairs
//! [`crate::stacking::style_from_computed`] reads, and the hit-test result
//! (`elementFromPoint`, recursing into same-origin iframes the way CDP hit
//! tests descend). `observe` then runs `DOM.getDocument`, which sees the
//! injected attributes and joins each record to its `backendNodeId`.
//!
//! Elements the page cannot show to a same-origin walk — cross-origin
//! iframe content, closed shadow roots — carry no `data-hu-k`; `observe`
//! falls back to the per-node calls for exactly those. A replay script
//! that does not script the eval step falls back entirely, which keeps
//! every pre-compact fixture working unchanged.
//!
//! Semantics preserved from the per-node calls: `display:none` (no client
//! rects) omits a node like a `getBoxModel` error; the rect is the content
//! box (border box minus computed border + padding) in top-viewport
//! coordinates; a hit that lands on an iframe descends into it when the
//! iframe is same-origin.

use std::collections::BTreeMap;

use serde_json::Value;

use hyper_use_core::Rect;

use crate::error::{BrowserError, CdpError};

/// Attribute the walk injects. Read back off `DOM.getDocument` nodes.
pub(crate) const HU_K_ATTR: &str = "data-hu-k";

/// The single-eval walk. Returns `{nodes: {"<k>": {r,s,h}}}` where `r` is
/// `[x,y,w,h]` or null, `s` the computed-style name/value pairs
/// `style_from_computed` consumes, and `h` the `data-hu-k` of the element
/// under the content center (or null when nothing is there).
pub(crate) const COMPACT_JS: &str = r#"(function(){
var ATTR='data-hu-k';
var PROPS=['z-index','position','opacity','transform','filter','isolation','mix-blend-mode','will-change','pointer-events'];
var nodes={};
var next=0;
function px(v){var n=parseFloat(v);return isFinite(n)?n:0;}
// Offset of a document's origin in top-viewport coordinates, via the
// frameElement chain (same-origin only).
function docOffset(doc){
  var x=0,y=0;
  try{
    var w=doc.defaultView;
    var f=w&&w!==w.top?w.frameElement:null;
    while(f){
      var r=f.getBoundingClientRect();
      var cs=f.ownerDocument.defaultView.getComputedStyle(f);
      x+=r.left+px(cs.borderLeftWidth)+px(cs.paddingLeft);
      y+=r.top+px(cs.borderTopWidth)+px(cs.paddingTop);
      var w2=f.ownerDocument.defaultView;
      f=w2&&w2!==w2.top?w2.frameElement:null;
    }
  }catch(e){}
  return[x,y];
}
// Content box in top-viewport coordinates, or null when the element has
// no client rects (display:none subtree) — the same omission a
// getBoxModel protocol error causes.
function contentRect(el,off){
  if(!el.getClientRects||el.getClientRects().length===0)return null;
  var r=el.getBoundingClientRect();
  var cs=el.ownerDocument.defaultView.getComputedStyle(el);
  var x=r.left+px(cs.borderLeftWidth)+px(cs.paddingLeft)+off[0];
  var y=r.top+px(cs.borderTopWidth)+px(cs.paddingTop)+off[1];
  var w=r.width-px(cs.borderLeftWidth)-px(cs.borderRightWidth)-px(cs.paddingLeft)-px(cs.paddingRight);
  var h=r.height-px(cs.borderTopWidth)-px(cs.borderBottomWidth)-px(cs.paddingTop)-px(cs.paddingBottom);
  if(!(w>0)||!(h>0))return null;
  return[x,y,w,h];
}
function stylePairs(el){
  var cs=el.ownerDocument.defaultView.getComputedStyle(el);
  var out=[];
  for(var i=0;i<PROPS.length;i++)out.push([PROPS[i],cs.getPropertyValue(PROPS[i])]);
  return out;
}
// Nearest tagged ancestor-or-self of a hit node, climbing out of shadow
// roots and (same-origin) document boundaries.
function hitKey(node){
  var n=node;
  while(n){
    if(n.nodeType===1&&n.hasAttribute&&n.hasAttribute(ATTR))return parseInt(n.getAttribute(ATTR),10);
    if(n.parentNode){n=n.parentNode;continue;}
    if(n.host){n=n.host;continue;}
    break;
  }
  return null;
}
// Top-down hit test: elementFromPoint in `doc`, descending into
// same-origin iframe hits the way CDP hit testing does.
function hitAt(doc,x,y){
  var el=null;
  try{el=doc.elementFromPoint(x,y);}catch(e){return null;}
  if(!el)return null;
  try{
    if(el.contentDocument){
      var o=docOffset(el.contentDocument);
      var inner=hitAt(el.contentDocument,x-o[0],y-o[1]);
      if(inner!==null)return inner;
    }
  }catch(e){}
  return hitKey(el);
}
function visit(el,doc,off){
  if(el.nodeType!==1)return;
  var k=next++;
  el.setAttribute(ATTR,String(k));
  var rec={r:null,s:[],h:null};
  var r=contentRect(el,off);
  if(r){
    rec.r=r;
    rec.s=stylePairs(el);
    var lx=r[0]-off[0]+r[2]/2;
    var ly=r[1]-off[1]+r[3]/2;
    rec.h=hitAt(doc,lx,ly);
  }
  nodes[k]=rec;
  var kids=el.children;
  if(kids)for(var i=0;i<kids.length;i++)visit(kids[i],doc,off);
  if(el.shadowRoot){
    var shadow=el.shadowRoot.children;
    if(shadow)for(var i=0;i<shadow.length;i++)visit(shadow[i],doc,off);
  }
  try{
    var cd=el.contentDocument;
    if(cd&&cd.children){
      var off2=docOffset(cd);
      for(var i=0;i<cd.children.length;i++)visit(cd.children[i],cd,off2);
    }
  }catch(e){}
}
(function(){
  var off=docOffset(document);
  var top=document.children;
  for(var i=0;i<top.length;i++)visit(top[i],document,off);
})();
return{nodes:nodes};
})()"#;

/// Per-element evidence from one [`COMPACT_JS`] run.
#[derive(Clone, Debug, Default)]
pub(crate) struct CompactNode {
    /// Content box in top-viewport coordinates; `None` = omit like a
    /// `getBoxModel` protocol error.
    pub rect: Option<Rect>,
    /// `CSS.getComputedStyleForNode` name/value pairs.
    pub style: Vec<(String, String)>,
    /// `data-hu-k` of the element under the content center.
    pub hit: Option<u32>,
}

/// One compact snapshot keyed by `data-hu-k`.
#[derive(Clone, Debug, Default)]
pub(crate) struct CompactSnapshot {
    pub nodes: BTreeMap<u32, CompactNode>,
}

impl CompactSnapshot {
    pub fn node(&self, k: u32) -> Option<&CompactNode> {
        self.nodes.get(&k)
    }
}

/// Parse the `Runtime.evaluate` result of [`COMPACT_JS`].
pub(crate) fn parse(eval_result_json: &str) -> Result<CompactSnapshot, BrowserError> {
    let value: Value = serde_json::from_str(eval_result_json).map_err(|err| CdpError::BadJson {
        message: err.to_string(),
    })?;
    let nodes = value
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("nodes"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            BrowserError::Cdp(CdpError::BadJson {
                message: "compact snapshot result has no nodes object".into(),
            })
        })?;
    let mut out = CompactSnapshot::default();
    for (k, record) in nodes {
        let k: u32 = k.parse().map_err(|_| CdpError::BadJson {
            message: format!("compact node key `{k}` is not a u32"),
        })?;
        let rect = record.get("r").and_then(Value::as_array).and_then(|r| {
            if r.len() < 4 {
                return None;
            }
            let x = r[0].as_f64()?;
            let y = r[1].as_f64()?;
            let w = r[2].as_f64()?;
            let h = r[3].as_f64()?;
            Rect::try_new(x, y, w, h).ok()
        });
        let style = record
            .get("s")
            .and_then(Value::as_array)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter_map(|pair| {
                        let pair = pair.as_array()?;
                        let name = pair.first()?.as_str()?;
                        let value = pair.get(1)?.as_str()?;
                        Some((name.to_owned(), value.to_owned()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let hit = record.get("h").and_then(Value::as_u64).map(|h| h as u32);
        out.nodes.insert(k, CompactNode { rect, style, hit });
    }
    Ok(out)
}
