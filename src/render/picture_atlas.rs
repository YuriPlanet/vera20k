//! VERA-only, experimental picture art (`Picture=`,
//! `docs/picture-model-art.md`): a declared picture's frames as the SHP
//! blitters' RGBA source pages.
//!
//! Each frame is shrunk to game pixels (`PictureSize=`) on the CPU, for what
//! the game reads from it and what CPU consumers draw: its drawn rectangle
//! (cropped like a stored SHP frame), frame bounds, building render bounds,
//! stored-frame placement, and an index plane the blitters read as palette
//! indices: 0 for a hole, [`LIT_INDEX`] for an ordinary pixel and
//! [`UNLIT_INDEX`] for one light leaves alone (the mask's green). A
//! house-colour pixel (the mask's red) takes the owner's ramp step for its
//! brightness.
//!
//! The GPU holds each frame at game pixels for good, on a [`PicturePage`]
//! with a mip level past one texel per game pixel, and the blitters sample
//! the level the camera needs (`sprite_source.wgsl`). Art of a higher
//! resolution is streamed in only while the camera zooms in: the frames
//! drawn are decoded again from the file and put on HD pages at the
//! picture's own resolution (resampled to a power-of-two multiple of its
//! game pixels, [`page_scale`]; `render::picture_stream`). The file is read
//! one frame at a time ([`PictureFrameStream`]), so no picture is held whole
//! at its own resolution. In the sprite atlas a frame is drawn once for
//! every owner: a house-colour texel is black, its index is [`HOUSE_INDEX`]
//! plus the step, a house layer holds the step, and the blitters add the
//! owner's colour from the house ramps texture ([`RAMP_STRIP`],
//! [`PICTURE_HOUSE`]). Elsewhere it takes the ramp before upload. The SHP
//! blitters, their shaders and their Z, shadow and light rules are the ones
//! every SHP draws through.

use crate::assets::mix_archive::SharedBytes;
use crate::assets::pal_file::Color;
use crate::assets::picture_file::{FrameStream, PictureError, check_picture_storage};

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
/// The house ramps texture every picture page reads its owners' colours
/// from (`BatchRenderer::write_picture_ramps`): house colour `h`'s step `n` at
/// `((h % 16) * 16 + n, h / 16)`, for every house colour index.
pub(crate) const RAMP_STRIP: [u32; 2] = [256, 16];
/// The mip levels a picture page has below one texel per game pixel, for a
/// camera zoomed out (`camera::MIN_ZOOM` 0.5 is one level).
pub(crate) const ZOOM_OUT_LEVELS: u32 = 1;
/// Empty game pixels around every frame on a page: enough that no level's
/// filtering, down to the last zoom-out level, reaches a neighbour.
pub(crate) const PAGE_GAP: u32 = 1 << ZOOM_OUT_LEVELS;

/// The texels per game pixel a page holds art of `scale` picture pixels per
/// game pixel at: the next power of two, so that halving the page reaches
/// exactly one texel per game pixel. Art of 3, 5, 6 or 7 is resampled up
/// once at load (area-weighted, so each game pixel's mean is kept).
pub(crate) fn page_scale(scale: u32) -> u32 {
    scale.next_power_of_two()
}

/// The mip levels a page of `scale` texels per game pixel has: down to one
/// texel per game pixel and [`ZOOM_OUT_LEVELS`] past it.
pub(crate) fn page_levels(scale: u32) -> u32 {
    scale.ilog2() + 1 + ZOOM_OUT_LEVELS
}

/// A picture file's encoded bytes and its mask's, as shares of the assets'
/// storage (no copy): what its frames at its own resolution are decoded
/// from again, a frame at a time, whenever they are needed
/// ([`PictureFrameStream`]), on any thread.
#[derive(Clone)]
pub(crate) struct PictureBytes {
    pub picture: SharedBytes,
    pub mask: Option<SharedBytes>,
}

impl std::fmt::Debug for PictureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureBytes")
            .field("picture", &self.picture.len())
            .field("mask", &self.mask.as_ref().map(|mask| mask.len()))
            .finish()
    }
}

impl PartialEq for PictureBytes {
    fn eq(&self, other: &Self) -> bool {
        *self.picture == *other.picture && self.mask.as_deref() == other.mask.as_deref()
    }
}

impl Eq for PictureBytes {}

/// A picture's frames and its mask's, composited one at a time in lockstep
/// (`FrameStream`): memory stays at a couple of canvases whatever the frame
/// count, so no picture is too large to read.
pub(crate) struct PictureFrameStream<'a> {
    picture: FrameStream<'a>,
    mask: Option<FrameStream<'a>>,
}

impl<'a> PictureFrameStream<'a> {
    pub(crate) fn new(bytes: &'a PictureBytes) -> Result<Self, PictureError> {
        Ok(Self {
            picture: FrameStream::new(&bytes.picture)?,
            mask: bytes.mask.as_deref().map(FrameStream::new).transpose()?,
        })
    }

    /// The picture's canvas in its own pixels.
    pub(crate) fn size(&self) -> [u32; 2] {
        self.picture.size()
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.picture.frame_count()
    }

    /// The next frame's RGBA and its mask's; None past the last frame. A
    /// mask that ends first is an error.
    #[allow(clippy::type_complexity)]
    pub(crate) fn next_frame(&mut self) -> Option<Result<(&[u8], Option<&[u8]>), PictureError>> {
        let picture = match self.picture.next_frame()? {
            Ok(frame) => frame,
            Err(error) => return Some(Err(error)),
        };
        let mask = match self.mask.as_mut() {
            None => None,
            Some(mask) => match mask.next_frame() {
                Some(Ok(frame)) => Some(frame),
                Some(Err(error)) => return Some(Err(error)),
                None => return Some(Err(PictureError::NoFrames)),
            },
        };
        Some(Ok((picture, mask)))
    }
}

