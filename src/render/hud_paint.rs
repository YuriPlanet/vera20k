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
use crate::render::shell_paint::quad;
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
    /// is not whole 255ths; otherwise the node's colour. A picture is
    /// multiplied by it (none: as it is).
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
    runs: Runs,
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
        draw: impl FnMut(NodeId) -> NodeDraw,
    ) {
        self.build_from(hud, None, space, font, draw);
    }

    /// [`Self::build`] of the subtrees of `roots` alone, one after the other
    /// in that order ([`Gui::paint_from`]), or of the whole tree without
    /// them.
    pub(crate) fn build_from(
        &mut self,
        hud: &Gui,
        roots: Option<&[NodeId]>,
        space: HudSpace,
        font: Option<&BitFont>,
        mut draw: impl FnMut(NodeId) -> NodeDraw,
    ) {
        self.clear();
        let mut commands = recycle(std::mem::take(&mut self.commands));
        match roots {
            Some(roots) => {
                for &root in roots {
                    hud.paint_from(root, &mut commands);
                }
            }
            None => hud.paint(&mut commands),
        }
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
                        self.extend_runs(
                            Material::Pictures,
                            start as u32..self.instances.len() as u32,
                        );
                        continue;
                    }
                    let Some(picture) = node.picture else {
                        continue;
                    };
                    self.instances.push(quad(
                        node.position.unwrap_or_else(|| space.point(rect.x, rect.y)),
                        picture.pixel_size.map(|length| length * space.scale),
                        (picture.uv_origin, picture.uv_size),
                        node.tint.unwrap_or([1.0; 3]),
                        1.0,
                        node.depth,
                    ));
                    Material::Pictures
                }
                PaintOp::Text {
                    rect, key, color, ..
                } => {
                    let Some(font) = font else {
                        continue;
                    };
                    let color = color.unwrap_or(Rgba(255, 255, 255, 255));
                    let (rgb, alpha) = rgba_tint(color);
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
        self.runs.as_slice()
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

    /// A paint whose `material` lane is `instances` and whose texts are
    /// `texts`, for a host that keeps them between paints: no new buffers.
    pub(crate) fn from_lane(
        material: Material,
        instances: Vec<SpriteInstance>,
        texts: Vec<ShellTextDraw>,
    ) -> Self {
        let mut paint = HudPaint {
            instances,
            texts,
            ..Default::default()
        };
        paint.extend_runs(material, 0..paint.instances.len() as u32);
        paint
    }

    /// Moves `material`'s lane onto the end of `out`, out of this paint.
    pub(crate) fn drain_lane(&mut self, material: Material, out: &mut Vec<SpriteInstance>) {
        out.extend(self.lane(material));
        self.remove(&[material]);
    }

    /// `material`'s lane, out of this paint: [`Self::take_lanes`].
    pub(crate) fn take_lane(&mut self, material: Material) -> Vec<SpriteInstance> {
        self.take_lanes(&[material])
    }

    /// The lanes of `materials` one after the other, out of this paint.
    /// When the paint holds those lanes alone, already in that order (as a
    /// paint of one lane does), it is the paint's own buffer, neither copied
    /// nor allocated; otherwise one buffer of their size.
    pub(crate) fn take_lanes(&mut self, materials: &[Material]) -> Vec<SpriteInstance> {
        let rank = |material: Material| materials.iter().position(|of| *of == material);
        let mut last = 0;
        let in_order = self
            .runs()
            .iter()
            .all(|(material, _)| match rank(*material) {
                Some(at) if at >= last => {
                    last = at;
                    true
                }
                _ => false,
            });
        if in_order {
            self.runs.clear();
            return std::mem::take(&mut self.instances);
        }
        let size: usize = (self.runs().iter())
            .filter(|(material, _)| rank(*material).is_some())
            .map(|(_, run)| run.len())
            .sum();
        let mut taken = Vec::with_capacity(size);
        for material in materials {
            taken.extend(self.lane(*material));
        }
        self.remove(materials);
        taken
    }

    /// Drops the lanes of `materials`, moving the rest down in their order.
    fn remove(&mut self, materials: &[Material]) {
        let mut kept = 0u32;
        let mut runs = 0;
        for at in 0..self.runs.as_slice().len() {
            let (material, run) = self.runs.as_slice()[at].clone();
            if materials.contains(&material) {
                continue;
            }
            let len = run.end - run.start;
            if run.start != kept {
                (self.instances).copy_within(run.start as usize..run.end as usize, kept as usize);
            }
            // A dropped lane between two runs of one material joins them.
            let joins = runs > 0 && self.runs.as_slice()[runs - 1].0 == material;
            let slots = self.runs.as_mut_slice();
            if joins {
                slots[runs - 1].1.end = kept + len;
            } else {
                slots[runs] = (material, kept..kept + len);
                runs += 1;
            }
            kept += len;
        }
        self.runs.truncate(runs);
        self.instances.truncate(kept as usize);
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

/// How many runs a paint keeps without a buffer of its own: a shell
/// dialog's few lanes; a HUD of more keeps its buffer from frame to frame.
const INLINE_RUNS: usize = 16;

/// A paint's runs: in place while they are few, so a paint of a dialog's
/// widgets allocates nothing for them.
struct Runs {
    inline: [(Material, Range<u32>); INLINE_RUNS],
    len: usize,
    /// Every run once they outgrow `inline`, kept for its allocation.
    spill: Vec<(Material, Range<u32>)>,
    spilled: bool,
}

impl Default for Runs {
    fn default() -> Self {
        Runs {
            inline: std::array::from_fn(|_| (Material::Solid, 0..0)),
            len: 0,
            spill: Vec::new(),
            spilled: false,
        }
    }
}

impl Runs {
    fn as_slice(&self) -> &[(Material, Range<u32>)] {
        match self.spilled {
            true => &self.spill,
            false => &self.inline[..self.len],
        }
    }

    fn as_mut_slice(&mut self) -> &mut [(Material, Range<u32>)] {
        match self.spilled {
            true => &mut self.spill,
            false => &mut self.inline[..self.len],
        }
    }

    fn last_mut(&mut self) -> Option<&mut (Material, Range<u32>)> {
        self.as_mut_slice().last_mut()
    }

    fn push(&mut self, run: (Material, Range<u32>)) {
        if !self.spilled && self.len < INLINE_RUNS {
            self.inline[self.len] = run;
            self.len += 1;
            return;
        }
        if !self.spilled {
            self.spill.clear();
            self.spill.extend(self.inline[..self.len].iter().cloned());
            self.spilled = true;
        }
        self.spill.push(run);
    }

    /// Keeps the first `len` runs.
    fn truncate(&mut self, len: usize) {
        match self.spilled {
            true => self.spill.truncate(len),
            false => self.len = self.len.min(len),
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.spill.clear();
        self.spilled = false;
    }
}

fn fill(rect: Rect, color: Rgba, space: HudSpace, node: NodeDraw) -> SpriteInstance {
    let (rgb, alpha) = rgba_tint(color);
    let tint = node.tint.unwrap_or(rgb);
    let position = node.position.unwrap_or_else(|| space.point(rect.x, rect.y));
    let size = node
        .size
        .unwrap_or_else(|| [rect.w as f32 * space.scale, rect.h as f32 * space.scale]);
    quad(
        position,
        size,
        ([0.0; 2], [1.0; 2]),
        tint,
        alpha,
        node.depth,
    )
}

/// A colour as a quad's tint and alpha: the batch shader writes tints
/// straight to the 8-bit surface. The one conversion of the HUD's and the
/// shell skin's colours.
pub(crate) fn rgba_tint(Rgba(r, g, b, a): Rgba) -> ([f32; 3], f32) {
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
    fn a_bars_fills_are_the_quads_the_hud_built_before_one_quad_drew_them() {
        let mut hud = Gui::new(8);
        let root = hud
            .create_root(Layer::Field, Origin::Host, NodeKind::Panel)
            .unwrap();
        let bar = hud
            .create_by(NodeKind::Bar, Some(root), Origin::Host)
            .unwrap();
        hud.set(bar, Prop::Rect, Value::Rect(Rect::new(4, 6, 40, 10)))
            .unwrap();
        hud.set(bar, Prop::Fill, Value::Int(42)).unwrap();
        hud.set(bar, Prop::Color, Value::Rgba(Rgba(10, 200, 30, 255)))
            .unwrap();
        hud.layout(Rect::new(0, 0, 100, 100));
        let space = HudSpace {
            origin: [3.0, 5.0],
            scale: 2.0,
        };
        let mut paint = HudPaint::default();
        let node = NodeDraw {
            depth: 0.25,
            ..Default::default()
        };
        paint.build(&hud, space, None, |_| node);
        // `fill` as it built its instance by hand.
        let mut commands = Vec::new();
        hud.paint(&mut commands);
        let hand: Vec<SpriteInstance> = (commands.iter())
            .filter_map(|command| match command.op {
                PaintOp::Fill { rect, color } => Some(SpriteInstance {
                    position: space.point(rect.x, rect.y),
                    size: [rect.w as f32 * 2.0, rect.h as f32 * 2.0],
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    depth: 0.25,
                    tint: [color.0, color.1, color.2].map(|c| f32::from(c) / 255.0),
                    alpha: f32::from(color.3) / 255.0,
                    ..Default::default()
                }),
                _ => None,
            })
            .collect();
        // The outline's four edges and the fill.
        assert_eq!(hand.len(), 5);
        assert_eq!(
            bytemuck::cast_slice::<SpriteInstance, u8>(paint.instances()),
            bytemuck::cast_slice::<SpriteInstance, u8>(&hand)
        );
    }

    #[test]
    fn a_lane_taken_out_leaves_the_rest_in_order_and_alone_is_the_buffer() {
        let at = |x: f32| quad([x, 0.0], [1.0; 2], ([0.0; 2], [1.0; 2]), [1.0; 3], 1.0, 0.0);
        let mut paint = HudPaint::default();
        for (material, x) in [
            (Material::Controls, 0.0),
            (Material::Popup, 1.0),
            (Material::Controls, 2.0),
            (Material::Side, 3.0),
        ] {
            paint.draw(material, |out, _| out.push(at(x)));
        }
        let popup = paint.take_lane(Material::Popup);
        assert_eq!(
            popup.iter().map(|q| q.position[0]).collect::<Vec<_>>(),
            [1.0]
        );
        // The controls around it join.
        assert_eq!(
            paint.runs(),
            [(Material::Controls, 0..2), (Material::Side, 2..3)]
        );
        let mut art = Vec::new();
        paint.drain_lane(Material::Side, &mut art);
        assert_eq!(art.len(), 1);
        let buffer = paint.instances().as_ptr();
        let controls = paint.take_lane(Material::Controls);
        assert_eq!(controls.as_ptr(), buffer);
        assert_eq!(
            controls.iter().map(|q| q.position[0]).collect::<Vec<_>>(),
            [0.0, 2.0]
        );
        assert!(paint.runs().is_empty() && paint.instances().is_empty());
        // More runs than kept in place.
        let mut many = HudPaint::default();
        for x in 0..40 {
            let material = [Material::Controls, Material::Popup][x % 2];
            many.draw(material, |out, _| out.push(at(x as f32)));
        }
        assert_eq!(many.runs().len(), 40);
        let both = many.take_lanes(&[Material::Controls, Material::Popup]);
        assert_eq!(both.len(), 40);
        assert_eq!((both[0].position[0], both[20].position[0]), (0.0, 1.0));
    }

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
