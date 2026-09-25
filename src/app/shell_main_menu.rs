use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmedQuitOwner {
    ShellController,
    EguiFallback,
}

/// Private operation seam shared by both confirmed main-menu quit owners.
/// Keeping the ordering here lets unit tests observe persistence dispatch
/// without constructing the window, renderer, or audio-backed `AppState`.
trait ConfirmedQuitOperations {
    fn persist_settings(&mut self);
    fn dismiss_confirmation(&mut self, owner: ConfirmedQuitOwner);
    fn start_cascade(&mut self);
}

struct AppStateConfirmedQuitOperations<'a> {
    state: &'a mut AppState,
}

impl ConfirmedQuitOperations for AppStateConfirmedQuitOperations<'_> {
    fn persist_settings(&mut self) {
        App::persist_settings_on_quit(self.state);
    }

    fn dismiss_confirmation(&mut self, owner: ConfirmedQuitOwner) {
        match owner {
            ConfirmedQuitOwner::ShellController => {
                App::close_exit_confirm_modal_from_controller(self.state);
            }
            ConfirmedQuitOwner::EguiFallback => {
                self.state.frontend.exit_confirm_modal = None;
            }
        }
    }

    fn start_cascade(&mut self) {
        App::start_quit_cascade(self.state);
    }
}

fn dispatch_confirmed_quit(
    operations: &mut impl ConfirmedQuitOperations,
    owner: ConfirmedQuitOwner,
) {
    operations.dismiss_confirmation(owner);
    operations.persist_settings();
    operations.start_cascade();
}

fn dispatch_shell_controller_confirmed_quit(operations: &mut impl ConfirmedQuitOperations) {
    dispatch_confirmed_quit(operations, ConfirmedQuitOwner::ShellController);
}

fn dispatch_egui_fallback_confirmed_quit(operations: &mut impl ConfirmedQuitOperations) {
    dispatch_confirmed_quit(operations, ConfirmedQuitOwner::EguiFallback);
}

impl crate::app::persistence::options::launcher::LauncherPreviewOperations for AppState {
    fn play_cue(&mut self, cue: crate::ui::main_menu_dialogs::options::LauncherCue) {
        App::play_launcher_options_cue(self, cue);
    }
    fn store_resolution(&mut self, width: i32, height: i32) {
        self.persistence.options_profile.screen_width = width;
        self.persistence.options_profile.screen_height = height;
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
        // Rust-native display invalidation for native `0x004AE450`. The live
        // presentation reads the projection above on the requested frame.
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
        self.state.persistence.options_profile.score_volume = value;
    }

    fn apply_score_output(&mut self, value: f32) {
        if let Some(player) = self.state.audio.music_player.as_mut() {
            player.set_volume(f64::from(value));
        }
    }

    fn queue_then_stop_score_zero(&mut self) {
        let now_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
            self.state,
            Instant::now(),
        );
        self.state.audio.queue_then_stop_score_zero(now_ms);
    }

    fn store_sound_volume(&mut self, value: f32) {
        self.state.persistence.options_profile.sound_volume = value;
    }

    fn apply_sound_output(&mut self, value: f32) {
        if let Some(player) = self.state.audio.sfx_player.as_mut() {
            player.set_sound_volume(f64::from(value));
        }
    }

    fn store_voice_volume(&mut self, value: f32) {
        self.state.persistence.options_profile.voice_volume = value;
    }

    fn apply_voice_output(&mut self, value: f32) {
        if let Some(player) = self.state.audio.sfx_player.as_mut() {
            player.set_voice_volume(f64::from(value));
        }
    }

    fn primary_dropped(&mut self) {
        debug_assert!(self.state.frontend.options_dialog.is_none());
    }

    fn persist_profile(&mut self) {
        crate::app::persistence::options::persist_options_profile(self.state);
    }

    fn prepare_network_settings(&mut self) {
        self.state
            .frontend
            .offline_skirmish_runtime
            .refresh_local_multiplayer_preferences();
    }

    fn route_network(&mut self) {
        log::info!(
            "Launcher Options Network child boundary reached; RT_DIALOG 0xD7 is not implemented"
        );
    }

    fn route_keyboard(&mut self) {
        crate::app::input::keyboard::open(self.state, crate::ui::shell::keyboard::KeyboardParent::Launcher);
    }

    fn reopen_parent(&mut self) {
        // Native caller blocks in A3. Our asynchronous child resumes this
        // continuation on close, after Back/save or Escape/current-file reload.
        if self.state.frontend.keyboard_dialog.is_none() {
            App::open_launcher_options_dialog(self.state);
        }
    }
}

