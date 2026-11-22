//! Placing the nodes, in three linear passes without recursion.
//!
//! 1. The visible nodes are listed in paint order.
//! 2. Backwards over that list, each node's natural size is found: its
//!    `Width` and `Height`, where a `Row` or `Column` lacking one takes it
//!    from its visible flowing children's natural sizes. Along its axis that
//!    is their sum plus a `Gap` between each two, across it their largest.
//!    Any other node lacking one is 0.
//! 3. Forwards, each node is placed in its parent's final rect. A flowing
//!    child lacking a cross size takes the parent's, also a `Row` or
//!    `Column` that has a natural one; its own children then see that size.
//!
//! A child with a `Rect` is absolute and has no part in its parent's flow or
//! natural size. A negative size or gap counts as 0.

use std::collections::HashMap;

use crate::Rect;
use crate::node::{Node, NodeId, NodeKind, Prop, Tree, Value};

use super::Hud;

/// The nine points of a parent's rect a node holds to, as `Prop::Hold`'s int.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum Hold {
    #[default]
    TopLeft = 0,
    Top = 1,
    TopRight = 2,
    Left = 3,
    Center = 4,
    Right = 5,
    BottomLeft = 6,
    Bottom = 7,
    BottomRight = 8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    pub id: NodeId,
    /// Never of negative size, and `x + w` and `y + h` fit an i32.
    pub rect: Rect,
    /// The clip from the ancestors; none is the whole viewport.
    pub clip: Option<Rect>,
    /// Enabled through every ancestor.
    pub enabled: bool,
    /// Still visible through every ancestor, as of the last
    /// [`Layout::refresh_visible`].
    pub visible: bool,
}

/// The visible nodes in paint order.
#[derive(Default)]
pub(crate) struct Layout {
    nodes: Vec<Placed>,
    index: HashMap<NodeId, usize>,
    viewport: Rect,
    /// One for each of `nodes`: scratch, but for the parent's position.
    steps: Vec<Step>,
    /// A node may have been hidden, shown or removed since `visible` was read.
    visibility_stale: bool,
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

    /// The position of the parent of the node at `at`.
    pub fn parent(&self, at: usize) -> Option<usize> {
        self.steps[at].parent
    }

    pub fn visibility_changed(&mut self) {
        self.visibility_stale = true;
    }

    /// One pass, where asking each node would climb to its root.
    pub fn refresh_visible(&mut self, tree: &Tree) {
        if !std::mem::take(&mut self.visibility_stale) {
            return;
        }
        for at in 0..self.nodes.len() {
            let inherited = self.parent(at).is_none_or(|p| self.nodes[p].visible);
            let own = tree
                .node(self.nodes[at].id)
                .is_ok_and(|node| node.bool_prop(Prop::Visible, true));
            self.nodes[at].visible = inherited && own;
        }
    }

    /// The viewport of the last layout.
    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    fn fill(&mut self, tree: &Tree, roots: &[Vec<NodeId>], viewport: Rect) {
        self.viewport = bounded(viewport);
        self.visibility_stale = false;
        self.list(tree, roots);
        self.measure();
        self.place();
    }

    fn list(&mut self, tree: &Tree, roots: &[Vec<NodeId>]) {
        self.nodes.clear();
        self.index.clear();
        self.steps.clear();
        self.pending.clear();
        for root in roots.iter().flatten() {
            self.pending.push((*root, None));
            while let Some((id, parent)) = self.pending.pop() {
                let Ok(node) = tree.node(id) else { continue };
                if !node.bool_prop(Prop::Visible, true) {
                    continue;
                }
                let at = self.nodes.len();
                let inherited = parent.is_none_or(|p| self.nodes[p].enabled);
                self.nodes.push(Placed {
                    id,
                    rect: Rect::default(),
                    clip: None,
                    enabled: inherited && node.bool_prop(Prop::Enabled, true),
                    visible: true,
                });
                self.index.insert(id, at);
                self.steps.push(Step::read(node, parent));
                self.pending
                    .extend(node.children.iter().rev().map(|child| (*child, Some(at))));
            }
        }
    }

