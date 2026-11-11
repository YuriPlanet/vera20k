//! Application orchestration through eight explicit state owners.
//!
//! Platform, renderer, diagnostics, frontend, match, process assets, audio and
//! persistence each own their state and lifecycle. AppState coordinates these
//! owners; match-scoped input and presentation live inside MatchState.
//! The architecture guards keep unrelated flat fields from returning.

use super::{OverlayTypeRegistry, ResolvedTerrainGrid};

mod platform;

pub(crate) use platform::{CapturePresentation, PlatformState};

/// All initialized state. Created in `resumed()` when the window is available.
/// pub(crate) so the app presentation/render paths can access fields.
pub(crate) struct AppState {
    pub(crate) platform: PlatformState,
    /// Process-wide renderer owner (F12): GPU context, batch renderer,
    /// pools, passes, fonts, and rendering caches.
    pub(crate) renderer: crate::app::renderer_state::RendererState,
    /// Process diagnostics owner (F12): debug toggles, frame stepper,
    /// parity digest sink, dev-overlay bookkeeping.
    pub(crate) diag: crate::app::diagnostics::state::DiagnosticsState,
    /// Frontend owner (F12): shell/menu/score/dialog flow state.
    pub(crate) frontend: crate::app::frontend::state::FrontendState,
    /// Match owner (F12): everything scoped to the running (or last) match.
    pub(crate) match_state: crate::app::match_runtime::state::MatchState,
    /// Process-wide asset ownership (F11): the one retail MIX manager for
    /// the process, leased to the loading pipeline and always returned.
    pub(crate) process_assets: crate::app::process_assets::ProcessAssets,
    /// Process-wide audio owner (F12): players and registries.
    pub(crate) audio: crate::app::audio_runtime::AppAudioRuntime,
    /// Save repository, cached listing, and last save/load metadata.
    pub(crate) persistence: crate::app::persistence::PersistenceState,
}

/// Drop app-owned scenario-exit runtime and failure modal after a successful world replacement.
/// Serialized HouseState owns SavourDelay and pending flags; ScenarioSession
/// retains admitted requests. A saved frame cursor resumes its House wait
/// with a fresh process clock, without replaying the result announcement.
/// ClearScene685213..68521F and ordinary load cleanup69BB40 reset the shared
/// 87E758 output scalar, not Theme's slots/fading. ThemeLoad721040 must see
/// that process state unchanged; fresh-start request cancellation stays there.
pub(crate) fn reset_scenario_exit_runtime(state: &mut AppState) {
    state.match_state.scenario_outcome = None;
    state.match_state.scenario_exit = None;
    state.match_state.network_failure = None;
    state.match_state.frame_stopped = false;
    crate::app::lan_service::return_transport(state);
    state.audio.restore_sound_master();
}

/// The correction matrix for `[VERA] ColorVisionMode=`, or `None` for `Off`.
/// Startup needs it before an `AppState` exists, so the mapping lives here
/// rather than on the state: `render/` only ever sees a plain matrix.
pub(crate) fn color_vision_matrix_for(
    mode: crate::ui::main_menu_dialogs::options::ColorVisionMode,
) -> Option<crate::render::color_vision::Mat3> {
    use crate::render::color_vision as cv;
    use crate::ui::main_menu_dialogs::options::ColorVisionMode as Mode;
    match mode {
        Mode::Off => None,
        Mode::Generic | Mode::Deuteranopia => Some(cv::CORRECT_DEUTERANOPIA),
        Mode::Protanopia => Some(cv::CORRECT_PROTANOPIA),
        Mode::Tritanopia => Some(cv::CORRECT_TRITANOPIA),
    }
}

impl AppState {
    /// Client presentation preference; the simulation never reads this option.
    pub(crate) fn translucent_fog_enabled(&self) -> bool {
        self.platform
            .game_config
            .as_ref()
            .is_some_and(|config| config.graphics.translucent_fog)
    }

