//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): a declared picture's frames as the SHP
//! blitters' RGBA source pages.
//!
//! Each frame is shrunk to game pixels (`PictureSize=`), cropped to its drawn
//! pixels like a stored SHP frame, and given an index plane the blitters read
//! as palette indices: 0 for a hole, [`LIT_INDEX`] for an ordinary pixel and
//! [`UNLIT_INDEX`] for one light leaves alone (the mask's green). House-colour
//! pixels (the mask's red) take the owner's ramp by brightness before upload.
//! The SHP blitters, their shaders and their Z, shadow and light rules are
//! the ones every SHP draws through; nothing here changes them.
//!
//! Art above 1x also keeps each frame at its own resolution, for the zoomed
//! draw (`render::picture_zoom`): the same crop, `scale` times larger, on HD
//! twins of its pages laid out at the same normalised coordinates.

use crate::assets::pal_file::Color;
use crate::assets::picture_file::{PictureError, PictureFrames};

/// A house colour ramp's entries, palette indices 16–31
/// (`HouseColorRamps::ramp`).
const RAMP_SIZE: usize = 16;

/// The index-plane value of an ordinary drawn pixel: any index 1..=239 is lit
/// the same way (`native_palette_word`).
pub(crate) const LIT_INDEX: u8 = 1;
/// The index-plane value of a pixel light leaves as it is (indices 240–254,
/// `palette_light.wgsl`).
pub(crate) const UNLIT_INDEX: u8 = 240;
/// The largest whole multiple a picture may have of its `PictureSize=`.
pub(crate) const PICTURE_SCALE_MAX: u32 = 8;

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
}

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
        picture: &PictureFrames,
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
        let frames = picture
            .frames
            .iter()
            .enumerate()
            .map(|(index, rgba)| {
                let mask = mask.map(|mask| mask.frames[index].as_slice());
                shrink_frame(rgba, mask, size[0], canvas, scale)
            })
            .collect();
        let full = if scale > 1 {
            picture
                .frames
                .iter()
                .enumerate()
                .map(|(index, rgba)| GameFrame {
                    rgba: rgba.clone(),
                    classes: match mask {
                        Some(mask) => mask.frames[index].chunks_exact(4).map(mask_class).collect(),
                        None => vec![MaskClass::Plain; rgba.len() / 4],
                    },
                })
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

    /// Frame `frame` cropped to its drawn pixels, house-colour pixels in
    /// `ramp` (none leaves them their own colour). None for a frame past the
    /// end or with nothing drawn, as an SHP's empty frame.
    pub(crate) fn sprite(
        &self,
        frame: usize,
        ramp: Option<&[Color; RAMP_SIZE]>,
    ) -> Option<PictureSprite> {
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
                    (_, MaskClass::House) => {
                        if let Some(ramp) = ramp {
                            let color = house_ramp_color(pixel, ramp);
                            pixel = [color.r, color.g, color.b, 255];
                        }
                        LIT_INDEX
                    }
                    (_, MaskClass::Plain) => LIT_INDEX,
                };
                rgba.extend_from_slice(&pixel);
                indices.push(index);
            }
        }
        let hd = self.hd_sprite(frame, [left, top], size, ramp);
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
        ramp: Option<&[Color; RAMP_SIZE]>,
    ) -> Option<HdSprite> {
        let source = self.full.get(frame)?;
        let scale = self.scale;
        let width = self.canvas[0] * scale;
        let hd_size = size.map(|side| side * scale);
        let pixels = (hd_size[0] * hd_size[1]) as usize;
        let (mut rgba, mut indices) = (Vec::with_capacity(pixels * 4), Vec::with_capacity(pixels));
        for y in origin[1] * scale..origin[1] * scale + hd_size[1] {
            for x in origin[0] * scale..origin[0] * scale + hd_size[0] {
                let at = (y * width + x) as usize;
                let pixel = &source.rgba[at * 4..at * 4 + 4];
                if u32::from(pixel[3]) * 2 < 255 {
                    rgba.extend_from_slice(&[0; 4]);
                    indices.push(0);
                    continue;
                }
                let (color, index) = match (source.classes[at], ramp) {
                    (MaskClass::Unlit, _) => ([pixel[0], pixel[1], pixel[2]], UNLIT_INDEX),
                    (MaskClass::House, Some(ramp)) => {
                        let color = house_ramp_color([pixel[0], pixel[1], pixel[2], 255], ramp);
                        ([color.r, color.g, color.b], LIT_INDEX)
                    }
                    _ => ([pixel[0], pixel[1], pixel[2]], LIT_INDEX),
                };
                rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
                indices.push(index);
            }
        }
        Some(HdSprite {
            scale,
            size: hd_size,
            rgba,
            indices,
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
        (left < right).then_some(([left, top], [right - left, bottom - top]))
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
        let sprite = self.sprite(frame, None)?;
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
    for y in 0..canvas[1] {
        for x in 0..canvas[0] {
            let (mut covered, mut sum) = (0u32, [0u32; 3]);
            let mut votes = [0u32; 3];
            for dy in 0..scale {
                for dx in 0..scale {
                    let at = (((y * scale + dy) * picture_width + x * scale + dx) * 4) as usize;
                    let alpha = u32::from(rgba[at + 3]);
                    if alpha * 2 < 255 {
                        continue;
                    }
                    covered += 1;
                    for channel in 0..3 {
                        sum[channel] += u32::from(rgba[at + channel]);
                    }
                    let class = mask.map_or(MaskClass::Plain, |mask| mask_class(&mask[at..at + 4]));
                    votes[class as usize] += 1;
                }
            }
            if covered * 2 < area {
                out.rgba.extend_from_slice(&[0; 4]);
                out.classes.push(MaskClass::Plain);
                continue;
            }
            let mean = |channel: usize| ((sum[channel] + covered / 2) / covered) as u8;
            out.rgba
                .extend_from_slice(&[mean(0), mean(1), mean(2), 255]);
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

/// A house-colour pixel's colour on the owner's `ramp`: its brightness, the
/// largest of red, green and blue, picks the step, 255 the lightest (ramp
/// entry 0, palette index 16) and every 17 levels darker one step down.
fn house_ramp_color(pixel: [u8; 4], ramp: &[Color; RAMP_SIZE]) -> Color {
    let brightness = pixel[0].max(pixel[1]).max(pixel[2]);
    let step = usize::from((255 - brightness) / 17).min(RAMP_SIZE - 1);
    ramp[step]
}

/// One atlas key drawn from a picture: its sprite, placed as
/// `render_shp_frame` places an SHP frame (CC_Draw_Shape `0x004AED70`
/// centres the canvas and adds the frame's origin, and the art's
/// XDrawOffset/YDrawOffset).
pub(crate) struct PlacedPicture {
    pub key: crate::render::sprite_atlas::ShpSpriteKey,
    pub sprite: PictureSprite,
    pub offset: [f32; 2],
    pub canvas_rect: [f32; 4],
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

/// The pictures one atlas build draws, each decoded and shrunk once, by
/// [`PictureSource`]. A picture that is missing or does not fit its canvas is
/// logged once and draws nothing.
#[derive(Default)]
pub(crate) struct PictureSources {
    decoded: std::collections::HashMap<PictureSource, Option<GamePictureFrames>>,
}

impl PictureSources {
    /// `source`'s picture at game pixels, decoding it on first use.
    fn frames(
        &mut self,
        source: &PictureSource,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) -> Option<&GamePictureFrames> {
        self.decoded
            .entry(source.clone())
            .or_insert_with(|| {
                let section = source.section.as_str();
                let decl = &source.decl;
                let (file, bytes) = art
                    .picture_candidates(section, decl, theater_name)
                    .into_iter()
                    .find_map(|file| asset_manager.get_ref(&file).map(|bytes| (file, bytes)))?;
                let mask_name = format!(
                    "{}.mask.png",
                    file.rsplit_once('.')
                        .map_or(file.as_str(), |(stem, _)| stem)
                );
                let decoded = PictureFrames::decode(bytes).and_then(|picture| {
                    let mask = asset_manager
                        .get_ref(&mask_name)
                        .map(PictureFrames::decode)
                        .transpose()?;
                    Ok((picture, mask))
                });
                let shrunk = decoded
                    .map_err(PictureArtError::from)
                    .and_then(|(picture, mask)| {
                        let canvas = decl.size.map(|size| size.map(|side| side.unsigned_abs()));
                        GamePictureFrames::shrink(&picture, mask.as_ref(), canvas)
                    });
                match shrunk {
                    Ok(frames) => {
                        log::info!(
                            "[{section}] {file}: {} frames of {}x{}",
                            frames.frame_count(),
                            frames.canvas[0],
                            frames.canvas[1]
                        );
                        Some(frames)
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
            })
            .as_ref()
    }

    /// `key`'s frame of `source`, placed at the art's `draw_offsets`, house
    /// colour in `ramp`; None when it draws nothing. `select_frame` picks the
    /// frame drawn from the picture's frame count, as `render_shp_frame`
    /// picks an SHP's.
    pub(crate) fn place(
        &mut self,
        key: &crate::render::sprite_atlas::ShpSpriteKey,
        source: &PictureSource,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
        draw_offsets: (i32, i32),
        select_frame: impl FnOnce(usize) -> Option<usize>,
        ramp: Option<&[Color; RAMP_SIZE]>,
    ) -> Option<PlacedPicture> {
        let frames = self.frames(source, art, asset_manager, theater_name)?;
        let sprite = frames.sprite(select_frame(frames.frame_count())?, ramp)?;
        let [width, height] = frames.canvas;
        // -ShapeWidth/2, truncated, as `render_shp_frame`.
        let left = -((width / 2) as f32) + draw_offsets.0 as f32;
        let top = -((height / 2) as f32) + draw_offsets.1 as f32;
        Some(PlacedPicture {
            key: key.clone(),
            offset: [
                left + sprite.origin[0] as f32,
                top + sprite.origin[1] as f32,
            ],
            canvas_rect: [left, top, width as f32, height as f32],
            sprite,
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
        self.decoded.get(source).and_then(Option::as_ref)
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

/// The picture `file`, read as `bytes` for art section `section` that
/// declares it as `decl`, with its mask beside it, at game pixels. A
/// picture that cannot be decoded or does not fit its canvas is logged and
/// draws nothing.
fn decode_declared_picture(
    section: &str,
    file: &str,
    bytes: &[u8],
    decl: &crate::rules::art_data::PictureDecl,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> Option<GamePictureFrames> {
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
    let shrunk = decoded
        .map_err(PictureArtError::from)
        .and_then(|(picture, mask)| {
            let canvas = decl.size.map(|size| size.map(|side| side.unsigned_abs()));
            GamePictureFrames::shrink(&picture, mask.as_ref(), canvas)
        });
    match shrunk {
        Ok(frames) => {
            log::info!(
                "[{section}] {file}: {} frames of {}x{}",
                frames.frame_count(),
                frames.canvas[0],
                frames.canvas[1]
            );
            Some(frames)
        }
        Err(error) => {
            log::warn!("[{section}] {file} cannot be drawn: {error}");
            None
        }
    }
}

/// A destroyed building's rubble frames when its art declares `Picture=`:
/// the body picture rubble binding resolved (`RubbleArt::shp`), at game
/// pixels. None for SHP rubble, which never reaches here, and for a picture
/// that cannot be drawn.
pub(crate) fn rubble_picture(
    art: &crate::rules::building_rubble::RubbleArt,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> Option<GamePictureFrames> {
    let decl = art.picture.as_ref()?;
    let bytes = asset_manager.get_ref(&art.shp)?;
    decode_declared_picture(&art.shp, &art.shp, bytes, decl, asset_manager)
}

/// One RGBA source page of picture sprites: its size, pixels, index plane
/// and the sprites on it with their top-left corners.
pub(crate) struct PicturePage {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    pub sprites: Vec<(PlacedPicture, [u32; 2])>,
    /// VERA-only, experimental: for a page of art above 1x, its twin at the
    /// art's own resolution (`render::picture_zoom`).
    pub hd: Option<HdPage>,
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
}

impl HdPage {
    /// The mip levels the zoomed draw samples: down to one texel per game
    /// pixel (`log2(scale)` halvings), each the 2x2 mean of the level above
    /// (premultiplied, so holes do not darken edges). Their RGBA one after
    /// another, level 0 first, and the count.
    pub(crate) fn mip_chain(&self) -> (Vec<u8>, u32) {
        let levels = (self.scale.ilog2() + 1).min(self.size[0].max(self.size[1]).ilog2() + 1);
        let mut chain = self.rgba.clone();
        let (mut level, mut size) = (self.rgba.clone(), self.size);
        for _ in 1..levels {
            let next = size.map(|side| (side / 2).max(1));
            let mut out = Vec::with_capacity((next[0] * next[1] * 4) as usize);
            for y in 0..next[1] {
                for x in 0..next[0] {
                    let mut sum = [0u32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(size[0] - 1);
                        let sy = (y * 2 + dy).min(size[1] - 1);
                        let at = ((sy * size[0] + sx) * 4) as usize;
                        for channel in 0..4 {
                            sum[channel] += u32::from(level[at + channel]);
                        }
                    }
                    out.extend(sum.map(|total| ((total + 2) / 4) as u8));
                }
            }
            chain.extend_from_slice(&out);
            (level, size) = (out, next);
        }
        (chain, levels)
    }
}

/// Pack `pictures` onto pages no wider or taller than `max_side`, tallest
/// first in shelves. A sprite larger than a page is left out, logged.
///
/// Art of each scale above 1 packs onto pages of its own, a page side of at
/// most `max_side / scale` so its HD twin fits too, with one empty game
/// pixel between sprites so the twin's filtering stays inside each sprite.
pub(crate) fn pack_picture_pages(pictures: Vec<PlacedPicture>, max_side: u32) -> Vec<PicturePage> {
    let mut by_scale: std::collections::BTreeMap<u32, Vec<PlacedPicture>> = Default::default();
    for picture in pictures {
        let scale = picture.sprite.hd.as_ref().map_or(1, |hd| hd.scale);
        by_scale.entry(scale).or_default().push(picture);
    }
    by_scale
        .into_iter()
        .flat_map(|(scale, group)| pack_scale(group, max_side / scale, scale))
        .collect()
}

/// [`pack_picture_pages`] for pictures of one `scale`.
fn pack_scale(mut pictures: Vec<PlacedPicture>, max_side: u32, scale: u32) -> Vec<PicturePage> {
    let gap = u32::from(scale > 1);
    pictures.sort_by_key(|picture| std::cmp::Reverse(picture.sprite.size[1]));
    let mut placed: Vec<Vec<(PlacedPicture, [u32; 2])>> = Vec::new();
    let mut extents: Vec<[u32; 2]> = Vec::new();
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for picture in pictures {
        let [width, height] = picture.sprite.size;
        if width > max_side || height > max_side {
            log::warn!(
                "[{}] frame {} is {width}x{height}, larger than a {max_side} page",
                picture.key.type_id,
                picture.key.frame
            );
            crate::rules::art_problems::report(
                &picture.key.type_id,
                "Picture=",
                &format!("a frame is larger than a {max_side}x{max_side} page"),
                "such frames draw nothing",
            );
            continue;
        }
        if placed.is_empty() || x + width > max_side {
            (x, y, shelf) = (0, y + shelf, 0);
        }
        if placed.is_empty() || y + height > max_side {
            placed.push(Vec::new());
            extents.push([0, 0]);
            (x, y, shelf) = (0, 0, 0);
        }
        let extent = extents.last_mut().unwrap();
        *extent = [extent[0].max(x + width), extent[1].max(y + height)];
        placed.last_mut().unwrap().push((picture, [x, y]));
        x += width + gap;
        shelf = shelf.max(height + gap);
    }
    placed
        .into_iter()
        .zip(extents)
        .map(|(sprites, size)| {
            let mut rgba = vec![0; (size[0] * size[1] * 4) as usize];
            let mut indices = vec![0; (size[0] * size[1]) as usize];
            for (picture, at) in &sprites {
                let sprite = &picture.sprite;
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
                let mut rgba = vec![0; (hd_size[0] * hd_size[1] * 4) as usize];
                let mut indices = vec![0; (hd_size[0] * hd_size[1]) as usize];
                for (picture, at) in &sprites {
                    if let Some(hd) = &picture.sprite.hd {
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
                    }
                }
                HdPage {
                    scale,
                    size: hd_size,
                    rgba,
                    indices,
                }
            });
            PicturePage {
                size,
                rgba,
                indices,
                sprites,
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
    [x, y]: [u32; 2],
    [width, height]: [u32; 2],
    rgba: &[u8],
    indices: &[u8],
) {
    let width = width as usize;
    for row in 0..height {
        let to = ((y + row) * page_width + x) as usize;
        let from = (row as usize) * width;
        page_rgba[to * 4..(to + width) * 4].copy_from_slice(&rgba[from * 4..(from + width) * 4]);
        page_indices[to..to + width].copy_from_slice(&indices[from..from + width]);
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
            &picture(4, 2, vec![frame]),
            Some(&picture(4, 2, vec![mask])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = shrunk.sprite(0, None).unwrap();
        assert_eq!((sprite.origin, sprite.size), ([0, 0], [1, 1]));
        assert_eq!(sprite.rgba, [100, 0, 50, 255]);
        assert_eq!(sprite.indices, [UNLIT_INDEX], "three of four are unlit");
        assert!(shrunk.sprite(1, None).is_none());
    }

    /// A house-colour pixel takes the owner's ramp step for its brightness,
    /// and a picture whose size is not a whole multiple of its canvas is
    /// refused.
    #[test]
    fn house_pixels_take_the_ramp_by_brightness_and_bad_scales_are_refused() {
        let ramp: [Color; RAMP_SIZE] = std::array::from_fn(|step| Color::rgb(step as u8, 0, 0));
        let frame = vec![255, 0, 0, 255, 0, 100, 0, 255];
        let mask = vec![255, 0, 0, 255, 255, 0, 0, 255];
        let shrunk = GamePictureFrames::shrink(
            &picture(2, 1, vec![frame.clone()]),
            Some(&picture(2, 1, vec![mask])),
            None,
        )
        .unwrap();
        assert!(shrunk.has_house_color());
        let sprite = shrunk.sprite(0, Some(&ramp)).unwrap();
        // 255 is step 0; 100 is (255 - 100) / 17 = step 9.
        assert_eq!(sprite.rgba, [0, 0, 0, 255, 9, 0, 0, 255]);
        assert_eq!(sprite.indices, [LIT_INDEX, LIT_INDEX]);
        assert!(
            GamePictureFrames::shrink(&picture(2, 1, vec![frame]), None, Some([3, 1])).is_err()
        );
    }

    /// Sprites pack in shelves, tallest first, onto as few pages as fit, and
    /// each page holds their pixels and index planes where they were placed.
    #[test]
    fn pictures_pack_in_shelves_and_keep_their_pixels() {
        let sprite = |frame: u16, size: [u32; 2], value: u8| PlacedPicture {
            key: crate::render::sprite_atlas::ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim,
                type_id: "BOOM".into(),
                facing: 0,
                frame,
                house_color: crate::rules::house_colors::HouseColorIndex(0),
            },
            sprite: PictureSprite {
                origin: [0, 0],
                size,
                rgba: vec![value; (size[0] * size[1] * 4) as usize],
                indices: vec![LIT_INDEX; (size[0] * size[1]) as usize],
                hd: None,
            },
            offset: [0.0; 2],
            canvas_rect: [0.0; 4],
        };
        let pages = pack_picture_pages(
            vec![
                sprite(0, [3, 1], 10),
                sprite(1, [2, 2], 20),
                sprite(2, [4, 4], 30),
            ],
            4,
        );
        let placements: Vec<Vec<(u16, [u32; 2])>> = pages
            .iter()
            .map(|page| {
                page.sprites
                    .iter()
                    .map(|(p, at)| (p.key.frame, *at))
                    .collect()
            })
            .collect();
        assert_eq!(
            placements,
            [vec![(2, [0, 0])], vec![(1, [0, 0]), (0, [0, 2])]]
        );
        assert_eq!(pages[1].size, [3, 3]);
        // Frame 0 sits on row 2 of the second page; nothing is beside frame 1.
        assert_eq!(pages[1].rgba[(2 * 3) * 4], 10);
        assert_eq!(pages[1].indices[2], 0);
    }

    /// A 2x picture's sprite keeps its 1x crop at picture resolution
    /// (premultiplied holes, unlit indices), and 2x sprites pack onto a page
    /// of their own whose HD twin holds each at twice its 1x place, so the
    /// 1x entry's UV addresses its HD rectangle unchanged.
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
            &picture(4, 2, vec![rows.concat().concat()]),
            Some(&picture(4, 2, vec![mask_rows.concat().concat()])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = shrunk.sprite(0, None).unwrap();
        assert_eq!((sprite.origin, sprite.size), ([0, 0], [1, 1]));
        let hd = sprite.hd.clone().unwrap();
        assert_eq!((hd.scale, hd.size), (2, [2, 2]));
        assert_eq!(hd.rgba, [RED, NONE, RED, RED].concat());
        assert_eq!(hd.indices, [LIT_INDEX, 0, LIT_INDEX, UNLIT_INDEX]);

        let placed = |frame: u16| PlacedPicture {
            key: crate::render::sprite_atlas::ShpSpriteKey {
                palette_context: crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim,
                type_id: "BOOM".into(),
                facing: 0,
                frame,
                house_color: crate::rules::house_colors::HouseColorIndex(0),
            },
            sprite: sprite.clone(),
            offset: [0.0; 2],
            canvas_rect: [0.0; 4],
        };
        let mut plain = placed(9);
        plain.sprite.hd = None;
        let pages = pack_picture_pages(vec![placed(0), plain, placed(1)], 8);
        assert_eq!(pages.len(), 2, "1x and 2x art pack apart");
        assert!(pages[0].hd.is_none());
        let page = &pages[1];
        // One empty game pixel between 2x sprites.
        let places: Vec<[u32; 2]> = page.sprites.iter().map(|(_, at)| *at).collect();
        assert_eq!((places, page.size), (vec![[0, 0], [2, 0]], [3, 1]));
        let twin = page.hd.as_ref().unwrap();
        assert_eq!((twin.scale, twin.size), (2, [6, 2]));
        // The second sprite's u is 2/3 on both pages: HD column 4.
        let row = |y: usize, x: usize| &twin.rgba[(y * 6 + x) * 4..(y * 6 + x + 1) * 4];
        assert_eq!(
            (row(0, 4), row(0, 5), row(1, 5)),
            (&RED[..], &NONE[..], &RED[..])
        );
        assert_eq!(twin.indices[6 + 5], UNLIT_INDEX);
        assert_eq!(row(0, 2), &NONE[..], "the gap stays empty");
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
        };
        let (chain, levels) = page.mip_chain();
        assert_eq!(levels, 2);
        assert_eq!(chain[..16], page.rgba[..]);
        // Premultiplied mean: colour 100 at alpha 128 is 200 once divided.
        assert_eq!(chain[16..], [100, 0, 0, 128]);
    }
}
