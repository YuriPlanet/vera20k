//! Match-session focus, modal, return, quit and scenario-exit control.

use super::{App, AppState, GameScreen, Instant, ModifiersState};
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::sim_host::SimWrite;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;

impl App {
    /// Return the live Scenario cursor and header to the multiplayer shell.
    pub(super) fn capture_returned_multiplayer_scenario(
        frontend: &mut crate::app::frontend::state::FrontendState,
        sim_runtime: &crate::app::match_runtime::sim_host::SimHost,
    ) {
        let gameplay_rng = sim_runtime
            .runtime()
            .map(|rt| &rt.simulation)
            .map(|simulation| {
                (
                    simulation.clone_scenario_rng(),
                    simulation.session().start_header(),
                )
            });
        if let Some((gameplay_rng, start_header)) = gameplay_rng
            && frontend
                .multiplayer_shell_runtime
                .capture_returned_gameplay_scenario(gameplay_rng, start_header)
        {
            log::info!("Returned gameplay Scenario cursor and header to the multiplayer shell");
        }
    }

    /// Main_Game's post-scenario write `0x00699450` (`0x0048CF79`): the
    /// TauntsOn and ScrollText switches persist after a LAN or WOL scenario
    /// returns. Skirmish and campaign never write them.
    pub(crate) fn persist_returned_session_toggles(
        frontend: &mut crate::app::frontend::state::FrontendState,
        match_state: &crate::app::match_runtime::state::MatchState,
    ) {
        if match_state.session_mode().is_network() {
            frontend.multiplayer_shell_runtime.persist_session_toggles();
        }
    }

