use super::*;
use crate::app::persistence::options::audio::AudioVolumeOperations;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MultiplayerSettingsContext {
    Options,
    Shell,
}

/// Private operation seam for the retail shell's confirmed quit transaction.
/// Keeping the ordering here lets unit tests observe persistence dispatch
/// without constructing the window, renderer, or audio-backed `AppState`.
trait ConfirmedQuitOperations {
    fn persist_settings(&mut self);
    fn dismiss_confirmation(&mut self);
    fn start_cascade(&mut self);
}

struct AppStateConfirmedQuitOperations<'a> {
    state: &'a mut AppState,
}

impl ConfirmedQuitOperations for AppStateConfirmedQuitOperations<'_> {
    fn persist_settings(&mut self) {
        App::persist_settings_on_quit(self.state);
    }

    fn dismiss_confirmation(&mut self) {
        App::close_shell_message_box_from_controller(self.state);
    }

    fn start_cascade(&mut self) {
        App::start_quit_cascade(self.state);
    }
}

fn dispatch_shell_controller_confirmed_quit(operations: &mut impl ConfirmedQuitOperations) {
    operations.dismiss_confirmation();
    operations.persist_settings();
    operations.start_cascade();
}

impl crate::app::persistence::options::launcher::LauncherPreviewOperations for AppState {
    fn play_cue(&mut self, cue: crate::ui::main_menu_dialogs::options::LauncherCue) {
        App::play_launcher_options_cue(self, cue);
    }
    fn store_zoom(&mut self, zoom: crate::ui::main_menu_dialogs::options::Zoom) {
        self.persistence.options_profile.zoom = zoom;
        App::sync_zoom_pass(self);
    }
    fn store_sidebar(&mut self, size: crate::ui::main_menu_dialogs::options::SidebarSize) {
        // The next battle frame lays out and draws the sidebar at this size.
        self.persistence.options_profile.sidebar = size;
    }
    fn set_display(&mut self, display: crate::ui::main_menu_dialogs::options::DisplayTarget) {
        // Applied at once, like Full Screen: the window moves to the picked
        // screen (or back to the system's choice) before the page is accepted.
        self.persistence.options_profile.display = display.clone();
        let displays = crate::app::display_target::current_displays(&self.platform.window);
        crate::app::display_target::apply(&self.platform.window, &displays, &display);
    }
    fn set_full_screen(&mut self, full_screen: bool) {
        App::set_full_screen(self, full_screen);
    }
    fn set_fps_counter(&mut self, shown: bool) {
        // Live state only; no profile stores it, so each launch starts hidden.
        self.renderer.fps_overlay.set_visible(shown);
    }
    fn store_original_shape(&mut self, original_shape: bool) {
        self.persistence.options_profile.original_shape = original_shape;
        App::sync_zoom_pass(self);
    }
    fn store_yr_original_style(&mut self, yr_original_style: bool) {
        // The menu movie's session identity carries the choice, so its next
        // paint loads the selected game copy; the splash follows next launch.
        // The Skirmish atlas carries the same profile value; it is rebuilt
        // (or built) now, before another shell screen can paint.
        self.persistence.options_profile.yr_original_style = yr_original_style;
        let _ = App::ensure_skirmish_shell_chrome(self);
    }
    fn store_shift_queue(&mut self, shift_queue: bool) {
        // The next cameo press reads it; Options' accept writes it out.
        self.persistence.options_profile.shift_queue = shift_queue;
    }
    fn store_render_interpolation(&mut self, enabled: bool) {
        self.persistence.options_profile.render_interpolation = enabled;
        self.match_state
            .match_presentation
            .interpolation
            .set_enabled(enabled);
    }
    fn store_mouse_wheel_zoom(&mut self, enabled: bool) {
        self.persistence.options_profile.mouse_wheel_zoom = enabled;
        if !enabled {
            crate::app::input::camera::reset_view_distance(self);
        }
    }
    fn store_wol_test_service(&mut self, wol_test_service: bool) {
        // The next Internet visit picks its provider from it.
        self.persistence.options_profile.wol_test_service = wol_test_service;
    }
}

struct AppStateLauncherParentOperations<'a> {
    state: &'a mut AppState,
}

impl crate::app::persistence::options::launcher::LauncherParentOperations
    for AppStateLauncherParentOperations<'_>
{
    fn observe_pack(
        &mut self,
        _packed: crate::ui::main_menu_dialogs::options::LauncherOptionsPacked,
    ) {
        debug_assert!(self.state.frontend.options_dialog.is_none());
    }

    fn store_detail_level(&mut self, value: i32) -> bool {
        let changed = self.state.persistence.options_profile.detail_level != value;
        self.state.persistence.options_profile.detail_level = value;
        self.state
            .match_state
            .match_presentation
            .in_game_options
            .detail_level = value.max(0) as u32;
        changed
    }

    fn refresh_detail(&mut self) {
        // Display4AE450 resamples synchronously, before the next Logic visit
        // can copy a Cell's light into a grapple anim. This local operation
        // never enters Simulation's replay/sync hash.
        if let Some(runtime) = self.state.match_state.sim_runtime.as_ref()
            && let Some(terrain) = runtime.simulation.resolved_terrain()
        {
            let presentation = &mut self.state.match_state.match_presentation;
            presentation.lighting.refresh(
                terrain,
                &runtime.simulation,
                presentation.in_game_options.detail_level,
            );
        }
        self.state.platform.window.request_redraw();
    }

    fn store_difficulty(&mut self, value: i32) {
        self.state.persistence.options_profile.difficulty = value;
    }

    fn store_unit_action_lines(&mut self, value: bool) {
        self.state.persistence.options_profile.unit_action_lines = value;
        self.state
            .match_state
            .match_presentation
            .in_game_options
            .unit_action_lines = value;
    }

    fn refresh_unit_action_lines(&mut self) {
        let enabled = self
            .state
            .match_state
            .match_presentation
            .in_game_options
            .unit_action_lines;
        self.state
            .match_state
            .match_presentation
            .target_lines
            .set_unit_action_lines_enabled(enabled);
    }

    fn store_show_hidden(&mut self, value: bool) {
        self.state.persistence.options_profile.show_hidden = value;
        self.state
            .match_state
            .match_presentation
            .in_game_options
            .show_hidden = value;
    }

    fn store_tooltips(&mut self, value: bool) {
        self.state.persistence.options_profile.tooltips = value;
        self.state
            .match_state
            .match_presentation
            .in_game_options
            .tooltips = value;
        self.state
            .match_state
            .match_presentation
            .tooltips
            .set_enabled(value);
    }

    fn store_scroll_rate(&mut self, value: i32) {
        self.state.persistence.options_profile.scroll_rate = value;
        self.state
            .match_state
            .match_presentation
            .in_game_options
            .scroll_rate = value.max(0) as u32;
    }

    fn store_score_volume(&mut self, value: f32) {
        AudioVolumeOperations::store_score_volume(self.state, value);
    }

    fn apply_score_output(&mut self, value: f32) {
        AudioVolumeOperations::apply_score_output(self.state, value);
    }

    fn queue_then_stop_score_zero(&mut self) {
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        self.state.audio.queue_then_stop_score_zero(now_ms);
    }

    fn store_sound_volume(&mut self, value: f32) {
        AudioVolumeOperations::store_sound_volume(self.state, value);
    }

    fn apply_sound_output(&mut self, value: f32) {
        AudioVolumeOperations::apply_sound_output(self.state, value);
    }

    fn store_voice_volume(&mut self, value: f32) {
        AudioVolumeOperations::store_voice_volume(self.state, value);
    }

    fn apply_voice_output(&mut self, value: f32) {
        AudioVolumeOperations::apply_voice_output(self.state, value);
    }

    fn primary_dropped(&mut self) {
        debug_assert!(self.state.frontend.options_dialog.is_none());
    }

    fn persist_profile(&mut self) {
        crate::app::persistence::options::persist_options_profile(self.state);
    }

    fn prepare_network_settings(&mut self) {
        App::prepare_multiplayer_settings(self.state, MultiplayerSettingsContext::Options);
    }

    fn route_network(&mut self) {
        crate::app::input::network_options::open(self.state, &[]);
    }

    fn route_keyboard(&mut self) {
        crate::app::input::keyboard::open(
            self.state,
            crate::ui::shell::keyboard::KeyboardParent::Launcher,
        );
    }

    fn reopen_parent(&mut self) {
        // Native caller blocks in A3/D7. Our asynchronous child resumes this
        // continuation when it closes (after its teardown slide).
        if self.state.frontend.keyboard_dialog.is_none()
            && self.state.frontend.network_options.is_none()
        {
            App::open_launcher_options_dialog(self.state);
        }
    }
}

/// The options profile's `[Video]` bytes behind HIRES and TOGGLE, which the
/// native writer never saves. VERA's Options page has no resolution list to
/// read AllowHiResModes (`0x0056017A..0x005601FA`), and AllowModeToggle is
/// read only at startup, so neither code changes anything else this session.
fn typed_code_video_flags(
    profile: &mut crate::app::persistence::options_profile::RetailOptionsProfile,
) -> crate::ui::main_menu_shell::VideoCodeFlags<'_> {
    crate::ui::main_menu_shell::VideoCodeFlags {
        allow_hi_res_modes: &mut profile.allow_hi_res_modes,
        allow_mode_toggle: &mut profile.allow_mode_toggle,
    }
}

