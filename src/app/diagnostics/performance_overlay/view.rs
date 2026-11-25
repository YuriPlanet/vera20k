//! The panel as a HUD tree in window pixels. The nodes stay from frame to
//! frame and change only where a part does. At a match zoom a part falls
//! between whole pixels, so each also keeps its exact place.

use vera20k_gui::{Layer, Origin, Rect};

use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, NodeDraw};
use crate::render::retained_hud::{Rest, RetainedHud, rgb};
use crate::render::text_engine::Fit;

/// A track and a fill for each of 1,024 cores, with room for the rows.
const MAX_NODES: usize = 4096;
/// The boxes and bars lie under all the text, so each takes one draw: two
/// runs, each under a part of its own.
const SOLIDS: usize = 0;
const TEXTS: usize = 1;

pub(super) struct PanelView {
    nodes: RetainedHud,
    /// Window pixels per font pixel of the parts since [`Self::begin`].
    scale: f32,
}

impl PanelView {
    pub(super) fn new() -> Self {
        Self {
            nodes: RetainedHud::new(Layer::Field, Origin::Host, MAX_NODES, 2, Rest::Hide),
            scale: 1.0,
        }
    }

    pub(super) fn begin(&mut self, scale: f32) {
        self.scale = scale;
        self.nodes.begin();
        for run in [SOLIDS, TEXTS] {
            self.nodes.select(run);
            self.nodes.part(None);
        }
    }

    pub(super) fn solid(&mut self, at: [f32; 2], size: [f32; 2], color: [u8; 3]) {
        let draw = NodeDraw {
            position: Some(at),
            size: Some(size),
            ..Default::default()
        };
        self.nodes.select(SOLIDS);
        self.nodes.fill(whole(at, size), rgb(color), draw);
    }

    pub(super) fn text(
        &mut self,
        at: [f32; 2],
        size: [f32; 2],
        text: &str,
        color: [u8; 3],
        fit: Fit,
    ) {
        let draw = NodeDraw {
            position: Some(at),
            text_scale: Some(self.scale),
            fit,
            ..Default::default()
        };
        self.nodes.select(TEXTS);
        self.nodes.text(whole(at, size), text, rgb(color), draw);
    }

    /// Hide what the parts since [`Self::begin`] did not use and paint the
    /// rest.
    pub(super) fn paint(&mut self, font: &BitFont) -> &HudPaint {
        let space = HudSpace {
            origin: [0.0; 2],
            scale: 1.0,
        };
        self.nodes.finish(space, Some(font))
    }

    /// The last [`Self::paint`]'s instances.
    pub(super) fn painted(&self) -> &HudPaint {
        self.nodes.painted()
    }
}

/// The whole pixels a part reaches, at least one so that it paints.
fn whole([x, y]: [f32; 2], [w, h]: [f32; 2]) -> Rect {
    let (left, top) = (x.floor() as i32, y.floor() as i32);
    Rect::new(
        left,
        top,
        ((x + w).ceil() as i32).saturating_sub(left).max(1),
        ((y + h).ceil() as i32).saturating_sub(top).max(1),
    )
}