    pub(super) fn return_to_main_menu(state: &mut AppState) {
        if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
            // Main55D283 calls BattleControlTerminated686570 on shell return;
            // its retained Scenario mission counter resets to the ctor's 1.
            runtime
                .simulation
                .reset_campaign_mission_counter_on_shell_exit();
        }
        Self::retire_running_scenario(state);
        state.frontend.screen = GameScreen::MainMenu;
        Self::resume_shell_after_match(state);
        log::info!("Returned to the shell");
    }

    /// Wait_For_Players result 8 clears GameActive without an EXIT event, so
    /// Main_Game returns past the router without 0x00686570's EVA and fades
    /// (0x0064806C..0x006482A6, 0x0055D347).
    pub(super) fn return_disconnected_to_main_menu(state: &mut AppState) {
        Self::record_match_result(
            &mut state.frontend,
            &state.match_state,
            super::cncnet::ExternalMatchResult::Disconnected,
        );
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        Self::browser_match_ended(
            &state.frontend,
            &state.match_state,
            super::cncnet::ExternalMatchResult::Disconnected,
        );
        Self::return_to_main_menu(state);
    }

    /// Quit Game's leave: a frame suspended in Wait_For_Players finishes its
    /// tail before the result-8 return.
    pub(super) fn leave_network_wait_to_main_menu(state: &mut AppState) {
        crate::app::match_runtime::network_failure::hold_quit_line(
            &mut state.frontend,
            &state.match_state,
        );
        if state
            .match_state
            .sim_runtime
            .runtime()
            .is_some_and(|rt| rt.simulation.pending_network_wait())
        {
            crate::app::match_runtime::sim_tick::leave_network_wait(state);
        }
        Self::return_disconnected_to_main_menu(state);
    }

    /// Stop a running scenario without an outcome teardown: its world,
    /// modal, audio and diagnostic owners, leaving the runtime retained.
    /// Shared by the EXIT return and the campaign restart (`0x006863E0`
    /// stops the elapsed clock, theme, sounds and speech before
    /// Start_Scenario).
    fn retire_running_scenario(state: &mut AppState) {
        // Close out the last frame before retiring its movie hold or
        // returning its transport to the shell.
        crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
        // Shell return and campaign restart both end the installed match.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        Self::browser_match_ended(
            &state.frontend,
            &state.match_state,
            super::cncnet::ExternalMatchResult::Quit,
        );
        state.match_state.input.planning.reset();
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(
            &state.platform,
            &state.renderer,
            &state.diag,
            &state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &state.persistence,
            wall_ms,
        );
        crate::app::commander_feedback::retire(&mut state.match_state.match_presentation);
        crate::app::chrono_screen::retire(
            &state.frontend.screen,
            &mut state.match_state,
            &mut state.audio,
            wall_ms,
        );
        if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
            // Main_Game48CCC0 clears ScenarioStarted[A8ED5C] at48CEBF
            // after the match returns (EBX=0 from48CCD4); the restart clears
            // it at686424. The runtime may remain retained, but Tag
            // destruction must see a stopped session.
            // Source: original gamemd.exe instructions.
            runtime.simulation.retire_scenario_started();
        }
        state.match_state.scenario_restart_requested = false;
        state.match_state.debug_pause = false;
        crate::app::input::pause_menu::on_match_end(
            &mut state.platform,
            &state.renderer,
            &mut state.frontend,
            &mut state.match_state,
            &state.process_assets,
            &mut state.audio,
            &mut state.persistence,
        );
        state.match_state.match_presentation.options_page_category = None;
        state.match_state.match_presentation.in_game_menu =
            crate::ui::pause_menu::InGameMenuState::Closed;
        state.match_state.match_presentation.dialog_ui = Default::default();
        state.match_state.match_presentation.diplomacy_page = Default::default();
        state.match_state.match_presentation.sound_dialog = None;
        state.match_state.match_presentation.saved_game_browser = None;
        state.match_state.network_wait = Default::default();
        state.match_state.network_failure = None;
        crate::app::lan_service::return_transport(&mut state.frontend, &mut state.match_state);
        // Persist the deterministic diagnostic log before leaving the scenario.
        // Runtime and presentation resources remain retained in the shell.
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        state.match_state.frame_stopped = false;
        Self::capture_returned_multiplayer_scenario(
            &mut state.frontend,
            &state.match_state.sim_runtime,
        );
        Self::persist_returned_session_toggles(&mut state.frontend, &state.match_state);
        state.match_state.startup.clear();
        state.match_state.scenario_elapsed_clock.reset();
        state.audio.stop_theme();
        // Leaving the scenario silences all match audio: live SFX, voices and
        // queued EVA lines stop, and the sound master returns to full.
        if let Some(ref mut sfx) = state.audio.sfx_player {
            sfx.stop_all();
        }
        state.audio.restore_sound_master();
        state.match_state.match_audio.reset_for_new_match();
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
        // The next match starts at HD art's zoom again (`render::hd_switch`).
        state.renderer.hd_art_zoom_pending = true;
    }

    /// Start_Scenario's inputs for the installed campaign mission again
    /// (Scenario+0x125C, DL 0, Scenario+0x34CC), including a saved campaign.
    pub(super) fn prepare_campaign_restart(
        match_state: &crate::app::match_runtime::state::MatchState,
        profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    ) -> Option<crate::app::match_bootstrap::PreparedCampaignStartup> {
        let campaign = match_state.startup.installed_campaign()?;
        let game_speed = match_state
            .sim_runtime
            .runtime()
            .map_or(profile.game_speed, |runtime| {
                runtime.simulation.session().game_options().game_speed
            });
        Some(campaign.restart(
            profile.difficulty,
            game_speed,
            &mut crate::app::match_bootstrap::OrdinaryMatchSeedClock,
        ))
    }

    /// `0x006863E0`, the termination router's restart arm: tear the running
    /// mission down without fade, movie, prompt or score, then Start_Scenario
    /// on the same map through the one campaign start path.
    ///
    /// Residuals: the 60-bucket network wait after Start_Scenario is not
    /// ported (campaign is single-player). 6863E0 stops the elapsed clock
    /// into its +0x61C accumulator; no zeroing of that accumulator on this
    /// path was found, so native score time may run across a restart, while
    /// VERA starts a fresh clock (unproven, not player-visible until a won
    /// mission's score).
    fn restart_campaign_scenario(state: &mut AppState) {
        let Some(startup) =
            Self::prepare_campaign_restart(&state.match_state, &state.persistence.options_profile)
        else {
            log::warn!("Restart has no installed campaign start; returning to the shell");
            Self::return_to_main_menu(state);
            return;
        };
        log::info!("Restarting campaign mission {}", startup.scenario());
        // The router clears all four termination bytes before 6863E0: an
        // executed EXIT edge cannot follow the restart into the shell.
        if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Lifecycle) {
            let _ = runtime.simulation.take_executed_exit_owner();
        }
        Self::retire_running_scenario(state);
        // No tactical frame runs again: the Intro/Brief movie or loading owns
        // the screen until the restarted mission is installed.
        state.frontend.screen = GameScreen::MainMenu;
        Self::start_campaign_scenario(
            state,
            crate::app::loading::pump::LoadingRequest::campaign(startup),
        );
    }

    /// Consume the restart request through the termination router: a ready
    /// local outcome outranks it, and an outcome or exit teardown already in
    /// progress has consumed the request with the other termination bytes.
    pub(super) fn consume_scenario_restart_request(state: &mut AppState) {
        if Self::house_outcome_wait_pending(&state.match_state.sim_runtime) {
            return;
        }
        if !std::mem::take(&mut state.match_state.scenario_restart_requested) {
            return;
        }
        let outcome_ready = Self::local_outcome_exit_ready(&state.match_state);
        let route = crate::app::match_runtime::scenario_exit::route_scenario_termination(
            crate::app::match_runtime::scenario_exit::TerminationRequests {
                outcome_ready,
                restart: true,
                exit: false,
            },
        );
        if state.match_state.scenario_exit.is_some()
            || route != Some(crate::app::match_runtime::scenario_exit::TerminationRoute::Restart)
        {
            log::info!("Restart request dropped by a higher-priority scenario termination");
            return;
        }
        Self::restart_campaign_scenario(state);
    }

    pub(super) fn local_outcome_exit_ready(
        match_state: &crate::app::match_runtime::state::MatchState,
    ) -> bool {
        let Some(sim) = match_state.sim_runtime.runtime().map(|rt| &rt.simulation) else {
            return false;
        };
        crate::app::input::commands::preferred_local_owner(match_state)
            .as_deref()
            .and_then(|owner| sim.interner().get(owner))
            .is_some_and(|owner| sim.ready_outcome_for_owner(owner).is_some())
    }

    /// HouseClass4F860B/4F8732 services audio/network inside Main_Tick.
    /// The retained frame owns this hold; ordinary modal/input dispatch waits.
    pub(crate) fn house_outcome_wait_pending(
        sim_runtime: &crate::app::match_runtime::sim_host::SimHost,
    ) -> bool {
        sim_runtime
            .runtime()
            .is_some_and(|rt| rt.simulation.pending_outcome_wait().is_some())
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
    pub(super) fn set_window_active(
        platform: &mut PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        persistence: &PersistenceState,
        audio: &mut AppAudioRuntime,
        active: bool,
    ) {
        if platform.window_active == active {
            return;
        }
        platform.window_active = active;
        Self::sync_window_movies(
            platform,
            frontend,
            match_state.session_mode(),
            &mut match_state.match_presentation,
        );
        if active {
            platform.clear_attention();
        }
        match_state.input.keys_held.clear();
        // VERA formation line: its held key's release may never arrive.
        match_state.input.formation_line.clear_key();
        match_state.input.hotkey_modifiers = ModifiersState::empty();
        platform.live_modifiers = ModifiersState::empty();
        platform.held_modifier_keys.clear();
        match_state.input.type_select.clear_held();
        crate::app::input::planning_mode::release_on_focus_change(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            persistence,
        );
        // gamemd-derived: the `WM_ACTIVATEAPP` changed edge at 0x007778AC
        // stops/restores the primary DirectSound output through 0x00407020 /
        // 0x00407040 while secondary playback cursors continue. Native guards
        // that only with the window-identity check and the stored
        // `g_ApplicationActive` (0x00A8ED80) edge — never with a setting — so
        // the audio gate rides on every activation edge, and a window the
        // player minimises goes silent.
        if let Some(player) = audio.music_player.as_mut() {
            player.set_focus_output_active(active);
        }
        if let Some(player) = audio.sfx_player.as_mut() {
            player.set_focus_output_active(active);
        }
        if platform.pause_on_focus_loss && active && match_state.session_mode().is_single_player() {
            // Only the configured freeze leaves a deactivated span to forget:
            // without it the match keeps pacing in the background. Forget the
            // pacing window so exactly one frame runs immediately on return,
            // then normal pacing resumes; the span must not buy a catch-up
            // frame.
            platform.frame_pacer.reset_for_immediate_frame();
        }
        if active {
            platform.window.request_redraw();
        }
        log::info!(
            "Window {}",
            if active { "activated" } else { "deactivated" }
        );
    }

    /// Apply each visibility or focus edge even when both arrive between draws.
    fn sync_window_movies(
        platform: &PlatformState,
        frontend: &mut FrontendState,
        session: SessionMode,
        presentation: &mut MatchPresentationState,
    ) {
        if !platform.movie_activity(None).runs() {
            frontend.main_menu_movie_clock.pause();
        }
        if let Some(movie) = frontend.fullscreen_movie.as_mut() {
            movie.sync_window(platform, session);
        }
        crate::app::radar_movie::sync_activity(platform, session, presentation);
    }

    /// Apply one window-visibility edge. Waking on the un-hide edge is what
    /// gets the parked redraw loop turning again.
    pub(super) fn set_window_hidden(
        platform: &mut PlatformState,
        frontend: &mut FrontendState,
        session: SessionMode,
        presentation: &mut MatchPresentationState,
        hidden: bool,
    ) {
        if platform.window_hidden == hidden {
            return;
        }
        platform.window_hidden = hidden;
        Self::sync_window_movies(platform, frontend, session, presentation);
        if !hidden {
            platform.window.request_redraw();
        }
    }

    /// Does the in-scenario modal machine own this Escape press?
    ///
    /// Existing modals handle their own Escape. In the world, ProcessCommand
    /// 55DEE0 resolves bindings first; Options is its55E079 fallback. Opening
    /// B5 before that lookup would consume a reloaded custom Escape binding.
    pub(super) fn in_game_menu_owns_escape(
        presentation: &crate::app::presentation::state::MatchPresentationState,
    ) -> bool {
        crate::ui::pause_menu::escape_belongs_to_modal_machine(presentation.in_game_menu)
    }

    /// Route one Escape press, or the Options Back control, through the
    /// in-scenario modal machine.
    pub(super) fn route_in_game_menu_escape(
        platform: &mut PlatformState,
        renderer: &mut RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &mut ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &mut PersistenceState,
    ) {
        use crate::ui::pause_menu::InGameMenuState;

        let current = match_state.match_presentation.in_game_menu;
        if current == InGameMenuState::Menu
            && (crate::app::input::pause_menu::close_options_list(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
            ) || crate::app::input::pause_menu::back_from_options(
                &mut match_state.match_presentation,
            ))
        {
            return;
        }
        Self::enter_in_game_menu_state(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            current.on_escape(),
        );
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
        platform: &mut PlatformState,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &mut PersistenceState,
        next: crate::ui::pause_menu::InGameMenuState,
    ) {
        use crate::ui::pause_menu::InGameMenuState;

        let previous = match_state.match_presentation.in_game_menu;
        if previous == next {
            return;
        }
        if !previous.is_open() && next.is_open() && !Self::state_machine_admits_modal(match_state) {
            return;
        }
        if previous == InGameMenuState::Menu {
            crate::app::input::pause_menu::on_close(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
            );
        }
        if !next.is_open() {
            // Back in play by any route (Resume, or a cancelled Abort or
            // Surrender): the next Escape opens on Game Controls again.
            match_state.match_presentation.options_page_category = None;
        }
        let was_open = previous.is_open();
        let will_be_open = next.is_open();
        if !was_open && will_be_open {
            // The skirmish shell's chrome atlas, dropped when the match
            // started: the menu's pages draw with it
            // (`native_in_game_shell_active`), so it is back before anything
            // in this frame reads the open state.
            Self::ensure_skirmish_shell_chrome(
                renderer,
                frontend,
                process_assets,
                &persistence.options_profile,
            );
        }
        if was_open != will_be_open {
            let locked = crate::app::input::campaign_input::is_locked(match_state);
            let cursor = &mut match_state.input.cursor_visibility;
            if will_be_open {
                cursor.show_for_menus();
            } else {
                cursor.leave_menus(locked);
            }
        }
        if was_open != will_be_open {
            // StateMachine__EnterPause 0x683EB0 / ExitPause 0x683FB0 bracket
            // every in-game modal in every game mode.
            if will_be_open {
                audio.enter_state_machine_pause();
            } else {
                audio.exit_state_machine_pause();
            }
        }
        if was_open != will_be_open && !match_state.session_mode().is_network() {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            if will_be_open
                || match_state
                    .match_presentation
                    .radar_anim
                    .as_ref()
                    .is_some_and(|radar| radar.movie_pause())
            {
                match_state.scenario_elapsed_clock.pause(now_ms);
            } else {
                match_state.scenario_elapsed_clock.resume(now_ms);
            }
        }
        match_state.match_presentation.in_game_menu = next;
        crate::app::radar_movie::sync_activity(
            platform,
            match_state.session_mode(),
            &mut match_state.match_presentation,
        );
        // The menus' dialogs go; the chrome under them (the sidebar's and
        // the command bar's nodes) stays.
        match_state.match_presentation.dialog_ui.close_all();
        match_state.match_presentation.diplomacy_page = Default::default();
        match_state.match_presentation.sound_dialog = None;
        if will_be_open {
            // Deactivate683EB0/4AEAD0 tears down drag/capture on the way into
            // the modal — WITHOUT dropping a pending building placement or
            // repair/sell (handle_options_hotkey). A release that arrives
            // while the overlay owns the mouse never reaches the tactical
            // body, so anything still armed here would stay latched for the
            // rest of the match: tactical capture freezes edge auto-scroll
            // and an active minimap press swallows every cursor move. The
            // chrome's capture went with `dialog_ui.close_all` above; its
            // pushed looks go here.
            match_state.input.cancel_tactical_drag();
            crate::app::input::chrome_input::cancel(&mut match_state.match_presentation);
        }
        if next == InGameMenuState::Menu {
            match_state.match_presentation.pause_menu_has_saves =
                persistence.repository.has_browser_entry();
            crate::app::input::pause_menu::on_open(
                platform,
                &renderer.performance_overlay,
                frontend,
                match_state,
                process_assets,
                audio.launcher_audio_available,
                &persistence.options_profile,
            );
        }
        if next == InGameMenuState::AbortConfirm {
            crate::app::input::abort::on_open(match_state);
        }

        // An open modal is the pause (`MatchState::paused`); any transition
        // ends the debug pause.
        match_state.debug_pause = false;
        if !next.is_open() {
            // Elapsed modal time must not cause a catch-up frame.
            platform.frame_pacer.reset_for_immediate_frame();
        }
        platform.window.request_redraw();
    }

    /// `State_Machine @ 0x0048C8B0`'s admission of a requested modal
    /// (`0x0048C8C6..0x0048C918`), before `EnterPause`; see
    /// [`Self::state_machine_admission`]. Writes the relatched value.
    fn state_machine_admits_modal(
        match_state: &mut crate::app::match_runtime::state::MatchState,
    ) -> bool {
        if Self::house_outcome_wait_pending(&match_state.sim_runtime) {
            return false;
        }
        // A result that is already published and waiting for its commander
        // clip is decided, but the player may still want out of the clip: the
        // menu has to open for them. Leaving through it cuts the clip instead
        // of queueing an EXIT event the stopped world could never execute.
        if crate::app::commander_feedback::verdict_is_waiting_for_clip(
            &match_state.match_presentation,
        ) {
            return true;
        }
        let sim = match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation);
        let latch = Self::state_machine_admission(
            sim.is_some_and(|sim| !sim.session().uses_campaign_world()),
            match_state.match_presentation.modal_latch,
            sim.and_then(|sim| {
                sim.session()
                    .current_house()
                    .and_then(|house| sim.houses().get(&house))
            }),
        );
        if let Some(latch) = latch {
            match_state.match_presentation.modal_latch = latch;
        }
        latch.is_some()
    }

    /// Outside campaign (GameMode 0 skips the test) a requested modal is
    /// cleared without opening while the local House has won (`+0x1F7`),
    /// lost (`+0x1F8`) or is flagged to die (`+0x1F6`), and while the latch
    /// `[0x822CFC]` is clear unless the House is defeated (`+0x1F5`), which
    /// sets the latch again (`0x0048C911`). `None` refuses; `Some` admits
    /// with the latch's new value.
    pub(crate) fn state_machine_admission(
        uses_match_latch: bool,
        latch: bool,
        local: Option<&crate::sim::house_state::HouseState>,
    ) -> Option<bool> {
        let Some(house) = local.filter(|_| uses_match_latch) else {
            return Some(latch);
        };
        if house.has_won() || house.has_lost() || house.is_to_die() {
            return None;
        }
        (latch || house.is_defeated()).then_some(true)
    }

    /// Every physical or fallback modal commits through the same state/exit owner.
    pub(crate) fn apply_in_game_modal_outcome(
        platform: &mut PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &mut PersistenceState,
        outcome: crate::ui::pause_menu::ModalOutcome,
    ) {
        use crate::ui::pause_menu::ModalOutcome;
        match outcome {
            ModalOutcome::Stay => {}
            ModalOutcome::Enter(next) => Self::enter_in_game_menu_state(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                next,
            ),
            ModalOutcome::LeaveMatch { destruct } => {
                if destruct {
                    Self::queue_local_destruct(match_state);
                }
                Self::exit_match_to_shell(
                    platform,
                    renderer,
                    diag,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                )
            }
            ModalOutcome::Observe => {
                // 0x0048CAB2: DESTRUCT and the latch clear; no EXIT. The
                // latch keeps every modal closed until the House is
                // Defeated, which re-arms it (0x0048C911).
                Self::queue_local_destruct(match_state);
                match_state.match_presentation.modal_latch = false;
                Self::enter_in_game_menu_state(
                    platform,
                    renderer,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                    crate::ui::pause_menu::InGameMenuState::Closed,
                );
            }
            ModalOutcome::RestartScenario => {
                // State machine48CB2E raises8B41C0 and the modal state
                // returns to 0; the router consumes it on the next pass.
                match_state.scenario_restart_requested = true;
                Self::enter_in_game_menu_state(
                    platform,
                    renderer,
                    frontend,
                    match_state,
                    process_assets,
                    audio,
                    persistence,
                    crate::ui::pause_menu::InGameMenuState::Closed,
                );
            }
        }
    }

    /// Queue EVENT_DESTRUCT (type 8) for the local House: B6 results 3 and 4
    /// construct `EventClass(PlayerPtr->ArrayIndex, 8)` (`0x0048CA34`,
    /// `0x0048CAB2`) and add it to the ring only while it holds fewer than
    /// 0x80 events; the latch is cleared either way.
    fn queue_local_destruct(match_state: &mut crate::app::match_runtime::state::MatchState) {
        let Some(owner) = crate::app::input::commands::preferred_local_owner(match_state) else {
            log::warn!("Abort DESTRUCT has no local command owner");
            return;
        };
        if crate::app::input::commands::try_schedule_command(
            match_state,
            &owner,
            crate::sim::command::Command::Destruct,
        )
        .is_none()
        {
            log::warn!("Abort DESTRUCT event could not be queued for '{owner}'");
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
    fn exit_match_to_shell(
        platform: &mut PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &mut PersistenceState,
    ) {
        // The result is already decided and only its commander clip is left.
        // Leaving now cuts that clip: the card, the theme and the score page
        // follow exactly as they would after it — the terminal frame latched
        // the score before this point. Queueing the native EXIT event instead
        // would go nowhere: no frame runs after a published result, so the
        // event would never execute.
        if crate::app::commander_feedback::verdict_is_waiting_for_clip(
            &match_state.match_presentation,
        ) {
            let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            crate::app::commander_feedback::skip_verdict_clip(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                wall_ms,
            );
            Self::enter_in_game_menu_state(
                platform,
                renderer,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                crate::ui::pause_menu::InGameMenuState::Closed,
            );
            return;
        }
        log::info!("Abort Mission confirmed — queueing EXIT event");
        // 0x0048CAA1 clears the admission latch after queueing EXIT, even
        // when the event ring is full: until EXIT runs, State_Machine opens
        // no modal for a player who is not defeated.
        match_state.match_presentation.modal_latch = false;
        if match_state.scenario_exit.is_some() {
            return;
        }
        let Some(owner) = crate::app::input::commands::preferred_local_owner(match_state) else {
            log::warn!("Abort Mission confirmation has no local command owner");
            return;
        };
        if crate::app::input::commands::try_schedule_command(
            match_state,
            &owner,
            crate::sim::command::Command::ExitMatch,
        )
        .is_none()
        {
            log::warn!("Abort Mission EXIT event could not be queued for '{owner}'");
            return;
        }
        if let Some(live) = match_state.live_network.as_mut() {
            // Keep the native EXIT in its queue. The network owner resumes
            // through any pending pause request so this event can execute.
            live.keep_running_for_exit();
        }
        Self::enter_in_game_menu_state(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            crate::ui::pause_menu::InGameMenuState::Closed,
        );
    }

    /// Consume EventClass opcode `0x13`'s executed edge and enter the existing
    /// battle-abort teardown. A repeated drain without another dispatch is a
    /// no-op because the simulation edge is taken.
    pub(super) fn consume_executed_abort_exit(
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
        wall_ms: u64,
    ) {
        if Self::house_outcome_wait_pending(&match_state.sim_runtime) {
            return;
        }
        // Remote EXIT senders run DestroyConnection first (0x0064C9B1).
        crate::app::match_runtime::network_match::drain_departures(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
        crate::app::match_runtime::network_match::post_speed_changes(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
        crate::app::match_runtime::network_match::post_pause_changes(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
        crate::app::match_runtime::network_match::post_connection_changes(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
        crate::app::match_runtime::network_match::post_caught_up(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            &mut audio.sfx_player,
            persistence,
        );
        let local_owner = crate::app::input::commands::preferred_local_owner(match_state);
        let local_owner_id = local_owner.as_deref().and_then(|owner| {
            match_state
                .sim_runtime
                .runtime()
                .map(|rt| &rt.simulation)
                .and_then(|sim| sim.interner().get(owner))
        });
        let local_outcome_exit_ready = Self::local_outcome_exit_ready(match_state);
        // Almost every pass has no EXIT to take. Only one that does takes
        // the world for writing, which waits for a tick on the simulation
        // thread and drops the published frame.
        if !match_state
            .sim_runtime
            .runtime()
            .is_some_and(|rt| rt.simulation.executed_exit_owner_pending())
        {
            return;
        }
        let executed_owner = match_state
            .sim_runtime
            .write_sim(SimWrite::Lifecycle)
            .map(|rt| &mut rt.simulation)
            .and_then(|sim| sim.take_executed_exit_owner());
        let Some(executed_owner) = executed_owner else {
            return;
        };
        if Some(executed_owner) != local_owner_id {
            log::warn!("Ignoring executed EXIT event not owned by the local player");
            return;
        }
        if crate::app::match_runtime::scenario_exit::route_scenario_termination(
            crate::app::match_runtime::scenario_exit::TerminationRequests {
                outcome_ready: local_outcome_exit_ready,
                exit: true,
                ..Default::default()
            },
        ) != Some(crate::app::match_runtime::scenario_exit::TerminationRoute::Exit)
        {
            // Main_Game observes the ready victory/loss route first. The EXIT
            // edge is still consumed, but cannot clear or replace that route.
            return;
        }
        if match_state.scenario_exit.is_some() {
            return;
        }

        // Abort is the independent EXIT-event route: it never inherits the
        // victory/defeat HouseClass SavourDelay or its outcome-voice wait.
        match_state.scenario_outcome = None;
        Self::record_match_result(
            frontend,
            match_state,
            super::cncnet::ExternalMatchResult::Quit,
        );
        //686570 clears the movie queue before its independent audio fade.
        crate::app::radar_movie::retire(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            wall_ms,
        );
        crate::app::commander_feedback::retire(&mut match_state.match_presentation);
        crate::app::chrono_screen::retire(&frontend.screen, match_state, audio, wall_ms);
        let elapsed_seconds = match_state.scenario_elapsed_clock.stop(wall_ms);
        // gamemd provenance: battle abort teardown; verified
        // GameExit__BattleControlTerminated @ 0x00686570 starts Theme's fade
        // (`ThemeClass::Stop(1)` @ 0x006865FA), then fades the independent
        // audio master, bounds its voice pump to 300 timer buckets, and
        // finally hard-stops audio.
        audio.fade_out_theme(wall_ms);
        let destination = Self::aborted_score_destination(
            frontend,
            match_state,
            process_assets,
            &persistence.options_profile,
            elapsed_seconds,
        )
        .unwrap_or(crate::app::match_runtime::scenario_exit::ScenarioExitDestination::MainMenu);
        let showed_score = !matches!(
            &destination,
            crate::app::match_runtime::scenario_exit::ScenarioExitDestination::MainMenu
        );
        let mut scenario_exit =
            crate::app::match_runtime::scenario_exit::ScenarioExitCascade::start(
                wall_ms,
                destination,
            );
        if showed_score {
            scenario_exit = scenario_exit.with_battle_control_terminated();
        }
        // `0x00686570` requests EVA_BattleControlTerminated as an INTERRUPT
        // immediately before waiting for the two simultaneous audio fades.
        if let Some(action) = scenario_exit.take_start_voice_action() {
            Self::apply_scenario_exit_voice_action(match_state, process_assets, audio, action);
        }
        match_state.scenario_exit = Some(scenario_exit);
    }

    /// The score page for a skirmish the player left on purpose, or `None`
    /// when this exit is not a skirmish's.
    ///
    /// Retail returns to the shell on abort. The counters a match has already
    /// earned do not depend on how it ended, and the games this mode follows
    /// (RA3, SC2, AoE2) show their result page after a resign. A mission, a
    /// network match and a co-op round keep their own destinations: the first
    /// owns its result dialogs, the second reports through cncnet, the third
    /// has `CooperativeResult`.
    fn aborted_score_destination(
        frontend: &mut crate::app::frontend::state::FrontendState,
        match_state: &crate::app::match_runtime::state::MatchState,
        process_assets: &crate::app::process_assets::ProcessAssets,
        options_profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
        elapsed_seconds: i32,
    ) -> Option<crate::app::match_runtime::scenario_exit::ScenarioExitDestination> {
        let eligible = match_state
            .sim_runtime
            .runtime()
            .is_some_and(|runtime| !runtime.simulation.session().uses_campaign_world())
            && match_state.live_network.is_none()
            && !crate::app::cooperative_round::applies(match_state)
            && !match_state.map_basic.skip_score.unwrap_or(false);
        if !eligible {
            return None;
        }
        // The page numbers itself from this counter, and the natural exit
        // increments before it builds the model.
        frontend.finished_game_count = frontend.finished_game_count.saturating_add(1);
        log::info!("Abort Mission: the score page follows ({elapsed_seconds} s played)");
        Some(
            crate::app::match_runtime::scenario_exit::ScenarioExitDestination::Score {
                title: crate::app::match_runtime::scenario_exit::outcome_title(
                    crate::sim::house_state::HouseOutcomeKind::Defeat,
                ),
                detail: crate::i18n::tr("VERA:YouLeftTheBattle"),
                model: crate::app::match_runtime::sim_tick::build_aborted_score_screen_model(
                    frontend,
                    match_state,
                    process_assets,
                    options_profile,
                    elapsed_seconds,
                ),
            },
        )
    }
}

impl App {
    /// Commit the complete retained Options/Video/Audio profile on the already
    /// verified quit-confirm boundaries, strictly before teardown. A write
    /// failure is logged by the persistence owner and never blocks quitting.
    /// Retail provenance: `OptionsClass__WriteToINI @ 0x005FAD10`.
    pub(super) fn persist_settings_on_quit(
        platform: &crate::app::PlatformState,
        profile: &crate::app::persistence::options_profile::RetailOptionsProfile,
    ) {
        crate::app::persistence::options::persist_options_profile(platform, profile);
    }

    /// Begin the graceful quit cascade from the main-menu Exit-confirm OK. The
    /// caller persists settings FIRST, then calls this instead of exiting
    /// immediately; `render_frame` drives it to completion and then exits the
    /// event loop. `Main_PrepareSession` case 7 opens with Theme `Stop(1)`
    /// (`0x0052E764..0x0052E76B`), Theme's own 1000 ms stream fade.
    pub(super) fn start_quit_cascade(
        platform: &PlatformState,
        renderer: &RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
    ) {
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            wall_ms,
        );
        crate::app::commander_feedback::retire(&mut match_state.match_presentation);
        crate::app::chrono_screen::retire(&frontend.screen, match_state, audio, wall_ms);
        audio.fade_out_theme(wall_ms);
        frontend.quit_cascade = Some(crate::app::frontend::quit_cascade::QuitCascade::start(
            Instant::now(),
        ));
    }

    pub(super) fn drive_scenario_exit(state: &mut AppState, wall_ms: u64) {
        if state.match_state.scenario_exit.is_none() {
            return;
        }
        let master_moving = state.audio.sound_master_moving();
        let poll_voices = state
            .match_state
            .scenario_exit
            .as_ref()
            .is_some_and(|exit| exit.needs_voice_poll(wall_ms, master_moving));
        let voices_active = poll_voices
            && match (
                &mut state.audio.sfx_player,
                state.process_assets.manager(),
                state.process_assets.audio_catalog(),
            ) {
                (Some(sfx), Some(assets), Some(catalog)) => {
                    sfx.pump_and_check_voices(catalog.eva(), assets, catalog.index())
                }
                _ => false,
            };
        let tick = state
            .match_state
            .scenario_exit
            .as_mut()
            .expect("scenario exit remains present")
            .tick(wall_ms, master_moving, voices_active);

        state
            .audio
            .apply_scenario_exit_tick(&tick, state.process_assets.manager(), wall_ms);
        if !tick.finished {
            return;
        }

        let destination = state.match_state.scenario_exit.as_mut().and_then(
            crate::app::match_runtime::scenario_exit::ScenarioExitCascade::take_destination,
        );
        state.match_state.scenario_exit = None;
        let Some(destination) = destination else {
            log::error!("Scenario exit finished without a destination");
            return;
        };
        // Browser launchers own their result UI. Snapshot before teardown;
        // external launches skip score/movie/next-mission presentation.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        {
            Self::browser_match_ended(
                &state.frontend,
                &state.match_state,
                super::cncnet::ExternalMatchResult::Quit,
            );
            if state.frontend.external_launch {
                Self::return_to_main_menu(state);
                return;
            }
        }
        let movie = state.process_assets.native_rules().and_then(|rules| {
            crate::app::campaign_victory::win_movie(
                &destination,
                &state.match_state.map_basic,
                rules.movies(),
            )
            .map(str::to_owned)
        });
        if let Some(movie) = movie {
            // Keep the stopped world and its campaign identity until the movie
            // returns. The shared movie presenter runs on the shell screen.
            state.frontend.screen = GameScreen::MainMenu;
            Self::start_fullscreen_movie(
                state,
                &movie,
                crate::app::frontend::fullscreen_movie::MovieReturn::CampaignResult(Box::new(
                    destination,
                )),
            );
        } else {
            Self::complete_scenario_exit(state, destination);
        }
    }

    /// The outcome's sole destination dispatch, shared by the audio teardown
    /// and Victory685928's movie return. The latter cannot replay the movie.
    pub(super) fn complete_scenario_exit(
        state: &mut AppState,
        destination: crate::app::match_runtime::scenario_exit::ScenarioExitDestination,
    ) {
        use crate::app::match_runtime::scenario_exit::ScenarioExitDestination;
        match destination {
            ScenarioExitDestination::Score {
                title,
                detail,
                model,
            } => {
                Self::open_score_page(
                    &state.renderer,
                    &mut state.frontend,
                    &state.process_assets,
                    model,
                    title,
                    detail,
                );
            }
            ScenarioExitDestination::CampaignScore {
                title,
                detail,
                model,
            } => {
                Self::open_campaign_score_page(
                    &state.renderer,
                    &mut state.frontend,
                    &state.process_assets,
                    &mut state.audio,
                    model,
                    title,
                    detail,
                );
            }
            ScenarioExitDestination::CampaignVictoryContinue => {
                Self::leave_mission_result_screen(state)
            }
            ScenarioExitDestination::CampaignDefeat => Self::begin_campaign_defeat(state),
            ScenarioExitDestination::CooperativeResult { victory } => {
                crate::app::cooperative_round::open_result(state, victory)
            }
            ScenarioExitDestination::AuthoredCampaignResult(outcome) => {
                crate::app::match_runtime::stage_result::open(state, outcome)
            }
            ScenarioExitDestination::MainMenu => {
                Self::return_to_main_menu(state);
            }
        }
    }

    fn apply_scenario_exit_voice_action(
        match_state: &crate::app::match_runtime::state::MatchState,
        process_assets: &crate::app::process_assets::ProcessAssets,
        audio: &mut crate::app::audio_runtime::AppAudioRuntime,
        action: crate::app::match_runtime::scenario_exit::ScenarioExitVoiceAction,
    ) {
        match action {
            crate::app::match_runtime::scenario_exit::ScenarioExitVoiceAction::InterruptBattleControlTerminated => {
                if match_state.local_player_owner().is_none() {
                    log::warn!("Battle-control termination EVA has no pinned local owner");
                    return;
                }
                // `GameExit::BattleControlTerminated 0x00686616`:
                // `VoxClass::PlayEVA("EVA_BattleControlTerminated", 2)` — the
                // INTERRUPT override; the entry itself is STANDARD CRITICAL.
                if let (Some(sfx), Some(assets), Some(catalog)) = (&mut audio.sfx_player, process_assets.manager(), process_assets.audio_catalog()) {
                    let _ = sfx.play_eva(
                        "EVA_BattleControlTerminated",
                        Some(crate::rules::sound_ini::EvaType::Interrupt),
                        catalog.eva(),
                        assets,
                        catalog.index(),
                    );
                }
            }
        }
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    /// Every in-scenario modal route, including BBB's Keyboard (state 4,
    /// `0x004E1FE0` control 0x52C) and Sound (state 6, control 0x52D)
    /// children that `State_Machine 0x0048C8B0` returns to state 5, commits
    /// through `App::enter_in_game_menu_state`. A direct write skips the
    /// Options-anchor, interaction and BBB-init effects that ride on it.
    #[test]
    fn only_the_transition_owner_writes_the_in_game_menu_state() {
        fn direct_writers(dir: &std::path::Path, writers: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    direct_writers(&path, writers);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs") || path.ends_with("in_game.rs") {
                    continue;
                }
                let source: String = std::fs::read_to_string(&path)
                    .unwrap()
                    .split_whitespace()
                    .collect();
                if source
                    .match_indices(".in_game_menu=")
                    .any(|(at, write)| !source[at + write.len()..].starts_with('='))
                {
                    writers.push(path);
                }
            }
        }
        let mut writers = Vec::new();
        direct_writers(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app"),
            &mut writers,
        );
        assert!(writers.is_empty(), "{writers:?}");
    }
}