    /// `[VERA] HealthBars=`: the client health-strip policy. Presentation only;
    /// the simulation never reads it.
    pub(crate) fn health_bar_mode(&self) -> crate::ui::main_menu_dialogs::options::HealthBarMode {
        self.persistence.options_profile.health_bars
    }

    /// `[VERA] ColorVisionMode=`: the final-frame colour filter. Presentation
    /// only; the simulation never reads it, and the frame reads it live.
    pub(crate) fn color_vision_mode(
        &self,
    ) -> crate::ui::main_menu_dialogs::options::ColorVisionMode {
        self.persistence.options_profile.color_vision
    }

    /// Publish this frame's matrix to the shared frame-output filter.
    pub(crate) fn color_vision_matrix(&self) -> Option<crate::render::color_vision::Mat3> {
        color_vision_matrix_for(self.color_vision_mode())
    }

    /// Is the match frozen because the window is not the foreground? Only the
    /// configured freeze, and only offline: `Main_Tick` (`0x0055D38D`) sleeps
    /// until reactivated in GameMode 0 or 5, but a network game sleeps 10 ms
    /// and runs on, so peers never stall on a deactivated player.
    pub(crate) fn focus_freeze_active(&self) -> bool {
        self.platform.focus_freeze_configured()
            && self.match_state.session_mode().is_single_player()
    }

