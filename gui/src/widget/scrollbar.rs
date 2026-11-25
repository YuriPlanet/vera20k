//! `ScrollBar` behaviour: a lone vertical scrollbar on the shared scrollbar
//! art (`0x0061C690`) that pages rows its host lays out itself. `Pos` runs over `0..=Max`; `RowCount` and
//! `VisibleRows` (the rows it pages over and how many show) size the thumb.
//!
//! It acts on the press as a trackbar does: an arrow (the drop-down
//! scrollbar's arrow height at either end) steps one position, with no
//! repeat; a press on the thumb or the track takes the position under the
//! pointer and holds the mouse, the position following it until the
//! release. Each change is a `Changed` (`final_` false); the release sends
//! the final one.

use super::{Capture, Ctx};
use crate::Rect;
use crate::gui::UiEvent;
use crate::node::{Node, NodeId, Prop, Tree, Value};
use crate::shell::geom::ShellScale;
use crate::shell::list::{ListMetrics, scrollbar_thumb_y, scrollbar_top_at_pointer, thumb_height};

/// What the host paints of a `ScrollBar`: the bar, its thumb at `Pos`, and
/// whether a press on the track holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollBarView {
    pub bar: Rect,
    pub thumb: Rect,
    pub held: bool,
}

/// The node's position range and thumb height on its bar `rect`.
struct Bar {
    rect: Rect,
    pos: i32,
    max: i32,
    thumb_h: i32,
    metrics: ListMetrics,
}

fn bar(ctx: &Ctx<'_>, node: NodeId) -> Option<Bar> {
    Some(bar_of(ctx.tree.node(node).ok()?, ctx.scale))
}

fn bar_of(n: &Node, scale: ShellScale) -> Bar {
    let rect = n.rect();
    let metrics = ListMetrics::scaled(scale);
    let range = n.int_prop(Prop::RowCount, 0) - n.int_prop(Prop::VisibleRows, 0);
    let max = n.int_prop(Prop::Max, 0).max(0);
    let pos = n.int_prop(Prop::Pos, 0).clamp(0, max);
    let thumb_h = thumb_height(rect.h, range.max(0) as usize, metrics);
    Bar { rect, pos, max, thumb_h, metrics }
}

/// ScrollBar `node` as it paints at its dialog's `scale` (`0x0061C890`'s thumb
/// placement); `held` when a press holds it.
pub(crate) fn view(tree: &Tree, node: NodeId, scale: ShellScale, held: bool) -> Option<ScrollBarView> {
    let b = bar_of(tree.node(node).ok()?, scale);
    let y = scrollbar_thumb_y(b.rect, b.thumb_h, b.pos as usize, b.max as usize, b.metrics);
    Some(ScrollBarView { bar: b.rect, thumb: Rect::new(b.rect.x, y, b.rect.w, b.thumb_h), held })
}

impl Bar {
    fn pos_at(&self, y: i32) -> i32 {
        scrollbar_top_at_pointer(self.rect, self.thumb_h, self.max as usize, y, self.metrics) as i32
    }
}

fn move_to(ctx: &mut Ctx<'_>, node: NodeId, pos: i32) {
    let Some(b) = bar(ctx, node) else { return };
    let pos = pos.clamp(0, b.max);
    if pos != b.pos {
        let _ = ctx.tree.set(node, Prop::Pos, Value::Int(pos));
        ctx.events.push(UiEvent::Changed { node, value: Value::Int(pos), final_: false });
    }
}

pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, _x: i32, y: i32) -> Capture {
    let Some(b) = bar(ctx, node) else { return Capture::No };
    let arrow_h = b.metrics.arrow_h;
    if y < b.rect.y + arrow_h {
        move_to(ctx, node, b.pos - 1);
        return Capture::No;
    }
    if y >= b.rect.y + b.rect.h - arrow_h {
        move_to(ctx, node, b.pos + 1);
        return Capture::No;
    }
    move_to(ctx, node, b.pos_at(y));
    Capture::Yes
}

/// A move while the press holds it: the position under the pointer.
pub(crate) fn drag(ctx: &mut Ctx<'_>, node: NodeId, y: i32) {
    if let Some(b) = bar(ctx, node) {
        move_to(ctx, node, b.pos_at(y));
    }
}

pub(crate) fn release(ctx: &mut Ctx<'_>, node: NodeId) {
    if let Some(b) = bar(ctx, node) {
        ctx.events.push(UiEvent::Changed { node, value: Value::Int(b.pos), final_: true });
    }
}

#[cfg(test)]
mod tests {
    use crate::Rect;
    use crate::gui::{DialogKind, Gui, InputEvent, MouseButton, UiEvent};
    use crate::node::{NodeId, NodeKind, Prop, Value};

    /// A 20x200 bar at (100, 50) over 13 rows, 7 showing, in two positions.
    fn bar() -> (Gui, NodeId) {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let bar = g.create(NodeKind::ScrollBar, Some(root)).unwrap();
        g.set(bar, Prop::Rect, Value::Rect(Rect::new(100, 50, 20, 200))).unwrap();
        for (prop, v) in [(Prop::Max, 1), (Prop::RowCount, 13), (Prop::VisibleRows, 7)] {
            g.set(bar, prop, Value::Int(v)).unwrap();
        }
        g.push_dialog(root, DialogKind::Page).unwrap();
        (g, bar)
    }

    fn input(g: &mut Gui, event: InputEvent) -> Vec<UiEvent> {
        g.input(event, 0);
        g.drain_events().into_iter().filter(|e| !matches!(e, UiEvent::Hovered { .. })).collect()
    }

    fn down(x: i32, y: i32) -> InputEvent {
        InputEvent::PointerDown { x, y, button: MouseButton::Left }
    }

    fn up(x: i32, y: i32) -> InputEvent {
        InputEvent::PointerUp { x, y, button: MouseButton::Left }
    }

    #[test]
    fn arrows_step_once_and_the_track_takes_the_pointer_until_the_release() {
        let (mut g, bar) = bar();
        let changed = |pos, final_| UiEvent::Changed { node: bar, value: Value::Int(pos), final_ };
        input(&mut g, InputEvent::PointerMove { x: 110, y: 245 });
        assert_eq!(input(&mut g, down(110, 245)), vec![changed(1, false)]);
        assert_eq!((g.pressed(), g.poll(5000)), (None, None), "no hold, no repeat");
        assert_eq!(input(&mut g, down(110, 245)), vec![], "the last position clamps");
        assert_eq!(input(&mut g, down(110, 52)), vec![changed(0, false)]);
        // The track: the position under the pointer, following it.
        assert_eq!(input(&mut g, down(110, 200)), vec![changed(1, false)]);
        assert_eq!(g.pressed(), Some(bar));
        assert_eq!(input(&mut g, InputEvent::PointerMove { x: 10, y: 60 }), vec![changed(0, false)]);
        assert_eq!(input(&mut g, up(10, 60)), vec![changed(0, true)]);
        assert_eq!(g.get(bar, Prop::Pos).unwrap(), Some(&Value::Int(0)));
    }

    #[test]
    fn its_view_puts_the_thumb_at_the_position_and_shows_the_hold() {
        let (mut g, bar) = bar();
        let top = g.scrollbar_view(bar).unwrap();
        assert_eq!((top.bar, top.held), (Rect::new(100, 50, 20, 200), false));
        input(&mut g, InputEvent::PointerMove { x: 110, y: 200 });
        input(&mut g, down(110, 200));
        let held = g.scrollbar_view(bar).unwrap();
        assert!(held.held && held.thumb.y > top.thumb.y && held.thumb.h == top.thumb.h);
    }
}
