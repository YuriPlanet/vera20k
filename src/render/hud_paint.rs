//! Turns a [`vera20k_gui::Gui`]'s paint list into sprite instances and
//! the runs that draw them in the list's order.
//!
//! It draws fills, pictures, text and host areas (whose instance the host
//! hands over), what its users have. The world commands come with the
//! first HUD that shows one.
//!
//! A shell dialog's widgets draw here too, in their host's skin
//! ([`crate::app::frontend::skirmish_shell_render::gui_paint`]): each
//! skin's sprites on a lane of its own material ([`HudPaint::draw`]), its
//! texts as the shell draws them ([`HudPaint::texts`]).

use std::ops::Range;

use vera20k_gui::{Gui, NodeId, PaintCmd, PaintOp, Rect, Rgba, recycle};

use crate::render::batch::SpriteInstance;
use crate::render::bit_font::BitFont;
use crate::render::shell_text::ShellTextDraw;
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
    /// The shell's control art (the skirmish shell chrome).
    Controls,
    /// The sidebar art: an in-game dialog's right-panel buttons.
    Side,
    /// The main-menu chrome: a family page's column buttons.
    Column,
    /// A WOL page's own art: its icons and glyphs.
    Icons,
    /// A page's own art: its PCX and shape statics.
    PageArt,
    /// The control art over everything else: an open drop-down list.
    Popup,
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

/// What a host adds to one node's drawing.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeDraw {
    /// The squeeze a text draws at.
    pub(crate) fit: Fit,
    /// Target pixels per font pixel, where not the [`HudSpace`]'s scale.
    pub(crate) text_scale: Option<f32>,
    /// The node's top left on the target, where it falls between the HUD's
    /// whole pixels; otherwise its rect's through the [`HudSpace`].
    pub(crate) position: Option<[f32; 2]>,
    /// A fill's size on the target, where a rect cannot hold it: between
    /// whole pixels, empty or negative.
    pub(crate) size: Option<[f32; 2]>,
    /// What the node's `Art` shows.
    pub(crate) picture: Option<Picture>,
    /// The depth its instances carry, for a host whose shader sorts by it.
    pub(crate) depth: f32,
    /// A picture the host has already made into its instance: drawn as it
    /// is, in the node's place in the order, instead of from `picture`.
    pub(crate) instance: Option<SpriteInstance>,
    /// A text that reveals itself unit by unit (`BitFont::build_text_label`):
    /// how many of its UTF-16 units show, at the node's depth and the font's
    /// own size.
    pub(crate) reveal: Option<u32>,
    /// The tint a fill or a plain text draws in, where the host's colour
    /// is not whole 255ths; otherwise the node's colour.
    pub(crate) tint: Option<[f32; 3]>,
}

/// A part of the host's picture atlas, drawn at its own size from its node's
/// top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Picture {
    pub(crate) uv_origin: [f32; 2],
    pub(crate) uv_size: [f32; 2],
    pub(crate) pixel_size: [f32; 2],
}

/// One HUD's instances, with buffers kept from frame to frame.
#[derive(Default)]
pub(crate) struct HudPaint {
    commands: Vec<PaintCmd<'static>>,
    instances: Vec<SpriteInstance>,
    runs: Vec<(Material, Range<u32>)>,
    /// A shell skin's texts, each clipped on its own, in their order.
    pub(crate) texts: Vec<ShellTextDraw>,
    /// The texts over [`Material::Popup`].
    pub(crate) popup_texts: Vec<ShellTextDraw>,
}