    /// The game folder map lookups start from: the configured one, else the
    /// asset manager's.
    pub(crate) fn ra2_dir(&self) -> Option<&std::path::Path> {
        self.platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.as_path())
            .or_else(|| self.process_assets.manager().map(|assets| assets.ra2_dir()))
    }

    /// Current projection: frontend/result use physical window pixels; tactical
    /// rendering and pending tactical installation use the optional zoom pass.
    /// Loading artwork explicitly uses GPU/window dimensions in loading::pump.
    fn render_dimensions(&self) -> (u32, u32) {
        platform::render_dimensions(
            &self.frontend.screen,
            (
                self.renderer.gpu.config.width,
                self.renderer.gpu.config.height,
            ),
            self.renderer
                .upscale_pass
                .as_ref()
                .map(|up| (up.src_width(), up.src_height())),
        )
    }

    pub(crate) fn render_width(&self) -> u32 {
        self.render_dimensions().0
    }

    pub(crate) fn render_height(&self) -> u32 {
        self.render_dimensions().1
    }

    /// The sidebar's place and size in the match frame (`ui::sidebar::frame`),
    /// drawn at the window's pixels when the match zooms.
    pub(crate) fn sidebar_frame(&self) -> crate::ui::sidebar::SidebarFrame {
        let zoom = self
            .match_zoom_pass()
            .map_or(100, |pass| pass.geometry().percent);
        self.sidebar_frame_for(self.render_width(), self.render_height())
            .in_window(zoom, self.hd_radar() || self.smooth_sidebar())
    }

    /// Whether VERA's HD radar is on (`[VERA] HdRadar=`); a capture takes its
    /// profile's choice (`CapturePresentation`).
    pub(crate) fn hd_radar(&self) -> bool {
        if self.platform.capture_client_size.is_some() {
            self.platform.capture_presentation.hd_radar
        } else {
            self.persistence.options_profile.hd_radar
        }
    }

    /// Whether VERA's smooth sidebar is on (`[VERA] SmoothSidebar=`); a
    /// capture takes its profile's choice (`CapturePresentation`).
    pub(crate) fn smooth_sidebar(&self) -> bool {
        if self.platform.capture_client_size.is_some() {
            self.platform.capture_presentation.smooth_sidebar
        } else {
            self.persistence.options_profile.smooth_sidebar
        }
    }

    /// Whether VERA's online client draws its enlarged art smoothed
    /// (`[VERA] SmoothOnlineMenus=`). The native-UI diagnostics, the only
    /// sessions without that client, keep their art's own pixels.
    pub(crate) fn smooth_online_menus(&self) -> bool {
        self.persistence.options_profile.smooth_online_menus && !self.frontend.native_shell_ui
    }

    /// Whether the setup screens show VERA's own map pictures
    /// (`[VERA] HdMapPreviews=`, `app::map_picture`). Captures keep the map's
    /// `[PreviewPack]` picture, which retail comparisons check, and the
    /// browser build cannot start the hidden run that draws one.
    pub(crate) fn hd_map_previews(&self) -> bool {
        crate::ui::main_menu_dialogs::options::HAS_HD_MAP_PREVIEWS
            && self.platform.capture_client_size.is_none()
            && self.persistence.options_profile.hd_map_previews
    }

    /// The same for a frame of `width` x `height`. Captures take their own
    /// size (`CapturePresentation`), Normal unless the profile says.
    pub(crate) fn sidebar_frame_for(
        &self,
        width: u32,
        height: u32,
    ) -> crate::ui::sidebar::SidebarFrame {
        let size = if self.platform.capture_client_size.is_some() {
            self.platform.capture_presentation.sidebar
        } else {
            self.persistence.options_profile.sidebar
        };
        // The side's layout decides how wide the column is and where it and
        // the bar sit.
        let spec = self.match_state.match_presentation.sidebar_layout_spec;
        crate::ui::sidebar::SidebarFrame::new(width, height, size.percent(width, height))
            .with_column_width(spec.sidebar_width as u32)
            .with_band_height(spec.side_height as u32)
            .with_places(spec.places)
    }

    /// The zoom pass a match frame goes through: the battlefield and the
    /// active-game shells draw into it at the source size. Loading and the
    /// menus never zoom.
    pub(crate) fn match_zoom_pass(&self) -> Option<&crate::render::upscale_pass::UpscalePass> {
        (self.frontend.screen == crate::ui::game_screen::GameScreen::InGame)
            .then_some(self.renderer.upscale_pass.as_ref())
            .flatten()
    }

    /// The zoom between the window and the match frame, whose cursor, layout
    /// and drawing all use source pixels.
    pub(crate) fn tactical_zoom(&self) -> Option<crate::render::upscale_pass::ZoomGeometry> {
        self.match_zoom_pass().map(|pass| pass.geometry())
    }

    /// Store a pointer position given in window pixels, in the space this
    /// screen reads it in.
    pub(crate) fn set_cursor_from_window(&mut self, x: f32, y: f32) {
        let zoom = self.tactical_zoom();
        self.match_state.input.set_cursor_from_window(
            (x, y),
            zoom,
            self.platform.pointer_held.get() && self.platform.window_active,
        );
    }

    /// Move the stored cursor into the space the current screen reads it in,
    /// before each window event: leaving a zoomed match or resizing one would
    /// otherwise leave it in the old space until the pointer moves.
    pub(crate) fn sync_cursor_space(&mut self) {
        let zoom = self.tactical_zoom();
        self.match_state.input.sync_cursor_space(zoom);
    }

    /// The pointer in the pixels the current screen paints and hit-tests in:
    /// the match frame's during a match (source pixels when zoomed), the
    /// window's elsewhere.
    pub(crate) fn cursor_position(&self) -> (f32, f32) {
        (
            self.match_state.input.cursor_x,
            self.match_state.input.cursor_y,
        )
    }

    /// The status cards paint in window pixels, even over a zoomed match.
    pub(crate) fn window_cursor_position(&self) -> (f32, f32) {
        let (x, y) = (
            self.match_state.input.cursor_x,
            self.match_state.input.cursor_y,
        );
        self.tactical_zoom()
            .map_or((x, y), |zoom| zoom.source_to_window(x, y))
    }

    /// Whether the software cursor (mouse.shp) should be active this frame.
    /// Asset-independent status cards and diagnostic modals use the OS cursor,
    /// and so does the pointer once it leaves the client area: mouse.shp has no
    /// art for the title bar or resize border, and the OS arrow is drawn there
    /// whether VERA asks for it or not. The same holds over an inactive window. Main-menu dialogs (Options, Keyboard,
    /// the exit confirm) are shell pages that draw mouse.shp themselves, so
    /// they keep it; excluding them showed the OS arrow beside it.
    pub(crate) fn use_software_cursor(&self) -> bool {
        self.platform.owns_pointer()
            && !(self.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
                && self.frontend.main_menu_shell_error.is_some())
            && self
                .match_state
                .match_presentation
                .software_cursor
                .is_some()
            && (!self.match_state.paused()
                || crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(self)
                || self
                    .match_state
                    .match_presentation
                    .in_game_menu
                    .draws_over_battlefield())
    }

    /// The main menu at rest, where a box may ask or tell the player
    /// something: its page takes input (not sliding, no movie, page or error
    /// over it), past the splash, with no dialog.
    pub(crate) fn main_menu_at_rest(&self) -> bool {
        use crate::app::input::precedence::{InputLayer, ScreenPage, input_layer};
        input_layer(self) == InputLayer::Screen(ScreenPage::MainMenu)
            && self.frontend.shell_route == crate::app::shell_route::ShellRoute::MainMenu
            && self.frontend.startup_splash.is_none()
            && !self.main_menu_dialog_open()
    }

    /// Whether any main-menu modal dialog (exit confirm, options, keyboard)
    /// is currently open.
    pub(crate) fn main_menu_dialog_open(&self) -> bool {
        self.frontend.shell_message_box.is_some()
            || self.frontend.options_dialog.is_some()
            || self.frontend.keyboard_dialog.is_some()
    }

    /// Return the building-placement section name if the targeting mode
    /// is set to `BuildingPlacement`, else `None`.
    pub(crate) fn armed_building_type(&self) -> Option<&str> {
        self.match_state
            .input
            .targeting_mode
            .as_ref()
            .and_then(crate::app::types::TargetingMode::as_building_placement)
    }

    /// Return the SW section name if the targeting mode is set to
    /// `SuperWeapon`, else `None`.
    pub(crate) fn armed_super_weapon_type(&self) -> Option<&str> {
        self.match_state
            .input
            .targeting_mode
            .as_ref()
            .and_then(crate::app::types::TargetingMode::as_super_weapon)
    }
}

