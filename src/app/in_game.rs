//! Match-session focus, modal, return, quit and scenario-exit control.

use super::{App, AppState, GameScreen, Instant, ModifiersState};

impl App {
    /// Return the live Scenario cursor and header to the multiplayer shell.
    pub(super) fn capture_returned_multiplayer_scenario(state: &mut AppState) {
        let gameplay_rng = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
            .map(|simulation| {
                (
                    simulation.clone_scenario_rng(),
                    simulation.session().start_header(),
                )
            });
        if let Some((gameplay_rng, start_header)) = gameplay_rng
            && state
                .frontend
                .multiplayer_shell_runtime
                .capture_returned_gameplay_scenario(gameplay_rng, start_header)
        {
            log::info!("Returned gameplay Scenario cursor and header to the multiplayer shell");
        }
    }

    pub(super) fn return_to_main_menu(state: &mut AppState) {
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
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

    /// Stop a running scenario without an outcome teardown: its world,
    /// modal, audio and diagnostic owners, leaving the runtime retained.
    /// Shared by the EXIT return and the campaign restart (`0x006863E0`
    /// stops the elapsed clock, theme, sounds and speech before
    /// Start_Scenario).
    fn retire_running_scenario(state: &mut AppState) {
        state.match_state.input.planning.reset();
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(state, wall_ms);
        crate::app::chrono_screen::retire(state, wall_ms);
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
            // Main_Game48CCC0 clears ScenarioStarted[A8ED5C] at48CEBF
            // after the match returns (EBX=0 from48CCD4); the restart clears
            // it at686424. The runtime may remain retained, but Tag
            // destruction must see a stopped session.
            // Source: original gamemd.exe instructions, SHA2561cdd1180..84298c.
            runtime.simulation.retire_scenario_started();
        }
        state.match_state.scenario_restart_requested = false;
        state.match_state.debug_pause = false;
        state.match_state.match_presentation.in_game_menu =
            crate::ui::pause_menu::InGameMenuState::Closed;
        state.match_state.match_presentation.in_game_options_anchor = None;
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.diplomacy = Default::default();
        state.match_state.match_presentation.sound_dialog = None;
        state.match_state.match_presentation.saved_game_browser = None;
        state.match_state.network_wait = Default::default();
        // Persist the deterministic diagnostic log before leaving the scenario.
        // Runtime and presentation resources remain retained in the shell.
        crate::app::match_runtime::sim_tick::flush_replay_log(state);
        Self::capture_returned_multiplayer_scenario(state);
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
        }
        state.audio.set_master_output_scale(1.0);
        state.match_state.match_audio.reset_for_new_match();
        state.match_state.input.zoom_level = 1.0;
        state.match_state.input.zoom_target = 1.0;
    }

    /// Start_Scenario's inputs for the installed campaign mission again
    /// (Scenario+0x125C, DL 0, Scenario+0x34CC). `None` for a restored saved
    /// game, which carries no campaign index.
    pub(super) fn prepare_campaign_restart(
        state: &AppState,
    ) -> Option<crate::app::match_bootstrap::PreparedCampaignStartup> {
        let campaign = state.match_state.startup.installed_campaign()?;
        let game_speed = state
            .match_state
            .sim_runtime
            .as_ref()
            .map_or(state.persistence.options_profile.game_speed, |runtime| {
                runtime.simulation.session().game_options().game_speed
            });
        Some(campaign.restart(
            state.persistence.options_profile.difficulty,
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
        let Some(startup) = Self::prepare_campaign_restart(state) else {
            log::warn!("Restart has no installed campaign start; returning to the shell");
            Self::return_to_main_menu(state);
            return;
        };
        log::info!("Restarting campaign mission {}", startup.scenario());
        // The router clears all four termination bytes before 6863E0: an
        // executed EXIT edge cannot follow the restart into the shell.
        if let Some(runtime) = state.match_state.sim_runtime.as_mut() {
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
        if !std::mem::take(&mut state.match_state.scenario_restart_requested) {
            return;
        }
        let outcome_ready =
            state.match_state.scenario_outcome.is_some() || Self::local_outcome_exit_ready(state);
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

    fn local_outcome_exit_ready(state: &AppState) -> bool {
        let Some(sim) = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|rt| &rt.simulation)
        else {
            return false;
        };
        crate::app::input::commands::preferred_local_owner(state)
            .as_deref()
            .and_then(|owner| sim.interner().get(owner))
            .is_some_and(|owner| sim.ready_outcome_for_owner(owner).is_some())
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
        state
            .match_state
            .match_presentation
            .radar_movies
            .set_active(active);
        state.match_state.input.keys_held.clear();
        state.match_state.input.hotkey_modifiers = ModifiersState::empty();
        state.platform.live_modifiers = ModifiersState::empty();
        state.platform.held_modifier_keys.clear();
        state.match_state.input.type_select.clear_held();
        crate::app::input::planning_mode::release_on_focus_change(state);
        // gamemd-derived: the `WM_ACTIVATEAPP` changed edge at 0x007778AC
        // stops/restores the primary DirectSound output through 0x00407020 /
        // 0x00407040 while secondary playback cursors continue. Keep this on
        // the same edge as the main-loop gate rather than pausing each stream.
        // Without the configured freeze the world keeps running, so its audio
        // keeps playing and the pacer never has a deactivated span to forget.
        if state.platform.pause_on_focus_loss {
            if let Some(player) = state.audio.music_player.as_mut() {
                player.set_focus_output_active(active);
            }
            if let Some(player) = state.audio.sfx_player.as_mut() {
                player.set_focus_output_active(active);
            }
            if active {
                // The deactivated span must not buy a catch-up frame: forget
                // the pacing window so exactly one frame runs immediately,
                // then normal pacing resumes.
                state.platform.frame_pacer.reset_for_immediate_frame();
            }
        }
        if active {
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
    /// Existing modals handle their own Escape. In the world, ProcessCommand
    /// 55DEE0 resolves bindings first; Options is its55E079 fallback. Opening
    /// B5 before that lookup would consume a reloaded custom Escape binding.
    pub(super) fn in_game_menu_owns_escape(state: &AppState) -> bool {
        crate::ui::pause_menu::escape_belongs_to_modal_machine(
            state.match_state.match_presentation.in_game_menu,
        )
    }

    /// Route one Escape press, or the Options Back control, through the
    /// in-scenario modal machine.
    pub(super) fn route_in_game_menu_escape(state: &mut AppState) {
        use crate::ui::pause_menu::InGameMenuState;

        let current = state.match_state.match_presentation.in_game_menu;
        // Escape and Back leave Options the same way — apply and persist the
        // touched `[Options]` values, then return to the menu — so the two
        // cannot disagree about what was saved.
        if current == InGameMenuState::Options {
            crate::app::persistence::options::accept_in_game_options(state);
            log::info!(
                "In-game Options closed at {} tps",
                state.match_state.sim_speed_tps
            );
        }
        Self::enter_in_game_menu_state(state, current.on_escape());
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
        if !previous.is_open() && next.is_open() && !Self::state_machine_admits_modal(state) {
            return;
        }
        let was_open = previous.is_open();
        let will_be_open = next.is_open();
        if was_open != will_be_open {
            let locked = crate::app::input::campaign_input::is_locked(state);
            let cursor = &mut state.match_state.input.cursor_visibility;
            if will_be_open {
                cursor.show_for_menus();
            } else {
                cursor.leave_menus(locked);
            }
        }
        if was_open != will_be_open && !state.match_state.session.mode().is_network() {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            if will_be_open
                || state
                    .match_state
                    .match_presentation
                    .radar_anim
                    .as_ref()
                    .is_some_and(|radar| radar.movie_pause())
            {
                state.match_state.scenario_elapsed_clock.pause(now_ms);
            } else {
                state.match_state.scenario_elapsed_clock.resume(now_ms);
            }
        }
        state.match_state.match_presentation.in_game_menu = next;
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.diplomacy = Default::default();
        state.match_state.match_presentation.sound_dialog = None;
        if will_be_open {
            // Deactivate683EB0/4AEAD0 tears down drag/capture on the way into
            // the modal — WITHOUT dropping a pending building placement or
            // repair/sell (handle_options_hotkey). A release that arrives
            // while the overlay owns the mouse never reaches the tactical
            // body, so anything still armed here would stay latched for the
            // rest of the match: tactical capture freezes edge auto-scroll,
            // `minimap_dragging` swallows every cursor move, and a stale
            // gadget sticky capture eats the next click.
            state.match_state.input.cancel_tactical_drag();
            state
                .match_state
                .match_presentation
                .in_game_gadgets
                .cancel_pointer_capture();
        }
        if next == InGameMenuState::Menu {
            state.match_state.match_presentation.pause_menu_has_saves =
                !state.persistence.repository.browser_entries().is_empty();
            crate::app::input::pause_menu::on_open(state);
        }
        if next == InGameMenuState::AbortConfirm {
            crate::app::input::abort::on_open(state);
        }

        // Leaving Options: drop the cached `0xBBB` hit-test anchor so the
        // overlay's own mouse handler cannot claim clicks aimed at the menu.
        if previous == InGameMenuState::Options {
            state.match_state.match_presentation.in_game_options_anchor = None;
        }
        if next == InGameMenuState::Options {
            crate::app::input::in_game_options::on_open(state);
        }

        // An open modal is the pause (`MatchState::paused`); any transition
        // ends the debug pause.
        state.match_state.debug_pause = false;
        if !next.is_open() {
            // Elapsed modal time must not cause a catch-up frame.
            state.platform.frame_pacer.reset_for_immediate_frame();
        }
        state.platform.window.request_redraw();
    }

    /// `State_Machine @ 0x0048C8B0`'s admission of a requested modal
    /// (`0x0048C8C6..0x0048C918`), before `EnterPause`; see
    /// [`Self::state_machine_admission`]. Writes the relatched value.
    fn state_machine_admits_modal(state: &mut AppState) -> bool {
        let sim = state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation);
        let latch = Self::state_machine_admission(
            state.match_state.session.mode().is_nonzero(),
            state.match_state.match_presentation.modal_latch,
            sim.and_then(|sim| {
                sim.session()
                    .current_house()
                    .and_then(|house| sim.houses().get(&house))
            }),
        );
        if let Some(latch) = latch {
            state.match_state.match_presentation.modal_latch = latch;
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
        game_mode_nonzero: bool,
        latch: bool,
        local: Option<&crate::sim::house_state::HouseState>,
    ) -> Option<bool> {
        let Some(house) = local.filter(|_| game_mode_nonzero) else {
            return Some(latch);
        };
        if house.has_won() || house.has_lost() || house.is_to_die() {
            return None;
        }
        (latch || house.is_defeated()).then_some(true)
    }

    /// Every physical or fallback modal commits through the same state/exit owner.
    pub(crate) fn apply_in_game_modal_outcome(
        state: &mut AppState,
        outcome: crate::ui::pause_menu::ModalOutcome,
    ) {
        use crate::ui::pause_menu::ModalOutcome;
        match outcome {
            ModalOutcome::Stay => {}
            ModalOutcome::Enter(next) => Self::enter_in_game_menu_state(state, next),
            ModalOutcome::LeaveMatch { destruct } => {
                if destruct {
                    Self::queue_local_destruct(state);
                }
                Self::exit_match_to_shell(state)
            }
            ModalOutcome::Observe => {
                // 0x0048CAB2: DESTRUCT and the latch clear; no EXIT. The
                // latch keeps every modal closed until the House is
                // Defeated, which re-arms it (0x0048C911).
                Self::queue_local_destruct(state);
                state.match_state.match_presentation.modal_latch = false;
                Self::enter_in_game_menu_state(
                    state,
                    crate::ui::pause_menu::InGameMenuState::Closed,
                );
            }
            ModalOutcome::RestartScenario => {
                // State machine48CB2E raises8B41C0 and the modal state
                // returns to 0; the router consumes it on the next pass.
                state.match_state.scenario_restart_requested = true;
                Self::enter_in_game_menu_state(
                    state,
                    crate::ui::pause_menu::InGameMenuState::Closed,
                );
            }
        }
    }

    /// Queue EVENT_DESTRUCT (type 8) for the local House: B6 results 3 and 4
    /// construct `EventClass(PlayerPtr->ArrayIndex, 8)` (`0x0048CA34`,
    /// `0x0048CAB2`) and add it to the ring only while it holds fewer than
    /// 0x80 events; the latch is cleared either way.
    fn queue_local_destruct(state: &mut AppState) {
        let Some(owner) = crate::app::input::commands::preferred_local_owner(state) else {
            log::warn!("Abort DESTRUCT has no local command owner");
            return;
        };
        if crate::app::input::commands::try_schedule_command(
            state,
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
    fn exit_match_to_shell(state: &mut AppState) {
        log::info!("Abort Mission confirmed — queueing EXIT event");
        // 0x0048CAA1 clears the admission latch after queueing EXIT, even
        // when the event ring is full: until EXIT runs, State_Machine opens
        // no modal for a player who is not defeated.
        state.match_state.match_presentation.modal_latch = false;
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
        // Remote EXIT senders run DestroyConnection first (0x0064C9B1).
        crate::app::match_runtime::network_match::drain_departures(state);
        let local_owner = crate::app::input::commands::preferred_local_owner(state);
        let local_owner_id = local_owner.as_deref().and_then(|owner| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| &rt.simulation)
                .and_then(|sim| sim.interner().get(owner))
        });
        let local_outcome_exit_ready = Self::local_outcome_exit_ready(state);
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
        if state.match_state.scenario_exit.is_some() {
            return;
        }

        // Abort is the independent EXIT-event route: it never inherits the
        // victory/defeat HouseClass SavourDelay or its outcome-voice wait.
        state.match_state.scenario_outcome = None;
        //686570 clears the movie queue before its independent audio fade.
        crate::app::radar_movie::retire(state, wall_ms);
        crate::app::chrono_screen::retire(state, wall_ms);
        let _ = state.match_state.scenario_elapsed_clock.stop(wall_ms);
        // gamemd provenance: battle abort teardown; verified
        // GameExit__BattleControlTerminated @ 0x00686570 starts Theme's fade
        // (`ThemeClass::Stop(1)` @ 0x006865FA), then fades the independent
        // audio master, bounds its voice pump to 300 timer buckets, and
        // finally hard-stops audio.
        state.audio.fade_out_theme(wall_ms);
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
        let wall_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        crate::app::radar_movie::retire(state, wall_ms);
        crate::app::chrono_screen::retire(state, wall_ms);
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
            && match (
                &mut state.audio.sfx_player,
                state.process_assets.manager(),
                state.process_assets.audio_catalog(),
            ) {
                (Some(sfx), Some(assets), Some(catalog)) => {
                    sfx.pump_and_check_voices(catalog.sounds(), assets, catalog.index())
                }
                _ => false,
            };
        let tick = state
            .match_state
            .scenario_exit
            .as_mut()
            .expect("scenario exit remains present")
            .tick(wall_ms, voices_active);

        if let Some(scale) = tick.master_output_scale {
            state.audio.set_master_output_scale(scale);
        }
        if tick.stop_audio {
            state.audio.stop_theme();
            if let Some(player) = state.audio.sfx_player.as_mut() {
                player.stop_all();
            }
            state.audio.set_master_output_scale(1.0);
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
                Self::open_score_page(state, model, title, detail);
            }
            Some(
                crate::app::match_runtime::scenario_exit::ScenarioExitDestination::CampaignScore {
                    title,
                    detail,
                    model,
                },
            ) => {
                Self::open_campaign_score_page(state, model, title, detail);
            }
            Some(
                crate::app::match_runtime::scenario_exit::ScenarioExitDestination::CampaignVictoryContinue,
            ) => Self::leave_mission_result_screen(state),
            Some(crate::app::match_runtime::scenario_exit::ScenarioExitDestination::CampaignDefeat) => {
                Self::begin_campaign_defeat(state)
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
                if state.match_state.local_player_owner().is_none() {
                    log::warn!("Battle-control termination EVA has no pinned local owner");
                    return;
                }
                // `GameExit::BattleControlTerminated 0x00686616`:
                // `VoxClass::PlayEVA("EVA_BattleControlTerminated", 2)` — the
                // INTERRUPT override; the entry itself is STANDARD CRITICAL.
                let eva_side = crate::app::presentation::building_anim::local_eva_side(state);
                if let (Some(sfx), Some(assets), Some(catalog)) = (&mut state.audio.sfx_player, state.process_assets.manager(), state.process_assets.audio_catalog()) {
                    let _ = sfx.play_eva(
                        "EVA_BattleControlTerminated",
                        Some(crate::rules::sound_ini::EvaType::Interrupt),
                        catalog.eva(),
                        eva_side,
                        catalog.sounds(),
                        assets,
                        catalog.index(),
                    );
                }
            }
        }
    }
}
