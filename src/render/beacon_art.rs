//! Beacon art. `Init_BeaconArt 0x004309D0`, called from the scenario's
//! Full_Init (`0x00687BB2`), loads PBEACON.SHP into `[0x0089C474]` and
//! RDRBEACN.SHP into `[0x0089C478]`. It keeps each one's header frame count
//! (`+0x6C`, `+0x78`) and four times RDRBEACN's (`+0x7C`).
//! `BeaconClass::Draw 0x00430250` (tactical) and `0x00430650` (radar) blit
//! them through the owning House's ColorScheme Convert (`ColorScheme+0x30C`,
//! `0x004302EE`, `0x004306ED`) at brightness 1000 with flags `0x600`:
//! centred, with no A-buffer and no Z. Init_Theater loads the theater's
//! `UNIT%s.PAL` into `0x00886380` (`0x00534CB2..0x00534CD9`) and rebuilds
//! every scheme's Convert from it (`0x00534D77` -> `0x0068C860`), so that
//! Convert's palette is the unit palette with indices 16..31 replaced by the
//! House's ramp (`ColorScheme__BuildRampPalette 0x0068C3B0`).
//!
//! ## Dependency rules
//! - Part of render/ — the caller supplies the visible records.

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, SpriteInstance};
use crate::render::palette_light::PaletteLight;
use crate::render::picture_atlas::PictureSprite;
use crate::render::selection_overlay::CenteredShapeStrip;
use crate::render::sidebar_chrome::side_mix_file;

/// One beacon a client draws, in the House-major order of the native loops
/// (`0x00430AC0`, `0x00431700`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeaconDraw {
    /// BeaconManager row: the owning House's array index.
    pub house: usize,
    /// Leptons.
    pub coord: [i32; 3],
    pub selected: bool,
}

pub struct BeaconArt {
    /// PBEACON.SHP in each row's ColorScheme Convert, or VERA's picture
    /// beside it with its mask's house pixels in the row's ramp.
    tactical: Vec<Option<CenteredShapeStrip>>,
    tactical_frames: u32,
    radar: Option<ShpFile>,
    /// Each row's Convert of every RDRBEACN index at brightness 1000.
    radar_colors: Vec<[[u8; 4]; 256]>,
    /// VERA-only, experimental (`docs/picture-model-art.md`, "Interface
    /// art"): `rdrbeacn.png` beside RDRBEACN.SHP at game pixels, drawn in
    /// place of the SHP's pixels; it needs no SHP.
    radar_picture: Option<RadarPicture>,
}

/// `rdrbeacn.png` at game pixels: its canvas and each row's frames with the
/// row's ramp.
struct RadarPicture {
    canvas: [u32; 2],
    frames: u32,
    rows: Vec<Vec<Option<PictureSprite>>>,
}

impl BeaconArt {
    pub fn house_count(&self) -> usize {
        self.tactical.len()
    }

