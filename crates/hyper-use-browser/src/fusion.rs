//! Fuse a DOM node and an accessibility node that describe one control.
//!
//! Version 1 merges when all of these hold:
//! - label Jaccard is at least [`MIN_LABEL_JACCARD`], or either label is empty
//! - roles are equal, or either role is [`Role::Generic`]
//! - intersection-over-union is at least [`MIN_IOU`], or centers are at most
//!   [`MAX_CENTROID_PX`] pixels apart
//!
//! Pairing is greedy: each DOM node takes the compatible accessibility node
//! with the highest IoU, then the smaller centroid distance, then the lower
//! index. Leftovers stay separate. The stored rectangle is the DOM rectangle
//! when both exist, so a one-pixel accessibility shift does not move the region.
//! The fused id is `n{backendNodeId}` from the DOM node. The session's identity
//! map may carry an earlier stable id over it. It is not derived
//! from the rectangle, the enabled bit, or the label.
//!
//! Accepted downside: two same-label, same-role controls whose centers are
//! within 8px can fuse even when IoU is low. A second accessibility node for
//! the same DOM node is left over; it is not deleted.

use std::collections::BTreeMap;

use hyper_use_core::{
    token_jaccard, InteractionManifold, InteractionRegion, Rect, RegionFlags, RegionId,
    RegionParts, Role, SourceMask, UnitInterval,
};
use hyper_use_geometry::is_fully_offscreen;

use crate::error::BrowserError;
use crate::extract::{AxElement, DomElement};

pub const MIN_LABEL_JACCARD: f64 = 0.5;
pub const MIN_IOU: f64 = 0.5;
pub const MAX_CENTROID_PX: f64 = 8.0;

#[derive(Clone, Debug)]
pub(crate) struct RawNode {
    pub dom_node_id: Option<i64>,
    pub backend_node_id: Option<i64>,
    pub role: Role,
    pub label: String,
    pub rect: Rect,
    pub actions: Vec<hyper_use_core::Action>,
    pub disabled: bool,
    pub hidden: bool,
    pub from_dom: bool,
    pub from_ax: bool,
}

impl RawNode {
    pub(crate) fn from_dom(element: &DomElement, rect: Rect) -> Self {
        Self {
            dom_node_id: Some(element.node_id),
            backend_node_id: Some(element.backend_node_id),
            role: element.role,
            label: element.label.clone(),
            rect,
            actions: element.actions.clone(),
            disabled: element.disabled,
            hidden: element.hidden,
            from_dom: true,
            from_ax: false,
        }
    }

