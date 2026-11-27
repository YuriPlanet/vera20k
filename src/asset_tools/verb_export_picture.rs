//! `asset export-picture <NAME>`: an SHP as picture art
//! (`docs/picture-model-art.md`): `<NAME>.png`, an APNG with one whole-canvas
//! frame per SHP frame, and `<NAME>.mask.png` beside it when the SHP has
//! house-colour or unlit pixels.
//!
//! Palette index 0 is transparent. House-colour indices 16–31 are written
//! grey, 255 for index 16 and 17 darker per step, and marked red in the mask,
//! so the game puts each back on the owner's ramp at the same step. Indices
//! 240–254 keep their colour and are marked green (unlit). Everything else
//! takes its palette colour. Declared with `Picture=`, such a picture draws as
//! its SHP does. `--scale N` writes it at N times the resolution (each pixel
//! N x N), to be declared with `PictureSize=` the SHP's canvas. Files made
//! from retail art stay on the player's machine.
//!
//! `--plain` keeps indices 16–31 their palette colour, for art that has no
//! house colour (overlays, terrain objects, smudges).
//!
//! A name with another extension is that file: a theater SHP (`TREE01.TEM`)
//! writes `<NAME>.<ext>.png`, the name a `Theater=yes` section's picture is
//! looked up by; a TMP template (`CLEAR01.TEM`) writes `<NAME>.png`, one
//! frame per cell slot, each cell's canvas at its top-left corner, the frames
//! as large as the largest cell, for a tileset's `PictureFileName=`.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` only.

use crate::asset_tools::report::ErrorReport;
use crate::asset_tools::verb_render::RenderOptions;
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;

/// Subdirectory under the output root that this verb owns.
const PICTURE_SUBDIR: &str = "picture";
/// The palette pictures take their colours from unless `--palette` names one.
const DEFAULT_PALETTE: &str = "unittem.pal";

#[derive(Debug, serde::Serialize)]
pub struct ExportPictureReport {
    pub picture: String,
    pub mask: Option<String>,
    pub frames: usize,
    pub canvas: [u32; 2],
    pub palette: String,
    pub house_pixels: usize,
    pub unlit_pixels: usize,
}

fn fail(error: String) -> ErrorReport {
    ErrorReport { error, hint: None }
}

