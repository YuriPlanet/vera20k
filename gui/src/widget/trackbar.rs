//! `Trackbar` behaviour: the shell's subclassed `msctls_trackbar32`
//! (`ShellTrackbar__WndProc` `0x0061D950`) on the arithmetic in
//! [`crate::shell::trackbar`].
//!
//! Every press on the window takes the capture until the release; a press on
//! the thumb drags it, one beside it jumps once. Each move is a `Changed`
//! (`final_` false); the release sends the final one.
//!
//! Node data: `Rect`, `Min`, `Max`, `Step`, `Pos`, `ShowValue` (the value
//! plaque, on unless false) and `ScaleScreen` (the window a stretched
//! trackbar hits in).

use super::Ctx;
use crate::gui::UiEvent;
use crate::node::{Node, NodeId, Prop, Value};
use crate::shell::geom::ShellScale;
use crate::shell::trackbar::{TrackbarPress, TrackbarRange};

/// The trackbar's range; without its value plaque the track runs to the
/// window's right end (`0x4AC` with 0).
pub(crate) fn range(n: &Node) -> TrackbarRange {
    TrackbarRange::new(
        n.int_prop(Prop::Min, 0),
        n.int_prop(Prop::Max, 100),
        n.int_prop(Prop::Step, 1),
    )
    .with_plaque(n.bool_prop(Prop::ShowValue, true))
}

/// The scale trackbar `n` hits at: its `ScaleScreen`'s, else its dialog's.
fn scale_of(ctx: &Ctx<'_>, n: &Node) -> ShellScale {
    match n.props.get(&Prop::ScaleScreen) {
        Some(Value::Rect(r)) => ctx.base_scale.in_window(r.w, r.h),
        _ => ctx.scale,
    }
}

/// A left press: none when it missed the window, else whether the press
/// drags the thumb (a jump beside it moves it once, no drag).
pub(crate) fn press(ctx: &mut Ctx<'_>, node: NodeId, x: i32, y: i32) -> Option<bool> {
    let n = ctx.tree.node(node).ok()?;
    let range = range(n);
    let value = n.int_prop(Prop::Pos, range.min);
    let rect = n.rect();
    // On Android the area above the thumb also admits a drag.
    let touch = cfg!(target_os = "android");
    match range.press_in(value, x, y, rect, scale_of(ctx, n)) {
        TrackbarPress::Miss => None,
        TrackbarPress::Hold => Some(touch),
        TrackbarPress::Drag => Some(true),
        TrackbarPress::Jump(to) => {
            // A press beside the thumb jumps once; it does not start a drag
            // (a track press that drags is hard to control on a touchpad).
            move_to(ctx, node, to);
            Some(false)
        }
    }
}

/// A move while the press drags the thumb: the value under the pointer.
pub(crate) fn drag(ctx: &mut Ctx<'_>, node: NodeId, x: i32) {
    let Ok(n) = ctx.tree.node(node) else { return };
    let to = range(n).value_at_in(x, n.rect(), scale_of(ctx, n));
    move_to(ctx, node, to);
}

/// The release reports the final value.
pub(crate) fn release(ctx: &mut Ctx<'_>, node: NodeId) {
    let Ok(n) = ctx.tree.node(node) else { return };
    let value = Value::Int(n.int_prop(Prop::Pos, 0));
    ctx.events.push(UiEvent::Changed {
        node,
        value,
        final_: true,
    });
}

fn move_to(ctx: &mut Ctx<'_>, node: NodeId, to: i32) {
    let from = ctx
        .tree
        .node(node)
        .map_or(to, |n| n.int_prop(Prop::Pos, range(n).min));
    if from != to {
        let _ = ctx.tree.set(node, Prop::Pos, Value::Int(to));
        ctx.events.push(UiEvent::Changed {
            node,
            value: Value::Int(to),
            final_: false,
        });
    }
}

/// The wheel over the trackbar: one step a notch, positive lines up the range.
#[cfg(target_os = "android")]
pub(crate) fn wheel(ctx: &mut Ctx<'_>, node: NodeId, lines: i32) -> bool {
    let Ok(n) = ctx.tree.node(node) else {
        return false;
    };
    let range = range(n);
    if range.max <= range.min {
        return false;
    }
    let pos = n.int_prop(Prop::Pos, range.min);
    let step = range.step.max(1).saturating_mul(lines);
    let to = (pos.saturating_add(step)).clamp(range.min, range.max);
    if to == pos {
        return false;
    }
    move_to(ctx, node, to);
    release(ctx, node);
    true
}
