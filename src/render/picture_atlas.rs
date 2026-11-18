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
        Ok(Self { canvas, frames })
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
        if left >= right {
            return None;
        }
        let size = [right - left, bottom - top];
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
        Some(PictureSprite {
            origin: [left, top],
            size,
            rgba,
            indices,
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

/// The pictures one atlas build draws, each decoded and shrunk once, by art
/// section. A picture that is missing or does not fit its canvas is logged
/// once and draws nothing.
#[derive(Default)]
pub(crate) struct PictureSources {
    decoded: std::collections::HashMap<String, Option<GamePictureFrames>>,
}

impl PictureSources {
    /// `section`'s declared picture at game pixels, decoding it on first use.
    fn frames(
        &mut self,
        art: &crate::rules::art_data::ArtRegistry,
        section: &str,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
    ) -> Option<&GamePictureFrames> {
        let upper = section.to_ascii_uppercase();
        self.decoded
            .entry(upper)
            .or_insert_with(|| {
                let decl = art.get(section)?.picture.clone()?;
                let (file, bytes) = art
                    .picture_candidates(section, &decl, theater_name)
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
                        None
                    }
                }
            })
            .as_ref()
    }

    /// `key`'s frame of its type's picture, placed at the art's
    /// `draw_offsets`, house colour in `ramp`; None when it draws nothing.
    pub(crate) fn place(
        &mut self,
        key: &crate::render::sprite_atlas::ShpSpriteKey,
        art: &crate::rules::art_data::ArtRegistry,
        asset_manager: &crate::assets::asset_manager::AssetManager,
        theater_name: &str,
        draw_offsets: (i32, i32),
        ramp: Option<&[Color; RAMP_SIZE]>,
    ) -> Option<PlacedPicture> {
        let frames = self.frames(art, &key.type_id, asset_manager, theater_name)?;
        let sprite = frames.sprite(usize::from(key.frame), ramp)?;
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
}

/// One RGBA source page of picture sprites: its size, pixels, index plane
/// and the sprites on it with their top-left corners.
pub(crate) struct PicturePage {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub indices: Vec<u8>,
    pub sprites: Vec<(PlacedPicture, [u32; 2])>,
}

/// Pack `pictures` onto pages no wider or taller than `max_side`, tallest
/// first in shelves. A sprite larger than a page is left out, logged.
pub(crate) fn pack_picture_pages(
    mut pictures: Vec<PlacedPicture>,
    max_side: u32,
) -> Vec<PicturePage> {
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
        x += width;
        shelf = shelf.max(height);
    }
    placed
        .into_iter()
        .zip(extents)
        .map(|(sprites, size)| {
            let mut rgba = vec![0; (size[0] * size[1] * 4) as usize];
            let mut indices = vec![0; (size[0] * size[1]) as usize];
            for (picture, [x, y]) in &sprites {
                let [width, height] = picture.sprite.size;
                for row in 0..height {
                    let to = ((y + row) * size[0] + x) as usize;
                    let from = (row * width) as usize;
                    rgba[to * 4..(to + width as usize) * 4].copy_from_slice(
                        &picture.sprite.rgba[from * 4..(from + width as usize) * 4],
                    );
                    indices[to..to + width as usize]
                        .copy_from_slice(&picture.sprite.indices[from..from + width as usize]);
                }
            }
            PicturePage {
                size,
                rgba,
                indices,
                sprites,
            }
        })
        .collect()
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
}
