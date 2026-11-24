//! Application orchestration through eight explicit state owners.
//!
//! Platform, renderer, diagnostics, frontend, match, process assets, audio and
//! persistence each own their state and lifecycle. AppState coordinates these
//! owners; match-scoped input and presentation live inside MatchState.
//! The architecture guards keep unrelated flat fields from returning.

use super::{OverlayTypeRegistry, ResolvedTerrainGrid};
use crate::app::shell_route::ShellRoute;

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
    state
        .match_state
        .match_presentation
        .outcome_banner_presentation = Default::default();
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
    pub(crate) fn translucent_fog_enabled(&self) -> bool {
        self.platform.translucent_fog_enabled()
    }

    pub(crate) fn health_bar_mode(&self) -> crate::ui::main_menu_dialogs::options::HealthBarMode {
        self.persistence.health_bar_mode()
    }

    pub(crate) fn color_vision_matrix(&self) -> Option<crate::render::color_vision::Mat3> {
        self.persistence.color_vision_matrix()
    }

    pub(crate) fn focus_freeze_active(&self) -> bool {
        focus_freeze_active(&self.platform, &self.match_state)
    }

    pub(crate) fn ra2_dir(&self) -> Option<&std::path::Path> {
        ra2_dir(&self.platform, &self.process_assets)
    }

    pub(crate) fn render_width(&self) -> u32 {
        self.renderer.render_width(&self.frontend.screen)
    }

    pub(crate) fn render_height(&self) -> u32 {
        self.renderer.render_height(&self.frontend.screen)
    }

    pub(crate) fn sidebar_frame(&self) -> crate::ui::sidebar::SidebarFrame {
        sidebar_frame(
            &self.platform,
            &self.persistence,
            &self.renderer,
            &self.frontend.screen,
            &self.match_state.match_presentation.sidebar_layout_spec,
        )
    }

    pub(crate) fn hd_radar(&self) -> bool {
        self.persistence.hd_radar(self.platform.capture_choices())
    }

    pub(crate) fn smooth_sidebar(&self) -> bool {
        self.persistence
            .smooth_sidebar(self.platform.capture_choices())
    }

    pub(crate) fn smooth_online_menus(&self) -> bool {
        self.persistence
            .smooth_online_menus(self.frontend.native_shell_ui)
    }

    pub(crate) fn hd_map_previews(&self) -> bool {
        self.persistence
            .hd_map_previews(self.platform.capture_choices())
    }

    pub(crate) fn sidebar_frame_for(
        &self,
        width: u32,
        height: u32,
    ) -> crate::ui::sidebar::SidebarFrame {
        sidebar_frame_for(
            &self.platform,
            &self.persistence,
            &self.match_state.match_presentation.sidebar_layout_spec,
            width,
            height,
        )
    }

    pub(crate) fn match_zoom_pass(&self) -> Option<&crate::render::upscale_pass::UpscalePass> {
        self.renderer.match_zoom_pass(&self.frontend.screen)
    }

    pub(crate) fn tactical_zoom(
        &self,
    ) -> Option<crate::ui::main_menu_dialogs::options::ZoomGeometry> {
        self.renderer.tactical_zoom(&self.frontend.screen)
    }

    /// Store a pointer position given in window pixels, in the space this
    /// screen reads it in.
    pub(crate) fn set_cursor_from_window(&mut self, x: f32, y: f32) {
        let zoom = self.renderer.tactical_zoom(&self.frontend.screen);
        self.match_state.input.set_cursor_from_window(
            (x, y),
            zoom,
            self.platform.confines_pointer(),
        );
    }

    /// Move the stored cursor into the space the current screen reads it in,
    /// before each window event: leaving a zoomed match or resizing one would
    /// otherwise leave it in the old space until the pointer moves.
    pub(crate) fn sync_cursor_space(&mut self) {
        let zoom = self.renderer.tactical_zoom(&self.frontend.screen);
        self.match_state.input.sync_cursor_space(zoom);
    }

    pub(crate) fn cursor_position(&self) -> (f32, f32) {
        self.match_state.input.cursor_position()
    }

    pub(crate) fn window_cursor_position(&self) -> (f32, f32) {
        self.match_state
            .input
            .window_cursor_position(self.renderer.tactical_zoom(&self.frontend.screen))
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
            && self.frontend.shell_route == ShellRoute::MainMenu
            && self.frontend.startup_splash.is_none()
            && !self.frontend.main_menu_dialog_open()
    }

    pub(crate) fn main_menu_dialog_open(&self) -> bool {
        self.frontend.main_menu_dialog_open()
    }

    pub(crate) fn armed_building_type(&self) -> Option<&str> {
        self.match_state.input.armed_building_type()
    }

    pub(crate) fn armed_super_weapon_type(&self) -> Option<&str> {
        self.match_state.input.armed_super_weapon_type()
    }

    pub(crate) fn sim_view(&self) -> Option<crate::sim::runtime::SimView<'_>> {
        self.match_state.sim_view()
    }

    pub(crate) fn resolved_terrain(
        &self,
    ) -> Option<&crate::map::resolved_terrain::ResolvedTerrainGrid> {
        self.match_state.resolved_terrain()
    }

    pub(crate) fn overlay_registry(&self) -> Option<&OverlayTypeRegistry> {
        active_overlay_registry(&self.frontend, &self.match_state)
    }

    pub(crate) fn rules(&self) -> Option<&crate::rules::ruleset::RuleSet> {
        active_rules(&self.frontend, &self.match_state)
    }

    pub(crate) fn lobby_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        lobby_colors(&self.frontend, &self.match_state)
    }

    pub(crate) fn terrain_template(&self) -> Option<&ResolvedTerrainGrid> {
        self.match_state.terrain_template()
    }
}

