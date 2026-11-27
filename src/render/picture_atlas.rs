//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): a declared picture's frames as the SHP
//! blitters' RGBA source pages.
//!
//! Each frame is shrunk to game pixels (`PictureSize=`), cropped to its drawn
//! pixels like a stored SHP frame, and given an index plane the blitters read
//! as palette indices: 0 for a hole, [`LIT_INDEX`] for an ordinary pixel and
//! [`UNLIT_INDEX`] for one light leaves alone (the mask's green). A house-colour
//! pixel (the mask's red) takes the owner's ramp step for its brightness. In
//! the sprite atlas it is drawn once for every owner: its index is
//! [`HOUSE_INDEX`] plus the step, and the blitters look the colour up in the
//! owner's row of the [`RAMP_STRIP`] every page holds at its top left
//! (`sprite_source.wgsl`, [`PICTURE_HOUSE`]). Elsewhere it takes the ramp
//! before upload. The SHP blitters, their shaders and their Z, shadow and
//! light rules are the ones every SHP draws through.
//!
//! Art above 1x also keeps each frame at its own resolution, for the zoomed
//! draw (`render::picture_zoom`): the same crop, `scale` times larger, on HD
//! twins of its pages laid out at the same normalised coordinates.

use crate::assets::pal_file::Color;
use crate::assets::picture_file::{PictureError, PictureFrames};

/// A house colour ramp's entries, palette indices 16–31
/// (`HouseColorRamps::ramp`).
pub(crate) const RAMP_SIZE: usize = 16;

/// The index-plane value of an ordinary drawn pixel: any index 1..=239 is lit
/// the same way (`native_palette_word`).
pub(crate) const LIT_INDEX: u8 = 1;
/// The index-plane value of a pixel light leaves as it is (indices 240–254,
/// `palette_light.wgsl`).
pub(crate) const UNLIT_INDEX: u8 = 240;
/// The largest whole multiple a picture may have of its `PictureSize=`.
pub(crate) const PICTURE_SCALE_MAX: u32 = 8;
/// The index-plane value of a house-colour pixel at ramp step 0; step `n` is
/// `HOUSE_INDEX + n`, palette indices 16–31 as an SHP's house colour. Lit as
/// any index 1..=239.
pub(crate) const HOUSE_INDEX: u8 = 16;
/// The `ShpSpriteEntry::source_palette` bit of an RGBA picture entry whose
/// house-colour pixels take the ramp of the house colour in its low byte.
pub(crate) const PICTURE_HOUSE: u32 = 1 << 31;
/// The house ramps at the top left of every picture page: house colour `h`'s
/// step `n` at `((h % 16) * 16 + n, h / 16)`, for every house colour index.
pub(crate) const RAMP_STRIP: [u32; 2] = [256, 16];

/// How a sprite's house-colour pixels are coloured.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HouseColor<'a> {
    /// Their own colour.
    Own,
    /// The ramp's colour for their brightness, before upload.
    Baked(&'a [Color; RAMP_SIZE]),
    /// Their own colour, with index [`HOUSE_INDEX`] plus their ramp step, for
    /// the draw to colour (and, at HD, on the house layer instead).
    Deferred,
}

/// What a mask pixel marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskClass {
    Plain,
    House,
    Unlit,
}

/// A picture's frames at game pixels: straight RGBA with binary alpha, the
/// mask's class of each pixel, on the `PictureSize=` canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GamePictureFrames {
    pub canvas: [u32; 2],
    frames: Vec<GameFrame>,
    /// The picture's pixels per game pixel on each axis.
    scale: u32,
    /// VERA-only, experimental: with `scale` above 1, each frame at the
    /// picture's own resolution, its alpha as decoded; empty for 1x art.
    full: Vec<GameFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GameFrame {
    rgba: Vec<u8>,
    classes: Vec<MaskClass>,
}

/// One frame ready for a page: the drawn rectangle of the canvas, its RGBA
/// and its index plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PictureSprite {
    /// The drawn rectangle's place on the canvas, as a stored SHP frame's.
    pub origin: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    /// The same rectangle at the picture's own resolution, for art above 1x.
    pub hd: Option<HdSprite>,
}

/// VERA-only, experimental: a sprite's rectangle at the picture's own
/// resolution, `scale` picture pixels per game pixel. Its RGBA is
/// premultiplied (holes are 0, so filtering never darkens an edge) and its
/// index plane holds the sprite's index-plane values per picture pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HdSprite {
    pub scale: u32,
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    /// For [`HouseColor::Deferred`], the house layer: per picture pixel, its
    /// ramp step times [`HOUSE_LAYER_STEP`] and 255 for a house-colour pixel
    /// (whose RGBA is black), 0 and 0 elsewhere; premultiplied, so the
    /// zoomed draw adds the owner's colour by coverage.
    pub house: Option<Vec<u8>>,
}

/// A ramp step's value on the house layer: step 15 is 255.
pub(crate) const HOUSE_LAYER_STEP: u8 = 17;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PictureArtError {
    #[error(transparent)]
    Decode(#[from] PictureError),
    #[error(
        "{picture:?} is not a whole multiple (1 to {PICTURE_SCALE_MAX}) of PictureSize={canvas:?}"
    )]
    Scale { picture: [u32; 2], canvas: [u32; 2] },
    #[error(
        "the mask is {mask:?} with {mask_frames} frames, the picture {picture:?} with {frames}"
    )]
    MaskShape {
        mask: [u32; 2],
        mask_frames: usize,
        picture: [u32; 2],
        frames: usize,
    },
}

impl GamePictureFrames {
    /// Shrink `picture`, and its `mask` when it has one, to `canvas` game
    /// pixels. A game pixel is drawn when at least half the picture pixels it
    /// covers are; its colour is their coverage-weighted mean, and it takes
    /// the class most of its covered pixels have. Without `canvas` the
    /// picture is 1x art.
    pub(crate) fn shrink(
        picture: PictureFrames,
        mask: Option<&PictureFrames>,
        canvas: Option<[u32; 2]>,
    ) -> Result<Self, PictureArtError> {
        let size = [picture.width, picture.height];
        let canvas = canvas.unwrap_or(size);
        let scale = size[0] / canvas[0].max(1);
        if scale == 0 || scale > PICTURE_SCALE_MAX || size != [canvas[0] * scale, canvas[1] * scale]
        {
            return Err(PictureArtError::Scale {
                picture: size,
                canvas,
            });
        }
        if let Some(mask) = mask
            && ([mask.width, mask.height] != size || mask.frames.len() != picture.frames.len())
        {
            return Err(PictureArtError::MaskShape {
                mask: [mask.width, mask.height],
                mask_frames: mask.frames.len(),
                picture: size,
                frames: picture.frames.len(),
            });
        }
        // Frame by frame side by side: one building's picture can hold a
        // couple of hundred frames, which would otherwise keep one worker
        // busy long after the others are done.
        let indices: Vec<usize> = (0..picture.frames.len()).collect();
        let frames = crate::sim::parallel::map(&indices, |&index| {
            let mask = mask.map(|mask| mask.frames[index].as_slice());
            shrink_frame(&picture.frames[index], mask, size[0], canvas, scale)
        });
        // The HD frames keep the picture's own pixels, moved, not copied.
        let full = if scale > 1 {
            let classes = crate::sim::parallel::map(&indices, |&index| match mask {
                Some(mask) => mask.frames[index].chunks_exact(4).map(mask_class).collect(),
                None => vec![MaskClass::Plain; picture.frames[index].len() / 4],
            });
            picture
                .frames
                .into_iter()
                .zip(classes)
                .map(|(rgba, classes)| GameFrame { classes, rgba })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            canvas,
            frames,
            scale,
            full,
        })
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Frame `frame` cropped to its drawn pixels, house-colour pixels as
    /// `house` says. None for a frame past the end or with nothing drawn, as
    /// an SHP's empty frame.
    pub(crate) fn sprite(&self, frame: usize, house: HouseColor<'_>) -> Option<PictureSprite> {
        let source = self.frames.get(frame)?;
        let width = self.canvas[0];
        let ([left, top], size) = self.drawn_rect(frame)?;
        let (right, bottom) = (left + size[0], top + size[1]);
        let mut rgba = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        let mut indices = Vec::with_capacity((size[0] * size[1]) as usize);
        for y in top..bottom {
            for x in left..right {
                let at = (y * width + x) as usize;
                let mut pixel: [u8; 4] = source.rgba[at * 4..at * 4 + 4].try_into().unwrap();
                let index = match (pixel[3], source.classes[at]) {
                    (0, _) => 0,
                    (_, MaskClass::Unlit) => UNLIT_INDEX,
                    (_, MaskClass::House) => match house {
                        HouseColor::Own => LIT_INDEX,
                        HouseColor::Baked(ramp) => {
                            let color = ramp[ramp_step(pixel)];
                            pixel = [color.r, color.g, color.b, 255];
                            LIT_INDEX
                        }
                        HouseColor::Deferred => HOUSE_INDEX + ramp_step(pixel) as u8,
                    },
                    (_, MaskClass::Plain) => LIT_INDEX,
                };
                rgba.extend_from_slice(&pixel);
                indices.push(index);
            }
        }
        let hd = self.hd_sprite(frame, [left, top], size, house);
        Some(PictureSprite {
            origin: [left, top],
            size,
            rgba,
            indices,
            hd,
        })
    }

