//! House FlagWin4FC9E0/FlagLose4FCBD0 -> Tactical SetScreenText6D4DB0,
//! Draw6D4B25 ->6D4E20. Steam gamemd original body/caller bytes.
//!
//! The selected campaign has one PlayerPtr result. Its text/frame is derived
//! from that House's serialized outcome, the result the simulation published,
//! or the verdict the exit router took; no second result or expiry clock lives
//! in presentation. New world/load/exit replaces that source. The
//! immutable process art follows InitGame52C1EF..52C302's literal files, with
//! native palette shifts and index-zero transparency; never theater/remap art.
//! An Ares side naming `GraphicalText.Image=`/`.Palette=` replaces those two
//! slots per game load (`Game_LoadUI_LoadSideData` 0x1002F4A0), each slot
//! falling back to the literal file on its own.

use crate::app::match_runtime::scenario_exit::ScenarioExitCascade;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::assets::{asset_manager::AssetManager, pal_file::Palette, shp_file::ShpFile};
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::shell_ui_style_assets::{decode_ui_shp, ui_art_candidates};
use crate::sim::house_state::HouseOutcomeKind;
use crate::ui::game_screen::GameScreen;

/// The verdict in the last battlefield picture handed off for presentation.
#[derive(Default)]
pub(crate) struct OutcomeBannerPresentation {
    last_presented: Option<HouseOutcomeKind>,
}

impl OutcomeBannerPresentation {
    pub(crate) fn record_presented(&mut self, verdict: Option<HouseOutcomeKind>) {
        self.last_presented = verdict;
    }

    pub(crate) fn hold_on_exit(
        &self,
        verdict: HouseOutcomeKind,
        custom_banner: bool,
        visible_skirmish: bool,
    ) -> bool {
        custom_banner || (visible_skirmish && self.last_presented != Some(verdict))
    }
}

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
        let texture_limit = gpu.device.limits().max_texture_dimension_2d;
        let (shp, atlas_width, atlas_height) =
            ui_art_candidates(assets, image).find_map(|asset| {
                let shp = if assets.mod_selected() {
                    decode_ui_shp(asset.bytes).ok()?
                } else {
                    ShpFile::from_bytes(asset.bytes).ok()?
                };
                // 6D4E8D sign-extends the SHP frame count. The invalid-count
                // branch uses text.
                if (shp.frames.len() as i16) <= 0 {
                    return None;
                }
                let width = shp
                    .frames
                    .iter()
                    .map(|frame| u32::from(frame.frame_width))
                    .sum::<u32>();
                let height = shp
                    .frames
                    .iter()
                    .map(|frame| u32::from(frame.frame_height))
                    .max()?;
                if width == 0 || height == 0 || width.max(height) > texture_limit {
                    return None;
                }
                log::debug!("Outcome banner {image} resolved from {}", asset.source);
                Some((shp, width, height))
            })?;
        //6D4E20 draws the frame with CC_Draw_Shape SHAPE_NORMAL through the
        //GRFXTXT ConvertClass: the shared palette conversion, index 0 skipped.
        let palette_for = |name| {
            ui_art_candidates(assets, name).find_map(|asset| Palette::from_bytes(asset.bytes).ok())
        };
        let palette = palette
            .and_then(palette_for)
            .or_else(|| palette_for(STOCK_PALETTE))?;
        let mut rgba = vec![0; atlas_width.checked_mul(atlas_height)?.checked_mul(4)? as usize];
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

/// The verdict the exit router took. Only the country card reads it alone:
/// the card goes up when the router takes the result.
fn router_verdict(scenario_exit: &Option<ScenarioExitCascade>) -> Option<HouseOutcomeKind> {
    scenario_exit.as_ref()?.verdict()
}

/// The result the banner shows, from the frame the House is flagged.
///
/// Native sets the text once, at Flag time (FlagWin4FC9E0 calls
/// `Tactical SetScreenText 6D4DB0` at 4FCB5D/4FCB9A, FlagLose4FCBD0 at
/// 4FCD46/4FCD83), and the text outlives the rest of the sequence. The House
/// flag does not: House4F8657 clears Won when the whole-Vox wait ends
/// (`complete_outcome`), before Main_Game routes the result, while
/// 4F8780..4F87BB keeps Lost. So the banner reads the router's verdict, then
/// [`sim_outcome`] — the result the simulation already published.
///
/// Nothing is withheld while the losing commander's clip plays: the card is
/// the verdict from the kill frame on, exactly as the native text is, and it
/// stays on screen through the clip until the score page takes the picture.
fn local_outcome(match_state: &MatchState) -> Option<HouseOutcomeKind> {
    router_verdict(&match_state.scenario_exit)
        .or_else(|| sim_outcome(&match_state.sim_runtime.presentation()?.simulation))
}