/// How a sprite's house-colour pixels are coloured.
#[derive(Debug, Clone, Copy)]
pub(crate) enum HouseColor<'a> {
    /// Their own colour.
    Own,
    /// The ramp's colour for their brightness, before upload.
    Baked(&'a [Color; RAMP_SIZE]),
    /// Index [`HOUSE_INDEX`] plus their ramp step, for the draw to colour: on
    /// a page black, with the step on the house layer; at game pixels their
    /// own colour.
    Deferred,
}

/// What a mask pixel marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaskClass {
    Plain,
    House,
    Unlit,
}

/// A picture's frames on the `PictureSize=` canvas at game pixels (straight
/// RGBA with binary alpha, the mask's class of each pixel), which is what the
/// game reads, CPU consumers draw and the atlas keeps on the GPU for good;
/// and the file they were shrunk from, whose frames at the picture's own
/// resolution are streamed from it again when needed
/// ([`Self::hd_page_sprites`], `render::picture_stream`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GamePictureFrames {
    pub canvas: [u32; 2],
    frames: Vec<GameFrame>,
    /// The picture's pixels per game pixel on each axis.
    scale: u32,
    bytes: PictureBytes,
}

/// What streams a picture's frames at its own resolution
/// ([`GamePictureFrames::hd_source`]): its file, canvas and scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HdSource {
    pub bytes: PictureBytes,
    pub canvas: [u32; 2],
    /// The picture's pixels per game pixel, above 1.
    pub scale: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GameFrame {
    rgba: Vec<u8>,
    classes: Vec<MaskClass>,
}

/// One frame at game pixels, for the CPU: the drawn rectangle of the
/// canvas, its RGBA and its index plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PictureSprite {
    /// The drawn rectangle's place on the canvas, as a stored SHP frame's.
    pub origin: [u32; 2],
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
}