    /// Frame `frame`'s canvas rectangle at `origin`, `size` game pixels, at
    /// the picture's own resolution; None for 1x art. A picture pixel is
    /// drawn when at least half covered, as `shrink` counts it.
    fn hd_sprite(
        &self,
        frame: usize,
        origin: [u32; 2],
        size: [u32; 2],
        house: HouseColor<'_>,
    ) -> Option<HdSprite> {
        let source = self.full.get(frame)?;
        let scale = self.scale;
        let width = self.canvas[0] * scale;
        let hd_size = size.map(|side| side * scale);
        let pixels = (hd_size[0] * hd_size[1]) as usize;
        let (mut rgba, mut indices) = (Vec::with_capacity(pixels * 4), Vec::with_capacity(pixels));
        let deferred = matches!(house, HouseColor::Deferred);
        let mut layer = Vec::with_capacity(if deferred { pixels * 2 } else { 0 });
        for y in origin[1] * scale..origin[1] * scale + hd_size[1] {
            for x in origin[0] * scale..origin[0] * scale + hd_size[0] {
                let at = (y * width + x) as usize;
                let pixel: [u8; 4] = source.rgba[at * 4..at * 4 + 4].try_into().unwrap();
                let mut on_layer = [0; 2];
                if u32::from(pixel[3]) * 2 < 255 {
                    rgba.extend_from_slice(&[0; 4]);
                    indices.push(0);
                } else {
                    let (color, index) = match (source.classes[at], house) {
                        (MaskClass::Unlit, _) => ([pixel[0], pixel[1], pixel[2]], UNLIT_INDEX),
                        (MaskClass::House, HouseColor::Baked(ramp)) => {
                            let color = ramp[ramp_step(pixel)];
                            ([color.r, color.g, color.b], LIT_INDEX)
                        }
                        (MaskClass::House, HouseColor::Deferred) => {
                            // The colour comes from the house layer.
                            let step = ramp_step(pixel) as u8;
                            on_layer = [step * HOUSE_LAYER_STEP, 255];
                            ([0; 3], HOUSE_INDEX + step)
                        }
                        _ => ([pixel[0], pixel[1], pixel[2]], LIT_INDEX),
                    };
                    rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
                    indices.push(index);
                }
                if deferred {
                    layer.extend_from_slice(&on_layer);
                }
            }
        }
        Some(HdSprite {
            scale,
            size: hd_size,
            rgba,
            indices,
            house: deferred.then_some(layer),
        })
    }

    /// Frame `frame`'s drawn rectangle on the canvas, origin and size, as a
    /// stored SHP frame's; None for a frame past the end or with nothing
    /// drawn.
    fn drawn_rect(&self, frame: usize) -> Option<([u32; 2], [u32; 2])> {
        let source = self.frames.get(frame)?;
        let width = self.canvas[0];
        let drawn = |x: u32, y: u32| source.rgba[((y * width + x) * 4 + 3) as usize] != 0;
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..self.canvas[1] {
            for x in 0..width {
                if drawn(x, y) {
                    (left, top) = (left.min(x), top.min(y));
                    (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
                }
            }
        }
        (left < right).then(|| ([left, top], [right - left, bottom - top]))
    }

    /// Frame `frame`'s rectangle `[x, y, w, h]` as `SHPStruct::GetFrameBounds`
    /// (`0x0069E7E0`) reads a stored SHP frame's: all zero for an empty frame
    /// or one past the end.
    fn frame_bounds(&self, frame: usize) -> [i32; 4] {
        self.drawn_rect(frame).map_or([0; 4], |(origin, size)| {
            let word = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
            [
                word(origin[0]),
                word(origin[1]),
                word(size[0]),
                word(size[1]),
            ]
        })
    }

    /// Whether any drawn pixel takes the owner's colour, so the frames
    /// differ by house.
    #[cfg(test)]
    pub(crate) fn has_house_color(&self) -> bool {
        self.frames.iter().any(|frame| {
            frame
                .classes
                .iter()
                .zip(frame.rgba.chunks_exact(4))
                .any(|(class, pixel)| *class == MaskClass::House && pixel[3] != 0)
        })
    }

    /// Frame `frame` without house colour, placed as a stored SHP frame
    /// relative to its draw point (CC_Draw_Shape `0x004AED70`: the canvas
    /// half, truncated, then the frame's origin; `stored_frame_offset`).
    /// None when it draws nothing.
    pub(crate) fn stored_frame(&self, frame: usize) -> Option<StoredPictureFrame> {
        let sprite = self.sprite(frame, HouseColor::Own)?;
        let half = self
            .canvas
            .map(|side| i32::try_from(side / 2).unwrap_or(i32::MAX));
        let origin = sprite
            .origin
            .map(|side| i32::try_from(side).unwrap_or(i32::MAX));
        Some(StoredPictureFrame {
            offset: [(origin[0] - half[0]) as f32, (origin[1] - half[1]) as f32],
            draw_point: [half[0] - origin[0], half[1] - origin[1]],
            sprite,
        })
    }
}

/// A frame placed as a stored SHP frame ([`GamePictureFrames::stored_frame`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredPictureFrame {
    pub sprite: PictureSprite,
    /// The sprite's top-left corner from the draw point.
    pub offset: [f32; 2],
    /// The draw point's place in the sprite.
    pub draw_point: [i32; 2],
}

/// One frame of `picture_width`-wide RGBA shrunk by `scale` onto `canvas`.
/// Each game row adds up the `scale` picture rows it covers, one picture
/// pixel after another, into per-column totals.
fn shrink_frame(
    rgba: &[u8],
    mask: Option<&[u8]>,
    picture_width: u32,
    canvas: [u32; 2],
    scale: u32,
) -> GameFrame {
    let pixels = (canvas[0] * canvas[1]) as usize;
    let mut out = GameFrame {
        rgba: Vec::with_capacity(pixels * 4),
        classes: Vec::with_capacity(pixels),
    };
    let area = scale * scale;
    let (columns, row_bytes) = (canvas[0] as usize, picture_width as usize * 4);
    let (scale, picture_width) = (scale as usize, picture_width as usize);
    // Per game column: covered pixels, colour sums, class votes.
    let mut covered = vec![0u32; columns];
    let mut sums = vec![[0u32; 3]; columns];
    let mut votes = vec![[0u32; 3]; columns];
    for y in 0..canvas[1] as usize {
        covered.fill(0);
        sums.fill([0; 3]);
        votes.fill([0; 3]);
        for dy in 0..scale {
            let start = (y * scale + dy) * row_bytes;
            let row = &rgba[start..start + row_bytes];
            let mask_row = mask.map(|mask| &mask[start..start + row_bytes]);
            for x in 0..picture_width {
                let pixel = &row[x * 4..x * 4 + 4];
                if u32::from(pixel[3]) * 2 < 255 {
                    continue;
                }
                let column = x / scale;
                covered[column] += 1;
                let sum = &mut sums[column];
                sum[0] += u32::from(pixel[0]);
                sum[1] += u32::from(pixel[1]);
                sum[2] += u32::from(pixel[2]);
                let class =
                    mask_row.map_or(MaskClass::Plain, |mask| mask_class(&mask[x * 4..x * 4 + 4]));
                votes[column][class as usize] += 1;
            }
        }
        for column in 0..columns {
            let covered = covered[column];
            if covered * 2 < area {
                out.rgba.extend_from_slice(&[0; 4]);
                out.classes.push(MaskClass::Plain);
                continue;
            }
            let sum = sums[column];
            let mean = |channel: usize| ((sum[channel] + covered / 2) / covered) as u8;
            out.rgba
                .extend_from_slice(&[mean(0), mean(1), mean(2), 255]);
            let votes = votes[column];
            let class = [MaskClass::Plain, MaskClass::House, MaskClass::Unlit]
                .into_iter()
                .max_by_key(|class| (votes[*class as usize], *class == MaskClass::Plain))
                .unwrap();
            out.classes.push(class);
        }
    }
    out
}