impl HudPaint {
    /// Replace the last frame's instances with `hud`'s. A text is literal,
    /// starts at its node's top left, and draws nothing without a `font`.
    pub(crate) fn build(
        &mut self,
        hud: &Gui,
        space: HudSpace,
        font: Option<&BitFont>,
        mut draw: impl FnMut(NodeId) -> NodeDraw,
    ) {
        self.clear();
        let mut commands = recycle(std::mem::take(&mut self.commands));
        hud.paint(&mut commands);
        for command in &commands {
            let start = self.instances.len();
            let node = draw(command.node);
            let material = match command.op {
                PaintOp::Fill { rect, color } => {
                    self.instances.push(fill(rect, color, space, node));
                    Material::Solid
                }
                PaintOp::Image { rect, .. } => {
                    if let Some(instance) = node.instance {
                        self.instances.push(instance);
                        if let Some(clip) = command.clip {
                            clip_from(&mut self.instances, start, clip, space);
                        }
                        self.extend_runs(Material::Pictures, start as u32..self.instances.len() as u32);
                        continue;
                    }
                    let Some(picture) = node.picture else {
                        continue;
                    };
                    self.instances.push(SpriteInstance {
                        position: node.position.unwrap_or_else(|| space.point(rect.x, rect.y)),
                        size: picture.pixel_size.map(|length| length * space.scale),
                        uv_origin: picture.uv_origin,
                        uv_size: picture.uv_size,
                        depth: node.depth,
                        tint: [1.0; 3],
                        alpha: 1.0,
                        ..Default::default()
                    });
                    Material::Pictures
                }
                PaintOp::Text {
                    rect, key, color, ..
                } => {
                    let Some(font) = font else {
                        continue;
                    };
                    let color = color.unwrap_or(Rgba(255, 255, 255, 255));
                    let (rgb, alpha) = tint(color);
                    let [x, y] = node.position.unwrap_or_else(|| space.point(rect.x, rect.y));
                    let scale = node.text_scale.unwrap_or(space.scale);
                    let glyphs = font.fitted(node.fit, |font| match node.reveal {
                        Some(count) => {
                            let Rgba(r, g, b, _) = color;
                            font.build_text_label(key, x, y, node.depth, [0.0; 2], [r, g, b], count)
                        }
                        None => font.build_text(
                            key,
                            x,
                            y,
                            scale,
                            node.depth,
                            node.tint.unwrap_or(rgb),
                            [0.0; 2],
                        ),
                    });
                    self.instances
                        .extend(glyphs.into_iter().map(|glyph| SpriteInstance {
                            alpha: glyph.alpha * alpha,
                            depth: node.depth,
                            ..glyph
                        }));
                    Material::Glyphs
                }
                // A host area: what the host draws there, handed over as
                // its instance, in the node's place in the order.
                PaintOp::External { .. } => {
                    let Some(instance) = node.instance else {
                        continue;
                    };
                    self.instances.push(instance);
                    Material::Pictures
                }
            };
            if let Some(clip) = command.clip {
                clip_from(&mut self.instances, start, clip, space);
            }
            self.extend_runs(material, start as u32..self.instances.len() as u32);
        }
        self.commands = recycle(commands);
    }

    /// Empties every lane and text list, keeping their buffers.
    pub(crate) fn clear(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.texts.clear();
        self.popup_texts.clear();
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

    /// Appends what `draw` pushes onto `material`'s lane, after all that
    /// is there; `draw` only pushes. It gets the texts too, for a skin
    /// that lays a text down with its sprites.
    pub(crate) fn draw<R>(
        &mut self,
        material: Material,
        draw: impl FnOnce(&mut Vec<SpriteInstance>, &mut Vec<ShellTextDraw>) -> R,
    ) -> R {
        let start = self.instances.len();
        let drawn = draw(&mut self.instances, &mut self.texts);
        self.extend_runs(material, start as u32..self.instances.len() as u32);
        drawn
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

fn fill(rect: Rect, color: Rgba, space: HudSpace, node: NodeDraw) -> SpriteInstance {
    let (rgb, alpha) = tint(color);
    let tint = node.tint.unwrap_or(rgb);
    let position = node.position.unwrap_or_else(|| space.point(rect.x, rect.y));
    let size = node
        .size
        .unwrap_or_else(|| [rect.w as f32 * space.scale, rect.h as f32 * space.scale]);
    SpriteInstance {
        position,
        size,
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        depth: node.depth,
        tint,
        alpha,
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
    use vera20k_gui::{Layer, NodeKind, Origin, Prop, Value};

    #[test]
    fn a_clipped_hud_keeps_paint_order_and_cuts_each_quad_with_its_texture() {
        let font = crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 4)], 3);
        let mut hud = Gui::new(16);
        let root = hud
            .create_root(Layer::Field, Origin::Host, NodeKind::Panel)
            .unwrap();
        hud.set(root, Prop::Rect, Value::Rect(Rect::new(0, 0, 10, 40)))
            .unwrap();
        hud.set(root, Prop::Clip, Value::Bool(true)).unwrap();
        let mut child = |kind, rect| {
            let node = hud.create_by(kind, Some(root), Origin::Host).unwrap();
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
        paint.build(&hud, space, Some(&font), |_| NodeDraw::default());
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
