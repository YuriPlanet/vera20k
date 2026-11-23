//! Placing the nodes, in one linear pass without recursion.
//!
//! The visible nodes are listed in paint order, parents before children, so
//! each is placed in its parent's rect as it is listed. A node with a `Rect`
//! has that rect; any other has no size and sits at its parent's top left
//! corner, a root at the viewport's.

use std::collections::HashMap;

use crate::Rect;
use crate::node::{NodeId, Prop, Tree, Value};

use super::Hud;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub id: NodeId,
    /// Never of negative size, and `x + w` and `y + h` fit an i32.
    pub rect: Rect,
    /// The clip from the ancestors; none is the whole viewport.
    pub clip: Option<Rect>,
    /// Enabled through every ancestor.
    pub enabled: bool,
}

/// The visible nodes in paint order.
#[derive(Default)]
pub(crate) struct Layout {
    nodes: Vec<Placed>,
    index: HashMap<NodeId, usize>,
    viewport: Rect,
    /// One for each of `nodes`: the clip its children get.
    child_clips: Vec<Option<Rect>>,
    /// Scratch: nodes still to list, with their parent's position.
    pending: Vec<(NodeId, Option<usize>)>,
}

impl Layout {
    pub fn nodes(&self) -> &[Placed] {
        &self.nodes
    }

    pub fn placed(&self, id: NodeId) -> Option<&Placed> {
        self.index.get(&id).and_then(|i| self.nodes.get(*i))
    }

    /// The viewport of the last layout.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    fn fill(&mut self, tree: &Tree, roots: &[Vec<NodeId>], viewport: Rect) {
        self.viewport = viewport.bounded();
        self.nodes.clear();
        self.index.clear();
        self.child_clips.clear();
        self.pending.clear();
        for root in roots.iter().flatten() {
            self.pending.push((*root, None));
            while let Some((id, parent)) = self.pending.pop() {
                let Ok(node) = tree.node(id) else { continue };
                if !node.bool_prop(Prop::Visible, true) {
                    continue;
                }
                let (area, clip, inherited) = match parent {
                    Some(p) => (self.nodes[p].rect, self.child_clips[p], self.nodes[p].enabled),
                    None => (self.viewport, None, true),
                };
                let rect = match node.props.get(&Prop::Rect) {
                    Some(Value::Rect(rect)) => rect.bounded(),
                    _ => Rect::new(area.x, area.y, 0, 0),
                };
                let at = self.nodes.len();
                self.nodes.push(Placed {
                    id,
                    rect,
                    clip,
                    enabled: inherited && node.bool_prop(Prop::Enabled, true),
                });
                self.child_clips.push(if node.bool_prop(Prop::Clip, false) {
                    Some(clip.map_or(rect, |clip| clip.intersect(rect)))
                } else {
                    clip
                });
                self.index.insert(id, at);
                self.pending
                    .extend(node.children.iter().rev().map(|child| (*child, Some(at))));
            }
        }
    }
}

impl Hud {
    /// Places every visible node in `viewport`: at its `Rect`, or without
    /// one at its parent's top left corner with no size.
    pub fn layout(&mut self, viewport: Rect) {
        self.layout.fill(&self.tree, &self.roots, viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{Layer, Origin};
    use crate::node::NodeKind;

    const VIEW: Rect = Rect::new(0, 0, 200, 100);

    fn hud() -> Hud {
        Hud::new(250_000)
    }

    fn root(hud: &mut Hud, kind: NodeKind) -> NodeId {
        hud.create_root(Layer::Panels, Origin::Host, kind).unwrap()
    }

    fn child(hud: &mut Hud, parent: NodeId, kind: NodeKind) -> NodeId {
        hud.create(kind, parent, Origin::Host).unwrap()
    }

    fn flag(hud: &mut Hud, id: NodeId, prop: Prop, v: bool) {
        hud.set(id, prop, Value::Bool(v)).unwrap();
    }

    fn fixed(hud: &mut Hud, id: NodeId, rect: Rect) {
        hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
    }

    fn listed(hud: &Hud) -> Vec<NodeId> {
        hud.layout.nodes().iter().map(|placed| placed.id).collect()
    }

    #[test]
    fn a_node_without_a_rect_sits_at_its_parents_corner() {
        let mut hud = hud();
        let top = root(&mut hud, NodeKind::Panel);
        let panel = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, panel, Rect::new(10, 20, 100, 60));
        let id = child(&mut hud, panel, NodeKind::Text);
        hud.layout(Rect::new(7, 9, 200, 100));
        assert_eq!(hud.rect(top), Some(Rect::new(7, 9, 0, 0)));
        assert_eq!(hud.rect(id), Some(Rect::new(10, 20, 0, 0)));
    }

    #[test]
    fn a_rect_is_absolute() {
        let mut hud = hud();
        let panel = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, panel, Rect::new(50, 50, 10, 10));
        let id = child(&mut hud, panel, NodeKind::Text);
        fixed(&mut hud, id, Rect::new(1, 2, 3, 4));
        hud.layout(VIEW);
        assert_eq!(hud.rect(id), Some(Rect::new(1, 2, 3, 4)));
    }

