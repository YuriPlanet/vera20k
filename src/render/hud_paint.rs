//! Turns a [`vera20k_gui::hud::Hud`]'s paint list into sprite instances and
//! the runs that draw them in the list's order.
//!
//! It draws fills, pictures and text, what its users have. Host areas come
//! with the first HUD that shows one, and so do the world commands.

use std::ops::Range;

use vera20k_gui::hud::{Hud, PaintCmd, PaintOp, recycle};
use vera20k_gui::{NodeId, Rect, Rgba};

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::sidebar_chrome::SidebarChromeEntry;
use crate::render::text_engine::Fit;

/// What a run of instances samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Material {
    /// One white texel, tinted.
    Solid,
    /// The font's atlas.
    Glyphs,
    /// The atlas the host's pictures are in.
    Pictures,
}

/// What the host adds to a node's commands.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeDraw {
    /// The squeeze of the node's text.
    pub(crate) fit: Fit,
    /// The fill's size where it is not the rect's. A HUD neither holds a
    /// negative size nor paints an empty rect, so such a fill's node has a
    /// rect one pixel big.
    pub(crate) size: Option<[i32; 2]>,
    /// What the node's `Art` shows, drawn at its own size from the rect's top
    /// left.
    pub(crate) picture: Option<SidebarChromeEntry>,
}

/// Where a HUD's pixels land on the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HudSpace {
    pub(crate) origin: [f32; 2],
    /// Target pixels per HUD pixel.
    pub(crate) scale: f32,
}

impl HudSpace {
    fn point(self, x: i32, y: i32) -> [f32; 2] {
        [
            self.origin[0] + x as f32 * self.scale,
            self.origin[1] + y as f32 * self.scale,
        ]
    }
}

/// One HUD's instances, with buffers kept from frame to frame.
#[derive(Default)]
pub(crate) struct HudPaint {
    commands: Vec<PaintCmd<'static>>,
    instances: Vec<SpriteInstance>,
    runs: Vec<(Material, Range<u32>)>,
}

impl HudPaint {
    /// Replace the last frame's instances with `hud`'s. A text is literal,
    /// starts at its rect's top left and is not held to its size. `host`
    /// gives what a node's commands leave to it.
    pub(crate) fn build(
        &mut self,
        hud: &Hud,
        space: HudSpace,
        font: &BitFont,
        host: impl Fn(NodeId) -> NodeDraw,
    ) {
        self.instances.clear();
        self.runs.clear();
        let mut commands = recycle(std::mem::take(&mut self.commands));
        hud.paint(&mut commands);
        for command in &commands {
            let start = self.instances.len();
            let material = match command.op {
                PaintOp::Fill { rect, color } => {
                    let [w, h] = host(command.node).size.unwrap_or([rect.w, rect.h]);
                    let rect = Rect::new(rect.x, rect.y, w, h);
                    self.instances.push(fill(rect, color, space));
                    Material::Solid
                }
                PaintOp::Image { rect, .. } => {
                    let Some(art) = host(command.node).picture else {
                        continue;
                    };
                    self.instances.push(picture(rect, art, space));
                    Material::Pictures
                }
                PaintOp::Text {
                    rect, key, color, ..
                } => {
                    let (rgb, alpha) = tint(color.unwrap_or(Rgba(255, 255, 255, 255)));
                    let [x, y] = space.point(rect.x, rect.y);
                    let glyphs = font.fitted(host(command.node).fit, |font| {
                        font.build_text(key, x, y, space.scale, 0.0, rgb, [0.0; 2])
                    });
                    self.instances
                        .extend(glyphs.into_iter().map(|glyph| SpriteInstance {
                            alpha: glyph.alpha * alpha,
                            ..glyph
                        }));
                    Material::Glyphs
                }
                PaintOp::External { .. } => continue,
            };
            if let Some(clip) = command.clip {
                clip_from(&mut self.instances, start, clip, space);
            }
            self.extend_runs(material, start as u32..self.instances.len() as u32);
        }
        self.commands = recycle(commands);
    }

    pub(crate) fn instances(&self) -> &[SpriteInstance] {
        &self.instances
    }

    /// The draws, bottom to top: only neighbours of one material share one.
    pub(crate) fn runs(&self) -> &[(Material, Range<u32>)] {
        &self.runs
    }

    /// The instances of one material, bottom to top, for a host that draws
    /// each material in a lane of its own.
    pub(crate) fn lane(&self, material: Material) -> impl Iterator<Item = &SpriteInstance> {
        self.runs()
            .iter()
            .filter(move |(of, _)| *of == material)
            .flat_map(move |(_, run)| &self.instances()[run.start as usize..run.end as usize])
    }

