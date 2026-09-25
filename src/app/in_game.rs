//! Match-session focus, modal, return, save/load, and developer-overlay control.

use super::{App, AppState, GameScreen, Instant, ModifiersState};
use crate::app::input::dispatch;

/// Caption gamemd loads onto the abort-mission confirmation's action button.
/// Its two mode-dependent siblings (`GUI:Restart` in campaign, `GUI:Observe` in
/// multiplayer) sit on a second button that offline skirmish hides outright.
const ABORT_CONFIRM_LEAVE_KEY: &str = "GUI:Leave";
/// The shipped English table resolves `GUI:Leave` to "Quit"; the fallback only
/// applies when the string table is missing entirely, so it says the same.
const ABORT_CONFIRM_LEAVE_FALLBACK: &str = "Quit";

impl App {
    /// Hand the scenario RNG cursor back to the offline shell when a match ends.
    pub(super) fn capture_returned_skirmish_rng(state: &mut AppState) {
        let gameplay_rng = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map(crate::sim::world::Simulation::clone_scenario_rng);
        if let Some(gameplay_rng) = gameplay_rng
            && state
                .frontend
                .offline_skirmish_runtime
                .capture_returned_gameplay_rng(gameplay_rng)
        {
            log::info!("Returned gameplay Scenario cursor to the offline shell");
        }
    }

    pub(super) fn return_to_main_menu(state: &mut AppState) {
        state.match_state.paused = false;
        state.match_state.match_presentation.in_game_menu =
            crate::ui::pause_menu::InGameMenuState::Closed;
        state.match_state.match_presentation.in_game_options_anchor = None;
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.sound_dialog = None;
        state.match_state.match_presentation.saved_game_browser = None;
        // The dev save/load panel belongs to the match; nothing draws or
        // closes it at the main menu.
        state.match_state.match_presentation.show_save_load_panel = false;
        // Persist the deterministic diagnostic log before leaving the scenario.
        // Runtime and presentation resources remain retained in the shell.
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        Self::capture_returned_skirmish_rng(state);
        state.match_state.startup.clear();
        state.match_state.scenario_elapsed_clock.reset();
        state.audio.stop_theme();
        // F11: leaving a match silences match audio completely. Previously
        // only music stopped — live SFX, the voice player, and queued EVA
        // lines survived and played over the main menu, and the SFX output
        // scale stayed wherever the exit cascade left it (this Esc route
        // bypasses drive_scenario_exit entirely).
        if let Some(ref mut sfx) = state.audio.sfx_player {
            sfx.stop_all();
            sfx.set_output_scale(1.0);
        }
        if let Some(ref mut player) = state.audio.music_player {
            player.set_output_scale(1.0);
        }
        state.match_state.match_audio.reset_for_new_match();
        state.frontend.screen = GameScreen::MainMenu;
        Self::enter_shell_window_mode(state);
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        state.platform.window.set_cursor_visible(true);
        log::info!("Returned to main menu");
    }

    /// Apply one foreground-activation edge.
    ///
    /// gamemd handles `WM_ACTIVATEAPP` edge-triggered — it compares the new
    /// value against the stored one and does nothing when it is unchanged — and
    /// runs a focus-restore on the regain edge that flushes the recorded
    /// keyboard and mouse state. Held keys are dropped on both edges here: the
    /// modifier that performed an Alt+Tab never delivers its release to this
    /// window, so it would otherwise stay latched and suppress force-fire and
    /// the ordinary cursor for the rest of the match.
    pub(super) fn set_window_active(state: &mut AppState, active: bool) {
        if state.platform.window_active == active {
            return;
        }
        state.platform.window_active = active;
        state.match_state.input.keys_held.clear();
        state.match_state.input.hotkey_modifiers = ModifiersState::empty();
        state.platform.live_modifiers = ModifiersState::empty();
        state.match_state.input.type_select.clear_held();
        // gamemd-derived: the `WM_ACTIVATEAPP` changed edge at 0x007778AC
        // stops/restores the primary DirectSound output through 0x00407020 /
        // 0x00407040 while secondary playback cursors continue. Keep this on
        // the same edge as the main-loop gate rather than pausing each stream.
        if let Some(player) = state.audio.music_player.as_mut() {
            player.set_focus_output_active(active);
        }
        if let Some(player) = state.audio.sfx_player.as_mut() {
            player.set_focus_output_active(active);
        }
        if active {
            // The deactivated span must not buy a catch-up frame: forget the
            // pacing window so exactly one frame runs immediately, then normal
            // pacing resumes.
            state.platform.frame_pacer.reset_for_immediate_frame();
            state.platform.window.request_redraw();
        }
        log::info!(
            "Window {}",
            if active { "activated" } else { "deactivated" }
        );
    }

