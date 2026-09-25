use super::*;

/// Where Skirmish Back leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkirmishBackOutcome {
    /// The shell continues (a teardown slide or an immediate close).
    Leaving,
    /// The developer-only Skirmish launch has no shell to return to.
    ExitApp,
}

impl App {
    pub(super) fn dev_skirmish_shell_enabled() -> bool {
        std::env::var(DEV_SKIRMISH_SHELL_ENV)
            .ok()
            .is_some_and(|value| {
                let value = value.trim();
                !value.is_empty()
                    && value != "0"
                    && !value.eq_ignore_ascii_case("false")
                    && !value.eq_ignore_ascii_case("off")
                    && !value.eq_ignore_ascii_case("no")
            })
    }

    pub(super) fn native_skirmish_shell_active(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::MainMenu
            && (state.frontend.shell_route.skirmish() || state.frontend.dev_skirmish_shell_enabled)
    }
    fn skirmish_shell_layout(state: &AppState) -> crate::ui::skirmish_shell::SkirmishShellLayout {
        crate::ui::skirmish_shell::compute_layout(state.render_width(), state.render_height())
    }

    fn skirmish_choose_map_layout(
        state: &AppState,
    ) -> crate::ui::skirmish_shell::ChooseMapModalLayout {
        crate::ui::skirmish_shell::compute_choose_map_modal_layout(
            state.render_width(),
            state.render_height(),
        )
    }

    fn validation_modal_dialog_id() -> crate::ui::shell::descriptor::DialogId {
        crate::ui::shell::descriptor::DialogId(0x00CE)
    }

