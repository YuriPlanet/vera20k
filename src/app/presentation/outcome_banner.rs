//! House FlagWin4FC9E0/FlagLose4FCBD0 -> Tactical SetScreenText6D4DB0,
//! Draw6D4B25 ->6D4E20. Steam gamemd original body/caller bytes.
//!
//! The selected campaign has one PlayerPtr result. Its text/frame is derived
//! from that House's existing serialized outcome; no second result or expiry
//! clock lives in presentation. New world/load/exit replaces that source. The
//! immutable process art follows InitGame52C1EF..52C302's literal files, with
//! native palette shifts and index-zero transparency; never theater/remap art.

use crate::app::AppState;
use crate::assets::{asset_manager::AssetManager, pal_file::Palette, shp_file::ShpFile};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::sim::house_state::HouseOutcomeKind;

struct Frame {
    size: [u32; 2],
    offset: [u16; 2],
    atlas_x: u32,
}

/// Process-lifetime GRFXTXT art. The frames retain SHP header coordinates.
pub(crate) struct OutcomeBannerArt {
    texture: BatchTexture,
    canvas: [i32; 2],
    atlas: [u32; 2],
    frames: Vec<Frame>,
}

impl OutcomeBannerArt {
    pub(crate) fn build(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
    ) -> Option<Self> {
        let shp = ShpFile::from_bytes(assets.get_ref("GRFXTXT.SHP")?).ok()?;
        //6D4E8D sign-extends the SHP frame count; its invalid-count branch
        //uses text. The normal retail shape has a small positive count.
        if (shp.frames.len() as i16) <= 0 {
            return None;
        }
        //6D4E20 draws the frame with CC_Draw_Shape SHAPE_NORMAL through the
        //GRFXTXT ConvertClass: the shared palette conversion, index 0 skipped.
        let palette = Palette::from_bytes(assets.get_ref("GRFXTXT.PAL")?).ok()?;
        let atlas_width: u32 = shp
            .frames
            .iter()
            .map(|frame| u32::from(frame.frame_width))
            .sum();
        let atlas_height = shp
            .frames
            .iter()
            .map(|frame| u32::from(frame.frame_height))
            .max()?;
        if atlas_width == 0 || atlas_height == 0 {
            return None;
        }
        let mut rgba = vec![0; (atlas_width * atlas_height * 4) as usize];
        let mut x = 0;
        let mut frames = Vec::with_capacity(shp.frames.len());
        for (index, frame) in shp.frames.iter().enumerate() {
            let pixels = shp.frame_to_rgba(index, &palette).ok()?;
            let width = u32::from(frame.frame_width);
            let height = u32::from(frame.frame_height);
            for row in 0..height {
                for col in 0..width {
                    let from = ((row * width + col) * 4) as usize;
                    let to = ((row * atlas_width + x + col) * 4) as usize;
                    let pixel: [u8; 4] = pixels[from..from + 4].try_into().ok()?;
                    //52C235..52C283 shifts raw PAL bytes then48E740 binds
                    //the converter. Reuse the enrolled RGB565 presentation
                    //boundary shared by native sidebar/loading surfaces.
                    rgba[to..to + 4].copy_from_slice(
                        &crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
                            .quantize_rgba8(pixel),
                    );
                }
            }
            frames.push(Frame {
                size: [width, height],
                offset: [frame.frame_x, frame.frame_y],
                atlas_x: x,
            });
            x += width;
        }
        Some(Self {
            texture: batch.create_texture(gpu, &rgba, atlas_width, atlas_height),
            canvas: [i32::from(shp.width as i16), i32::from(shp.height as i16)],
            atlas: [atlas_width, atlas_height],
            frames,
        })
    }

    pub(crate) fn texture(&self) -> &BatchTexture {
        &self.texture
    }

    fn quad(&self, index: usize, view: [i32; 2], camera: [f32; 2]) -> Option<SpriteInstance> {
        let frame = self.frames.get(index)?;
        let origin = centered_origin(view, self.canvas);
        Some(SpriteInstance {
            position: [
                origin[0] as f32 + f32::from(frame.offset[0]) + camera[0],
                origin[1] as f32 + f32::from(frame.offset[1]) + camera[1],
            ],
            size: frame.size.map(|value| value as f32),
            uv_origin: [frame.atlas_x as f32 / self.atlas[0] as f32, 0.0],
            uv_size: [
                frame.size[0] as f32 / self.atlas[0] as f32,
                frame.size[1] as f32 / self.atlas[1] as f32,
            ],
            depth: 0.00022,
            tint: [1.0; 3],
            alpha: 1.0,
            ..Default::default()
        })
    }
}