/// One frame as a picture page holds it: the drawn rectangle of
/// [`PictureSprite`], in game pixels, and its texels, `scale` per game
/// pixel (1 on the atlas's own pages, [`GamePictureFrames::page_sprite`];
/// the picture's page scale on an HD page, [`hd_page_sprite`]). Its RGBA is premultiplied (holes are 0, so
/// filtering never darkens an edge) and its index plane holds the
/// index-plane value of each texel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageSprite {
    pub origin: [u32; 2],
    pub size: [u32; 2],
    /// Texels per game pixel, [`page_scale`] of the picture's.
    pub scale: u32,
    /// `size` times `scale`.
    pub texel_size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    /// For [`HouseColor::Deferred`], the house layer: per texel, its ramp
    /// step times [`HOUSE_LAYER_STEP`] and 255 for a house-colour texel
    /// (whose RGBA is black), 0 and 0 elsewhere; premultiplied, so the draw
    /// adds the owner's colour by coverage.
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
    /// Shrink the picture `bytes` holds, and its mask when it has one, to
    /// `canvas` game pixels, a frame at a time as the file is read (each
    /// frame at the picture's resolution is dropped once shrunk). A game
    /// pixel is drawn when at least half the picture pixels it covers are;
    /// its colour is their coverage-weighted mean, and it takes the class
    /// most of its covered pixels have. Without `canvas` the picture is 1x
    /// art.
    pub(crate) fn stream(
        bytes: PictureBytes,
        canvas: Option<[u32; 2]>,
    ) -> Result<Self, PictureArtError> {
        let mut stream = PictureFrameStream::new(&bytes)?;
        let size = stream.size();
        let canvas = canvas.unwrap_or(size);
        let scale = size[0] / canvas[0].max(1);
        if scale == 0 || scale > PICTURE_SCALE_MAX || size != [canvas[0] * scale, canvas[1] * scale]
        {
            return Err(PictureArtError::Scale {
                picture: size,
                canvas,
            });
        }
        let frame_count = stream.frame_count();
        if let Some(mask) = &stream.mask
            && (mask.size() != size || mask.frame_count() != frame_count)
        {
            return Err(PictureArtError::MaskShape {
                mask: mask.size(),
                mask_frames: mask.frame_count(),
                picture: size,
                frames: frame_count,
            });
        }
        // Only the game-sized frames are retained. Include their descriptors
        // before using the APNG's declared count as an allocation size.
        let game_pixels = u64::from(canvas[0]) * u64::from(canvas[1]);
        let per_frame =
            game_pixels * (4 + size_of::<MaskClass>() as u64) + size_of::<GameFrame>() as u64;
        check_picture_storage(per_frame.saturating_mul(frame_count as u64))?;
        let mut frames = Vec::with_capacity(frame_count);
        while let Some(frame) = stream.next_frame() {
            let (rgba, mask) = frame?;
            frames.push(shrink_frame(rgba, mask, size[0], canvas, scale));
        }
        drop(stream);
        Ok(Self {
            canvas,
            frames,
            scale,
            bytes,
        })
    }

    /// [`Self::stream`] of frames already decoded, encoded again: for tests.
    #[cfg(test)]
    pub(crate) fn shrink(
        picture: crate::assets::picture_file::PictureFrames,
        mask: Option<&crate::assets::picture_file::PictureFrames>,
        canvas: Option<[u32; 2]>,
    ) -> Result<Self, PictureArtError> {
        let encode = |frames: &crate::assets::picture_file::PictureFrames| -> SharedBytes {
            let bytes = crate::assets::picture_file::encode_frames(
                frames.width,
                frames.height,
                &frames.frames,
            )
            .expect("test frames encode");
            std::sync::Arc::<[u8]>::from(bytes).into()
        };
        Self::stream(
            PictureBytes {
                picture: encode(&picture),
                mask: mask.map(encode),
            },
            canvas,
        )
    }

    /// What streams this picture at its own resolution; None for 1x art,
    /// whose game frames are its pixels.
    pub(crate) fn hd_source(&self) -> Option<HdSource> {
        (self.scale > 1).then(|| HdSource {
            bytes: self.bytes.clone(),
            canvas: self.canvas,
            scale: self.scale,
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
        Some(PictureSprite {
            origin: [left, top],
            size,
            rgba,
            indices,
        })
    }

    /// Frame `frame` as the atlas keeps it on the GPU for good: its drawn
    /// rectangle (the crop of [`Self::sprite`]) at game pixels, one texel
    /// each, house-colour pixels as `house` says. None when
    /// [`Self::sprite`] is.
    pub(crate) fn page_sprite(&self, frame: usize, house: HouseColor<'_>) -> Option<PageSprite> {
        let (origin, size) = self.drawn_rect(frame)?;
        let source = self.frames.get(frame)?;
        Some(crop_page_sprite(
            &source.rgba,
            |at| source.classes[at],
            self.canvas[0],
            1,
            origin,
            size,
            house,
        ))
    }

    /// Every frame as [`Self::page_sprite`] crops it, at the picture's own
    /// resolution ([`hd_page_sprite`]), streamed from the file again and
    /// kept: for a small picture that wants its full resolution from the
    /// start (the interface strips, the placement preview). 1x art's are its
    /// page sprites; so are a picture's whose file cannot be read again
    /// (logged).
    pub(crate) fn hd_page_sprites(&self, house: HouseColor<'_>) -> Vec<Option<PageSprite>> {
        let page_sprites = || -> Vec<Option<PageSprite>> {
            (0..self.frame_count())
                .map(|frame| self.page_sprite(frame, house))
                .collect()
        };
        if self.scale == 1 {
            return page_sprites();
        }
        let streamed = || -> Result<Vec<Option<PageSprite>>, PictureError> {
            let mut stream = PictureFrameStream::new(&self.bytes)?;
            let mut sprites = Vec::with_capacity(self.frame_count());
            for frame in 0..self.frame_count() {
                let (rgba, mask) = stream.next_frame().ok_or(PictureError::NoFrames)??;
                sprites.push(self.drawn_rect(frame).map(|(origin, size)| {
                    hd_page_sprite(rgba, mask, self.canvas, self.scale, origin, size, house)
                }));
            }
            Ok(sprites)
        };
        match streamed() {
            Ok(sprites) => sprites,
            Err(error) => {
                log::warn!("A picture cannot be read again at its resolution: {error}");
                page_sprites()
            }
        }
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

/// A frame's drawn rectangle (`origin`, `size` in game pixels, the crop of
/// [`GamePictureFrames::sprite`]) at the picture's own resolution, from that
/// frame decoded at `scale` picture pixels per game pixel on `canvas`
/// (`rgba`, and its `mask` when it has one), as an HD page holds it:
/// resampled to [`page_scale`], house-colour pixels as `house` says
/// (`render::picture_stream`, [`GamePictureFrames::hd_page_sprites`]).
pub(crate) fn hd_page_sprite(
    rgba: &[u8],
    mask: Option<&[u8]>,
    canvas: [u32; 2],
    scale: u32,
    origin: [u32; 2],
    size: [u32; 2],
    house: HouseColor<'_>,
) -> PageSprite {
    let class = |at: usize| {
        mask.map_or(MaskClass::Plain, |mask| {
            mask_class(&mask[at * 4..at * 4 + 4])
        })
    };
    crop_page_sprite(rgba, class, canvas[0] * scale, scale, origin, size, house)
}

/// The page sprite of the rectangle `origin`, `size` (game pixels) of a frame
/// `width` pixels wide at `scale` pixels per game pixel, each pixel's mask
/// class from `class_at` (by pixel index): a pixel is drawn when at least
/// half covered, as the shrink counts it; premultiplied, so holes are 0;
/// resampled to [`page_scale`] when `scale` is not a power of two. One
/// implementation for the game-pixel page sprites (`scale` 1) and the HD
/// ones.
fn crop_page_sprite(
    rgba: &[u8],
    class_at: impl Fn(usize) -> MaskClass,
    width: u32,
    scale: u32,
    origin: [u32; 2],
    size: [u32; 2],
    house: HouseColor<'_>,
) -> PageSprite {
    let hd_size = size.map(|side| side * scale);
    let pixels = (hd_size[0] * hd_size[1]) as usize;
    let (mut out, mut indices) = (Vec::with_capacity(pixels * 4), Vec::with_capacity(pixels));
    let deferred = matches!(house, HouseColor::Deferred);
    let mut layer = Vec::with_capacity(if deferred { pixels * 2 } else { 0 });
    for y in origin[1] * scale..origin[1] * scale + hd_size[1] {
        for x in origin[0] * scale..origin[0] * scale + hd_size[0] {
            let at = (y * width + x) as usize;
            let pixel: [u8; 4] = rgba[at * 4..at * 4 + 4].try_into().unwrap();
            let mut on_layer = [0; 2];
            if u32::from(pixel[3]) * 2 < 255 {
                out.extend_from_slice(&[0; 4]);
                indices.push(0);
            } else {
                let (color, index) = match (class_at(at), house) {
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
                out.extend_from_slice(&[color[0], color[1], color[2], 255]);
                indices.push(index);
            }
            if deferred {
                layer.extend_from_slice(&on_layer);
            }
        }
    }
    let page = page_scale(scale);
    let texel_size = size.map(|side| side * page);
    let mut layer = deferred.then_some(layer);
    if page != scale {
        out = resample(&out, hd_size, 4, scale, page);
        indices = resample_nearest(&indices, hd_size, scale, page);
        layer = layer.map(|layer| resample(&layer, hd_size, 2, scale, page));
    }
    PageSprite {
        origin,
        size,
        scale: page,
        texel_size,
        rgba: out,
        indices,
        house: layer,
    }
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

/// A frame drawn for the sprite atlas: its picture's canvas and its page
/// sprite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PictureFrame {
    pub canvas: [u32; 2],
    pub sprite: PageSprite,
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

/// What one sprite atlas build adds from pictures: the frames it drew (at
/// game pixels), the keys drawn from them or from frames the atlas already
/// holds, the house ramps their pages read ([`ramp_strip`]), and what
/// streams the new frames' files at their own resolution while the camera
/// zooms in (`render::picture_stream`).
#[derive(Default)]
pub(crate) struct PreparedPictures {
    pub frames: Vec<(PictureFrameId, PictureFrame)>,
    /// New frames that draw nothing.
    pub empty: Vec<PictureFrameId>,
    pub keys: Vec<PictureKey>,
    pub ramps: Vec<u8>,
    /// The files of `frames` above 1x art, by [`PictureFrameId`]'s file and
    /// size.
    pub hd_sources: Vec<(PictureFile, HdSource)>,
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

/// A picture file as [`stream_picture`] shrinks it: the file found and the
/// declared `PictureSize=`.
pub(crate) type PictureFile = (String, Option<[i32; 2]>);
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
        let todo: Vec<(&PictureSource, Option<(String, SharedBytes)>)> = sources
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
        let files: Vec<(
            PictureFile,
            SharedBytes,
            &crate::rules::art_data::PictureDecl,
        )> = todo
            .iter()
            .filter_map(|(source, found)| {
                let (file, bytes) = found.as_ref()?;
                let key = (file.clone(), source.decl.size);
                (!self.files.contains_key(&key) && new_files.insert(key.clone()))
                    .then(|| (key, bytes.clone(), &source.decl))
            })
            .collect();
        let decoded = crate::sim::parallel::map(&files, |((file, _), bytes, decl)| {
            decode_file(file, bytes.clone(), decl, asset_manager)
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

    /// Frame `frame` of `source`'s picture as the atlas keeps it, at game
    /// pixels ([`GamePictureFrames::page_sprite`]), house colour as `house`
    /// says; None when it draws nothing, or when [`Self::frames`] or
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
            sprite: frames.page_sprite(frame, house)?,
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

    /// What streams `source`'s picture at its own resolution
    /// ([`GamePictureFrames::hd_source`]), once decoded.
    pub(crate) fn hd_source(&self, source: &PictureSource) -> Option<HdSource> {
        self.decoded_frames(source)?.hd_source()
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
/// candidate file is there (`ArtRegistry::find_picture`).
fn locate_source(
    source: &PictureSource,
    art: &crate::rules::art_data::ArtRegistry,
    asset_manager: &crate::assets::asset_manager::AssetManager,
    theater_name: &str,
) -> Option<(String, SharedBytes)> {
    art.find_picture(&source.section, &source.decl, theater_name, |file| {
        asset_manager.get_shared(file)
    })
}

/// [`stream_picture`], shared by every source that draws `file`.
fn decode_file(
    file: &str,
    bytes: SharedBytes,
    decl: &crate::rules::art_data::PictureDecl,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> DecodedFile {
    stream_picture(file, bytes, decl, asset_manager)
        .map(std::sync::Arc::new)
        .map_err(std::sync::Arc::new)
}

/// The picture `file` (`bytes`), with its mask beside it, shrunk to the
/// game pixels `decl` gives as it is read ([`GamePictureFrames::stream`]).
fn stream_picture(
    file: &str,
    bytes: SharedBytes,
    decl: &crate::rules::art_data::PictureDecl,
    asset_manager: &crate::assets::asset_manager::AssetManager,
) -> Result<GamePictureFrames, PictureArtError> {
    let mask_name = format!(
        "{}.mask.png",
        file.rsplit_once('.').map_or(file, |(stem, _)| stem)
    );
    let canvas = decl.size.map(|size| size.map(|side| side.unsigned_abs()));
    let mask = crate::assets::picture_file::find_picture(&mask_name, |name| {
        asset_manager.get_shared(name)
    })
    .map(|(_, mask)| mask);
    GamePictureFrames::stream(
        PictureBytes {
            picture: bytes,
            mask,
        },
        canvas,
    )
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
    let files: Vec<(
        PictureFile,
        SharedBytes,
        &crate::rules::art_data::PictureDecl,
    )> = arts
        .iter()
        .filter_map(|art| {
            let decl = art.picture.as_ref()?;
            let key = (art.shp.clone(), decl.size);
            if !seen.insert(key.clone()) {
                return None;
            }
            Some((key, asset_manager.get_shared(&art.shp)?, decl))
        })
        .collect();
    let decoded = crate::sim::parallel::map(&files, |((file, _), bytes, decl)| {
        decode_file(file, bytes.clone(), decl, asset_manager)
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

/// `plane`, `size` texels of `channels` bytes at `from` texels per game
/// pixel, at `to` texels per game pixel: each new texel is the area-weighted
/// mean of the old texels it overlaps, so every game pixel keeps its mean
/// (and its coverage, the plane being premultiplied). In units of
/// `1 / (from * to)` game pixel, old texel `j` spans `[j * to, (j + 1) * to)`
/// and new texel `i` spans `[i * from, (i + 1) * from)`.
fn resample(plane: &[u8], size: [u32; 2], channels: usize, from: u32, to: u32) -> Vec<u8> {
    let out_size = size.map(|side| side / from * to);
    let taps = |count: u32| -> Vec<Vec<(usize, u32)>> {
        (0..count)
            .map(|i| {
                let (start, end) = (i * from, (i + 1) * from);
                (start / to..end.div_ceil(to))
                    .map(|j| (j as usize, end.min((j + 1) * to) - start.max(j * to)))
                    .collect()
            })
            .collect()
    };
    let (columns, rows) = (taps(out_size[0]), taps(out_size[1]));
    let (width, out_width) = (size[0] as usize, out_size[0] as usize);
    // Across first, each sum weighted by `from` in all; then down.
    let mut across = vec![0u32; out_width * size[1] as usize * channels];
    for y in 0..size[1] as usize {
        for (x, taps) in columns.iter().enumerate() {
            let out_at = (y * out_width + x) * channels;
            for &(j, weight) in taps {
                let at = (y * width + j) * channels;
                for channel in 0..channels {
                    across[out_at + channel] += u32::from(plane[at + channel]) * weight;
                }
            }
        }
    }
    let area = from * from;
    let mut out = Vec::with_capacity(out_width * out_size[1] as usize * channels);
    for taps in &rows {
        for x in 0..out_width {
            for channel in 0..channels {
                let sum: u32 = taps
                    .iter()
                    .map(|&(j, weight)| across[(j * out_width + x) * channels + channel] * weight)
                    .sum();
                out.push(((sum + area / 2) / area) as u8);
            }
        }
    }
    out
}

/// [`resample`] for an index plane: each new texel takes the old texel under
/// its centre.
fn resample_nearest(plane: &[u8], size: [u32; 2], from: u32, to: u32) -> Vec<u8> {
    let out_size = size.map(|side| side / from * to);
    let source = |i: u32| ((2 * i + 1) * from / (2 * to)) as usize;
    let mut out = Vec::with_capacity((out_size[0] * out_size[1]) as usize);
    for y in 0..out_size[1] {
        let row = source(y) * size[0] as usize;
        out.extend((0..out_size[0]).map(|x| plane[row + source(x)]));
    }
    out
}

/// One picture page as the GPU holds it: frames at `scale` texels per game
/// pixel, premultiplied RGBA, the index plane at the same texels, and the
/// house layer ([`PageSprite::house`]) when a frame on it has one. Its sides
/// are whole multiples of its last mip level's texel.
pub(crate) struct PicturePage {
    pub scale: u32,
    /// In texels.
    pub size: [u32; 2],
    /// Premultiplied RGBA, level 0.
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    /// Two bytes a texel.
    pub house: Option<Vec<u8>>,
}

impl PicturePage {
    /// A page `extent` game pixels large (at most `max_side` texels a side)
    /// with each of `sprites` (all of one `scale`) at its game-pixel corner.
    pub(crate) fn compose(
        scale: u32,
        extent: [u32; 2],
        max_side: u32,
        sprites: &[(&PageSprite, [u32; 2])],
    ) -> Self {
        let align = scale << ZOOM_OUT_LEVELS;
        let size = extent.map(|side| (side * scale).next_multiple_of(align).clamp(1, max_side));
        let texels = (size[0] * size[1]) as usize;
        let mut rgba = vec![0; texels * 4];
        let mut indices = vec![0; texels];
        let mut house = sprites
            .iter()
            .any(|(sprite, _)| sprite.house.is_some())
            .then(|| vec![0; texels * 2]);
        for (sprite, at) in sprites {
            let at = at.map(|side| side * scale);
            blit_plane(&mut rgba, size[0], at, sprite.texel_size, &sprite.rgba, 4);
            blit_plane(
                &mut indices,
                size[0],
                at,
                sprite.texel_size,
                &sprite.indices,
                1,
            );
            if let (Some(page), Some(layer)) = (house.as_mut(), &sprite.house) {
                blit_plane(page, size[0], at, sprite.texel_size, layer, 2);
            }
        }
        Self {
            scale,
            size,
            rgba,
            indices,
            house,
        }
    }

    /// The page's size in game pixels: a game point `p` on it is at UV
    /// `p / uv_extent`.
    pub(crate) fn uv_extent(&self) -> [f32; 2] {
        self.size.map(|side| side as f32 / self.scale as f32)
    }

    /// The mip levels the blitters sample: down to one texel per game pixel
    /// (`log2(scale)` halvings) and [`ZOOM_OUT_LEVELS`] past it, each the
    /// 2x2 mean of the level above (premultiplied, so holes do not darken
    /// edges). The RGBA levels one after another, level 0 first, then the
    /// count.
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
        page_levels(self.scale).min(self.size[0].max(self.size[1]).ilog2() + 1)
    }
}

/// A packed picture page and the frames on it, each at its game-pixel
/// corner.
pub(crate) struct PackedPicturePage {
    pub page: PicturePage,
    pub frames: Vec<(PagedFrame, [u32; 2])>,
}

/// A frame on a page: what an atlas entry drawn from it needs.
pub(crate) struct PagedFrame {
    pub id: PictureFrameId,
    pub canvas: [u32; 2],
    pub origin: [u32; 2],
    pub size: [u32; 2],
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

/// Pack `frames` onto pages no wider or taller than `max_side` texels,
/// tallest first in shelves, [`PAGE_GAP`] empty game pixels around each. A
/// sprite larger than a page is left out, logged.
///
/// Art of each page scale packs onto pages of its own, a page side of at
/// most `max_side / scale` game pixels.
pub(crate) fn pack_picture_pages(
    frames: Vec<(PictureFrameId, PictureFrame)>,
    max_side: u32,
) -> Vec<PackedPicturePage> {
    let mut by_scale: std::collections::BTreeMap<u32, Vec<(PictureFrameId, PictureFrame)>> =
        Default::default();
    for frame in frames {
        by_scale
            .entry(frame.1.sprite.scale)
            .or_default()
            .push(frame);
    }
    by_scale
        .into_iter()
        .flat_map(|(scale, group)| pack_scale(group, max_side, scale))
        .collect()
}

/// [`pack_picture_pages`] for frames of one page `scale`.
fn pack_scale(
    mut frames: Vec<(PictureFrameId, PictureFrame)>,
    max_side: u32,
    scale: u32,
) -> Vec<PackedPicturePage> {
    let page_side = max_side / scale;
    frames.sort_by_key(|(_, frame)| std::cmp::Reverse(frame.sprite.size[1]));
    let mut placed: Vec<Vec<((PictureFrameId, PictureFrame), [u32; 2])>> = Vec::new();
    let mut extents: Vec<[u32; 2]> = Vec::new();
    let (mut x, mut y, mut shelf) = (PAGE_GAP, PAGE_GAP, 0u32);
    for (id, frame) in frames {
        let [width, height] = frame.sprite.size;
        if width + 2 * PAGE_GAP > page_side || height + 2 * PAGE_GAP > page_side {
            log::warn!(
                "[{}] frame {} is {width}x{height}, larger than a {page_side} page",
                id.file,
                id.frame
            );
            crate::rules::art_problems::report(
                &id.file,
                "Picture=",
                &format!("a frame is larger than a {page_side}x{page_side} page"),
                "such frames draw nothing",
            );
            continue;
        }
        if !placed.is_empty() && x + width + PAGE_GAP > page_side {
            (x, y, shelf) = (PAGE_GAP, y + shelf, 0);
        }
        if placed.is_empty() || y + height + PAGE_GAP > page_side {
            placed.push(Vec::new());
            extents.push([0, 0]);
            (x, y, shelf) = (PAGE_GAP, PAGE_GAP, 0);
        }
        let extent = extents.last_mut().unwrap();
        *extent = [
            extent[0].max(x + width + PAGE_GAP),
            extent[1].max(y + height + PAGE_GAP),
        ];
        placed.last_mut().unwrap().push(((id, frame), [x, y]));
        x += width + PAGE_GAP;
        shelf = shelf.max(height + PAGE_GAP);
    }
    placed
        .into_iter()
        .zip(extents)
        .map(|(frames, extent)| {
            let sprites: Vec<(&PageSprite, [u32; 2])> = frames
                .iter()
                .map(|((_, frame), at)| (&frame.sprite, *at))
                .collect();
            let page = PicturePage::compose(scale, extent, max_side, &sprites);
            PackedPicturePage {
                page,
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
            }
        })
        .collect()
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
    use crate::assets::picture_file::PictureFrames;

    fn picture(width: u32, height: u32, frames: Vec<Vec<u8>>) -> PictureFrames {
        PictureFrames {
            width,
            height,
            frames,
        }
    }

    /// Frame 0 at the picture's own resolution, as HD pages and the
    /// interface strips take it.
    fn hd(frames: &GamePictureFrames, house: HouseColor<'_>) -> Option<PageSprite> {
        frames.hd_page_sprites(house).into_iter().next().flatten()
    }

    fn frame_id(frame: usize) -> PictureFrameId {
        PictureFrameId {
            file: "BOOM.png".into(),
            size: None,
            frame,
            deferred: false,
        }
    }

    /// A 1x frame `size` large, every texel `value`, at canvas origin 0.
    fn plain_frame(frame: usize, size: [u32; 2], value: u8) -> (PictureFrameId, PictureFrame) {
        (
            frame_id(frame),
            PictureFrame {
                canvas: size,
                sprite: PageSprite {
                    origin: [0, 0],
                    size,
                    scale: 1,
                    texel_size: size,
                    rgba: vec![value; (size[0] * size[1] * 4) as usize],
                    indices: vec![LIT_INDEX; (size[0] * size[1]) as usize],
                    house: None,
                },
            },
        )
    }

    /// Each page's frames, by frame number, with their game-pixel corners.
    fn placements(pages: &[PackedPicturePage]) -> Vec<Vec<(usize, [u32; 2])>> {
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

    /// A deferred house-colour pixel keeps its own colour at game pixels and
    /// carries its ramp step in its index, [`HOUSE_INDEX`] plus the step. On
    /// a page it is black with the same index, and the house layer holds its
    /// step; other texels, holes included, are 0 on that layer. 1x art's page
    /// sprite is its game frame. Its own colours draw no house layer.
    #[test]
    fn deferred_house_pixels_carry_their_ramp_step_in_the_index_and_house_layer() {
        const H: [u8; 4] = [100, 0, 0, 255];
        const P: [u8; 4] = [50, 50, 50, 255];
        const NONE: [u8; 4] = [0; 4];
        const RED: [u8; 4] = [255, 0, 0, 255];
        const BLACK: [u8; 4] = [0, 0, 0, 255];
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
        let page = hd(&shrunk, HouseColor::Deferred).unwrap();
        assert_eq!((page.scale, page.texel_size), (1, [3, 1]));
        assert_eq!(page.rgba, [BLACK, BLACK, P].concat());
        assert_eq!(page.indices, sprite.indices);
        let on = |step: u8| [step * HOUSE_LAYER_STEP, 255];
        assert_eq!(page.house.unwrap(), [on(0), on(9), [0; 2]].concat());

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
        let page = hd(&shrunk, HouseColor::Deferred).unwrap();
        assert_eq!((page.origin, page.size), (sprite.origin, sprite.size));
        assert_eq!((page.scale, page.texel_size), (2, [4, 2]));
        assert_eq!(
            page.rgba,
            [[BLACK, BLACK, P, P], [NONE, BLACK, P, P]]
                .concat()
                .concat()
        );
        let house = HOUSE_INDEX + 9;
        assert_eq!(
            page.indices,
            [
                house, house, LIT_INDEX, LIT_INDEX, 0, house, LIT_INDEX, LIT_INDEX
            ]
        );
        assert_eq!(
            page.house.unwrap(),
            [
                [on(9), on(9), [0; 2], [0; 2]],
                [[0; 2], on(9), [0; 2], [0; 2]]
            ]
            .concat()
            .concat()
        );
        let own = hd(&shrunk, HouseColor::Own).unwrap();
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

    /// Frames pack in shelves, tallest first, onto as few pages as fit, two
    /// empty game pixels around each; each page holds their texels where
    /// they were placed, its sides a whole number of last-level texels. A
    /// frame no page fits is left out.
    #[test]
    fn pictures_pack_in_shelves_with_a_gap_around_each_and_keep_their_texels() {
        let pages = pack_picture_pages(
            vec![
                plain_frame(0, [3, 1], 10),
                plain_frame(1, [2, 2], 20),
                plain_frame(2, [20, 4], 30),
                plain_frame(3, [10, 30], 40),
                // 37 and a gap each side is past a 40 page.
                plain_frame(4, [1, 37], 50),
            ],
            40,
        );
        assert_eq!(
            placements(&pages),
            [vec![(3, [2, 2]), (2, [14, 2]), (1, [36, 2]), (0, [2, 34])]]
        );
        let page = &pages[0].page;
        assert_eq!((page.scale, page.size), (1, [40, 38]));
        assert_eq!(pages[0].page.uv_extent(), [40.0, 38.0]);
        let width = page.size[0] as usize;
        assert_eq!(page.rgba[(2 * width + 14) * 4], 30);
        assert_eq!(page.rgba[(3 * width + 37) * 4], 20);
        assert_eq!(page.rgba[(34 * width + 4) * 4], 10);
        assert_eq!(page.indices[2 * width + 13], 0, "the gap stays empty");
        assert_eq!(page.indices[width + 2], 0, "the gap above the shelf");
        assert!(page.house.is_none());

        // Two frames that do not share a 20 page.
        let pages = pack_picture_pages(
            vec![plain_frame(0, [10, 10], 1), plain_frame(1, [10, 10], 2)],
            20,
        );
        assert_eq!(placements(&pages), [vec![(0, [2, 2])], vec![(1, [2, 2])]]);
        assert_eq!(pages[1].page.size, [14, 14]);
    }

    /// A 2x picture's page sprite keeps its game-pixel crop at the picture's
    /// resolution (premultiplied holes, unlit indices), and packs onto a page
    /// of its own whose texels sit at twice the frame's game-pixel corner,
    /// its UV extent in game pixels.
    #[test]
    fn a_2x_frame_packs_apart_at_twice_its_game_corner() {
        const RED: [u8; 4] = [200, 0, 0, 255];
        const NONE: [u8; 4] = [0; 4];
        // Canvas 2x1 of a 4x2 picture: the left game pixel has three of four
        // pixels drawn (drawn), the right one has one (not drawn).
        let rows = [[RED, NONE, RED, NONE], [RED, RED, NONE, NONE]];
        let unlit = [0, 255, 0, 255];
        let mask_rows = [[[0; 4]; 4], [[0; 4], unlit, [0; 4], [0; 4]]];
        let shrunk = GamePictureFrames::shrink(
            picture(4, 2, vec![rows.concat().concat()]),
            Some(&picture(4, 2, vec![mask_rows.concat().concat()])),
            Some([2, 1]),
        )
        .unwrap();
        let sprite = hd(&shrunk, HouseColor::Own).unwrap();
        assert_eq!((sprite.origin, sprite.size), ([0, 0], [1, 1]));
        assert_eq!((sprite.scale, sprite.texel_size), (2, [2, 2]));
        assert_eq!(sprite.rgba, [RED, NONE, RED, RED].concat());
        assert_eq!(sprite.indices, [LIT_INDEX, 0, LIT_INDEX, UNLIT_INDEX]);
        assert!(sprite.house.is_none());

        let placed = (
            frame_id(0),
            PictureFrame {
                canvas: shrunk.canvas,
                sprite,
            },
        );
        let pages = pack_picture_pages(vec![placed, plain_frame(9, [1, 1], 7)], 64);
        assert_eq!(pages.len(), 2, "1x and 2x art pack apart");
        assert_eq!(pages[0].page.scale, 1);
        let page = &pages[1].page;
        assert_eq!(placements(&pages[1..]), [vec![(0, [2, 2])]]);
        // 5 game pixels, 10 texels, rounded up to the last level's 4.
        assert_eq!((page.scale, page.size), (2, [12, 12]));
        assert_eq!(page.uv_extent(), [6.0, 6.0]);
        let texel = |x: usize, y: usize| &page.rgba[(y * 12 + x) * 4..(y * 12 + x + 1) * 4];
        assert_eq!(
            (texel(4, 4), texel(5, 4), texel(5, 5), texel(3, 4)),
            (&RED[..], &NONE[..], &RED[..], &NONE[..])
        );
        assert_eq!(page.indices[5 * 12 + 5], UNLIT_INDEX);
    }

    /// Art of 3x is resampled to 4x by area, so a game pixel keeps its mean
    /// colour and coverage; its index plane takes the texel under each new
    /// texel's centre.
    #[test]
    fn a_3x_picture_is_resampled_to_4x_by_area() {
        const RED: [u8; 4] = [255, 0, 0, 255];
        const NONE: [u8; 4] = [0; 4];
        // One game pixel of 3x3, its two left columns drawn.
        let row = [RED, RED, NONE];
        let shrunk = GamePictureFrames::shrink(
            picture(3, 3, vec![[row; 3].concat().concat()]),
            None,
            Some([1, 1]),
        )
        .unwrap();
        let sprite = hd(&shrunk, HouseColor::Own).unwrap();
        assert_eq!((sprite.scale, sprite.texel_size), (4, [4, 4]));
        let row = [RED, RED, [170, 0, 0, 170], NONE].concat();
        assert_eq!(
            sprite.rgba,
            [row.clone(), row.clone(), row.clone(), row].concat()
        );
        let row = [LIT_INDEX, LIT_INDEX, LIT_INDEX, 0];
        assert_eq!(sprite.indices, [row; 4].concat());
    }

    /// A page holds the house layer of the deferred frames on it at their
    /// texels, zero elsewhere.
    #[test]
    fn pages_hold_the_house_layer_of_deferred_frames() {
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
            sprite: hd(&shrunk, HouseColor::Deferred).unwrap(),
        };
        let pages = pack_picture_pages(vec![(id, frame)], 64);
        assert_eq!(pages[0].frames[0].1, [2, 2]);
        let page = &pages[0].page;
        let house = page.house.as_ref().unwrap();
        let width = page.size[0] as usize;
        assert_eq!(house.len(), width * page.size[1] as usize * 2);
        let at = |y: usize, x: usize| &house[(y * width + x) * 2..(y * width + x + 1) * 2];
        assert_eq!((at(4, 4), at(5, 5)), (&[0, 255][..], &[0, 255][..]));
        assert_eq!((at(4, 3), at(0, 0)), (&[0, 0][..], &[0, 0][..]));
        assert_eq!(page.indices[4 * width + 4], HOUSE_INDEX);
    }

    /// A page's mips go one level past one texel per game pixel and average
    /// premultiplied texels, so a hole does not darken its drawn neighbour;
    /// a page too small stops at one texel.
    #[test]
    fn page_mips_average_premultiplied_texels_past_game_pixels() {
        let mut page = PicturePage {
            scale: 2,
            size: [2, 2],
            rgba: [[200, 0, 0, 255], [0; 4], [0; 4], [200, 0, 0, 255]].concat(),
            indices: vec![LIT_INDEX, 0, 0, LIT_INDEX],
            house: None,
        };
        let (chain, levels) = page.mip_chain();
        assert_eq!(levels, 2, "a 2x2 page has two levels");
        assert_eq!(chain[..16], page.rgba[..]);
        // Premultiplied mean: colour 100 at alpha 128 is 200 once divided.
        assert_eq!(chain[16..], [100, 0, 0, 128]);
        assert!(page.house_mip_chain().is_none());
        page.scale = 1;
        assert_eq!(page.mip_chain().1, 2, "1x art keeps a zoom-out level");
        page.size = [4, 4];
        page.rgba = vec![0; 64];
        page.scale = 2;
        assert_eq!(page.mip_chain().1, 3, "2x, 1x and the zoom-out level");
    }

    /// The house layer's mips are the 2x2 means of its two channels, as the
    /// colour mips are, down to the same level: partly covered game pixels
    /// keep their step once divided by the coverage.
    #[test]
    fn page_house_mips_average_the_two_channel_layer_like_the_colour() {
        let page = PicturePage {
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

    /// A 4x picture is kept at game pixels, one texel each, which is what
    /// the atlas's pages hold; its frames at 4x come from streaming the file
    /// again, each the same crop, frame after frame in order.
    #[test]
    fn a_4x_picture_keeps_game_pixels_and_streams_its_own_resolution() {
        const RED: [u8; 4] = [200, 0, 0, 255];
        const BLUE: [u8; 4] = [0, 0, 100, 255];
        const NONE: [u8; 4] = [0; 4];
        // 8x4 picture pixels on a 2x1 canvas: frame 0 the left game pixel,
        // frame 1 the right one.
        let frame =
            |left: [u8; 4], right: [u8; 4]| [[left; 4], [right; 4]].concat().repeat(4).concat();
        let shrunk = GamePictureFrames::shrink(
            picture(8, 4, vec![frame(RED, NONE), frame(NONE, BLUE)]),
            None,
            Some([2, 1]),
        )
        .unwrap();
        let resident = shrunk.page_sprite(0, HouseColor::Own).unwrap();
        assert_eq!(
            (resident.scale, resident.size, resident.texel_size),
            (1, [1, 1], [1, 1])
        );
        assert_eq!(resident.rgba, RED);
        assert!(shrunk.hd_source().is_some_and(|source| source.scale == 4));
        let hd = shrunk.hd_page_sprites(HouseColor::Own);
        let (first, second) = (hd[0].as_ref().unwrap(), hd[1].as_ref().unwrap());
        assert_eq!((first.origin, first.size), (resident.origin, resident.size));
        assert_eq!((first.scale, first.texel_size), (4, [4, 4]));
        assert_eq!(first.rgba, RED.repeat(16));
        assert_eq!((second.origin, second.texel_size), ([1, 0], [4, 4]));
        assert_eq!(second.rgba, BLUE.repeat(16));
    }
}