    fn validation_modal_feed(state: &AppState) -> Vec<crate::ui::shell::layout::LaidOutControl> {
        let layout = crate::ui::shell::modal::body_ok_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        );
        vec![crate::ui::shell::layout::LaidOutControl {
            id: crate::ui::shell::modal::control::OK,
            rect: layout.ok,
        }]
    }

    pub(super) fn shell_key_for_code(code: KeyCode) -> Option<ShellKey> {
        match code {
            KeyCode::Tab => Some(ShellKey::Tab),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(ShellKey::Enter),
            KeyCode::Escape => Some(ShellKey::Escape),
            _ => None,
        }
    }

    fn close_native_skirmish_shell(state: &mut AppState) {
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        state.frontend.dev_skirmish_shell_enabled = false;
        state.frontend.skirmish_shell_state.choose_map_modal = None;
        state.frontend.skirmish_shell_state.validation_modal = None;
        state.frontend.skirmish_shell_state.open_combo_dropdown = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_drag = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_press = None;
        state.frontend.skirmish_shell_state.trackbar_drag = None;
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        state.frontend.skirmish_preview_texture = None;
        Self::enter_shell_window_mode(state);
    }

    fn selected_skirmish_mode_is_cooperative(state: &AppState, mode_id: i32) -> bool {
        crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, mode_id)
            .is_some_and(|mode| mode.override_file.eq_ignore_ascii_case("MPCoopMD.ini"))
    }

    fn selected_shell_map_file(state: &AppState) -> Option<String> {
        state
            .frontend.scenario_catalog.shell_maps()
            .get(state.frontend.skirmish_shell_state.selected_map_idx)
            .map(|map| map.file_name.clone())
    }

    fn apply_selected_shell_map_file(state: &mut AppState, file_name: &str) -> bool {
        let Some(map_idx) = state
            .frontend.scenario_catalog.shell_maps()
            .iter()
            .position(|map| map.file_name.eq_ignore_ascii_case(file_name))
        else {
            log::warn!("No loadable Skirmish map entry exists for {file_name}");
            return false;
        };
        Self::apply_selected_shell_map_index(state, map_idx)
    }

    /// One production mutation point for shell selection plus retained-RMG
    /// identity invalidation, whether selection came from Cooperative repair or
    /// the Choose Map dialog.
    fn apply_selected_shell_map_index(state: &mut AppState, map_idx: usize) -> bool {
        let Some(file_name) = state
            .frontend.scenario_catalog.shell_maps()
            .get(map_idx)
            .map(|map| map.file_name.clone())
        else {
            return false;
        };
        crate::ui::skirmish_shell::accept_selected_map(
            &mut state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            map_idx,
        );
        state.frontend.random_map_retention.select_map(&file_name);
        if let Some(legacy_idx) = state
            .frontend.available_maps
            .iter()
            .position(|map| map.file_name.eq_ignore_ascii_case(&file_name))
        {
            state.frontend.skirmish_settings.selected_map_idx = legacy_idx;
        }
        state.frontend.skirmish_preview_texture = None;
        true
    }

    pub(super) fn ensure_active_cooperative_shell_selection(state: &mut AppState) {
        if !Self::selected_skirmish_mode_is_cooperative(
            state,
            state.frontend.skirmish_shell_state.selected_mode_id,
        ) {
            return;
        }
        let Some(file_name) = Self::selected_shell_map_file(state) else {
            return;
        };
        let chosen_map = match state
            .frontend.offline_skirmish_runtime
            .ensure_cooperative_selection(&file_name, state.frontend.scenario_catalog.shell_maps())
        {
            Ok(chosen_map) => chosen_map,
            Err(err) => {
                log::warn!("Could not bind Cooperative shell progress: {err}");
                None
            }
        };
        if let Some(chosen_map) = chosen_map {
            Self::apply_selected_shell_map_file(state, &chosen_map);
        }
    }

    fn ensure_active_cooperative_modal_selection(state: &mut AppState) {
        let Some(modal) = state.frontend.skirmish_shell_state.choose_map_modal.as_ref() else {
            return;
        };
        let mode_id = modal.selected_mode_id;
        let record_index = modal.selected_record_index();
        if !Self::selected_skirmish_mode_is_cooperative(state, mode_id) {
            return;
        }
        let Some(file_name) = record_index
            .and_then(|index| state.frontend.scenario_catalog.records().get(index))
            .map(|record| record.file_name.clone())
        else {
            return;
        };
        if let Err(err) = state
            .frontend.offline_skirmish_runtime
            .ensure_cooperative_selection(&file_name, state.frontend.scenario_catalog.shell_maps())
        {
            log::warn!("Could not bind Cooperative Choose Map progress: {err}");
        }
    }

    fn sync_legacy_skirmish_settings_from_shell(state: &mut AppState) {
        let selected_file = Self::selected_shell_map_file(state);
        let mut settings = crate::ui::skirmish_shell::launch_settings(&state.frontend.skirmish_shell_state);
        settings.selected_map_idx = selected_file
            .as_deref()
            .and_then(|file_name| {
                state
                    .frontend.available_maps
                    .iter()
                    .position(|map| map.file_name.eq_ignore_ascii_case(file_name))
            })
            .unwrap_or(0);
        state.frontend.skirmish_settings = settings;
    }

    /// Skirmish Back (`0x5C0`): the proc packs the session like Start does
    /// (`0x006ACEE0`), then the runner tears the dialog down. From the Single
    /// Player route that is the teardown slide back to `0x100`; the developer
    /// routes close at once.
    pub(super) fn handle_skirmish_back(state: &mut AppState) -> SkirmishBackOutcome {
        match crate::ui::skirmish_shell::pack_launch_session_without_start_validation(
            &state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            &state.frontend.skirmish_modes,
        ) {
            Ok(raw_session) => {
                if let Err(err) = state
                    .frontend
                    .offline_skirmish_runtime
                    .close_shell_transaction(
                        &state.frontend.skirmish_shell_state,
                        state.frontend.scenario_catalog.shell_maps(),
                        &state.frontend.skirmish_modes,
                        &raw_session,
                    )
                {
                    // Invalid Cooperative content has no parity-safe
                    // retry cap. Keep Back usable, surface the malformed
                    // data, and retain every draw consumed before error.
                    log::error!("Could not complete Cooperative Back randomization: {err}");
                }
            }
            Err(err) => {
                log::warn!("Could not pack raw Skirmish Back session: {err:?}");
            }
        }
        if state
            .frontend
            .shell_route
            .skirmish_returns_to_single_player()
        {
            Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::SkirmishBack,
            );
            SkirmishBackOutcome::Leaving
        } else if Self::native_skirmish_shell_active(state) {
            Self::close_native_skirmish_shell(state);
            state.frontend.offline_skirmish_runtime.persist_snapshot();
            SkirmishBackOutcome::Leaving
        } else {
            state.frontend.offline_skirmish_runtime.persist_snapshot();
            SkirmishBackOutcome::ExitApp
        }
    }

    /// Skirmish Start after the teardown slide: `0x006AE2C0` destroys the
    /// dialog, writes the session fields (`0x006990A0`) and returns true, and
    /// Main_Game launches the scenario.
    pub(super) fn commit_skirmish_start(
        state: &mut AppState,
        session: crate::skirmish_launch::SkirmishLaunchSession,
    ) {
        Self::teardown_skirmish_shell_for_start(state);
        state.frontend.offline_skirmish_runtime.persist_snapshot();
        Self::start_skirmish_session(state, session);
    }

    /// Skirmish Back after the teardown slide: the session fields are written
    /// (`0x006990A0`), then state 1 builds the Single Player page.
    pub(super) fn commit_skirmish_back(state: &mut AppState) {
        state.frontend.offline_skirmish_runtime.persist_snapshot();
        Self::return_from_skirmish_to_single_player_shell(state);
    }

    pub(super) fn teardown_skirmish_shell_for_start(state: &mut AppState) {
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        state.frontend.skirmish_shell_state.choose_map_modal = None;
        state.frontend.skirmish_shell_state.validation_modal = None;
        state.frontend.skirmish_shell_state.open_combo_dropdown = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_drag = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_press = None;
        state.frontend.skirmish_shell_state.trackbar_drag = None;
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        state.frontend.skirmish_preview_texture = None;
    }

    pub(super) fn start_selected_skirmish(state: &mut AppState) {
        let map_name = state
            .frontend.available_maps
            .get(state.frontend.skirmish_settings.selected_map_idx)
            .map(|m| m.file_name.clone())
            .unwrap_or_else(|| "auto".to_string());
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::MainMenu;
        state.frontend.shell_first_paint_slide = None;
        let request = crate::app::loading::pump::LoadingRequest::generic_map_load(
            map_name,
            state.frontend.skirmish_settings.clone(),
        );
        crate::app::loading::pump::begin_loading(state, request);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    fn start_skirmish_session(
        state: &mut AppState,
        session: crate::skirmish_launch::SkirmishLaunchSession,
    ) {
        let accepted_random_map = state
            .frontend.random_map_retention
            .take_acceptance_for_loading(session.selected_map_file.as_deref());
        let request = match crate::match_bootstrap::classify_startup_session(&session) {
            crate::match_bootstrap::StartupSessionClassification::AcceptedExplicitFixedBattle(
                accepted,
            ) => {
                let correlation = match crate::match_bootstrap::allocate_match_correlation(
                    &mut state.frontend.next_match_correlation,
                ) {
                    Ok(correlation) => correlation,
                    Err(err) => {
                        log::error!("Cannot start accepted match: {err}");
                        return;
                    }
                };
                let mut clock = crate::match_bootstrap::OrdinaryMatchSeedClock;
                let startup = crate::match_bootstrap::prepare_match_startup(
                    correlation,
                    accepted,
                    &mut clock,
                );
                crate::app::loading::pump::LoadingRequest::accepted_skirmish(
                    startup,
                    state.frontend.skirmish_settings.clone(),
                )
            }
            crate::match_bootstrap::StartupSessionClassification::UnverifiedLegacy(reason) => {
                log::warn!("Skirmish startup uses unverified compatibility path: {reason:?}");
                let mut clock = crate::match_bootstrap::OrdinaryMatchSeedClock;
                let seed = crate::match_bootstrap::read_match_seed(&mut clock);
                crate::app::loading::pump::LoadingRequest::unverified_legacy_skirmish(
                    session,
                    seed,
                    state.frontend.skirmish_settings.clone(),
                )
            }
        }
        .with_accepted_random_map(accepted_random_map);
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        // GameMode stays 5 through the game, so the shell resumes on a new
        // `0x102` afterwards (`App::resume_shell_after_match`).
        state.frontend.shell_route = crate::app::shell_route::ShellRoute::Skirmish {
            return_to_single_player: true,
        };
        state.frontend.shell_first_paint_slide = None;
        state.frontend.skirmish_preview_texture = None;
        crate::app::loading::pump::begin_loading(state, request);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    pub(crate) fn ensure_skirmish_shell_chrome(state: &mut AppState) -> bool {
        if state.frontend.skirmish_shell_chrome.is_some() {
            return true;
        }

        let Some(assets) = state.process_assets.manager() else {
            log::warn!(
                "Could not prepare Skirmish shell chrome: process asset manager is unavailable"
            );
            return false;
        };

        state.frontend.skirmish_shell_chrome =
            crate::render::skirmish_shell_chrome::build_skirmish_shell_chrome_atlas(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                assets,
            );
        let ready = state.frontend.skirmish_shell_chrome.is_some();
        if !ready {
            log::warn!(
                "Could not prepare Skirmish shell chrome from the registered retail archives"
            );
        }
        ready
    }

    pub(super) fn handle_skirmish_shell_action(
        state: &mut AppState,
        action: crate::ui::skirmish_shell::SkirmishShellAction,
        event_loop: &ActiveEventLoop,
    ) {
        let action = crate::ui::skirmish_shell::apply_action(
            &mut state.frontend.skirmish_shell_state,
            action,
            state.frontend.scenario_catalog.shell_maps(),
        );

        match action {
            crate::ui::skirmish_shell::SkirmishShellAction::StartGame => {
                Self::start_game_from_shell(state);
            }
            crate::ui::skirmish_shell::SkirmishShellAction::BackOrExit => {
                if Self::handle_skirmish_back(state) == SkirmishBackOutcome::ExitApp {
                    event_loop.exit();
                }
            }
            crate::ui::skirmish_shell::SkirmishShellAction::ChooseMap => {
                // `0x102` slides out (not torn down) before `0x6B` runs.
                Self::leave_shell_dialog(
                    state,
                    crate::app::frontend::shell_transition::ShellExitThen::SkirmishChooseMap,
                );
            }
            crate::ui::skirmish_shell::SkirmishShellAction::None
            | crate::ui::skirmish_shell::SkirmishShellAction::SelectColor(_)
            | crate::ui::skirmish_shell::SkirmishShellAction::SelectMap(_) => {}
        }
    }

    /// Start Game on `0x102` (`0x006ACEE0` case `0x617`): validate and pack
    /// the session, then slide the dialog out before the launch.
    pub(crate) fn start_game_from_shell(state: &mut AppState) {
        match crate::ui::skirmish_shell::launch_session(
            &state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            &state.frontend.skirmish_modes,
        ) {
            Ok(raw_session) => {
                match state
                    .frontend
                    .offline_skirmish_runtime
                    .close_shell_transaction(
                        &state.frontend.skirmish_shell_state,
                        state.frontend.scenario_catalog.shell_maps(),
                        &state.frontend.skirmish_modes,
                        &raw_session,
                    ) {
                    Ok(resolved_session) => {
                        // 0x006ACEE0 packs the session and writes
                        // result 0x617; the runner (0x006AE2C0) then
                        // slides the dialog out before the launch.
                        Self::sync_legacy_skirmish_settings_from_shell(state);
                        Self::leave_shell_dialog(
                            state,
                            crate::app::frontend::shell_transition::ShellExitThen::SkirmishStart(
                                Box::new(resolved_session),
                            ),
                        );
                    }
                    Err(err) => {
                        log::error!("Could not resolve Cooperative shell assignments: {err}");
                        state.platform.window.request_redraw();
                    }
                }
            }
            Err(err) => {
                if let Some(modal) = Self::skirmish_validation_modal_for_error(state, &err) {
                    Self::show_skirmish_validation_modal(state, modal);
                    state.platform.window.request_redraw();
                } else {
                    log::warn!("Could not start skirmish shell session: {err:?}");
                }
            }
        }
    }

    fn skirmish_shell_label(state: &AppState, key: &str, fallback: &str) -> String {
        Self::csf_label(state, key, fallback)
    }

    fn skirmish_validation_modal_for_error(
        state: &AppState,
        err: &crate::skirmish_launch::LaunchValidationError,
    ) -> Option<crate::ui::skirmish_shell::SkirmishValidationModalState> {
        let ok = Self::skirmish_shell_label(state, "TXT_OK", "OK");
        let message = match err {
            crate::skirmish_launch::LaunchValidationError::MapCapacityExceeded {
                capacity, ..
            } => {
                let template = Self::skirmish_shell_label(
                    state,
                    "TXT_SCENARIO_TOO_SMALL",
                    "This map has a %d player max. The max includes human and computer players.",
                );
                template.replace("%d", &capacity.to_string())
            }
            crate::skirmish_launch::LaunchValidationError::NoEnabledOpponent => {
                Self::skirmish_shell_label(
                    state,
                    "TXT_NEED_AT_LEAST_TWO_PLAYERS",
                    "You need at least two players to start the game!",
                )
            }
            crate::skirmish_launch::LaunchValidationError::SameExplicitTeam { .. } => {
                Self::skirmish_shell_label(
                    state,
                    "TXT_CANNOT_ALLY",
                    "Must have more than one team to start a game!",
                )
            }
            _ => return None,
        };
        Some(crate::ui::skirmish_shell::SkirmishValidationModalState::new(message, ok))
    }

    fn show_skirmish_validation_modal(
        state: &mut AppState,
        modal: crate::ui::skirmish_shell::SkirmishValidationModalState,
    ) {
        state.frontend.skirmish_shell_state.validation_modal = Some(modal);
        state
            .frontend.shell_controller
            .ensure_active(Self::validation_modal_dialog_id(), true);
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_state.open_combo_dropdown = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_drag = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_press = None;
        state.frontend.skirmish_shell_state.trackbar_drag = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
    }

    pub(super) fn open_choose_map_modal(state: &mut AppState) {
        state.frontend.skirmish_shell_state.open_combo_dropdown = None;
        state.frontend.skirmish_shell_state.dropdown_scroll_drag = None;
        state.frontend.skirmish_shell_state.trackbar_drag = None;
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        crate::ui::skirmish_shell::clear_status_help_text(&mut state.frontend.skirmish_shell_state);
        let current_record_index = Self::current_choose_map_record_index(state);
        let layout = Self::skirmish_choose_map_layout(state);
        state.frontend.skirmish_shell_state.choose_map_modal =
            Some(crate::ui::skirmish_shell::ChooseMapModalState::open(
                state.frontend.skirmish_shell_state.selected_mode_id,
                current_record_index,
                &state.frontend.skirmish_modes,
                state.frontend.scenario_catalog.records(),
                &layout,
            ));
        Self::ensure_active_cooperative_modal_selection(state);
    }

    fn current_choose_map_record_index(state: &AppState) -> Option<usize> {
        let file_name = state
            .frontend.scenario_catalog.shell_maps()
            .get(state.frontend.skirmish_shell_state.selected_map_idx)?
            .file_name
            .as_str();
        state
            .frontend.scenario_catalog.records()
            .iter()
            .position(|record| record.file_name.eq_ignore_ascii_case(file_name))
    }

    pub(super) fn close_choose_map_modal(state: &mut AppState) {
        if let Some(modal) = state.frontend.skirmish_shell_state.choose_map_modal.as_mut() {
            modal.pressed_button = None;
        }
        state.frontend.skirmish_shell_state.choose_map_modal = None;
        state.frontend.skirmish_shell_state.pressed_owner_draw_button = None;
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
    }

    pub(super) fn commit_choose_map_selection(
        state: &mut AppState,
        selection: crate::ui::skirmish_shell::ChooseMapSelection,
    ) -> bool {
        let Some(record_idx) = selection.record_index else {
            return false;
        };
        let Some(record) = state.frontend.scenario_catalog.records().get(record_idx) else {
            return false;
        };
        let clicked_file_name = record.file_name.clone();
        let selected_file_name =
            if Self::selected_skirmish_mode_is_cooperative(state, selection.mode_id) {
                match state
                    .frontend.offline_skirmish_runtime
                    .accept_cooperative_selection(&clicked_file_name, state.frontend.scenario_catalog.shell_maps())
                {
                    Ok(Some(chosen_map)) => chosen_map,
                    Ok(None) => clicked_file_name,
                    Err(err) => {
                        log::warn!("Could not accept Cooperative campaign selection: {err}");
                        return false;
                    }
                }
            } else {
                clicked_file_name
            };
        let Some(map_idx) = state
            .frontend.scenario_catalog.shell_maps()
            .iter()
            .position(|map| map.file_name.eq_ignore_ascii_case(&selected_file_name))
        else {
            log::warn!(
                "Choose Map selected {}, but no loadable map entry exists yet",
                selected_file_name
            );
            return false;
        };

        state.frontend.skirmish_shell_state.selected_mode_id = selection.mode_id;
        crate::ui::skirmish_shell::repair_teams_for_selected_mode(
            &mut state.frontend.skirmish_shell_state,
            &state.frontend.skirmish_modes,
        );
        let applied = Self::apply_selected_shell_map_index(state, map_idx);
        debug_assert!(applied, "validated chooser map index must remain loadable");
        // `0x102` shows its statics again when its entry slide ends; a changed
        // game type or map name restarts that reveal (`0x006ADA99`, `0x4B2`).
        true
    }

    pub(super) fn handle_choose_map_modal_mouse_down(state: &mut AppState) -> bool {
        if state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
        {
            return false;
        }
        if Self::handle_choose_map_eject_mouse_down(state) {
            return true;
        }
        let layout = Self::skirmish_choose_map_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let frontend = &mut state.frontend;
        let modal = frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
            .expect("checked above");
        let double_click = modal.is_double_click(
            &layout,
            (x, y),
            Instant::now(),
            crate::ui::shell::saved_file_input::host_double_click_limits(),
        );
        if double_click {
            return true;
        }
        if let Some(button) = crate::ui::skirmish_shell::choose_map_modal_button_at(&layout, x, y) {
            if modal.press_button(button) {
                Self::play_main_menu_button_sound(state);
            }
            return true;
        }
        let press = modal.mouse_down(
            &layout,
            &frontend.skirmish_modes,
            frontend.scenario_catalog.records(),
            &mut frontend.choose_map_last_mode_row,
            (x, y),
            Instant::now(),
        );
        match press {
            crate::ui::skirmish_shell::ChooseMapListPress::RowClicked { rebuilt } => {
                // The list subclass plays GenericClick on a row press
                // (`0x0061AA5A`).
                Self::play_skirmish_shell_generic_click_sound(state);
                if rebuilt {
                    Self::ensure_active_cooperative_modal_selection(state);
                }
                state.platform.window.request_redraw();
                true
            }
            crate::ui::skirmish_shell::ChooseMapListPress::Consumed => {
                state.platform.window.request_redraw();
                true
            }
            crate::ui::skirmish_shell::ChooseMapListPress::Missed => layout.dialog.contains(x, y),
        }
    }

    /// Scrollbar arrow auto-repeat on the chooser's lists; returns the next
    /// wake deadline.
    pub(super) fn poll_choose_map_scroll(state: &mut AppState) -> Option<Instant> {
        state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_ref()?;
        let layout = Self::skirmish_choose_map_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let modal = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()?;
        if modal.poll_scroll(&layout, x, y, Instant::now()) {
            state.platform.window.request_redraw();
        }
        state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_ref()
            .and_then(|modal| modal.scroll_deadline())
    }

    pub(super) fn handle_choose_map_modal_mouse_up(state: &mut AppState) -> bool {
        if Self::handle_choose_map_eject_mouse_up(state) {
            return true;
        }
        let layout = Self::skirmish_choose_map_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let Some(modal) = state.frontend.skirmish_shell_state.choose_map_modal.as_mut() else {
            return false;
        };
        modal.mouse_up();
        let released_button = crate::ui::skirmish_shell::choose_map_modal_button_at(&layout, x, y);
        let (had_pressed_button, fired_button) = modal.release_button(released_button);
        let Some(fired_button) = fired_button else {
            return layout.dialog.contains(x, y) || had_pressed_button;
        };
        // Every way out slides `0x6B` out first (`0x007757E0` ->
        // `ShellDialog__SlideOutAndWait`); its result runs afterwards.
        let then = match fired_button {
            crate::ui::skirmish_shell::ChooseMapModalButton::UseMap0x6c5 => {
                // No map selected: Use Map does nothing (`0x005E7160`).
                let Some(selection) = modal.accept_selection() else {
                    return true;
                };
                if Self::prompt_choose_map_eject(state, selection) {
                    return true;
                }
                crate::app::frontend::shell_transition::ShellExitThen::ChooseMapUse(selection)
            }
            crate::ui::skirmish_shell::ChooseMapModalButton::Cancel0x5c0 => {
                crate::app::frontend::shell_transition::ShellExitThen::ChooseMapCancel
            }
            crate::ui::skirmish_shell::ChooseMapModalButton::CreateRandomMap0x583 => {
                crate::app::frontend::shell_transition::ShellExitThen::ChooseMapRandomMap
            }
        };
        Self::leave_shell_dialog(state, then);
        true
    }

    /// Use Map's eject check (`0x005E72A5`): when occupied AI rows would not
    /// fit the chosen map, show `GUI:EjectAIPlayers` and report it.
    pub(super) fn prompt_choose_map_eject(
        state: &mut AppState,
        selection: crate::ui::skirmish_shell::ChooseMapSelection,
    ) -> bool {
        if !Self::choose_map_would_eject_ai(state, selection) {
            return false;
        }
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
        {
            modal.eject_prompt = Some(crate::ui::skirmish_shell::EjectPrompt {
                selection,
                pressed: None,
            });
        }
        state.platform.window.request_redraw();
        true
    }

    /// Whether Use Map must ask to eject AI players: an occupied AI row at or
    /// past the chosen map's player limit (`0x005E723C` `0x005E6520`,
    /// `0x006ACCA0`).
    fn choose_map_would_eject_ai(
        state: &AppState,
        selection: crate::ui::skirmish_shell::ChooseMapSelection,
    ) -> bool {
        let Some(limit) = selection
            .record_index
            .and_then(|record| state.frontend.scenario_catalog.records().get(record))
            .map(|record| record.player_capacity)
        else {
            return false;
        };
        crate::ui::skirmish_shell::ai_rows_beyond_limit_occupied(
            &state.frontend.skirmish_shell_state.opponents,
            limit,
        )
    }

    /// The eject box's two buttons (the two-button message box `0x120`).
    fn choose_map_eject_button_at(
        state: &AppState,
        x: i32,
        y: i32,
    ) -> Option<crate::ui::skirmish_shell::EjectPromptButton> {
        let layout = crate::ui::shell::modal::quit_confirm_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        );
        if layout.ok.contains(x, y) {
            Some(crate::ui::skirmish_shell::EjectPromptButton::Ok)
        } else if layout.cancel.contains(x, y) {
            Some(crate::ui::skirmish_shell::EjectPromptButton::Cancel)
        } else {
            None
        }
    }

    /// A press while the eject box shows: its owner-draw buttons play the
    /// press sound (`0x00612B70`).
    fn handle_choose_map_eject_mouse_down(state: &mut AppState) -> bool {
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let button = Self::choose_map_eject_button_at(state, x, y);
        let Some(prompt) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
            .and_then(|modal| modal.eject_prompt.as_mut())
        else {
            return false;
        };
        prompt.pressed = button;
        if button.is_some() {
            Self::play_main_menu_button_sound(state);
        }
        state.platform.window.request_redraw();
        true
    }

    /// A release while the eject box shows: OK goes on to Use Map's slide-out,
    /// Cancel keeps the chooser.
    fn handle_choose_map_eject_mouse_up(state: &mut AppState) -> bool {
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let released = Self::choose_map_eject_button_at(state, x, y);
        let Some(modal) = state.frontend.skirmish_shell_state.choose_map_modal.as_mut() else {
            return false;
        };
        let Some(prompt) = modal.eject_prompt.as_mut() else {
            return false;
        };
        let pressed = prompt.pressed.take();
        if pressed.is_none() || pressed != released {
            state.platform.window.request_redraw();
            return true;
        }
        let selection = prompt.selection;
        modal.eject_prompt = None;
        if pressed == Some(crate::ui::skirmish_shell::EjectPromptButton::Ok) {
            Self::leave_shell_dialog(
                state,
                crate::app::frontend::shell_transition::ShellExitThen::ChooseMapUse(selection),
            );
        }
        state.platform.window.request_redraw();
        true
    }

    /// Enter or Escape on the eject box answer Cancel (`0x005D370D`).
    pub(super) fn cancel_choose_map_eject_prompt(state: &mut AppState) -> bool {
        let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
        else {
            return false;
        };
        if modal.eject_prompt.take().is_none() {
            return false;
        }
        state.platform.window.request_redraw();
        true
    }

    /// `0x6B`'s slide-out has run for Use Map: commit the selection, close
    /// the chooser; `0x102` shows again and replays its entry slide.
    pub(super) fn commit_choose_map_use(
        state: &mut AppState,
        selection: crate::ui::skirmish_shell::ChooseMapSelection,
    ) {
        if Self::commit_choose_map_selection(state, selection) {
            Self::close_choose_map_modal(state);
        }
    }

    /// `0x6B`'s slide-out has run for Create Random Map: the chooser stays
    /// hidden behind the random-map dialog `0x105`, which reopens it on
    /// Cancel (the chooser then slides in again).
    pub(super) fn commit_choose_map_random_map(state: &mut AppState) {
        let Some(previous) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_ref()
            .map(|modal| modal.cancel_selection())
        else {
            return;
        };
        let options = state
            .frontend
            .offline_skirmish_runtime
            .random_map_options_for_setup();
        let available = Self::saved_seed_dir(state)
            .is_some_and(|dir| crate::map::rmg::saved_seeds::saved_seeds_available(&dir));
        state.frontend.skirmish_shell_state.random_map_setup_modal =
            Some(crate::ui::skirmish_shell::RandomMapSetupModalState::open(
                options,
                Some(previous),
                available,
            ));
    }

    fn sync_player_name_edit_scroll(state: &mut AppState) {
        let layout = Self::skirmish_shell_layout(state);
        let text_rect = crate::ui::skirmish_shell::player_name_edit_text_rect(layout.player_name);
        let prefix_width =
            state
                .renderer.bit_font
                .text_width(crate::ui::skirmish_shell::player_name_caret_prefix(
                    &state.frontend.skirmish_shell_state,
                ));
        crate::ui::skirmish_shell::update_player_name_scroll_for_caret(
            &mut state.frontend.skirmish_shell_state,
            text_rect.w,
            prefix_width,
        );
    }

    fn localized_status_help_text(state: &AppState, key: &str) -> String {
        state
            .process_assets.csf
            .as_ref()
            .map(|csf| csf.text(key).into_owned())
            .unwrap_or_default()
    }

    fn update_skirmish_shell_status_help(
        state: &mut AppState,
        layout: &crate::ui::skirmish_shell::SkirmishShellLayout,
        x: i32,
        y: i32,
    ) {
        let text = crate::ui::skirmish_shell::hovered_shell_control(
            layout,
            &state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            x,
            y,
        )
        .and_then(crate::ui::skirmish_shell::status_help_key_for_hover)
        .map(|key| Self::localized_status_help_text(state, key))
        .unwrap_or_default();

        // `0x102`'s proc runs the common handler first (`0x006AE40A`): every
        // move writes the help, which repaints the status line.
        if crate::ui::skirmish_shell::set_status_help_text(&mut state.frontend.skirmish_shell_state, text) {
            state.platform.window.request_redraw();
        }
    }

    fn localized_choose_map_status_help_text(
        state: &AppState,
        target: crate::ui::skirmish_shell::ChooseMapHoverTarget,
    ) -> String {
        // Over a game-type row the help is the mode's description (`0x4E9`,
        // `0x005E6E44`).
        if let crate::ui::skirmish_shell::ChooseMapHoverTarget::ModeListRow0x6eb { mode_id } =
            target
            && let Some(mode) =
                crate::skirmish_modes::mode_by_id(&state.frontend.skirmish_modes, mode_id)
            && !mode.tooltip_key.is_empty()
        {
            let text = Self::localized_status_help_text(state, &mode.tooltip_key);
            if !text.is_empty() {
                return text;
            }
        }

        crate::ui::skirmish_shell::status_help_key_for_choose_map_hover(target)
            .map(|key| Self::localized_status_help_text(state, key))
            .unwrap_or_default()
    }

    /// A child's mouse move writes its help; over the background the last
    /// text stays (`0x6B` has no clearing hit test).
    fn update_choose_map_modal_status_help(
        state: &mut AppState,
        layout: &crate::ui::skirmish_shell::ChooseMapModalLayout,
        x: i32,
        y: i32,
    ) {
        let Some(text) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_ref()
            .and_then(|modal| {
                crate::ui::skirmish_shell::hovered_choose_map_modal_control(layout, modal, x, y)
            })
            .map(|target| Self::localized_choose_map_status_help_text(state, target))
        else {
            return;
        };
        // A child's hover message repaints the status line (`0x00615EF7`).
        state.frontend.shell_status_line.repaint();
        if let Some(modal) = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .as_mut()
            && modal.status_help != text
        {
            modal.status_help = text;
            state.platform.window.request_redraw();
        }
    }

    pub(super) fn handle_skirmish_shell_key_input(
        state: &mut AppState,
        code: KeyCode,
        text: Option<&str>,
    ) -> bool {
        if !state.frontend.skirmish_shell_state.player_name_edit.focused {
            return false;
        }

        let changed = match code {
            KeyCode::Backspace => crate::ui::skirmish_shell::handle_player_name_backspace(
                &mut state.frontend.skirmish_shell_state,
            ),
            KeyCode::Delete => crate::ui::skirmish_shell::handle_player_name_delete(
                &mut state.frontend.skirmish_shell_state,
            ),
            KeyCode::ArrowLeft => {
                crate::ui::skirmish_shell::handle_player_name_left(&mut state.frontend.skirmish_shell_state)
            }
            KeyCode::ArrowRight => {
                crate::ui::skirmish_shell::handle_player_name_right(&mut state.frontend.skirmish_shell_state)
            }
            KeyCode::Home => {
                crate::ui::skirmish_shell::handle_player_name_home(&mut state.frontend.skirmish_shell_state)
            }
            KeyCode::End => {
                crate::ui::skirmish_shell::handle_player_name_end(&mut state.frontend.skirmish_shell_state)
            }
            KeyCode::Tab => {
                crate::ui::skirmish_shell::handle_player_name_tab(&mut state.frontend.skirmish_shell_state)
            }
            _ => text.is_some_and(|text| {
                crate::ui::skirmish_shell::insert_player_name_text(
                    &mut state.frontend.skirmish_shell_state,
                    text,
                )
            }),
        };

        if changed {
            Self::sync_player_name_edit_scroll(state);
            state.platform.window.request_redraw();
        }
        true
    }

    fn close_validation_modal_from_controller(state: &mut AppState) {
        crate::ui::skirmish_shell::dismiss_validation_modal(&mut state.frontend.skirmish_shell_state);
        if state.frontend.shell_controller.top_id() == Some(Self::validation_modal_dialog_id()) {
            state.frontend.shell_controller.pop();
        }
    }

    pub(super) fn route_validation_modal_key(state: &mut AppState, key: ShellKey) -> bool {
        if state.frontend.skirmish_shell_state.validation_modal.is_none() {
            return false;
        }
        state
            .frontend.shell_controller
            .ensure_active(Self::validation_modal_dialog_id(), true);
        if !state.frontend.shell_controller.on_key(key) {
            return false;
        }
        Self::close_validation_modal_from_controller(state);
        state.platform.window.request_redraw();
        true
    }

    fn route_validation_modal_mouse_down(state: &mut AppState) -> bool {
        if state.frontend.skirmish_shell_state.validation_modal.is_none() {
            return false;
        }
        let feed = Self::validation_modal_feed(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        state
            .frontend.shell_controller
            .ensure_active(Self::validation_modal_dialog_id(), true);
        state.frontend.shell_controller.on_pointer_down(x, y, &feed);
        // `0xCE`'s owner-draw OK plays GUIMainButtonSound on the press
        // (`0x00612B70` via the message box `0x005D3490`).
        if state.frontend.shell_controller.pressed().is_some() {
            Self::play_main_menu_button_sound(state);
        }
        state.platform.window.request_redraw();
        true
    }

    fn route_validation_modal_mouse_up(state: &mut AppState) -> bool {
        if state.frontend.skirmish_shell_state.validation_modal.is_none() {
            return false;
        }
        let feed = Self::validation_modal_feed(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        state
            .frontend.shell_controller
            .ensure_active(Self::validation_modal_dialog_id(), true);
        let activated = state.frontend.shell_controller.on_pointer_up(x, y, &feed);
        if activated == Some(crate::ui::shell::modal::control::OK) {
            Self::close_validation_modal_from_controller(state);
        }
        state.platform.window.request_redraw();
        true
    }

    pub(super) fn handle_skirmish_shell_mouse_down(state: &mut AppState) {
        if Self::route_validation_modal_mouse_down(state) {
            return;
        }
        // The browser sits over the setup dialog, which sits over the
        // chooser, so input is offered in that order.
        if state.frontend.skirmish_shell_state.saved_seed_browser.is_some() {
            Self::handle_saved_seed_browser_mouse_down(state);
            return;
        }
        if state.frontend.skirmish_shell_state.random_map_setup_modal.is_some() {
            Self::handle_random_map_setup_mouse_down(state);
            return;
        }
        if Self::handle_choose_map_modal_mouse_down(state) {
            return;
        }
        let layout = Self::skirmish_shell_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        if crate::ui::skirmish_shell::player_name_edit_rect_hit(&layout, x, y) {
            crate::ui::skirmish_shell::focus_player_name_edit(&mut state.frontend.skirmish_shell_state);
            Self::sync_player_name_edit_scroll(state);
            state.platform.window.request_redraw();
            return;
        }
        if state.frontend.skirmish_shell_state.player_name_edit.focused {
            crate::ui::skirmish_shell::blur_player_name_edit(&mut state.frontend.skirmish_shell_state);
            state.platform.window.request_redraw();
        }
        if crate::ui::skirmish_shell::combo_dropdown_open(&state.frontend.skirmish_shell_state) {
            crate::ui::skirmish_shell::handle_option_mouse_down(
                &mut state.frontend.skirmish_shell_state,
                &layout,
                state.frontend.scenario_catalog.shell_maps(),
                x,
                y,
            );
            Self::drain_skirmish_shell_ui_sounds(state);
            return;
        }
        state.frontend.skirmish_shell_state.pressed_owner_draw_button =
            crate::ui::skirmish_shell::hit_test_owner_draw_button(&layout, x, y);
        if state
            .frontend.skirmish_shell_state
            .pressed_owner_draw_button
            .is_some()
        {
            Self::play_main_menu_button_sound(state);
        } else {
            crate::ui::skirmish_shell::handle_option_mouse_down(
                &mut state.frontend.skirmish_shell_state,
                &layout,
                state.frontend.scenario_catalog.shell_maps(),
                x,
                y,
            );
            Self::drain_skirmish_shell_ui_sounds(state);
        }
    }

    pub(super) fn handle_skirmish_shell_mouse_up(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
    ) {
        if Self::route_validation_modal_mouse_up(state) {
            return;
        }
        // The browser sits over the setup dialog, which sits over the
        // chooser, so input is offered in that order.
        if state.frontend.skirmish_shell_state.saved_seed_browser.is_some() {
            Self::handle_saved_seed_browser_mouse_up(state);
            return;
        }
        if state.frontend.skirmish_shell_state.random_map_setup_modal.is_some() {
            Self::handle_random_map_setup_mouse_up(state);
            return;
        }
        if state.frontend.skirmish_shell_state.choose_map_modal.is_some() {
            Self::handle_choose_map_modal_mouse_up(state);
            return;
        }
        let layout = Self::skirmish_shell_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        let released_button = crate::ui::skirmish_shell::hit_test_owner_draw_button(&layout, x, y);
        let pressed_button = state.frontend.skirmish_shell_state.pressed_owner_draw_button.take();
        state.frontend.skirmish_shell_last_painted_pressed_button = None;
        if pressed_button.is_some() && pressed_button == released_button {
            if let Some(button) = released_button {
                crate::ui::skirmish_shell::handle_option_mouse_up(&mut state.frontend.skirmish_shell_state);
                Self::drain_skirmish_shell_ui_sounds(state);
                let action = crate::ui::skirmish_shell::action_for_owner_draw_button(button);
                Self::handle_skirmish_shell_action(state, action, event_loop);
                return;
            }
        }

        crate::ui::skirmish_shell::handle_option_mouse_up(&mut state.frontend.skirmish_shell_state);
        Self::drain_skirmish_shell_ui_sounds(state);

        if released_button.is_some() {
            return;
        }

        let action = crate::ui::skirmish_shell::hit_test(&layout, x, y);
        Self::handle_skirmish_shell_action(state, action, event_loop);
    }

    pub(super) fn handle_skirmish_shell_mouse_move(state: &mut AppState) {
        if state.frontend.skirmish_shell_state.saved_seed_browser.is_some() {
            Self::update_saved_seed_browser_scroll(state, true);
            return;
        }
        if state.frontend.skirmish_shell_state.random_map_setup_modal.is_some() {
            Self::handle_random_map_setup_mouse_move(state);
            return;
        }
        if state.frontend.skirmish_shell_state.choose_map_modal.is_some() {
            let layout = Self::skirmish_choose_map_layout(state);
            let x = state.match_state.input.cursor_x.round() as i32;
            let y = state.match_state.input.cursor_y.round() as i32;
            if let Some(modal) = state
                .frontend
                .skirmish_shell_state
                .choose_map_modal
                .as_mut()
            {
                modal.mouse_move(&layout, x, y);
            }
            Self::update_choose_map_modal_status_help(state, &layout, x, y);
            state.platform.window.request_redraw();
            return;
        }
        if state.frontend.skirmish_shell_state.validation_modal.is_some() {
            if crate::ui::skirmish_shell::clear_status_help_text(&mut state.frontend.skirmish_shell_state) {
                state.platform.window.request_redraw();
            }
            return;
        }
        let layout = Self::skirmish_shell_layout(state);
        let x = state.match_state.input.cursor_x.round() as i32;
        let y = state.match_state.input.cursor_y.round() as i32;
        Self::update_skirmish_shell_status_help(state, &layout, x, y);
        crate::ui::skirmish_shell::handle_option_mouse_move(
            &mut state.frontend.skirmish_shell_state,
            &layout,
            state.frontend.scenario_catalog.shell_maps(),
            x,
            y,
        );
        Self::drain_skirmish_shell_ui_sounds(state);
    }

    pub(super) fn handle_skirmish_shell_mouse_wheel(state: &mut AppState, lines: f32) -> bool {
        if state.frontend.skirmish_shell_state.saved_seed_browser.is_some() {
            return true;
        }
        if state.frontend.skirmish_shell_state.validation_modal.is_some() {
            return true;
        }
        // The chooser's lists have no wheel case (`0x00618D40`).
        if state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some()
        {
            return true;
        }
        let consumed = crate::ui::skirmish_shell::handle_option_mouse_wheel(
            &mut state.frontend.skirmish_shell_state,
            state.frontend.scenario_catalog.shell_maps(),
            lines,
        );
        Self::drain_skirmish_shell_ui_sounds(state);
        consumed
    }

    fn drain_skirmish_shell_ui_sounds(state: &mut AppState) {
        let _trackbar_parent_notifications =
            state.frontend.skirmish_shell_state.drain_pending_trackbar_hscrolls();
        for sound in
            crate::ui::skirmish_shell::drain_pending_ui_sounds(&mut state.frontend.skirmish_shell_state)
        {
            Self::play_skirmish_shell_ui_sound(state, sound);
        }
    }

    pub(crate) fn play_skirmish_shell_generic_click_sound(state: &mut AppState) {
        Self::play_skirmish_shell_ui_sound(
            state,
            crate::ui::skirmish_shell::SkirmishShellUiSound::GenericClick,
        );
    }

    fn skirmish_shell_ui_sound_id<'a>(
        general: &'a crate::rules::ruleset::GeneralRules,
        sound: crate::ui::skirmish_shell::SkirmishShellUiSound,
    ) -> Option<&'a str> {
        match sound {
            crate::ui::skirmish_shell::SkirmishShellUiSound::GuiCheckboxSound => {
                general.gui_checkbox_sound.as_deref()
            }
            crate::ui::skirmish_shell::SkirmishShellUiSound::GenericClick => {
                general.generic_click_sound.as_deref()
            }
            crate::ui::skirmish_shell::SkirmishShellUiSound::GuiComboOpenSound => {
                general.gui_combo_open_sound.as_deref()
            }
            crate::ui::skirmish_shell::SkirmishShellUiSound::GuiComboCloseSound => {
                general.gui_combo_close_sound.as_deref()
            }
        }
    }

    fn play_skirmish_shell_ui_sound(
        state: &mut AppState,
        sound: crate::ui::skirmish_shell::SkirmishShellUiSound,
    ) {
        let sound_id = state
            .rules()
            .and_then(|rules| Self::skirmish_shell_ui_sound_id(&rules.general, sound))
            .map(str::to_string);
        Self::play_shell_ui_sound_by_id(state, sound_id.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_key_translation_matches_dialog_controller_route() {
        assert_eq!(App::shell_key_for_code(KeyCode::Tab), Some(ShellKey::Tab));
        assert_eq!(
            App::shell_key_for_code(KeyCode::Enter),
            Some(ShellKey::Enter)
        );
        assert_eq!(
            App::shell_key_for_code(KeyCode::NumpadEnter),
            Some(ShellKey::Enter)
        );
        assert_eq!(
            App::shell_key_for_code(KeyCode::Escape),
            Some(ShellKey::Escape)
        );
        assert_eq!(App::shell_key_for_code(KeyCode::Space), None);
    }
}
