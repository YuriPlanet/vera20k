//! `Edit` behaviour (to be ported; see the module plan in widget/mod.rs).

use super::{Capture, Ctx};
use crate::node::NodeId;

#[derive(Debug, Default)]
pub struct EditState {}

pub(crate) fn press(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) -> Capture {
    Capture::No
}

pub(crate) fn drag(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

pub(crate) fn release(_ctx: &mut Ctx<'_>, _node: NodeId, _x: i32, _y: i32) {}

/// A key while the edit has the focus; true when consumed.
pub(crate) fn key(_ctx: &mut Ctx<'_>, _node: NodeId, _key: crate::gui::Key) -> bool {
    false
}

/// Typed text while the edit has the focus.
pub(crate) fn text(_ctx: &mut Ctx<'_>, _node: NodeId, _text: &str) {}
