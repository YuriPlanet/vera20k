//! VERA-only, experimental (`docs/picture-model-art.md`, "Interface art"):
//! the picture beside an interface SHP or PCX, found by file name.
//!
//! No art section declares the interface art (the cursors, pips, health
//! bars, the placement grid, beacons, the repair and power markers, and the
//! files rules keys name: Ares `IvanBomb.Image`, insignia, Phobos
//! DigitalDisplay shapes, superweapon cameos and sidebar PCXs), so a picture
//! for one is declared by its file name: `NAME.png` beside `NAME.shp` (or
//! `.sha`, `.pcx`), found through the same lookup as that file. The picture
//! needs no SHP or PCX: its game size is, in this order, `PictureSize=` of
//! the art section named `NAME`, the SHP or PCX canvas, else the picture's
//! own size (1x). With a size it must be that size times one whole scale
//! (1 to [`PICTURE_SCALE_MAX`]); with an SHP it must have its frame count.
//! A picture that does not fit is reported in the game and the SHP's or
//! PCX's own pixels are drawn, or nothing without one. The interface only
//! draws, so nothing here reaches the simulation or its hash.

use crate::assets::picture_file::PictureFrames;
use crate::assets::shp_file::ShpFile;
use crate::render::picture_atlas::{GamePictureFrames, PICTURE_SCALE_MAX, PictureBytes};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;

/// What the file beside the picture gives it: the canvas in game pixels and
/// the frame count (an SHP header's words; a PCX is one frame its size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ArtShape {
    pub canvas: [u32; 2],
    pub frames: usize,
}

impl ArtShape {
    pub(crate) fn of_shp(shp: &ShpFile) -> Self {
        Self {
            canvas: [u32::from(shp.width), u32::from(shp.height)],
            frames: shp.frames.len(),
        }
    }
}

/// Where `PictureSize=` comes from: the match's art registry, or before a
/// match (the startup menu's cursor) the process's fixed art INI. Both read
/// the key with [`crate::rules::art_data::read_picture_size`].
#[derive(Clone, Copy)]
pub enum PictureSizes<'a> {
    Art(&'a ArtRegistry),
    Ini(&'a IniFile),
    None,
}

impl PictureSizes<'_> {
    /// `PictureSize=` of the art section named `section`.
    fn get(self, section: &str) -> Option<[u32; 2]> {
        let size = match self {
            Self::Art(art) => art.picture_size(section),
            // Sections match case-insensitively, as the registry's do.
            Self::Ini(ini) => {
                let found = ini.section(section).or_else(|| {
                    let name = ini
                        .section_names()
                        .into_iter()
                        .find(|name| name.eq_ignore_ascii_case(section))?;
                    ini.section(name)
                })?;
                crate::rules::art_data::read_picture_size(section, found)
            }
            Self::None => None,
        }?;
        Some(size.map(|side| side.unsigned_abs()))
    }
}

/// A fitting picture for an interface file.
pub(crate) struct InterfacePicture {
    pub picture: PictureFrames,
    /// Picture pixels per game pixel.
    pub scale: u32,
    /// The game canvas: `PictureSize=`, else the SHP's or PCX's, else the
    /// picture's own size.
    pub canvas: [u32; 2],
    /// The file `picture` was decoded from and `NAME.mask.png`, when the
    /// caller takes one and it exists: what [`Self::game_frames`] streams.
    bytes: PictureBytes,
}

impl InterfacePicture {
    /// The picture on its game canvas (`GamePictureFrames::stream`): each
    /// frame cropped to its drawn pixels as an SHP's stored frame and placed
    /// on the canvas, at game pixels for what the CPU reads and draws (the
    /// radar beacon, the cameos); the strip's picture page streams its own
    /// resolution from it (`GamePictureFrames::hd_page_sprites`).
    pub(crate) fn game_frames(&self) -> Option<GamePictureFrames> {
        GamePictureFrames::stream(self.bytes.clone(), Some(self.canvas)).ok()
    }
}

/// `NAME.png` for the file `file_name` (any extension).
pub(crate) fn picture_name(file_name: &str) -> String {
    format!("{}.png", stem(file_name))
}