/// The local House's result as the simulation holds it: the one it published,
/// else the House's own flag.
fn sim_outcome(sim: &crate::sim::world::Simulation) -> Option<HouseOutcomeKind> {
    let owner = sim.session().current_house()?;
    sim.ready_outcome_for_owner(owner)
        .or_else(|| sim.houses().get(&owner)?.outcome())
        .map(|outcome| outcome.kind)
}

/// The country card this match selected, when it can actually be drawn.
///
/// One owner for both halves of the draw: [`instances`] samples these frames
/// and the tactical pass binds [`OutcomeBannerArt::texture`] for the emitted
/// quads. Splitting the decision once made the quads address the retail
/// `GRFXTXT` atlas while they carried the card's size and offsets, which is a
/// garbled picture rather than a missing one.
pub(crate) fn active_art<'a>(
    match_state: &'a MatchState,
    options_profile: &RetailOptionsProfile,
) -> Option<&'a OutcomeBannerArt> {
    if !options_profile.custom_end_game_banner {
        return None;
    }
    // A match, not a mission: the campaign's own `SCENARIO WON`/`LOST`
    // banners belong to the campaign's art.
    if !match_state
        .sim_runtime
        .presentation()?
        .simulation
        .session()
        .game_mode_nonzero()
    {
        return None;
    }
    // The same answer the instances draw from. Reading the router only would
    // leave the card unavailable for the whole clip wait — the router has not
    // taken the result yet — and the draw would fall back to the retail text
    // the simulation's own published flag already selects. The two must agree
    // about which half is on screen, so both ask [`local_outcome`].
    let kind = local_outcome(match_state)?;
    match_state
        .match_presentation
        .end_game_banner
        .as_ref()?
        .art(kind)
        .filter(|art| art.frames.first().is_some() && art.canvas[0] > 0 && art.canvas[1] > 0)
}

/// The atlas the outcome pass must bind: whichever picture [`instances`] is
/// about to describe. `None` keeps the caller's own default, the retail
/// `GRFXTXT` slot.
pub(crate) fn texture<'a>(
    outcome_banner_art: &'a Option<OutcomeBannerArt>,
    match_state: &'a MatchState,
    options_profile: &RetailOptionsProfile,
) -> Option<&'a BatchTexture> {
    active_art(match_state, options_profile)
        .or(match_state
            .match_presentation
            .outcome_banner_side_art
            .as_ref())
        .or(outcome_banner_art.as_ref())
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

fn result_view(kind: HouseOutcomeKind, campaign: bool) -> (usize, &'static str) {
    match (campaign, kind) {
        (true, HouseOutcomeKind::Victory) => (0, "TXT_SCENARIO_WON"),
        (true, HouseOutcomeKind::Defeat) => (1, "TXT_SCENARIO_LOST"),
        (false, HouseOutcomeKind::Victory) => (2, "TXT_VICTORIOUS"),
        (false, HouseOutcomeKind::Defeat) => (3, "TXT_LOST"),
    }
}

pub(crate) fn visible_verdict(match_state: &MatchState) -> Option<HouseOutcomeKind> {
    local_outcome(match_state).filter(|_| {
        crate::app::presentation::field_parts::shows(
            &match_state.match_presentation,
            crate::ui::field_layout::FieldPart::Banner,
        )
    })
}

/// Tactical6D4E20 chooses shape first; centred white text is its missing-art
/// branch. Its geometry follows the tactical viewport and is screen-fixed.
/// The banner as nodes: its picture, or the result's text where no
/// picture loaded; either is the layout's part `Banner`.
pub(crate) struct BannerHuds {
    picture: crate::app::presentation::picture_hud::PictureHud,
    text: crate::app::presentation::text_hud::TextHud,
}

impl Default for BannerHuds {
    fn default() -> Self {
        Self {
            picture: Default::default(),
            text: crate::app::presentation::text_hud::TextHud::named(BANNER),
        }
    }
}

/// The banner's `veraui.ini` key.
const BANNER: &str = "Banner";

impl BannerHuds {
    /// No banner this frame.
    pub(crate) fn clear(&mut self) {
        self.picture.begin();
        self.picture.finish();
        self.text.begin();
    }
}