fn centered_origin(view: [i32; 2], canvas: [i32; 2]) -> [i32; 2] {
    //6D4E99..6D4EB7 uses signed subtraction/division toward zero. Negative
    //origins crop against the tactical rectangle, never fit/scale the shape.
    std::array::from_fn(|axis| view[axis].wrapping_sub(canvas[axis]) / 2)
}

fn result_view(kind: HouseOutcomeKind, campaign: bool) -> (usize, &'static str) {
    match (campaign, kind) {
        (true, HouseOutcomeKind::Victory) => (0, "TXT_SCENARIO_WON"),
        (true, HouseOutcomeKind::Defeat) => (1, "TXT_SCENARIO_LOST"),
        (false, HouseOutcomeKind::Victory) => (2, "TXT_VICTORIOUS"),
        (false, HouseOutcomeKind::Defeat) => (3, "TXT_LOST"),
    }
}

/// Tactical6D4E20 chooses shape first; centred white text is its missing-art
/// branch. Its geometry follows the tactical viewport and is screen-fixed.
pub(crate) fn instances(state: &AppState) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
        return Default::default();
    };
    let sim = &runtime.simulation;
    let Some(outcome) = sim
        .session()
        .current_house()
        .and_then(|house| sim.houses().get(&house))
        .and_then(|house| house.outcome())
    else {
        return Default::default();
    };
    let (frame, label) = result_view(outcome.kind, !sim.session().game_mode_nonzero());
    let (width, height) = crate::app::input::camera::tactical_viewport_size(state);
    let view = [width as i32, height as i32];
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    if let Some(quad) = state
        .renderer
        .outcome_banner_art
        .as_ref()
        .and_then(|art| art.quad(frame, view, camera))
    {
        return (vec![quad], Vec::new());
    }
    let text = state
        .process_assets
        .csf
        .as_ref()
        .map_or_else(String::new, |csf| csf.text(label).into_owned());
    let font = &state.renderer.bit_font;
    let origin = centered_origin(
        view,
        [font.text_width(&text) as i32, font.cell_height() as i32],
    );
    (
        Vec::new(),
        crate::render::sidebar_text::build_text(
            font,
            &text,
            origin[0] as f32,
            origin[1] as f32,
            1.0,
            0.00022,
            [1.0; 3],
            camera,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_signed_centering_keeps_large_shape_crop() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_outcomes.json",
        ))
        .unwrap();
        for row in corpus["banner_centers"].as_array().unwrap() {
            let read =
                |key: &str| std::array::from_fn(|axis| row[key][axis].as_i64().unwrap() as i32);
            assert_eq!(
                centered_origin(read("view"), read("canvas")),
                read("origin")
            );
        }
    }

    #[test]
    fn native_flag_banner_selectors_and_csf_literals() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/house_outcomes.json",
        ))
        .unwrap();
        for row in corpus["flag_controls"].as_array().unwrap() {
            let calls = row["calls"].as_array().unwrap();
            let Some(setter) = calls.iter().find(|call| call["address"] == "0x6d4db0") else {
                continue;
            };
            let csf = calls
                .iter()
                .find(|call| call["address"] == "0x734e60")
                .unwrap();
            let kind = if row["won"] == 1 {
                HouseOutcomeKind::Victory
            } else {
                HouseOutcomeKind::Defeat
            };
            let campaign = row["input"]["game_mode"].as_i64().unwrap_or(0) == 0;
            let (index, label) = result_view(kind, campaign);
            assert_eq!(
                index as u64,
                setter["stack"][1].as_u64().unwrap(),
                "{}",
                row["input"]["name"]
            );
            assert_eq!(label, csf["name"].as_str().unwrap());
        }
    }

    #[test]
    fn retail_result_art_is_bound_as_native_ui_shp_and_palette() {
        let Some((_directory, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let shp = ShpFile::from_bytes(
            assets
                .get_ref("GRFXTXT.SHP")
                .expect("InitGame literal shape"),
        )
        .unwrap();
        let palette = Palette::from_bytes(
            assets
                .get_ref("GRFXTXT.PAL")
                .expect("InitGame literal palette"),
        )
        .unwrap();
        assert!(shp.frames.len() >= 4, "Flag bodies select frames0..3");
        for index in 0..4 {
            let rgba = shp.frame_to_rgba(index, &palette).unwrap();
            assert_eq!(
                rgba.len(),
                usize::from(shp.frames[index].frame_width)
                    * usize::from(shp.frames[index].frame_height)
                    * 4
            );
            assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
        }
    }
}