impl App {
    /// A right-panel menu page (Single Player `0x100` or Movies & Credits
    /// `0x101`) owns the `MainMenu` screen.
    pub(super) fn menu_page_active(state: &AppState) -> bool {
        crate::app::frontend::menu_page_render::ActiveMenuPage::from_state(state).is_some()
    }

    /// The end-of-match score screen owns input whenever it has both a resolved
    /// model and the shell chrome to draw it with; without either, the result
    /// screen falls back to its egui form and egui keeps the input.
    pub(super) fn score_shell_active(state: &AppState) -> bool {
        matches!(state.frontend.screen, GameScreen::MissionResult { .. })
            && state.frontend.score_screen.is_some()
            && state.frontend.main_menu_shell_chrome.is_some()
    }

    fn score_shell_layout(state: &AppState) -> crate::ui::score_shell::ScoreShellLayout {
        crate::ui::score_shell::compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    }

    pub(super) fn handle_score_shell_mouse_move(state: &mut AppState) {
        let layout = Self::score_shell_layout(state);
        state.frontend.score_shell_state.continue_hovered = layout.hit_continue(
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        );
    }

    pub(super) fn handle_score_shell_mouse_down(state: &mut AppState) {
        let layout = Self::score_shell_layout(state);
        let inside = layout.hit_continue(
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        );
        state.frontend.score_shell_state.continue_pressed = inside;
        if inside {
            Self::play_main_menu_button_sound(state);
        }
    }

    /// Release inside the button leaves the score screen. This is the only exit:
    /// the native dialog is modal with one Continue button and dismisses to the
    /// shell, so there is no cancel path to mirror.
    pub(super) fn handle_score_shell_mouse_up(state: &mut AppState) {
        let layout = Self::score_shell_layout(state);
        let inside = layout.hit_continue(
            state.match_state.input.cursor_x.round() as i32,
            state.match_state.input.cursor_y.round() as i32,
        );
        let activated = state.frontend.score_shell_state.continue_pressed && inside;
        state.frontend.score_shell_state.continue_pressed = false;
        if activated {
            Self::leave_mission_result_screen(state);
        }
    }