/// `Main__PrepareSession` before a LAN or WOL session.
pub(crate) fn clear_typed_codes_for_network_session(state: &mut AppState) {
    state
        .frontend
        .main_menu_shell_state
        .typed_codes
        .clear_for_network_session(typed_code_video_flags(
            &mut state.persistence.options_profile,
        ));
}

/// Reduce a startup error to text the status card can draw.
///
/// The card's in-repo face carries ASCII only, while `std::io::Error` is
/// localized: on a Chinese Windows the detail line would otherwise be a row of
/// boxes. Dropping the inaccessible characters keeps the error code and the
/// asset-manager context, and the log still holds the full original message.
fn display_error(error: &str) -> String {
    let mut ascii = String::with_capacity(error.len());
    let mut pending_space = false;
    for ch in error.chars() {
        if ch.is_ascii() {
            if ch.is_whitespace() {
                pending_space = true;
                continue;
            }
            if pending_space && !ascii.is_empty() {
                ascii.push(' ');
            }
            pending_space = false;
            ascii.push(ch);
        } else {
            // The run becomes a word boundary rather than a hole.
            pending_space = true;
        }
    }
    if ascii.trim().chars().count() < 12 {
        "could not open the game files (see the log for the exact error)".to_owned()
    } else {
        ascii
    }
}

