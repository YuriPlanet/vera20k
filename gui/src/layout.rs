//! Placing the nodes, in one linear pass without recursion.
//!
//! The visible nodes are listed in paint order, parents before children, so
//! each is placed in its parent's rect as it is listed. A node with a `Rect`
//! has that rect; any other has no size and sits at its parent's top left
//! corner, a root at the viewport's.
//!
//! Each root's nodes are a span of the list, kept at the root's revision:
//! a layout places again only the roots that changed since (or all, in a
//! new viewport), and copies the rest.

use std::collections::HashMap;

use crate::Rect;
use crate::gui::Gui;
use crate::node::{Node, NodeId, Prop, Tree, Value};

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

/// One root's nodes in the list, placed at its `revision`.
#[derive(Clone, Copy, Debug)]
struct Span {
    root: NodeId,
    revision: u64,
    start: usize,
    len: usize,
}

/// The visible nodes in paint order.
#[derive(Default)]
pub(crate) struct Layout {
    nodes: Vec<Placed>,
    /// One for each root, hidden ones too, in paint order.
    spans: Vec<Span>,
    /// Each root's place in `spans`.
    span_of: HashMap<NodeId, usize>,
    /// Each listed node's root and place in its root's span.
    index: HashMap<NodeId, (NodeId, usize)>,
    viewport: Rect,
    /// The last layout's list and spans while the next is made, kept for
    /// their allocations.
    old_nodes: Vec<Placed>,
    old_spans: Vec<Span>,
    /// Scratch: for each node of the root being placed, the clip its
    /// children get.
    child_clips: Vec<Option<Rect>>,
    /// Scratch: nodes still to list, with their parent's place in the root's
    /// span.
    pending: Vec<(NodeId, Option<usize>)>,
}

impl Layout {
    pub fn nodes(&self) -> &[Placed] {
        &self.nodes
    }

    /// The nodes of `root`, a top-level node.
    pub fn span(&self, root: NodeId) -> Option<&[Placed]> {
        let span = self.spans.get(*self.span_of.get(&root)?)?;
        (span.root == root).then(|| &self.nodes[span.start..span.start + span.len])
    }

    /// The nodes of `id`'s subtree, any node's: its root's span from it
    /// to the first node after it that is not under it. The tree is the one
    /// the list was made from.
    pub fn subtree(&self, tree: &Tree, id: NodeId) -> Option<&[Placed]> {
        let (root, at) = *self.index.get(&id)?;
        let span = self.span(root)?;
        let under = |mut node: NodeId| loop {
            match tree.parent(node) {
                Ok(Some(parent)) if parent == id => return true,
                Ok(Some(parent)) => node = parent,
                _ => return false,
            }
        };
        let len = span[at + 1..]
            .iter()
            .position(|placed| !under(placed.id))
            .unwrap_or(span.len() - at - 1);
        Some(&span[at..=at + len])
    }

    pub fn placed(&self, id: NodeId) -> Option<&Placed> {
        let (root, at) = *self.index.get(&id)?;
        self.span(root)?.get(at)
    }

    /// The viewport of the last layout.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    fn fill(&mut self, tree: &Tree, roots: &[Vec<NodeId>], revision_of: impl Fn(NodeId) -> u64, viewport: Rect) {
        let viewport = viewport.bounded();
        let moved = viewport != self.viewport;
        self.viewport = viewport;
        std::mem::swap(&mut self.nodes, &mut self.old_nodes);
        std::mem::swap(&mut self.spans, &mut self.old_spans);
        self.nodes.clear();
        self.spans.clear();
        // While the roots are the last layout's in its order, each finds
        // its old span at its own place; past that, `span_of` (still the
        // last layout's) does.
        let mut in_order = true;
        for &root in roots.iter().flatten() {
            let at = self.spans.len();
            in_order &= self.old_spans.get(at).is_some_and(|old| old.root == root);
            let old = if in_order {
                Some(self.old_spans[at])
            } else {
                self.span_of.get(&root).map(|&i| self.old_spans[i])
            };
            let revision = revision_of(root);
            let start = self.nodes.len();
            match old {
                Some(old) if !moved && old.revision == revision => {
                    self.nodes
                        .extend_from_slice(&self.old_nodes[old.start..old.start + old.len]);
                }
                old => {
                    if let Some(old) = old {
                        self.unindex(old);
                    }
                    self.place(tree, root);
                }
            }
            self.spans.push(Span {
                root,
                revision,
                start,
                len: self.nodes.len() - start,
            });
        }
        if !in_order || self.spans.len() != self.old_spans.len() {
            self.span_of.clear();
            for (at, span) in self.spans.iter().enumerate() {
                self.span_of.insert(span.root, at);
            }
            // The roots gone since.
            for at in 0..self.old_spans.len() {
                let old = self.old_spans[at];
                if !self.span_of.contains_key(&old.root) {
                    self.unindex(old);
                }
            }
        }
    }

    /// Drops the last layout's `span` from the index.
    fn unindex(&mut self, span: Span) {
        for placed in &self.old_nodes[span.start..span.start + span.len] {
            self.index.remove(&placed.id);
        }
    }

    /// Lists `root`'s visible nodes after the others.
    fn place(&mut self, tree: &Tree, root: NodeId) {
        let base = self.nodes.len();
        self.child_clips.clear();
        let viewport = self.viewport;
        let (nodes, child_clips, index) = (&mut self.nodes, &mut self.child_clips, &mut self.index);
        tree.walk_visible_with(root, None, &mut self.pending, |id, node, parent: Option<usize>| {
            let (area, clip, inherited) = match parent {
                Some(p) => (nodes[base + p].rect, child_clips[p], nodes[base + p].enabled),
                None => (viewport, None, true),
            };
            let rect = match node.props.get(&Prop::Rect) {
                Some(Value::Rect(rect)) => rect.bounded(),
                _ => Rect::new(area.x, area.y, 0, 0),
            };
            let at = nodes.len() - base;
            nodes.push(Placed {
                id,
                rect,
                clip,
                enabled: inherited && node.bool_prop(Prop::Enabled, true),
            });
            child_clips.push(if node.bool_prop(Prop::Clip, false) {
                Some(clip.map_or(rect, |clip| clip.intersect(rect)))
            } else {
                clip
            });
            index.insert(id, (root, at));
            Some(at)
        });
    }
}