    /// Apply one window-visibility edge. Waking on the un-hide edge is what
    /// gets the parked redraw loop turning again.
    pub(super) fn set_window_hidden(state: &mut AppState, hidden: bool) {
        if state.platform.window_hidden == hidden {
            return;
        }
        state.platform.window_hidden = hidden;
        if !hidden {
            state.platform.window.request_redraw();
        }
    }

    /// Does the in-scenario modal machine own this Escape press?
    ///
    /// Native Escape reaches Options through5372D0→647040→4C7939. B5 and B6
    /// ignore IDCANCEL, while Game Controls returns to B5. Escape keeps its
    /// in-world cancel duties: while no modal is
    /// open and a placement/targeting or repair/sell mode is armed, Escape
    /// cancels that instead and the machine stays out of it.
    pub(super) fn in_game_menu_owns_escape(state: &AppState) -> bool {
        let in_world_mode_armed = state.match_state.input.targeting_mode.is_some()
            || state
                .match_state
                .match_presentation
                .sidebar_gadget_state
                .repair_mode_on
            || state
                .match_state
                .match_presentation
                .sidebar_gadget_state
                .sell_mode_on;
        crate::ui::pause_menu::escape_belongs_to_modal_machine(
            state.match_state.match_presentation.in_game_menu,
            in_world_mode_armed,
        )
    }

    /// Route one Escape press through the in-scenario modal machine.
    pub(super) fn route_in_game_menu_escape(state: &mut AppState) {
        use crate::ui::pause_menu::InGameMenuState;

        // Backing out of Options takes the same exit its Back control does —
        // apply and persist the touched `[Options]` values — so the two ways of
        // leaving the dialog cannot disagree about what was saved.
        if state.match_state.match_presentation.in_game_menu == InGameMenuState::Options {
            crate::app::persistence::options::in_game_options_close(state);
        }
        let next = state
            .match_state
            .match_presentation
            .in_game_menu
            .on_escape();
        Self::enter_in_game_menu_state(state, next);
    }

    /// Commit an in-scenario modal transition and the app-layer effects that
    /// ride on it.
    ///
    /// The simulation freezes for every non-zero state: gamemd's modal pump
    /// never reaches its main tick in offline campaign or skirmish, so no
    /// per-tick update, frame-counter step or tactical recomposition happens
    /// while a dialog is up. Freezing does not skip ticks — the tick simply
    /// stops advancing and resumes from the same number, so the tick stream is
    /// unchanged and a replay of the match still reproduces.
    pub(crate) fn enter_in_game_menu_state(
        state: &mut AppState,
        next: crate::ui::pause_menu::InGameMenuState,
    ) {
        use crate::ui::pause_menu::InGameMenuState;

        let previous = state.match_state.match_presentation.in_game_menu;
        if previous == next {
            return;
        }
        let was_open = previous.is_open();
        let will_be_open = next.is_open();
        if was_open != will_be_open
            && !crate::app::match_runtime::sim_tick::current_session_mode(state).is_network()
        {
            let now_ms = crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(
                state,
                Instant::now(),
            );
            if will_be_open {
                state.match_state.scenario_elapsed_clock.pause(now_ms);
            } else {
                state.match_state.scenario_elapsed_clock.resume(now_ms);
            }
        }
        state.match_state.match_presentation.in_game_menu = next;
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.sound_dialog = None;
        if next == InGameMenuState::Menu {
            state.match_state.match_presentation.pause_menu_has_saves = !state.persistence.repository.browser_entries().is_empty();
        }

        // Leaving Options: drop the cached `0xBBB` hit-test anchor so the
        // overlay's own mouse handler cannot claim clicks aimed at the menu.
        if previous == InGameMenuState::Options {
            state.match_state.match_presentation.in_game_options_anchor = None;
        }
        if next == InGameMenuState::Options {
            state.match_state.match_presentation.in_game_options.sound_enabled=state.audio.launcher_audio_available;
            // Reset the transient interaction flags so the drag-gated
            // value-label quirk resets on every open.
            state
                .match_state
                .match_presentation
                .in_game_options
                .on_open();
        }

        state.match_state.paused = next.is_open();
        if next.is_open() {
            // Show the OS cursor so the modal is clickable.
            if state
                .match_state
                .match_presentation
                .software_cursor
                .is_some()
            {
                state.platform.window.set_cursor_visible(true);
            }
        } else {
            // Elapsed modal time must not cause a catch-up frame.
            state.platform.frame_pacer.reset_for_immediate_frame();
            if state
                .match_state
                .match_presentation
                .software_cursor
                .is_some()
            {
                state.platform.window.set_cursor_visible(false);
            }
        }
        state.platform.window.request_redraw();
    }