impl AppState {
    /// Immutable view of the running simulation (F10): the read boundary
    /// presentation cones consume. `None` outside a match. Sites that also
    /// hold `&mut` app fields keep the `sim_runtime` field chain and call
    /// `rt.view()` directly for split borrows.
    pub(crate) fn sim_view(&self) -> Option<crate::sim::runtime::SimView<'_>> {
        self.match_state.sim_runtime.runtime().map(|rt| rt.view())
    }

    /// Current terrain authority, replaced together with the running world on
    /// map load and restore. No presentation height projection is retained.
    pub(crate) fn resolved_terrain(
        &self,
    ) -> Option<&crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.match_state
            .sim_runtime
            .runtime()?
            .simulation
            .resolved_terrain()
    }
}

impl AppState {
    /// The overlay registry: runtime-bound during a match, shell-retained
    /// (last loaded) otherwise — exactly the old field's lifecycle.
    pub(crate) fn overlay_registry(&self) -> Option<&OverlayTypeRegistry> {
        self.match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.resources.overlay_registry)
            .or(self.frontend.shell_preview_overlay_registry.as_ref())
    }
}

impl AppState {
    /// The active rules: runtime-bound during a match, startup-shell rules
    /// otherwise. Matches the old field's Option shape at every consumer.
    pub(crate) fn rules(&self) -> Option<&crate::rules::ruleset::RuleSet> {
        self.match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.resources.rules)
            .or(self.frontend.frontend_rules.as_ref())
    }

    /// The colours a lobby lists under the loaded rules; YR's eight before
    /// any rules load.
    pub(crate) fn lobby_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        self.rules()
            .map(crate::rules::ruleset::RuleSet::skirmish_colors)
            .unwrap_or_default()
    }
}

