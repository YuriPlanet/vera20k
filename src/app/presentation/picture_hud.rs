//! A HUD of atlas pictures whose host already knows each quad.
//!
//! The sidebar's chrome and the bottom command bar are runs of pictures from
//! one atlas, each placed by the original's own arithmetic. Here they become
//! picture nodes of a retained `vera20k-gui` tree, one per piece, in drawing
//! order and with the piece's rect in its layout space, so the tree owns
//! which pieces stand, their order and their hit rects. Each node's
//! instance is the one its host made, handed to the adapter unchanged, so
//! every instance is what the host drew before.

use std::collections::HashMap;

use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect, Value};

use crate::render::batch::SpriteInstance;
use crate::render::hud_paint::{HudPaint, HudSpace, NodeDraw};

/// The pieces' tree, kept from frame to frame, and the instances it draws.
pub(crate) struct PictureHud {
    hud: Hud,
    /// The picture nodes, in drawing order.
    nodes: Vec<NodeId>,
    /// How many of `nodes` this frame has reached.
    used: usize,
    /// The tree's revision at its last layout.
    laid_out: u64,
    /// Each node's instance this frame.
    draws: HashMap<NodeId, NodeDraw>,
    paint: HudPaint,
}

impl Default for PictureHud {
    fn default() -> Self {
        Self {
            hud: Hud::new(usize::MAX, InputPolicy::default()),
            nodes: Vec::new(),
            used: 0,
            laid_out: 0,
            draws: HashMap::new(),
            paint: HudPaint::default(),
        }
    }
}

/// The `Art` every node names; which picture it is, its [`NodeDraw`] says.
const PICTURE: &str = "picture";

/// Where the tree lays out: everywhere, as the host placed every piece.
const UNBOUNDED: Rect = Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX);

/// The host places every quad itself.
const SPACE: HudSpace = HudSpace {
    origin: [0.0; 2],
    scale: 1.0,
};

impl PictureHud {
    /// Start a frame: the pieces [`Self::picture`] adds from here replace
    /// the last frame's.
    pub(crate) fn begin(&mut self) {
        self.used = 0;
        self.draws.clear();
    }

    /// The frame's next piece: `rect` in the host's layout space, drawn as
    /// `instance`.
    pub(crate) fn picture(&mut self, rect: Rect, instance: SpriteInstance) {
        let Some(node) = self.node(rect) else {
            return;
        };
        let draw = NodeDraw {
            instance: Some(instance),
            ..Default::default()
        };
        self.draws.insert(node, draw);
    }

    /// End the frame: drop the pieces it no longer has and draw the rest,
    /// bottom to top.
    pub(crate) fn finish(&mut self) -> &[SpriteInstance] {
        self.forget_from(self.used);
        if self.laid_out != self.hud.revision() {
            self.hud.layout(UNBOUNDED);
            self.laid_out = self.hud.revision();
        }
        let draws = &self.draws;
        self.paint.build_pictures(&self.hud, SPACE, |node| {
            draws.get(&node).copied().unwrap_or_default()
        });
        self.paint.instances()
    }

    /// The pieces' rects, bottom to top, for a host that hit-tests them.
    #[cfg(test)]
    pub(crate) fn rects(&self) -> Vec<Rect> {
        self.nodes
            .iter()
            .filter_map(|&node| match self.hud.get(node, Prop::Rect) {
                Ok(Some(Value::Rect(rect))) => Some(*rect),
                _ => None,
            })
            .collect()
    }

    /// The frame's next node, last frame's where there is one.
    fn node(&mut self, rect: Rect) -> Option<NodeId> {
        let rect = Rect::new(rect.x, rect.y, rect.w.max(1), rect.h.max(1));
        let node = match self.nodes.get(self.used) {
            Some(&node) => node,
            None => {
                let node = self
                    .hud
                    .create_root(Layer::Panels, Origin::Host, NodeKind::Image)
                    .ok()?;
                let _ = self
                    .hud
                    .set(node, Prop::Art, Value::Text(PICTURE.to_owned()));
                self.nodes.push(node);
                node
            }
        };
        self.used += 1;
        let same = matches!(self.hud.get(node, Prop::Rect), Ok(Some(Value::Rect(shown))) if *shown == rect);
        if !same {
            let _ = self.hud.set(node, Prop::Rect, Value::Rect(rect));
        }
        Some(node)
    }

    fn forget_from(&mut self, kept: usize) {
        for &node in self.nodes.get(kept..).unwrap_or_default() {
            let _ = self.hud.remove(node);
        }
        self.nodes.truncate(kept);
    }
}

/// The whole pixels a piece covers in its layout space: from the floor of
/// its corner to the ceiling of its far edge.
pub(crate) fn covering(x: f32, y: f32, w: f32, h: f32) -> Rect {
    let (left, top) = (x.floor(), y.floor());
    Rect::new(
        left as i32,
        top as i32,
        ((x + w).ceil() - left) as i32,
        ((y + h).ceil() - top) as i32,
    )
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    fn quad(x: f32) -> SpriteInstance {
        SpriteInstance {
            position: [x, 2.5],
            size: [3.0, 4.0],
            uv_origin: [0.125, 0.25],
            uv_size: [0.0625, 0.5],
            depth: 0.00048,
            tint: [1.0; 3],
            alpha: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn pieces_draw_their_quads_in_order_and_a_shorter_frame_drops_the_rest() {
        let mut hud = PictureHud::default();
        hud.begin();
        for x in [5.0, 1.0, 9.5] {
            hud.picture(covering(x, 2.5, 3.0, 4.0), quad(x));
        }
        let drawn: Vec<_> = hud.finish().iter().map(|q| q.position[0]).collect();
        assert_eq!(drawn, [5.0, 1.0, 9.5]);
        let first = hud.finish()[0];
        assert_eq!((first.size, first.depth, first.alpha), ([3.0, 4.0], 0.00048, 1.0));
        assert_eq!(hud.rects()[2], Rect::new(9, 2, 4, 5));
        hud.begin();
        hud.picture(covering(7.0, 0.0, 1.0, 1.0), quad(7.0));
        assert_eq!(hud.finish().len(), 1);
        assert_eq!(hud.rects(), [Rect::new(7, 0, 1, 1)]);
    }
}