/// A mask pixel's class: red at 128 or more marks house colour, else green at
/// 128 or more marks an unlit pixel (`docs/picture-model-art.md`).
fn mask_class(pixel: &[u8]) -> MaskClass {
    if pixel[0] >= 128 {
        MaskClass::House
    } else if pixel[1] >= 128 {
        MaskClass::Unlit
    } else {
        MaskClass::Plain
    }
}

/// A house-colour pixel's step on the owner's ramp: its brightness, the
/// largest of red, green and blue, picks it, 255 the lightest (ramp entry 0,
/// palette index 16) and every 17 levels darker one step down.
fn ramp_step(pixel: [u8; 4]) -> usize {
    let brightness = pixel[0].max(pixel[1]).max(pixel[2]);
    usize::from((255 - brightness) / 17).min(RAMP_SIZE - 1)
}

/// The [`RAMP_STRIP`] of `ramps`: every house colour index's ramp, RGBA.
pub(crate) fn ramp_strip(ramps: &crate::rules::house_colors::HouseColorRamps) -> Vec<u8> {
    let [width, height] = RAMP_STRIP;
    let mut strip = vec![0; (width * height * 4) as usize];
    for house in 0..=u8::MAX {
        let ramp = ramps.ramp(crate::rules::house_colors::HouseColorIndex(house));
        let [x, y] = [u32::from(house % 16) * 16, u32::from(house / 16)];
        for (step, color) in ramp.iter().enumerate() {
            let at = ((y * width + x + step as u32) * 4) as usize;
            strip[at..at + 4].copy_from_slice(&[color.r, color.g, color.b, 255]);
        }
    }
    strip
}

/// One frame of one picture file as the sprite atlas holds it: drawn once
/// for every atlas key that draws it, whatever the section, house colour or
/// draw offsets. Keys that take a house colour share the frame drawn
/// [`HouseColor::Deferred`]; keys that keep the picture's own colours share
/// the one drawn [`HouseColor::Own`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PictureFrameId {
    /// The file found and its `PictureSize=`, as [`PictureSources`] decodes it.
    pub file: String,
    pub size: Option<[i32; 2]>,
    pub frame: usize,
    pub deferred: bool,
}

/// A frame drawn for the sprite atlas: its picture's canvas and its sprite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PictureFrame {
    pub canvas: [u32; 2],
    pub sprite: PictureSprite,
}

/// One atlas key drawn from a picture frame, placed as `render_shp_frame`
/// places an SHP frame (CC_Draw_Shape `0x004AED70` centres the canvas and
/// adds the frame's origin, and the art's XDrawOffset/YDrawOffset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PictureKey {
    pub key: crate::render::sprite_atlas::ShpSpriteKey,
    pub frame: PictureFrameId,
    pub draw_offsets: (i32, i32),
    /// The SHP frame format bit that lets the extended walker consume the
    /// BUILDNGZ shape (`ShpSpriteEntry::extended`). A picture has no frame
    /// format, so the caller sets it for the art whose SHP frames carry it:
    /// building bodies and Buildup.
    pub extended: bool,
    /// The house colour its house-colour pixels take, for a frame drawn
    /// [`HouseColor::Deferred`].
    pub house: Option<crate::rules::house_colors::HouseColorIndex>,
}

impl PictureKey {
    /// The key's `ShpSpriteEntry::source_palette`: the RGBA source mode, with
    /// its house colour when it has one.
    pub(crate) fn source_palette(&self) -> u32 {
        self.house
            .map_or(0, |house| PICTURE_HOUSE | u32::from(house.0))
    }

    /// The key's offset from its draw point to the sprite's top-left corner,
    /// and its canvas `[x, y, width, height]` from the draw point, for a
    /// frame drawn at `origin` on `canvas`.
    pub(crate) fn placement(&self, canvas: [u32; 2], origin: [u32; 2]) -> ([f32; 2], [f32; 4]) {
        let [width, height] = canvas;
        // -ShapeWidth/2, truncated, as `render_shp_frame`.
        let left = -((width / 2) as f32) + self.draw_offsets.0 as f32;
        let top = -((height / 2) as f32) + self.draw_offsets.1 as f32;
        (
            [left + origin[0] as f32, top + origin[1] as f32],
            [left, top, width as f32, height as f32],
        )
    }
}

/// What one sprite atlas build adds from pictures: the frames it drew, the
/// keys drawn from them or from frames the atlas already holds, and the
/// house ramps new pages hold ([`ramp_strip`]).
#[derive(Default)]
pub(crate) struct PreparedPictures {
    pub frames: Vec<(PictureFrameId, PictureFrame)>,
    /// New frames that draw nothing.
    pub empty: Vec<PictureFrameId>,
    pub keys: Vec<PictureKey>,
    pub ramps: Vec<u8>,
}

/// Where an atlas key's frames are drawn from: the art section that declares
/// `Picture=`, which gives the theater substitution and the log name, and the
/// picture, the section's own `Picture=` or one a building's `Buildup=` or
/// `BibShape=` names beside it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PictureSource {
    pub section: String,
    pub decl: crate::rules::art_data::PictureDecl,
}

impl PictureSource {
    /// The picture `name` names beside this section's `Picture=`: `NAME.png`,
    /// on the same `PictureSize=` canvas (a building's `Buildup=` and
    /// `BibShape=`, `docs/picture-model-art.md`).
    pub(crate) fn named(&self, name: &str) -> Self {
        Self {
            section: self.section.clone(),
            decl: crate::rules::art_data::PictureDecl {
                file: format!("{name}.png"),
                size: self.decl.size,
            },
        }
    }
}

/// The pictures one atlas build draws, by [`PictureSource`]. Each picture
/// file is decoded and shrunk once, however many sections draw it (a
/// building's damaged and rubble animations often name one file). A picture
/// that is missing or does not fit its canvas is logged once per section and
/// draws nothing.
#[derive(Default)]
pub(crate) struct PictureSources {
    decoded: std::collections::HashMap<PictureSource, Option<std::sync::Arc<GamePictureFrames>>>,
    /// Each file decoded so far, by the file found and the canvas it was
    /// shrunk to.
    files: std::collections::HashMap<PictureFile, DecodedFile>,
}

/// A picture file as [`shrink_picture`] decodes it: the file found and the
/// declared `PictureSize=`.
type PictureFile = (String, Option<[i32; 2]>);
type DecodedFile = Result<std::sync::Arc<GamePictureFrames>, std::sync::Arc<PictureArtError>>;

impl PictureSources {
    /// `source`'s picture at game pixels, decoding it on first use.
    pub(crate) fn frames(
        &mut self,
        source: &PictureSource,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) -> Option<&GamePictureFrames> {
        if !self.decoded.contains_key(source) {
            let frames = locate_source(source, art, asset_manager, theater_name).and_then(
                |(file, bytes)| {
                    let decoded = self
                        .files
                        .entry((file.clone(), source.decl.size))
                        .or_insert_with(|| decode_file(&file, bytes, &source.decl, asset_manager));
                    report_decoded(source, &file, decoded)
                },
            );
            self.decoded.insert(source.clone(), frames);
        }
        self.decoded_frames(source)
    }