/// `file_name` without its extension: the art section named for it.
fn stem(file_name: &str) -> &str {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
}

/// The picture beside `file_name`, whose own file gives `original` (None
/// without one), and its mask when `masked`, through `lookup`, the file's
/// own lookup; its size from `sizes`. None without the picture; None,
/// reported in the game, for one that cannot be read or does not fit.
pub(crate) fn find<'a>(
    file_name: &str,
    original: Option<ArtShape>,
    sizes: PictureSizes<'_>,
    masked: bool,
    lookup: impl Fn(&str) -> Option<&'a [u8]>,
) -> Option<InterfacePicture> {
    let name = picture_name(file_name);
    let bytes = lookup(&name)?;
    let mask_name = format!("{}.mask.png", stem(file_name));
    // An interface file is small: its bytes are copied once, so the strips
    // can stream it again at its own resolution.
    let shared = |bytes: &[u8]| {
        crate::assets::mix_archive::SharedBytes::from(std::sync::Arc::<[u8]>::from(bytes))
    };
    let read = || -> Result<InterfacePicture, String> {
        let picture = PictureFrames::decode(bytes)
            .map_err(|error| format!("it is not readable ({error})"))?;
        let (canvas, scale) = fit(&picture, original, sizes.get(stem(file_name)))?;
        let mask = match masked.then(|| lookup(&mask_name)).flatten() {
            Some(bytes) => {
                let mask = crate::assets::picture_file::PictureHeader::read(bytes)
                    .map_err(|error| format!("{mask_name} is not readable ({error})"))?;
                if [mask.width, mask.height] != [picture.width, picture.height]
                    || usize::try_from(mask.frames).ok() != Some(picture.frames.len())
                {
                    return Err(format!("{mask_name} is not the picture's size and frames"));
                }
                Some(shared(bytes))
            }
            None => None,
        };
        Ok(InterfacePicture {
            picture,
            scale,
            canvas,
            bytes: PictureBytes {
                picture: shared(bytes),
                mask,
            },
        })
    };
    match read() {
        Ok(found) => {
            log::info!(
                "{file_name} drawn from {name}: {}x{} game pixels at {}x",
                found.canvas[0],
                found.canvas[1],
                found.scale
            );
            Some(found)
        }
        Err(problem) => {
            log::warn!("{name} cannot be drawn: {problem}");
            let outcome = if original.is_some() {
                format!("{file_name}'s own pixels are drawn")
            } else {
                "nothing is drawn".to_owned()
            };
            crate::rules::art_problems::report(file_name, &name, &problem, &outcome);
            None
        }
    }
}