impl App {
    /// Shared pre-dialog Session read: D7 at55FD31, Skirmish at52E11E,
    /// and LAN (including failed IPX admission) at52E37D. Preferences stay
    /// in their process caches; the original reads INI8870C0, not disk.
    pub(crate) fn prepare_multiplayer_settings(
        state: &mut AppState,
        context: MultiplayerSettingsContext,
    ) {
        //6980D6→6722F0 and475540 read the same process INI each visit and
        // may allocate a Country before the optional52E123 body pass.
        state
            .frontend
            .multiplayer_shell_runtime
            .read_multiplayer_settings(&mut state.process_assets);
        let main = crate::app::state::process_main_draws(
            state.match_state.sim_runtime.as_mut(),
            &mut state.frontend.frontend_main_rng,
        );
        state
            .match_state
            .session
            .prepare_multiplayer_settings(&main);
        if context == MultiplayerSettingsContext::Shell {
            state.process_assets.refresh_multiplayer_country_bodies();
        }
        // These are compatibility projections of the one process registry.
        // A finished world can remain resident behind the shell, and rules()
        // reads its resource projection, so refresh both consumers together.
        if let Some(rules) = state.frontend.frontend_rules.as_mut() {
            state.process_assets.project_multiplayer_countries(rules);
        }
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
            state
                .process_assets
                .project_multiplayer_countries(&mut runtime.resources.rules);
        }
        if let Some(rules) = state.frontend.frontend_rules.as_ref() {
            state
                .frontend
                .skirmish_shell_state
                .refresh_country_catalog(rules);
        } else if let Some(runtime) = state.match_state.sim_runtime.as_ref() {
            state
                .frontend
                .skirmish_shell_state
                .refresh_country_catalog(&runtime.resources.rules);
        }
        state
            .frontend
            .multiplayer_shell_runtime
            .refresh_multiplayer_country_catalog(&state.process_assets);
    }

    /// A right-panel menu page (Single Player `0x100` or Movies & Credits
    /// `0x101`) owns the `MainMenu` screen.
    pub(super) fn menu_page_active(state: &AppState) -> bool {
        crate::app::frontend::menu_page_render::ActiveMenuPage::from_state(state).is_some()
    }

    /// The score page `0x108` owns the result screen once its model is set
    /// and its art and the shell chrome exist; otherwise the result screen
    /// uses the asset-independent status card when retail score art is absent.
    pub(super) fn score_shell_active(state: &AppState) -> bool {
        matches!(state.frontend.screen, GameScreen::MissionResult { .. })
            && state.frontend.score_page.is_some()
            && state.frontend.score_art.is_some()
            && state.frontend.main_menu_shell_chrome.is_some()
    }

    /// Campaign victories own the native StandaloneScore dialog `0x103`.
    /// This page has no Skirmish score-art dependency.
    pub(super) fn campaign_score_shell_active(state: &AppState) -> bool {
        matches!(state.frontend.screen, GameScreen::MissionResult { .. })
            && state.frontend.campaign_score_page.is_some()
            && state.frontend.main_menu_shell_chrome.is_some()
    }

    /// `ScoreDialog__Run` `0x005C9720` after a skirmish win or loss: the side
    /// art (`ScoreArt__Load` `0x0072D730`), then dialog `0x108` with every
    /// text its init (`0x497`) sets. The exit cascade has already restored the
    /// shell size and started SCORE.
    pub(crate) fn open_score_page(
        state: &mut AppState,
        model: crate::ui::score_shell::ScoreScreenModel,
        title: String,
        detail: String,
    ) {
        use crate::app::frontend::shell_pass::resolve_csf;
        use crate::ui::score_shell::{ScorePage, ScoreTexts, format_elapsed, format_game_number};
        let texts = ScoreTexts {
            game: format_game_number(&model, &resolve_csf(state, "TXT_GAME")),
            time: format_elapsed(&model, &resolve_csf(state, "TXT_TIME_FORMAT_HOURS")),
            headers: [
                "GUI:Player",
                "GUI:Kills",
                "GUI:Losses",
                "GUI:Built",
                "GUI:Score",
            ]
            .map(|key| resolve_csf(state, key).into_owned()),
        };
        state.frontend.score_art = state.process_assets.manager().and_then(|assets| {
            crate::render::main_menu_shell_chrome::build_score_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                model.side,
                crate::app::frontend::shell_layout::art_key_screen_width(state),
                state.process_assets.side_score_art(model.side),
            )
        });
        state.frontend.campaign_score_page = None;
        state.frontend.campaign_score_art = None;
        if state.frontend.score_art.is_none() {
            log::warn!("Could not prepare the score art; the result screen falls back");
        }
        state.frontend.score_page = Some(ScorePage::open(model, &texts, Instant::now()));
        state.frontend.screen = GameScreen::MissionResult { title, detail };
        state
            .frontend
            .shell_controller
            .reset_to(crate::ui::score_shell::SCORE_DIALOG, false);
    }

    /// Open retail Campaign StandaloneScore dialog `0x103` from the app-owned
    /// projection of the campaign result fields.
    pub(crate) fn open_campaign_score_page(
        state: &mut AppState,
        model: crate::ui::campaign_score_shell::CampaignScoreScreenModel,
        title: String,
        detail: String,
    ) {
        use crate::app::frontend::shell_pass::resolve_csf;
        use crate::ui::campaign_score_shell::{CampaignScorePage, CampaignScoreTexts};

        state.frontend.score_page = None;
        state.frontend.score_art = None;
        let side = model.side;
        state.frontend.campaign_score_art = state.process_assets.manager().and_then(|assets| {
            crate::render::main_menu_shell_chrome::build_campaign_score_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                side,
                crate::app::frontend::shell_layout::art_key_screen_width(state),
                state.process_assets.side_score_art(side),
            )
        });
        if state.frontend.campaign_score_art.is_none() {
            log::warn!(
                "Could not prepare Campaign StandaloneScore background; Continue remains available"
            );
        }
        let texts = CampaignScoreTexts {
            title: resolve_csf(state, "GUI:StandaloneScore").into_owned(),
            continue_label: resolve_csf(state, "GUI:Continue").into_owned(),
            continue_status: resolve_csf(state, "STT:StandScoreButtonReturn").into_owned(),
            par_time_label: resolve_csf(state, "GUI:ParTime").into_owned(),
            your_time_label: resolve_csf(state, "GUI:YourTime").into_owned(),
            enemies_destroyed_label: resolve_csf(state, "GUI:HeadsTaken").into_owned(),
        };
        let now = Instant::now();
        // ScoreDialog_Handle_ScoreThemeA/B (0x1002F920/0x1002F9A0) replace the
        // literal the dialog pushes with the side's par-branch theme.
        let theme = model
            .theme
            .clone()
            .unwrap_or_else(|| crate::app::match_runtime::scenario_exit::SCORE_THEME.to_owned());
        let page = CampaignScorePage::open(model, texts, now);
        state.frontend.campaign_score_page = Some(page);
        state.frontend.shell_first_paint_slide = None;
        state.frontend.screen = GameScreen::MissionResult { title, detail };
        state.frontend.shell_controller.reset_to(
            crate::ui::campaign_score_shell::CAMPAIGN_SCORE_DIALOG,
            false,
        );
        // gamemd StandaloneScore dialog proc6C8FF0, WM_INIT6C9357..6C936C:
        // the theme starts here, after Victory685928's Win movie has returned.
        if let Some(assets) = state.process_assets.manager() {
            let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            let _ = state.audio.play_theme(&theme, assets, wall_ms);
        }
    }

    /// Cooperative `+0xA4`'s campaign end (`0x00471420`): the side's score
    /// art again (`0x0072D300`, `ScoreArt__Load`), the empty backdrop, then
    /// dialog `0xBC7` with the texts its init writes. No theme changes.
    pub(crate) fn open_campaign_end_page(
        state: &mut AppState,
        model: crate::ui::campaign_score_shell::CampaignEndModel,
    ) {
        use crate::ui::campaign_score_shell::{CAMPAIGN_END_DIALOG, CampaignScorePage};
        state.frontend.campaign_score_art = state.process_assets.manager().and_then(|assets| {
            crate::render::main_menu_shell_chrome::build_campaign_score_art(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
                model.side,
                crate::app::frontend::shell_layout::art_key_screen_width(state),
                state.process_assets.side_score_art(model.side),
            )
        });
        if state.frontend.campaign_score_art.is_none() {
            log::warn!("Could not prepare the campaign-end score art; Continue remains available");
        }
        let page = CampaignScorePage::open_campaign_end(
            model,
            |key| Self::csf_label(state, key, ""),
            Instant::now(),
        );
        state.frontend.campaign_score_page = Some(page);
        state.frontend.shell_first_paint_slide = None;
        state.frontend.screen = GameScreen::MissionResult {
            title: String::new(),
            detail: String::new(),
        };
        state
            .frontend
            .shell_controller
            .reset_to(CAMPAIGN_END_DIALOG, false);
    }

    fn score_layout(state: &AppState) -> crate::ui::score_shell::ScoreLayout {
        crate::app::frontend::shell_layout::score_layout(state)
    }

    /// Continue and VERA result paging through the shared controller.
    fn score_input(
        state: &mut AppState,
    ) -> (
        crate::ui::score_shell::ScoreLayout,
        Vec<crate::ui::shell::layout::LaidOutControl>,
        (i32, i32),
    ) {
        let layout = Self::score_layout(state);
        let mut feed = Self::menu_page_button_feed(&layout.page);
        if state
            .frontend
            .score_page
            .as_ref()
            .is_some_and(|page| page.page_count() > 1)
        {
            for id in [
                crate::ui::score_shell::PREVIOUS_PAGE,
                crate::ui::score_shell::NEXT_PAGE,
            ] {
                feed.push(crate::ui::shell::layout::LaidOutControl {
                    id,
                    rect: layout.paging_button(id).expect("paging button"),
                });
            }
        }
        state
            .frontend
            .shell_controller
            .ensure_active(crate::ui::score_shell::SCORE_DIALOG, false);
        let cursor = state.match_state.input.cursor_px();
        (layout, feed, cursor)
    }

    /// Only Continue has help. Everywhere else the proc's common handler
    /// (`0x00622B50`, `WM_NCHITTEST` at `0x00622CCB..0x00622E83`) finds the
    /// dialog, a band (bands sit above the cells in Z-order) or another
    /// static without help and writes an empty text: every move writes the
    /// status line, which repaints it (`0x00615EF7`).
    pub(super) fn handle_score_shell_mouse_move(state: &mut AppState) {
        let (_, feed, (x, y)) = Self::score_input(state);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        state.frontend.shell_status_line.repaint();
    }

    pub(super) fn handle_score_shell_mouse_down(state: &mut AppState) {
        let (_, feed, (x, y)) = Self::score_input(state);
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // GUIMainButtonSound through the owner-draw subclass (0x0061376B).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    /// Continue (`0x6D1`, result 1 at `0x005CA06C`) tears `0x108` down with
    /// its slide; the match then ends and the shell resumes.
    pub(super) fn handle_score_shell_mouse_up(state: &mut AppState) {
        let (_, feed, (x, y)) = Self::score_input(state);
        match state.frontend.shell_controller.on_pointer_up(x, y, &feed) {
            Some(crate::ui::score_shell::CONTINUE_BUTTON) => Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::ScoreContinue,
            ),
            Some(
                id @ (crate::ui::score_shell::PREVIOUS_PAGE | crate::ui::score_shell::NEXT_PAGE),
            ) => {
                if let Some(page) = state.frontend.score_page.as_mut() {
                    page.change_page(id == crate::ui::score_shell::NEXT_PAGE, Instant::now());
                }
                state.platform.window.request_redraw();
            }
            _ => {}
        }
    }

    fn campaign_score_layout(
        state: &AppState,
    ) -> crate::ui::campaign_score_shell::CampaignScoreLayout {
        let (width, height) = crate::app::frontend::shell_layout::shell_screen_size(state);
        let fills_window = crate::app::frontend::shell_layout::fills_window(state);
        state.frontend.campaign_score_page.as_ref().map_or_else(
            || crate::ui::campaign_score_shell::CampaignScoreLayout::compute_layout(width, height),
            |page| page.layout(width, height, fills_window),
        )
    }

    fn campaign_score_input(
        state: &mut AppState,
    ) -> (
        crate::ui::campaign_score_shell::CampaignScoreLayout,
        Vec<crate::ui::shell::layout::LaidOutControl>,
        Vec<crate::ui::shell::layout::LaidOutControl>,
        (i32, i32),
    ) {
        let dialog = Self::campaign_score_dialog(state);
        let layout = Self::campaign_score_layout(state);
        // Hover tracks the value statics' help strings. Keep the separate
        // button-only press feed so those static ids are never pressed.
        let hover_feed = dialog
            .controls()
            .iter()
            .filter(|control| control.action.is_some() || control.tooltip_key.is_some())
            .filter_map(|control| {
                layout.control_rect(control.id).map(|rect| {
                    crate::ui::shell::layout::LaidOutControl {
                        id: control.id,
                        rect,
                    }
                })
            })
            .collect();
        let press_feed = vec![crate::ui::shell::layout::LaidOutControl {
            id: dialog.continue_button(),
            rect: layout.continue_button,
        }];
        state
            .frontend
            .shell_controller
            .ensure_active(dialog.id(), false);
        let cursor = state.match_state.input.cursor_px();
        (layout, hover_feed, press_feed, cursor)
    }

    pub(super) fn handle_campaign_score_mouse_move(state: &mut AppState) {
        let (_, hover_feed, _, (x, y)) = Self::campaign_score_input(state);
        state
            .frontend
            .shell_controller
            .on_pointer_move(x, y, &hover_feed);
    }

    pub(super) fn handle_campaign_score_mouse_down(state: &mut AppState) {
        let (_, _, press_feed, (x, y)) = Self::campaign_score_input(state);
        state
            .frontend
            .shell_controller
            .on_pointer_down(x, y, &press_feed);
        if state.frontend.shell_controller.pressed()
            == Some(Self::campaign_score_dialog(state).continue_button())
        {
            Self::play_main_menu_button_sound(state);
        }
    }

    fn campaign_score_dialog(
        state: &AppState,
    ) -> crate::ui::campaign_score_shell::CampaignScoreDialog {
        state
            .frontend
            .campaign_score_page
            .as_ref()
            .map_or_else(Default::default, |page| page.dialog)
    }

    /// Native `0x103` Continue (`0x423`, result 1) tears the dialog down with
    /// its slide before GameExit::Victory advances the campaign; `0xBC7`'s
    /// (`0x6D1`, result 1) before Main_Game returns to the shell.
    pub(super) fn handle_campaign_score_mouse_up(state: &mut AppState) {
        use crate::app::frontend::shell_transition::ShellExitThen;
        use crate::ui::campaign_score_shell::CampaignScoreDialog;
        let dialog = Self::campaign_score_dialog(state);
        let (_, _, press_feed, (x, y)) = Self::campaign_score_input(state);
        if state
            .frontend
            .shell_controller
            .on_pointer_up(x, y, &press_feed)
            != Some(dialog.continue_button())
        {
            return;
        }
        match dialog {
            CampaignScoreDialog::Victory => {
                // `0x423` ends the global VocHandle before the result returns.
                Self::stop_campaign_score_anim_sound(state);
                Self::leave_shell_dialog(state, ShellExitThen::CampaignScoreContinue);
            }
            CampaignScoreDialog::CampaignEnd => {
                Self::leave_shell_dialog(state, ShellExitThen::CampaignEndContinue)
            }
        }
    }

    /// Shared teardown for both result-screen forms: flush the deterministic log
    /// while its simulation is still alive, hand the scenario stream back to the
    /// offline shell, retaining the runtime and presentation resources.
    pub(super) fn leave_mission_result_screen(state: &mut AppState) {
        // WM_DESTROY ends 0x103's global VocHandle even when no Continue
        // command did; the stop is idempotent.
        Self::stop_campaign_score_anim_sound(state);
        // A won campaign mission continues in GameExit::Victory instead of
        // returning to the shell; read it before the teardown below.
        let campaign_victory = Self::take_campaign_victory(state);
        Self::close_finished_scenario(state);
        match campaign_victory {
            Some(victory) => Self::advance_campaign_victory(state, victory),
            None => Self::resume_shell_after_match(state),
        }
    }

    /// Retire a scenario whose outcome teardown has run: flush the
    /// deterministic log while its simulation is still alive and hand the
    /// scenario stream back to the offline shell, retaining the runtime and
    /// presentation resources.
    pub(super) fn close_finished_scenario(state: &mut AppState) {
        Self::retire_finished_scenario(state);
        Self::persist_returned_session_toggles(state);
        state.frontend.score_page = None;
        state.frontend.campaign_score_page = None;
        // 0x0072D780 releases the score art.
        state.frontend.score_art = None;
        state.frontend.campaign_score_art = None;
        state.frontend.screen = GameScreen::MainMenu;
    }

    /// The teardown every finished scenario shares, the co-op continuation
    /// included, which never reaches Main_Game's shell return.
    pub(crate) fn retire_finished_scenario(state: &mut AppState) {
        // Main_Game48CEBF also retires ScenarioStarted after score Continue;
        // source: original gamemd.exe Main_Game48CCC0 (EBX=0 at48CCD4).
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
            runtime.simulation.retire_scenario_started();
        }
        // The accepted match owns the socket until either exit path returns
        // it to the visit's shared transport owner, including score Continue.
        crate::app::lan_service::return_transport(state);
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        Self::capture_returned_multiplayer_scenario(state);
        state.match_state.startup.clear();
        state.match_state.scenario_elapsed_clock.reset();
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    /// PrepareSession re-enters the active mode's shell: LAN starts a new BB,
    /// WOL its lobby 0x113, Skirmish a new0x102. The session keeps the
    /// admitted network return owner.
    pub(crate) fn resume_shell_after_match(state: &mut AppState) {
        if state.frontend.external_launch {
            // A relay launch first reports its result to the match service;
            // its live service starts the same quit once that settles.
            if !crate::app::shell_wol::end_spawned_match(state) {
                Self::start_quit_cascade(state);
            }
            return;
        }
        if state.match_state.session.network_return_context().is_some() {
            crate::app::shell_network_setup::return_after_match(state);
            return;
        }
        if state.frontend.shell_route.skirmish() {
            Self::enter_native_skirmish_from_single_player(state);
        }
    }

    /// `0x497` enables Load Saved Game when the scan `0x00559C20` finds a
    /// save the list reader accepts; the list `0x005596A0` uses the same
    /// reader, so an enabled button always opens a non-empty list.
    fn refresh_single_player_load_state(state: &mut AppState) {
        state
            .frontend
            .single_player_shell_state
            .load_saved_game_enabled = state.persistence.repository.has_browser_entry();
    }

    pub(super) fn open_single_player_shell(state: &mut AppState) {
        // Native destroys 0xE2 (including child 0x71A) before constructing
        // 0x100. Invalidate at the route edge rather than waiting for a paint:
        // a queued Back/Escape can otherwise return to 0xE2 before 0x100 draws
        // and incorrectly preserve the old main-menu movie timeline.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::SinglePlayer;
        // A new 0x100 instance starts with no press or hover.
        state.frontend.shell_controller.reset_to(
            crate::ui::single_player_shell::SINGLE_PLAYER_PAGE.dialog,
            false,
        );
        Self::refresh_single_player_load_state(state);
    }

    pub(super) fn close_single_player_shell(state: &mut AppState) {
        // Result 0x12 destroys 0x100 before the main-menu loop constructs a new
        // 0xE2. Clear immediately so a same-event-loop round trip cannot reuse
        // the source dialog's movie session.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
    }

    /// A new `0x102` from Single Player's Skirmish or, after a Skirmish game,
    /// from PrepareSession: GameMode stays 5, so state `0x10` re-reads the
    /// settings and runs the Skirmish runner again (`0x0052DB09..0x0052DB24`,
    /// `0x0052E119`, `0x0052E168` -> `0x006AE2C0`). Its Back returns to
    /// Single Player either way (GameMode 0, state 1, `0x0052E175`).
    pub(super) fn enter_native_skirmish_from_single_player(state: &mut AppState) {
        Self::prepare_multiplayer_settings(state, MultiplayerSettingsContext::Shell);
        // Prepare dialog 0x102 from the process-lifetime MIX list before
        // destroying its 0x100 source. Active YR's FUN_00534E50 registers the
        // neutral pair on that shared list before the shell SHPs are loaded.
        if !Self::ensure_skirmish_shell_chrome(state) {
            log::warn!("Skirmish shell chrome unavailable; retaining the Single Player shell");
            return;
        }

        // Native destroys dialog 0x100 and its child 0x71A movie handle before
        // constructing 0x102. Drop the hidden Rust session as well so returning
        // to 0x100 cannot continue the pre-Skirmish RA2TS timeline.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::Skirmish;
        state
            .frontend
            .skirmish_shell_state
            .pressed_owner_draw_button = None;
        // A new 0x102 creates its kind-1 statics hidden (0x0060A5B0).
        state.frontend.skirmish_shell_state.statics.reset();
        // Native constructs 0x102 fresh on every visit, and
        // `SkirmishDialog__OnSetup497` hands its controls the Session values that
        // `prepare_multiplayer_settings` just refreshed above. The retained Rust
        // controls follow that same order, so a durable record changed since the
        // last visit reaches the page instead of the page keeping its old face.
        state.frontend.multiplayer_shell_runtime.hydrate_shell(
            &mut state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            &state.frontend.skirmish_modes,
        );
        crate::ui::skirmish_shell::repair_teams_for_selected_mode(
            &mut state.frontend.skirmish_shell_state,
            &state.frontend.skirmish_modes,
        );
        crate::ui::skirmish_shell::initialize_rows_for_selected_map(
            &mut state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
        );
        // The skirmish dialog (0x102) slides its controls in on first paint like
        // every shell dialog; the per-frame slide trigger starts that wave once
        // the skirmish shell becomes the showing screen. Clear any stale wave
        // from the source shell here so the trigger restarts cleanly.
        state.frontend.shell_first_paint_slide = None;
    }

    pub(super) fn return_from_skirmish_to_single_player_shell(state: &mut AppState) {
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        state.frontend.skirmish_shell_state.choose_map_modal = None;
        state.frontend.skirmish_shell_state.validation_modal = None;
        state.frontend.skirmish_shell_state.open_combo_dropdown = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_drag = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_press = None;
        state.frontend.skirmish_shell_state.trackbar_hold = None;
        state
            .frontend
            .skirmish_shell_state
            .pressed_owner_draw_button = None;
        crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
        state.frontend.skirmish_preview_texture = None;
        Self::open_single_player_shell(state);
    }

    pub(super) fn render_shell_error(
        state: &mut AppState,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> Result<()> {
        let error = state
            .frontend
            .main_menu_shell_error
            .get_or_insert_with(|| "Required game-menu resources could not be loaded.".to_owned())
            .clone();
        let asset_root = state
            .platform
            .game_config
            .as_ref()
            .map(|config| config.paths.ra2_dir.display().to_string())
            .unwrap_or_else(|| "No game installation configured".to_owned());
        // No archives at all means the installation is missing or wrong; some
        // archives but no menu artwork means the install is incomplete. Say
        // which, so the player is not sent looking for files that are present.
        let title = if state.process_assets.manager().is_some() {
            "Unable to load the game menu"
        } else {
            "Game files not found"
        };
        // The card's in-repo face carries ASCII only, so a non-ASCII install
        // path draws as boxes. Point the player at the log, which holds the
        // exact path in UTF-8.
        let path_note = if asset_root.is_ascii() {
            String::new()
        } else {
            "\n(characters this screen cannot draw are shown as boxes; the log has the exact path)"
                .to_owned()
        };
        let details = display_error(&error);
        crate::app::frontend::status_screen::render(
            state,
            encoder,
            view,
            title,
            &format!(
                "VERA20K needs Red Alert 2: Yuri's Revenge to run.\n\n\
1. Copy vera20k.exe into your Red Alert 2: Yuri's Revenge folder and start it there.\n\n\
2. Or set [paths] ra2_dir in config.toml, either in the folder you start it from or beside vera20k.exe.\n\n\
Game files: {asset_root}{path_note}\n\
Log: logs/ra2.log\n\n\
Details: {details}"
            ),
            crate::app::frontend::status_screen::Buttons::QUIT,
        );
        Ok(())
    }

    /// Resolve a CSF string key to display text, falling back to the supplied
    /// English string when the table is absent or missing the key.
    pub(super) fn csf_label(state: &AppState, key: &str, fallback: &str) -> String {
        state
            .process_assets
            .csf
            .as_ref()
            .map(|csf| csf.text(key).into_owned())
            .unwrap_or_else(|| fallback.to_string())
    }

    /// Construct a fresh launcher Options primary from the current retained
    /// profile, the window's full-screen state, live CSF table, and frozen
    /// process-start common audio gate. Nothing is applied here.
    pub(crate) fn open_launcher_options_dialog(state: &mut AppState) {
        // State 5 destroys `0xE2` (and its RA2TS static) before Options runs;
        // closing Options creates a new `0xE2` with its own entry slide.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        Self::ensure_skirmish_shell_chrome(state);
        if state.frontend.skirmish_shell_chrome.is_none() {
            state.frontend.main_menu_shell_error =
                Some("Could not load the retail Options artwork.".to_owned());
            state.platform.window.request_redraw();
            return;
        }
        use crate::app::persistence::options::launcher::launcher_dialog_from_profile;
        use crate::ui::main_menu_dialogs::options::LauncherOptionsLabels;

        let labels = LauncherOptionsLabels::resolve(&|key| {
            state
                .process_assets
                .csf
                .as_ref()
                .and_then(|csf| csf.get(key))
                .map(str::to_owned)
        });
        // The Display list is VERA's own: it reads the screens attached now, so
        // a screen plugged in or removed since the last launch shows correctly.
        let displays = crate::app::display_target::current_displays(&state.platform.window)
            .iter()
            .map(|display| display.choice().clone())
            .collect();
        let dialog = launcher_dialog_from_profile(
            &state.persistence.options_profile,
            labels,
            state.platform.window.fullscreen().is_some(),
            state.renderer.fps_overlay.is_visible(),
            state.audio.launcher_audio_available,
            displays,
        );
        state.frontend.options_dialog = Some(dialog);
    }

    pub(crate) fn play_launcher_options_cue(
        state: &mut AppState,
        cue: crate::ui::main_menu_dialogs::options::LauncherCue,
    ) {
        use crate::ui::main_menu_dialogs::options::LauncherCue;

        let sound_id = state.rules().and_then(|rules| match cue {
            LauncherCue::MainButton => rules.general.gui_main_button_sound.as_deref(),
            LauncherCue::GenericClick => rules.general.generic_click_sound.as_deref(),
            LauncherCue::Checkbox => rules.general.gui_checkbox_sound.as_deref(),
            LauncherCue::ComboOpen => rules.general.gui_combo_open_sound.as_deref(),
            LauncherCue::ComboClose => rules.general.gui_combo_close_sound.as_deref(),
        });
        let sound_id = sound_id.map(str::to_owned);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    pub(crate) fn native_launcher_options_active(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::MainMenu
            && state.frontend.options_dialog.is_some()
            && state.frontend.skirmish_shell_chrome.is_some()
    }

    /// Both physical shell input and the assetless fallback drain the same
    /// semantic transaction; profile and audio authority remain in their owners.
    fn dispatch_launcher_options_output(
        state: &mut AppState,
        dialog: crate::ui::main_menu_dialogs::OptionsDialogState,
        output: crate::ui::main_menu_dialogs::options::LauncherOptionsFrameOutput,
    ) {
        {
            for event in output.events {
                crate::app::persistence::options::launcher::dispatch_launcher_preview_event(
                    state, event,
                );
            }
        }
        match output.result {
            // 0x0055FD06 tears D5 down before every continuation, including
            // Network (55FD28), Keyboard, and the final Main Menu return.
            Some(
                result @ (crate::ui::main_menu_dialogs::options::LauncherParentResult::Back
                | crate::ui::main_menu_dialogs::options::LauncherParentResult::Keyboard
                | crate::ui::main_menu_dialogs::options::LauncherParentResult::Network),
            ) => {
                state.frontend.options_dialog = Some(dialog);
                Self::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::Options(result),
                );
            }
            Some(result) => {
                let mut operations = AppStateLauncherParentOperations { state };
                crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
                    &mut operations,
                    dialog,
                    result,
                );
            }
            None => {
                state.frontend.options_dialog = Some(dialog);
                // Vera Options or Back: the showing page slides out first.
                if let Some(page) = output.page {
                    Self::leave_shell_dialog(
                        state,
                        crate::app::frontend::shell_transition::ShellExitThen::OptionsPage(page),
                    );
                }
            }
        }
    }

    /// The other Options page has slid out: show `page`, whose column slides
    /// in on the next frame. The dialog and its values stay as they are.
    fn commit_options_page(
        state: &mut AppState,
        page: crate::ui::main_menu_dialogs::options::OptionsPage,
    ) {
        if let Some(dialog) = state.frontend.options_dialog.as_mut() {
            dialog.show_page(page);
        }
        state.platform.window.request_redraw();
    }

    /// `0xD5`'s teardown slide has run: commit the controls (`0x0055FAA0`),
    /// then write the profile and build `0xE2` (Main Menu, `0x005FAD10`) or
    /// run the Keyboard page.
    pub(super) fn commit_launcher_options_result(
        state: &mut AppState,
        result: crate::ui::main_menu_dialogs::options::LauncherParentResult,
    ) {
        let Some(dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        let mut operations = AppStateLauncherParentOperations { state };
        crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
            &mut operations,
            dialog,
            result,
        );
    }

    pub(crate) fn handle_launcher_options_mouse(state: &mut AppState, down: Option<bool>) {
        if down.is_none() {
            // Every hover message repaints the status line (0x00615EF7).
            state.frontend.shell_status_line.repaint();
        }
        let Some(mut dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        let (x, y) = (
            state.match_state.input.cursor_x as i32,
            state.match_state.input.cursor_y as i32,
        );
        let layout =
            crate::app::frontend::shell_layout::launcher_options_layout(state, dialog.page());
        match down {
            Some(true) => dialog.shell_mouse_down(x, y, &layout),
            Some(false) => dialog.shell_mouse_up(x, y, &layout),
            None => dialog.shell_mouse_move(x, y, &layout),
        }
        let output = dialog.drain_output();
        Self::dispatch_launcher_options_output(state, dialog, output);
        state.platform.window.request_redraw();
    }

    /// Pump-terminal completion enters the same always-apply parent transaction
    /// as Back, then writes once before the caller performs unrelated exit work.
    pub(crate) fn close_launcher_options_terminal(state: &mut AppState) {
        let Some(dialog) = state.frontend.options_dialog.take() else {
            return;
        };
        let mut operations = AppStateLauncherParentOperations { state };
        crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
            &mut operations,
            dialog,
            crate::ui::main_menu_dialogs::options::LauncherParentResult::Terminal,
        );
    }

    /// Adapt the laid-out main-menu buttons into the shared controller's
    /// button-only input feed. Statics (title/monitor) are deliberately excluded,
    /// so the controller never hit-tests or hover-tracks them.
    fn main_menu_shell_button_feed(
        layout: &crate::ui::main_menu_shell::MainMenuShellLayout,
    ) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        layout
            .buttons
            .iter()
            .map(|b| crate::ui::shell::layout::LaidOutControl {
                id: b.id.resource_id(),
                rect: b.rect,
            })
            .collect()
    }

    /// Owner-draw button feed for a right-panel menu page (0x100 / 0x101).
    pub(super) fn menu_page_button_feed(
        layout: &crate::ui::shell::menu_page::MenuPageLayout,
    ) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        layout
            .buttons
            .iter()
            .map(|b| crate::ui::shell::layout::LaidOutControl {
                id: b.id,
                rect: b.rect,
            })
            .collect()
    }

    /// Mirror the controller's press/hover state into the per-shell struct the
    /// render path reads. Slice-2/Slice-3 boundary: render is retired off these in
    /// Slice 3, after which the controller is the sole authority.
    fn mirror_shell_controller_to_main_menu(state: &mut AppState) {
        state
            .frontend
            .main_menu_shell_state
            .pressed_owner_draw_button = state
            .frontend
            .shell_controller
            .pressed()
            .and_then(crate::ui::main_menu_shell::MainMenuControlId::from_resource_id);
        state
            .frontend
            .main_menu_shell_state
            .hovered_owner_draw_button = state
            .frontend
            .shell_controller
            .hovered()
            .and_then(crate::ui::main_menu_shell::MainMenuControlId::from_resource_id);
    }

    pub(super) fn handle_main_menu_shell_mouse_down(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::main_menu_layout(state);
        let feed = Self::main_menu_shell_button_feed(&layout);
        let (x, y) = state.match_state.input.cursor_px();
        state
            .frontend
            .shell_controller
            .ensure_active(crate::ui::shell::descriptor::DialogId(0x00E2), false);
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        let pressed = state.frontend.shell_controller.pressed().is_some();
        Self::mirror_shell_controller_to_main_menu(state);
        // The original plays the button sound on mouse-DOWN over a button (not on
        // release); `pressed` is button-only by construction, so a static never
        // triggers it.
        if pressed {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_main_menu_shell_mouse_move(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::main_menu_layout(state);
        let feed = Self::main_menu_shell_button_feed(&layout);
        let (x, y) = state.match_state.input.cursor_px();
        state
            .frontend
            .shell_controller
            .ensure_active(crate::ui::shell::descriptor::DialogId(0x00E2), false);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        Self::mirror_shell_controller_to_main_menu(state);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    pub(super) fn handle_main_menu_shell_mouse_up(state: &mut AppState) {
        let layout = crate::app::frontend::shell_layout::main_menu_layout(state);
        let feed = Self::main_menu_shell_button_feed(&layout);
        let (x, y) = state.match_state.input.cursor_px();
        state
            .frontend
            .shell_controller
            .ensure_active(crate::ui::shell::descriptor::DialogId(0x00E2), false);
        let activated = state.frontend.shell_controller.on_pointer_up(x, y, &feed);
        Self::mirror_shell_controller_to_main_menu(state);
        if let Some(action) = activated
            .and_then(crate::ui::main_menu_shell::MainMenuControlId::from_resource_id)
            .map(crate::ui::main_menu_shell::action_for_control)
        {
            use crate::app::frontend::shell_transition::ShellExitThen;
            use crate::ui::main_menu_shell::MainMenuShellAction;
            match action {
                // These results destroy 0xE2 (Exit's confirmation is state 6,
                // shown over the empty shell backdrop).
                MainMenuShellAction::SinglePlayer
                | MainMenuShellAction::MoviesAndCredits
                | MainMenuShellAction::Options
                | MainMenuShellAction::Network
                | MainMenuShellAction::WwOnline
                | MainMenuShellAction::ExitGame => {
                    Self::leave_shell_dialog(state, ShellExitThen::MainMenu(action))
                }
                _ => Self::handle_main_menu_shell_action(state, action),
            }
        }
    }

    /// Count-rule message-box controls share the rendered template and geometry.
    fn shell_message_box_feed(
        state: &AppState,
    ) -> Option<(
        crate::ui::shell::descriptor::DialogId,
        Vec<crate::ui::shell::layout::LaidOutControl>,
    )> {
        use crate::ui::shell::layout::LaidOutControl;
        use crate::ui::shell::modal;
        let kind = state.frontend.shell_message_box.as_ref()?.labels.kind();
        let layout = crate::app::frontend::shell_layout::shell_message_box_layout(state, kind);
        let mut feed = vec![LaidOutControl {
            id: modal::control::OK,
            rect: layout.ok,
        }];
        if let Some(rect) = layout.cancel {
            feed.push(LaidOutControl {
                id: modal::control::CANCEL,
                rect,
            });
        }
        Some((
            crate::ui::shell::descriptor::DialogId(kind.template_id(false)),
            feed,
        ))
    }

    pub(super) fn handle_shell_message_box_mouse_down(state: &mut AppState) {
        let Some((dialog, feed)) = Self::shell_message_box_feed(state) else {
            return;
        };
        let (x, y) = state.match_state.input.cursor_px();
        if state.frontend.shell_controller.top_id() != Some(dialog) {
            return;
        }
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // The message box (`0x005D3490`, proc `0x005D36A0`) subclasses its
        // owner-draw OK/Cancel to `0x00612B70`, which plays
        // GUIMainButtonSound on the press (`0x0061374B`).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_shell_message_box_mouse_up(state: &mut AppState) {
        let Some((dialog, feed)) = Self::shell_message_box_feed(state) else {
            return;
        };
        let (x, y) = state.match_state.input.cursor_px();
        if state.frontend.shell_controller.top_id() != Some(dialog) {
            return;
        }
        let activated = state.frontend.shell_controller.on_pointer_up(x, y, &feed);
        if let Some(result) =
            activated.and_then(crate::ui::shell::modal::ModalResult::from_message_box_control)
        {
            Self::resolve_shell_message_box(state, result);
        }
    }

    /// The box's result returns to its caller. Callback `0x005D36A0` writes 0
    /// for `0x5AE` and 1 for control 2 or IDOK.
    pub(super) fn resolve_shell_message_box(
        state: &mut AppState,
        result: crate::ui::shell::modal::ModalResult,
    ) {
        use crate::app::frontend::state::ShellMessageBoxPurpose;
        let Some(message_box) = state.frontend.shell_message_box.take() else {
            return;
        };
        match message_box.purpose {
            // OK -> quit (result 0). Dismiss the confirmation first, persist
            // settings to VERA.INI, then run the graceful cascade (music fade
            // → trailing-voice wait → hard stop → exit) via render_frame instead
            // of exiting immediately. The screen fade-to-black is sub-step 4b-ii-b.
            ShellMessageBoxPurpose::ExitGame
                if result == crate::ui::shell::modal::ModalResult::MessageBox(0) =>
            {
                let mut operations = AppStateConfirmedQuitOperations { state };
                dispatch_shell_controller_confirmed_quit(&mut operations);
            }
            // Cancel -> stay; close the modal via the controller pop (D-B3)
            // so mouse and Esc converge on the same teardown.
            ShellMessageBoxPurpose::ExitGame => {
                Self::close_shell_message_box_from_controller(state);
                state.platform.window.request_redraw();
            }
            ShellMessageBoxPurpose::CampaignReplay(retry) => {
                Self::close_shell_message_box_from_controller(state);
                Self::answer_campaign_replay(
                    state,
                    retry,
                    crate::app::campaign_defeat::replay_route(result),
                );
            }
            ShellMessageBoxPurpose::CampaignVictory(victory) => {
                Self::close_shell_message_box_from_controller(state);
                Self::advance_campaign_victory(state, *victory);
            }
        }
    }

    /// Bind the shared controller to the active menu page for one pointer
    /// event and refresh its runtime-disabled controls. `ensure_active` only
    /// resets on a dialog change, so a gesture's press survives to its release.
    fn prepare_menu_page_input(
        state: &mut AppState,
        page: crate::app::frontend::menu_page_render::ActiveMenuPage,
    ) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        use crate::app::frontend::menu_page_render::ActiveMenuPage;
        let spec = page.spec();
        let layout = crate::app::frontend::shell_layout::menu_page_layout(state, spec);
        state
            .frontend
            .shell_controller
            .ensure_active(spec.dialog, false);
        if page == ActiveMenuPage::SinglePlayer {
            let load_enabled = state
                .frontend
                .single_player_shell_state
                .load_saved_game_enabled;
            state.frontend.shell_controller.set_disabled(
                crate::ui::single_player_shell::SinglePlayerControlId::LoadSavedGame0x689
                    .resource_id(),
                !load_enabled,
            );
        }
        Self::menu_page_button_feed(&layout)
    }

    pub(super) fn handle_menu_page_mouse_down(state: &mut AppState) {
        let Some(page) = crate::app::frontend::menu_page_render::ActiveMenuPage::from_state(state)
        else {
            return;
        };
        let feed = Self::prepare_menu_page_input(state, page);
        let (x, y) = state.match_state.input.cursor_px();
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // The owner-draw press cue plays on mouse-down over an enabled button.
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
    }

    pub(super) fn handle_menu_page_mouse_move(state: &mut AppState) {
        let Some(page) = crate::app::frontend::menu_page_render::ActiveMenuPage::from_state(state)
        else {
            return;
        };
        let feed = Self::prepare_menu_page_input(state, page);
        let (x, y) = state.match_state.input.cursor_px();
        // Hover is enable-unfiltered: a disabled button still drives 0x695.
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.repaint();
    }

    pub(super) fn handle_menu_page_mouse_up(state: &mut AppState) {
        use crate::app::frontend::menu_page_render::ActiveMenuPage;
        let Some(page) = ActiveMenuPage::from_state(state) else {
            return;
        };
        let feed = Self::prepare_menu_page_input(state, page);
        let (x, y) = state.match_state.input.cursor_px();
        let Some(activated) = state.frontend.shell_controller.on_pointer_up(x, y, &feed) else {
            return;
        };
        use crate::app::frontend::shell_transition::ShellExitThen;
        match page {
            ActiveMenuPage::SinglePlayer => {
                use crate::ui::single_player_shell::SinglePlayerShellAction;
                if let Some(action) =
                    crate::ui::single_player_shell::SinglePlayerControlId::from_resource_id(
                        activated,
                    )
                    .map(crate::ui::single_player_shell::action_for_control)
                {
                    match action {
                        // States 0x12, 8, 9 and 0xB destroy 0x100.
                        SinglePlayerShellAction::MainMenu
                        | SinglePlayerShellAction::NewCampaign
                        | SinglePlayerShellAction::LoadSavedGame
                        | SinglePlayerShellAction::Skirmish => {
                            Self::leave_shell_dialog(state, ShellExitThen::SinglePlayer(action));
                        }
                        _ => Self::handle_single_player_shell_action(state, action),
                    }
                }
            }
            ActiveMenuPage::MoviesAndCredits => {
                if let Some(action) = crate::ui::movies_credits_shell::action_for_control(activated)
                {
                    Self::leave_shell_dialog(state, ShellExitThen::MoviesCredits(action));
                }
            }
        }
    }

    /// GUIMainButtonSound (`Rules+0x188`): the owner-draw button procedure
    /// `0x00612B70` plays it on every admitted press (`0x0061374B..0x00613771`),
    /// in shell and in-game dialogs alike.
    /// RESIDUAL: a frame-type-0 (sliced PCX) text button also plays
    /// GenericClick when it paints pressed after an unpressed type-0 paint
    /// (`0x0061323C..0x00613289`); that paint-time cue is not ported. Every
    /// in-game dialog button is type 2 or 3 (`0x0060A400`), so it is a shell
    /// difference only.
    pub(super) fn play_main_menu_button_sound(state: &mut AppState) {
        let sound_id = state
            .rules()
            .and_then(|rules| rules.general.gui_main_button_sound.as_deref())
            .map(str::to_string);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    /// Play the shell first-paint slide-in cue ([AudioVisual] GUIMoveInSound,
    /// stock `MenuSlideIn`), once at the start of each allow-listed shell
    /// dialog's controls-reveal slide. A no-op when the key is empty/unset.
    pub(crate) fn play_shell_slide_in_sound(state: &mut AppState) {
        let sound_id = state
            .rules()
            .and_then(|rules| rules.general.gui_move_in_sound.as_deref())
            .map(str::to_string);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    /// Play the shell teardown slide-out cue ([AudioVisual] GUIMoveOutSound,
    /// stock `MenuSlideOut`) as `0x00608070` starts. A no-op when unset.
    fn play_shell_slide_out_sound(state: &mut AppState) {
        let sound_id = state
            .rules()
            .and_then(|rules| rules.general.gui_move_out_sound.as_deref())
            .map(str::to_string);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    /// Leave the showing family dialog with the result `then`: its teardown
    /// (`0x00622720`) slides the buttons out first (`0x00608070`), and the
    /// result runs when the slide has ended. A dialog that is not showing
    /// steady commits at once; a dialog already sliding out keeps its result.
    pub(super) fn leave_shell_dialog(
        state: &mut AppState,
        then: crate::app::frontend::shell_transition::ShellExitThen,
    ) {
        use crate::app::frontend::shell_transition::ShellExitStart;
        match crate::app::frontend::shell_transition::begin_shell_exit(state, then) {
            ShellExitStart::Sliding => Self::play_shell_slide_out_sound(state),
            ShellExitStart::Immediate(then) => Self::commit_shell_exit(state, then),
            ShellExitStart::AlreadyLeaving => {}
        }
    }

    /// Run a finished teardown slide's result.
    pub(super) fn drive_shell_exit(state: &mut AppState) {
        if let Some(then) =
            crate::app::frontend::shell_transition::advance_shell_exit(state, Instant::now())
        {
            Self::commit_shell_exit(state, then);
        }
    }

    fn commit_shell_exit(
        state: &mut AppState,
        then: crate::app::frontend::shell_transition::ShellExitThen,
    ) {
        use crate::app::frontend::shell_transition::ShellExitThen;
        match then {
            ShellExitThen::MainMenu(action) => Self::handle_main_menu_shell_action(state, action),
            ShellExitThen::SinglePlayer(action) => {
                Self::handle_single_player_shell_action(state, action)
            }
            ShellExitThen::MoviesCredits(action) => {
                Self::handle_movies_credits_action(state, action)
            }
            ShellExitThen::PlayMovie => Self::play_selected_movie(state),
            ShellExitThen::MovieListBack => Self::open_movies_credits_page(state),
            ShellExitThen::SkirmishStart(session) => Self::commit_skirmish_start(state, *session),
            ShellExitThen::SkirmishBack => Self::commit_skirmish_back(state),
            ShellExitThen::CampaignBack => Self::commit_campaign_back(state),
            ShellExitThen::CampaignStart(side) => Self::commit_campaign_start(state, side),
            ShellExitThen::LoadSavedGameBack => Self::commit_load_saved_game_back(state),
            ShellExitThen::Options(result) => Self::commit_launcher_options_result(state, result),
            ShellExitThen::OptionsPage(page) => Self::commit_options_page(state, page),
            ShellExitThen::KeyboardClose(exit) => {
                crate::app::input::keyboard::commit_close(state, exit)
            }
            ShellExitThen::NetworkOptionsClose => {
                crate::app::input::network_options::commit_close(state)
            }
            ShellExitThen::LanBack => Self::commit_lan_back(state),
            ShellExitThen::LanCreate { observe } => Self::commit_lan_create(state, observe),
            ShellExitThen::LanJoinAccepted(accepted) => {
                Self::open_lan_joined_setup(state, accepted)
            }
            ShellExitThen::NetworkSetupBack(_) => {
                crate::app::shell_network_setup::commit_back(state)
            }
            ShellExitThen::NetworkSetupChooseMap(_) => {
                crate::app::shell_network_setup::open_choose_map(state)
            }
            ShellExitThen::NetworkStart { agreed_seed, .. } => {
                crate::app::shell_network_setup::commit_start(state, agreed_seed)
            }
            ShellExitThen::SkirmishChooseMap => {
                let mode_id = state.frontend.skirmish_shell_state.selected_mode_id;
                let record_index = Self::current_choose_map_record_index(state);
                Self::open_choose_map_modal(state, mode_id, record_index);
            }
            ShellExitThen::ChooseMapUse(selection) => Self::commit_choose_map_use(state, selection),
            ShellExitThen::ChooseMapCancel => Self::close_choose_map_modal(state),
            ShellExitThen::ChooseMapRandomMap => Self::commit_choose_map_random_map(state),
            ShellExitThen::RandomMapUse => Self::accept_random_map_setup(state),
            ShellExitThen::RandomMapCancel => Self::cancel_random_map_setup(state),
            ShellExitThen::WolBack => Self::return_from_wol(state),
            ShellExitThen::ScoreContinue => Self::leave_mission_result_screen(state),
            ShellExitThen::CampaignScoreContinue => Self::leave_mission_result_screen(state),
            ShellExitThen::CampaignEndContinue => {
                crate::app::cooperative_round::campaign_end_continue(state)
            }
            ShellExitThen::WolAction(mode) => Self::commit_wol_action(state, mode),
            ShellExitThen::WolLoginClose(result) => Self::commit_wol_login_close(state, result),
            ShellExitThen::WolLobbyBack => crate::app::shell_wol::lobby::commit_back(state),
            ShellExitThen::WolLobbyBuddy => crate::app::shell_wol::lobby::commit_buddy(state),
            ShellExitThen::WolLobbyHide => crate::app::shell_wol::lobby::commit_hide(state),
            ShellExitThen::WolCreateAccepted { observe } => {
                crate::app::shell_wol::lobby::commit_create(state, observe)
            }
            ShellExitThen::WolOptionsOpen => crate::app::shell_wol::options::commit_open(state),
            ShellExitThen::WolOptionsClose(staged) => {
                crate::app::shell_wol::options::commit_close(state, staged)
            }
            ShellExitThen::WolSoundOptionsOpen => {
                crate::app::shell_wol::options::commit_open_sound(state)
            }
            ShellExitThen::WolSoundOptionsBack => {
                crate::app::shell_wol::options::commit_sound_back(state)
            }
            ShellExitThen::WolAddBuddyClose { yes, ask_again } => {
                crate::app::shell_wol::persona::commit_add_buddy(state, yes, ask_again)
            }
            ShellExitThen::WolAutoLoginClose(answer) => {
                crate::app::shell_wol::persona::commit_auto_login(state, answer)
            }
            ShellExitThen::WolLocaleClose(locale) => {
                crate::app::shell_wol::persona::commit_locale(state, locale)
            }
            ShellExitThen::WolMyInformationClose(control) => {
                crate::app::shell_wol::my_info::commit_close(state, control)
            }
            ShellExitThen::WolBuddyClose(control) => {
                crate::app::shell_wol::buddy::commit_close(state, control)
            }
        }
    }

    /// `ShellButtonSlideSound`, which the slide engine `0x006071E0` plays as
    /// a slide-in ends, before the dialog's SHOW completion `0x4EC`
    /// (`0x00607F46..0x00607F9B`); a slide-out ends with `0x4ED` and no
    /// sound. Stock rules leave the key empty, so this is silent there.
    pub(crate) fn play_shell_slide_completion_sound(state: &mut AppState) {
        let sound_id = state
            .rules()
            .and_then(|rules| rules.general.shell_button_slide_sound.as_deref())
            .map(str::to_string);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }

    pub(super) fn maintain_main_menu_intro(state: &mut AppState) {
        if state.frontend.screen != GameScreen::MainMenu || state.frontend.quit_cascade.is_some() {
            return;
        }
        // Play_Movie runs no audio pump; the paused theme resumes afterwards.
        if state.frontend.fullscreen_movie.is_some() {
            return;
        }
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        // The common service pumps Theme for Credits and WOL too, without
        // these routes running the menu loop's Play_Song(INTRO).
        if state.frontend.credits_roll.is_some() {
            return;
        }
        // GameExit::Defeat6860E5's replay prompt is still inside Main_Game.
        // Its modal pump services the retained Theme; PrepareSession52D9BF
        // selects INTRO only after No leaves the game. Yes uses StartScenario.
        // Source: original gamemd.exe 6860EA..686296, 48CFAA, 52D9A0.
        if matches!(
            state
                .frontend
                .shell_message_box
                .as_ref()
                .map(|message| &message.purpose),
            Some(
                crate::app::frontend::state::ShellMessageBoxPurpose::CampaignReplay(_)
                    | crate::app::frontend::state::ShellMessageBoxPurpose::CampaignVictory(_)
            )
        ) {
            return;
        }
        if state.frontend.shell_route.wol_visit() {
            return;
        }
        if let Some(assets) = state.process_assets.manager() {
            state.audio.maintain_main_menu_theme(assets, now_ms);
        }
    }

    pub(crate) fn play_shell_ui_sound_by_id(state: &mut AppState, sound_id: Option<&str>) {
        let Some(sound_id) = sound_id else {
            return;
        };
        let (Some(sfx), Some(catalog)) = (
            &mut state.audio.sfx_player,
            state.process_assets.audio_catalog(),
        ) else {
            return;
        };
        sfx.play_sound(sound_id, catalog.sounds());
    }

    /// `MainMenuDialog0xE2__Run` `0x00531CC0` reads the Keyboard queue while
    /// the bare 0xE2 runs, and only there: every other shell page runs its
    /// own loop. True when that loop owns the key.
    ///
    /// RESIDUAL: VERA drops keys typed during 0xE2's entry slide
    /// (`blocks_shell_input`), and no mouse click reaches the buffer.
    pub(super) fn handle_main_menu_typed_code_key(
        state: &mut AppState,
        event: &winit::event::KeyEvent,
    ) -> bool {
        if state.frontend.screen != GameScreen::MainMenu
            || state.frontend.shell_route != crate::app::shell_route::ShellRoute::MainMenu
            || state.frontend.main_menu_shell_error.is_some()
        {
            return false;
        }
        // InputManagerClass__HandleWindowMessage `0x0054F790` queues no
        // key-down repeat (lParam bit 30); 0xE2 skips releases (`0x00531E13`).
        if !event.state.is_pressed() || event.repeat {
            return true;
        }
        let unmodified = event.key_without_modifiers();
        let key = crate::app::input::hotkeys::binding_logical_key(
            &event.logical_key,
            &unmodified,
            event.location,
        );
        // A key with no virtual key here is no letter either.
        let virtual_key =
            crate::app::input::hotkeys::logical_virtual_key(key, event.location).unwrap_or(0);
        let matched = state.frontend.main_menu_shell_state.typed_codes.key_down(
            virtual_key,
            typed_code_video_flags(&mut state.persistence.options_profile),
        );
        if matched.is_none() {
            return true;
        }
        // 0x00531ECB..0x00531EDB: PlayGlobal(Rules->OptionsChanged, centre, 1.0).
        let sound = state
            .rules()
            .and_then(|rules| rules.general.options_changed_sound())
            .map(str::to_owned);
        Self::play_shell_ui_sound_by_id(state, sound.as_deref());
        // RESIDUAL: 0x00531EE4..0x00531EFA paints the empty backdrop with
        // every right-panel tile shuttered (`Shell__DrawEmptyBackdrop
        // 0x0052FEC0`, DL=1) and nothing invalidates 0xE2 afterwards, so
        // retail may briefly show shuttered buttons on a match. VERA redraws
        // the whole menu.
        state.platform.window.request_redraw();
        true
    }

    /// `Dialog0x103__DlgProc`'s global `VocHandle` at `0x00B0BF28`: 0x4EC
    /// starts the looping `ScoreAnimSound`; Continue and WM_DESTROY end it.
    /// `HandleOwner::Global` is the process-global VocHandle kind, unused by
    /// other VERA cues.
    const CAMPAIGN_SCORE_ANIM_HANDLE: u32 = 0;

    pub(crate) fn play_campaign_score_anim_sound(state: &mut AppState, sound_id: Option<&str>) {
        let Some(sound_id) = sound_id else {
            return;
        };
        // 0x4EC runs `VocHandle__StopAndClear` before the new PlayGlobal, so a
        // repeated SHOW completion cannot stack two loops on the handle.
        Self::stop_campaign_score_anim_sound(state);
        let (Some(sfx), Some(catalog)) = (
            &mut state.audio.sfx_player,
            state.process_assets.audio_catalog(),
        ) else {
            return;
        };
        sfx.play_animation_sound_spatial(
            crate::audio::arbiter::HandleOwner::Global(Self::CAMPAIGN_SCORE_ANIM_HANDLE),
            sound_id,
            crate::audio::sfx::SpatialGain::CENTRED_FULL,
            catalog.sounds(),
        );
    }

    pub(crate) fn stop_campaign_score_anim_sound(state: &mut AppState) {
        if let Some(sfx) = state.audio.sfx_player.as_mut() {
            sfx.stop_animation_sound(crate::audio::arbiter::HandleOwner::Global(
                Self::CAMPAIGN_SCORE_ANIM_HANDLE,
            ));
        }
    }

    pub(super) fn handle_single_player_shell_action(
        state: &mut AppState,
        action: crate::ui::single_player_shell::SinglePlayerShellAction,
    ) {
        use crate::ui::single_player_shell::SinglePlayerShellAction;

        match action {
            SinglePlayerShellAction::None => {}
            SinglePlayerShellAction::Skirmish => {
                Self::enter_native_skirmish_from_single_player(state);
            }
            SinglePlayerShellAction::MainMenu => {
                Self::close_single_player_shell(state);
            }
            SinglePlayerShellAction::LoadSavedGame => {
                if state
                    .frontend
                    .single_player_shell_state
                    .load_saved_game_enabled
                {
                    Self::open_load_saved_game_page(state);
                }
            }
            SinglePlayerShellAction::NewCampaign => Self::open_campaign_page(state),
        }
    }

    pub(super) fn handle_main_menu_shell_action(
        state: &mut AppState,
        action: crate::ui::main_menu_shell::MainMenuShellAction,
    ) {
        use crate::ui::main_menu_shell::MainMenuShellAction;

        match action {
            MainMenuShellAction::None => {}
            // The original pops a confirm message box here; it does NOT quit on
            // the first Exit click. Quitting happens only on confirm.
            MainMenuShellAction::ExitGame => Self::open_exit_confirm(state),
            MainMenuShellAction::SinglePlayer => {
                Self::open_single_player_shell(state);
            }
            MainMenuShellAction::Options => {
                Self::open_launcher_options_dialog(state);
            }
            MainMenuShellAction::MoviesAndCredits => {
                Self::open_movies_credits_page(state);
            }
            MainMenuShellAction::Network => Self::open_lan_lobby(state),
            MainMenuShellAction::WwOnline => Self::open_wol_welcome_page(state),
        }
    }

    /// Open the Exit-Game confirm message box, resolving its labels from CSF.
    fn open_exit_confirm(state: &mut AppState) {
        // State 6 runs after 0xE2 is destroyed: it paints the empty shell
        // backdrop (`0x0052FEC0`) and asks; Cancel builds a new 0xE2 (state
        // 0x12, `0x0052DE39..0x0052DE42`).
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        let csf = |key: &str, fallback: &str| Self::csf_label(state, key, fallback);
        let labels = crate::ui::main_menu_dialogs::ShellMessageBoxLabels::exit_confirm(&csf);
        Self::open_shell_message_box(
            state,
            crate::app::frontend::state::ShellMessageBox {
                labels,
                purpose: crate::app::frontend::state::ShellMessageBoxPurpose::ExitGame,
            },
        );
    }

    /// ShellMessageBox__Run `0x005D3490` over the empty shell backdrop.
    pub(super) fn open_shell_message_box(
        state: &mut AppState,
        message_box: crate::app::frontend::state::ShellMessageBox,
    ) {
        // The SHP modal sources PUDLGBGN/MNBTTN from the skirmish chrome atlas; load
        // it on demand so the box renders straight from the main menu.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        // Host the modal as a TRUE LIFO push over the active shell (D-B3):
        // teardown pops back to it with focus restored. (ensure_active would
        // reset_to-clobber the stack — the prior "0x120 over 0xE2" comment
        // described behavior that never happened.)
        let dialog =
            crate::ui::shell::descriptor::DialogId(message_box.labels.kind().template_id(false));
        if state.frontend.shell_controller.top_id() != Some(dialog) {
            state.frontend.shell_controller.push(dialog, true);
        }
        state.frontend.shell_message_box = Some(message_box);
        state.platform.window.request_redraw();
    }

    /// Whether any main-menu modal dialog is currently open. Used to route
    /// keyboard/mouse to the modal first.
    pub(crate) fn main_menu_dialog_open(state: &AppState) -> bool {
        state.main_menu_dialog_open()
    }

    /// Close retained main-menu child dialogs (options — never on the
    /// controller stack). The exit-confirm modal closes through
    /// close_shell_message_box_from_controller (D-B3).
    pub(crate) fn close_main_menu_dialogs(state: &mut AppState) {
        state.frontend.shell_message_box = None;
        state.frontend.options_dialog = None;
    }

    /// Controller-routed exit-confirm teardown (D-B3): dismiss the modal UI
    /// state, then LIFO-pop its 0x120 instance so focus returns to the shell
    /// beneath. Mirrors `close_validation_modal_from_controller` — every Esc
    /// and mouse close path converges here.
    pub(super) fn close_shell_message_box_from_controller(state: &mut AppState) {
        state.frontend.shell_message_box = None;
        if matches!(
            state.frontend.shell_controller.top_id(),
            Some(crate::ui::shell::descriptor::DialogId(0x00CE | 0x0120))
        ) {
            state.frontend.shell_controller.pop();
        }
    }

    pub(super) fn route_shell_message_box_key(state: &mut AppState, key: ShellKey) -> bool {
        if state.frontend.shell_message_box.is_none() {
            return false;
        }
        if !state.frontend.shell_controller.on_key(key) {
            return false;
        }
        // Native MessageBoxProc5D36A0 maps IDOK1 (Enter with no default
        // pushbutton) and IDCANCEL2 (Escape) to result1, the negative answer.
        Self::resolve_shell_message_box(state, crate::ui::shell::modal::ModalResult::MessageBox(1));
        state.platform.window.request_redraw();
        true
    }

    pub(super) fn invalidate_main_menu_movie_if_base_changed(state: &mut AppState) {
        let movie_base = crate::ui::main_menu_shell::movie_base_for_screen_width(
            state.renderer.gpu.config.width,
        );
        if state
            .frontend
            .main_menu_movie_identity
            .is_some_and(|identity| identity.base() != movie_base)
        {
            crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        }
    }
}

#[cfg(test)]
mod status_error_tests {
    use super::display_error;

    #[test]
    fn localized_os_text_is_dropped_and_the_code_kept() {
        // A zh-CN host reports ERROR_PATH_NOT_FOUND as the localized sentence
        // "the system cannot find the path specified"; the card cannot draw it.
        let text = display_error(
            "Could not load the game archives: IO error reading asset file: \
\u{7cfb}\u{7edf}\u{627e}\u{4e0d}\u{5230}\u{6307}\u{5b9a}\u{7684}\u{8def}\u{5f84}\u{3002} (os error 3)",
        );
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("os error 3"), "{text}");
        assert!(text.contains("game archives"), "{text}");
    }

    #[test]
    fn ascii_errors_pass_through() {
        let text =
            display_error("Required game-menu artwork SDBTNANM.SHP is missing or unreadable");
        assert_eq!(
            text,
            "Required game-menu artwork SDBTNANM.SHP is missing or unreadable"
        );
    }

    #[test]
    fn an_error_with_nothing_drawable_becomes_a_generic_line() {
        let text = display_error(
            "\u{7cfb}\u{7edf}\u{627e}\u{4e0d}\u{5230}\u{6307}\u{5b9a}\u{7684}\u{8def}\u{5f84}\u{3002}",
        );
        assert!(text.is_ascii(), "{text}");
        assert!(text.contains("see the log"), "{text}");
    }
}