    pub(crate) fn from_ax(element: &AxElement, rect: Rect) -> Self {
        Self {
            dom_node_id: None,
            backend_node_id: element.backend_dom_node_id,
            role: element.role,
            label: element.name.clone(),
            rect,
            actions: crate::extract::actions_for_role(element.role),
            disabled: element.disabled,
            hidden: false,
            from_dom: false,
            from_ax: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NodeBinding {
    pub dom_node_id: Option<i64>,
    pub backend_node_id: Option<i64>,
    pub center_x: f64,
    pub center_y: f64,
}

pub(crate) fn fuse(
    viewport: Rect,
    dom_nodes: &[RawNode],
    ax_nodes: &[RawNode],
) -> Result<(InteractionManifold, BTreeMap<RegionId, NodeBinding>), BrowserError> {
    let mut used = vec![false; ax_nodes.len()];
    let mut fused = Vec::new();
    for dom in dom_nodes {
        let mut best: Option<(usize, f64, f64)> = None;
        for (index, ax) in ax_nodes.iter().enumerate() {
            if used[index] || !compatible(dom, ax) {
                continue;
            }
            let overlap = iou(dom.rect, ax.rect);
            let distance = dom.rect.center().distance(ax.rect.center());
            let replace = match best {
                None => true,
                Some((_, best_iou, best_distance)) => {
                    overlap > best_iou
                        || ((overlap - best_iou).abs() < 1e-12 && distance < best_distance)
                }
            };
            if replace {
                best = Some((index, overlap, distance));
            }
        }
        if let Some((index, _, _)) = best {
            used[index] = true;
            fused.push(merge(dom, &ax_nodes[index]));
        } else {
            fused.push(dom.clone());
        }
    }
    for (index, ax) in ax_nodes.iter().enumerate() {
        if !used[index] {
            fused.push(ax.clone());
        }
    }

    let mut regions = Vec::new();
    let mut bindings = BTreeMap::new();
    for node in fused {
        let id = region_id(&node)?;
        let region = to_region(&id, &node, viewport)?;
        let center = region.rect().center();
        bindings.insert(
            id.clone(),
            NodeBinding {
                dom_node_id: node.dom_node_id,
                backend_node_id: node.backend_node_id,
                center_x: center.x(),
                center_y: center.y(),
            },
        );
        regions.push(region);
    }
    let manifold = InteractionManifold::try_new(viewport, regions, 0)
        .map_err(|err| BrowserError::DuplicateRegion(err.to_string()))?;
    Ok((manifold, bindings))
}

fn merge(dom: &RawNode, ax: &RawNode) -> RawNode {
    let label = if ax.label.is_empty() {
        dom.label.clone()
    } else {
        ax.label.clone()
    };
    let mut actions = dom.actions.clone();
    actions.extend(ax.actions.iter().copied());
    RawNode {
        dom_node_id: dom.dom_node_id,
        backend_node_id: dom.backend_node_id.or(ax.backend_node_id),
        role: if dom.role == Role::Generic {
            ax.role
        } else {
            dom.role
        },
        label,
        rect: dom.rect,
        actions,
        disabled: dom.disabled || ax.disabled,
        hidden: dom.hidden || ax.hidden,
        from_dom: true,
        from_ax: true,
    }
}

fn compatible(dom: &RawNode, ax: &RawNode) -> bool {
    labels_compatible(&dom.label, &ax.label)
        && roles_compatible(dom.role, ax.role)
        && geometry_compatible(dom.rect, ax.rect)
}

fn labels_compatible(left: &str, right: &str) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    token_jaccard(left, right) >= MIN_LABEL_JACCARD
}

fn roles_compatible(left: Role, right: Role) -> bool {
    left == right || left == Role::Generic || right == Role::Generic
}

fn geometry_compatible(left: Rect, right: Rect) -> bool {
    iou(left, right) >= MIN_IOU || left.center().distance(right.center()) <= MAX_CENTROID_PX
}

fn iou(left: Rect, right: Rect) -> f64 {
    let x1 = left.x().max(right.x());
    let y1 = left.y().max(right.y());
    let x2 = left.right().min(right.right());
    let y2 = left.bottom().min(right.bottom());
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = left.width() * left.height() + right.width() * right.height() - intersection;
    if union <= 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn region_id(node: &RawNode) -> Result<RegionId, BrowserError> {
    let raw = if node.from_dom {
        match node.backend_node_id {
            Some(id) => format!("n{id}"),
            None => match node.dom_node_id {
                Some(id) => format!("dom{id}"),
                None => "dom-unknown".to_owned(),
            },
        }
    } else {
        match node.backend_node_id {
            Some(id) => format!("ax{id}"),
            None => "ax-unknown".to_owned(),
        }
    };
    RegionId::try_new(raw).map_err(|err| BrowserError::DuplicateRegion(err.to_string()))
}

fn to_region(
    id: &RegionId,
    node: &RawNode,
    viewport: Rect,
) -> Result<InteractionRegion, BrowserError> {
    let mut sources = SourceMask::NONE;
    if node.from_dom {
        sources = sources.union(SourceMask::DOM);
    }
    if node.from_ax {
        sources = sources.union(SourceMask::ACCESSIBILITY);
    }
    let mut flags = RegionFlags::none();
    flags.set_disabled(node.disabled);
    flags.set_hidden(node.hidden);
    if is_fully_offscreen(node.rect, viewport) {
        flags.set_offscreen(true);
    }
    InteractionRegion::try_new(RegionParts {
        id: id.clone(),
        role: node.role,
        label: node.label.clone(),
        rect: node.rect,
        actions: node.actions.clone(),
        parent: None,
        sources,
        flags,
        temporal_stability: UnitInterval::ONE,
    })
    .map_err(|err| BrowserError::BadViewport(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_use_core::Action;

    fn rect(x: f64, y: f64) -> Rect {
        Rect::try_new(x, y, 80.0, 32.0).unwrap()
    }

    fn dom(label: &str, role: Role, x: f64) -> RawNode {
        RawNode {
            dom_node_id: Some(10),
            backend_node_id: Some(100),
            role,
            label: label.into(),
            rect: rect(x, 200.0),
            actions: vec![Action::Click],
            disabled: false,
            hidden: false,
            from_dom: true,
            from_ax: false,
        }
    }

    fn ax(label: &str, role: Role, x: f64) -> RawNode {
        RawNode {
            dom_node_id: None,
            backend_node_id: Some(100),
            role,
            label: label.into(),
            rect: rect(x, 200.0),
            actions: vec![Action::Click],
            disabled: false,
            hidden: false,
            from_dom: false,
            from_ax: true,
        }
    }

    fn viewport() -> Rect {
        Rect::try_viewport(0.0, 0.0, 1280.0, 720.0).unwrap()
    }

    #[test]
    fn one_pixel_shift_merges_and_different_labels_do_not() {
        let (merged, bindings) = fuse(
            viewport(),
            &[dom("Sign in", Role::Button, 400.0)],
            &[ax("Sign in", Role::Button, 401.0)],
        )
        .unwrap();
        assert_eq!(merged.len(), 1);
        let region = merged.get_str("n100").unwrap();
        assert_eq!(region.label(), "Sign in");
        assert_eq!(region.rect().x(), 400.0);
        assert!(region.sources().contains(SourceMask::DOM));
        assert!(region.sources().contains(SourceMask::ACCESSIBILITY));
        assert_eq!(bindings.get(region.id()).unwrap().dom_node_id, Some(10));

        let (separate, _) = fuse(
            viewport(),
            &[dom("Sign in", Role::Button, 400.0)],
            &[ax("Cancel", Role::Button, 401.0)],
        )
        .unwrap();
        assert_eq!(separate.len(), 2, "different labels must not fuse");
        assert!(separate.get_str("n100").is_some());
        assert!(separate.get_str("ax100").is_some());
    }

    #[test]
    fn role_mismatch_does_not_merge_and_empty_label_may() {
        let (separate, _) = fuse(
            viewport(),
            &[dom("Sign in", Role::Button, 400.0)],
            &[ax("Sign in", Role::Link, 400.0)],
        )
        .unwrap();
        assert_eq!(separate.len(), 2);

        let (merged, _) = fuse(
            viewport(),
            &[dom("Sign in", Role::Button, 400.0)],
            &[ax("", Role::Button, 401.0)],
        )
        .unwrap();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged.get_str("n100").unwrap().label(), "Sign in");
    }
}