    /// Decode each of `sources` not decoded yet, every file once, side by
    /// side on the worker pool, so the atlas build that follows finds them
    /// all decoded. A map whose art declares many pictures otherwise decodes
    /// and shrinks them one by one on the loading thread. Logs and art
    /// problems are told in the order of `sources`, as [`Self::frames`]
    /// would tell them.
    pub(crate) fn prefetch<'s>(
        &mut self,
        sources: impl IntoIterator<Item = &'s PictureSource>,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) {
        let mut seen = std::collections::HashSet::new();
        let todo: Vec<(&PictureSource, Option<(String, &[u8])>)> = sources
            .into_iter()
            .filter(|source| !self.decoded.contains_key(*source) && seen.insert(*source))
            .map(|source| {
                (
                    source,
                    locate_source(source, art, asset_manager, theater_name),
                )
            })
            .collect();
        let mut new_files = std::collections::HashSet::new();
        let files: Vec<(PictureFile, &[u8], &crate::rules::art_data::PictureDecl)> = todo
            .iter()
            .filter_map(|(source, found)| {
                let (file, bytes) = found.as_ref()?;
                let key = (file.clone(), source.decl.size);
                (!self.files.contains_key(&key) && new_files.insert(key.clone())).then_some((
                    key,
                    *bytes,
                    &source.decl,
                ))
            })
            .collect();
        let decoded = crate::sim::parallel::map(&files, |((file, _), bytes, decl)| {
            decode_file(file, bytes, decl, asset_manager)
        });
        for ((key, _, _), decoded) in files.into_iter().zip(decoded) {
            self.files.insert(key, decoded);
        }
        for (source, found) in todo {
            let frames = found.and_then(|(file, _)| {
                report_decoded(
                    source,
                    &file,
                    &self.files[&(file.clone(), source.decl.size)],
                )
            });
            self.decoded.insert(source.clone(), frames);
        }
    }

    /// Frame `frame` of `source`'s picture, house colour as `house` says;
    /// None when it draws nothing, or when [`Self::frames`] or
    /// [`Self::prefetch`] has not decoded `source`. Reads only, so frames can
    /// be drawn side by side.
    pub(crate) fn frame(
        &self,
        source: &PictureSource,
        frame: usize,
        house: HouseColor<'_>,
    ) -> Option<PictureFrame> {
        let frames = self.decoded_frames(source)?;
        Some(PictureFrame {
            canvas: frames.canvas,
            sprite: frames.sprite(frame, house)?,
        })
    }

    /// `BuildingTypeClass::GetRenderDimensions` (`0x00465570`) of a building
    /// drawn from pictures: [`building_render_bounds`] of its `body`,
    /// `buildup` and `bib` pictures. A buildup or bib picture that cannot be
    /// drawn is left out, as an SHP the theater lacks; a body that cannot be
    /// drawn gives the empty rectangle, as a missing body SHP does.
    pub(crate) fn building_bounds(
        &mut self,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
        body: &PictureSource,
        buildup: Option<&PictureSource>,
        bib: Option<&PictureSource>,
    ) -> [i32; 4] {
        for source in [Some(body), buildup, bib].into_iter().flatten() {
            self.frames(source, art, asset_manager, theater_name);
        }
        let Some(body) = self.decoded_frames(body) else {
            return [0; 4];
        };
        building_render_bounds(
            body,
            buildup.and_then(|source| self.decoded_frames(source)),
            bib.and_then(|source| self.decoded_frames(source)),
        )
    }

    /// `source`'s picture if [`Self::frames`] decoded it.
    fn decoded_frames(&self, source: &PictureSource) -> Option<&GamePictureFrames> {
        self.decoded.get(source).and_then(Option::as_deref)
    }
}

/// `BuildingTypeClass::GetRenderDimensions` (`0x00465570`) over pictures, as
/// `sprite_atlas::building_type_render_bounds` folds an SHP's frame
/// rectangles: every body frame, every buildup frame, then bib frame 0, in
/// that order, centred by the body's canvas only.
pub(crate) fn building_render_bounds(
    body: &GamePictureFrames,
    buildup: Option<&GamePictureFrames>,
    bib: Option<&GamePictureFrames>,
) -> [i32; 4] {
    let mut accumulated = None;
    for frames in [Some(body), buildup].into_iter().flatten() {
        for frame in 0..frames.frame_count() {
            crate::util::rect::union_native_draw_bounds(
                &mut accumulated,
                frames.frame_bounds(frame),
            );
        }
    }
    if let Some(bib) = bib {
        crate::util::rect::union_native_draw_bounds(&mut accumulated, bib.frame_bounds(0));
    }
    let mut bounds = accumulated.unwrap_or([0; 4]);
    let canvas = body
        .canvas
        .map(|side| i32::try_from(side).unwrap_or(i32::MAX));
    bounds[0] -= canvas[0] / 2;
    bounds[1] -= canvas[1] / 2;
    bounds
}

/// The file `source` loads in `theater_name`; None when no candidate file is
/// there.
pub(crate) fn picture_file(
    source: &PictureSource,
    art: &crate::rules::art_data::ArtRegistry,
    asset_manager: &crate::assets::asset_manager::AssetManager,
    theater_name: &str,
) -> Option<String> {
    locate_source(source, art, asset_manager, theater_name).map(|(file, _)| file)
}

/// The file `source` loads in `theater_name` and its bytes; None when no
/// candidate file is there.
fn locate_source<'a>(
    source: &PictureSource,
    art: &crate::rules::art_data::ArtRegistry,
    asset_manager: &'a crate::assets::asset_manager::AssetManager,
    theater_name: &str,
) -> Option<(String, &'a [u8])> {
    art.picture_candidates(&source.section, &source.decl, theater_name)
        .into_iter()
        .find_map(|file| asset_manager.get_ref(&file).map(|bytes| (file, bytes)))
}

