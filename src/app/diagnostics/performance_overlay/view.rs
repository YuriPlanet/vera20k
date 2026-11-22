//! The panel as a HUD tree in window pixels. The nodes stay from frame to
//! frame and change only where a part does. At a match zoom a part falls
//! between whole pixels, so each also keeps its exact place.

use std::collections::HashMap;

use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect, Rgba, Value};

use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, HudSpace, NodeDraw};
use crate::render::text_engine::Fit;

/// A track and a fill for each of 1,024 cores, with room for the rows.
const MAX_NODES: usize = 4096;
/// The HUD culls nothing: the window's own edges clip the panel.
const EVERYWHERE: Rect = Rect::new(-(1 << 29), -(1 << 29), 1 << 30, 1 << 30);

pub(super) struct PanelView {
    hud: Hud,
    /// The boxes and bars lie under all the text, so each takes one draw.
    solids: Slots,
    texts: Slots,
    draws: HashMap<NodeId, NodeDraw>,
    /// Window pixels per font pixel of the parts since [`Self::begin`].
    scale: f32,
    /// The revision of the last layout.
    laid_out: Option<u64>,
}

impl PanelView {
    pub(super) fn new() -> Self {
        let mut hud = Hud::new(MAX_NODES, InputPolicy::default());
        let solids = Slots::new(&mut hud, NodeKind::Panel);
        let texts = Slots::new(&mut hud, NodeKind::Text);
        Self {
            hud,
            solids,
            texts,
            draws: HashMap::new(),
            scale: 1.0,
            laid_out: None,
        }
    }

    pub(super) fn begin(&mut self, scale: f32) {
        self.scale = scale;
        self.solids.used = 0;
        self.texts.used = 0;
    }

    pub(super) fn solid(&mut self, at: [f32; 2], size: [f32; 2], [r, g, b]: [u8; 3]) {
        if let Some(node) = self.solids.next(&mut self.hud, whole(at, size)) {
            let color = Value::Rgba(Rgba(r, g, b, 255));
            let _ = self.hud.set(node, Prop::Background, color);
            let draw = NodeDraw {
                position: Some(at),
                size: Some(size),
                ..Default::default()
            };
            self.draws.insert(node, draw);
        }
    }

    pub(super) fn text(
        &mut self,
        at: [f32; 2],
        size: [f32; 2],
        text: &str,
        [r, g, b]: [u8; 3],
        fit: Fit,
    ) {
        if let Some(node) = self.texts.next(&mut self.hud, whole(at, size)) {
            let _ = self.hud.set(node, Prop::Text, Value::Text(text.to_owned()));
            let _ = self
                .hud
                .set(node, Prop::Color, Value::Rgba(Rgba(r, g, b, 255)));
            let draw = NodeDraw {
                position: Some(at),
                size: Some(size),
                text_scale: Some(self.scale),
                fit,
                ..Default::default()
            };
            self.draws.insert(node, draw);
        }
    }

    /// Hide what the parts since [`Self::begin`] did not use.
    pub(super) fn finish(&mut self) {
        self.solids.hide_rest(&mut self.hud);
        self.texts.hide_rest(&mut self.hud);
    }

    pub(super) fn paint(&mut self, out: &mut HudPaint, font: &BitFont) {
        let revision = self.hud.revision();
        if self.laid_out != Some(revision) {
            self.hud.layout(EVERYWHERE);
            self.laid_out = Some(revision);
        }
        let space = HudSpace {
            origin: [0.0; 2],
            scale: 1.0,
        };
        out.build(&self.hud, space, font, |node| {
            self.draws.get(&node).copied().unwrap_or_default()
        });
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

/// Nodes of one kind under a root of their own, reused in order.
struct Slots {
    root: Option<NodeId>,
    kind: NodeKind,
    nodes: Vec<NodeId>,
    used: usize,
}

impl Slots {
    fn new(hud: &mut Hud, kind: NodeKind) -> Self {
        Self {
            root: hud
                .create_root(Layer::Field, Origin::Host, NodeKind::Panel)
                .ok(),
            kind,
            nodes: Vec::new(),
            used: 0,
        }
    }

    /// The next node, shown at `rect`; none past the node limit.
    fn next(&mut self, hud: &mut Hud, rect: Rect) -> Option<NodeId> {
        if self.used == self.nodes.len() {
            let node = hud.create(self.kind, self.root?, Origin::Host).ok()?;
            self.nodes.push(node);
        }
        let node = self.nodes[self.used];
        self.used += 1;
        let _ = hud.set(node, Prop::Visible, Value::Bool(true));
        let _ = hud.set(node, Prop::Rect, Value::Rect(rect));
        Some(node)
    }

    fn hide_rest(&self, hud: &mut Hud) {
        for &node in &self.nodes[self.used..] {
            let _ = hud.set(node, Prop::Visible, Value::Bool(false));
        }
    }
}
