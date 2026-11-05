//! The hidden `--map-picture` run. It loads one map the way the
//! `RA2_QUICKPLAY` sandbox does (a Battle with one house and no starting
//! forces), with the shroud and fog off, never advances it, and moves the
//! camera across the picture's rectangle, keeping one production frame at
//! each place. The frames' battlefields are shrunk into the picture
//! ([`super::canvas`]), which is written as a PNG. Nothing here touches
//! simulation state; the process ends with the picture.

use anyhow::{Context, Result, bail, ensure};

use super::MapPictureRequest;
use super::canvas::{Canvas, Tile, WorldRect, tiles};
use crate::app::AppState;
use crate::app::presentation::render::GameRenderOutput;
use crate::ui::game_screen::GameScreen;
use crate::util::time::{Duration, Instant};

/// The hidden window's frame. 2048 is the largest edge every GPU allows
/// (wgpu's downlevel limit); the battlefield is this less the sidebar.
const FRAME: (u32, u32) = (2048, 2048);
/// How long a picture may take before the run gives up.
const TIMEOUT: Duration = Duration::from_secs(120);
/// Frames drawn at each camera place before the one kept, so the picture
/// never depends on work a frame starts, such as sprites first seen there
/// (`publish_entity_atlas_growth`).
const SETTLE_FRAMES: u32 = 2;
const PUMP_INTERVAL: Duration = Duration::from_millis(1);

struct Plan {
    tiles: Vec<Tile>,
    canvas: Canvas,
    /// The battlefield's size in each frame (`tactical_viewport_size`).
    battlefield: (u32, u32),
}

/// A map-picture run's state across the event loop's pumps.
pub(crate) struct MapPictureSession {
    request: MapPictureRequest,
    started_at: Instant,
    /// `[Header]`'s preview rectangle, when the map has one.
    header: Option<WorldRect>,
    settle_frames: u32,
    plan: Option<Plan>,
    next_tile: usize,
    readback_armed: bool,
    outcome: Option<std::result::Result<(), String>>,
}