/// Is the match frozen because the window is not the foreground? Only the
/// configured freeze, and only offline: `Main_Tick` (`0x0055D38D`) sleeps
/// until reactivated in GameMode 0 or 5, but a network game sleeps 10 ms
/// and runs on, so peers never stall on a deactivated player.
pub(crate) fn focus_freeze_active(
    platform: &PlatformState,
    match_state: &crate::app::match_runtime::state::MatchState,
) -> bool {
    platform.focus_freeze_configured() && match_state.session_mode().is_single_player()
}

/// The game folder map lookups start from: the configured one, else the
/// asset manager's.
pub(crate) fn ra2_dir<'a>(
    platform: &'a PlatformState,
    process_assets: &'a crate::app::process_assets::ProcessAssets,
) -> Option<&'a std::path::Path> {
    platform
        .game_config
        .as_ref()
        .map(|config| config.paths.ra2_dir.as_path())
        .or_else(|| process_assets.manager().map(|assets| assets.ra2_dir()))
}

/// The sidebar's place and size in the match frame (`ui::sidebar::frame`),
/// drawn at the window's pixels when the match zooms. `spec` is the side's
/// layout (`MatchPresentationState::sidebar_layout_spec`).
pub(crate) fn sidebar_frame(
    platform: &PlatformState,
    persistence: &crate::app::persistence::PersistenceState,
    renderer: &crate::app::renderer_state::RendererState,
    screen: &crate::ui::game_screen::GameScreen,
    spec: &crate::ui::sidebar::SidebarChromeLayoutSpec,
) -> crate::ui::sidebar::SidebarFrame {
    let zoom = renderer
        .match_zoom_pass(screen)
        .map_or(100, |pass| pass.geometry().percent);
    let capture = platform.capture_choices();
    let (width, height) = renderer.render_size(screen);
    sidebar_frame_for(platform, persistence, spec, width, height).in_window(
        zoom,
        persistence.hd_radar(capture) || persistence.smooth_sidebar(capture),
    )
}

/// [`sidebar_frame`] for a frame of `width` x `height`. Captures take their
/// own size (`CapturePresentation`), Normal unless the profile says.
pub(crate) fn sidebar_frame_for(
    platform: &PlatformState,
    persistence: &crate::app::persistence::PersistenceState,
    spec: &crate::ui::sidebar::SidebarChromeLayoutSpec,
    width: u32,
    height: u32,
) -> crate::ui::sidebar::SidebarFrame {
    let size = persistence.sidebar_size(platform.capture_choices());
    // The side's layout decides how wide the column is and where it and
    // the bar sit.
    crate::ui::sidebar::SidebarFrame::for_layout(width, height, size.percent(width, height), spec)
}