    /// `unit_palette` is the theater's unit palette; `ramps` holds each
    /// beacon row's House ramp, in House order; `sizes` the pictures'
    /// `PictureSize=` (`render::interface_picture`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        side_index: i32,
        ares: bool,
        unit_palette: Option<&Palette>,
        ramps: &[[Color; 16]],
        sizes: crate::render::interface_picture::PictureSizes<'_>,
    ) -> Self {
        let file = |name: &str| side_mix_file(assets, side_index, ares, name);
        let shape = |name| {
            use crate::render::shell_ui_style_assets::{decode_ui_shp, ui_art_candidates};
            if assets.mod_selected() {
                ui_art_candidates(assets, name)
                    .find_map(|asset| decode_ui_shp(asset.bytes).ok())
                    .or_else(|| decode_ui_shp(side_mix_file(assets, side_index, ares, name)?).ok())
            } else {
                ShpFile::from_bytes(side_mix_file(assets, side_index, ares, name)?).ok()
            }
        };
        let palettes: Vec<Palette> = unit_palette
            .map(|base| {
                ramps
                    .iter()
                    .map(|ramp| base.with_house_colors(ramp))
                    .collect()
            })
            .unwrap_or_default();
        // VERA-only, experimental: the pictures beside the shapes, by name
        // in the same side MIX lookup (`render::interface_picture`).
        let picture = |name, shp: Option<&ShpFile>| {
            let shape = shp.map(crate::render::interface_picture::ArtShape::of_shp);
            crate::render::interface_picture::find(name, shape, sizes, true, file)?.game_frames()
        };
        let tactical_shape = shape("pbeacon.shp");
        let tactical_picture = picture("pbeacon.shp", tactical_shape.as_ref());
        let tactical = palettes
            .iter()
            .zip(ramps)
            .map(|(palette, ramp)| match &tactical_picture {
                Some(frames) => CenteredShapeStrip::from_picture_on_device(
                    device,
                    queue,
                    batch,
                    frames,
                    Some(ramp),
                ),
                None => CenteredShapeStrip::from_shp_on_device(
                    device,
                    queue,
                    batch,
                    tactical_shape.as_ref()?,
                    palette,
                ),
            })
            .collect();
        let convert = PaletteLight::color_scheme([1000; 3], 1000);
        let radar_colors = palettes
            .iter()
            .map(|palette| {
                std::array::from_fn(|index| {
                    let color = palette.color(index as u8);
                    let word = convert.rgb565([color.r, color.g, color.b], index as u8, 127);
                    let [r, g, b] = crate::render::native_surface_format::RGB565.unpack_rgb8(word);
                    [r, g, b, 255]
                })
            })
            .collect();
        let radar = shape("rdrbeacn.shp");
        let radar_picture = picture("rdrbeacn.shp", radar.as_ref()).map(|frames| RadarPicture {
            canvas: frames.canvas,
            frames: frames.frame_count() as u32,
            rows: ramps
                .iter()
                .take(palettes.len())
                .map(|ramp| {
                    (0..frames.frame_count())
                        .map(|frame| {
                            frames.sprite(
                                frame,
                                crate::render::picture_atlas::HouseColor::Baked(ramp),
                            )
                        })
                        .collect()
                })
                .collect(),
        });
        // 0x00430A2D and 0x00430A97 report each missing shape to the debug
        // log; play goes on without it.
        if tactical_shape.is_none() && tactical_picture.is_none() {
            log::warn!("Failed to load BeaconArt");
        }
        if radar.is_none() && radar_picture.is_none() {
            log::warn!("Failed to load RadarBeaconArt");
        }
        let tactical_frames = match (&tactical_picture, &tactical_shape) {
            (Some(frames), _) => frames.frame_count() as u32,
            (None, Some(shp)) => shp.frames.len() as u32,
            (None, None) => 0,
        };
        Self {
            tactical,
            tactical_frames,
            radar,
            radar_colors,
            radar_picture,
        }
    }

    /// The row's PBEACON strip, the texture of [`Self::tactical_instance`].
    pub fn tactical_strip(&self, house: usize) -> Option<&CenteredShapeStrip> {
        self.tactical.get(house)?.as_ref()
    }

    /// `BeaconClass::Draw`'s shape at the coordinate's tactical point
    /// (`CoordsToClient2 0x006D2140`), in VERA's world-pixel frame.
    pub fn tactical_instance(&self, beacon: &BeaconDraw, frame: u32) -> Option<SpriteInstance> {
        let index = tactical_frame(self.tactical_frames, beacon.selected, frame)?;
        let [x, y, z] = beacon.coord;
        let point = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        let mut instance = self
            .tactical_strip(beacon.house)?
            .instance(index, point, 0.0)?;
        instance.palette_light = PaletteLight::color_scheme([1000; 3], 1000);
        Some(instance)
    }

    /// `0x00430650`'s shape centred on `center`, the beacon's radar surface
    /// pixel (`0x006557F0` with clamping, `0x0043067D`). `put` receives each
    /// drawn surface pixel and its colour; index 0 is not drawn.
    pub fn blit_radar(
        &self,
        house: usize,
        frame: u32,
        center: (i32, i32),
        mut put: impl FnMut((i32, i32), [u8; 4]),
    ) {
        if let Some(picture) = &self.radar_picture {
            let Some(index) = radar_frame(picture.frames, frame) else {
                return;
            };
            let Some(sprite) = picture
                .rows
                .get(house)
                .and_then(|sprites| sprites.get(index))
                .and_then(Option::as_ref)
            else {
                return;
            };
            // The SHP's convert at brightness 1000, on the picture's colours.
            let convert = PaletteLight::color_scheme([1000; 3], 1000);
            let origin = (
                center.0 - (picture.canvas[0] / 2) as i32 + sprite.origin[0] as i32,
                center.1 - (picture.canvas[1] / 2) as i32 + sprite.origin[1] as i32,
            );
            let width = sprite.size[0] as usize;
            for (offset, &index) in sprite.indices.iter().enumerate() {
                if index != 0 {
                    let rgb = &sprite.rgba[offset * 4..offset * 4 + 3];
                    let word = convert.rgb565([rgb[0], rgb[1], rgb[2]], index, 127);
                    let [r, g, b] = crate::render::native_surface_format::RGB565.unpack_rgb8(word);
                    let pixel = (
                        origin.0 + (offset % width) as i32,
                        origin.1 + (offset / width) as i32,
                    );
                    put(pixel, [r, g, b, 255]);
                }
            }
            return;
        }
        let (Some(shp), Some(colors)) = (self.radar.as_ref(), self.radar_colors.get(house)) else {
            return;
        };
        let Some(index) = radar_frame(shp.frames.len() as u32, frame) else {
            return;
        };
        let Some(shape) = shp.frames.get(index) else {
            return;
        };
        let origin = (
            center.0 - i32::from(shp.width / 2) + i32::from(shape.frame_x),
            center.1 - i32::from(shp.height / 2) + i32::from(shape.frame_y),
        );
        let width = usize::from(shape.frame_width).max(1);
        for (offset, &index) in shape.pixels.iter().enumerate() {
            if index != 0 {
                let pixel = (
                    origin.0 + (offset % width) as i32,
                    origin.1 + (offset / width) as i32,
                );
                put(pixel, colors[usize::from(index)]);
            }
        }
    }
}

