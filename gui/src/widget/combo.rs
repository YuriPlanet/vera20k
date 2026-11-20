//! `Combo` behaviour (to be ported; see the module plan in widget/mod.rs).

use super::{Capture, Ctx};
use crate::node::NodeId;

#[derive(Debug, Default)]
pub struct ComboState {}

pub(crate) fn press(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) -> Capture {
    Capture::No
}

pub(crate) fn drag(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// Pointer input while this combo's drop-down is the dialog's popup.
pub(crate) fn popup_press(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn popup_move(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn popup_release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}