/// Write `<out>/picture/<NAME>.png` (and `.mask.png`) from `<NAME>.SHP`.
pub fn run(
    asset_manager: &AssetManager,
    name: &str,
    opts: &RenderOptions,
) -> Result<ExportPictureReport, ErrorReport> {
    let (stem, extension) = name
        .rsplit_once('.')
        .map_or((name, "shp"), |(stem, ext)| (stem, ext));
    let stem = stem.to_ascii_uppercase();
    let extension = extension.to_ascii_lowercase();
    let shp_name = format!("{stem}.{}", extension.to_ascii_uppercase());
    let bytes = asset_manager
        .get_ref(&shp_name)
        .ok_or_else(|| ErrorReport {
            error: format!("asset not found: {shp_name}"),
            hint: Some(format!("run `asset find {shp_name}`")),
        })?;
    let palette_name = opts.palette.as_deref().unwrap_or(DEFAULT_PALETTE);
    let palette = asset_manager
        .get_ref(palette_name)
        .and_then(|bytes| Palette::from_bytes(bytes).ok())
        .ok_or_else(|| fail(format!("palette not found: {palette_name}")))?;
    let shp = match ShpFile::from_bytes(bytes) {
        Ok(shp) => shp,
        Err(error) if extension != "shp" => {
            return export_tile_picture(bytes, &stem, &palette, palette_name, opts)
                .map_err(|tile| fail(format!("{shp_name} is no SHP ({error}) nor TMP ({tile})")));
        }
        Err(error) => return Err(fail(format!("{shp_name} does not parse: {error}"))),
    };
    let stem = if extension == "shp" {
        stem
    } else {
        format!("{stem}.{extension}")
    };
    let (width, height) = (u32::from(shp.width), u32::from(shp.height));
    let canvas = (width * height) as usize;
    let mut pictures = Vec::with_capacity(shp.frames.len());
    let mut masks = Vec::with_capacity(shp.frames.len());
    let (mut house_pixels, mut unlit_pixels) = (0, 0);
    for (index, frame) in shp.frames.iter().enumerate() {
        let mut picture = vec![0u8; canvas * 4];
        let mut mask = vec![0u8; canvas * 4];
        let indices = ShpFile::decode_frame(bytes, index)
            .map_err(|error| fail(format!("{shp_name} frame {index}: {error}")))?;
        let (fw, fh) = (u32::from(frame.frame_width), u32::from(frame.frame_height));
        for row in 0..fh {
            for column in 0..fw {
                let (x, y) = (
                    u32::from(frame.frame_x) + column,
                    u32::from(frame.frame_y) + row,
                );
                let Some(&value) = indices.get((row * fw + column) as usize) else {
                    continue;
                };
                if value == 0 || x >= width || y >= height {
                    continue;
                }
                let at = ((y * width + x) * 4) as usize;
                let color = palette.color(value);
                let (rgb, mark) = match value {
                    16..=31 if !opts.plain => {
                        house_pixels += 1;
                        let grey = 255 - (value - 16) * 17;
                        ([grey; 3], [255, 0, 0, 255])
                    }
                    240..=254 => {
                        unlit_pixels += 1;
                        ([color.r, color.g, color.b], [0, 255, 0, 255])
                    }
                    _ => ([color.r, color.g, color.b], [0, 0, 0, 255]),
                };
                picture[at..at + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                mask[at..at + 4].copy_from_slice(&mark);
            }
        }
        pictures.push(picture);
        masks.push(mask);
    }
    // `--scale N`: N x N copies of each pixel, the 1x art at N times its
    // resolution for `PictureSize=` (width, height) and the zoomed drawing.
    let scale = opts.scale.unwrap_or(1).clamp(1, 8);
    let enlarge = |frame: &Vec<u8>| -> Vec<u8> {
        if scale == 1 {
            return frame.clone();
        }
        let mut out = Vec::with_capacity(frame.len() * (scale * scale) as usize);
        for y in 0..height * scale {
            for x in 0..width * scale {
                let at = (((y / scale) * width + x / scale) * 4) as usize;
                out.extend_from_slice(&frame[at..at + 4]);
            }
        }
        out
    };
    let encode = |frames: &[Vec<u8>]| {
        let frames: Vec<Vec<u8>> = frames.iter().map(enlarge).collect();
        crate::assets::picture_file::encode_frames(width * scale, height * scale, &frames)
            .map_err(|error| fail(format!("encoding {stem}: {error}")))
    };
    let dir = opts.out.join(PICTURE_SUBDIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| fail(format!("cannot create {}: {error}", dir.display())))?;
    let write = |file: String, bytes: Vec<u8>| {
        let path = dir.join(&file);
        std::fs::write(&path, bytes)
            .map_err(|error| fail(format!("cannot write {}: {error}", path.display())))?;
        Ok::<_, ErrorReport>(path.display().to_string())
    };
    let picture = write(format!("{stem}.png"), encode(&pictures)?)?;
    let mask = if house_pixels + unlit_pixels > 0 {
        Some(write(format!("{stem}.mask.png"), encode(&masks)?)?)
    } else {
        None
    };
    Ok(ExportPictureReport {
        picture,
        mask,
        frames: shp.frames.len(),
        canvas: [width, height],
        palette: palette_name.to_owned(),
        house_pixels,
        unlit_pixels,
    })
}

/// A TMP template as a tile picture (`PictureFileName=`): `<NAME>.png`, one
/// frame per cell slot (an empty slot is an empty frame), each cell's canvas
/// through `palette` at the frame's top-left corner.
fn export_tile_picture(
    bytes: &[u8],
    stem: &str,
    palette: &Palette,
    palette_name: &str,
    opts: &RenderOptions,
) -> Result<ExportPictureReport, String> {
    let tmp = crate::assets::tmp_file::TmpFile::from_bytes(bytes).map_err(|e| e.to_string())?;
    let [width, height] = tmp.tiles.iter().flatten().fold([1u32, 1], |max, tile| {
        [max[0].max(tile.pixel_width), max[1].max(tile.pixel_height)]
    });
    let scale = opts.scale.unwrap_or(1).clamp(1, 8);
    let mut frames = Vec::with_capacity(tmp.tiles.len());
    for (index, tile) in tmp.tiles.iter().enumerate() {
        let mut frame = vec![0u8; (width * scale * height * scale * 4) as usize];
        if let Some(tile) = tile {
            let rgba = tmp
                .tile_to_rgba(index, palette)
                .map_err(|e| e.to_string())?;
            for y in 0..tile.pixel_height * scale {
                for x in 0..tile.pixel_width * scale {
                    let from = (((y / scale) * tile.pixel_width + x / scale) * 4) as usize;
                    let to = ((y * width * scale + x) * 4) as usize;
                    frame[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
                }
            }
        }
        frames.push(frame);
    }
    let encoded =
        crate::assets::picture_file::encode_frames(width * scale, height * scale, &frames)
            .map_err(|e| e.to_string())?;
    let dir = opts.out.join(PICTURE_SUBDIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{stem}.png"));
    std::fs::write(&path, encoded).map_err(|e| e.to_string())?;
    Ok(ExportPictureReport {
        picture: path.display().to_string(),
        mask: None,
        frames: frames.len(),
        canvas: [width, height],
        palette: palette_name.to_owned(),
        house_pixels: 0,
        unlit_pixels: 0,
    })
}
