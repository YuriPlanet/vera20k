//! Top-level frame orchestration, screen dispatch, presentation, and readback.
//!
//! Simulation admission, draw composition, submit/present, transition commits,
//! captures, and loading-after-present remain in their original source order.

use super::input::precedence::{ModalPage, ScreenPage};
use super::loading::transitions;
use super::{
    ActiveEventLoop, App, AppState, GameScreen, Instant, Result, frontend::startup_splash, render,
    sim_tick,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellFramePreludeStep {
    CommitTeardown,
    MaintainIntro,
    ObserveEntry,
}

/// A finished teardown slide destroys its dialog and runs the result
/// (`0x00622720` returns into PrepareSession) before the next dialog is armed
/// or the surface is acquired, so the frame after the last slide-out tick is
/// already the next screen's first frame.
const MAIN_MENU_SHELL_PRELUDE: &[ShellFramePreludeStep] = &[
    ShellFramePreludeStep::CommitTeardown,
    ShellFramePreludeStep::MaintainIntro,
    ShellFramePreludeStep::ObserveEntry,
];

/// The runtime admission belongs to the pass before Logic mutates pause state.
#[derive(Default)]
pub(super) struct RuntimeServiceResult {
    pub(super) quit_requested: bool,
    pub(super) main_tick_admitted: bool,
    /// Offset selected for this service's draw: normal Input result, the
    /// paused render-before-Input snapshot, or the last presented freeze value.
    pub(super) screen_shake_offset: Option<[i32; 2]>,
    completed_frames: u32,
    last_frame_completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameRenderOutcome {
    Presented,
    Unavailable,
    Deferred,
}

/// The composition's full-height battlefield/command-bar region. Native
/// UpdatePrimarySurface @0x004F4780 copies the composite into primary pixels
/// and keeps the sidebar surface fixed; it shifts the complete vertical span,
/// including the command bar beneath the tactical viewport.
fn shake_central_rect(state: &AppState, target: &wgpu::Texture) -> [u32; 4] {
    let source_height = state.render_height();
    let sidebar_frame = state.sidebar_frame();
    let central_source_width = sidebar_frame.column().0;

    if let Some(geometry) = state
        .renderer
        .upscale_pass
        .as_ref()
        .map(|pass| pass.geometry())
    {
        let (frame_width, frame_height) = geometry.frame_size();
        let central_width = central_source_width
            .saturating_mul(geometry.percent)
            .saturating_div(100);
        [
            geometry.origin.0,
            geometry.origin.1,
            central_width.min(frame_width),
            frame_height.min(target.height().saturating_sub(geometry.origin.1)),
        ]
    } else {
        [
            0,
            0,
            central_source_width.min(target.width()),
            source_height.min(target.height()),
        ]
    }
}

fn apply_screen_shake(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::Texture,
    offset_px: [i32; 2],
) {
    let central_rect = shake_central_rect(state, target);
    let device = &state.renderer.gpu.device;
    state.renderer.screen_presentation.apply_offset_px(
        device,
        encoder,
        target,
        central_rect,
        offset_px,
    );
}

impl App {
    /// Service the match runtime for one pass: the outcome voice wait, the
    /// scenario exit cascade, the quit cascade, the audio pump and, when
    /// admitted, one simulation advance. Shared by the render pass and the
    /// hidden-window service loop in `about_to_wait`, so the world keeps the
    /// same cadence whether or not a frame is drawn. Returns quit state and
    /// the pre-Logic Main_Tick admission for its later consumers.
    pub(super) fn service_match_runtime(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
        simulation_allowed: bool,
    ) -> RuntimeServiceResult {
        let started = Instant::now();
        let mut result = RuntimeServiceResult::default();
        let mut attempts = 0;
        loop {
            let before = state.match_state.sim_runtime.as_ref().map(|runtime| {
                (
                    runtime.simulation.session().tick(),
                    runtime.simulation.session().binary_frame(),
                )
            });
            let pass = Self::service_match_runtime_once(state, event_loop, simulation_allowed);
            result.main_tick_admitted |= pass.main_tick_admitted;
            result.screen_shake_offset = pass.screen_shake_offset;
            result.quit_requested |= pass.quit_requested;
            let after = state.match_state.sim_runtime.as_ref().map(|runtime| {
                (
                    runtime.simulation.session().tick(),
                    runtime.simulation.session().binary_frame(),
                )
            });
            result.last_frame_completed = pass.main_tick_admitted
                && before.zip(after).is_some_and(|(before, after)| {
                    after.0 == before.0.wrapping_add(1) && after.1 == before.1.wrapping_add(1)
                });
            result.completed_frames += u32::from(result.last_frame_completed);
            attempts += 1;
            if result.quit_requested
                || !simulation_allowed
                || state.frontend.screen != GameScreen::InGame
                || state.match_state.scenario_exit.is_some()
                || state.match_state.scenario_outcome.is_some()
                || Self::house_outcome_wait_pending(state)
                || crate::app::match_runtime::network_failure::pending(state)
                || !crate::app::match_runtime::frame_pacer::LocalFramePacer::permits_catch_up(
                    attempts,
                    started.elapsed(),
                )
            {
                break;
            }
            let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
                break;
            };
            let session = runtime.simulation.session();
            if before == Some((session.tick(), session.binary_frame())) {
                break;
            }
            let Some(live) = state
                .match_state
                .live_network
                .as_ref()
                .filter(|live| live.uses_sequenced_lockstep())
            else {
                break;
            };
            if !live.has_terminal_network_error() {
                let frame = session.binary_frame();
                let frontier = live.committed_frontier();
                let may_continue = if let Some(timing) = live.sequenced_timing().filter(|_| {
                    !state.match_state.paused()
                        && !state.focus_freeze_active()
                        && !session.scenario_paused()
                        && !state.match_state.match_diagnostics.is_replay_playback()
                }) {
                    let recovering = live
                        .recovery_status(
                            crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
                            frame,
                        )
                        .is_some();
                    state
                        .platform
                        .frame_pacer
                        .online_frame_decision(
                            Instant::now(),
                            frame,
                            frontier,
                            timing,
                            runtime.simulation.pending_network_wait(),
                            recovering,
                        )
                        .admits()
                } else {
                    crate::app::match_runtime::frame_pacer::LocalFramePacer::admits_canonical_frame(
                        frame, frontier,
                    )
                };
                if !may_continue {
                    break;
                }
            }
        }
        result
    }

    /// One retained production transaction. Catch-up repeats this owner,
    /// including transport, output consumers and terminal routing; it never
    /// repeats Logic inside a suspended frame or draws intermediate worlds.
    fn service_match_runtime_once(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
        simulation_allowed: bool,
    ) -> RuntimeServiceResult {
        let mut main_tick_admitted = false;
        // GScreen Input reads the current native frame parity but does not
        // advance it. Hidden background service still owns Input while the
        // match runs; the focus-freeze sleep path and shell do not.
        let network_wait_pending = state
            .match_state
            .sim_runtime
            .as_ref()
            .is_some_and(|rt| rt.simulation.pending_network_wait());
        let in_game = state.frontend.screen == GameScreen::InGame;
        // Wait_For_Players still enters GScreen Input at 0x006485B7 and
        // 0x00649827; repeated services read the same binary-frame parity.
        let input_is_active = in_game
            && (!state.focus_freeze_active()
                || Self::house_outcome_wait_pending(state)
                || network_wait_pending);
        let input_frame = input_is_active.then(|| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| rt.simulation.session().binary_frame())
        });
        let screen_shake_offset = if in_game {
            let paused_render_path = state.match_state.match_presentation.in_game_menu.is_open();
            Some(
                state
                    .match_state
                    .match_presentation
                    .screen_shake
                    .service_offset_snapshot(input_frame.flatten(), paused_render_path),
            )
        } else {
            None
        };
        #[cfg(feature = "telemetry-dashboard")]
        crate::app::telemetry_capture::synchronize_observer(state);
        #[cfg(feature = "debug-tools")]
        crate::app::debug_tools::service(state);
        // HouseClass keeps simulating for SavourDelay, then blocks on the
        // current outcome Vox before it raises the victory/defeat exit global.
        // Drive that gate before deciding whether another sim frame is legal.
        let scenario_now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        if state.match_state.match_diagnostics.is_replay_playback() {
            // Playback never services lobby/relay or routes recorded EXIT and
            // outcomes into live teardown. Keep the final battlefield visible.
            crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);
            if in_game && simulation_allowed {
                main_tick_admitted =
                    sim_tick::advance_in_game_runtime(state, scenario_now_ms, false);
            }
            return RuntimeServiceResult {
                main_tick_admitted,
                screen_shake_offset,
                ..Default::default()
            };
        }
        crate::app::lan_service::poll(state, scenario_now_ms);
        Self::poll_external_launch_progress(state);
        Self::poll_spawn_actions(state);
        // Resume receipts must release only the relay clock hold even while
        // focus or a local modal prevents another simulation transaction.
        state
            .match_state
            .sync_multiplayer_pause_clock(scenario_now_ms);
        if crate::app::match_runtime::network_failure::pending(state) {
            // ExecuteDoList64CD0C blocks inside ShellMessageBox5D3490. Its
            // message pump still reaches Network_ServiceLoop48D080/audio,
            // while Main_Tick's terminal tests and frame tail wait for Stop.
            crate::app::match_runtime::network_failure::show(state);
            crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);
            let frame_stopped = state.match_state.frame_stopped;
            if crate::app::match_runtime::network_failure::take_acknowledged(state) {
                // A stopped frame has no suspended native frame to finish and
                // its world must not run again: Stop goes straight to the shell.
                if !frame_stopped {
                    main_tick_admitted =
                        crate::app::match_runtime::sim_tick::resume_after_network_failure(state);
                    // Main_Tick55DE4F..55DE71 and Main_Game55CFD0 retain their
                    // independent restart/EXIT/outcome priority after the modal.
                    Self::consume_scenario_restart_request(state);
                    Self::consume_executed_abort_exit(state, scenario_now_ms);
                    crate::app::match_runtime::sim_tick::drive_local_player_outcome_exit(
                        state,
                        scenario_now_ms,
                    );
                    Self::drive_scenario_exit(state, scenario_now_ms);
                }
                if matches!(state.frontend.screen, GameScreen::InGame)
                    && state.match_state.scenario_exit.is_none()
                    && state.match_state.scenario_outcome.is_none()
                {
                    // ProcessNetwork6475F0 clears GameActive at64826F;
                    // Main_Game55D347 returns directly to shell cleanup.
                    // This is the same route as WaitPass::Leave, without an
                    // EXIT command or BattleControlTerminated686570.
                    Self::return_disconnected_to_main_menu(state);
                }
            }
            return RuntimeServiceResult {
                main_tick_admitted,
                screen_shake_offset,
                ..Default::default()
            };
        }
        if Self::house_outcome_wait_pending(state) {
            // The native House wait services Network_ServiceLoop/audio before
            // continuing this same House. Keep that service live while later
            // House callbacks and MainTick's termination router remain held.
            crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);
            main_tick_admitted = sim_tick::advance_in_game_runtime(state, scenario_now_ms, false);
            if !Self::house_outcome_wait_pending(state)
                && !crate::app::match_runtime::network_failure::pending(state)
            {
                Self::consume_scenario_restart_request(state);
                Self::consume_executed_abort_exit(state, scenario_now_ms);
                crate::app::match_runtime::sim_tick::drive_local_player_outcome_exit(
                    state,
                    scenario_now_ms,
                );
                Self::drive_scenario_exit(state, scenario_now_ms);
            }
            return RuntimeServiceResult {
                main_tick_admitted,
                screen_shake_offset,
                ..Default::default()
            };
        }
        // Main_Tick55DE4F skips the frame while the restart byte is set and
        // Main_Game's router55CFD0 takes it before EXIT.
        Self::consume_scenario_restart_request(state);
        Self::consume_executed_abort_exit(state, scenario_now_ms);
        crate::app::match_runtime::sim_tick::drive_local_player_outcome_exit(
            state,
            scenario_now_ms,
        );

        // The native victory/defeat handlers synchronously finish their audio
        // teardown before entering the score dialog. Drive the equivalent
        // sequence before either another sim frame or the destination screen.
        Self::drive_scenario_exit(state, scenario_now_ms);

        // Drive the graceful quit wait (started on Exit-confirm OK). It
        // (`0x0052E79C..0x0052E7E4`) holds while Theme `Still_Playing` and then
        // `VoxClass::PumpAndCheckActive @ 0x007529E0` both answer yes.
        if state.frontend.quit_cascade.is_some() {
            let now = Instant::now();
            let waiting = state.audio.theme_still_playing()
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
            let finished = state
                .frontend
                .quit_cascade
                .as_ref()
                .expect("cascade present")
                .finished(now, waiting);
            if finished {
                // `Stop(0)` (`0x0052E7EC`) finds nothing active after the
                // `Stop(1)` that began the wait; the exit ends any fade tail.
                state.audio.stop_theme();
                state.frontend.quit_cascade = None;
                // Browser launchers receive the report after network cleanup,
                // immediately before their engine exits.
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                if state.frontend.external_launch {
                    Self::finish_browser_external_launch(state, None);
                    Self::poll_browser_exit(state, event_loop);
                    return RuntimeServiceResult {
                        quit_requested: true,
                        main_tick_admitted,
                        screen_shake_offset,
                        ..Default::default()
                    };
                }
                event_loop.exit();
                return RuntimeServiceResult {
                    quit_requested: true,
                    main_tick_admitted,
                    screen_shake_offset,
                    ..Default::default()
                };
            }
        }

        // The audio service pass. gamemd drives `AudioSystem::Pump @
        // 0x00406F70` from `Network_ServiceLoop @ 0x0048D080`, which the main
        // tick, the network modes' frame throttler (`Main::ThrottleFrame @
        // 0x0055E160` skips it in `g_GameMode` 0 and 5), the modal dialog
        // pump, the shell dialog loop and the loading screens all reach — so
        // the sound arbiter, the EVA queue and `ThemeClass::AI` keep being
        // serviced on the main menu, behind a pause or an open menu. It
        // therefore sits here, outside every screen and simulation gate, and
        // carries its own `> 33 ms` rate limit. Offline, the running match
        // pumps inside Main_Tick instead, and a parked window not at all;
        // this pass then keeps only the outputs.
        let main_tick_pumps_audio = crate::app::match_runtime::sim_tick::main_tick_owns_audio_pump(
            state,
            simulation_allowed,
        );
        if main_tick_pumps_audio {
            crate::app::match_runtime::sim_tick::service_audio_outputs(state, scenario_now_ms);
        } else {
            crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);
        }
        crate::app::radar_movie::service(state, scenario_now_ms);
        crate::app::commander_feedback::service(state, scenario_now_ms);

        // Deactivated windows do not simulate when `pause_on_focus_loss` is
        // configured. gamemd parks its main tick in a sleep-and-network-only
        // loop while the app is not the foreground, so the world is exactly
        // where the player left it on Alt+Tab return; the default keeps the
        // match running behind other windows instead. The gate sits at the
        // call site, not inside the runtime, so a focus edge never re-anchors
        // the frame pacer on its own.
        let network_frame_pending = state
            .match_state
            .sim_runtime
            .as_ref()
            .is_some_and(|rt| rt.simulation.pending_network_wait());
        if matches!(state.frontend.screen, GameScreen::InGame)
            && (network_frame_pending
                || (simulation_allowed
                    && !state.focus_freeze_active()
                    && state.match_state.scenario_exit.is_none()
                    && state.match_state.scenario_outcome.is_none()))
        {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            // Wait_For_Players runs inside the frame's network service, ahead
            // of the frame it holds back.
            if crate::app::match_runtime::network_match::service_wait(state, now_ms)
                == crate::app::match_runtime::network_match::WaitPass::Leave
            {
                Self::leave_network_wait_to_main_menu(state);
                return RuntimeServiceResult {
                    main_tick_admitted,
                    screen_shake_offset,
                    ..Default::default()
                };
            }
            main_tick_admitted =
                sim_tick::advance_in_game_runtime(state, now_ms, main_tick_pumps_audio);
            if Self::house_outcome_wait_pending(state)
                || crate::app::match_runtime::network_failure::pending(state)
            {
                return RuntimeServiceResult {
                    main_tick_admitted,
                    screen_shake_offset,
                    ..Default::default()
                };
            }
            // EventClass EXIT is dispatched at the simulation tail. Consume
            // its terminal edge before any outcome route can claim teardown.
            Self::consume_executed_abort_exit(state, now_ms);
            // The House's own Vox wait has finished before its result request.
            // Route the retained request after all remaining frame callbacks.
            crate::app::match_runtime::sim_tick::drive_local_player_outcome_exit(state, now_ms);
            Self::drive_scenario_exit(state, now_ms);
        }
        #[cfg(feature = "telemetry-dashboard")]
        crate::app::telemetry_capture::publish_if_due(state);
        RuntimeServiceResult {
            quit_requested: false,
            main_tick_admitted,
            screen_shake_offset,
            ..Default::default()
        }
    }

    /// The render continuation is allowed only outside every modal, focus,
    /// exact-step and terminal path. The sequencer remains frame authority.
    pub(super) fn next_online_render_frame(state: &AppState) -> Option<(u32, u32)> {
        if state.frontend.screen != GameScreen::InGame
            || !state.platform.window_active
            || state.platform.window_hidden
            || state.focus_freeze_active()
            || state.match_state.paused()
            || state.diag.debug_frame_step_requested
            || state.frontend.keyboard_dialog.is_some()
            || state.frontend.shell_message_box.is_some()
            || state.frontend.quit_cascade.is_some()
            || state.match_state.scenario_exit.is_some()
            || state.match_state.scenario_outcome.is_some()
            || Self::house_outcome_wait_pending(state)
            || crate::app::match_runtime::network_failure::pending(state)
            || state.match_state.network_wait.dialog_open()
            || state.match_state.match_diagnostics.is_replay_playback()
        {
            return None;
        }
        let live =
            state.match_state.live_network.as_ref().filter(|live| {
                live.uses_sequenced_lockstep() && !live.has_terminal_network_error()
            })?;
        let sim = &state.match_state.sim_runtime.as_ref()?.simulation;
        if sim.session().scenario_paused()
            || sim.pending_network_wait()
            || sim.pending_network_interruption().is_some()
            || sim.pending_network_failure().is_some()
        {
            return None;
        }
        let frame = sim.session().binary_frame();
        if let Some(timing) = live.sequenced_timing() {
            let recovering = live
                .recovery_status(
                    crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
                    frame,
                )
                .is_some();
            if !state.platform.frame_pacer.online_catch_up_ready(
                Instant::now(),
                frame,
                live.committed_frontier(),
                timing,
                recovering,
            ) {
                return None;
            }
        }
        (frame > 0).then_some((frame, live.committed_frontier()))
    }

    /// Does a hidden window still have a running match to service? True while
    /// in-game and the configured focus freeze is not holding the world; a
    /// tactical capture never reaches the hidden service loop.
    pub(super) fn match_runs_while_hidden(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::InGame
            && (Self::house_outcome_wait_pending(state) || !state.focus_freeze_active())
    }

    /// The one owner of the window's own cursor after the startup splash, set
    /// after each frame's draw. gamemd's WWMouse constructor (`0x007B8730`)
    /// hides it with `ShowCursor(FALSE)` (`0x007B88BA`) and the game draws
    /// mouse.shp itself, so it shows only where VERA draws no cursor but the
    /// player points: over a status screen; over the battlefield where
    /// `use_software_cursor` turns mouse.shp off (the debug pause, whose
    /// developer overlay takes the pointer, or a missing mouse.sha) while
    /// WWMouse's depth is visible; wherever the pointer left the client area,
    /// because `DefWindowProc` answers a non-client `WM_SETCURSOR` with the
    /// system arrow and mouse.shp has no title-bar art; and elsewhere only
    /// without mouse.sha, which the shell pages draw whenever it loaded.
    fn show_os_cursor_only_without_game_cursor(state: &mut AppState, battlefield_drawn: bool) {
        let status_screen_drawn = std::mem::take(&mut state.frontend.status_screen_drawn);
        let visible = !state.platform.pointer_in_client
            || if state.platform.cursor_confined {
                // Confined: the game owns the pointer through its
                // software cursor, so keep the OS cursor hidden everywhere the game
                // draws one of its own — that leaves a single (software) cursor and
                // avoids the OS arrow + game arrow flicker at the clip edge. Where
                // the game draws *no* software cursor (the status screen, and any
                // debug pause, since the sim is paused and `use_software_cursor`
                // is false there), surface the OS cursor so a pointer stays
                // visible. Do NOT gate on `battlefield_drawn`: the sim stops
                // redrawing while paused, so the battlefield is not "drawn" yet the
                // debug pause still needs a visible pointer.
                status_screen_drawn || !state.use_software_cursor()
            } else {
                status_screen_drawn
                    || if battlefield_drawn {
                        !state.use_software_cursor()
                            && state.match_state.input.cursor_visibility.is_visible()
                    } else {
                        state
                            .match_state
                            .match_presentation
                            .software_cursor
                            .is_none()
                    }
            };
        // Asking winit is not free on Windows: `set_cursor_visible` runs
        // `refresh_os_cursor`, which clears any OS cursor clip winit does not own
        // itself, so repeating the same answer every frame would tear
        // `cursor_confined` down once per frame and let the pointer slip out of
        // the window between two re-asserts. Hand over only a transition, then put
        // the clip straight back.
        if state.platform.os_cursor_visible == Some(visible) {
            return;
        }
        state.platform.os_cursor_visible = Some(visible);
        state.platform.window.set_cursor_visible(visible);
        crate::app::input::cursor::reassert_cursor_confinement(state);
    }

    /// Dispatch rendering based on current GameScreen state.
    pub(super) fn render_frame(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
        mut shell_capture: Option<&mut crate::app::diagnostics::shell_capture::ShellCaptureSession>,
        mut tactical_capture: Option<
            &mut crate::app::diagnostics::tactical_capture::session::TacticalCaptureSession,
        >,
    ) -> Result<FrameRenderOutcome> {
        anyhow::ensure!(
            shell_capture.is_none() || tactical_capture.is_none(),
            "shell and tactical capture cannot share a render"
        );
        if state
            .match_state
            .match_diagnostics
            .actor_presentation_observer_active()
        {
            // wgpu dispatches completed submission callbacks at poll/submit.
            // Observe already-finished work before this pass's CPU rendering;
            // Poll never waits for GPU completion and the callback keeps now.
            let _ = state.renderer.gpu.device.poll(wgpu::PollType::Poll);
        }
        state.platform.frame_pacer.clear_render_continuation();
        #[cfg(feature = "telemetry-dashboard")]
        crate::app::telemetry_capture::synchronize_observer(state);
        if let Some(session) = tactical_capture.as_deref_mut() {
            session.drive_before_render(state)?;
        }
        state
            .match_state
            .match_diagnostics
            .begin_presentation_observation();
        crate::app::presentation::interpolation::prepare(state, tactical_capture.is_some());
        let tooltip_ms = crate::app::input::tooltips::update(state);
        // `Init_Game` plays the startup logo (`0x0052C5E2`) before it presents
        // the splash (`0x0052C5F3`): the splash waits while a movie owns the
        // screen through the ordinary FullscreenMovie page below.
        let startup_movie_playing = state.frontend.fullscreen_movie.is_some();
        if !startup_movie_playing
            && state
                .frontend
                .startup_splash
                .as_ref()
                .is_some_and(|splash| splash.is_active(Instant::now()))
        {
            state.diag.frame_timer.sample(Instant::now());
            let splash = state
                .frontend
                .startup_splash
                .as_ref()
                .expect("active startup splash exists");
            let presented = startup_splash::render_and_present(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                &state.renderer.shell_surface_presenter,
                &state.renderer.depth_view,
                &state.renderer.fps_overlay,
                Some(splash),
            )?;
            if presented {
                state
                    .platform
                    .frame_pacer
                    .record_presented(Instant::now(), false);
                state
                    .frontend
                    .startup_splash
                    .as_mut()
                    .expect("active startup splash exists")
                    .mark_presented(Instant::now());
            } else {
                Self::follow_window_size(state);
            }
            return Ok(if presented {
                FrameRenderOutcome::Presented
            } else {
                FrameRenderOutcome::Unavailable
            });
        }
        if !startup_movie_playing {
            state.frontend.startup_splash = None;
            // `0x0052CB50` (from `0x0052CA65`) plays the first-run intro after
            // the splash and before the shell's first paint.
            if std::mem::take(&mut state.frontend.startup_intro_pending) {
                Self::start_fullscreen_movie(
                    state,
                    crate::app::frontend::fullscreen_movie::STARTUP_INTRO_MOVIE,
                    crate::app::frontend::fullscreen_movie::MovieReturn::StartupIntro,
                );
            }
        }

        // Outcome gates, exit cascades, the audio pump and the simulation
        // advance, shared with the hidden-window service loop.
        let shell_capture_owns_simulation = shell_capture
            .as_deref()
            .is_some_and(|capture| capture.owns_simulation_steps(state));
        let ordinary_online = tactical_capture.is_none()
            && !shell_capture_owns_simulation
            && Self::next_online_render_frame(state).is_some();
        let runtime_pass = Self::service_match_runtime(
            state,
            event_loop,
            tactical_capture.is_none() && !shell_capture_owns_simulation,
        );
        if runtime_pass.quit_requested {
            return Ok(FrameRenderOutcome::Unavailable);
        }
        if Self::house_outcome_wait_pending(state)
            || (state.frontend.screen == GameScreen::InGame
                && state.match_state.scenario_exit.is_some()
                && Self::local_outcome_exit_ready(state))
        {
            // House4F860B/4F8732 and outcome exits685670/685DC0 service
            // audio/network without drawing Tactical again. Keep the surface
            // presented before Won was cleared or later House flags changed.
            return Ok(FrameRenderOutcome::Unavailable);
        }
        // Native polls timed pause before Logic, then Manage55DDA5 afterwards.
        // Retain that admission: Action110 can enter a new hold during Logic.
        // Exact-step diagnostics already advanced their admitted frame above.
        let message_ms = if state.focus_freeze_active() {
            None
        } else {
            crate::app::input::messages::update(
                state,
                runtime_pass.main_tick_admitted || tactical_capture.is_some(),
            )
        };
        let next_canonical = ordinary_online
            .then(|| Self::next_online_render_frame(state))
            .flatten();
        if state.platform.frame_pacer.defer_online_render(
            Instant::now(),
            runtime_pass.completed_frames,
            runtime_pass.last_frame_completed,
            next_canonical,
        ) {
            return Ok(FrameRenderOutcome::Deferred);
        }
        state.diag.frame_timer.sample(Instant::now());

        // Native queues/maintains [INTRO] before arming the 0xE2 first-paint
        // owner. An arm stays silent until the matching surface is acquired.
        for step in MAIN_MENU_SHELL_PRELUDE {
            match step {
                ShellFramePreludeStep::CommitTeardown => Self::drive_shell_exit(state),
                ShellFramePreludeStep::MaintainIntro => Self::maintain_main_menu_intro(state),
                ShellFramePreludeStep::ObserveEntry => {
                    crate::app::frontend::shell_transition::prepare_main_menu_first_paint_before_acquire(state)
                }
            }
        }
        match crate::app::frontend::shell_transition::poll_main_menu_first_paint_before_acquire(
            state,
            Instant::now(),
        )? {
            crate::app::frontend::shell_transition::MainMenuFirstPaintPoll::WaitUntil(_) => {
                return Ok(FrameRenderOutcome::Unavailable);
            }
            crate::app::frontend::shell_transition::MainMenuFirstPaintPoll::Completed => {
                if let Some(session) = shell_capture.as_deref_mut()
                    && session.completion_handoff()
                        == crate::app::diagnostics::shell_capture::ShellCompletionHandoff::
                            FinalizeExitReturnBeforeAcquire
                {
                    session.complete_entry_sequence_after_wave(state)?;
                    event_loop.exit();
                    return Ok(FrameRenderOutcome::Unavailable);
                }
            }
            crate::app::frontend::shell_transition::MainMenuFirstPaintPoll::Acquire => {}
        }

        let Some(output) = state
            .renderer
            .gpu
            .acquire_frame()
            .map_err(|e| anyhow::anyhow!("Surface texture: {}", e))?
        else {
            Self::follow_window_size(state);
            return Ok(FrameRenderOutcome::Unavailable);
        };
        let view: wgpu::TextureView = output.texture.create_view(&Default::default());
        let mut encoder: wgpu::CommandEncoder =
            state
                .renderer
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Frame"),
                });
        let mut pending_main_menu_entry_token = None;
        let mut pending_main_menu_title_receipt = None;
        use crate::app::diagnostics::shell_capture::PresentedShell;
        let mut presented_shell = PresentedShell::Other;

        crate::app::frontend::shell_transition::activate_shell_first_paint_after_acquire(state);
        crate::app::frontend::shell_transition::poll_main_menu_title_reveal(state);
        let mut shell_capture_current_frame = match shell_capture.as_deref_mut() {
            Some(session) => session.should_capture_current_frame(state)?,
            None => false,
        };
        let mut game_render_output: Option<crate::app::presentation::render::GameRenderOutput> =
            None;

        if state.match_state.match_presentation.in_game_menu.is_open()
            || state.match_state.network_wait.dialog_open()
            || crate::app::match_runtime::network_failure::pending(state)
        {
            Self::ensure_skirmish_shell_chrome(state);
        }
        Self::update_saved_game_browser(state, false);
        // Input routes to these same pages (`input::precedence`).
        let (page, screen_page) = crate::app::input::precedence::presented(state);
        match &state.frontend.screen {
            _ if page == Some(ModalPage::ShellError) => {
                Self::render_shell_error(state, &mut encoder, &view)?;
            }
            _ if page == Some(ModalPage::NetworkFailure) => {
                crate::app::frontend::skirmish_shell_render::render_network_failure(
                    state, &mut encoder, &output.texture,
                )?;
            }
            _ if page == Some(ModalPage::NetworkWait)
                && crate::app::frontend::skirmish_shell_render::connection_shell_drawable(state) =>
            {
                // 0xEA is pushed over whatever the game shows, in-game A3 included.
                crate::app::frontend::skirmish_shell_render::render_connection_shell(state, &mut encoder, &output.texture)?;
            }
            _ if page == Some(ModalPage::Keyboard) => {
                // The launcher's A3 is a family page, so its entry wave has to be
                // pumped here: it is the only place that advances and completes a
                // non-main-menu wave, and an uncompleted wave leaves the page
                // painting its slide-suppressed frame forever (no Back button,
                // heading or status line). The in-game A3 arms no wave, so it
                // falls through to the direct render.
                let slid = matches!(
                    crate::app::frontend::shell_transition::render_shell_first_paint_slide(
                        state,
                        &mut encoder,
                        &output.texture,
                    )?,
                    crate::app::frontend::shell_transition::ShellFirstPaintRenderResult::Rendered {
                        ..
                    }
                );
                if !slid {
                    crate::app::frontend::skirmish_shell_render::render_keyboard_shell(state, &mut encoder, &output.texture)?;
                }
            }
            _ if page == Some(ModalPage::NetworkOptions) => {
                presented_shell = PresentedShell::NetworkOptions;
                let slid = matches!(
                    crate::app::frontend::shell_transition::render_shell_first_paint_slide(
                        state, &mut encoder, &output.texture,
                    )?,
                    crate::app::frontend::shell_transition::ShellFirstPaintRenderResult::Rendered { .. }
                );
                if !slid {
                    crate::app::frontend::skirmish_shell_render::render_network_options(state, &mut encoder, &output.texture)?;
                }
            }
            _ if screen_page == ScreenPage::FullscreenMovie => {
                Self::advance_fullscreen_movie(state)?;
                if state.frontend.fullscreen_movie.is_some() {
                    crate::app::frontend::movies_credits_render::render_fullscreen_movie(
                        state,
                        &mut encoder,
                        &output.texture,
                    )?;
                    presented_shell = PresentedShell::FullscreenMovie;
                } else {
                    // The movie ended this frame: recreate the caller's
                    // dialog on the next frame's normal dispatch.
                    crate::app::frontend::movies_credits_render::render_fullscreen_black(
                        state,
                        &mut encoder,
                        &output.texture,
                    );
                }
            }
            _ if screen_page == ScreenPage::CreditsRoll => {
                Self::advance_credits_roll(state);
                if state.frontend.credits_roll.is_some() {
                    crate::app::frontend::movies_credits_render::render_credits_roll(
                        state,
                        &mut encoder,
                        &output.texture,
                    )?;
                    presented_shell = PresentedShell::CreditsRoll;
                } else {
                    crate::app::frontend::movies_credits_render::render_fullscreen_black(
                        state,
                        &mut encoder,
                        &output.texture,
                    );
                }
            }
            GameScreen::MainMenu => {
                if let crate::app::frontend::shell_transition::ShellFirstPaintRenderResult::Rendered {
                    main_menu_entry_token,
                } = crate::app::frontend::shell_transition::render_shell_first_paint_slide(
                    state,
                    &mut encoder,
                    &output.texture,
                )? {
                    pending_main_menu_entry_token = main_menu_entry_token;
                } else {
                    match screen_page {
                        ScreenPage::MovieList => {
                            if crate::app::frontend::movies_credits_render::render_movie_list(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                presented_shell = PresentedShell::MovieList;
                            } else {
                                Self::render_shell_error(state, &mut encoder, &view)?;
                            }
                        }
                        ScreenPage::Campaign => {
                            if crate::app::frontend::campaign_shell_render::render_campaign_page(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                presented_shell = PresentedShell::Campaign;
                            } else {
                                Self::render_shell_error(state, &mut encoder, &view)?;
                            }
                        }
                        ScreenPage::LanLobby => {
                            crate::app::frontend::skirmish_shell_render::render_lan_lobby(state, &mut encoder, &output.texture)?;
                            presented_shell = PresentedShell::LanLobby;
                        }
                        ScreenPage::NetworkSetup => {
                            if state.frontend.skirmish_shell_state.choose_map_modal.is_some() {
                                crate::app::frontend::skirmish_shell_render::render_skirmish_shell(state, &mut encoder, &output.texture)?;
                            } else {
                                crate::app::frontend::skirmish_shell_render::render_multiplayer_setup(state, &mut encoder, &output.texture)?;
                            }
                            presented_shell = PresentedShell::NetworkSetup;
                        }
                        ScreenPage::Wol => {
                            if crate::app::frontend::wol_welcome_render::render_wol_page(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                presented_shell = crate::app::frontend::wol_welcome_render::presented_wol_page(state);
                            } else {
                                Self::render_shell_error(state, &mut encoder, &view)?;
                            }
                        }
                        ScreenPage::LoadSavedGame => {
                            if crate::app::frontend::load_saved_game_render::render_load_saved_game_page(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                presented_shell = PresentedShell::LoadSavedGame;
                            } else {
                                Self::render_shell_error(state, &mut encoder, &view)?;
                            }
                        }
                        ScreenPage::LauncherOptions => {
                            crate::app::frontend::skirmish_shell_render::render_launcher_options(
                                state, &mut encoder, &output.texture,
                            )?;
                            presented_shell = PresentedShell::Options;
                        }
                        ScreenPage::Skirmish => {
                            crate::app::frontend::skirmish_shell_render::render_skirmish_shell(
                                state,
                                &mut encoder,
                                &output.texture,
                            )?;
                            if state.frontend.skirmish_shell_chrome.is_some() {
                                presented_shell = PresentedShell::Skirmish;
                            }
                        }
                        ScreenPage::SinglePlayer | ScreenPage::MoviesAndCredits => {
                            match crate::app::frontend::menu_page_render::render_active_menu_page(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                crate::app::frontend::menu_page_render::MenuPageRenderResult::Rendered => {
                                    presented_shell = if screen_page == ScreenPage::SinglePlayer {
                                        PresentedShell::SinglePlayer
                                    } else {
                                        PresentedShell::MoviesAndCredits
                                    };
                                }
                                crate::app::frontend::menu_page_render::MenuPageRenderResult::Fallback => {
                                    Self::render_shell_error(state, &mut encoder, &view)?;
                                }
                            }
                        }
                        _ => {
                            match crate::app::frontend::main_menu_shell_render::render_main_menu_shell(
                                state,
                                &mut encoder,
                                &output.texture,
                            )? {
                                crate::app::frontend::main_menu_shell_render::MainMenuShellRenderResult::Rendered {
                                    title_receipt,
                                } => {
                                    pending_main_menu_title_receipt = title_receipt;
                                    presented_shell = PresentedShell::MainMenu;
                                }
                                crate::app::frontend::main_menu_shell_render::MainMenuShellRenderResult::Fallback => {
                                    Self::render_shell_error(state, &mut encoder, &view)?;
                                }
                            }
                        }
                    }
                }
            }
            GameScreen::Loading => {
                match crate::app::loading::pump::render_loading_screen(
                    state,
                    &mut encoder,
                    &output.texture,
                ) {
                    crate::app::loading::pump::LoadingRenderResult::NativeRendered => {}
                    crate::app::loading::pump::LoadingRenderResult::GenericFallback => {
                        let map_name_display = crate::app::loading::pump::loading_map_name(state)
                            .unwrap_or("auto")
                            .to_string();
                        crate::app::frontend::status_screen::render(
                            state, &mut encoder, &view,
                            "Loading...", &format!("Map: {map_name_display}"),
                            crate::app::frontend::status_screen::Buttons::NONE,
                        );
                    }
                    crate::app::loading::pump::LoadingRenderResult::Failed => {
                        transitions::clear_screen(&mut encoder, &view);
                    }
                }
            }
            _ if matches!(screen_page, ScreenPage::InGameShell(_)) => {
                // 621FCE -> 72F540 clears and paints a complete active-game shell.
                // No battlefield commands share this encoder/camera upload.
                use crate::ui::pause_menu::InGameMenuState;
                match screen_page {
                    ScreenPage::InGameShell(InGameMenuState::Menu) => {
                        let buttons = crate::app::input::pause_menu::button_states(state);
                        crate::app::frontend::skirmish_shell_render::render_pause_menu_shell(
                            state, &mut encoder, &output.texture, buttons,
                        )?;
                    }
                    ScreenPage::InGameShell(InGameMenuState::AbortConfirm) => {
                        crate::app::frontend::skirmish_shell_render::render_abort_shell(state, &mut encoder, &output.texture)?;
                    }
                    ScreenPage::InGameShell(InGameMenuState::Sound) => {
                        crate::app::frontend::skirmish_shell_render::render_sound_shell(state, &mut encoder, &output.texture)?;
                    }
                    ScreenPage::InGameShell(InGameMenuState::Diplomacy) => {
                        crate::app::frontend::skirmish_shell_render::render_diplomacy_shell(state, &mut encoder, &output.texture)?;
                    }
                    ScreenPage::InGameShell(InGameMenuState::MissionBriefing) => {
                        crate::app::frontend::skirmish_shell_render::render_restate_shell(state, &mut encoder, &output.texture)?;
                    }
                    ScreenPage::InGameShell(InGameMenuState::SavedGame(_)) => {
                        crate::app::frontend::skirmish_shell_render::render_saved_game_shell(state, &mut encoder, &output.texture)?;
                    }
                    _ => {
                        crate::app::frontend::skirmish_shell_render::render_in_game_options_shell(
                            state, &mut encoder, &output.texture,
                        )?;
                    }
                }
            }
            _ if screen_page == ScreenPage::InGameCard => {
                let abort = state.match_state.match_presentation.in_game_menu
                    == crate::ui::pause_menu::InGameMenuState::AbortConfirm;
                crate::app::frontend::status_screen::render(
                    state, &mut encoder, &view,
                    if abort { "Leave this mission?" } else { "Menu resources unavailable" },
                    "Required retail dialog artwork could not be loaded. Check your game installation. Full details are in logs/ra2.log.",
                    if abort {
                        crate::app::frontend::status_screen::Buttons::ABORT_CONFIRM
                    } else {
                        crate::app::frontend::status_screen::Buttons::MISSING_MENU
                    },
                );
            }
            GameScreen::InGame => {
                //53B460 copies the already-composed surface, before this
                //frame's world replaces it. The effect owns the retained copy.
                let chrono_tactical = crate::app::input::camera::tactical_viewport_px(state);
                state.match_state.match_presentation.chrono_screen.before_frame(
                    &state.renderer.gpu,
                    &mut encoder,
                    state.renderer.game_frame_texture(),
                    [chrono_tactical.0, chrono_tactical.1, chrono_tactical.2, chrono_tactical.3],
                );
                let times = render::GameRenderTimes {
                    radar_ms: state.radar_presentation_ms(),
                    tooltip_ms,
                    message_ms,
                };
                let game_output = if state.renderer.upscale_pass.is_some() {
                    // Render game to intermediate texture, then upscale to swapchain.
                    let up = state.renderer.upscale_pass.as_ref().unwrap();
                    let game_depth = up.depth_view().clone();
                    let saved_depth = std::mem::replace(&mut state.renderer.depth_view, game_depth);
                    let result = render::render_game(state, &mut encoder, times);
                    state.renderer.depth_view = saved_depth;
                    let render_output = result?;
                    state.renderer.copy_game_frame_to(
                        &mut encoder,
                        state.renderer.upscale_pass.as_ref().unwrap().color_texture(),
                    );
                    state
                        .renderer.upscale_pass
                        .as_ref()
                        .unwrap()
                        .draw(&mut encoder, &view);
                    render_output
                } else {
                    let render_output = render::render_game(state, &mut encoder, times)?;
                    state
                        .renderer
                        .copy_game_frame_to(&mut encoder, &output.texture);
                    render_output
                };
                #[cfg(feature = "telemetry-dashboard")]
                if let Some(phases) = game_output.phases {
                    state.diag.render_timing.record(phases);
                }
                game_render_output = Some(game_output);
            }
            GameScreen::MissionResult { title, detail } => {
                // The fallback card's strings are copied out before the score
                // render takes `state` mutably.
                let (title, detail) = (title.clone(), detail.clone());
                // A finished match presents the native score screen. Result
                // screens with no native analogue (a load failure, a
                // trigger-driven campaign end) carry no model and keep the
                // non-art card.
                let score_rendered = match screen_page {
                    ScreenPage::CoopScore => {
                        crate::app::frontend::skirmish_shell_render::render_coop_score(
                            state,
                            &mut encoder,
                            &output.texture,
                        )?
                    }
                    ScreenPage::CampaignScore => {
                        matches!(
                            crate::app::frontend::shell_transition::render_shell_first_paint_slide(
                                state,
                                &mut encoder,
                                &output.texture,
                            )?,
                            crate::app::frontend::shell_transition::ShellFirstPaintRenderResult::Rendered { .. }
                        ) || crate::app::frontend::campaign_score_shell_render::render_campaign_score_page(
                            state,
                            &mut encoder,
                            &output.texture,
                        )?
                    }
                    ScreenPage::Score => {
                        matches!(
                            crate::app::frontend::shell_transition::render_shell_first_paint_slide(
                                state,
                                &mut encoder,
                                &output.texture,
                            )?,
                            crate::app::frontend::shell_transition::ShellFirstPaintRenderResult::Rendered { .. }
                        ) || crate::app::frontend::score_shell_render::render_score_page(
                            state,
                            &mut encoder,
                            &output.texture,
                        )?
                    }
                    _ => false,
                };
                if score_rendered {
                    presented_shell = PresentedShell::Score;
                } else {
                    crate::app::frontend::status_screen::render(
                        state, &mut encoder, &view, &title, &detail,
                        crate::app::frontend::status_screen::Buttons::BACK_TO_MENU,
                    );
                }
            }
        }
        state.renderer.fps_overlay.encode(
            &state.renderer.gpu,
            &state.renderer.batch_renderer,
            &mut encoder,
            &output.texture,
        );
        if state.frontend.screen == GameScreen::InGame {
            let status = state
                .match_state
                .match_diagnostics
                .playback_status()
                .or_else(|| {
                    let live = state.match_state.live_network.as_ref()?;
                    let frame = state
                        .match_state
                        .sim_runtime
                        .as_ref()?
                        .simulation
                        .session()
                        .binary_frame();
                    match live.recovery_status(
                        crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
                        frame,
                    )? {
                        crate::net::live_commands::NetworkRecoveryStatus::Reconnecting => Some(
                            "Reconnecting - waiting for connection; match remains active"
                                .to_owned(),
                        ),
                        crate::net::live_commands::NetworkRecoveryStatus::CatchingUp { frames } => {
                            Some(format!("Catching up - {frames} frames behind"))
                        }
                    }
                })
                .or_else(|| {
                    crate::app::input::pause_menu::multiplayer_control(state)?
                        .status
                        .map(str::to_owned)
                });
            if let Some(status) = status {
                state.renderer.fps_overlay.encode_status(
                    &state.renderer.gpu,
                    &state.renderer.batch_renderer,
                    &mut encoder,
                    &output.texture,
                    &status,
                );
            }
        }
        if state.frontend.screen == GameScreen::InGame
            && let Some(offset_px) = runtime_pass.screen_shake_offset
        {
            // Native RenderFrame_main finishes composing overlays and its
            // software cursor before Screen__UpdatePrimarySurface moves the
            // central primary-client rectangle.
            apply_screen_shake(state, &mut encoder, &output.texture, offset_px);
        }
        Self::show_os_cursor_only_without_game_cursor(state, game_render_output.is_some());

        let entry_sequence_identity = match shell_capture.as_deref_mut() {
            Some(session) if session.is_entry_sequence() => {
                let token = pending_main_menu_entry_token
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("entry-sequence frame produced no token"))?;
                session.observe_entry_sequence_after_render(state, token)?
            }
            _ => None,
        };
        let pending_entry_sequence = if entry_sequence_identity.is_some() {
            Some(crate::render::frame_readback::PendingBgra8Readback::encode(
                &state.renderer.gpu.device,
                &mut encoder,
                &output.texture,
                state.renderer.gpu.config.format,
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
            )?)
        } else {
            None
        };
        let tactical_capture_current_frame =
            match (tactical_capture.as_deref_mut(), game_render_output.as_ref()) {
                (Some(session), Some(render_output)) => {
                    session.observe_after_render(state, render_output)?
                }
                (Some(_), None) | (None, _) => false,
            };
        if let Some(session) = shell_capture.as_deref_mut() {
            shell_capture_current_frame |=
                session.should_capture_after_render(state, presented_shell)?;
        }
        let replay_capture_current_frame = game_render_output.is_some()
            && state.match_state.match_diagnostics.playback_capture_ready();
        let capture_current_frame = shell_capture_current_frame
            || tactical_capture_current_frame
            || replay_capture_current_frame;
        let pending_capture = if capture_current_frame {
            Some(crate::render::frame_readback::PendingBgra8Readback::encode(
                &state.renderer.gpu.device,
                &mut encoder,
                &output.texture,
                state.renderer.gpu.config.format,
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
            )?)
        } else {
            None
        };
        let retail_screenshot_current_frame =
            std::mem::take(&mut state.match_state.input.retail_screenshot_requested);
        let screenshot_central_rect = shake_central_rect(state, &output.texture);
        let pending_retail_screenshot = state
            .renderer
            .retail_screenshot_frame_cache
            .capture_previous_if_requested(
                retail_screenshot_current_frame,
                &state.renderer.gpu.device,
                &mut encoder,
                state.renderer.gpu.config.format,
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
                state.renderer.upscale_pass.as_ref(),
                &mut state.renderer.screen_presentation,
                screenshot_central_rect,
            )?;
        let capture_timeout = if capture_current_frame {
            Some(if replay_capture_current_frame {
                crate::render::screenshot::READBACK_TIMEOUT
            } else if shell_capture_current_frame {
                shell_capture
                    .as_deref()
                    .expect("shell capture session exists when readback is requested")
                    .readback_timeout()?
            } else {
                tactical_capture
                    .as_deref()
                    .expect("tactical capture session exists when readback is requested")
                    .readback_timeout()?
            })
        } else {
            None
        };
        let submission = state
            .renderer
            .gpu
            .queue
            .submit(std::iter::once(encoder.finish()));
        output.present(&state.renderer.gpu.queue);
        state
            .platform
            .frame_pacer
            .record_presented(Instant::now(), state.frontend.screen == GameScreen::InGame);
        if let Some(completed) = state
            .match_state
            .match_diagnostics
            .commit_presentation_observation()
        {
            // Diagnostic callback is an upper bound on this submitted frame's
            // GPU completion. It never waits or enters simulation state.
            state.renderer.gpu.queue.on_submitted_work_done(move || {
                *completed.lock().unwrap_or_else(|error| error.into_inner()) =
                    Some(crate::app::match_diagnostics::ObservationTime::now());
            });
        }
        // wgpu 27 requires released surface references before reconfiguration;
        // the diagnostic action below may resize the window.
        // https://docs.rs/wgpu/27.0.1/wgpu/struct.Surface.html#method.configure
        drop(view);
        state
            .renderer
            .retail_screenshot_frame_cache
            .commit_presented(if state.frontend.screen == GameScreen::InGame {
                runtime_pass.screen_shake_offset.unwrap_or([0, 0])
            } else {
                [0, 0]
            });
        state
            .match_state
            .match_presentation
            .screen_shake
            .record_presented(if state.frontend.screen == GameScreen::InGame {
                runtime_pass.screen_shake_offset.unwrap_or([0, 0])
            } else {
                [0, 0]
            });
        // A family renderer that drew a timer-driven 0x71C frame this pass
        // advances it now that the frame reached the screen.
        state.frontend.shell_monitor.commit_presented();
        state.frontend.shell_page_title.commit_presented();
        state.frontend.shell_status_line.commit_presented();
        if let Some(page) = state.frontend.score_page.as_mut() {
            page.commit_presented();
        }
        if let Some(page) = state.frontend.campaign_score_page.as_mut() {
            page.commit_presented();
        }
        let skirmish = &mut state.frontend.skirmish_shell_state;
        skirmish.statics.commit_presented();
        if let Some(modal) = skirmish.choose_map_modal.as_mut() {
            modal.statics.commit_presented();
        }
        if let Some(setup) = skirmish.random_map_setup_modal.as_mut() {
            setup.statics.commit_presented();
        }
        if let Some(token) = pending_main_menu_entry_token.take() {
            crate::app::frontend::shell_transition::record_main_menu_entry_presented(state, token)?;
        }
        if let (Some(identity), Some(readback)) = (entry_sequence_identity, pending_entry_sequence)
        {
            shell_capture
                .as_deref_mut()
                .expect("entry-sequence session exists for retained readback")
                .record_entry_sequence_submission(identity, readback, submission.clone())?;
        }
        if let Some(receipt) = pending_main_menu_title_receipt.take() {
            anyhow::ensure!(
                state
                    .frontend
                    .main_menu_shell_state
                    .title_reveal
                    .record_presented(receipt),
                "main-menu title receipt was stale at present commit"
            );
        }
        if let Some(dialog) = state.frontend.keyboard_dialog.as_mut()
            && let Some(receipt) = dialog.title_receipt.take()
        {
            anyhow::ensure!(
                dialog.title.record_presented(receipt),
                "keyboard title receipt was stale at present commit"
            );
        }
        if let Some(session) = shell_capture.as_deref_mut() {
            session.after_present(state, presented_shell)?;
        }
        if let Some(pending_capture) = pending_capture {
            let pixels = pending_capture.finish(
                &state.renderer.gpu.device,
                submission.clone(),
                capture_timeout.expect("capture timeout exists with pending readback"),
            )?;
            let surface_format = state.renderer.gpu.config.format;
            let capture_terminal = if replay_capture_current_frame {
                let runtime = state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .expect("rendered replay runtime");
                state
                    .match_state
                    .match_diagnostics
                    .complete_playback_capture(
                        runtime,
                        state.renderer.gpu.config.width,
                        state.renderer.gpu.config.height,
                        &pixels,
                    )?;
                true
            } else if shell_capture_current_frame {
                let session = shell_capture
                    .as_deref_mut()
                    .expect("shell capture session exists when readback completes");
                session.complete(state, surface_format, &pixels)?;
                session.is_terminal()
            } else {
                tactical_capture
                    .expect("tactical capture session exists when readback completes")
                    .complete_after_readback(state, surface_format, &pixels)?;
                true
            };
            if capture_terminal {
                event_loop.exit();
            }
        }
        if let Some(pending_screenshot) = pending_retail_screenshot {
            match pending_screenshot.finish(
                &state.renderer.gpu.device,
                submission,
                crate::render::screenshot::READBACK_TIMEOUT,
            ) {
                Ok(pixels) => match crate::render::screenshot::write_retail_screenshot(
                    state.renderer.gpu.config.width,
                    state.renderer.gpu.config.height,
                    state.renderer.gpu.config.format,
                    &pixels,
                ) {
                    Ok(path) => log::info!("Saved screenshot {}", path.display()),
                    Err(error) => log::error!("Screenshot write failed: {error:#}"),
                },
                Err(error) => log::error!("Screenshot readback failed: {error}"),
            }
        }

        crate::app::loading::pump::after_loading_frame_presented(
            state,
            shell_capture
                .as_deref()
                .and_then(|capture| capture.loading_observer()),
        )?;
        #[cfg(feature = "telemetry-dashboard")]
        crate::app::telemetry_capture::publish_if_due(state);

        Ok(FrameRenderOutcome::Presented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teardown_commits_before_intro_and_entry_observation() {
        assert_eq!(
            MAIN_MENU_SHELL_PRELUDE,
            [
                ShellFramePreludeStep::CommitTeardown,
                ShellFramePreludeStep::MaintainIntro,
                ShellFramePreludeStep::ObserveEntry,
            ]
        );
    }
}
