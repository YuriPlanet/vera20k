//! Pointer and Escape routing over the last layout.
//!
//! The nodes under the pointer that are clickable, hoverable or blockers are
//! the candidates, down to the first blocker, in the highest layer with any.
//! A press goes to the clickable one the [`ClickOrder`] picks, hover to the
//! clickable or hoverable one the [`HoverOrder`] picks, and the wheel to the
//! hovered one, else to the blocker. So a node that only hovers lets a press
//! through to a clickable node beneath it in its layer, never to a lower one.
//! A disabled node takes its turn like any other and then does nothing. In
//! `Field`, where all the candidates only hover, the event is the world's.
//!
//! A press belongs to whoever got it until its release. While the world holds
//! one, no node hovers and moves are the world's, even if the HUD holds the
//! other button. A release whose press was not seen is the world's.
//!
//! Geometry comes from the last layout: a node shown since then takes no
//! input and a modal one does not block yet. Visibility, enabling and the
//! pointer props are read live. Each layout looks again at what lies under
//! the pointer's last position.

use crate::Rect;
use crate::gui::MouseButton;
use crate::node::{Node, NodeId, NodeKind, Prop};

use super::layout::Placed;
use super::{Hud, Layer, Origin};

/// The most events kept for [`Hud::drain_events`], hover aside.
const MAX_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClickOrder {
    #[default]
    Topmost,
    /// The earliest node in tree order within the top layer hit.
    FirstInTree,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HoverOrder {
    #[default]
    Topmost,
    /// The smallest rect within the top layer hit; the later node on a tie.
    SmallestArea,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputPolicy {
    pub click: ClickOrder,
    pub hover: HoverOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Move {
        x: i32,
        y: i32,
    },
    Down {
        x: i32,
        y: i32,
        button: MouseButton,
    },
    Up {
        x: i32,
        y: i32,
        button: MouseButton,
    },
    Wheel {
        x: i32,
        y: i32,
        lines: i32,
    },
    Escape,
    /// The window lost the pointer: captures end, nothing activates.
    Cancel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The world must not see this event.
    pub consumed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HudEvent {
    Click {
        node: NodeId,
        button: MouseButton,
        command: Option<String>,
    },
    Hover {
        node: NodeId,
        entered: bool,
    },
    Wheel {
        node: NodeId,
        lines: i32,
    },
    Dismiss {
        node: NodeId,
    },
}

#[derive(Clone, Copy, Debug)]
enum Press {
    World,
    /// `node` is none when nothing can activate; the HUD keeps the press all
    /// the same. `fired` is a click already reported on the press.
    Hud {
        node: Option<NodeId>,
        fired: bool,
    },
}

#[derive(Clone, Copy, Debug)]
struct Hover {
    node: NodeId,
    /// Enter was reported, so leave will be.
    reported: bool,
}

pub(crate) struct State {
    policy: InputPolicy,
    events: Vec<HudEvent>,
    /// By [`MouseButton`].
    presses: [Option<Press>; 2],
    hover: Option<Hover>,
    /// What a press at the pointer would go to.
    over: Option<NodeId>,
    /// Where the pointer last was; none once cancelled.
    pointer: Option<(i32, i32)>,
}

impl State {
    pub fn new(policy: InputPolicy) -> Self {
        State {
            policy,
            events: Vec::new(),
            presses: [None; 2],
            hover: None,
            over: None,
            pointer: None,
        }
    }

    fn world_holds(&self) -> bool {
        self.presses
            .iter()
            .any(|press| matches!(press, Some(Press::World)))
    }

    fn hud_holds(&self) -> bool {
        self.presses
            .iter()
            .any(|press| matches!(press, Some(Press::Hud { .. })))
    }

    fn set_hover(&mut self, hover: Option<Hover>) {
        let reported = |hover: Option<Hover>| hover.filter(|h| h.reported).map(|h| h.node);
        let (old, new) = (reported(self.hover), reported(hover));
        self.hover = hover;
        if old == new {
            return;
        }
        if let Some(node) = old {
            self.push_hover(node, false);
        }
        if let Some(node) = new {
            self.push_hover(node, true);
        }
    }

    /// An enter and a leave of one node cancel out until drained, so only
    /// the net change waits: one leave and one enter at most.
    fn push_hover(&mut self, node: NodeId, entered: bool) {
        let opposite = HudEvent::Hover {
            node,
            entered: !entered,
        };
        match self.events.iter().position(|event| *event == opposite) {
            Some(at) => {
                self.events.remove(at);
            }
            None => self.events.push(HudEvent::Hover { node, entered }),
        }
    }

    fn push_wheel(&mut self, node: NodeId, lines: i32) {
        if lines == 0 {
            return;
        }
        let newest = self
            .events
            .iter()
            .rposition(|e| !matches!(e, HudEvent::Hover { .. }));
        if let Some(at) = newest
            && let HudEvent::Wheel {
                node: last,
                lines: total,
            } = &mut self.events[at]
            && *last == node
        {
            *total = total.saturating_add(lines);
            if *total == 0 {
                self.events.remove(at);
            }
            return;
        }
        self.push(HudEvent::Wheel { node, lines });
    }

    fn full(&self) -> bool {
        self.events.len() >= MAX_EVENTS
    }

    fn push(&mut self, event: HudEvent) {
        if !self.full() {
            self.events.push(event);
        }
    }
}

/// What lies at a point.
#[derive(Clone, Copy, Debug, Default)]
struct Hit {
    modal: bool,
    /// The candidates take the event from the world.
    takes: bool,
    /// The layer that decides; none when no candidate is there.
    layer: Option<Layer>,
    click: Option<NodeId>,
    hover: Option<Hover>,
    wheel: Option<NodeId>,
}

impl Hit {
    fn blocks_world(&self) -> bool {
        self.modal || self.takes
    }
}

/// `Rect::contains` would overflow at the ends of i32.
fn contains(rect: Rect, x: i32, y: i32) -> bool {
    let within = |start: i32, size: i32, at: i32| {
        (0..i64::from(size)).contains(&(i64::from(at) - i64::from(start)))
    };
    within(rect.x, rect.w, x) && within(rect.y, rect.h, y)
}

fn clickable(node: &Node) -> bool {
    node.bool_prop(Prop::Clickable, node.kind == NodeKind::Button)
}

fn hoverable(node: &Node) -> bool {
    node.bool_prop(Prop::Hoverable, false)
}

fn area(rect: Rect) -> i64 {
    i64::from(rect.w.max(0)) * i64::from(rect.h.max(0))
}

impl Hud {
    /// Routes one event over the last [`Self::layout`]; what it caused waits
    /// in [`Self::drain_events`]. `Cancel` is never consumed, and Escape only
    /// under a modal or by a dismissal of a node whose origin is not external.
    pub fn input(&mut self, event: Pointer) -> Response {
        self.layout.refresh_visible(&self.tree);
        if let Pointer::Move { x, y }
        | Pointer::Down { x, y, .. }
        | Pointer::Up { x, y, .. }
        | Pointer::Wheel { x, y, .. } = event
        {
            self.input.pointer = Some((x, y));
        }
        let before = self.highlight();
        let consumed = match event {
            Pointer::Move { x, y } => self.pointer_move(x, y),
            Pointer::Down { x, y, button } => self.pointer_down(x, y, button),
            Pointer::Up { x, y, button } => self.pointer_up(x, y, button),
            Pointer::Wheel { x, y, lines } => self.pointer_wheel(x, y, lines),
            Pointer::Escape => self.escape(),
            Pointer::Cancel => {
                self.input.presses = [None; 2];
                self.input.over = None;
                self.input.pointer = None;
                self.input.set_hover(None);
                false
            }
        };
        self.redraw_if_changed(before);
        Response { consumed }
    }

    /// The events since the last call, oldest first. While they wait, an
    /// enter and a leave of one node cancel out and wheel turns over one node
    /// add up. Past 256 waiting, new clicks, dismissals and wheel turns are
    /// dropped (still consumed); hover changes are always kept.
    pub fn drain_events(&mut self) -> Vec<HudEvent> {
        std::mem::take(&mut self.input.events)
    }

    /// The hoverable or clickable node under the pointer, disabled or not.
    pub fn hovered(&self) -> Option<NodeId> {
        self.input.hover.map(|hover| hover.node)
    }

    /// The left button went down on the enabled node and is still over it.
    pub fn shows_pressed(&self, id: NodeId) -> bool {
        self.pressed() == Some(id)
    }

    fn pressed(&self) -> Option<NodeId> {
        match self.input.presses[MouseButton::Left as usize] {
            Some(Press::Hud { node, .. }) => node.filter(|id| self.input.over == Some(*id)),
            _ => None,
        }
    }

    /// The hovered and the pressed node: what input changes in the picture.
    fn highlight(&self) -> [Option<NodeId>; 2] {
        [self.hovered(), self.pressed()]
    }

    fn redraw_if_changed(&mut self, before: [Option<NodeId>; 2]) {
        if self.highlight() != before {
            self.revision += 1;
        }
    }

    /// After a layout: nodes may have moved or opened under a still pointer.
    pub(super) fn retrack(&mut self) {
        let Some((x, y)) = self.input.pointer else {
            return;
        };
        let before = self.highlight();
        let hit = self.hit(x, y);
        self.track(&hit);
        self.redraw_if_changed(before);
    }

    /// Drops hover and press targets now removed, hidden or no longer
    /// hoverable or clickable, and press targets now disabled. The press
    /// itself stays the HUD's, so its release does not fall through to the
    /// world.
    pub(crate) fn forget_inactive(&mut self) {
        self.layout.visibility_changed();
        let tree = &self.tree;
        let shown = |id: NodeId| tree.effective(id, Prop::Visible);
        let clicks = |id: NodeId| shown(id) && tree.node(id).is_ok_and(clickable);
        for press in self.input.presses.iter_mut().flatten() {
            if let Press::Hud { node, .. } = press {
                *node = node.filter(|id| clicks(*id) && tree.effective(*id, Prop::Enabled));
            }
        }
        self.input.over = self.input.over.filter(|id| clicks(*id));
        let hover = self.input.hover.and_then(|Hover { node: id, .. }| {
            let node = tree.node(id).ok().filter(|_| shown(id))?;
            let reported = hoverable(node);
            (reported || clickable(node)).then_some(Hover { node: id, reported })
        });
        self.input.set_hover(hover);
    }

    fn pointer_move(&mut self, x: i32, y: i32) -> bool {
        let hit = self.hit(x, y);
        self.track(&hit);
        // A drag in the world goes on over the HUD, though not under a modal.
        hit.modal || (!self.input.world_holds() && (self.input.hud_holds() || hit.blocks_world()))
    }

    fn pointer_down(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        let hit = self.hit(x, y);
        let dismissed = self.dismissed_by_press(x, y, &hit, true);
        // An external popup takes only the world's press, never the HUD's,
        // and leaves the rule of the popup beneath it in force.
        let beneath = self
            .dismissed_by_press(x, y, &hit, false)
            .filter(|id| Some(*id) != dismissed);
        let eaten = beneath.is_some()
            || dismissed.is_some_and(|id| !self.is_external(id) || !hit.blocks_world());
        let press = if eaten {
            Press::Hud {
                node: None,
                fired: false,
            }
        } else if hit.blocks_world() {
            let node = hit.click.filter(|id| self.can_activate(*id));
            let fired = node.is_some_and(|id| {
                self.tree
                    .node(id)
                    .is_ok_and(|n| n.bool_prop(Prop::ActivateOnPress, false))
            });
            Press::Hud { node, fired }
        } else {
            Press::World
        };
        self.input.presses[button as usize] = Some(press);
        self.track(&hit);

        for node in [dismissed, beneath].into_iter().flatten() {
            self.input.push(HudEvent::Dismiss { node });
        }
        if let Press::Hud {
            node: Some(node),
            fired: true,
        } = press
        {
            self.click(node, button);
        }
        matches!(press, Press::Hud { .. })
    }

    fn pointer_up(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        let press = self.input.presses[button as usize].take();
        let hit = self.hit(x, y);
        self.track(&hit);
        let Some(Press::Hud { node, fired }) = press else {
            return false;
        };
        if let Some(node) = node
            && !fired
            && hit.click == Some(node)
            && self.can_activate(node)
        {
            self.click(node, button);
        }
        true
    }

    fn pointer_wheel(&mut self, x: i32, y: i32, lines: i32) -> bool {
        let hit = self.hit(x, y);
        self.track(&hit);
        if let Some(node) = hit.wheel.filter(|id| self.can_activate(*id)) {
            self.input.push_wheel(node, lines);
        }
        hit.blocks_world()
    }

    fn escape(&mut self) -> bool {
        let modal = self.modal_open();
        let Some((_, node)) = self.dismissable(modal, true) else {
            return modal;
        };
        self.input.push(HudEvent::Dismiss { node });
        if !self.is_external(node) {
            return true;
        }
        // An external popup hears of it and hides nothing beneath it.
        let Some((_, beneath)) = self.dismissable(modal, false) else {
            return false;
        };
        self.input.push(HudEvent::Dismiss { node: beneath });
        true
    }

    fn is_external(&self, node: NodeId) -> bool {
        self.meta
            .get(&node)
            .is_some_and(|meta| matches!(meta.origin, Origin::External(_)))
    }

    fn track(&mut self, hit: &Hit) {
        self.input.over = hit.click;
        let hover = if self.input.world_holds() {
            None
        } else {
            hit.hover
        };
        self.input.set_hover(hover);
    }

    fn click(&mut self, node: NodeId, button: MouseButton) {
        if self.input.full() {
            return;
        }
        let Ok(n) = self.tree.node(node) else { return };
        let command = n.text_prop(Prop::Command).map(str::to_owned);
        self.input.push(HudEvent::Click {
            node,
            button,
            command,
        });
    }

    /// Enabled both as laid out and now.
    fn can_activate(&self, id: NodeId) -> bool {
        self.layout.placed(id).is_some_and(|placed| placed.enabled)
            && self.tree.effective(id, Prop::Enabled)
    }

    /// Whether the point is on what of the node was painted.
    fn covers(&self, placed: &Placed, x: i32, y: i32) -> bool {
        contains(self.layout.viewport(), x, y)
            && contains(placed.rect, x, y)
            && placed.clip.is_none_or(|clip| contains(clip, x, y))
    }

    fn modal_open(&self) -> bool {
        // The layout is in layer order, so only its top end is looked at.
        self.layout
            .nodes()
            .iter()
            .rev()
            .filter_map(|placed| {
                self.meta
                    .get(&placed.id)
                    .map(|meta| (placed.visible, meta.layer))
            })
            .take_while(|(_, layer)| *layer >= Layer::Modal)
            .any(|(visible, layer)| visible && layer == Layer::Modal)
    }

    fn hit(&self, x: i32, y: i32) -> Hit {
        let mut hit = Hit {
            modal: self.modal_open(),
            ..Hit::default()
        };
        let mut lowest = if hit.modal {
            Layer::Modal
        } else {
            Layer::Field
        };
        let mut hover_area = i64::MAX;
        let mut front = None;
        for placed in self.layout.nodes().iter().rev() {
            if !self.covers(placed, x, y) {
                continue;
            }
            let id = placed.id;
            let (Some(meta), Ok(node)) = (self.meta.get(&id), self.tree.node(id)) else {
                continue;
            };
            if meta.layer < lowest {
                break;
            }
            let (clicks, hovers) = (clickable(node), hoverable(node));
            let blocker = node.bool_prop(Prop::BlocksPointer, false);
            if !(clicks || hovers || blocker) || !placed.visible {
                continue;
            }
            hit.takes |= clicks || blocker || meta.layer > Layer::Field;
            lowest = meta.layer;
            hit.layer = Some(meta.layer);
            front.get_or_insert(id);

            // The walk is front to back.
            if clicks {
                let takes = match self.input.policy.click {
                    ClickOrder::Topmost => hit.click.is_none(),
                    ClickOrder::FirstInTree => true,
                };
                if takes {
                    hit.click = Some(id);
                }
            }
            if clicks || hovers {
                let size = area(placed.rect);
                let takes = match self.input.policy.hover {
                    HoverOrder::Topmost => hit.hover.is_none(),
                    HoverOrder::SmallestArea => size < hover_area,
                };
                if takes {
                    hit.hover = Some(Hover {
                        node: id,
                        reported: hovers,
                    });
                    hover_area = size;
                }
            }
            // What is painted after a blocker was seen already, its
            // descendants among it.
            if blocker {
                break;
            }
        }
        if hit.takes {
            hit.wheel = hit.hover.map(|hover| hover.node).or(front);
        }
        hit
    }

    /// The topmost visible `Dismissable` node and its place in the layout.
    /// With a modal open, none beneath the modal layer.
    fn dismissable(&self, modal: bool, external: bool) -> Option<(usize, NodeId)> {
        let lowest = if modal { Layer::Modal } else { Layer::Field };
        self.layout
            .nodes()
            .iter()
            .enumerate()
            .rev()
            .find_map(|(at, placed)| {
                let id = placed.id;
                let node = self.tree.node(id).ok()?;
                let found = node.bool_prop(Prop::Dismissable, false)
                    && (external || !self.is_external(id))
                    && self.meta.get(&id)?.layer >= lowest
                    && placed.visible;
                found.then_some((at, id))
            })
    }

    fn dismissed_by_press(&self, x: i32, y: i32, hit: &Hit, external: bool) -> Option<NodeId> {
        let (at, id) = self.dismissable(hit.modal, external)?;
        let layer = self.meta.get(&id)?.layer;
        if hit.layer.is_some_and(|top| top > layer) || self.subtree_covers(at, x, y) {
            return None;
        }
        Some(id)
    }

    /// Whether the point is on the node at `root` in the layout or on any of
    /// its visible descendants, which follow it there without a gap.
    fn subtree_covers(&self, root: usize, x: i32, y: i32) -> bool {
        for (at, placed) in self.layout.nodes().iter().enumerate().skip(root) {
            if at > root && self.layout.parent(at).is_none_or(|parent| parent < root) {
                return false;
            }
            if placed.visible && self.covers(placed, x, y) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Value;

    const LEFT: MouseButton = MouseButton::Left;
    const RIGHT: MouseButton = MouseButton::Right;

    fn new_hud() -> Hud {
        Hud::new(64, InputPolicy::default())
    }

    fn place(hud: &mut Hud, id: NodeId, (x, y, w, h): (i32, i32, i32, i32)) {
        for (prop, v) in [
            (Prop::X, x),
            (Prop::Y, y),
            (Prop::Width, w),
            (Prop::Height, h),
        ] {
            hud.set(id, prop, Value::Int(v)).unwrap();
        }
    }

    fn root(hud: &mut Hud, layer: Layer, kind: NodeKind, at: (i32, i32, i32, i32)) -> NodeId {
        let id = hud.create_root(layer, Origin::Host, kind).unwrap();
        place(hud, id, at);
        id
    }

    /// `at` is relative to the parent.
    fn child(hud: &mut Hud, parent: NodeId, kind: NodeKind, at: (i32, i32, i32, i32)) -> NodeId {
        let id = hud.create(kind, parent, Origin::Host).unwrap();
        place(hud, id, at);
        id
    }

    fn button(hud: &mut Hud, layer: Layer, at: (i32, i32, i32, i32)) -> NodeId {
        root(hud, layer, NodeKind::Button, at)
    }

    fn blocker(hud: &mut Hud, layer: Layer, at: (i32, i32, i32, i32)) -> NodeId {
        let id = root(hud, layer, NodeKind::Panel, at);
        turn_on(hud, id, Prop::BlocksPointer);
        id
    }

    fn turn_on(hud: &mut Hud, id: NodeId, prop: Prop) {
        hud.set(id, prop, Value::Bool(true)).unwrap();
    }

    fn turn_off(hud: &mut Hud, id: NodeId, prop: Prop) {
        hud.set(id, prop, Value::Bool(false)).unwrap();
    }

    fn lay_out(hud: &mut Hud) {
        hud.layout(Rect::new(0, 0, 500, 400));
    }

    fn move_to(hud: &mut Hud, x: i32, y: i32) -> bool {
        hud.input(Pointer::Move { x, y }).consumed
    }

    fn down(hud: &mut Hud, x: i32, y: i32, button: MouseButton) -> bool {
        hud.input(Pointer::Down { x, y, button }).consumed
    }

    fn up(hud: &mut Hud, x: i32, y: i32, button: MouseButton) -> bool {
        hud.input(Pointer::Up { x, y, button }).consumed
    }

    fn wheel(hud: &mut Hud, x: i32, y: i32, lines: i32) -> bool {
        hud.input(Pointer::Wheel { x, y, lines }).consumed
    }

    fn escape(hud: &mut Hud) -> bool {
        hud.input(Pointer::Escape).consumed
    }

    /// The nodes clicked since the last drain.
    fn clicked(hud: &mut Hud) -> Vec<NodeId> {
        hud.drain_events()
            .into_iter()
            .filter_map(|event| match event {
                HudEvent::Click { node, .. } => Some(node),
                _ => None,
            })
            .collect()
    }

    /// Presses and releases the left button at a point; what was clicked.
    fn tap(hud: &mut Hud, x: i32, y: i32) -> Vec<NodeId> {
        down(hud, x, y, LEFT);
        up(hud, x, y, LEFT);
        clicked(hud)
    }

    #[test]
    fn without_a_layout_everything_is_the_worlds() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        assert!(!move_to(&mut hud, 10, 10));
        assert!(!down(&mut hud, 10, 10, LEFT));
        assert!(!up(&mut hud, 10, 10, LEFT));
        assert!(!wheel(&mut hud, 10, 10, 1));
        assert!(!escape(&mut hud));
        hud.remove(node).unwrap();
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn only_candidates_are_hit() {
        let mut hud = new_hud();
        root(&mut hud, Layer::Panels, NodeKind::Panel, (0, 0, 50, 50));
        blocker(&mut hud, Layer::Panels, (100, 0, 50, 50));
        let hover = root(&mut hud, Layer::Panels, NodeKind::Image, (200, 0, 50, 50));
        turn_on(&mut hud, hover, Prop::Hoverable);
        let made = root(&mut hud, Layer::Panels, NodeKind::Image, (300, 0, 50, 50));
        turn_on(&mut hud, made, Prop::Clickable);
        let inert = button(&mut hud, Layer::Panels, (400, 0, 50, 50));
        turn_off(&mut hud, inert, Prop::Clickable);
        lay_out(&mut hud);

        assert!(!move_to(&mut hud, 10, 10));
        assert!(move_to(&mut hud, 110, 10));
        assert_eq!(hud.hovered(), None);
        assert!(move_to(&mut hud, 210, 10));
        assert_eq!(tap(&mut hud, 310, 10), [made]);
        assert!(!move_to(&mut hud, 410, 10));
    }

    #[test]
    fn a_hit_is_inside_the_rect_and_the_clip() {
        let mut hud = new_hud();
        let panel = root(&mut hud, Layer::Panels, NodeKind::Panel, (10, 10, 40, 40));
        turn_on(&mut hud, panel, Prop::Clip);
        let node = child(&mut hud, panel, NodeKind::Button, (20, 0, 60, 20));
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 30, 10), [node]);
        assert_eq!(tap(&mut hud, 49, 29), [node]);
        assert!(!move_to(&mut hud, 50, 15), "clipped");
        assert!(!move_to(&mut hud, 35, 30), "below the rect");
        button(&mut hud, Layer::Panels, (480, 0, 60, 20));
        lay_out(&mut hud);
        assert!(move_to(&mut hud, 499, 10));
        assert!(!move_to(&mut hud, 500, 10), "outside the viewport");
    }

    #[test]
    fn the_highest_layer_with_a_hit_decides() {
        let mut hud = new_hud();
        let top = button(&mut hud, Layer::Overlay, (0, 0, 50, 50));
        let low = button(&mut hud, Layer::Field, (0, 0, 100, 50));
        let label = root(&mut hud, Layer::Panels, NodeKind::Text, (60, 0, 20, 50));
        turn_on(&mut hud, label, Prop::Hoverable);
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 10, 10), [top]);
        assert_eq!(tap(&mut hud, 90, 10), [low]);
        assert_eq!(
            tap(&mut hud, 70, 10),
            [],
            "a hover-only node keeps lower layers out"
        );
    }

    #[test]
    fn click_order_works_within_a_layer() {
        for order in [ClickOrder::Topmost, ClickOrder::FirstInTree] {
            let mut hud = Hud::new(
                64,
                InputPolicy {
                    click: order,
                    ..InputPolicy::default()
                },
            );
            let field = button(&mut hud, Layer::Field, (0, 0, 100, 100));
            let first = button(&mut hud, Layer::Panels, (0, 0, 80, 80));
            let last = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
            lay_out(&mut hud);
            let want = if order == ClickOrder::Topmost {
                last
            } else {
                first
            };
            assert_eq!(tap(&mut hud, 10, 10), [want]);
            assert_eq!(tap(&mut hud, 90, 90), [field]);
        }
    }

    #[test]
    fn hover_order_works_within_a_layer() {
        for order in [HoverOrder::Topmost, HoverOrder::SmallestArea] {
            let mut hud = Hud::new(
                64,
                InputPolicy {
                    hover: order,
                    ..InputPolicy::default()
                },
            );
            button(&mut hud, Layer::Field, (0, 0, 15, 15));
            let small = button(&mut hud, Layer::Panels, (0, 0, 20, 20));
            let big = button(&mut hud, Layer::Panels, (0, 0, 90, 90));
            lay_out(&mut hud);
            move_to(&mut hud, 10, 10);
            let want = if order == HoverOrder::Topmost {
                big
            } else {
                small
            };
            assert_eq!(hud.hovered(), Some(want));

            let tie = button(&mut hud, Layer::Panels, (0, 0, 20, 20));
            lay_out(&mut hud);
            move_to(&mut hud, 10, 10);
            assert_eq!(hud.hovered(), Some(tie), "the later node on a tie");
        }
    }

    #[test]
    fn a_hover_only_node_lets_a_press_through_in_its_layer() {
        let mut hud = new_hud();
        let below = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let label = root(&mut hud, Layer::Panels, NodeKind::Text, (0, 0, 50, 50));
        turn_on(&mut hud, label, Prop::Hoverable);
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 10, 10), [below]);
        assert_eq!(hud.hovered(), Some(label));
    }

    #[test]
    fn a_blocker_hides_what_is_beneath_but_not_its_descendants() {
        for click in [ClickOrder::Topmost, ClickOrder::FirstInTree] {
            let mut hud = Hud::new(
                64,
                InputPolicy {
                    click,
                    ..InputPolicy::default()
                },
            );
            button(&mut hud, Layer::Panels, (0, 0, 200, 200));
            let panel = blocker(&mut hud, Layer::Panels, (0, 0, 100, 100));
            let inner = child(&mut hud, panel, NodeKind::Button, (0, 0, 20, 20));
            lay_out(&mut hud);
            assert_eq!(tap(&mut hud, 10, 10), [inner]);
            assert!(down(&mut hud, 50, 50, LEFT));
            assert!(up(&mut hud, 50, 50, LEFT));
            assert_eq!(clicked(&mut hud), []);
            assert_eq!(hud.hovered(), None);
        }
    }

    #[test]
    fn a_disabled_node_blocks_and_hovers_but_never_activates() {
        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 100, 100));
        let off = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        turn_on(&mut hud, off, Prop::Hoverable);
        turn_on(&mut hud, off, Prop::ActivateOnPress);
        turn_off(&mut hud, off, Prop::Enabled);
        lay_out(&mut hud);

        assert!(move_to(&mut hud, 10, 10));
        assert_eq!(hud.hovered(), Some(off));
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node: off,
                entered: true
            }]
        );
        assert!(down(&mut hud, 10, 10, LEFT));
        assert!(!hud.shows_pressed(off));
        assert!(up(&mut hud, 10, 10, LEFT));
        assert!(wheel(&mut hud, 10, 10, 1));
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn disabled_by_an_ancestor_or_since_the_layout_never_activates() {
        let mut hud = new_hud();
        let panel = root(&mut hud, Layer::Panels, NodeKind::Panel, (0, 0, 100, 100));
        let node = child(&mut hud, panel, NodeKind::Button, (0, 0, 50, 50));
        lay_out(&mut hud);
        turn_off(&mut hud, panel, Prop::Enabled);
        assert_eq!(tap(&mut hud, 10, 10), []);
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 10, 10), []);
        assert_eq!(hud.hovered(), Some(node));
    }

    #[test]
    fn removed_or_hidden_since_the_layout_receives_nothing() {
        let mut hud = new_hud();
        let gone = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let panel = root(&mut hud, Layer::Panels, NodeKind::Panel, (100, 0, 50, 50));
        child(&mut hud, panel, NodeKind::Button, (0, 0, 50, 50));
        let unseen = button(&mut hud, Layer::Panels, (200, 0, 50, 50));
        turn_off(&mut hud, unseen, Prop::Visible);
        lay_out(&mut hud);

        hud.remove(gone).unwrap();
        turn_off(&mut hud, panel, Prop::Visible);
        turn_on(&mut hud, unseen, Prop::Visible);
        for x in [10, 110, 210] {
            assert!(!down(&mut hud, x, 10, LEFT), "at {x}");
            assert!(!up(&mut hud, x, 10, LEFT), "at {x}");
        }
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn a_click_is_a_release_over_the_same_node_and_carries_the_command() {
        let mut hud = new_hud();
        let one = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        button(&mut hud, Layer::Panels, (100, 0, 50, 50));
        hud.set(one, Prop::Command, Value::Text("first".into()))
            .unwrap();
        lay_out(&mut hud);

        for button in [LEFT, RIGHT] {
            assert!(down(&mut hud, 10, 10, button));
            assert_eq!(hud.drain_events(), []);
            assert!(up(&mut hud, 20, 20, button));
            let command = Some("first".to_owned());
            assert_eq!(
                hud.drain_events(),
                [HudEvent::Click {
                    node: one,
                    button,
                    command
                }]
            );
        }
        down(&mut hud, 10, 10, LEFT);
        assert!(up(&mut hud, 110, 10, LEFT));
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn activate_on_press_clicks_once() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        turn_on(&mut hud, node, Prop::ActivateOnPress);
        lay_out(&mut hud);
        for button in [LEFT, RIGHT] {
            assert!(down(&mut hud, 10, 10, button));
            assert_eq!(clicked(&mut hud), [node]);
            assert!(up(&mut hud, 10, 10, button));
            assert_eq!(clicked(&mut hud), []);
        }
    }

    #[test]
    fn a_world_drag_released_over_a_panel_stays_the_worlds() {
        let mut hud = new_hud();
        let panel = blocker(&mut hud, Layer::Panels, (0, 0, 100, 100));
        let node = child(&mut hud, panel, NodeKind::Button, (0, 0, 20, 20));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);

        assert!(!down(&mut hud, 300, 300, LEFT));
        assert!(!move_to(&mut hud, 50, 50));
        assert!(!move_to(&mut hud, 10, 10));
        assert_eq!(hud.hovered(), None);
        assert!(!hud.shows_pressed(node));
        assert!(!up(&mut hud, 10, 10, LEFT));
        // The drag is over, so the node may hover, but nothing was activated.
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node,
                entered: true
            }]
        );
        assert!(move_to(&mut hud, 50, 50));
    }

    #[test]
    fn a_press_on_the_hud_keeps_moves_and_release_from_the_world() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        assert!(down(&mut hud, 10, 10, LEFT));
        assert!(move_to(&mut hud, 300, 300));
        assert!(!hud.shows_pressed(node));
        assert!(up(&mut hud, 300, 300, LEFT));
        assert_eq!(clicked(&mut hud), []);
        assert!(!move_to(&mut hud, 300, 300));
    }

    #[test]
    fn each_button_has_its_own_capture() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        assert!(down(&mut hud, 10, 10, RIGHT));
        assert!(!hud.shows_pressed(node));
        assert!(!down(&mut hud, 300, 300, LEFT));
        assert!(!move_to(&mut hud, 10, 10), "the world's drag goes on");
        assert!(up(&mut hud, 10, 10, RIGHT));
        assert_eq!(clicked(&mut hud), [node]);
        assert!(!up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn an_unseen_press_has_its_release_in_the_world() {
        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        assert!(!up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn shows_pressed_follows_the_pointer_and_enabled() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        down(&mut hud, 10, 10, LEFT);
        assert!(hud.shows_pressed(node));
        move_to(&mut hud, 90, 90);
        assert!(!hud.shows_pressed(node));
        move_to(&mut hud, 10, 10);
        assert!(hud.shows_pressed(node));
        turn_off(&mut hud, node, Prop::Enabled);
        assert!(!hud.shows_pressed(node));
    }

    #[test]
    fn a_node_removed_hidden_or_disabled_while_held_activates_nothing() {
        for change in 0..4 {
            let mut hud = new_hud();
            let panel = root(&mut hud, Layer::Panels, NodeKind::Panel, (0, 0, 100, 100));
            let node = child(&mut hud, panel, NodeKind::Button, (0, 0, 50, 50));
            turn_on(&mut hud, node, Prop::Hoverable);
            lay_out(&mut hud);
            assert!(down(&mut hud, 10, 10, LEFT));
            hud.drain_events();

            match change {
                0 => hud.remove(node).unwrap(),
                1 => hud.remove(panel).unwrap(),
                2 => turn_off(&mut hud, panel, Prop::Visible),
                _ => turn_off(&mut hud, node, Prop::Enabled),
            }
            assert!(!hud.shows_pressed(node));
            if change < 3 {
                assert_eq!(hud.hovered(), None);
                assert_eq!(
                    hud.drain_events(),
                    [HudEvent::Hover {
                        node,
                        entered: false
                    }]
                );
            } else {
                assert_eq!(hud.hovered(), Some(node));
            }
            assert!(move_to(&mut hud, 10, 10));
            assert!(up(&mut hud, 10, 10, LEFT), "the world never saw the press");
            assert_eq!(clicked(&mut hud), []);
        }
    }

    #[test]
    fn hidden_and_shown_again_while_held_does_not_click() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        down(&mut hud, 10, 10, LEFT);
        turn_off(&mut hud, node, Prop::Visible);
        turn_on(&mut hud, node, Prop::Visible);
        assert!(up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn hover_is_reported_for_hoverable_nodes_only() {
        let mut hud = new_hud();
        let plain = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let node = button(&mut hud, Layer::Panels, (100, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);

        move_to(&mut hud, 10, 10);
        assert_eq!(hud.hovered(), Some(plain));
        assert_eq!(hud.drain_events(), []);
        move_to(&mut hud, 110, 10);
        move_to(&mut hud, 120, 10);
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node,
                entered: true
            }]
        );
        move_to(&mut hud, 10, 10);
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node,
                entered: false
            }]
        );
    }

    #[test]
    fn cancel_ends_captures_and_hover_without_activation() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);
        down(&mut hud, 10, 10, LEFT);
        hud.drain_events();

        assert!(!hud.input(Pointer::Cancel).consumed);
        assert_eq!(hud.hovered(), None);
        assert!(!hud.shows_pressed(node));
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node,
                entered: false
            }]
        );
        assert!(!move_to(&mut hud, 300, 300), "no capture is left");
        assert!(!up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn the_wheel_goes_to_the_node_hit() {
        let mut hud = new_hud();
        let panel = blocker(&mut hud, Layer::Panels, (0, 0, 100, 100));
        let node = child(&mut hud, panel, NodeKind::Button, (0, 0, 20, 20));
        lay_out(&mut hud);
        assert!(wheel(&mut hud, 50, 50, 2));
        assert!(wheel(&mut hud, 10, 10, -1));
        assert!(!wheel(&mut hud, 300, 300, 5));
        let want = [
            HudEvent::Wheel {
                node: panel,
                lines: 2,
            },
            HudEvent::Wheel { node, lines: -1 },
        ];
        assert_eq!(hud.drain_events(), want);
    }

    #[test]
    fn a_modal_blocks_the_layers_beneath_and_the_world_everywhere() {
        let mut hud = new_hud();
        let low = button(&mut hud, Layer::Overlay, (0, 0, 50, 50));
        let modal = root(&mut hud, Layer::Modal, NodeKind::Panel, (200, 200, 50, 50));
        let answer = child(&mut hud, modal, NodeKind::Button, (0, 0, 20, 20));
        lay_out(&mut hud);

        for (x, y) in [(10, 10), (400, 300)] {
            assert!(move_to(&mut hud, x, y));
            assert!(wheel(&mut hud, x, y, 1));
            assert!(down(&mut hud, x, y, RIGHT));
            assert!(up(&mut hud, x, y, RIGHT));
            assert_eq!(hud.hovered(), None);
        }
        assert_eq!(hud.drain_events(), []);
        assert_eq!(tap(&mut hud, 210, 210), [answer]);

        turn_off(&mut hud, modal, Prop::Visible);
        assert_eq!(tap(&mut hud, 10, 10), [low], "hidden since the layout");
    }

    #[test]
    fn a_modal_that_appeared_takes_a_world_drags_moves_but_not_its_release() {
        let mut hud = new_hud();
        lay_out(&mut hud);
        assert!(!down(&mut hud, 10, 10, LEFT));
        button(&mut hud, Layer::Modal, (100, 100, 50, 50));
        lay_out(&mut hud);
        assert!(move_to(&mut hud, 110, 110));
        assert!(!up(&mut hud, 110, 110, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn a_modal_that_appeared_stops_a_click_beneath_it() {
        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        assert!(down(&mut hud, 10, 10, LEFT));
        root(&mut hud, Layer::Modal, NodeKind::Panel, (100, 100, 50, 50));
        lay_out(&mut hud);
        assert!(up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn system_works_over_a_modal_and_blocks_only_where_hit() {
        let mut hud = new_hud();
        let low = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let system = root(&mut hud, Layer::System, NodeKind::Panel, (0, 0, 500, 400));
        let urgent = child(&mut hud, system, NodeKind::Button, (100, 100, 50, 50));
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 10, 10), [low]);
        assert!(!move_to(&mut hud, 300, 300));

        blocker(&mut hud, Layer::Modal, (90, 90, 100, 100));
        lay_out(&mut hud);
        assert_eq!(tap(&mut hud, 110, 110), [urgent]);
        assert_eq!(tap(&mut hud, 10, 10), []);
    }

    #[test]
    fn a_press_outside_dismisses_and_is_consumed() {
        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let popup = blocker(&mut hud, Layer::Panels, (100, 100, 50, 50));
        turn_on(&mut hud, popup, Prop::Dismissable);
        // Beyond the popup's own rect, yet part of it.
        let wing = child(&mut hud, popup, NodeKind::Button, (80, 0, 20, 20));
        lay_out(&mut hud);

        for (x, y) in [(10, 10), (400, 300)] {
            assert!(down(&mut hud, x, y, LEFT));
            assert!(up(&mut hud, x, y, LEFT));
            assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node: popup }]);
        }
        assert_eq!(tap(&mut hud, 190, 110), [wing]);
        down(&mut hud, 120, 120, LEFT);
        up(&mut hud, 120, 120, LEFT);
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn a_press_inside_a_plain_dismissable_is_not_outside_it() {
        let mut hud = new_hud();
        let popup = root(&mut hud, Layer::Panels, NodeKind::Panel, (100, 100, 50, 50));
        turn_on(&mut hud, popup, Prop::Dismissable);
        lay_out(&mut hud);
        assert!(!down(&mut hud, 120, 120, LEFT));
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn only_the_topmost_dismissable_is_dismissed() {
        let mut hud = new_hud();
        let outer = blocker(&mut hud, Layer::Panels, (0, 0, 100, 100));
        turn_on(&mut hud, outer, Prop::Dismissable);
        let inner = child(&mut hud, outer, NodeKind::Panel, (0, 0, 20, 20));
        turn_on(&mut hud, inner, Prop::Dismissable);
        lay_out(&mut hud);
        assert!(down(&mut hud, 60, 60, LEFT));
        assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node: inner }]);

        turn_off(&mut hud, inner, Prop::Visible);
        assert!(escape(&mut hud));
        assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node: outer }]);
    }

    #[test]
    fn a_hit_in_a_higher_layer_takes_the_press_without_a_dismiss() {
        let mut hud = new_hud();
        let popup = blocker(&mut hud, Layer::Panels, (100, 100, 50, 50));
        turn_on(&mut hud, popup, Prop::Dismissable);
        let over = button(&mut hud, Layer::Overlay, (0, 0, 50, 50));
        lay_out(&mut hud);
        down(&mut hud, 10, 10, LEFT);
        up(&mut hud, 10, 10, LEFT);
        let want = HudEvent::Click {
            node: over,
            button: LEFT,
            command: None,
        };
        assert_eq!(hud.drain_events(), [want]);
    }

    #[test]
    fn escape_is_consumed_unless_the_dismissable_is_external() {
        let cases = [
            (Origin::Host, true),
            (Origin::Layout, true),
            (Origin::Player, true),
            (Origin::External(3), false),
        ];
        for (origin, consumed) in cases {
            let mut hud = new_hud();
            let node = hud
                .create_root(Layer::Overlay, origin, NodeKind::Panel)
                .unwrap();
            turn_on(&mut hud, node, Prop::Dismissable);
            lay_out(&mut hud);
            assert_eq!(escape(&mut hud), consumed, "{origin:?}");
            assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node }]);
        }
    }

    #[test]
    fn nothing_beneath_a_modal_is_dismissed() {
        let mut hud = new_hud();
        let popup = root(&mut hud, Layer::Overlay, NodeKind::Panel, (0, 0, 50, 50));
        turn_on(&mut hud, popup, Prop::Dismissable);
        let modal = root(&mut hud, Layer::Modal, NodeKind::Panel, (100, 100, 50, 50));
        lay_out(&mut hud);
        assert!(escape(&mut hud), "the modal keeps it from the world");
        assert!(down(&mut hud, 300, 300, LEFT));
        assert_eq!(hud.drain_events(), []);

        turn_on(&mut hud, modal, Prop::Dismissable);
        assert!(escape(&mut hud));
        assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node: modal }]);
    }

    #[test]
    fn an_external_popup_leaves_the_press_outside_a_host_one_dismissing() {
        let mut hud = new_hud();
        let host = blocker(&mut hud, Layer::Overlay, (0, 0, 50, 50));
        turn_on(&mut hud, host, Prop::Dismissable);
        let client = hud
            .create_root(Layer::Overlay, Origin::External(3), NodeKind::Panel)
            .unwrap();
        place(&mut hud, client, (0, 0, 500, 400));
        turn_on(&mut hud, client, Prop::BlocksPointer);
        turn_on(&mut hud, client, Prop::Dismissable);
        lay_out(&mut hud);
        assert!(down(&mut hud, 300, 300, LEFT));
        assert_eq!(hud.drain_events(), [HudEvent::Dismiss { node: host }]);
        assert!(up(&mut hud, 300, 300, LEFT));

        place(&mut hud, client, (100, 0, 50, 50));
        lay_out(&mut hud);
        assert!(down(&mut hud, 300, 300, LEFT));
        let both = [client, host].map(|node| HudEvent::Dismiss { node });
        assert_eq!(hud.drain_events(), both);
    }

    #[test]
    fn a_change_of_hover_or_pressed_moves_the_revision() {
        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        let steps = [
            (Pointer::Move { x: 90, y: 90 }, false),
            (Pointer::Move { x: 10, y: 10 }, true),
            (Pointer::Move { x: 20, y: 20 }, false),
            (
                Pointer::Down {
                    x: 20,
                    y: 20,
                    button: LEFT,
                },
                true,
            ),
            (Pointer::Move { x: 90, y: 90 }, true),
            (Pointer::Move { x: 20, y: 20 }, true),
            (Pointer::Cancel, true),
        ];
        for (event, changes) in steps {
            let before = hud.revision();
            hud.input(event);
            assert_eq!(hud.revision() != before, changes, "{event:?}");
        }
    }

    #[test]
    fn a_layout_tracks_a_still_pointer_until_cancel() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (100, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);
        move_to(&mut hud, 10, 10);
        place(&mut hud, node, (0, 0, 50, 50));
        lay_out(&mut hud);
        let hover = |entered| [HudEvent::Hover { node, entered }];
        assert_eq!(hud.drain_events(), hover(true));
        down(&mut hud, 10, 10, LEFT);
        assert!(hud.shows_pressed(node));

        let modal = blocker(&mut hud, Layer::Modal, (200, 200, 50, 50));
        let before = hud.revision();
        lay_out(&mut hud);
        assert_ne!(hud.revision(), before);
        assert!(!hud.shows_pressed(node));
        assert_eq!(hud.drain_events(), hover(false));

        hud.input(Pointer::Cancel);
        turn_off(&mut hud, modal, Prop::Visible);
        lay_out(&mut hud);
        assert_eq!(hud.hovered(), None);
    }

    #[test]
    fn a_field_node_that_only_hovers_passes_input_to_the_world() {
        let mut hud = new_hud();
        let node = root(&mut hud, Layer::Field, NodeKind::Image, (0, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);
        assert!(!move_to(&mut hud, 10, 10));
        assert!(!wheel(&mut hud, 10, 10, 1));
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Hover {
                node,
                entered: true
            }]
        );
        assert!(!down(&mut hud, 10, 10, LEFT));
        assert!(!up(&mut hud, 10, 10, LEFT));
        turn_on(&mut hud, node, Prop::BlocksPointer);
        assert!(down(&mut hud, 10, 10, LEFT));
    }

    #[test]
    fn no_longer_clickable_or_hoverable_drops_press_and_hover() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);
        down(&mut hud, 10, 10, LEFT);
        hud.drain_events();

        turn_off(&mut hud, node, Prop::Clickable);
        assert!(!hud.shows_pressed(node));
        assert_eq!(hud.hovered(), Some(node));
        turn_off(&mut hud, node, Prop::Hoverable);
        assert_eq!(hud.hovered(), None);
        let left = HudEvent::Hover {
            node,
            entered: false,
        };
        assert_eq!(hud.drain_events(), [left]);
        turn_on(&mut hud, node, Prop::Clickable);
        assert!(up(&mut hud, 10, 10, LEFT));
        assert_eq!(clicked(&mut hud), []);
    }

    #[test]
    fn a_command_has_the_text_limit() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        let most = crate::hud::world::MAX_TEXT_BYTES;
        let mut set = |len| hud.set(node, Prop::Command, Value::Text("c".repeat(len)));
        assert_eq!(set(most), Ok(()));
        assert_eq!(set(most + 1), Err(crate::node::Refused::Limit));
    }

    #[test]
    fn the_queue_is_bounded() {
        let mut hud = new_hud();
        let node = button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        turn_on(&mut hud, node, Prop::Hoverable);
        lay_out(&mut hud);
        for _ in 0..MAX_EVENTS * 2 {
            assert!(down(&mut hud, 10, 10, LEFT));
            assert!(up(&mut hud, 10, 10, LEFT));
        }
        let events = hud.drain_events();
        assert_eq!(events.len(), MAX_EVENTS);
        assert_eq!(
            events[0],
            HudEvent::Hover {
                node,
                entered: true
            }
        );

        for _ in 0..MAX_EVENTS * 2 {
            down(&mut hud, 10, 10, LEFT);
            up(&mut hud, 10, 10, LEFT);
        }
        move_to(&mut hud, 300, 300);
        let events = hud.drain_events();
        assert_eq!(events.len(), MAX_EVENTS + 1, "hover is kept when full");
        assert_eq!(
            events.last(),
            Some(&HudEvent::Hover {
                node,
                entered: false
            })
        );
    }

    #[test]
    fn waiting_hover_keeps_only_the_net_change() {
        let mut hud = new_hud();
        let nodes = [0, 100, 200].map(|x| {
            let id = button(&mut hud, Layer::Panels, (x, 0, 50, 50));
            turn_on(&mut hud, id, Prop::Hoverable);
            id
        });
        lay_out(&mut hud);
        move_to(&mut hud, 10, 10);
        hud.drain_events();
        for _ in 0..1000 {
            for x in [110, 300, 10, 210] {
                move_to(&mut hud, x, 10);
            }
        }
        let want = [
            HudEvent::Hover {
                node: nodes[0],
                entered: false,
            },
            HudEvent::Hover {
                node: nodes[2],
                entered: true,
            },
        ];
        assert_eq!(hud.drain_events(), want);
    }

    #[test]
    fn waiting_wheel_turns_add_up_without_overflow() {
        let mut hud = new_hud();
        let node = blocker(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        for lines in [i32::MAX, i32::MAX, 0, -7] {
            wheel(&mut hud, 10, 10, lines);
        }
        assert_eq!(
            hud.drain_events(),
            [HudEvent::Wheel {
                node,
                lines: i32::MAX - 7
            }]
        );
        wheel(&mut hud, 10, 10, 4);
        wheel(&mut hud, 10, 10, -4);
        assert_eq!(hud.drain_events(), []);
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        let (min, max) = (i32::MIN, i32::MAX);
        assert!(contains(Rect::new(max - 1, max - 1, max, max), max, max));
        assert!(contains(Rect::new(min, min, max, max), min, min));
        assert!(!contains(Rect::new(min, min, max, max), max, max));
        assert!(!contains(Rect::new(max, max, 1, 1), min, min));
        assert!(!contains(Rect::new(0, 0, min, min), 0, 0));
        assert_eq!(
            area(Rect::new(0, 0, max, max)),
            i64::from(max) * i64::from(max)
        );

        let mut hud = new_hud();
        button(&mut hud, Layer::Panels, (0, 0, 50, 50));
        lay_out(&mut hud);
        for (x, y) in [(min, min), (max, max), (min, max), (max, min)] {
            assert!(!move_to(&mut hud, x, y));
            assert!(!wheel(&mut hud, x, y, min));
        }
    }
}