/// [`shrink_picture`], shared by every source that draws `file`.
fn decode_file(
    file: &str,
    bytes: &[u8],
    decl: &crate::rules::art_data::PictureDecl,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> DecodedFile {
    shrink_picture(file, bytes, decl, asset_manager)
        .map(std::sync::Arc::new)
        .map_err(std::sync::Arc::new)
}

/// The picture `file` (`bytes`), with its mask beside it, shrunk to the
/// game pixels `decl` gives.
fn shrink_picture(
    file: &str,
    bytes: &[u8],
    decl: &crate::rules::art_data::PictureDecl,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> Result<GamePictureFrames, PictureArtError> {
    let mask_name = format!(
        "{}.mask.png",
        file.rsplit_once('.').map_or(file, |(stem, _)| stem)
    );
    let decoded = PictureFrames::decode(bytes).and_then(|picture| {
        let mask = asset_manager
            .get_ref(&mask_name)
            .map(PictureFrames::decode)
            .transpose()?;
        Ok((picture, mask))
    });
    decoded
        .map_err(PictureArtError::from)
        .and_then(|(picture, mask)| {
            let canvas = decl.size.map(|size| size.map(|side| side.unsigned_abs()));
            GamePictureFrames::shrink(picture, mask.as_ref(), canvas)
        })
}

/// Log `source`'s picture `file` as decoded, or why it cannot be drawn, as
/// an art problem told in the match.
fn report_decoded(
    source: &PictureSource,
    file: &str,
    decoded: &DecodedFile,
) -> Option<std::sync::Arc<GamePictureFrames>> {
    let (section, decl) = (source.section.as_str(), &source.decl);
    match decoded {
        Ok(frames) => {
            log::info!(
                "[{section}] {file}: {} frames of {}x{}",
                frames.frame_count(),
                frames.canvas[0],
                frames.canvas[1]
            );
            Some(frames.clone())
        }
        Err(error) => {
            log::warn!("[{section}] {file} cannot be drawn: {error}");
            crate::rules::art_problems::report(
                section,
                &format!("Picture={}", decl.file),
                &format!("{file} cannot be drawn ({error})"),
                "it draws nothing",
            );
            None
        }
    }
}

/// A destroyed building's rubble frames, for each of `arts` in order, when
/// its art declares `Picture=`: the body picture rubble binding resolved
/// (`RubbleArt::shp`), at game pixels. None for SHP rubble and for a picture
/// that cannot be drawn. Each declared picture file is decoded and shrunk
/// once, side by side on the worker pool (each is a whole building body),
/// then logged and reported in `arts`' order. All None, and no pool work,
/// when none declares a picture.
pub(crate) fn rubble_pictures(
    arts: &[crate::rules::building_rubble::RubbleArt],
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> Vec<Option<std::sync::Arc<GamePictureFrames>>> {
    if arts.iter().all(|art| art.picture.is_none()) {
        return arts.iter().map(|_| None).collect();
    }
    let mut seen = std::collections::HashSet::new();
    let files: Vec<(PictureFile, &[u8], &crate::rules::art_data::PictureDecl)> = arts
        .iter()
        .filter_map(|art| {
            let decl = art.picture.as_ref()?;
            let key = (art.shp.clone(), decl.size);
            if !seen.insert(key.clone()) {
                return None;
            }
            Some((key, asset_manager.get_ref(&art.shp)?, decl))
        })
        .collect();
    let decoded = crate::sim::parallel::map(&files, |((file, _), bytes, decl)| {
        decode_file(file, bytes, decl, asset_manager)
    });
    let decoded: std::collections::HashMap<PictureFile, DecodedFile> = files
        .into_iter()
        .map(|(key, _, _)| key)
        .zip(decoded)
        .collect();
    arts.iter()
        .map(|art| {
            let decl = art.picture.clone()?;
            let decoded = decoded.get(&(art.shp.clone(), decl.size))?;
            let source = PictureSource {
                section: art.shp.clone(),
                decl,
            };
            report_decoded(&source, &art.shp, decoded)
        })
        .collect()
}

/// One RGBA source page of picture sprites: its size, pixels, index plane
/// and the frames on it with their top-left corners. Its top left holds the
/// house ramps ([`RAMP_STRIP`]).
pub(crate) struct PicturePage {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    pub frames: Vec<(PagedFrame, [u32; 2])>,
    /// VERA-only, experimental: for a page of art above 1x, its twin at the
    /// art's own resolution (`render::picture_zoom`).
    pub hd: Option<HdPage>,
}

/// A frame on a page: what an atlas entry drawn from it needs.
pub(crate) struct PagedFrame {
    pub id: PictureFrameId,
    pub canvas: [u32; 2],
    pub origin: [u32; 2],
    pub size: [u32; 2],
}

/// A picture page `scale` times larger: each sprite's [`HdSprite`] at
/// `scale` times its place on the 1x page, so a UV on one page addresses the
/// same picture point on the other.
pub(crate) struct HdPage {
    pub scale: u32,
    pub size: [u32; 2],
    /// Premultiplied RGBA, level 0.
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    /// The house layer ([`HdSprite::house`]), two bytes a texel, when a
    /// frame on the page has one.
    pub house: Option<Vec<u8>>,
}

impl HdPage {
    /// The mip levels the zoomed draw samples: down to one texel per game
    /// pixel (`log2(scale)` halvings), each the 2x2 mean of the level above
    /// (premultiplied, so holes do not darken edges). The RGBA levels one
    /// after another, level 0 first, then the count.
    pub(crate) fn mip_chain(&self) -> (Vec<u8>, u32) {
        let levels = self.levels();
        (mip_chain(&self.rgba, self.size, 4, levels), levels)
    }

    /// [`Self::mip_chain`] of the house layer, when the page has one.
    pub(crate) fn house_mip_chain(&self) -> Option<Vec<u8>> {
        let levels = self.levels();
        Some(mip_chain(self.house.as_ref()?, self.size, 2, levels))
    }

    fn levels(&self) -> u32 {
        (self.scale.ilog2() + 1).min(self.size[0].max(self.size[1]).ilog2() + 1)
    }
}

/// `levels` mip levels of `level0`, `channels` bytes a texel: each the 2x2
/// mean of the level above, one after another, level 0 first.
fn mip_chain(level0: &[u8], size: [u32; 2], channels: usize, levels: u32) -> Vec<u8> {
    let mut chain = level0.to_vec();
    let (mut level, mut size) = (level0.to_vec(), size);
    for _ in 1..levels {
        let next = size.map(|side| (side / 2).max(1));
        let mut out = Vec::with_capacity(next[0] as usize * next[1] as usize * channels);
        for y in 0..next[1] {
            for x in 0..next[0] {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(size[0] - 1);
                    let sy = (y * 2 + dy).min(size[1] - 1);
                    let at = (sy * size[0] + sx) as usize * channels;
                    for channel in 0..channels {
                        sum[channel] += u32::from(level[at + channel]);
                    }
                }
                out.extend(sum[..channels].iter().map(|total| ((total + 2) / 4) as u8));
            }
        }
        chain.extend_from_slice(&out);
        (level, size) = (out, next);
    }
    chain
}

/// Pack `frames` onto pages no wider or taller than `max_side`, tallest
/// first in shelves, after the house ramps each page holds at its top left
/// (`ramps`, [`RAMP_STRIP`]). A sprite larger than a page is left out,
/// logged.
///
/// Art of each scale above 1 packs onto pages of its own, a page side of at
/// most `max_side / scale` so its HD twin fits too, with one empty game
/// pixel between sprites so the twin's filtering stays inside each sprite.
pub(crate) fn pack_picture_pages(
    frames: Vec<(PictureFrameId, PictureFrame)>,
    ramps: &[u8],
    max_side: u32,
) -> Vec<PicturePage> {
    let mut by_scale: std::collections::BTreeMap<u32, Vec<(PictureFrameId, PictureFrame)>> =
        Default::default();
    for frame in frames {
        let scale = frame.1.sprite.hd.as_ref().map_or(1, |hd| hd.scale);
        by_scale.entry(scale).or_default().push(frame);
    }
    by_scale
        .into_iter()
        .flat_map(|(scale, group)| pack_scale(group, ramps, max_side / scale, scale))
        .collect()
}

/// [`pack_picture_pages`] for frames of one `scale`.
fn pack_scale(
    mut frames: Vec<(PictureFrameId, PictureFrame)>,
    ramps: &[u8],
    max_side: u32,
    scale: u32,
) -> Vec<PicturePage> {
    let gap = u32::from(scale > 1);
    frames.sort_by_key(|(_, frame)| std::cmp::Reverse(frame.sprite.size[1]));
    let mut placed: Vec<Vec<((PictureFrameId, PictureFrame), [u32; 2])>> = Vec::new();
    let mut extents: Vec<[u32; 2]> = Vec::new();
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for (id, frame) in frames {
        let [width, height] = frame.sprite.size;
        // A fresh page fits it beside or below the ramps.
        if width > max_side || height + RAMP_STRIP[1] + gap > max_side || max_side < RAMP_STRIP[0] {
            log::warn!(
                "[{}] frame {} is {width}x{height}, larger than a {max_side} page",
                id.file,
                id.frame
            );
            crate::rules::art_problems::report(
                &id.file,
                "Picture=",
                &format!("a frame is larger than a {max_side}x{max_side} page"),
                "such frames draw nothing",
            );
            continue;
        }
        if !placed.is_empty() && x + width > max_side {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if placed.is_empty() || y + height > max_side {
            placed.push(Vec::new());
            // The ramps open the page's first shelf.
            extents.push(RAMP_STRIP);
            (x, y, shelf) = (RAMP_STRIP[0] + gap, 0, RAMP_STRIP[1] + gap);
            if x + width > max_side {
                (x, y, shelf) = (0, shelf, 0);
            }
        }
        let extent = extents.last_mut().unwrap();
        *extent = [extent[0].max(x + width), extent[1].max(y + height)];
        placed.last_mut().unwrap().push(((id, frame), [x, y]));
        x += width + gap;
        shelf = shelf.max(height + gap);
    }
    placed
        .into_iter()
        .zip(extents)
        .map(|(frames, size)| {
            let mut rgba = vec![0; (size[0] * size[1] * 4) as usize];
            let mut indices = vec![0; (size[0] * size[1]) as usize];
            let [strip_width, strip_height] = RAMP_STRIP;
            for row in 0..strip_height as usize {
                let width = strip_width as usize * 4;
                let to = row * size[0] as usize * 4;
                rgba[to..to + width].copy_from_slice(&ramps[row * width..(row + 1) * width]);
            }
            for ((_, frame), at) in &frames {
                let sprite = &frame.sprite;
                blit(
                    &mut rgba,
                    &mut indices,
                    size[0],
                    *at,
                    sprite.size,
                    &sprite.rgba,
                    &sprite.indices,
                );
            }
            let hd = (scale > 1).then(|| {
                let hd_size = size.map(|side| side * scale);
                let texels = (hd_size[0] * hd_size[1]) as usize;
                let mut rgba = vec![0; texels * 4];
                let mut indices = vec![0; texels];
                let mut house = frames
                    .iter()
                    .any(|((_, frame), _)| {
                        frame
                            .sprite
                            .hd
                            .as_ref()
                            .is_some_and(|hd| hd.house.is_some())
                    })
                    .then(|| vec![0; texels * 2]);
                for ((_, frame), at) in &frames {
                    if let Some(hd) = &frame.sprite.hd {
                        let at = at.map(|side| side * scale);
                        blit(
                            &mut rgba,
                            &mut indices,
                            hd_size[0],
                            at,
                            hd.size,
                            &hd.rgba,
                            &hd.indices,
                        );
                        if let (Some(page), Some(layer)) = (house.as_mut(), &hd.house) {
                            blit_plane(page, hd_size[0], at, hd.size, layer, 2);
                        }
                    }
                }
                HdPage {
                    scale,
                    size: hd_size,
                    rgba,
                    indices,
                    house,
                }
            });
            PicturePage {
                size,
                rgba,
                indices,
                frames: frames
                    .into_iter()
                    .map(|((id, frame), at)| {
                        (
                            PagedFrame {
                                id,
                                canvas: frame.canvas,
                                origin: frame.sprite.origin,
                                size: frame.sprite.size,
                            },
                            at,
                        )
                    })
                    .collect(),
                hd,
            }
        })
        .collect()
}

/// Copy a `size` sprite's RGBA and index plane to `at` on a page
/// `page_width` wide.
fn blit(
    page_rgba: &mut [u8],
    page_indices: &mut [u8],
    page_width: u32,
    at: [u32; 2],
    size: [u32; 2],
    rgba: &[u8],
    indices: &[u8],
) {
    blit_plane(page_rgba, page_width, at, size, rgba, 4);
    blit_plane(page_indices, page_width, at, size, indices, 1);
}

/// Copy a `size` plane of `channels` bytes a texel to `at` on a page plane
/// `page_width` wide.
fn blit_plane(
    page: &mut [u8],
    page_width: u32,
    [x, y]: [u32; 2],
    [width, height]: [u32; 2],
    plane: &[u8],
    channels: usize,
) {
    let row_bytes = width as usize * channels;
    for row in 0..height as usize {
        let to = ((y as usize + row) * page_width as usize + x as usize) * channels;
        page[to..to + row_bytes].copy_from_slice(&plane[row * row_bytes..(row + 1) * row_bytes]);
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, frames: Vec<Vec<u8>>) -> PictureFrames {
        PictureFrames {
            width,
            height,
            frames,
        }
    }

    /// A [`RAMP_STRIP`] whose bytes on row `y` are all `y + 1`, so a page's
    /// copy shows each row landed where it belongs.
    fn row_marked_ramps() -> Vec<u8> {
        let [width, height] = RAMP_STRIP;
        (0..height)
            .flat_map(|row| std::iter::repeat_n(row as u8 + 1, (width * 4) as usize))
            .collect()
    }

    /// Whether `page` (`width` texels wide) holds `ramps` at its top left.
    fn holds_ramps(page: &[u8], width: u32, ramps: &[u8]) -> bool {
        let row_bytes = (RAMP_STRIP[0] * 4) as usize;
        (0..RAMP_STRIP[1] as usize).all(|row| {
            let at = row * width as usize * 4;
            page[at..at + row_bytes] == ramps[row * row_bytes..(row + 1) * row_bytes]
        })
    }

    fn frame_id(frame: usize) -> PictureFrameId {
        PictureFrameId {
            file: "BOOM.png".into(),
            size: None,
            frame,
            deferred: false,
        }
    }

    /// A 1x frame `size` large, every pixel `value`, at canvas origin 0.
    fn plain_frame(frame: usize, size: [u32; 2], value: u8) -> (PictureFrameId, PictureFrame) {
        (
            frame_id(frame),
            PictureFrame {
                canvas: size,
                sprite: PictureSprite {
                    origin: [0, 0],
                    size,
                    rgba: vec![value; (size[0] * size[1] * 4) as usize],
                    indices: vec![LIT_INDEX; (size[0] * size[1]) as usize],
                    hd: None,
                },
            },
        )
    }

    /// Each page's frames, by frame number, with their places.
    fn placements(pages: &[PicturePage]) -> Vec<Vec<(usize, [u32; 2])>> {
        pages
            .iter()
            .map(|page| {
                page.frames
                    .iter()
                    .map(|(frame, at)| (frame.id.frame, *at))
                    .collect()
            })
            .collect()
    }

    /// A 2x picture shrinks onto its canvas: a game pixel is drawn when half
    /// its picture pixels are, takes their mean colour and its mask's class,
    /// and the sprite is cropped to the drawn pixels.
    #[test]
    fn a_2x_picture_shrinks_by_coverage_and_crops_like_an_shp_frame() {
        const RED: [u8; 4] = [200, 0, 0, 255];
        const BLUE: [u8; 4] = [0, 0, 100, 255];
        const NONE: [u8; 4] = [0; 4];
        // 4x2 picture pixels make a 2x1 canvas. Left game pixel: two red
        // and two blue (drawn, mean colour). Right: one red only (not drawn).
        let rows = [[RED, BLUE, RED, NONE], [BLUE, RED, NONE, NONE]];
        let frame: Vec<u8> = rows.concat().concat();
        let mask_rows = [
            [[0, 255, 0, 255]; 4],
            [[0, 255, 0, 255], [0, 0, 0, 255], [0; 4], [0; 4]],
        ];
        let mask: Vec<u8> = mask_rows.concat().concat();
        let shrunk = GamePictureFrames::shrink(
            picture(4, 2, vec![frame]),
            Some(&picture(4, 2, vec![mask])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = shrunk.sprite(0, HouseColor::Own).unwrap();
        assert_eq!((sprite.origin, sprite.size), ([0, 0], [1, 1]));
        assert_eq!(sprite.rgba, [100, 0, 50, 255]);
        assert_eq!(sprite.indices, [UNLIT_INDEX], "three of four are unlit");
        assert!(shrunk.sprite(1, HouseColor::Own).is_none());
    }

    /// A house-colour pixel baked with the owner's ramp takes the ramp step
    /// its brightness picks, and a picture whose size is not a whole
    /// multiple of its canvas is refused.
    #[test]
    fn house_pixels_take_the_ramp_by_brightness_and_bad_scales_are_refused() {
        let ramp: [Color; RAMP_SIZE] = std::array::from_fn(|step| Color::rgb(step as u8, 0, 0));
        let frame = vec![255, 0, 0, 255, 0, 100, 0, 255];
        let mask = vec![255, 0, 0, 255, 255, 0, 0, 255];
        let shrunk = GamePictureFrames::shrink(
            picture(2, 1, vec![frame.clone()]),
            Some(&picture(2, 1, vec![mask])),
            None,
        )
        .unwrap();
        assert!(shrunk.has_house_color());
        let sprite = shrunk.sprite(0, HouseColor::Baked(&ramp)).unwrap();
        // 255 is step 0; 100 is (255 - 100) / 17 = step 9.
        assert_eq!(sprite.rgba, [0, 0, 0, 255, 9, 0, 0, 255]);
        assert_eq!(sprite.indices, [LIT_INDEX, LIT_INDEX]);
        assert!(GamePictureFrames::shrink(picture(2, 1, vec![frame]), None, Some([3, 1])).is_err());
    }

    /// A deferred house-colour pixel keeps its own colour at 1x and carries
    /// its ramp step in its index, [`HOUSE_INDEX`] plus the step, for the
    /// draw to colour. At HD it is black with the same index, and the house
    /// layer holds its step for the zoomed draw; other texels, holes
    /// included, are 0 on that layer. Its own colours draw no house layer.
    #[test]
    fn deferred_house_pixels_carry_their_ramp_step_in_the_index_and_house_layer() {
        const H: [u8; 4] = [100, 0, 0, 255];
        const P: [u8; 4] = [50, 50, 50, 255];
        const NONE: [u8; 4] = [0; 4];
        const RED: [u8; 4] = [255, 0, 0, 255];
        // A 1x picture: a house pixel at step 0, one at step 9, a plain one.
        let shrunk = GamePictureFrames::shrink(
            picture(3, 1, vec![[RED, H, P].concat()]),
            Some(&picture(3, 1, vec![[RED, RED, [0, 0, 0, 255]].concat()])),
            None,
        )
        .unwrap();
        let sprite = shrunk.sprite(0, HouseColor::Deferred).unwrap();
        assert_eq!(sprite.rgba, [RED, H, P].concat(), "colours untouched");
        assert_eq!(sprite.indices, [HOUSE_INDEX, HOUSE_INDEX + 9, LIT_INDEX]);
        assert!(sprite.hd.is_none());

        // A 2x picture on a 2x1 canvas: the left game pixel is three house
        // pixels at step 9 and a hole, the right one plain.
        let rows = [[H, H, P, P], [NONE, H, P, P]];
        let mask_rows = [[RED, RED, NONE, NONE], [NONE, RED, NONE, NONE]];
        let shrunk = GamePictureFrames::shrink(
            picture(4, 2, vec![rows.concat().concat()]),
            Some(&picture(4, 2, vec![mask_rows.concat().concat()])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = shrunk.sprite(0, HouseColor::Deferred).unwrap();
        assert_eq!(sprite.rgba, [H, P].concat());
        assert_eq!(sprite.indices, [HOUSE_INDEX + 9, LIT_INDEX]);
        let hd = sprite.hd.unwrap();
        const BLACK: [u8; 4] = [0, 0, 0, 255];
        assert_eq!(
            hd.rgba,
            [[BLACK, BLACK, P, P], [NONE, BLACK, P, P]]
                .concat()
                .concat()
        );
        let house = HOUSE_INDEX + 9;
        assert_eq!(
            hd.indices,
            [
                house, house, LIT_INDEX, LIT_INDEX, 0, house, LIT_INDEX, LIT_INDEX
            ]
        );
        let on = [9 * HOUSE_LAYER_STEP, 255];
        assert_eq!(
            hd.house.unwrap(),
            [[on, on, [0; 2], [0; 2]], [[0; 2], on, [0; 2], [0; 2]]]
                .concat()
                .concat()
        );
        let own = shrunk.sprite(0, HouseColor::Own).unwrap().hd.unwrap();
        assert!(own.house.is_none());
        assert_eq!(own.rgba[..4], H, "its own colour");
    }

    /// The ramp strip holds house colour `h`'s step `n` at
    /// `((h % 16) * 16 + n, h / 16)`, opaque, for every index: the
    /// `[Colors]` entries, the fallback past them and the relation bands.
    #[test]
    fn the_ramp_strip_holds_each_house_colours_ramp_in_its_cell() {
        use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps, RELATION_ENEMY};
        let scheme = |name: &str, hsv: [u8; 3]| crate::rules::color_scheme::ColorSchemeEntry {
            name: name.into(),
            hsv,
        };
        let ramps = HouseColorRamps::from_schemes(&[
            scheme("A", [0, 230, 255]),
            scheme("B", [153, 214, 212]),
            scheme("C", [0, 0, 240]),
        ]);
        let strip = ramp_strip(&ramps);
        let [width, height] = RAMP_STRIP;
        assert_eq!(strip.len(), (width * height * 4) as usize);
        for house in [1u8, 17, RELATION_ENEMY.0, 255] {
            let ramp = ramps.ramp(HouseColorIndex(house));
            for (step, color) in ramp.iter().enumerate() {
                let x = u32::from(house % 16) * 16 + step as u32;
                let at = ((u32::from(house / 16) * width + x) * 4) as usize;
                assert_eq!(
                    strip[at..at + 4],
                    [color.r, color.g, color.b, 255],
                    "house {house} step {step}"
                );
            }
        }
    }

    /// Frames pack in shelves, tallest first, onto as few pages as fit, after
    /// the ramp strip each page holds at its top left: beside it on the first
    /// shelf when they fit there, else below it. Each page holds their
    /// pixels and index planes where they were placed, and a frame no fresh
    /// page fits beside the strip is left out.
    #[test]
    fn pictures_pack_in_shelves_after_the_ramp_strip_and_keep_their_pixels() {
        let ramps = row_marked_ramps();
        let pages = pack_picture_pages(
            vec![
                plain_frame(0, [3, 1], 10),
                plain_frame(1, [2, 2], 20),
                plain_frame(2, [40, 4], 30),
                plain_frame(3, [30, 280], 40),
                // 290 + the strip's 16 is past a 300 page.
                plain_frame(4, [1, 290], 50),
                plain_frame(5, [30, 280], 60),
            ],
            &ramps,
            300,
        );
        assert_eq!(
            placements(&pages),
            [
                vec![(3, [256, 0])],
                vec![(5, [256, 0]), (2, [0, 280]), (1, [40, 280]), (0, [42, 280])],
            ]
        );
        assert_eq!((pages[0].size, pages[1].size), ([286, 280], [286, 284]));
        for page in &pages {
            assert!(holds_ramps(&page.rgba, page.size[0], &ramps));
            assert_eq!(page.indices[0], 0, "the strip is no sprite");
        }
        let width = pages[1].size[0] as usize;
        let page = &pages[1];
        // Frame 5 beside the strip, frame 1 on row 281, frame 0 on row 280
        // and nothing after it.
        assert_eq!(page.rgba[256 * 4], 60);
        assert_eq!(page.rgba[(281 * width + 41) * 4], 20);
        assert_eq!(page.rgba[(280 * width + 42) * 4], 10);
        assert_eq!(page.indices[280 * width + 45], 0);
        assert_eq!(page.indices[281 * width + 42], 0, "nothing below frame 0");
    }

    /// A frame too wide to sit beside the ramp strip opens the page below it,
    /// a lone small frame still makes a page the strip fits on, and a page
    /// narrower than the strip takes no frame.
    #[test]
    fn a_frame_too_wide_for_the_strips_shelf_goes_below_it() {
        let ramps = row_marked_ramps();
        let pages = pack_picture_pages(
            vec![plain_frame(0, [50, 3], 10), plain_frame(1, [3, 2], 20)],
            &ramps,
            300,
        );
        assert_eq!(placements(&pages), [vec![(0, [0, 16]), (1, [50, 16])]]);
        assert_eq!(pages[0].size, [256, 19]);
        assert!(holds_ramps(&pages[0].rgba, 256, &ramps));
        assert_eq!(pages[0].rgba[(16 * 256 + 50) * 4], 20);

        let lone = pack_picture_pages(vec![plain_frame(0, [1, 1], 10)], &ramps, 300);
        assert_eq!(placements(&lone), [vec![(0, [256, 0])]]);
        assert_eq!(lone[0].size, [257, 16]);

        assert!(pack_picture_pages(vec![plain_frame(0, [1, 1], 10)], &ramps, 200).is_empty());
    }

    /// A 2x picture's sprite keeps its 1x crop at picture resolution
    /// (premultiplied holes, unlit indices), and 2x sprites pack onto a page
    /// of their own, one empty game pixel after the ramp strip and between
    /// sprites, whose HD twin holds each at twice its 1x place, so the 1x
    /// entry's UV addresses its HD rectangle unchanged.
    #[test]
    fn hd_sprites_keep_the_crop_and_sit_at_the_same_uv_on_the_twin_page() {
        const RED: [u8; 4] = [200, 0, 0, 255];
        const NONE: [u8; 4] = [0; 4];
        // Canvas 2x1 of a 4x2 picture: the left game pixel has three of four
        // pixels drawn (drawn), the right one has one (a hole at 1x).
        let rows = [[RED, NONE, RED, NONE], [RED, RED, NONE, NONE]];
        let unlit = [0, 255, 0, 255];
        let mask_rows = [[[0; 4]; 4], [[0; 4], unlit, [0; 4], [0; 4]]];
        let shrunk = GamePictureFrames::shrink(
            picture(4, 2, vec![rows.concat().concat()]),
            Some(&picture(4, 2, vec![mask_rows.concat().concat()])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = shrunk.sprite(0, HouseColor::Own).unwrap();
        assert_eq!((sprite.origin, sprite.size), ([0, 0], [1, 1]));
        let hd = sprite.hd.clone().unwrap();
        assert_eq!((hd.scale, hd.size), (2, [2, 2]));
        assert_eq!(hd.rgba, [RED, NONE, RED, RED].concat());
        assert_eq!(hd.indices, [LIT_INDEX, 0, LIT_INDEX, UNLIT_INDEX]);
        assert!(hd.house.is_none());

        let placed = |frame: usize| {
            (
                frame_id(frame),
                PictureFrame {
                    canvas: shrunk.canvas,
                    sprite: sprite.clone(),
                },
            )
        };
        let mut plain = placed(9);
        plain.1.sprite.hd = None;
        let ramps = row_marked_ramps();
        // Half of 600 holds the strip and a 2x sprite at its right.
        let pages = pack_picture_pages(vec![placed(0), plain, placed(1)], &ramps, 600);
        assert_eq!(pages.len(), 2, "1x and 2x art pack apart");
        assert!(pages[0].hd.is_none());
        let page = &pages[1];
        let places: Vec<[u32; 2]> = page.frames.iter().map(|(_, at)| *at).collect();
        assert_eq!((places, page.size), (vec![[257, 0], [259, 0]], [260, 16]));
        assert!(holds_ramps(&page.rgba, page.size[0], &ramps));
        assert_eq!(page.indices[256], 0, "the gap after the strip stays empty");
        let twin = page.hd.as_ref().unwrap();
        assert_eq!((twin.scale, twin.size), (2, [520, 32]));
        assert!(twin.house.is_none(), "no frame has a house layer");
        // The second sprite's u is 259/260 on both pages: HD column 518.
        let row = |y: usize, x: usize| &twin.rgba[(y * 520 + x) * 4..(y * 520 + x + 1) * 4];
        assert_eq!(
            (row(0, 518), row(0, 519), row(1, 519)),
            (&RED[..], &NONE[..], &RED[..])
        );
        assert_eq!(twin.indices[520 + 519], UNLIT_INDEX);
        // The first sprite at HD column 514 (1x column 257).
        assert_eq!((row(0, 514), row(1, 515)), (&RED[..], &RED[..]));
        assert_eq!(
            (row(0, 512), row(1, 513), row(0, 516), row(1, 517)),
            (&NONE[..], &NONE[..], &NONE[..], &NONE[..]),
            "the gaps after the strip and between sprites stay empty"
        );
    }

    /// An HD page holds the house layer of the deferred frames on it at
    /// their HD places, zero elsewhere.
    #[test]
    fn hd_pages_hold_the_house_layer_of_deferred_frames() {
        const RED: [u8; 4] = [255, 0, 0, 255];
        let shrunk = GamePictureFrames::shrink(
            picture(2, 2, vec![[RED; 4].concat()]),
            Some(&picture(2, 2, vec![[RED; 4].concat()])),
            Some([1, 1]),
        )
        .unwrap();
        let mut id = frame_id(0);
        id.deferred = true;
        let frame = PictureFrame {
            canvas: shrunk.canvas,
            sprite: shrunk.sprite(0, HouseColor::Deferred).unwrap(),
        };
        let pages = pack_picture_pages(vec![(id, frame)], &row_marked_ramps(), 600);
        assert_eq!(pages[0].frames[0].1, [257, 0]);
        let twin = pages[0].hd.as_ref().unwrap();
        let house = twin.house.as_ref().unwrap();
        let width = twin.size[0] as usize;
        assert_eq!(house.len(), width * twin.size[1] as usize * 2);
        let at = |y: usize, x: usize| &house[(y * width + x) * 2..(y * width + x + 1) * 2];
        assert_eq!((at(0, 514), at(1, 515)), (&[0, 255][..], &[0, 255][..]));
        assert_eq!((at(0, 513), at(0, 0)), (&[0, 0][..], &[0, 0][..]));
        assert_eq!(twin.indices[514], HOUSE_INDEX);
    }

    /// An HD page's mips stop at one texel per game pixel and average
    /// premultiplied texels, so a hole does not darken its drawn neighbour.
    #[test]
    fn hd_mips_average_premultiplied_texels_down_to_game_pixels() {
        let page = HdPage {
            scale: 2,
            size: [2, 2],
            rgba: [[200, 0, 0, 255], [0; 4], [0; 4], [200, 0, 0, 255]].concat(),
            indices: vec![LIT_INDEX, 0, 0, LIT_INDEX],
            house: None,
        };
        let (chain, levels) = page.mip_chain();
        assert_eq!(levels, 2);
        assert_eq!(chain[..16], page.rgba[..]);
        // Premultiplied mean: colour 100 at alpha 128 is 200 once divided.
        assert_eq!(chain[16..], [100, 0, 0, 128]);
        assert!(page.house_mip_chain().is_none());
    }

    /// The house layer's mips are the 2x2 means of its two channels, as the
    /// colour mips are, down to the same level: partly covered game pixels
    /// keep their step once divided by the coverage.
    #[test]
    fn hd_house_mips_average_the_two_channel_layer_like_the_colour() {
        let page = HdPage {
            scale: 2,
            size: [2, 2],
            rgba: vec![0; 16],
            indices: vec![0; 4],
            house: Some([[255, 255], [0, 0], [0, 0], [153, 255]].concat()),
        };
        let chain = page.house_mip_chain().unwrap();
        assert_eq!(chain[..8], page.house.as_ref().unwrap()[..]);
        // (255 + 153) / 4 and 2 * 255 / 4, rounded.
        assert_eq!(chain[8..], [102, 128]);
        assert_eq!(chain.len(), page.mip_chain().0.len() / 2);
    }

    /// A picture key's source palette is 0, the RGBA source mode, without a
    /// house colour and [`PICTURE_HOUSE`] with the house colour in its low
    /// byte with one; it is placed as `render_shp_frame` places an SHP frame:
    /// the canvas half off, truncated, plus its draw offsets, then the
    /// frame's origin.
    #[test]
    fn a_picture_key_carries_its_house_and_is_placed_as_an_shp_frame() {
        use crate::rules::house_colors::HouseColorIndex;
        let mut key = PictureKey {
            key: crate::render::sprite_atlas::ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim,
                type_id: "BOOM".into(),
                facing: 0,
                frame: 0,
                house_color: HouseColorIndex(5),
            },
            frame: frame_id(0),
            draw_offsets: (3, -2),
            extended: false,
            house: None,
        };
        assert_eq!(key.source_palette(), 0);
        key.house = Some(HouseColorIndex(5));
        assert_eq!(key.source_palette(), PICTURE_HOUSE | 5);
        assert!(!crate::render::sprite_atlas::source_palette_indexed(
            key.source_palette()
        ));
        // -7/2 = -3 plus 3, and -4/2 = -2 less 2.
        assert_eq!(
            key.placement([7, 4], [2, 1]),
            ([2.0, -3.0], [0.0, -4.0, 7.0, 4.0])
        );
    }
}