/// The game canvas and scale of `picture`: frame count `original`'s when
/// there is one; canvas `declared` (`PictureSize=`), else `original`'s,
/// each frame that canvas times one whole scale (1 to
/// [`PICTURE_SCALE_MAX`]) on both axes, else the picture's own size at 1x.
/// Else what is wrong with it.
fn fit(
    picture: &PictureFrames,
    original: Option<ArtShape>,
    declared: Option<[u32; 2]>,
) -> Result<([u32; 2], u32), String> {
    if let Some(original) = original
        && picture.frames.len() != original.frames
    {
        return Err(format!(
            "it has {} frames and the file beside it {}",
            picture.frames.len(),
            original.frames
        ));
    }
    let (canvas, source) = match (declared, original) {
        (Some(size), _) => (size, "PictureSize="),
        (None, Some(original)) => (original.canvas, "the file beside it"),
        (None, None) => return Ok(([picture.width, picture.height], 1)),
    };
    let [width, height] = canvas;
    let scale = picture.width / width.max(1);
    if scale == 0
        || scale > PICTURE_SCALE_MAX
        || picture.width != width * scale
        || picture.height != height * scale
    {
        return Err(format!(
            "it is {}x{}, not {source} {width}x{height} times 1 to {PICTURE_SCALE_MAX}",
            picture.width, picture.height
        ));
    }
    Ok((canvas, scale))
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    fn sized(width: u32, height: u32, frames: usize) -> PictureFrames {
        PictureFrames {
            width,
            height,
            frames: vec![vec![0; (width * height * 4) as usize]; frames],
        }
    }

    fn shape(width: u32, height: u32, frames: usize) -> Option<ArtShape> {
        Some(ArtShape {
            canvas: [width, height],
            frames,
        })
    }

    /// Beside an SHP the picture must have its frames, each its canvas
    /// times one whole scale up to 8; anything else leaves the SHP's pixels.
    #[test]
    fn a_picture_needs_the_shp_frame_count_and_a_whole_scale() {
        let shp = shape(2, 1, 2);
        assert_eq!(fit(&sized(4, 2, 2), shp, None), Ok(([2, 1], 2)));
        assert_eq!(
            fit(&sized(4, 2, 1), shp, None),
            Err("it has 1 frames and the file beside it 2".to_owned())
        );
        assert!(fit(&sized(4, 1, 2), shp, None).is_err(), "2x1 scales apart");
        assert!(fit(&sized(18, 9, 2), shp, None).is_err(), "9x");
        assert!(fit(&sized(16, 8, 2), shp, None).is_ok(), "8x");
    }

    /// `PictureSize=` is the game size before the SHP's canvas, and the
    /// picture a whole multiple of it (HD); without SHP or size the picture
    /// is 1x at its own size, with the APNG's frames; a PCX is one frame.
    #[test]
    fn the_size_is_picture_size_then_the_file_then_the_picture() {
        assert_eq!(
            fit(&sized(8, 4, 2), shape(2, 1, 2), Some([4, 2])),
            Ok(([4, 2], 2))
        );
        assert_eq!(fit(&sized(8, 4, 3), None, Some([2, 1])), Ok(([2, 1], 4)));
        assert!(fit(&sized(8, 4, 3), None, Some([3, 1])).is_err());
        assert_eq!(fit(&sized(7, 5, 9), None, None), Ok(([7, 5], 1)));
        let pcx = shape(60, 48, 1);
        assert_eq!(fit(&sized(120, 96, 1), pcx, None), Ok(([60, 48], 2)));
        assert!(
            fit(&sized(120, 96, 2), pcx, None).is_err(),
            "a PCX is one frame"
        );
    }

    /// The picture is named after the file, found through its lookup, and
    /// a misfit is reported and left out; `PictureSize=` comes from the art
    /// section named after the file.
    #[test]
    fn the_picture_beside_a_file_is_found_by_name_and_a_misfit_reported() {
        assert_eq!(picture_name("pips.shp"), "pips.png");
        assert_eq!(picture_name("mouse.sha"), "mouse.png");
        let shp = shape(2, 1, 2);
        let good =
            crate::assets::picture_file::encode_frames(4, 2, &sized(4, 2, 2).frames).unwrap();
        let lookup = |name: &str| (name == "wrench.png").then_some(good.as_slice());
        let found = find("wrench.shp", shp, PictureSizes::None, false, lookup).expect("fits");
        assert_eq!((found.scale, found.picture.frames.len()), (2, 2));
        assert!(
            find("pips.shp", shp, PictureSizes::None, false, lookup).is_none(),
            "no file"
        );
        let art = IniFile::from_str("[WRENCH]\nPictureSize=1,1\n");
        let sized_art = PictureSizes::Ini(&art);
        assert!(
            find("wrench.shp", None, sized_art, false, lookup).is_none(),
            "4x2 is not k x 1x1"
        );
        let one = crate::assets::picture_file::encode_frames(4, 2, &sized(4, 2, 1).frames).unwrap();
        assert!(
            find("wrench.shp", shp, PictureSizes::None, false, |_| Some(
                one.as_slice()
            ))
            .is_none()
        );
        let lines = crate::rules::art_problems::take();
        assert!(
            lines.iter().any(
                |line| line.starts_with("[wrench.shp] wrench.png: it has 1 frames")
                    && line.contains("own pixels are drawn")
            ),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("not PictureSize= 1x1")
                    && line.contains("nothing is drawn")),
            "{lines:?}"
        );
    }
}
