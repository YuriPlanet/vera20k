//! A HUD of black boxes and lines of text whose host already knows where
//! each one lands.
//!
//! The battlefield's text overlays (the timer lines, the message rows, the
//! tooltip) are boxes filled in one colour and texts printed with the
//! game font, each placed by the original's own arithmetic. Here each box is
//! a panel node and each text a text node of a retained `vera20k-gui` tree,
//! in drawing order and with its rect in the overlay's own pixels, so the
//! tree owns which pieces stand, their order, their colours and their
//! words. The host hands each node its exact place on the target, so the
//! instances are the ones the overlay drew before: the boxes in one lane,
//! the glyphs in another, each in drawing order.

use std::collections::HashMap;

use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect, Rgba, Value};

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, Material, NodeDraw};

/// The pieces' tree, kept from frame to frame, and the lanes it draws.
pub(crate) struct TextHud {
    hud: Hud,
    /// One node per box and text, in drawing order.
    nodes: Vec<(NodeKind, NodeId)>,
    /// How many of `nodes` this frame has reached.
    used: usize,
    /// The tree's revision at its last layout.
    laid_out: u64,
    /// Each node's place on the target this frame.
    draws: HashMap<NodeId, NodeDraw>,
    paint: HudPaint,
}

impl Default for TextHud {
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

/// Where the tree lays out: everywhere, as the host placed every piece.
const UNBOUNDED: Rect = Rect::new(i32::MIN / 2, i32::MIN / 2, i32::MAX, i32::MAX);

/// The host places every piece itself.
const SPACE: HudSpace = HudSpace {
    origin: [0.0; 2],
    scale: 1.0,
};

impl TextHud {
    /// Start a frame: the pieces added from here replace the last frame's.
    pub(crate) fn begin(&mut self) {
        self.used = 0;
        self.draws.clear();
    }

    /// The frame's next box: `rect` in the overlay's pixels, filled with
    /// `color` over `size` at `position` on the target.
    pub(crate) fn fill(&mut self, rect: Rect, position: [f32; 2], size: [f32; 2], color: Rgba) {
        let Some(node) = self.node(NodeKind::Panel, rect) else {
            return;
        };
        if !matches!(self.hud.get(node, Prop::Background), Ok(Some(Value::Rgba(shown))) if *shown == color)
        {
            let _ = self.hud.set(node, Prop::Background, Value::Rgba(color));
        }
        self.draws.insert(
            node,
            NodeDraw {
                position: Some(position),
                size: Some(size),
                ..Default::default()
            },
        );
    }

    /// The frame's next text: `rect` in the overlay's pixels, `text` printed
    /// in `color` from `position` on the target.
    pub(crate) fn text(&mut self, rect: Rect, text: &str, position: [f32; 2], color: Rgba) {
        let Some(node) = self.node(NodeKind::Text, rect) else {
            return;
        };
        if !matches!(self.hud.get(node, Prop::Color), Ok(Some(Value::Rgba(shown))) if *shown == color)
        {
            let _ = self.hud.set(node, Prop::Color, Value::Rgba(color));
        }
        // Compared first: a value to set is a string of its own.
        if !matches!(self.hud.get(node, Prop::Text), Ok(Some(Value::Text(shown))) if shown == text)
        {
            let _ = self.hud.set(node, Prop::Text, Value::Text(text.to_owned()));
        }
        self.draws.insert(
            node,
            NodeDraw {
                position: Some(position),
                ..Default::default()
            },
        );
    }

    /// End the frame: drop the pieces it no longer has and draw the rest
    /// with `font` at its raster of now.
    pub(crate) fn finish(&mut self, font: &BitFont) {
        self.forget_from(self.used);
        if self.laid_out != self.hud.revision() {
            self.hud.layout(UNBOUNDED);
            self.laid_out = self.hud.revision();
        }
        let draws = &self.draws;
        self.paint.build(&self.hud, SPACE, font, |node| {
            draws.get(&node).copied().unwrap_or_default()
        });
    }

    /// The boxes, in drawing order.
    pub(crate) fn boxes(&self) -> impl Iterator<Item = &SpriteInstance> {
        self.paint.lane(Material::Solid)
    }

    /// The glyphs, in drawing order.
    pub(crate) fn glyphs(&self) -> impl Iterator<Item = &SpriteInstance> {
        self.paint.lane(Material::Glyphs)
    }

    /// The frame's next node of `kind`: last frame's where it is one, else a
    /// new one in its place and those after it, as a newer root is on top.
    /// Its rect is never empty, which the HUD would not paint.
    fn node(&mut self, kind: NodeKind, rect: Rect) -> Option<NodeId> {
        let node = match self.nodes.get(self.used) {
            Some(&(of, node)) if of == kind => node,
            _ => {
                self.forget_from(self.used);
                let node = self
                    .hud
                    .create_root(Layer::Overlay, Origin::Host, kind)
                    .ok()?;
                self.nodes.push((kind, node));
                node
            }
        };
        self.used += 1;
        let rect = Rect::new(rect.x, rect.y, rect.w.max(1), rect.h.max(1));
        if !matches!(self.hud.get(node, Prop::Rect), Ok(Some(Value::Rect(shown))) if *shown == rect) {
            let _ = self.hud.set(node, Prop::Rect, Value::Rect(rect));
        }
        Some(node)
    }

    fn forget_from(&mut self, kept: usize) {
        for &(_, node) in self.nodes.get(kept..).unwrap_or_default() {
            let _ = self.hud.remove(node);
        }
        self.nodes.truncate(kept);
    }
}

/// An opaque colour from whole 255ths, as the overlays give theirs.
pub(crate) fn rgb(channels: [u8; 3]) -> Rgba {
    Rgba(channels[0], channels[1], channels[2], 255)
}
