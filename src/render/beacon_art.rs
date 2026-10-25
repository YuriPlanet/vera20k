//! Beacon art. `Init_BeaconArt 0x004309D0`, called from the scenario's
//! Full_Init (`0x00687BB2`), loads PBEACON.SHP into `[0x0089C474]` and
//! RDRBEACN.SHP into `[0x0089C478]`. It keeps each one's header frame count
//! (`+0x6C`, `+0x78`) and four times RDRBEACN's (`+0x7C`).
//! `BeaconClass::Draw 0x00430250` (tactical) and `0x00430650` (radar) blit
//! them through the owning House's ColorScheme Convert (`ColorScheme+0x30C`,
//! `0x004302EE`, `0x004306ED`) at brightness 1000 with flags `0x600`:
//! centred, with no A-buffer and no Z. That Convert's palette is TEMPERAT.PAL
//! with indices 16..31 replaced by the House's ramp
//! (`ColorScheme__Constructor 0x0068C710`).
//!
//! ## Dependency rules
//! - Part of render/ — the caller supplies the visible records.

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, SpriteInstance};
use crate::render::palette_light::PaletteLight;
use crate::render::selection_overlay::CenteredShapeStrip;

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
    /// PBEACON.SHP in each row's ColorScheme Convert.
    tactical: Vec<Option<CenteredShapeStrip>>,
    tactical_frames: u32,
    radar: Option<ShpFile>,
    /// Each row's Convert of every RDRBEACN index at brightness 1000.
    radar_colors: Vec<[[u8; 4]; 256]>,
}

impl BeaconArt {
    pub fn house_count(&self) -> usize {
        self.tactical.len()
    }

    /// `ramps` holds each beacon row's House ramp, in House order.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        assets: &AssetManager,
        ramps: &[[Color; 16]],
    ) -> Self {
        let shape = |name| ShpFile::from_bytes(assets.get_ref(name)?).ok();
        let base = assets
            .get_ref("temperat.pal")
            .and_then(|bytes| Palette::from_bytes(bytes).ok());
        let palettes: Vec<Palette> = base
            .map(|base| {
                ramps
                    .iter()
                    .map(|ramp| base.with_house_colors(ramp))
                    .collect()
            })
            .unwrap_or_default();
        let tactical_shape = shape("pbeacon.shp");
        let tactical = palettes
            .iter()
            .map(|palette| {
                CenteredShapeStrip::from_shp_on_device(
                    device,
                    queue,
                    batch,
                    tactical_shape.as_ref()?,
                    palette,
                )
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
        Self {
            tactical,
            tactical_frames: tactical_shape.map_or(0, |shp| shp.frames.len() as u32),
            radar: shape("rdrbeacn.shp"),
            radar_colors,
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
        let (Some(shp), Some(colors)) = (self.radar.as_ref(), self.radar_colors.get(house)) else {
            return;
        };
        let Some(shape) =
            radar_frame(shp.frames.len() as u32, frame).and_then(|i| shp.frames.get(i))
        else {
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

#[cfg(test)]
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
}