impl Tree {
    /// `root`'s visible subtree in paint order, the one the layout lists:
    /// each node before its children, a hidden node's subtree left out. No
    /// recursion: a caller may nest as deep as the node limit.
    pub fn walk_visible(&self, root: NodeId, mut visit: impl FnMut(NodeId)) {
        let mut pending = Vec::new();
        self.walk_visible_with(root, (), &mut pending, |id, _, ()| visit(id));
    }

    /// [`Self::walk_visible`], with `pending` as its scratch: `visit` gets
    /// each node and what its parent's visit returned (`top` for `root`),
    /// and returns what the node's children get.
    pub(crate) fn walk_visible_with<P: Copy>(
        &self,
        root: NodeId,
        top: P,
        pending: &mut Vec<(NodeId, P)>,
        mut visit: impl FnMut(NodeId, &Node, P) -> P,
    ) {
        pending.clear();
        pending.push((root, top));
        while let Some((id, parent)) = pending.pop() {
            let Ok(node) = self.node(id) else { continue };
            if !node.bool_prop(Prop::Visible, true) {
                continue;
            }
            let below = visit(id, node, parent);
            pending.extend(node.children.iter().rev().map(|child| (*child, below)));
        }
    }
}

impl Gui {
    /// Places every visible node in `viewport`: at its `Rect`, or without
    /// one at its parent's top left corner with no size. A root unchanged
    /// since the last layout, in the same viewport, keeps its places.
    pub fn layout(&mut self, viewport: Rect) {
        let meta = &self.meta;
        let revision_of = |root: NodeId| meta.get(&root).map_or(0, |m| m.revision);
        self.layout.fill(&self.tree, &self.roots, revision_of, viewport);
    }

    /// The viewport of the last [`Self::layout`].
    pub fn viewport(&self) -> Rect {
        self.layout.viewport()
    }

    /// The rect from the last [`Self::layout`]; none while hidden or removed,
    /// also if only since then.
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        if !self.tree.effective(id, Prop::Visible) {
            return None;
        }
        self.layout.placed(id).map(|p| p.rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::Layer;
    use crate::origin::Origin;
    use crate::node::NodeKind;

    const VIEW: Rect = Rect::new(0, 0, 200, 100);

    fn hud() -> Gui {
        Gui::new(250_000)
    }

    fn root(hud: &mut Gui, kind: NodeKind) -> NodeId {
        hud.create_root(Layer::Panels, Origin::Host, kind).unwrap()
    }

    fn child(hud: &mut Gui, parent: NodeId, kind: NodeKind) -> NodeId {
        hud.create_by(kind, Some(parent), Origin::Host).unwrap()
    }

    fn flag(hud: &mut Gui, id: NodeId, prop: Prop, v: bool) {
        hud.set(id, prop, Value::Bool(v)).unwrap();
    }

    fn fixed(hud: &mut Gui, id: NodeId, rect: Rect) {
        hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
    }

    fn listed(hud: &Gui) -> Vec<NodeId> {
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
    fn only_a_changed_root_is_placed_again() {
        let mut hud = hud();
        let still = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, still, Rect::new(1, 1, 5, 5));
        let moving = root(&mut hud, NodeKind::Panel);
        let leaf = child(&mut hud, moving, NodeKind::Text);
        hud.layout(VIEW);
        let before = hud.root_revision(still).unwrap();
        // A change the revisions do not see: the still root keeps its places.
        hud.tree.set(still, Prop::Rect, Value::Rect(Rect::new(2, 2, 5, 5))).unwrap();
        fixed(&mut hud, leaf, Rect::new(3, 4, 5, 6));
        assert_eq!(hud.root_revision(still), Ok(before));
        assert!(hud.root_revision(leaf).unwrap() > before);
        hud.layout(VIEW);
        assert_eq!(hud.rect(still), Some(Rect::new(1, 1, 5, 5)));
        assert_eq!(hud.rect(leaf), Some(Rect::new(3, 4, 5, 6)));
        // A new viewport places every root again.
        hud.layout(Rect::new(0, 0, 300, 300));
        assert_eq!(hud.rect(still), Some(Rect::new(2, 2, 5, 5)));
    }

    #[test]
    fn roots_made_and_removed_between_layouts_keep_the_others_places() {
        let mut hud = hud();
        let mut ids = Vec::new();
        for x in 0..4 {
            let id = root(&mut hud, NodeKind::Panel);
            fixed(&mut hud, id, Rect::new(x, 0, 1, 1));
            ids.push(id);
        }
        hud.layout(VIEW);
        hud.remove(ids[1]).unwrap();
        let late = root(&mut hud, NodeKind::Panel);
        hud.layout(VIEW);
        assert_eq!(listed(&hud), [ids[0], ids[2], ids[3], late]);
        assert!(hud.layout.placed(ids[1]).is_none());
        assert_eq!(hud.rect(ids[0]), Some(Rect::new(0, 0, 1, 1)));
        assert_eq!(hud.rect(ids[3]), Some(Rect::new(3, 0, 1, 1)));
        assert_eq!(hud.rect(late), Some(Rect::default()));
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