impl MapPictureSession {
    pub(crate) fn new(request: MapPictureRequest) -> Self {
        Self {
            request,
            started_at: Instant::now(),
            header: None,
            settle_frames: 0,
            plan: None,
            next_tile: 0,
            readback_armed: false,
            outcome: None,
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        FRAME
    }

    /// Check the map is the one asked for, then start loading it.
    pub(crate) fn prepare_state(&mut self, state: &mut AppState) -> Result<()> {
        let config = state
            .platform
            .game_config
            .as_ref()
            .context("VERA20k has no game folder")?;
        let assets = state
            .process_assets
            .manager()
            .context("the game's archives are not loaded")?;
        let loaded = crate::map::source::load_map_ini_by_name_or_path_with_assets(
            &config.paths.ra2_dir,
            &self.request.map,
            assets,
        )?;
        let (_, sha256) = loaded
            .source
            .physical_identity()
            .context("the map is not a file")?;
        ensure!(
            sha256.eq_ignore_ascii_case(&self.request.sha256),
            "{} changed since its picture was asked for",
            self.request.map
        );
        self.header =
            crate::map::scenario_menu::preview_source_bounds_from_verified_source(&loaded.ini)
                .as_ref()
                .and_then(WorldRect::of_preview_bounds);

        let mut session = crate::app::initialize::quickplay_launch_session(
            self.request.map.clone(),
            &state.frontend.skirmish_modes,
        )
        .context("the game has no Battle mode")?;
        session.options.shroud = false;
        session.options.fog_of_war = false;
        #[cfg(feature = "ares")]
        crate::app::frontend::skirmish_session::prepare_external_skirmish_countries(
            &mut state.process_assets,
            &session,
        );
        // A fixed seed, so a map always gets the same picture.
        let seed = crate::app::match_bootstrap::MatchSeed {
            value: 0,
            source: crate::app::match_bootstrap::MatchSeedSource::Controlled,
            seed_authority_certifying: false,
        };
        state.platform.capture_presentation.map_picture = true;
        crate::app::loading::pump::begin_loading(
            state,
            crate::app::loading::pump::LoadingRequest::unverified_legacy_skirmish(session, seed),
        );
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        Ok(())
    }

    /// Wait for the battle, then put the camera where the next frame goes.
    pub(crate) fn drive_before_render(&mut self, state: &mut AppState) -> Result<()> {
        ensure!(self.outcome.is_none(), "the map picture already ended");
        ensure!(
            self.started_at.elapsed() < TIMEOUT,
            "the map picture took longer than {} seconds",
            TIMEOUT.as_secs()
        );
        match &state.frontend.screen {
            GameScreen::Loading => return Ok(()),
            GameScreen::InGame => {}
            GameScreen::MissionResult { title, detail } => bail!("{title}: {detail}"),
            _ => bail!("{} did not load", self.request.map),
        }
        if self.plan.is_none() {
            self.plan = Some(self.plan(state)?);
            // The battle starts with the cursor at the battlefield's centre
            // (`loading::transitions`); it rests on the sidebar instead,
            // outside every frame's battlefield.
            state.match_state.input.cursor_x = (FRAME.0 - 80) as f32;
            state.match_state.input.cursor_y = (FRAME.1 - 44) as f32;
        }
        let plan = self.plan.as_ref().expect("planned above");
        let camera = plan.tiles[self.next_tile].camera;
        let (width, height) = (state.render_width() as f32, state.render_height() as f32);
        crate::app::input::camera::set_camera_position(
            &mut state.match_state.input,
            (camera.0 as f32, camera.1 as f32),
        );
        crate::app::input::camera::clamp_camera_to_playable_area(state, width, height);
        Ok(())
    }

    /// The frames that cover `[Header]`'s rectangle, or all the camera can
    /// show on a map without one.
    fn plan(&self, state: &AppState) -> Result<Plan> {
        let view = crate::app::input::camera::tactical_view_area(state)
            .and_then(WorldRect::of_view_area)
            .context("the loaded map has no visible area")?;
        let area = self.header.unwrap_or(view);
        let battlefield = crate::app::input::camera::tactical_viewport_size(state);
        let tiles = tiles(area, view, battlefield);
        ensure!(
            !tiles.is_empty(),
            "[Header]'s preview rectangle is outside the visible map"
        );
        let canvas = Canvas::new(area).context("[Header]'s preview rectangle is empty")?;
        log::info!(
            "Drawing {} in {} frames: {area:?}, visible {view:?}",
            self.request.map,
            tiles.len()
        );
        Ok(Plan {
            tiles,
            canvas,
            battlefield,
        })
    }

    /// Keep this frame once it has settled at its place and draws whole.
    pub(crate) fn observe_after_render(
        &mut self,
        state: &AppState,
        output: &GameRenderOutput,
    ) -> Result<bool> {
        if self.plan.is_none() || state.frontend.screen != GameScreen::InGame {
            return Ok(false);
        }
        if self.settle_frames < SETTLE_FRAMES {
            self.settle_frames += 1;
            return Ok(false);
        }
        self.readback_armed = output.sidebar_view.is_some()
            && !state.match_state.paused()
            && !state.main_menu_dialog_open();
        Ok(self.readback_armed)
    }

    pub(crate) fn readback_timeout(&self) -> Result<Duration> {
        TIMEOUT
            .checked_sub(self.started_at.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .context("the map picture ran out of time")
    }

    /// Add the kept frame; after the last one write the picture. True once
    /// the run is over.
    pub(crate) fn complete_after_readback(
        &mut self,
        state: &AppState,
        format: wgpu::TextureFormat,
        pixels: &[u8],
    ) -> Result<bool> {
        ensure!(
            std::mem::take(&mut self.readback_armed),
            "a frame arrived that was not asked for"
        );
        ensure!(
            matches!(
                format,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
            "the frame is {format:?}, not BGRA8"
        );
        let plan = self.plan.as_mut().context("no plan for the frame")?;
        // The renderer draws the world at the rounded camera (`update_camera`).
        let input = &state.match_state.input;
        let camera = (input.camera_x.round() as i32, input.camera_y.round() as i32);
        plan.canvas.add(
            plan.tiles[self.next_tile].fills,
            camera,
            pixels,
            state.renderer.gpu.config.width,
            plan.battlefield,
        );
        self.next_tile += 1;
        self.settle_frames = 0;
        if self.next_tile < plan.tiles.len() {
            return Ok(false);
        }
        let (width, height, rgb) = self.plan.take().expect("planned").canvas.finish();
        super::write_png(&self.request.output, width, height, &rgb)?;
        log::info!(
            "Wrote the {width}x{height} picture of {} in {:.1}s",
            self.request.map,
            self.started_at.elapsed().as_secs_f32()
        );
        self.outcome = Some(Ok(()));
        Ok(true)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.outcome.is_some()
    }

    pub(crate) fn next_wake_deadline(&self) -> Instant {
        Instant::now() + PUMP_INTERVAL
    }

    pub(crate) fn fail(&mut self, error: impl std::fmt::Display) {
        if self.outcome.is_none() {
            let error = error.to_string();
            log::error!("Map picture of {} failed: {error}", self.request.map);
            self.outcome = Some(Err(error));
        }
    }

    pub(crate) fn take_outcome(&mut self) -> Result<()> {
        match self.outcome.take() {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => bail!("{error}"),
            None => bail!("the map picture ended before it was written"),
        }
    }
}