    /// Draw whichever in-scenario modal card is open and commit its route.
    ///
    /// Options is the native `0xBBB` overlay, drawn earlier in the frame; this
    /// only reconciles the machine when that overlay closes itself.
    pub(super) fn handle_in_game_menu(state: &mut AppState) {
        use crate::ui::pause_menu::{self, InGameMenuState, ModalOutcome};

        let outcome = match state.match_state.match_presentation.in_game_menu {
            InGameMenuState::Closed => ModalOutcome::Stay,
            InGameMenuState::Menu => pause_menu::resolve_menu_action(
                pause_menu::draw_in_game_menu(&state.renderer.egui.ctx),
            ),
            InGameMenuState::AbortConfirm => {
                let leave_label =
                    Self::csf_label(state, ABORT_CONFIRM_LEAVE_KEY, ABORT_CONFIRM_LEAVE_FALLBACK);
                pause_menu::resolve_abort_action(pause_menu::draw_abort_confirm(
                    &state.renderer.egui.ctx,
                    &leave_label,
                ))
            }
            // Options is the native `0xBBB` overlay, drawn earlier in the frame
            // and reconciled by `sync_in_game_menu_with_options_overlay`.
            InGameMenuState::Options | InGameMenuState::Sound | InGameMenuState::Keyboard | InGameMenuState::SavedGame(_) => ModalOutcome::Stay,
        };

        Self::apply_in_game_modal_outcome(state, outcome);
    }

    /// Every physical or fallback modal commits through the same state/exit owner.
    pub(crate) fn apply_in_game_modal_outcome(state: &mut AppState, outcome: crate::ui::pause_menu::ModalOutcome) {
        use crate::ui::pause_menu::ModalOutcome;
        match outcome {
            ModalOutcome::Stay => {}
            ModalOutcome::Enter(next) => Self::enter_in_game_menu_state(state, next),
            ModalOutcome::LeaveMatch => Self::exit_match_to_shell(state),
        }
    }

    /// Re-enter the in-game menu when the Options overlay closes itself.
    ///
    /// gamemd's Options dialog is a **child** of the in-game menu: when it
    /// returns, the state machine writes state 1, so closing Options puts the
    /// player back on the menu rather than back into the mission. The `0xBBB`
    /// overlay in this port owns its own Back handler and clears `paused`
    /// there, so the parent state is re-asserted here — before the frame's
    /// simulation advance, so the child's close cannot leak a stray tick.
    pub(super) fn sync_in_game_menu_with_options_overlay(state: &mut AppState) {
        use crate::ui::pause_menu::InGameMenuState;

        if state.match_state.match_presentation.in_game_menu == InGameMenuState::Options
            && !state.match_state.paused
        {
            Self::enter_in_game_menu_state(state, InGameMenuState::Menu);
        }
    }