    /// Shared teardown for both result-screen forms: flush the deterministic log
    /// while its simulation is still alive, hand the scenario stream back to the
    /// offline shell, retaining the runtime and presentation resources.
    pub(super) fn leave_mission_result_screen(state: &mut AppState) {
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        Self::capture_returned_skirmish_rng(state);
        state.match_state.startup.clear();
        state.match_state.scenario_elapsed_clock.reset();
        state.frontend.score_screen = None;
        state.frontend.score_shell_state = Default::default();
        state.frontend.screen = GameScreen::MainMenu;
        Self::enter_shell_window_mode(state);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    /// `0x497` enables Load Saved Game when the scan `0x00559C20` finds a
    /// save the list reader accepts; the list `0x005596A0` uses the same
    /// reader, so an enabled button always opens a non-empty list.
    fn refresh_single_player_load_state(state: &mut AppState) {
        state
            .frontend
            .single_player_shell_state
            .load_saved_game_enabled = !state.persistence.repository.browser_entries().is_empty();
    }

    pub(super) fn open_single_player_shell(state: &mut AppState) {
        Self::enter_shell_window_mode(state);
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

    fn enter_native_skirmish_from_single_player(state: &mut AppState) {
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
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::Skirmish {
            return_to_single_player: true,
        };
        state
            .frontend
            .skirmish_shell_state
            .pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        Self::ensure_active_cooperative_shell_selection(state);
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
        state.frontend.skirmish_shell_state.trackbar_drag = None;
        state
            .frontend
            .skirmish_shell_state
            .pressed_owner_draw_button = None;
        crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        state.frontend.skirmish_preview_texture = None;
        Self::open_single_player_shell(state);
    }

    fn draw_skirmish_shell_dev_toggle(ctx: &egui::Context, enabled: &mut bool) -> bool {
        let mut changed = false;
        egui::Window::new("Developer")
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .collapsible(true)
            .resizable(false)
            .show(ctx, |ui| {
                changed = ui
                    .checkbox(enabled, "Experimental Skirmish Shell")
                    .on_hover_text("Switches the setup screen to the research shell renderer.")
                    .changed();
            });
        changed
    }

    pub(super) fn render_egui_main_menu_fallback(
        state: &mut AppState,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        event_loop: &ActiveEventLoop,
    ) -> Result<()> {
        transitions::clear_screen(encoder, view);
        state.renderer.egui.begin_frame(&state.platform.window);
        let action = main_menu::draw_main_menu_with_maps(
            &state.renderer.egui.ctx,
            &state.frontend.available_maps,
            &mut state.frontend.skirmish_settings,
        );
        let mut dev_shell_enabled = state.frontend.dev_skirmish_shell_enabled;
        let dev_shell_changed =
            Self::draw_skirmish_shell_dev_toggle(&state.renderer.egui.ctx, &mut dev_shell_enabled);
        if dev_shell_changed {
            Self::enter_shell_window_mode(state);
            if dev_shell_enabled {
                if Self::ensure_skirmish_shell_chrome(state) {
                    state.frontend.dev_skirmish_shell_enabled = true;
                } else {
                    state.frontend.dev_skirmish_shell_enabled = false;
                    log::warn!(
                        "Development Skirmish shell unavailable; retaining the current shell"
                    );
                }
            } else {
                state.frontend.dev_skirmish_shell_enabled = false;
                state
                    .frontend
                    .skirmish_shell_state
                    .pressed_owner_draw_button = None;
            }
        }
        // Confirm modal can be open over the legacy egui menu too; draw it in
        // the same frame so its buttons receive input. This degraded egui path has
        // no SHP shell, so the quit-confirm renders as the egui card here.
        let confirm = Self::draw_main_menu_dialogs(state, true);
        // Degraded fallback (shell chrome failed to load) has no SHP cursor of
        // its own, so keep the OS cursor visible here rather than hiding it and
        // leaving the egui menu with no pointer at all.
        state.renderer.egui.end_frame_and_render(
            &state.renderer.gpu,
            encoder,
            view,
            &state.platform.window,
            false,
        );
        if confirm {
            event_loop.exit();
            return Ok(());
        }
        Self::handle_main_menu_action(state, action, event_loop);
        Ok(())
    }

    fn handle_main_menu_action(
        state: &mut AppState,
        action: main_menu::MenuAction,
        event_loop: &ActiveEventLoop,
    ) {
        let _ = event_loop;
        match action {
            main_menu::MenuAction::StartSelected => Self::start_selected_skirmish(state),
            // Route through the same confirm message box for consistency with
            // the native shell; the game does not quit on the first click.
            main_menu::MenuAction::Exit => Self::open_exit_confirm_modal(state),
            main_menu::MenuAction::None => {}
        }
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
    /// profile, current-monitor dimension pairs, live CSF table, and frozen
    /// process-start common audio gate. No display mode is applied here.
    pub(crate) fn open_launcher_options_dialog(state: &mut AppState) {
        // State 5 destroys `0xE2` (and its RA2TS static) before Options runs;
        // closing Options creates a new `0xE2` with its own entry slide.
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        Self::ensure_skirmish_shell_chrome(state);
        state.frontend.launcher_options_presentation = Default::default();
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
        let mode_pairs: Vec<_> = state
            .platform
            .window
            .current_monitor()
            .map(|monitor| {
                monitor
                    .video_modes()
                    .map(|mode| {
                        let size = mode.size();
                        (size.width, size.height)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let dialog = launcher_dialog_from_profile(
            &state.persistence.options_profile,
            labels,
            mode_pairs,
            state.audio.launcher_audio_available,
        );
        state.frontend.options_dialog = Some(dialog);
    }

    fn play_launcher_options_cue(
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
        if let Some(result) = output.result {
            let mut operations = AppStateLauncherParentOperations { state };
            crate::app::persistence::options::launcher::dispatch_launcher_parent_result(
                &mut operations, dialog, result,
            );
        } else {
            state.frontend.options_dialog = Some(dialog);
        }
    }

    pub(crate) fn handle_launcher_options_mouse(state: &mut AppState, down: Option<bool>) {
        let Some(mut dialog) = state.frontend.options_dialog.take() else { return; };
        let (x, y) = (state.match_state.input.cursor_x as i32, state.match_state.input.cursor_y as i32);
        let (w, h) = (state.render_width() as i32, state.render_height() as i32);
        match down {
            Some(true) => dialog.shell_mouse_down(x, y, w, h),
            Some(false) => dialog.shell_mouse_up(x, y, w, h),
            None => dialog.shell_mouse_move(x, y, w, h),
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
        let layout = crate::ui::main_menu_shell::compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let feed = Self::main_menu_shell_button_feed(&layout);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
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
        let layout = crate::ui::main_menu_shell::compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let feed = Self::main_menu_shell_button_feed(&layout);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        state
            .frontend
            .shell_controller
            .ensure_active(crate::ui::shell::descriptor::DialogId(0x00E2), false);
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        Self::mirror_shell_controller_to_main_menu(state);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.hover_repaint();
    }

    pub(super) fn handle_main_menu_shell_mouse_up(
        state: &mut AppState,
        _event_loop: &ActiveEventLoop,
    ) {
        let layout = crate::ui::main_menu_shell::compute_layout(
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let feed = Self::main_menu_shell_button_feed(&layout);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
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
                | MainMenuShellAction::ExitGame => {
                    Self::leave_shell_dialog(state, ShellExitThen::MainMenu(action))
                }
                _ => Self::handle_main_menu_shell_action(state, action),
            }
        }
    }

    /// The quit-confirm (0x120) modal's OK/Cancel button feed: resource-id'd pixel
    /// rects from the centered modal layout at the live screen size.
    fn exit_confirm_modal_feed(state: &AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        use crate::ui::shell::layout::LaidOutControl;
        use crate::ui::shell::modal;
        let layout = modal::quit_confirm_layout(
            state.renderer.gpu.config.width as i32,
            state.renderer.gpu.config.height as i32,
        );
        vec![
            LaidOutControl {
                id: modal::control::OK,
                rect: layout.ok,
            },
            LaidOutControl {
                id: modal::control::CANCEL,
                rect: layout.cancel,
            },
        ]
    }

    pub(super) fn handle_exit_confirm_modal_mouse_down(state: &mut AppState) {
        let feed = Self::exit_confirm_modal_feed(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        if state.frontend.shell_controller.top_id()
            != Some(crate::ui::shell::descriptor::DialogId(0x0120))
        {
            return;
        }
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
    }

    pub(super) fn handle_exit_confirm_modal_mouse_up(state: &mut AppState) {
        let feed = Self::exit_confirm_modal_feed(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        if state.frontend.shell_controller.top_id()
            != Some(crate::ui::shell::descriptor::DialogId(0x0120))
        {
            return;
        }
        let activated = state.frontend.shell_controller.on_pointer_up(x, y, &feed);
        match activated {
            // OK -> quit (result 0). Dismiss the confirmation first, persist
            // settings to RA2MD.INI, then run the graceful cascade (music fade
            // → trailing-voice wait → hard stop → exit) via render_frame instead
            // of exiting immediately. The screen fade-to-black is sub-step 4b-ii-b.
            Some(id) if id == crate::ui::shell::modal::control::OK => {
                let mut operations = AppStateConfirmedQuitOperations { state };
                dispatch_shell_controller_confirmed_quit(&mut operations);
            }
            // Cancel (control 2) -> stay; close the modal via the controller
            // pop (D-B3) so mouse and Esc converge on the same teardown.
            Some(id) if id == crate::ui::shell::modal::control::CANCEL => {
                Self::close_exit_confirm_modal_from_controller(state);
                state.platform.window.request_redraw();
            }
            _ => {}
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
        let layout = crate::ui::shell::menu_page::compute_layout(
            spec,
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
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
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
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
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        // Hover is enable-unfiltered: a disabled button still drives 0x695.
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        // Every hover message repaints the status line (0x00615EF7).
        state.frontend.shell_status_line.hover_repaint();
    }

    pub(super) fn handle_menu_page_mouse_up(state: &mut AppState) {
        use crate::app::frontend::menu_page_render::ActiveMenuPage;
        let Some(page) = ActiveMenuPage::from_state(state) else {
            return;
        };
        let feed = Self::prepare_menu_page_input(state, page);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
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
            ShellExitThen::LoadSavedGameBack => Self::commit_load_saved_game_back(state),
        }
    }

    /// Active-retail `ShellButtonSlideSound` completion hook. The stock key is
    /// empty in both rules layers, so this named edge intentionally has no
    /// audible output in the exact stock route.
    pub(crate) fn play_shell_slide_completion_sound(_state: &mut AppState) {}

    pub(super) fn maintain_main_menu_intro(state: &mut AppState) {
        if state.frontend.screen != GameScreen::MainMenu || state.frontend.quit_cascade.is_some() {
            return;
        }
        // Play_Movie runs no audio pump; the paused theme resumes afterwards.
        if state.frontend.fullscreen_movie.is_some() {
            return;
        }
        let now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        // Show_Credits' Call_Back pumps Theme AI (starting the queued CREDITS
        // song) without the menu loop's Play_Song(INTRO).
        if state.frontend.credits_roll.is_some() {
            if let Some(assets) = state.process_assets.manager() {
                state.audio.update_theme(assets, now_ms);
            }
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
        let (Some(sfx), Some(assets)) =
            (&mut state.audio.sfx_player, state.process_assets.manager())
        else {
            return;
        };
        sfx.play_sound(
            sound_id,
            &state.audio.sound_registry,
            assets,
            &state.audio.audio_indices,
        );
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
            MainMenuShellAction::ExitGame => Self::open_exit_confirm_modal(state),
            MainMenuShellAction::SinglePlayer => {
                Self::open_single_player_shell(state);
            }
            MainMenuShellAction::Options => {
                Self::open_launcher_options_dialog(state);
            }
            MainMenuShellAction::MoviesAndCredits => {
                Self::open_movies_credits_page(state);
            }
            MainMenuShellAction::WwOnline | MainMenuShellAction::Network => {
                log::info!(
                    "Main-menu shell action {:?} is preserved but downstream dialog is not implemented yet",
                    action
                );
            }
        }
    }

    /// Open the Exit-Game confirm message box, resolving its labels from CSF.
    fn open_exit_confirm_modal(state: &mut AppState) {
        // State 6 runs after 0xE2 is destroyed: it paints the empty shell
        // backdrop (`0x0052FEC0`) and asks; Cancel builds a new 0xE2 (state
        // 0x12, `0x0052DE39..0x0052DE42`).
        crate::app::frontend::main_menu_shell_render::clear_ra2ts_movie_session(state);
        crate::app::frontend::shell_transition::invalidate_main_menu_dialog_instance(state);
        let csf = |key: &str, fallback: &str| Self::csf_label(state, key, fallback);
        let modal = crate::ui::main_menu_dialogs::ExitConfirmModalState::open(&csf);
        // The SHP modal sources PUDLGBGN/MNBTTN from the skirmish chrome atlas; load
        // it on demand so the quit-confirm renders straight from the main menu.
        let _ = Self::ensure_skirmish_shell_chrome(state);
        // Host the modal as a TRUE LIFO push over the active shell (D-B3):
        // teardown pops back to it with focus restored. (ensure_active would
        // reset_to-clobber the stack — the prior "0x120 over 0xE2" comment
        // described behavior that never happened.)
        if state.frontend.shell_controller.top_id()
            != Some(crate::ui::shell::descriptor::DialogId(0x0120))
        {
            state
                .frontend
                .shell_controller
                .push(crate::ui::shell::descriptor::DialogId(0x0120), true);
        }
        state.frontend.exit_confirm_modal = Some(modal);
    }

    /// Whether any main-menu modal dialog is currently open. Used to route
    /// keyboard/mouse to the modal first.
    pub(crate) fn main_menu_dialog_open(state: &AppState) -> bool {
        state.main_menu_dialog_open()
    }

    /// Close the egui-only main-menu dialogs (options — never on the
    /// controller stack). The exit-confirm modal closes through
    /// close_exit_confirm_modal_from_controller (D-B3).
    pub(crate) fn close_main_menu_dialogs(state: &mut AppState) {
        state.frontend.exit_confirm_modal = None;
        state.frontend.options_dialog = None;
    }

    /// Controller-routed exit-confirm teardown (D-B3): dismiss the modal UI
    /// state, then LIFO-pop its 0x120 instance so focus returns to the shell
    /// beneath. Mirrors `close_validation_modal_from_controller` — every Esc
    /// and mouse close path converges here.
    pub(super) fn close_exit_confirm_modal_from_controller(state: &mut AppState) {
        state.frontend.exit_confirm_modal = None;
        if state.frontend.shell_controller.top_id()
            == Some(crate::ui::shell::descriptor::DialogId(0x0120))
        {
            state.frontend.shell_controller.pop();
        }
    }

    pub(super) fn route_exit_confirm_modal_key(state: &mut AppState, key: ShellKey) -> bool {
        if state.frontend.exit_confirm_modal.is_none() {
            return false;
        }
        if !state.frontend.shell_controller.on_key(key) {
            return false;
        }
        Self::close_exit_confirm_modal_from_controller(state);
        state.platform.window.request_redraw();
        true
    }

    /// Draw whichever main-menu modal dialog is open in the current egui frame
    /// and apply its outcome. Returns `true` when the player has confirmed
    /// quitting, so the caller should exit the event loop.
    /// Draw whichever egui main-menu modal dialog is open. `render_exit_confirm_egui`
    /// is true only on the degraded egui fallback path (where the SHP shell — and
    /// thus the SHP quit-confirm modal — is unavailable); the normal SHP shell path
    /// passes false and renders the quit-confirm as an SHP overlay instead.
    pub(super) fn draw_main_menu_dialogs(
        state: &mut AppState,
        render_exit_confirm_egui: bool,
    ) -> bool {
        use crate::ui::main_menu_dialogs as dialogs;

        if render_exit_confirm_egui {
            if let Some(modal) = state.frontend.exit_confirm_modal.clone() {
                match dialogs::draw_exit_confirm_modal(&state.renderer.egui.ctx, &modal) {
                    dialogs::ExitConfirmAction::Confirm => {
                        // Dismiss the confirmation first, persist settings, then
                        // start the graceful cascade. Return false (not true) so
                        // exit is owned by the cascade; this degraded egui-fallback
                        // path runs the audio phases (the SHP fade overlay is
                        // unavailable here).
                        let mut operations = AppStateConfirmedQuitOperations { state };
                        dispatch_egui_fallback_confirmed_quit(&mut operations);
                        return false;
                    }
                    dialogs::ExitConfirmAction::Cancel => {
                        state.frontend.exit_confirm_modal = None;
                    }
                    dialogs::ExitConfirmAction::None => {}
                }
                return false;
            }
        }

        if let Some(mut dialog) = state.frontend.options_dialog.take() {
            debug_assert_eq!(
                dialog.launcher_audio_available(),
                state.audio.launcher_audio_available
            );
            let output = dialogs::options::draw_launcher_options_dialog(
                &state.renderer.egui.ctx,
                &mut dialog,
            );
            Self::dispatch_launcher_options_output(state, dialog, output);
            return false;
        }

        false
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
mod confirmed_quit_transaction_tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum QuitEvent {
        Persist,
        Dismiss(ConfirmedQuitOwner),
        Cascade,
    }

    #[derive(Default)]
    struct RecordingQuitOperations {
        events: Vec<QuitEvent>,
        persists: usize,
        dismissals: usize,
        cascades: usize,
    }

    impl ConfirmedQuitOperations for RecordingQuitOperations {
        fn persist_settings(&mut self) {
            self.persists += 1;
            self.events.push(QuitEvent::Persist);
        }

        fn dismiss_confirmation(&mut self, owner: ConfirmedQuitOwner) {
            self.dismissals += 1;
            self.events.push(QuitEvent::Dismiss(owner));
        }

        fn start_cascade(&mut self) {
            self.cascades += 1;
            self.events.push(QuitEvent::Cascade);
        }
    }

    fn assert_single_dispatch(operations: &RecordingQuitOperations, owner: ConfirmedQuitOwner) {
        assert_eq!(operations.persists, 1);
        assert_eq!(operations.dismissals, 1);
        assert_eq!(operations.cascades, 1);
        assert_eq!(
            operations.events,
            [
                QuitEvent::Dismiss(owner),
                QuitEvent::Persist,
                QuitEvent::Cascade,
            ]
        );
    }

    #[test]
    fn shell_controller_confirmed_quit_dispatches_one_profile_write() {
        let mut operations = RecordingQuitOperations::default();

        dispatch_shell_controller_confirmed_quit(&mut operations);

        assert_single_dispatch(&operations, ConfirmedQuitOwner::ShellController);
    }

    #[test]
    fn egui_fallback_confirmed_quit_dispatches_one_profile_write() {
        let mut operations = RecordingQuitOperations::default();

        dispatch_egui_fallback_confirmed_quit(&mut operations);

        assert_single_dispatch(&operations, ConfirmedQuitOwner::EguiFallback);
    }
}
