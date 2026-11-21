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

use vera20k_gui::{Layer, Origin, Rect, Rgba};

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::{BitFont, Fit};
use crate::render::hud_paint::{HudSpace, Material, NodeDraw};
use crate::render::retained_hud::{Rest, RetainedHud};
pub(crate) use crate::render::retained_hud::{rgb, rounded};

/// The pieces' tree, kept from frame to frame, and the lanes it draws.
pub(crate) struct TextHud {
    nodes: RetainedHud,
    /// The overlay's `veraui.ini` key, and the part node of that name every
    /// piece goes into ([`Self::named`]).
    name: Option<&'static str>,
}

impl Default for TextHud {
    fn default() -> Self {
        Self {
            nodes: RetainedHud::new(Layer::Overlay, Origin::Host, usize::MAX, 1, Rest::Remove),
            name: None,
        }
    }
}

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
        self.nodes.begin();
        if let Some(name) = self.name {
            self.nodes.part(Some(name));
        }
    }

    /// Start the part `name` (a `veraui.ini` key, the node's `Name`): the
    /// pieces added from here until the next part are its children, so the
    /// part can be found by its name and hidden or restyled as one. A part
    /// draws nothing itself; its pieces keep their places and order.
    pub(crate) fn part(&mut self, name: &'static str) {
        self.nodes.part(Some(name));
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
        let draw = NodeDraw {
            position: Some(position),
            size: Some(size),
            depth,
            tint,
            ..Default::default()
        };
        self.nodes.fill(rect, color, draw);
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
        let draw = NodeDraw {
            position: Some(position),
            depth,
            reveal,
            tint,
            ..Default::default()
        };
        self.nodes.text(rect, text, color, draw);
    }

    /// The frame's next text of the game font: `rect` in the overlay's
    /// pixels, `text` printed from `position` on the target in the host's
    /// exact `tint` at `depth`, squeezed as `fit` says ([`BitFont::fitted`]).
    pub(crate) fn fitted_text(
        &mut self,
        rect: Rect,
        text: &str,
        position: [f32; 2],
        tint: [f32; 3],
        depth: f32,
        fit: Fit,
    ) {
        let draw = NodeDraw {
            fit,
            position: Some(position),
            depth,
            tint: Some(tint),
            ..Default::default()
        };
        self.nodes.text(rect, text, rounded(tint), draw);
    }

    /// End the frame: drop the pieces it no longer has and draw the rest
    /// with `font` at its raster of now.
    pub(crate) fn finish(&mut self, font: &BitFont) {
        self.nodes.finish(SPACE, Some(font));
    }

    /// The boxes, in drawing order.
    pub(crate) fn boxes(&self) -> impl Iterator<Item = &SpriteInstance> {
        self.nodes.painted().lane(Material::Solid)
    }

    /// The glyphs, in drawing order.
    pub(crate) fn glyphs(&self) -> impl Iterator<Item = &SpriteInstance> {
        self.nodes.painted().lane(Material::Glyphs)
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
        let part = hud.nodes.find("Timers").unwrap();
        assert_eq!(frame(&mut hud), (1, 1), "a frame keeps the one part");
        hud.nodes
            .hud_mut()
            .set(part, vera20k_gui::Prop::Visible, vera20k_gui::Value::Bool(false))
            .unwrap();
        assert_eq!(frame(&mut hud), (0, 0));
    }
}
