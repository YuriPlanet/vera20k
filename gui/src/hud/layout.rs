//! Placing the nodes.

use crate::Rect;
use crate::node::NodeId;

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
    index: std::collections::HashMap<NodeId, usize>,
}

impl Layout {
    pub fn nodes(&self) -> &[Placed] {
        &self.nodes
    }

    pub fn placed(&self, id: NodeId) -> Option<&Placed> {
        self.index.get(&id).and_then(|i| self.nodes.get(*i))
    }
}

impl Hud {
    /// Places every visible node in `viewport`. A root's parent rect is the
    /// viewport.
    pub fn layout(&mut self, viewport: Rect) {
        todo!("layout {viewport:?}")
    }
}
