//! `List` behaviour (to be ported; see the module plan in widget/mod.rs).

use super::{Capture, Ctx};
use crate::node::NodeId;

#[derive(Debug, Default)]
pub struct ListState {}

pub(crate) fn press(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) -> Capture {
    Capture::No
}

pub(crate) fn drag(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// The wheel over the list; true when it scrolled.
pub(crate) fn wheel(_ctx: &mut Ctx<'_>, _node: NodeId, _lines: i32) -> bool {
    false
}

/// Timers (scroll-arrow repeat): the next time the list wants a poll.
pub(crate) fn poll(_ctx: &mut Ctx<'_>, _node: NodeId) -> Option<crate::gui::UiMs> {
    None
}