    fn extend_runs(&mut self, material: Material, added: Range<u32>) {
        if added.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some((last, range)) if *last == material => range.end = added.end,
            _ => self.runs.push((material, added)),
        }
    }
}

fn fill(rect: Rect, color: Rgba, space: HudSpace) -> SpriteInstance {
    let (tint, alpha) = tint(color);
    SpriteInstance {
        position: space.point(rect.x, rect.y),
        size: [rect.w as f32 * space.scale, rect.h as f32 * space.scale],
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        depth: 0.0,
        tint,
        alpha,
        ..Default::default()
    }
}

fn picture(rect: Rect, art: SidebarChromeEntry, space: HudSpace) -> SpriteInstance {
    SpriteInstance {
        position: space.point(rect.x, rect.y),
        size: art.pixel_size.map(|length| length * space.scale),
        uv_origin: art.uv_origin,
        uv_size: art.uv_size,
        tint: [1.0; 3],
        alpha: 1.0,
        ..Default::default()
    }
}

/// The batch shader writes tints straight to the 8-bit surface.
fn tint(Rgba(r, g, b, a): Rgba) -> ([f32; 3], f32) {
    (
        [r, g, b].map(|channel| f32::from(channel) / 255.0),
        f32::from(a) / 255.0,
    )
}

/// Cut the instances from `start` to `clip`, dropping those wholly outside.
/// On the CPU, so a clip splits no draw.
fn clip_from(instances: &mut Vec<SpriteInstance>, start: usize, clip: Rect, space: HudSpace) {
    let low = space.point(clip.x, clip.y);
    let high = space.point(clip.x.saturating_add(clip.w), clip.y.saturating_add(clip.h));
    let mut kept = start;
    for at in start..instances.len() {
        let mut quad = instances[at];
        let mut shows = true;
        for axis in 0..2 {
            let (from, size) = (quad.position[axis], quad.size[axis]);
            let (cut_from, cut_to) = (from.max(low[axis]), (from + size).min(high[axis]));
            if cut_to <= cut_from {
                shows = false;
                break;
            }
            if cut_from > from || cut_to < from + size {
                let per_pixel = quad.uv_size[axis] / size;
                quad.position[axis] = cut_from;
                quad.size[axis] = cut_to - cut_from;
                quad.uv_origin[axis] += (cut_from - from) * per_pixel;
                quad.uv_size[axis] = (cut_to - cut_from) * per_pixel;
            }
        }
        if shows {
            instances[kept] = quad;
            kept += 1;
        }
    }
    instances.truncate(kept);
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;
    use vera20k_gui::hud::{InputPolicy, Layer, Origin};
    use vera20k_gui::{NodeKind, Prop, Value};

    #[test]
    fn a_clipped_hud_keeps_paint_order_and_cuts_each_quad_with_its_texture() {
        let font = crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 4)], 3);
        let mut hud = Hud::new(16, InputPolicy::default());
        let root = hud
            .create_root(Layer::Field, Origin::Host, NodeKind::Panel)
            .unwrap();
        hud.set(root, Prop::Rect, Value::Rect(Rect::new(0, 0, 10, 40)))
            .unwrap();
        hud.set(root, Prop::Clip, Value::Bool(true)).unwrap();
        let mut child = |kind, rect| {
            let node = hud.create(kind, root, Origin::Host).unwrap();
            hud.set(node, Prop::Rect, Value::Rect(rect)).unwrap();
            match kind {
                NodeKind::Text => hud.set(node, Prop::Text, Value::Text("a".into())).unwrap(),
                _ => hud
                    .set(node, Prop::Background, Value::Rgba(Rgba(255, 0, 0, 255)))
                    .unwrap(),
            }
        };
        child(NodeKind::Panel, Rect::new(6, 0, 8, 8));
        child(NodeKind::Text, Rect::new(0, 0, 6, 16));
        child(NodeKind::Panel, Rect::new(0, 4, 4, 4));
        hud.layout(Rect::new(0, 0, 100, 100));

        let mut paint = HudPaint::default();
        let space = HudSpace {
            origin: [100.0, 50.0],
            scale: 2.0,
        };
        paint.build(&hud, space, &font, |_| NodeDraw::default());
        assert_eq!(
            paint.runs(),
            [
                (Material::Solid, 0..1),
                (Material::Glyphs, 1..2),
                (Material::Solid, 2..3)
            ]
        );
        let cut = paint.instances()[0];
        assert_eq!((cut.position, cut.size), ([112.0, 50.0], [8.0, 16.0]));
        assert_eq!(cut.uv_size, [0.5, 1.0]);
    }
}