    #[test]
    fn clips_nest_and_reach_only_descendants() {
        let mut hud = hud();
        let outer = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, outer, Rect::new(0, 0, 50, 50));
        flag(&mut hud, outer, Prop::Clip, true);
        let between = child(&mut hud, outer, NodeKind::Panel);
        let inner = child(&mut hud, between, NodeKind::Panel);
        fixed(&mut hud, inner, Rect::new(30, 40, 50, 50));
        flag(&mut hud, inner, Prop::Clip, true);
        let leaf = child(&mut hud, inner, NodeKind::Text);
        let apart = child(&mut hud, inner, NodeKind::Panel);
        fixed(&mut hud, apart, Rect::new(90, 90, 5, 5));
        flag(&mut hud, apart, Prop::Clip, true);
        let lost = child(&mut hud, apart, NodeKind::Text);
        let beside = root(&mut hud, NodeKind::Panel);
        hud.layout(VIEW);
        let clip = |id| hud.layout.placed(id).unwrap().clip;
        let outer_rect = Some(Rect::new(0, 0, 50, 50));
        assert_eq!(clip(outer), None);
        assert_eq!(clip(between), outer_rect);
        assert_eq!(clip(inner), outer_rect);
        assert_eq!(clip(leaf), Some(Rect::new(30, 40, 20, 10)));
        let empty = clip(lost).unwrap();
        assert_eq!((empty.w, empty.h), (0, 0));
        assert_eq!(clip(beside), None);
    }

    #[test]
    fn extreme_values_saturate() {
        const MIN: i32 = i32::MIN;
        const MAX: i32 = i32::MAX;
        let mut hud = hud();
        let far = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, far, Rect::new(MAX, MAX, MAX, MAX));
        flag(&mut hud, far, Prop::Clip, true);
        let beyond = child(&mut hud, far, NodeKind::Text);
        let before = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, before, Rect::new(MIN, MIN, MIN, -1));

        hud.layout(VIEW);
        assert_eq!(hud.rect(far), Some(Rect::new(MAX, MAX, 0, 0)));
        assert_eq!(hud.rect(beyond), Some(Rect::new(MAX, MAX, 0, 0)));
        assert_eq!(hud.rect(before), Some(Rect::new(MIN, MIN, 0, 0)));

        for viewport in [Rect::new(MIN, MIN, MAX, MAX), Rect::new(MAX, MAX, MIN, MIN)] {
            hud.layout(viewport);
            for placed in hud.layout.nodes() {
                let r = placed.rect;
                assert!(r.w >= 0 && r.x.checked_add(r.w).is_some(), "{r:?}");
                assert!(r.h >= 0 && r.y.checked_add(r.h).is_some(), "{r:?}");
            }
        }
    }

    #[test]
    fn a_deep_tree_needs_no_recursion() {
        const DEPTH: usize = 100_000;
        let mut hud = hud();
        let top = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, top, Rect::new(3, 4, 5, 6));
        flag(&mut hud, top, Prop::Enabled, false);
        let mut parent = top;
        for _ in 1..DEPTH {
            parent = child(&mut hud, parent, NodeKind::Panel);
        }
        let leaf = child(&mut hud, parent, NodeKind::Text);
        hud.layout(VIEW);
        assert_eq!(hud.layout.nodes().len(), DEPTH + 1);
        assert_eq!(hud.rect(leaf), Some(Rect::new(3, 4, 0, 0)));
        assert!(!hud.layout.placed(leaf).unwrap().enabled);
    }

    #[test]
    fn a_hidden_ancestor_hides_the_subtree() {
        let mut hud = hud();
        let top = root(&mut hud, NodeKind::Panel);
        let middle = child(&mut hud, top, NodeKind::Panel);
        let leaf = child(&mut hud, middle, NodeKind::Text);
        flag(&mut hud, leaf, Prop::Visible, true);
        let other = root(&mut hud, NodeKind::Panel);
        hud.layout(VIEW);
        assert_eq!(listed(&hud), [top, middle, leaf, other]);

        flag(&mut hud, top, Prop::Visible, false);
        hud.layout(VIEW);
        assert_eq!(listed(&hud), [other]);
        assert_eq!(hud.rect(leaf), None);
        assert_eq!(hud.rect(other), Some(Rect::default()));

        flag(&mut hud, top, Prop::Visible, true);
        hud.layout(VIEW);
        assert_eq!(listed(&hud), [top, middle, leaf, other]);
    }

    #[test]
    fn enabled_is_inherited() {
        let mut hud = hud();
        let top = root(&mut hud, NodeKind::Panel);
        let off = child(&mut hud, top, NodeKind::Panel);
        flag(&mut hud, off, Prop::Enabled, false);
        let under = child(&mut hud, off, NodeKind::Button);
        flag(&mut hud, under, Prop::Enabled, true);
        let beside = child(&mut hud, top, NodeKind::Button);
        hud.layout(VIEW);
        let enabled = [top, off, under, beside].map(|id| hud.layout.placed(id).unwrap().enabled);
        assert_eq!(enabled, [true, false, false, true]);
    }

    #[test]
    fn nodes_are_listed_by_layer_then_tree_order() {
        let mut hud = hud();
        let mut made = Vec::new();
        for layer in Layer::ALL.into_iter().rev() {
            let first = hud
                .create_root(layer, Origin::Host, NodeKind::Panel)
                .unwrap();
            let second = hud
                .create_root(layer, Origin::Host, NodeKind::Panel)
                .unwrap();
            let a = child(&mut hud, first, NodeKind::Panel);
            let b = child(&mut hud, first, NodeKind::Text);
            let under_a = child(&mut hud, a, NodeKind::Text);
            made.push([first, a, under_a, b, second]);
        }
        hud.layout(VIEW);
        let expected: Vec<NodeId> = made.into_iter().rev().flatten().collect();
        assert_eq!(listed(&hud), expected);
    }
}
