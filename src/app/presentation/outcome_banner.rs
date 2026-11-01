//! House FlagWin4FC9E0/FlagLose4FCBD0 -> Tactical SetScreenText6D4DB0,
//! Draw6D4B25 ->6D4E20. Steam gamemd original body/caller bytes.
//!
//! The selected campaign has one PlayerPtr result. Its text/frame is derived
//! from that House's existing serialized outcome; no second result or expiry
//! clock lives in presentation. New world/load/exit replaces that source. The
//! immutable process art follows InitGame52C1EF..52C302's literal files, with
//! native palette shifts and index-zero transparency; never theater/remap art.
//! An Ares side naming `GraphicalText.Image=`/`.Palette=` replaces those two
//! slots per game load (`Game_LoadUI_LoadSideData` 0x1002F4A0), each slot
//! falling back to the literal file on its own.

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

/// InitGame52C1EF..52C302's literal overlay files.
const STOCK_IMAGE: &str = "GRFXTXT.SHP";
const STOCK_PALETTE: &str = "GRFXTXT.PAL";

impl OutcomeBannerArt {
    pub(crate) fn build(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
    ) -> Option<Self> {
        Self::build_slots(gpu, batch, assets, None, None)
    }

    /// `Game_LoadUI_LoadSideData` (`0x1002F4A0`) hands the local side's
    /// `GraphicalText.Image=` / `.Palette=` to the slot writer at `0x1002ECC0`,
    /// and the draw resolves each slot through `0x1002DF10` / `0x1002DF00`,
    /// which answer InitGame's literal surface whenever that slot is unset or
    /// never loaded. Each slot therefore falls back on its own.
    #[cfg(feature = "ares")]
    pub(crate) fn build_for_side(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        image: Option<&str>,
        palette: Option<&str>,
    ) -> Option<Self> {
        Self::build_slots(gpu, batch, assets, image, palette)
    }

    fn build_slots(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        image: Option<&str>,
        palette: Option<&str>,
    ) -> Option<Self> {
        // A named image that will not parse falls back to InitGame's literal
        // surface, which is what the native slot lookup does.
        Self::build_exact(gpu, batch, assets, image.unwrap_or(STOCK_IMAGE), palette)
            .or_else(|| Self::build_exact(gpu, batch, assets, STOCK_IMAGE, palette))
    }