/// `0x00430296..0x004302E4`: an unselected beacon cycles the first half of
/// PBEACON's frames with the frame counter, a selected one the second half.
fn tactical_frame(frames: u32, selected: bool, counter: u32) -> Option<usize> {
    let half = frames / 2;
    (half != 0).then(|| (if selected { half } else { 0 } + counter % half) as usize)
}

/// `0x00430650..0x004306A7`: RDRBEACN plays once, then stays dark for three
/// more lengths of itself.
fn radar_frame(frames: u32, counter: u32) -> Option<usize> {
    let period = frames.checked_mul(4).filter(|period| *period != 0)?;
    let frame = counter % period;
    (frame < frames).then_some(frame as usize)
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn selection_picks_the_second_half_and_the_radar_blinks_one_in_four() {
        assert_eq!(tactical_frame(16, false, 19), Some(3));
        assert_eq!(tactical_frame(16, true, 19), Some(11));
        assert_eq!(tactical_frame(1, true, 5), None, "half of one frame");
        assert_eq!(radar_frame(5, 24), Some(4));
        assert_eq!(radar_frame(5, 25), None);
        assert_eq!(radar_frame(5, 40), Some(0));
    }

    /// Retail ships PBEACON.SHP and RDRBEACN.SHP only in sidec01.mix and
    /// sidec02.mix, the side MIX groups Full_Init mounts before
    /// Init_BeaconArt.
    #[test]
    fn retail_beacon_art_comes_from_each_sides_mix_group() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        for side in 0..3 {
            for name in ["pbeacon.shp", "rdrbeacn.shp"] {
                let shape = side_mix_file(&assets, side, false, name)
                    .and_then(|bytes| ShpFile::from_bytes(bytes).ok());
                assert!(
                    shape.is_some_and(|shp| !shp.frames.is_empty()),
                    "side {side} {name}"
                );
            }
        }
    }
}