    /// Queue the running match's native graceful-exit event.
    ///
    /// gamemd's confirmed Abort queues an EXIT event for the local player; when
    /// that event executes it raises the graceful-exit session flag, and the
    /// session-end router tears the session down **without** the victory or
    /// defeat teardown — no outcome announcement, no result screen, straight
    /// back to the shell. Confirmation itself only constructs and queues the
    /// event; teardown begins after the event-tail dispatcher executes it.
    fn exit_match_to_shell(state: &mut AppState) {
        log::info!("Abort Mission confirmed — queueing EXIT event");
        if state.match_state.scenario_exit.is_some() {
            return;
        }
        let Some(owner) = crate::app::input::commands::preferred_local_owner(state) else {
            log::warn!("Abort Mission confirmation has no local command owner");
            return;
        };
        if crate::app::input::commands::try_schedule_command(
            state,
            &owner,
            crate::sim::command::Command::ExitMatch,
        )
        .is_none()
        {
            log::warn!("Abort Mission EXIT event could not be queued for '{owner}'");
            return;
        }
        Self::enter_in_game_menu_state(state, crate::ui::pause_menu::InGameMenuState::Closed);
    }

    /// Consume EventClass opcode `0x13`'s executed edge and enter the existing
    /// battle-abort teardown. A repeated drain without another dispatch is a
    /// no-op because the simulation edge is taken.
    pub(super) fn consume_executed_abort_exit(state: &mut AppState, wall_ms: u64) {
        let local_owner = crate::app::input::commands::preferred_local_owner(state);
        let local_owner_id = local_owner.as_deref().and_then(|owner| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .and_then(|sim| sim.interner.get(owner))
        });
        let local_outcome_exit_ready = local_owner_id.is_some_and(|owner| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .and_then(|sim| sim.ready_outcome_for_owner(owner))
                .is_some()
        });
        let executed_owner = state
            .match_state
            .sim_runtime
            .as_mut()
            .map(|rt| &mut rt.simulation)
            .and_then(|sim| sim.take_executed_exit_owner());
        let Some(executed_owner) = executed_owner else {
            return;
        };
        if Some(executed_owner) != local_owner_id {
            log::warn!("Ignoring executed EXIT event not owned by the local player");
            return;
        }
        if matches!(
            crate::app::match_runtime::scenario_exit::arbitrate_executed_exit(
                local_outcome_exit_ready
            ),
            crate::app::match_runtime::scenario_exit::ExecutedExitDisposition::Outcome
        ) {
            // Main_Game observes the ready victory/loss route first. The EXIT
            // edge is still consumed, but cannot clear or replace that route.
            return;
        }
        if state.match_state.scenario_exit.is_some() {
            return;
        }

        // Abort is the independent EXIT-event route: it never inherits the
        // victory/defeat HouseClass SavourDelay or its outcome-voice wait.
        state.match_state.scenario_outcome = None;
        let _ = state.match_state.scenario_elapsed_clock.stop(wall_ms);
        // gamemd provenance: battle abort teardown; verified
        // GameExit__BattleControlTerminated @ 0x00686570 starts Theme's fade,
        // then fades the independent audio master, bounds its voice pump to
        // 300 timer buckets, and finally hard-stops audio.
        let mut scenario_exit =
            crate::app::match_runtime::scenario_exit::ScenarioExitCascade::start(
                wall_ms,
                crate::app::match_runtime::scenario_exit::ScenarioExitDestination::MainMenu,
            );
        // `0x00686570` requests EVA_BattleControlTerminated as an INTERRUPT
        // immediately before waiting for the two simultaneous audio fades.
        if let Some(action) = scenario_exit.take_start_voice_action() {
            Self::apply_scenario_exit_voice_action(state, action);
        }
        state.match_state.scenario_exit = Some(scenario_exit);
    }

    /// Draw the save/load panel and handle its actions.
    pub(super) fn handle_save_load_panel(state: &mut AppState) {
        use crate::app::persistence::save_load_panel::SaveLoadAction;

        state.persistence.refresh_save_list_if_dirty();
        let action = crate::app::persistence::save_load_panel::draw_save_load_panel(
            &state.renderer.egui.ctx,
            state.persistence.save_list_cache.entries(),
        );

        match action {
            SaveLoadAction::Load(path) => {
                crate::app::persistence::commands::load_save_file(state, &path);
            }
            SaveLoadAction::Delete(path) => {
                if let Err(e) = state.persistence.repository.delete(&path) {
                    log::error!("Failed to delete save {}: {e}", path.display());
                } else {
                    log::info!("Deleted save: {}", path.display());
                }
                state.persistence.invalidate_save_list();
            }
            SaveLoadAction::Close => {
                state.match_state.match_presentation.show_save_load_panel = false;
            }
            SaveLoadAction::None => {}
        }
    }

    /// Draw the dev overlay and dispatch its actions. No-op when the
    /// overlay is hidden — caller checks `show_dev_overlay` before
    /// calling.
    pub(super) fn handle_dev_overlay(state: &mut AppState) {
        use crate::app::diagnostics::dev_overlay::{
            DevOverlayAction, DevOverlayInfo, RecentSaveRow,
        };

        // Build the recent-saves snapshot from the existing cache.
        state.persistence.refresh_save_list_if_dirty();
        let recent_saves: Vec<RecentSaveRow> = state
            .persistence
            .save_list_cache
            .entries()
            .iter()
            .take(5)
            .map(|e| RecentSaveRow {
                path: e.path.clone(),
                display_name: e
                    .path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string(),
                tick: e.header.tick,
                age_str: crate::app::persistence::save_load_panel::format_timestamp(
                    e.header.save_timestamp,
                ),
            })
            .collect();

        let last_save_age: Option<String> = state.persistence.last_save_instant().map(|t| {
            let secs = t.elapsed().as_secs();
            if secs < 60 {
                format!("{secs}s ago")
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else {
                format!("{}h {}m ago", secs / 3600, (secs % 3600) / 60)
            }
        });

        let last_load_available = state
            .persistence
            .last_loaded_save_path
            .as_ref()
            .map(|path| state.persistence.repository.exists(path))
            .unwrap_or(false);
        let last_load_display = state
            .persistence
            .last_loaded_save_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(str::to_string);

        // Temporarily move the save-name buffer out so it can be borrowed
        // mutably by the info struct without conflicting with state.
        let mut save_name = std::mem::take(&mut state.diag.dev_overlay_save_name);

        let mut info = DevOverlayInfo {
            sim_speed_tps: state.match_state.sim_speed_tps,
            paused: state.match_state.paused,
            music_volume: state
                .audio
                .music_player
                .as_ref()
                .map_or(0.5, |p| p.volume()),
            sfx_volume: state.audio.sfx_player.as_ref().map_or(0.7, |p| p.volume()),
            show_pathgrid: state.diag.debug_show_pathgrid,
            show_cell_grid: state.diag.debug_show_cell_grid,
            show_heightmap: state.diag.debug_show_heightmap,
            show_unit_inspector: state.diag.debug_unit_inspector,
            reveal_map: state.match_state.sandbox_full_visibility,
            fps: state.diag.frame_timer.fps(),
            frame_ms: state.diag.frame_timer.frame_ms_mean(),
            tick_budget_ms: if state.match_state.sim_speed_tps == 0 {
                0.0
            } else {
                1000.0 / state.match_state.sim_speed_tps as f32
            },
            entity_count: state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .map_or(0, |s| s.entities().len()),
            save_name_buf: &mut save_name,
            last_save_tick: state.persistence.last_save_tick(),
            last_save_age,
            last_load_available,
            last_load_display,
            recent_saves,
        };

        let action = crate::app::diagnostics::dev_overlay::draw_dev_overlay(
            &state.renderer.egui.ctx,
            &mut info,
        );

        // Restore the (possibly-edited) buffer.
        state.diag.dev_overlay_save_name = save_name;

        match action {
            DevOverlayAction::None => {}
            // Developer-only direct-tps override (fine-grained 1..200, for
            // debugging). It deliberately BYPASSES the 0..6 Options model (KD-3):
            // it writes `sim_speed_tps` without touching `in_game_options.game_speed`,
            // so the next native Options close reasserts the Options speed. The
            // 0..6 bucket model cannot represent these arbitrary tps values.
            DevOverlayAction::SetGameSpeed(tps) => {
                state.match_state.sim_speed_tps = tps.max(1);
                log::info!("Game speed: {} tps", state.match_state.sim_speed_tps);
            }
            DevOverlayAction::ResetGameSpeed => {
                state.match_state.sim_speed_tps = crate::app::types::default_yr_skirmish_tps();
                log::info!(
                    "Game speed reset to {} tps",
                    state.match_state.sim_speed_tps
                );
            }
            DevOverlayAction::SetMusicVolume(v) => {
                if let Some(p) = &mut state.audio.music_player {
                    p.set_volume(v);
                }
            }
            DevOverlayAction::SetSfxVolume(v) => {
                if let Some(p) = &mut state.audio.sfx_player {
                    p.set_volume(v);
                }
            }
            DevOverlayAction::TogglePause => {
                dispatch::toggle_debug_pause(state);
            }
            DevOverlayAction::ReturnToMenu => {
                Self::return_to_main_menu(state);
            }
            DevOverlayAction::StepOneTick => {
                if state.match_state.paused {
                    state.diag.debug_frame_step_requested = true;
                }
            }
            DevOverlayAction::TogglePathGrid => {
                dispatch::toggle_pathgrid_overlay(state);
            }
            DevOverlayAction::ToggleCellGrid => {
                state.diag.debug_show_cell_grid = !state.diag.debug_show_cell_grid;
            }
            DevOverlayAction::ToggleHeightmap => {
                state.diag.debug_show_heightmap = !state.diag.debug_show_heightmap;
            }
            DevOverlayAction::ToggleUnitInspector => {
                dispatch::toggle_unit_inspector(state);
            }
            DevOverlayAction::ToggleRevealMap => {
                state.match_state.sandbox_full_visibility =
                    !state.match_state.sandbox_full_visibility;
                log::info!(
                    "Reveal map: {}",
                    if state.match_state.sandbox_full_visibility {
                        "ON"
                    } else {
                        "OFF"
                    }
                );
            }
            DevOverlayAction::SaveAs => {
                let name = std::mem::take(&mut state.diag.dev_overlay_save_name);
                crate::app::persistence::commands::save_with_name(state, &name);
            }
            DevOverlayAction::ReloadLastLoad => {
                if let Some(path) = state.persistence.last_loaded_save_path.clone() {
                    if state.persistence.repository.exists(&path) {
                        crate::app::persistence::commands::load_save_file(state, &path);
                    } else {
                        log::warn!(
                            "Reload last load: file no longer exists: {}",
                            path.display()
                        );
                    }
                }
            }
            DevOverlayAction::LoadSave(path) => {
                crate::app::persistence::commands::load_save_file(state, &path);
            }
        }
    }
}