pub(crate) fn instances(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    outcome_kind: HouseOutcomeKind,
    huds: &mut BannerHuds,
) -> (Vec<SpriteInstance>, Vec<SpriteInstance>) {
    let quad = banner_quad(
        platform,
        renderer,
        screen,
        match_state,
        process_assets,
        persistence,
        outcome_kind,
    );
    huds.picture.begin();
    huds.picture.part(BANNER);
    huds.text.begin();
    let font = &renderer.bit_font;
    let result = match quad {
        BannerQuad::Picture(quad, camera) => {
            let [x, y] = [quad.position[0] - camera[0], quad.position[1] - camera[1]];
            huds.picture.picture(
                crate::app::presentation::picture_hud::covering(x, y, quad.size[0], quad.size[1]),
                quad,
            );
            (huds.picture.finish().to_vec(), Vec::new())
        }
        BannerQuad::Text(text, origin, camera) => {
            huds.picture.finish();
            huds.text.text_tinted(
                vera20k_gui::Rect::new(
                    origin[0],
                    origin[1],
                    font.text_width(&text) as i32,
                    font.cell_height() as i32,
                ),
                &text,
                [origin[0] as f32 + camera[0], origin[1] as f32 + camera[1]],
                crate::app::presentation::text_hud::rgb([255; 3]),
                0.00022,
                None,
                Some([1.0; 3]),
            );
            huds.text.finish(font);
            (Vec::new(), huds.text.glyphs().copied().collect())
        }
    };
    result
}

/// What the banner shows: its picture's quad, or the result's text from
/// its top left in the box's pixels; each with the lane camera.
enum BannerQuad {
    Picture(SpriteInstance, [f32; 2]),
    Text(String, [i32; 2], [f32; 2]),
}

fn banner_quad(
    platform: &PlatformState,
    renderer: &RendererState,
    screen: &GameScreen,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    outcome_kind: HouseOutcomeKind,
) -> BannerQuad {
    let campaign = !match_state
        .sim_runtime
        .presentation()
        .is_some_and(|runtime| runtime.simulation.session().game_mode_nonzero());
    let (frame, label) = result_view(outcome_kind, campaign);
    use crate::app::presentation::field_parts;
    use crate::ui::field_layout::FieldPart;
    // The banner is centred in its box, the battlefield in the original
    // interface, and laid out in the box's own pixels.
    let [_, _, width, height] = field_parts::rect(
        platform,
        renderer,
        screen,
        &match_state.match_presentation,
        persistence,
        FieldPart::Banner,
    );
    let view = [width, height];
    let camera = field_parts::lane_camera(
        platform,
        renderer,
        screen,
        match_state,
        persistence,
        FieldPart::Banner,
    );
    if let Some(quad) = active_art(match_state, &persistence.options_profile)
        .and_then(|art| art.fit_quad(view, camera))
    {
        return BannerQuad::Picture(quad, camera);
    }
    if let Some(quad) = match_state
        .match_presentation
        .outcome_banner_side_art
        .as_ref()
        .or(renderer.outcome_banner_art.as_ref())
        .and_then(|art| art.quad(frame, view, camera))
    {
        return BannerQuad::Picture(quad, camera);
    }
    let text = process_assets.csf_text(label);
    let font = &renderer.bit_font;
    let origin = centered_origin(
        view,
        [font.text_width(&text) as i32, font.cell_height() as i32],
    );
    BannerQuad::Text(text, origin, camera)
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;

    /// House4F8657 clears Won when the whole-Vox wait ends, before Main_Game
    /// routes the result; the banner keeps its text through that frame.
    #[test]
    fn a_victory_keeps_its_banner_after_the_house_clears_won() {
        use crate::sim::house_state::HouseState;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::scenario_session::ScenarioGameMode;
        use crate::sim::world::{FrameEffects, Simulation, TickLane};
        let mut sim = Simulation::with_seed(0x004F_8657);
        let owner = sim.intern("Human");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        assert!(house.accept_win_result(0, 0, false));
        sim.houses_mut_for_test().register(house);
        sim.session_mut_for_test()
            .set_game_mode(ScenarioGameMode::Skirmish);
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        assert_eq!(sim_outcome(&sim), Some(HouseOutcomeKind::Victory));
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime
            .advance_frame(&[], 67, TickLane::Ordinary, &mut FrameEffects::default())
            .unwrap();
        assert!(!runtime.simulation.houses()[&owner].has_won());
        assert_eq!(
            sim_outcome(&runtime.simulation),
            Some(HouseOutcomeKind::Victory)
        );
    }

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
