//! Pointer and Escape routing over the last layout.

use crate::gui::MouseButton;
use crate::node::NodeId;

use super::Hud;

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
    Move { x: i32, y: i32 },
    Down { x: i32, y: i32, button: MouseButton },
    Up { x: i32, y: i32, button: MouseButton },
    Wheel { x: i32, y: i32, lines: i32 },
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
    Click { node: NodeId, button: MouseButton, command: Option<String> },
    Hover { node: NodeId, entered: bool },
    Wheel { node: NodeId, lines: i32 },
    Dismiss { node: NodeId },
}

pub(crate) struct State {
    policy: InputPolicy,
    events: Vec<HudEvent>,
}

impl State {
    pub fn new(policy: InputPolicy) -> Self {
        State { policy, events: Vec::new() }
    }
}

impl Hud {
    pub fn input(&mut self, event: Pointer) -> Response {
        todo!("route {event:?} by {:?}", self.input.policy)
    }

    pub fn drain_events(&mut self) -> Vec<HudEvent> {
        std::mem::take(&mut self.input.events)
    }

    pub fn hovered(&self) -> Option<NodeId> {
        todo!("hovered")
    }

    pub fn shows_pressed(&self, id: NodeId) -> bool {
        todo!("pressed {id:?}")
    }

    /// Drops hover and capture of nodes now removed, hidden or disabled.
    pub(crate) fn forget_inactive(&mut self) {
        todo!("forget inactive")
    }
}
