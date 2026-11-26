//! VERA-only, experimental (`docs/picture-model-art.md`, "Interface art"):
//! the picture beside a hard-coded interface SHP, found by file name.
//!
//! No art or rules section names the interface SHPs (the cursors, pips,
//! health bars, the placement grid, beacons, the repair and power markers),
//! so a picture for one is declared by its file name: `NAME.png` beside
//! `NAME.shp` (or `NAME.sha`), found through the same lookup as the SHP. It
//! must have the SHP's frame count, each frame the SHP canvas times one
//! whole scale (1 to [`PICTURE_SCALE_MAX`]). A picture that does not fit is
//! reported in the game and the SHP is drawn. The interface only draws, so
//! nothing here reaches the simulation.

use crate::assets::picture_file::PictureFrames;
use crate::assets::shp_file::ShpFile;
use crate::render::picture_atlas::{GamePictureFrames, PICTURE_SCALE_MAX};

/// A fitting picture for an interface SHP.
pub(crate) struct InterfacePicture {
    pub picture: PictureFrames,
    /// `NAME.mask.png`, when the caller takes one and it exists.
    pub mask: Option<PictureFrames>,
    /// Picture pixels per SHP pixel.
    pub scale: u32,
}

impl InterfacePicture {
    /// The picture at the SHP's own pixels, as the picture art draws at 1x
    /// (`GamePictureFrames::shrink`), keeping its frames at their own
    /// resolution for the zoomed draw.
    pub(crate) fn game_frames(&self) -> Option<GamePictureFrames> {
        let canvas = [
            self.picture.width / self.scale,
            self.picture.height / self.scale,
        ];
        GamePictureFrames::shrink(self.picture.clone(), self.mask.as_ref(), Some(canvas)).ok()
    }
}

/// `NAME.png` for the SHP file `shp_name` (any extension).
pub(crate) fn picture_name(shp_name: &str) -> String {
    let stem = shp_name.rsplit_once('.').map_or(shp_name, |(stem, _)| stem);
    format!("{stem}.png")
}

/// The picture beside `shp_name` (parsed as `shp`), and its mask when
/// `masked`, through `lookup`, the SHP's own file lookup. None without the
/// file; None, reported in the game, for one that cannot be read or does
/// not fit.
pub(crate) fn find<'a>(
    shp_name: &str,
    shp: &ShpFile,
    masked: bool,
    lookup: impl Fn(&str) -> Option<&'a [u8]>,
) -> Option<InterfacePicture> {
    let name = picture_name(shp_name);
    let bytes = lookup(&name)?;
    let mask_name = name.replace(".png", ".mask.png");
    let read = || -> Result<InterfacePicture, String> {
        let picture = PictureFrames::decode(bytes)
            .map_err(|error| format!("it is not readable ({error})"))?;
        let scale = fit(&picture, shp)?;
        let mask = match masked.then(|| lookup(&mask_name)).flatten() {
            Some(bytes) => {
                let mask = PictureFrames::decode(bytes)
                    .map_err(|error| format!("{mask_name} is not readable ({error})"))?;
                if [mask.width, mask.height] != [picture.width, picture.height]
                    || mask.frames.len() != picture.frames.len()
                {
                    return Err(format!("{mask_name} is not the picture's size and frames"));
                }
                Some(mask)
            }
            None => None,
        };
        Ok(InterfacePicture {
            picture,
            mask,
            scale,
        })
    };
    match read() {
        Ok(found) => {
            log::info!("{shp_name} drawn from {name} at {}x", found.scale);
            Some(found)
        }
        Err(problem) => {
            log::warn!("{name} cannot be drawn: {problem}");
            crate::rules::art_problems::report(
                shp_name,
                &name,
                &problem,
                &format!("{shp_name}'s own pixels are drawn"),
            );
            None
        }
    }
}

/// The scale of `picture` over `shp` when it has the SHP's frame count and
/// each frame is the canvas times one whole scale (1 to
/// [`PICTURE_SCALE_MAX`]) on both axes; else what is wrong with it.
fn fit(picture: &PictureFrames, shp: &ShpFile) -> Result<u32, String> {
    if picture.frames.len() != shp.frames.len() {
        return Err(format!(
            "it has {} frames and the SHP {}",
            picture.frames.len(),
            shp.frames.len()
        ));
    }
    let (width, height) = (u32::from(shp.width), u32::from(shp.height));
    let scale = picture.width / width.max(1);
    if scale == 0
        || scale > PICTURE_SCALE_MAX
        || picture.width != width * scale
        || picture.height != height * scale
    {
        return Err(format!(
            "it is {}x{}, not the SHP's {width}x{height} times 1 to {PICTURE_SCALE_MAX}",
            picture.width, picture.height
        ));
    }
    Ok(scale)
}

/// A `width` x `height` SHP of `frames` empty frames, for tests.
#[cfg(test)]
pub(crate) fn test_shp(width: u16, height: u16, frames: usize) -> ShpFile {
    let frame = || crate::assets::shp_file::ShpFrame {
        frame_x: 0,
        frame_y: 0,
        frame_width: 0,
        frame_height: 0,
        format: 0,
        radar_color: [0; 3],
        pixels: Vec::new(),
    };
    ShpFile {
        width,
        height,
        frames: (0..frames).map(|_| frame()).collect(),
    }
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

    /// The picture must have the SHP's frames, each its canvas times one
    /// whole scale up to 8; anything else leaves the SHP's pixels.
    #[test]
    fn a_picture_needs_the_shp_frame_count_and_a_whole_scale() {
        let shp = test_shp(2, 1, 2);
        assert_eq!(fit(&sized(4, 2, 2), &shp), Ok(2));
        assert_eq!(
            fit(&sized(4, 2, 1), &shp),
            Err("it has 1 frames and the SHP 2".to_owned())
        );
        assert!(fit(&sized(4, 1, 2), &shp).is_err(), "2x1 scales apart");
        assert!(fit(&sized(18, 9, 2), &shp).is_err(), "9x");
        assert!(fit(&sized(16, 8, 2), &shp).is_ok(), "8x");
    }

    /// The picture is named after the SHP, found through the SHP's lookup,
    /// and a misfit is reported and left out.
    #[test]
    fn the_picture_beside_an_shp_is_found_by_name_and_a_misfit_reported() {
        assert_eq!(picture_name("pips.shp"), "pips.png");
        assert_eq!(picture_name("mouse.sha"), "mouse.png");
        let shp = test_shp(2, 1, 2);
        let good =
            crate::assets::picture_file::encode_frames(4, 2, &sized(4, 2, 2).frames).unwrap();
        let lookup = |name: &str| (name == "wrench.png").then_some(good.as_slice());
        let found = find("wrench.shp", &shp, false, lookup).expect("fits");
        assert_eq!((found.scale, found.picture.frames.len()), (2, 2));
        assert!(find("pips.shp", &shp, false, lookup).is_none(), "no file");
        let one = crate::assets::picture_file::encode_frames(4, 2, &sized(4, 2, 1).frames).unwrap();
        assert!(find("wrench.shp", &shp, false, |_| Some(one.as_slice())).is_none());
        let lines = crate::rules::art_problems::take();
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("[wrench.shp] wrench.png: it has 1 frames")),
            "{lines:?}"
        );
    }
}
