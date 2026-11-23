//! The panel as a HUD tree in window pixels. The nodes stay from frame to
//! frame and change only where a part does. At a match zoom a part falls
//! between whole pixels, so each also keeps its exact place.

use vera20k_gui::hud::{Hud, Layer, Origin};
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
    /// Window pixels per font pixel of the parts since [`Self::begin`].
    scale: f32,
    /// The revision of the last layout.
    laid_out: Option<u64>,
}

impl PanelView {
    pub(super) fn new() -> Self {
        let mut hud = Hud::new(MAX_NODES);
        let solids = Slots::new(&mut hud, NodeKind::Panel);
        let texts = Slots::new(&mut hud, NodeKind::Text);
        Self {
            hud,
            solids,
            texts,
            scale: 1.0,
            laid_out: None,
        }
    }

    pub(super) fn begin(&mut self, scale: f32) {
        self.scale = scale;
        self.solids.used = 0;
        self.texts.used = 0;
    }

    pub(super) fn solid(&mut self, at: [f32; 2], size: [f32; 2], rgb: [u8; 3]) {
        let draw = NodeDraw {
            position: Some(at),
            size: Some(size),
            ..Default::default()
        };
        self.solids
            .show(&mut self.hud, whole(at, size), rgb, "", draw);
    }

    pub(super) fn text(
        &mut self,
        at: [f32; 2],
        size: [f32; 2],
        text: &str,
        rgb: [u8; 3],
        fit: Fit,
    ) {
        let draw = NodeDraw {
            position: Some(at),
            text_scale: Some(self.scale),
            fit,
            ..Default::default()
        };
        self.texts
            .show(&mut self.hud, whole(at, size), rgb, text, draw);
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
        // The paint list names the shown nodes in slot order.
        let mut shown = self.solids.shown().iter().chain(self.texts.shown());
        out.build(&self.hud, space, font, |node| {
            shown
                .find(|slot| slot.node == node)
                .map(|slot| slot.draw)
                .unwrap_or_default()
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
    slots: Vec<Slot>,
    used: usize,
}

/// A node and what it was last set to, so that a still part costs a compare.
struct Slot {
    node: NodeId,
    visible: bool,
    rect: Option<Rect>,
    rgb: Option<[u8; 3]>,
    text: String,
    draw: NodeDraw,
}

impl Slots {
    fn new(hud: &mut Hud, kind: NodeKind) -> Self {
        Self {
            root: hud
                .create_root(Layer::Field, Origin::Host, NodeKind::Panel)
                .ok(),
            kind,
            slots: Vec::new(),
            used: 0,
        }
    }

    /// Show a part in the next node; none past the node limit.
    fn show(&mut self, hud: &mut Hud, rect: Rect, rgb: [u8; 3], text: &str, draw: NodeDraw) {
        if self.used == self.slots.len() {
            let Some(root) = self.root else { return };
            let Ok(node) = hud.create(self.kind, root, Origin::Host) else {
                return;
            };
            self.slots.push(Slot {
                node,
                visible: true,
                rect: None,
                rgb: None,
                text: String::new(),
                draw,
            });
        }
        let slot = &mut self.slots[self.used];
        self.used += 1;
        slot.draw = draw;
        let node = slot.node;
        if !slot.visible {
            slot.visible = true;
            let _ = hud.set(node, Prop::Visible, Value::Bool(true));
        }
        if slot.rect != Some(rect) {
            slot.rect = Some(rect);
            let _ = hud.set(node, Prop::Rect, Value::Rect(rect));
        }
        if slot.rgb != Some(rgb) {
            slot.rgb = Some(rgb);
            let prop = match self.kind {
                NodeKind::Text => Prop::Color,
                _ => Prop::Background,
            };
            let [r, g, b] = rgb;
            let _ = hud.set(node, prop, Value::Rgba(Rgba(r, g, b, 255)));
        }
        if slot.text != text {
            text.clone_into(&mut slot.text);
            let _ = hud.set(node, Prop::Text, Value::Text(text.to_owned()));
        }
    }

    fn shown(&self) -> &[Slot] {
        &self.slots[..self.used]
    }

    fn hide_rest(&mut self, hud: &mut Hud) {
        for slot in &mut self.slots[self.used..] {
            if slot.visible {
                slot.visible = false;
                let _ = hud.set(slot.node, Prop::Visible, Value::Bool(false));
            }
        }
    }
}