    /// One named SHP/PAL pair, no fallback: the country cards
    /// (`[EndGameScreens]`) must never quietly show the retail banner instead.
    fn build_exact(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        image: &str,
        palette: Option<&str>,
    ) -> Option<Self> {
        let shp = assets
            .get_ref(image)
            .and_then(|bytes| ShpFile::from_bytes(bytes).ok())?;
        //6D4E8D sign-extends the SHP frame count; its invalid-count branch
        //uses text. The normal retail shape has a small positive count.
        if (shp.frames.len() as i16) <= 0 {
            return None;
        }
        //6D4E20 draws the frame with CC_Draw_Shape SHAPE_NORMAL through the
        //GRFXTXT ConvertClass: the shared palette conversion, index 0 skipped.
        let palette = assets
            .get_ref(palette.unwrap_or(STOCK_PALETTE))
            .and_then(|bytes| Palette::from_bytes(bytes).ok())
            .or_else(|| {
                assets
                    .get_ref(STOCK_PALETTE)
                    .and_then(|bytes| Palette::from_bytes(bytes).ok())
            })?;
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

/// The local country's own win/lose card (`[EndGameScreens]`), drawn into the
/// same slot as the retail `GRFXTXT` banner.
///
/// The slot is the tactical viewport, not the screen: the sidebar and the
/// radar window stay uncovered, so a beaten commander's closing clip can play
/// there while the card is up, and the card itself is fitted into the
/// viewport instead of being cropped by it. One picture per outcome, built
/// once per match; a country that names neither half answers `None` and the
/// retail banner stays.
pub(crate) struct EndGameBanner {
    victory: Option<OutcomeBannerArt>,
    defeat: Option<OutcomeBannerArt>,
}

impl EndGameBanner {
    pub(crate) fn build(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        screens: &crate::rules::end_game::EndGameCatalog,
        country: &str,
    ) -> Option<Self> {
        let half = |kind| {
            let art = screens.presentation(country, kind)?.banner_art()?;
            if !assets.contains(&art.shape) {
                log::warn!(
                    "[EndGameScreens] {} is not installed; the retail end-game banner stays",
                    art.shape
                );
                return None;
            }
            OutcomeBannerArt::build_exact(gpu, batch, assets, &art.shape, art.palette.as_deref())
                .or_else(|| {
                    log::warn!(
                        "[EndGameScreens] {} could not be read; the retail end-game banner stays",
                        art.shape
                    );
                    None
                })
        };
        let victory = half(crate::rules::end_game::EndGameKind::Victory);
        let defeat = half(crate::rules::end_game::EndGameKind::Defeat);
        if victory.is_none() && defeat.is_none() {
            return None;
        }
        log::info!(
            "[EndGameScreens] in-game banner for {country}: victory={} defeat={}",
            victory.is_some(),
            defeat.is_some()
        );
        Some(Self { victory, defeat })
    }

    fn art(&self, kind: HouseOutcomeKind) -> Option<&OutcomeBannerArt> {
        match kind {
            HouseOutcomeKind::Victory => self.victory.as_ref(),
            HouseOutcomeKind::Defeat => self.defeat.as_ref(),
        }
    }
}

/// The verdict this exit router is presenting — the one FlagWin4FC9E0 /
/// FlagLose4FCBD0 would have turned into `TXT_VICTORIOUS`/`TXT_LOST`.
///
/// Native feeds `Tactical SetScreenText 6D4DB0` once, at Flag time, and the
/// text object outlives everything after it. The House flag does not:
/// `House4F8657` clears Won the instant the whole-Vox wait ends
/// (`complete_outcome`), which is before `Main_Game 55CFD0` routes the result.
/// Re-deriving the half from `house.outcome()` at draw time therefore answered
/// nothing for a victory once the verdict was taken late — the defeat half
/// kept working only because `4F8780..4F87BB` deliberately retains Lost. The
/// cascade holds the verdict the router took, and everything here reads it.
fn local_outcome(state: &AppState) -> Option<HouseOutcomeKind> {
    state.match_state.scenario_exit.as_ref()?.verdict()
}

/// The country card this match selected, when it can actually be drawn.
///
/// One owner for both halves of the draw: [`instances`] samples these frames
/// and the tactical pass binds [`OutcomeBannerArt::texture`] for the emitted
/// quads. Splitting the decision once made the quads address the retail
/// `GRFXTXT` atlas while they carried the card's size and offsets, which is a
/// garbled picture rather than a missing one.
pub(crate) fn active_art(state: &AppState) -> Option<&OutcomeBannerArt> {
    if !state.persistence.options_profile.custom_end_game_banner {
        return None;
    }
    // A match, not a mission: the campaign's own `SCENARIO WON`/`LOST`
    // banners belong to the campaign's art.
    if !state
        .match_state
        .sim_runtime
        .as_ref()?
        .simulation
        .session()
        .game_mode_nonzero()
    {
        return None;
    }
    let kind = local_outcome(state)?;
    state
        .match_state
        .match_presentation
        .end_game_banner
        .as_ref()?
        .art(kind)
        .filter(|art| art.frames.first().is_some() && art.canvas[0] > 0 && art.canvas[1] > 0)
}

/// The atlas the outcome pass must bind: whichever picture [`instances`] is
/// about to describe. `None` keeps the caller's own default, the retail
/// `GRFXTXT` slot.
pub(crate) fn texture(state: &AppState) -> Option<&BatchTexture> {
    active_art(state)
        .or(state
            .match_state
            .match_presentation
            .outcome_banner_side_art
            .as_ref())
        .or(state.renderer.outcome_banner_art.as_ref())
        .map(OutcomeBannerArt::texture)
}

impl OutcomeBannerArt {
    /// Fit the whole picture inside the tactical viewport and centre it.
    /// Retail's frames are already the size of their slot and keep
    /// [`Self::quad`]'s native crop; a country card is a full-screen
    /// composition and must not lose its edges to the sidebar.
    fn fit_quad(&self, view: [i32; 2], camera: [f32; 2]) -> Option<SpriteInstance> {
        let frame = self.frames.first()?;
        if self.canvas[0] <= 0 || self.canvas[1] <= 0 || view[0] <= 0 || view[1] <= 0 {
            return None;
        }
        let scale = (view[0] as f32 / self.canvas[0] as f32)
            .min(view[1] as f32 / self.canvas[1] as f32)
            .min(1.0);
        if scale <= 0.0 {
            return None;
        }
        let size = [self.canvas[0] as f32 * scale, self.canvas[1] as f32 * scale];
        Some(SpriteInstance {
            position: [
                (view[0] as f32 - size[0]) / 2.0 + f32::from(frame.offset[0]) * scale + camera[0],
                (view[1] as f32 - size[1]) / 2.0 + f32::from(frame.offset[1]) * scale + camera[1],
            ],
            size,
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

/// Throttle for the verdict-beat diagnostic in [`instances`].
static VERDICT_BEAT_LOG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    // Nothing is drawn until the exit router has taken the result and recorded
    // it: the House flag is cleared mid-wait, so it is no verdict source at
    // all. See [`local_outcome`].
    let Some(outcome_kind) = local_outcome(state) else {
        return Default::default();
    };
    let campaign = !state
        .match_state
        .sim_runtime
        .as_ref()
        .is_some_and(|runtime| runtime.simulation.session().game_mode_nonzero());
    let (frame, label) = result_view(outcome_kind, campaign);
    let (width, height) = crate::app::input::camera::tactical_viewport_size(state);
    let view = [width as i32, height as i32];
    let camera = [
        state.match_state.input.camera_x,
        state.match_state.input.camera_y,
    ];
    let card = active_art(state).and_then(|art| art.fit_quad(view, camera));
    // Runtime evidence for the verdict beat. Every earlier round reasoned about
    // this draw from the timing logs around it; none of them proved the frame
    // actually produced a quad, or which atlas it is about to sample. Throttled
    // to one line every 500 ms so a three-second beat leaves a handful.
    let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
    let previous = VERDICT_BEAT_LOG_MS.swap(now_ms, std::sync::atomic::Ordering::Relaxed);
    if now_ms.saturating_sub(previous) >= 500 {
        log::info!(
            "Verdict beat draw: outcome={} label={label} view={view:?} camera={camera:?} \
             card={} side_art={} retail_art={}",
            match outcome_kind {
                HouseOutcomeKind::Victory => "victory",
                HouseOutcomeKind::Defeat => "defeat",
            },
            card.is_some(),
            state
                .match_state
                .match_presentation
                .outcome_banner_side_art
                .is_some(),
            state.renderer.outcome_banner_art.is_some(),
        );
    }
    if let Some(quad) = card {
        return (vec![quad], Vec::new());
    }
    if let Some(quad) = state
        .match_state
        .match_presentation
        .outcome_banner_side_art
        .as_ref()
        .or(state.renderer.outcome_banner_art.as_ref())
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
