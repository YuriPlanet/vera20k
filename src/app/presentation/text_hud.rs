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
    /// The overlay's `veraui.ini` key, and the part node of that name every
    /// piece goes into ([`Self::named`]).
    name: Option<&'static str>,
    part: Option<NodeId>,
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
            name: None,
            part: None,
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
    /// An overlay that is one part of the layout, `name` its `veraui.ini`
    /// key: every box and text is a child of the part node of that name, so
    /// the part can be found and hidden or restyled as one. The part draws
    /// nothing itself; its pieces keep their places and order.
    pub(crate) fn named(name: &'static str) -> Self {
        Self {
            name: Some(name),
            ..Self::default()
        }
    }

    /// Start a frame: the pieces added from here replace the last frame's.
    pub(crate) fn begin(&mut self) {
        if let (Some(name), None) = (self.name, self.part)
            && let Ok(part) = self
                .hud
                .create_root(Layer::Overlay, Origin::Host, NodeKind::Panel)
        {
            let _ = self.hud.set(part, Prop::Name, Value::Text(name.to_owned()));
            self.part = Some(part);
        }
        self.used = 0;
        self.draws.clear();
    }

    /// The frame's next box: `rect` in the overlay's pixels, filled with
    /// `color` over `size` at `position` on the target, at `depth`.
    pub(crate) fn fill(
        &mut self,
        rect: Rect,
        position: [f32; 2],
        size: [f32; 2],
        color: Rgba,
        depth: f32,
    ) {
        self.fill_tinted(rect, position, size, color, depth, None);
    }

    /// [`Self::fill`] in the host's exact `tint`, where `color` (the node's
    /// prop) only rounds it.
    pub(crate) fn fill_tinted(
        &mut self,
        rect: Rect,
        position: [f32; 2],
        size: [f32; 2],
        color: Rgba,
        depth: f32,
        tint: Option<[f32; 3]>,
    ) {
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
                depth,
                tint,
                ..Default::default()
            },
        );
    }

    /// The frame's next text: `rect` in the overlay's pixels, `text` printed
    /// in `color` from `position` on the target; a label revealing `reveal`
    /// of its units at `depth`, or plain at depth 0.
    pub(crate) fn text(
        &mut self,
        rect: Rect,
        text: &str,
        position: [f32; 2],
        color: Rgba,
        label: Option<(f32, u32)>,
    ) {
        self.text_tinted(rect, text, position, color, label.map_or(0.0, |(depth, _)| depth), label.map(|(_, count)| count), None);
    }

    /// A text at `depth`, revealing `reveal` of its units when a label, in
    /// the host's exact `tint` where `color` only rounds it.
    pub(crate) fn text_tinted(
        &mut self,
        rect: Rect,
        text: &str,
        position: [f32; 2],
        color: Rgba,
        depth: f32,
        reveal: Option<u32>,
        tint: Option<[f32; 3]>,
    ) {
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
                depth,
                reveal,
                tint,
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
                let node = match self.part {
                    Some(part) => self.hud.create(kind, part, Origin::Host).ok()?,
                    None => self
                        .hud
                        .create_root(Layer::Overlay, Origin::Host, kind)
                        .ok()?,
                };
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

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    #[test]
    fn a_named_overlay_is_one_part_that_hides_as_one() {
        let font = crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 6)], 4);
        let mut hud = TextHud::named("Timers");
        let frame = |hud: &mut TextHud| {
            hud.begin();
            hud.fill(Rect::new(0, 0, 4, 4), [0.5, 0.0], [4.0, 4.0], rgb([0; 3]), 0.0);
            hud.text(Rect::new(1, 1, 6, 8), "a", [1.5, 1.0], rgb([255; 3]), None);
            hud.finish(&font);
            (hud.boxes().count(), hud.glyphs().count())
        };
        assert_eq!(frame(&mut hud), (1, 1));
        let part = hud.hud.find(Origin::Host, "Timers").unwrap();
        assert_eq!(frame(&mut hud), (1, 1), "a frame keeps the one part");
        hud.hud.set(part, Prop::Visible, Value::Bool(false)).unwrap();
        assert_eq!(frame(&mut hud), (0, 0));
    }
}

/// The nearest opaque colour to a tint, for a node's prop.
pub(crate) fn rounded(tint: [f32; 3]) -> Rgba {
    let [r, g, b] = tint.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    Rgba(r, g, b, 255)
}

/// An opaque colour from whole 255ths, as the overlays give theirs.
pub(crate) fn rgb(channels: [u8; 3]) -> Rgba {
    Rgba(channels[0], channels[1], channels[2], 255)
}