    /// Children follow their parent, so backwards each is complete before
    /// it counts towards its parent.
    fn measure(&mut self) {
        for at in (0..self.steps.len()).rev() {
            let step = self.steps[at];
            if let (Some(parent), None) = (step.parent, step.fixed) {
                self.steps[parent].add_child(step.natural_size());
            }
        }
    }

    fn place(&mut self) {
        for at in 0..self.steps.len() {
            let step = self.steps[at];
            let (area, clip) = match step.parent {
                Some(parent) => (self.nodes[parent].rect, self.steps[parent].child_clip),
                None => (self.viewport, None),
            };
            let rect = match (step.fixed, step.parent) {
                (Some(fixed), _) => fixed,
                (None, Some(parent)) if self.steps[parent].axis.is_some() => {
                    self.steps[parent].flow(&step, area)
                }
                (None, _) => step.held(area),
            };
            let rect = bounded(rect);
            self.nodes[at].rect = rect;
            self.nodes[at].clip = clip;
            self.steps[at].child_clip = if step.clips {
                Some(clip.map_or(rect, |clip| intersect(clip, rect)))
            } else {
                clip
            };
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Row,
    Column,
}

impl Axis {
    /// (width, height) as (main, cross), and the reverse.
    fn swap<T>(self, (a, b): (T, T)) -> (T, T) {
        match self {
            Axis::Row => (a, b),
            Axis::Column => (b, a),
        }
    }
}

/// What the passes need to know of one node.
#[derive(Clone, Copy, Debug)]
struct Step {
    parent: Option<usize>,
    fixed: Option<Rect>,
    width: Option<i32>,
    height: Option<i32>,
    hold: i32,
    offset: (i32, i32),
    clips: bool,
    child_clip: Option<Rect>,
    axis: Option<Axis>,
    gap: i32,
    /// The flowing children's extent along the axis and across it.
    content: (i64, i32),
    has_flow: bool,
    /// Where the next flowing child begins, from the node's start.
    cursor: i64,
}

impl Step {
    fn read(node: &Node, parent: Option<usize>) -> Step {
        let size = |prop| match node.props.get(&prop) {
            Some(Value::Int(v)) => Some((*v).max(0)),
            _ => None,
        };
        Step {
            parent,
            fixed: match node.props.get(&Prop::Rect) {
                Some(Value::Rect(rect)) => Some(bounded(*rect)),
                _ => None,
            },
            width: size(Prop::Width),
            height: size(Prop::Height),
            hold: node.int_prop(Prop::Hold, Hold::TopLeft as i32),
            offset: (node.int_prop(Prop::X, 0), node.int_prop(Prop::Y, 0)),
            clips: node.bool_prop(Prop::Clip, false),
            child_clip: None,
            axis: match node.kind {
                NodeKind::Row => Some(Axis::Row),
                NodeKind::Column => Some(Axis::Column),
                _ => None,
            },
            gap: node.int_prop(Prop::Gap, 0).max(0),
            content: (0, 0),
            has_flow: false,
            cursor: 0,
        }
    }

    fn natural_size(&self) -> (i32, i32) {
        let content = (saturate(self.content.0), self.content.1);
        let (w, h) = self.axis.map_or((0, 0), |axis| axis.swap(content));
        (self.width.unwrap_or(w), self.height.unwrap_or(h))
    }

    fn add_child(&mut self, size: (i32, i32)) {
        let Some(axis) = self.axis else { return };
        let (main, cross) = axis.swap(size);
        let gap = if self.has_flow { self.gap } else { 0 };
        self.has_flow = true;
        self.content.0 = self
            .content
            .0
            .saturating_add(i64::from(main) + i64::from(gap));
        self.content.1 = self.content.1.max(cross);
    }

    /// The rect of `child`, the next to flow in this node, which is at `area`.
    fn flow(&mut self, child: &Step, area: Rect) -> Rect {
        let (mut w, mut h) = child.natural_size();
        let (mut x, mut y) = (area.x, area.y);
        let main = if self.axis == Some(Axis::Row) {
            x = saturate(i64::from(x).saturating_add(self.cursor));
            h = child.height.unwrap_or(area.h);
            w
        } else {
            y = saturate(i64::from(y).saturating_add(self.cursor));
            w = child.width.unwrap_or(area.w);
            h
        };
        self.cursor = self
            .cursor
            .saturating_add(i64::from(main) + i64::from(self.gap));
        Rect::new(x, y, w, h)
    }

    /// The node's rect by its `Hold` point in `area`.
    fn held(&self, area: Rect) -> Rect {
        let known = Hold::TopLeft as i32..=Hold::BottomRight as i32;
        let hold = if known.contains(&self.hold) {
            self.hold
        } else {
            Hold::TopLeft as i32
        };
        // The nine run row by row; a column or row is that many half sizes in.
        let (column, row) = (i64::from(hold % 3), i64::from(hold / 3));
        let (w, h) = self.natural_size();
        let along = |start: i32, room: i32, size: i32, halves: i64, offset: i32| {
            let point = i64::from(start) + i64::from(room) * halves / 2;
            saturate(point - i64::from(size) * halves / 2 + i64::from(offset))
        };
        Rect::new(
            along(area.x, area.w, w, column, self.offset.0),
            along(area.y, area.h, h, row, self.offset.1),
            w,
            h,
        )
    }
}

fn saturate(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// `rect` without a negative size, its far edges saturated.
pub(super) fn bounded(rect: Rect) -> Rect {
    Rect::new(
        rect.x,
        rect.y,
        rect.w.clamp(0, i32::MAX - rect.x.max(0)),
        rect.h.clamp(0, i32::MAX - rect.y.max(0)),
    )
}

/// What `a` and `b` share; of zero size where they share nothing.
pub(super) fn intersect(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let right = a.x.saturating_add(a.w).min(b.x.saturating_add(b.w));
    let bottom = a.y.saturating_add(a.h).min(b.y.saturating_add(b.h));
    Rect::new(
        x,
        y,
        right.saturating_sub(x).max(0),
        bottom.saturating_sub(y).max(0),
    )
}

impl Hud {
    /// Places every visible node in `viewport`. A root's parent rect is the
    /// viewport.
    ///
    /// A `Rect` is absolute and wins. Any other node has its `Width` and
    /// `Height` and puts its `Hold` point on its parent's, moved by `X` and
    /// `Y`; in a `Row` or `Column` it follows the sibling before it instead,
    /// as wide or high as the parent across the flow unless it says. A `Row`
    /// or `Column` without a size is as large as its visible flowing
    /// children: their sum and gaps along it, the largest across it.
    ///
    /// Then hover and pressed are found again at the pointer's last
    /// position, so this may queue hover events and move [`Hud::revision`].
    pub fn layout(&mut self, viewport: Rect) {
        self.layout.fill(&self.tree, &self.roots, viewport);
        self.retrack();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::{Layer, Origin};

    const VIEW: Rect = Rect::new(0, 0, 200, 100);

    fn hud() -> Hud {
        Hud::new(250_000, Default::default())
    }

    fn root(hud: &mut Hud, kind: NodeKind) -> NodeId {
        hud.create_root(Layer::Panels, Origin::Host, kind).unwrap()
    }

    fn child(hud: &mut Hud, parent: NodeId, kind: NodeKind) -> NodeId {
        hud.create(kind, parent, Origin::Host).unwrap()
    }

    fn int(hud: &mut Hud, id: NodeId, prop: Prop, v: i32) {
        hud.set(id, prop, Value::Int(v)).unwrap();
    }

    fn flag(hud: &mut Hud, id: NodeId, prop: Prop, v: bool) {
        hud.set(id, prop, Value::Bool(v)).unwrap();
    }

    fn fixed(hud: &mut Hud, id: NodeId, rect: Rect) {
        hud.set(id, Prop::Rect, Value::Rect(rect)).unwrap();
    }

    fn sized(hud: &mut Hud, parent: NodeId, w: i32, h: i32) -> NodeId {
        let id = child(hud, parent, NodeKind::Text);
        int(hud, id, Prop::Width, w);
        int(hud, id, Prop::Height, h);
        id
    }

    fn listed(hud: &Hud) -> Vec<NodeId> {
        hud.layout.nodes().iter().map(|placed| placed.id).collect()
    }

    #[test]
    fn each_hold_puts_the_same_point_on_the_parents() {
        let cases = [
            (Hold::TopLeft, 13, 16),
            (Hold::Top, 53, 16),
            (Hold::TopRight, 92, 16),
            (Hold::Left, 13, 41),
            (Hold::Center, 53, 41),
            (Hold::Right, 92, 41),
            (Hold::BottomLeft, 13, 65),
            (Hold::Bottom, 53, 65),
            (Hold::BottomRight, 92, 65),
        ];
        for (hold, x, y) in cases {
            let mut hud = hud();
            let panel = root(&mut hud, NodeKind::Panel);
            fixed(&mut hud, panel, Rect::new(10, 20, 100, 60));
            let id = sized(&mut hud, panel, 21, 11);
            int(&mut hud, id, Prop::X, 3);
            int(&mut hud, id, Prop::Y, -4);
            int(&mut hud, id, Prop::Hold, hold as i32);
            hud.layout(VIEW);
            assert_eq!(hud.rect(id), Some(Rect::new(x, y, 21, 11)), "{hold:?}");
        }
    }

    #[test]
    fn an_unknown_hold_is_top_left() {
        for hold in [i32::MIN, -1, 9, i32::MAX] {
            let mut hud = hud();
            let id = root(&mut hud, NodeKind::Panel);
            int(&mut hud, id, Prop::Hold, hold);
            int(&mut hud, id, Prop::X, 5);
            hud.layout(VIEW);
            assert_eq!(hud.rect(id), Some(Rect::new(5, 0, 0, 0)), "{hold}");
        }
    }

    #[test]
    fn a_roots_parent_is_the_viewport() {
        let mut hud = hud();
        let id = root(&mut hud, NodeKind::Panel);
        int(&mut hud, id, Prop::Width, 30);
        int(&mut hud, id, Prop::Height, 20);
        int(&mut hud, id, Prop::Hold, Hold::BottomRight as i32);
        hud.layout(Rect::new(7, 9, 200, 100));
        assert_eq!(hud.rect(id), Some(Rect::new(177, 89, 30, 20)));
    }

    #[test]
    fn an_explicit_rect_wins() {
        let mut hud = hud();
        let panel = root(&mut hud, NodeKind::Panel);
        int(&mut hud, panel, Prop::X, 50);
        let id = sized(&mut hud, panel, 8, 8);
        int(&mut hud, id, Prop::Hold, Hold::Center as i32);
        fixed(&mut hud, id, Rect::new(1, 2, 3, 4));
        hud.layout(VIEW);
        assert_eq!(hud.rect(id), Some(Rect::new(1, 2, 3, 4)));
    }

    #[test]
    fn a_row_flows_past_hidden_and_absolute_children() {
        let mut hud = hud();
        let row = root(&mut hud, NodeKind::Row);
        fixed(&mut hud, row, Rect::new(10, 20, 90, 30));
        int(&mut hud, row, Prop::Gap, 4);
        let a = sized(&mut hud, row, 12, 8);
        int(&mut hud, a, Prop::X, 99);
        int(&mut hud, a, Prop::Y, 99);
        int(&mut hud, a, Prop::Hold, Hold::BottomRight as i32);
        let hidden = sized(&mut hud, row, 40, 40);
        flag(&mut hud, hidden, Prop::Visible, false);
        let absolute = child(&mut hud, row, NodeKind::Text);
        fixed(&mut hud, absolute, Rect::new(1, 2, 3, 4));
        let b = child(&mut hud, row, NodeKind::Text);
        int(&mut hud, b, Prop::Width, 7);
        hud.layout(VIEW);
        assert_eq!(hud.rect(a), Some(Rect::new(10, 20, 12, 8)));
        assert_eq!(hud.rect(hidden), None);
        assert_eq!(hud.rect(absolute), Some(Rect::new(1, 2, 3, 4)));
        assert_eq!(hud.rect(b), Some(Rect::new(26, 20, 7, 30)));
    }

    #[test]
    fn a_column_flows_down_and_gives_its_width() {
        let mut hud = hud();
        let column = root(&mut hud, NodeKind::Column);
        fixed(&mut hud, column, Rect::new(5, 6, 40, 90));
        int(&mut hud, column, Prop::Gap, 2);
        let a = sized(&mut hud, column, 11, 10);
        let hidden = sized(&mut hud, column, 11, 10);
        flag(&mut hud, hidden, Prop::Visible, false);
        let b = child(&mut hud, column, NodeKind::Text);
        int(&mut hud, b, Prop::Height, 9);
        hud.layout(VIEW);
        assert_eq!(hud.rect(a), Some(Rect::new(5, 6, 11, 10)));
        assert_eq!(hud.rect(b), Some(Rect::new(5, 18, 40, 9)));
    }

    #[test]
    fn flows_take_missing_sizes_from_their_children() {
        let mut hud = hud();
        let column = root(&mut hud, NodeKind::Column);
        int(&mut hud, column, Prop::Gap, 5);
        int(&mut hud, column, Prop::Hold, Hold::BottomRight as i32);
        let row = child(&mut hud, column, NodeKind::Row);
        int(&mut hud, row, Prop::Gap, 1);
        let a = sized(&mut hud, row, 10, 8);
        let b = sized(&mut hud, row, 20, 12);
        let hidden = sized(&mut hud, row, 70, 70);
        flag(&mut hud, hidden, Prop::Visible, false);
        let absolute = child(&mut hud, row, NodeKind::Text);
        fixed(&mut hud, absolute, Rect::new(0, 0, 80, 80));
        let wide = sized(&mut hud, column, 50, 6);
        hud.layout(VIEW);
        assert_eq!(hud.rect(column), Some(Rect::new(150, 77, 50, 23)));
        assert_eq!(hud.rect(row), Some(Rect::new(150, 77, 50, 12)));
        assert_eq!(hud.rect(a), Some(Rect::new(150, 77, 10, 8)));
        assert_eq!(hud.rect(b), Some(Rect::new(161, 77, 20, 12)));
        assert_eq!(hud.rect(wide), Some(Rect::new(150, 94, 50, 6)));
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
        let beyond = sized(&mut hud, far, MAX, MAX);
        int(&mut hud, beyond, Prop::X, MAX);
        int(&mut hud, beyond, Prop::Y, MAX);
        let before = root(&mut hud, NodeKind::Panel);
        fixed(&mut hud, before, Rect::new(MIN, MIN, MIN, -1));
        let held = sized(&mut hud, before, MAX, MIN);
        int(&mut hud, held, Prop::Hold, Hold::BottomRight as i32);
        int(&mut hud, held, Prop::X, MIN);
        let row = root(&mut hud, NodeKind::Row);
        int(&mut hud, row, Prop::Gap, MAX);
        sized(&mut hud, row, MAX, MAX);
        sized(&mut hud, row, MAX, MAX);
        let last = sized(&mut hud, row, MAX, MAX);
        let tight = root(&mut hud, NodeKind::Column);
        int(&mut hud, tight, Prop::Gap, MIN);
        sized(&mut hud, tight, 2, 3);
        let next = sized(&mut hud, tight, 2, 3);

        hud.layout(VIEW);
        assert_eq!(hud.rect(far), Some(Rect::new(MAX, MAX, 0, 0)));
        assert_eq!(hud.rect(beyond), Some(Rect::new(MAX, MAX, 0, 0)));
        assert_eq!(hud.rect(before), Some(Rect::new(MIN, MIN, 0, 0)));
        assert_eq!(hud.rect(held), Some(Rect::new(MIN, MIN, MAX, 0)));
        assert_eq!(hud.rect(row), Some(Rect::new(0, 0, MAX, MAX)));
        assert_eq!(hud.rect(last), Some(Rect::new(MAX, 0, 0, MAX)));
        assert_eq!(hud.rect(next), Some(Rect::new(0, 3, 2, 3)));

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
        let top = root(&mut hud, NodeKind::Row);
        flag(&mut hud, top, Prop::Enabled, false);
        let mut parent = top;
        for depth in 1..DEPTH {
            let kind = if depth % 2 == 0 {
                NodeKind::Row
            } else {
                NodeKind::Column
            };
            parent = child(&mut hud, parent, kind);
        }
        let leaf = sized(&mut hud, parent, 3, 4);
        hud.layout(VIEW);
        assert_eq!(hud.layout.nodes().len(), DEPTH + 1);
        assert_eq!(hud.rect(top), Some(Rect::new(0, 0, 3, 4)));
        assert_eq!(hud.rect(leaf), Some(Rect::new(0, 0, 3, 4)));
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