impl App {
    /// Commit the complete retained Options/Video/Audio profile on the already
    /// verified quit-confirm boundaries, strictly before teardown. A write
    /// failure is logged by the persistence owner and never blocks quitting.
    /// Retail provenance: `OptionsClass__WriteToINI @ 0x005FAD10`.
    pub(super) fn persist_settings_on_quit(state: &AppState) {
        crate::app::persistence::options::persist_options_profile(state);
    }

    /// Begin the graceful quit cascade from the main-menu Exit-confirm OK. The
    /// caller persists settings FIRST (so the captured volume is pre-fade), then
    /// calls this instead of exiting immediately; `render_frame` drives it to
    /// completion and then exits the event loop.
    pub(super) fn start_quit_cascade(state: &mut AppState) {
        let start_volume = state
            .audio
            .music_player
            .as_ref()
            .map_or(0.0, |p| p.volume());
        state.frontend.quit_cascade = Some(crate::app::frontend::quit_cascade::QuitCascade::start(
            Instant::now(),
            start_volume,
        ));
    }

    pub(super) fn drive_scenario_exit(state: &mut AppState, wall_ms: u64) {
        if state.match_state.scenario_exit.is_none() {
            return;
        }
        let poll_voices = state
            .match_state
            .scenario_exit
            .as_ref()
            .is_some_and(|exit| exit.needs_voice_poll(wall_ms));
        let voices_active = poll_voices
            && match (&mut state.audio.sfx_player, state.process_assets.manager()) {
                (Some(sfx), Some(assets)) => sfx.pump_and_check_voices(
                    &state.audio.sound_registry,
                    assets,
                    &state.audio.audio_indices,
                ),
                _ => false,
            };
        let tick = state
            .match_state
            .scenario_exit
            .as_mut()
            .expect("scenario exit remains present")
            .tick(wall_ms, voices_active);

        if let Some(scale) = tick.music_output_scale {
            if let Some(player) = state.audio.music_player.as_mut() {
                player.set_output_scale(scale);
            }
        }
        if let Some(scale) = tick.sfx_output_scale {
            if let Some(player) = state.audio.sfx_player.as_mut() {
                player.set_output_scale(scale);
            }
        }
        if tick.stop_audio {
            state.audio.stop_theme();
            if let Some(player) = state.audio.music_player.as_mut() {
                player.set_output_scale(1.0);
            }
            if let Some(player) = state.audio.sfx_player.as_mut() {
                player.stop_all();
                player.set_output_scale(1.0);
            }
        }
        // ScoreDialog__WndProc @ 0x005C9B10 resolves the literal SCORE theme
        // and starts it immediately on WM_INITDIALOG. Keep this after the
        // hard stop and output-scale restoration so ScoreX begins audible.
        if let Some(crate::app::match_runtime::scenario_exit::ScenarioExitAudioAction::PlayTheme(
            theme,
        )) = tick.after_stop
            && let Some(assets) = state.process_assets.manager()
        {
            let _ = state.audio.play_theme(theme, assets, wall_ms);
        }
        if !tick.finished {
            return;
        }

        let destination = state.match_state.scenario_exit.as_mut().and_then(
            crate::app::match_runtime::scenario_exit::ScenarioExitCascade::take_destination,
        );
        state.match_state.scenario_exit = None;
        match destination {
            Some(crate::app::match_runtime::scenario_exit::ScenarioExitDestination::Score {
                title,
                detail,
                model,
            }) => {
                state.frontend.score_screen = Some(model);
                state.frontend.score_shell_state = Default::default();
                state.frontend.screen = GameScreen::MissionResult { title, detail };
                // Victory 006857AE restores the shell pair before score
                // construction/run at 00685884/0068588B, not after Continue.
                Self::enter_shell_window_mode(state);
            }
            Some(crate::app::match_runtime::scenario_exit::ScenarioExitDestination::MainMenu) => {
                Self::return_to_main_menu(state);
            }
            None => log::error!("Scenario exit finished without a destination"),
        }
    }

    fn apply_scenario_exit_voice_action(
        state: &mut AppState,
        action: crate::app::match_runtime::scenario_exit::ScenarioExitVoiceAction,
    ) {
        match action {
            crate::app::match_runtime::scenario_exit::ScenarioExitVoiceAction::InterruptBattleControlTerminated => {
                if state.match_state.local_player_owner.is_none() {
                    log::warn!("Battle-control termination EVA has no pinned local owner");
                    return;
                }
                // `GameExit::BattleControlTerminated 0x00686616`:
                // `VoxClass::PlayEVA("EVA_BattleControlTerminated", 2)` — the
                // INTERRUPT override; the entry itself is STANDARD CRITICAL.
                let eva_side = crate::app::presentation::building_anim::local_eva_side(state);
                if let (Some(sfx), Some(assets)) = (&mut state.audio.sfx_player, state.process_assets.manager()) {
                    let _ = sfx.play_eva(
                        "EVA_BattleControlTerminated",
                        Some(crate::rules::sound_ini::EvaType::Interrupt),
                        &state.audio.eva_registry,
                        eva_side,
                        &state.audio.sound_registry,
                        assets,
                        &state.audio.audio_indices,
                    );
                }
            }
        }
    }
}