/// The overlay registry: runtime-bound during a match, shell-retained
/// (last loaded) otherwise — exactly the old field's lifecycle.
pub(crate) fn active_overlay_registry<'a>(
    frontend: &'a crate::app::frontend::state::FrontendState,
    match_state: &'a crate::app::match_runtime::state::MatchState,
) -> Option<&'a OverlayTypeRegistry> {
    match_state
        .sim_runtime
        .runtime()
        .map(|rt| rt.resources.rules.overlay_registry())
        .or(frontend.shell_preview_overlay_registry.as_deref())
}

/// The active rules: runtime-bound during a match, startup-shell rules
/// otherwise.
pub(crate) fn active_rules<'a>(
    frontend: &'a crate::app::frontend::state::FrontendState,
    match_state: &'a crate::app::match_runtime::state::MatchState,
) -> Option<&'a crate::rules::ruleset::RuleSet> {
    let route = frontend.shell_route;
    if frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
        && (matches!(route, ShellRoute::Wol | ShellRoute::WolLobby)
            || route == ShellRoute::LanLobby
            || route == ShellRoute::NetworkSetup)
    {
        // Network choices must agree before loading. Keep the old world's
        // Rules with its objects, but don't offer its map's colors or Types
        // in the next room. PrepareSession refreshed startup's Countries.
        return frontend.frontend_rules.as_ref();
    }
    match_state
        .sim_runtime
        .runtime()
        .map(|rt| &rt.resources.rules)
        .or(frontend.frontend_rules.as_ref())
}

/// The colours a lobby lists under the loaded rules; YR's eight before
/// any rules load.
pub(crate) fn lobby_colors(
    frontend: &crate::app::frontend::state::FrontendState,
    match_state: &crate::app::match_runtime::state::MatchState,
) -> crate::rules::skirmish_colors::SkirmishColorMode {
    active_rules(frontend, match_state)
        .map(crate::rules::ruleset::RuleSet::skirmish_colors)
        .unwrap_or_default()
}

/// The app's Main886B88 draws: voices, the theme and the other interface
/// draws. The app keeps its own process cell; a fresh install moves the
/// bootstrap cursor into it and the scenario's simulation continues from
/// there in a cell of its own (`Simulation::install_fresh_process_main`), so
/// the two threads share no RNG (docs/sim-thread.md).
pub(crate) fn process_main_draws(
    frontend: &crate::sim::rng::MainRng,
) -> crate::sim::rng::MainRngDraws {
    frontend.draws()
}

#[cfg(all(test, tests_app))]
mod main_draw_tests {
    use super::process_main_draws;
    use crate::sim::{
        rng::{MainRng, SimRng},
        world::Simulation,
    };

    #[test]
    fn an_installed_scenario_draws_its_own_main_from_the_installed_cursor() {
        let frontend = MainRng::new(31);
        let retained = frontend.draws();
        let mut installed = Simulation::with_seed(42);
        let scenario = installed.rng_state().scenario;
        let mut reference = SimRng::new(42);
        for _ in 0..7 {
            assert_eq!(
                installed.main_rng_mut_for_test().next_u32(),
                reference.next_u32()
            );
        }
        installed.install_fresh_process_main(&frontend);
        // The app's cell takes the bootstrap cursor ...
        assert_eq!(frontend.logical_state(), reference.logical_state());
        // ... and so does the simulation's, but they draw apart.
        assert_eq!(installed.rng_state().main, reference.logical_state());
        let mut world = reference.clone();
        assert_eq!(
            process_main_draws(&frontend).ranged(0, 4),
            reference.next_range_i32_inclusive(0, 4)
        );
        assert_eq!(
            installed.main_rng_mut_for_test().next_u32(),
            world.next_u32()
        );
        assert_eq!(retained.next_u32(), reference.next_u32());
        assert_eq!(installed.rng_state().main, world.logical_state());
        assert_eq!(installed.rng_state().scenario, scenario);
    }
}