impl AppState {
    /// The immutable base resolved-terrain template for the active match
    /// (static rendering + restore); never the live sim grid.
    pub(crate) fn terrain_template(&self) -> Option<&ResolvedTerrainGrid> {
        self.match_state
            .sim_runtime
            .runtime()
            .and_then(|rt| rt.resources.terrain_template.as_ref())
    }
}

/// Main886B88 follows the currently installed scenario even while its world
/// is retained behind the shell. Frontend retains the process cell; a fresh
/// install moves in the already-advanced bootstrap cursor and binds the world
/// to it. In-scenario Load preserves that cell, including outstanding draws.
/// There is never a separately seeded audio/Theme cursor.
pub(crate) fn process_main_draws(
    runtime: Option<&crate::sim::runtime::SimRuntime>,
    frontend: &crate::sim::rng::MainRng,
) -> crate::sim::rng::MainRngDraws {
    match runtime {
        Some(runtime) => runtime.simulation.presentation_main_draws(),
        None => frontend.draws(),
    }
}

#[cfg(test)]
mod main_draw_tests {
    use super::process_main_draws;
    use crate::sim::{
        rng::{MainRng, SimRng},
        runtime::SimRuntime,
        world::Simulation,
    };

    #[test]
    fn process_main_draws_uses_frontend_only_before_a_world_is_installed() {
        let frontend = MainRng::new(31);
        let mut reference = SimRng::new(31);
        {
            let main = process_main_draws(None, &frontend);
            assert_eq!(main.next_u32(), reference.next_u32());
            assert_eq!(
                main.ranged(0, 99),
                reference.next_range_i32_inclusive(0, 99)
            );
        }
        assert_eq!(frontend.logical_state(), reference.logical_state());

        let runtime = SimRuntime::from_simulation(Simulation::with_seed(42));
        let frontend_before = frontend.logical_state();
        let world_before = runtime.simulation.rng_state();
        let mut world_reference = SimRng::new(42);
        // The installed world remains authoritative when retained behind the
        // shell. There is deliberately no screen/active-frame switch here.
        for _ in 0..2 {
            let main = process_main_draws(Some(&runtime), &frontend);
            assert_eq!(main.next_u32(), world_reference.next_u32());
        }
        let world_after = runtime.simulation.rng_state();
        assert_eq!(world_after.main, world_reference.logical_state());
        assert_eq!(world_after.scenario, world_before.scenario);
        assert_eq!(world_after.mapgen, world_before.mapgen);
        assert_eq!(frontend.logical_state(), frontend_before);
    }

    #[test]
    fn fresh_scenario_install_moves_bootstrap_into_retained_process_main() {
        let frontend = MainRng::new(31);
        let retained = frontend.draws();
        let mut installed = Simulation::with_seed(42);
        let scenario = installed.rng_state().scenario;
        let mapgen = installed.rng_state().mapgen;
        let mut reference = SimRng::new(42);
        for _ in 0..7 {
            assert_eq!(installed.main_rng().next_u32(), reference.next_u32());
        }
        installed.install_fresh_process_main(&frontend);
        assert_eq!(frontend.logical_state(), reference.logical_state());
        assert_eq!(
            retained.ranged(0, 4),
            reference.next_range_i32_inclusive(0, 4)
        );
        assert_eq!(
            installed.presentation_main_draws().next_u32(),
            reference.next_u32()
        );
        assert_eq!(installed.rng_state().main, frontend.logical_state());
        assert_eq!(installed.rng_state().scenario, scenario);
        assert_eq!(installed.rng_state().mapgen, mapgen);

        // A later fresh scenario reuses that same process cell again.
        let mut replacement = Simulation::with_seed(99);
        let mut next_reference = SimRng::new(99);
        assert_eq!(replacement.main_rng().next_u32(), next_reference.next_u32());
        replacement.install_fresh_process_main(&frontend);
        drop(installed);
        assert_eq!(retained.next_u32(), next_reference.next_u32());
        assert_eq!(replacement.rng_state().main, next_reference.logical_state());
    }
}
