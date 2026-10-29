//! Top-level frame orchestration, screen dispatch, presentation, and readback.
//!
//! Simulation admission, draw composition, submit/present, transition commits,
//! captures, and loading-after-present remain in their original source order.

use super::loading::transitions;
use super::{
    ActiveEventLoop, App, AppState, GameScreen, Instant, Result, frontend::startup_splash, render,
    sim_tick,
};

#[cfg(target_os = "horizon")]
#[derive(Default)]
struct HorizonFrameTiming {
    frames: u32,
    sums: [std::time::Duration; 11],
    intervals: Vec<std::time::Duration>,
    service_calls: u32,
    main_admitted: u32,
    paced_returns: u32,
    #[cfg(feature = "horizon-frame-profile")]
    replay: (usize, usize, std::time::Duration, std::time::Duration),
    previous_complete: Option<Instant>,
    next_log: Option<Instant>,
}

#[cfg(target_os = "horizon")]
thread_local! {
    static HORIZON_FRAME_TIMING: std::cell::RefCell<HorizonFrameTiming> =
        std::cell::RefCell::new(HorizonFrameTiming::default());
}

/// Record a completed in-game event-loop iteration. The first row is work
/// through the main simulation admission; catch-up frames are counted
/// separately because they run after the present. `total` is the interval
/// between completed iterations, so the row sum also exposes event-loop or
/// driver time that is not owned by a named stage.
#[cfg(target_os = "horizon")]
fn record_horizon_frame(
    phases: [std::time::Duration; 10],
    active: bool,
    resolution: [u32; 4],
    render_budget: std::time::Duration,
) {
    HORIZON_FRAME_TIMING.with_borrow_mut(|timing| {
        if !active {
            *timing = HorizonFrameTiming::default();
            return;
        }
        let now = Instant::now();
        let total = timing
            .previous_complete
            .map_or(std::time::Duration::ZERO, |previous| now - previous);
        if timing.previous_complete.is_some() {
            timing.intervals.push(total);
        }
        timing.previous_complete = Some(now);
        timing.frames = timing.frames.saturating_add(1);
        for (sum, phase) in timing
            .sums
            .iter_mut()
            .zip(phases.into_iter().chain([total]))
        {
            *sum += phase;
        }
        let next_log = *timing
            .next_log
            .get_or_insert_with(|| now + std::time::Duration::from_secs(10));
        if now < next_log {
            return;
        }
        let scale = 1000.0 / timing.frames.max(1) as f32;
        let [
            service,
            pace,
            prepare,
            acquire,
            encode,
            finish,
            submit,
            present,
            tail,
            catch_up,
            total,
        ] = timing.sums.map(|sum| sum.as_secs_f32() * scale);
        log::info!(
            target: "switch::perf",
            "Frame CPU average over {} frames: service={service:.2}ms pace={pace:.2}ms \
             prepare={prepare:.2}ms acquire={acquire:.2}ms encode={encode:.2}ms \
             finish={finish:.2}ms submit={submit:.2}ms present={present:.2}ms tail={tail:.2}ms \
             catch_up={catch_up:.2}ms total={total:.2}ms",
            timing.frames,
        );
        timing.intervals.sort_unstable();
        let intervals = &timing.intervals;
        let percentile = |fraction: f64| {
            let index = (intervals.len() as f64 * fraction).ceil() as usize;
            intervals
                .get(index.saturating_sub(1))
                .copied()
                .unwrap_or_default()
                .as_secs_f64()
                * 1000.0
        };
        let over_budget = intervals
            .iter()
            .filter(|&&span| span > render_budget)
            .count();
        log::info!(
            target: "switch::perf",
            "Frame intervals samples={} p50_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2} \
             over_budget={} budget_ms={:.2} surface={}x{} render={}x{} service_calls={} \
             main_admitted={} paced_returns={}; stages=wall_time",
            intervals.len(),
            percentile(0.5),
            percentile(0.95),
            percentile(0.99),
            intervals
                .last()
                .copied()
                .unwrap_or_default()
                .as_secs_f64()
                * 1000.0,
            over_budget,
            render_budget.as_secs_f64() * 1000.0,
            resolution[0],
            resolution[1],
            resolution[2],
            resolution[3],
            timing.service_calls,
            timing.main_admitted,
            timing.paced_returns,
        );
        #[cfg(feature = "horizon-frame-profile")]
        {
            let (passes, submits, finish, submit) = timing.replay;
            log::info!(
                target: "switch::perf",
                "Replay per frame passes={:.2} internal_submits={:.2} finish_ms={:.3} \
                 submit_ms={:.3}; included_in_encode=true",
                passes as f64 / timing.frames.max(1) as f64,
                submits as f64 / timing.frames.max(1) as f64,
                finish.as_secs_f64() * scale as f64,
                submit.as_secs_f64() * scale as f64,
            );
        }
        *timing = HorizonFrameTiming {
            previous_complete: Some(now),
            next_log: Some(now + std::time::Duration::from_secs(10)),
            ..Default::default()
        };
    });
}

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
            && (!state.platform.focus_freeze_active()
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
        crate::app::lan_service::poll(state, scenario_now_ms);
        if crate::app::match_runtime::network_failure::pending(state) {
            // ExecuteDoList64CD0C blocks inside ShellMessageBox5D3490. Its
            // message pump still reaches Network_ServiceLoop48D080/audio,
            // while Main_Tick's terminal tests and frame tail wait for Stop.
            crate::app::match_runtime::network_failure::show(state);
            crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);
            if crate::app::match_runtime::network_failure::take_acknowledged(state) {
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
                if matches!(state.frontend.screen, GameScreen::InGame)
                    && state.match_state.scenario_exit.is_none()
                    && state.match_state.scenario_outcome.is_none()
                {
                    // ProcessNetwork6475F0 clears GameActive at64826F;
                    // Main_Game55D347 returns directly to shell cleanup.
                    // This is the same route as WaitPass::Leave, without an
                    // EXIT command or BattleControlTerminated686570.
                    Self::return_to_main_menu(state);
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
                event_loop.exit();
                return RuntimeServiceResult {
                    quit_requested: true,
                    main_tick_admitted,
                    screen_shake_offset,
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
                    && !state.platform.focus_freeze_active()
                    && state.match_state.scenario_exit.is_none()
                    && state.match_state.scenario_outcome.is_none()))
        {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            // Wait_For_Players runs inside the frame's network service, ahead
            // of the frame it holds back.
            if crate::app::match_runtime::network_match::service_wait(state, now_ms)
                == crate::app::match_runtime::network_match::WaitPass::Leave
            {
                if state
                    .match_state
                    .sim_runtime
                    .as_ref()
                    .is_some_and(|rt| rt.simulation.pending_network_wait())
                {
                    sim_tick::leave_network_wait(state);
                }
                // Result 8 clears GameActive without an EXIT event, so
                // Main_Game returns past the router without 0x00686570's
                // EVA and fades (0x0064806C..0x006482A6, 0x0055D347).
                Self::return_to_main_menu(state);
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
        }
    }

    /// Does a hidden window still have a running match to service? True while
    /// in-game and the configured focus freeze is not holding the world; a
    /// tactical capture never reaches the hidden service loop.
    pub(super) fn match_runs_while_hidden(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::InGame
            && (Self::house_outcome_wait_pending(state) || !state.platform.focus_freeze_active())
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
    ) -> Result<()> {
        #[cfg(target_os = "horizon")]
        let horizon_frame_started = Instant::now();
        anyhow::ensure!(
            shell_capture.is_none() || tactical_capture.is_none(),
            "shell and tactical capture cannot share a render"
        );
        // Captures own their own simulation stepping; the rendered match must
        // not advance behind them.
        let catch_up_allowed = shell_capture.is_none() && tactical_capture.is_none();
        #[cfg(feature = "telemetry-dashboard")]
        crate::app::telemetry_capture::synchronize_observer(state);
        if let Some(session) = tactical_capture.as_deref_mut() {
            session.drive_before_render(state)?;
        }
        if state
            .frontend
            .startup_splash
            .as_ref()
            .is_some_and(|splash| splash.is_active(Instant::now()))
        {
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
                splash,
            )?;
            if presented {
                state
                    .frontend
                    .startup_splash
                    .as_mut()
                    .expect("active startup splash exists")
                    .mark_presented(Instant::now());
            } else {
                Self::follow_window_size(state);
            }
            return Ok(());
        }
        state.frontend.startup_splash = None;

        // Outcome gates, exit cascades, the audio pump and the simulation
        // advance, shared with the hidden-window service loop.
        let shell_capture_owns_simulation = shell_capture
            .as_deref()
            .is_some_and(|capture| capture.owns_simulation_steps(state));
        let runtime_pass = Self::service_match_runtime(
            state,
            event_loop,
            tactical_capture.is_none() && !shell_capture_owns_simulation,
        );
        #[cfg(target_os = "horizon")]
        let horizon_after_service = Instant::now();
        #[cfg(target_os = "horizon")]
        if state.frontend.screen == GameScreen::InGame {
            HORIZON_FRAME_TIMING.with_borrow_mut(|timing| {
                timing.service_calls += 1;
                timing.main_admitted += u32::from(runtime_pass.main_tick_admitted);
            });
        }
        if runtime_pass.quit_requested {
            return Ok(());
        }
        if Self::house_outcome_wait_pending(state)
            || (state.frontend.screen == GameScreen::InGame
                && state.match_state.scenario_exit.is_some()
                && Self::local_outcome_exit_ready(state))
        {
            // House4F860B/4F8732 and outcome exits685670/685DC0 service
            // audio/network without drawing Tactical again. Keep the surface
            // presented before Won was cleared or later House flags changed.
            return Ok(());
        }
        // Native polls timed pause before Logic, then Manage55DDA5 afterwards.
        // Retain that admission: Action110 can enter a new hold during Logic.
        // Exact-step diagnostics already advanced their admitted frame above.
        let message_ms = if state.platform.focus_freeze_active() {
            None
        } else {
            crate::app::input::messages::update(
                state,
                runtime_pass.main_tick_admitted || tactical_capture.is_some(),
            )
        };

        #[cfg(target_os = "horizon")]
        {
            let now = Instant::now();
            let interval = state.persistence.options_profile.switch_render_interval();
            if let Some(last) = state.platform.last_render_start {
                let remaining = interval.saturating_sub(now.duration_since(last));
                if !remaining.is_zero() {
                    let slice = remaining.min(std::time::Duration::from_millis(4));
                    std::thread::sleep(slice);
                    // Sleep the exact remainder on the final wake so the
                    // present lands on the selected boundary instead of arriving
                    // up to one 4 ms slice late.
                    if remaining > slice {
                        HORIZON_FRAME_TIMING.with_borrow_mut(|timing| timing.paced_returns += 1);
                        return Ok(());
                    }
                }
            }
            state.platform.last_render_start = Some(Instant::now());
        }
        #[cfg(target_os = "horizon")]
        let horizon_after_pace = Instant::now();
        // These inputs are only observable on a drawn frame. Collecting
        // interpolation history and tooltip state on every sub-deadline wake
        // pollutes the history and costs an entity scan every few milliseconds.
        crate::app::presentation::interpolation::prepare(state, tactical_capture.is_some());
        let tooltip_ms = crate::app::input::tooltips::update(state);
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
                return Ok(());
            }
            crate::app::frontend::shell_transition::MainMenuFirstPaintPoll::Completed => {
                if let Some(session) = shell_capture.as_deref_mut()
                    && session.completion_handoff()
                        == crate::app::diagnostics::shell_capture::ShellCompletionHandoff::
                            FinalizeExitReturnBeforeAcquire
                {
                    session.complete_entry_sequence_after_wave(state)?;
                    event_loop.exit();
                    return Ok(());
                }
            }
            crate::app::frontend::shell_transition::MainMenuFirstPaintPoll::Acquire => {}
        }
        #[cfg(target_os = "horizon")]
        let horizon_before_acquire = Instant::now();

        let Some(output) = state
            .renderer
            .gpu
            .acquire_frame()
            .map_err(|e| anyhow::anyhow!("Surface texture: {}", e))?
        else {
            Self::follow_window_size(state);
            return Ok(());
        };
        #[cfg(target_os = "horizon")]
        let horizon_after_acquire = Instant::now();
        let view: wgpu::TextureView = output.texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Configured surface render view"),
            format: Some(state.renderer.gpu.surface_format),
            ..Default::default()
        });
        let mut encoder: wgpu::CommandEncoder =
            state
                .renderer
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Frame"),
                });
        #[cfg(all(target_os = "horizon", feature = "horizon-frame-profile"))]
        let gpu_profile_sample = state
            .renderer
            .gpu
            .profile
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .begin(
                &state.renderer.gpu.device,
                &mut encoder,
                state.frontend.screen == GameScreen::InGame
                    && !state.match_state.match_presentation.in_game_menu.is_open(),
            );
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
        match &state.frontend.screen {
            GameScreen::InGame if crate::app::match_runtime::network_failure::pending(state) => {
                crate::app::frontend::skirmish_shell_render::render_network_failure(
                    state, &mut encoder, &output.texture,
                )?;
            }
            GameScreen::MainMenu if state.frontend.main_menu_shell_error.is_some() => {
                Self::render_shell_error(state, &mut encoder, &view)?;
            }
            _ if state.frontend.keyboard_dialog.is_some() => {
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
            _ if state.frontend.network_options.is_some() => {
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
            GameScreen::MainMenu if state.frontend.fullscreen_movie.is_some() => {
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
            GameScreen::MainMenu if state.frontend.credits_roll.is_some() => {
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
                } else if Self::movie_list_active(state) {
                    if crate::app::frontend::movies_credits_render::render_movie_list(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        presented_shell = PresentedShell::MovieList;
                    } else {
                        Self::render_shell_error(state, &mut encoder, &view)?;
                    }
                } else if Self::campaign_active(state) {
                    if crate::app::frontend::campaign_shell_render::render_campaign_page(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        presented_shell = PresentedShell::Campaign;
                    } else {
                        Self::render_shell_error(state, &mut encoder, &view)?;
                    }
                } else if state.frontend.shell_route.lan_lobby() {
                    crate::app::frontend::skirmish_shell_render::render_lan_lobby(state, &mut encoder, &output.texture)?;
                    presented_shell = PresentedShell::LanLobby;
                } else if state.frontend.shell_route.network_setup() {
                    if state.frontend.skirmish_shell_state.choose_map_modal.is_some() {
                        crate::app::frontend::skirmish_shell_render::render_skirmish_shell(state, &mut encoder, &output.texture)?;
                    } else {
                        crate::app::frontend::skirmish_shell_render::render_multiplayer_setup(state, &mut encoder, &output.texture)?;
                    }
                    presented_shell = PresentedShell::NetworkSetup;
                } else if Self::wol_active(state) {
                    if crate::app::frontend::wol_welcome_render::render_wol_page(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        presented_shell = crate::app::frontend::wol_welcome_render::presented_wol_page(state);
                    } else {
                        Self::render_shell_error(state, &mut encoder, &view)?;
                    }
                } else if Self::load_saved_game_active(state) {
                    if crate::app::frontend::load_saved_game_render::render_load_saved_game_page(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        presented_shell = PresentedShell::LoadSavedGame;
                    } else {
                        Self::render_shell_error(state, &mut encoder, &view)?;
                    }
                } else if Self::native_launcher_options_active(state) {
                    crate::app::frontend::skirmish_shell_render::render_launcher_options(
                        state, &mut encoder, &output.texture,
                    )?;
                    presented_shell = PresentedShell::Options;
                } else if Self::native_skirmish_shell_active(state) {
                    crate::app::frontend::skirmish_shell_render::render_skirmish_shell(
                        state,
                        &mut encoder,
                        &output.texture,
                    )?;
                    if state.frontend.skirmish_shell_chrome.is_some() {
                        presented_shell = PresentedShell::Skirmish;
                    }
                } else if let Some(page) = crate::app::frontend::menu_page_render::ActiveMenuPage::from_state(state) {
                    match crate::app::frontend::menu_page_render::render_active_menu_page(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        crate::app::frontend::menu_page_render::MenuPageRenderResult::Rendered => {
                            presented_shell = match page {
                                crate::app::frontend::menu_page_render::ActiveMenuPage::SinglePlayer => {
                                    PresentedShell::SinglePlayer
                                }
                                crate::app::frontend::menu_page_render::ActiveMenuPage::MoviesAndCredits => {
                                    PresentedShell::MoviesAndCredits
                                }
                            };
                        }
                        crate::app::frontend::menu_page_render::MenuPageRenderResult::Fallback => {
                            Self::render_shell_error(state, &mut encoder, &view)?;
                        }
                    }
                } else if state.frontend.main_menu_shell_error.is_none() {
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
                } else {
                    Self::render_shell_error(state, &mut encoder, &view)?;
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
            GameScreen::InGame if crate::app::frontend::skirmish_shell_render::connection_shell_drawable(state) => {
                // 0xEA is pushed over whatever the game shows.
                crate::app::frontend::skirmish_shell_render::render_connection_shell(state, &mut encoder, &output.texture)?;
            }
            GameScreen::InGame if crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(state) => {
                // 621FCE -> 72F540 clears and paints a complete active-game shell.
                // No battlefield commands share this encoder/camera upload.
                if state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::Menu {
                    let buttons = crate::app::input::pause_menu::button_states(state);
                    crate::app::frontend::skirmish_shell_render::render_pause_menu_shell(
                        state, &mut encoder, &output.texture, buttons,
                    )?;
                } else if state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::AbortConfirm {
                    crate::app::frontend::skirmish_shell_render::render_abort_shell(state, &mut encoder, &output.texture)?;
                } else if state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::Sound {
                    crate::app::frontend::skirmish_shell_render::render_sound_shell(state, &mut encoder, &output.texture)?;
                } else if state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::Diplomacy {
                    crate::app::frontend::skirmish_shell_render::render_diplomacy_shell(state, &mut encoder, &output.texture)?;
                } else if state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::MissionBriefing {
                    crate::app::frontend::skirmish_shell_render::render_restate_shell(state, &mut encoder, &output.texture)?;
                } else if matches!(state.match_state.match_presentation.in_game_menu, crate::ui::pause_menu::InGameMenuState::SavedGame(_)) {
                    crate::app::frontend::skirmish_shell_render::render_saved_game_shell(state, &mut encoder, &output.texture)?;
                } else {
                    crate::app::frontend::skirmish_shell_render::render_in_game_options_shell(
                        state, &mut encoder, &output.texture,
                    )?;
                }
            }
            GameScreen::InGame
                if state.match_state.match_presentation.in_game_menu.is_open()
                    && !state.match_state.match_presentation.in_game_menu.draws_over_battlefield() =>
            {
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
                    state.renderer.combat_light_renderer.composition_texture(),
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
                    state.renderer.combat_light_renderer.copy_to(
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
                        .renderer.combat_light_renderer
                        .copy_to(&mut encoder, &output.texture);
                    render_output
                };
                #[cfg(any(feature = "telemetry-dashboard", target_os = "horizon"))]
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
                let score_rendered = if crate::app::input::coop_score::active(state) {
                    crate::app::frontend::skirmish_shell_render::render_coop_score(
                        state,
                        &mut encoder,
                        &output.texture,
                    )?
                } else if Self::campaign_score_shell_active(state) {
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
                } else {
                    Self::score_shell_active(state)
                        && (matches!(
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
                        )?)
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
        let capture_current_frame = shell_capture_current_frame || tactical_capture_current_frame;
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
            Some(if shell_capture_current_frame {
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
        #[cfg(target_os = "horizon")]
        let horizon_before_submit = Instant::now();
        #[cfg(all(target_os = "horizon", feature = "horizon-frame-profile"))]
        if let Some(sample) = gpu_profile_sample
            && game_render_output.is_some()
        {
            state
                .renderer
                .gpu
                .profile
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .end(&mut encoder, sample);
        }
        let commands = encoder.finish();
        #[cfg(target_os = "horizon")]
        let horizon_after_finish = Instant::now();
        let submission = state.renderer.gpu.queue.submit(std::iter::once(commands));
        #[cfg(target_os = "horizon")]
        let horizon_after_submit = Instant::now();
        crate::render::present::present(output);
        #[cfg(target_os = "horizon")]
        let horizon_after_present = Instant::now();
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
            let capture_terminal = if shell_capture_current_frame {
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
        #[cfg(target_os = "horizon")]
        let horizon_before_catch_up = Instant::now();
        if catch_up_allowed {
            Self::catch_up_match_runtime(state, event_loop);
        }
        #[cfg(all(target_os = "horizon", feature = "horizon-frame-profile"))]
        if game_render_output.is_some() {
            let (passes, submits, finish, submit) =
                state.renderer.terrain_draw_renderer.submission_profile();
            HORIZON_FRAME_TIMING.with_borrow_mut(|timing| {
                timing.replay.0 += passes;
                timing.replay.1 += submits;
                timing.replay.2 += finish;
                timing.replay.3 += submit;
            });
        }
        #[cfg(target_os = "horizon")]
        record_horizon_frame(
            [
                horizon_after_service - horizon_frame_started,
                horizon_after_pace - horizon_after_service,
                horizon_before_acquire - horizon_after_pace,
                horizon_after_acquire - horizon_before_acquire,
                horizon_before_submit - horizon_after_acquire,
                horizon_after_finish - horizon_before_submit,
                horizon_after_submit - horizon_after_finish,
                horizon_after_present - horizon_after_submit,
                horizon_before_catch_up - horizon_after_present,
                Instant::now() - horizon_before_catch_up,
            ],
            state.frontend.screen == GameScreen::InGame
                && !state.match_state.match_presentation.in_game_menu.is_open()
                && game_render_output.is_some(),
            [
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
                state.render_width(),
                state.render_height(),
            ],
            state.persistence.options_profile.switch_render_interval(),
        );

        Ok(())
    }

    /// Spend the time a blocking present took on the gameplay frames the pacer
    /// still owes.
    ///
    /// Retail admits one Logic pass per present, which is indistinguishable at
    /// 60 Hz. Horizon presents at 30 Hz or slower, so keeping that rule would
    /// run the match at half speed: scenario time, animations and fire rates
    /// all stretch. The pacing anchor advances one window per admitted frame,
    /// so this loop drains the backlog and stops as soon as the sim is current.
    /// The cap keeps a stalled GPU from spending the whole pass on catch-up.
    fn catch_up_match_runtime(state: &mut AppState, event_loop: &ActiveEventLoop) {
        const MAX_CATCH_UP_FRAMES: usize = 4;
        if state.frontend.screen != GameScreen::InGame
            || crate::app::match_runtime::network_match::frame_budget(state).is_some()
        {
            return;
        }
        let mut ran = 0usize;
        for _ in 0..MAX_CATCH_UP_FRAMES {
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            let game_speed = crate::app::match_runtime::sim_tick::current_game_speed(state);
            // A desktop's untimed scenario stays unpaced by design. A
            // display-limited port paces fastest, so it needs the same backlog.
            if state
                .platform
                .frame_pacer
                .budget_ms(game_speed, None)
                .is_none()
                || state.frontend.screen != GameScreen::InGame
            {
                break;
            }
            if !state.platform.frame_pacer.should_admit(
                now_ms,
                game_speed,
                None,
                state.match_state.paused(),
            ) {
                break;
            }
            if Self::service_match_runtime(state, event_loop, true).quit_requested {
                return;
            }
            ran += 1;
        }
        if ran != 0 {
            // Still owed after the cap means this present could not drain the
            // backlog: the match clock is losing ground against the wall clock.
            let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
            let game_speed = crate::app::match_runtime::sim_tick::current_game_speed(state);
            let starved = ran == MAX_CATCH_UP_FRAMES
                && state
                    .platform
                    .frame_pacer
                    .budget_ms(game_speed, None)
                    .is_some()
                && state.platform.frame_pacer.should_admit(
                    now_ms,
                    game_speed,
                    None,
                    state.match_state.paused(),
                );
            Self::report_catch_up(state, ran, starved);
        }
    }

    /// Report the achieved Logic cadence, at most every ten seconds, so a
    /// device log shows whether catch-up actually keeps the match clock in real
    /// time. The extra-frame count alone cannot: the same total is reached by a
    /// drained backlog and by a backlog that outgrows the catch-up cap.
    fn report_catch_up(state: &AppState, ran: usize, starved: bool) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static TOTALS: AtomicU64 = AtomicU64::new(0);
        static STARVED: AtomicU64 = AtomicU64::new(0);
        static NEXT_REPORT_MS: AtomicU64 = AtomicU64::new(0);
        static TICK_BASE: AtomicU64 = AtomicU64::new(u64::MAX);
        static WALL_MS_BASE: AtomicU64 = AtomicU64::new(u64::MAX);
        static SIM_MS_BASE: AtomicU64 = AtomicU64::new(u64::MAX);
        TOTALS.fetch_add(ran as u64, Ordering::Relaxed);
        if starved {
            STARVED.fetch_add(1, Ordering::Relaxed);
        }
        let now_ms = crate::app::match_runtime::frame_pacer::wall_clock_ms();
        let next = NEXT_REPORT_MS.load(Ordering::Relaxed);
        if now_ms >= next
            && NEXT_REPORT_MS
                .compare_exchange(next, now_ms + 10_000, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let total = TOTALS.swap(0, Ordering::Relaxed);
            let starved = STARVED.swap(0, Ordering::Relaxed);
            // The player-visible clock is the admitted-frame count: scenario time,
            // animation and ROF counters all advance one step per frame. A rate well
            // under the target is slow motion; the starved count says whether the
            // catch-up cap caused it — zero means the sim is current and the cost is
            // elsewhere, non-zero means Logic itself cannot keep up at this rate.
            let session = state
                .match_state
                .sim_runtime
                .as_ref()
                .map(|rt| rt.simulation.session());
            let tick = session.map_or(0, |session| session.tick());
            let sim_ms = session.map_or(0, |session| session.total_sim_ms());
            let tick_base = TICK_BASE.swap(tick, Ordering::Relaxed);
            let wall_base = WALL_MS_BASE.swap(now_ms, Ordering::Relaxed);
            let sim_base = SIM_MS_BASE.swap(sim_ms, Ordering::Relaxed);
            if tick_base == u64::MAX || wall_base == u64::MAX || sim_base == u64::MAX {
                log::info!("Frame pacer ran {total} extra Logic frames in the last 10 s");
            } else {
                let ticks = tick.saturating_sub(tick_base);
                let wall_ms = now_ms.saturating_sub(wall_base).max(1);
                let sim_advanced_ms = sim_ms.saturating_sub(sim_base);
                let target_rate = state.platform.frame_pacer.target_logic_rate(
                    crate::app::match_runtime::sim_tick::current_game_speed(state),
                );
                let present_ms = state.diag.frame_timer.frame_ms_mean();
                let presents_per_s = if present_ms > 0.0 {
                    1000.0 / present_ms
                } else {
                    0.0
                };
                log::info!(
                    "Frame pacer ran {total} extra Logic frames in the last 10 s \
                     ({starved} present(s) still owed after the cap): the pacer admitted \
                     {ticks} frame(s) in {wall_ms} ms wall ({:.1}/s of its own {:.1}/s target, \
                     {presents_per_s:.1} presents/s at {present_ms:.1} ms); \
                     sim clock advanced {sim_advanced_ms} ms ({:.3}x real time)",
                    ticks as f64 * 1000.0 / wall_ms as f64,
                    target_rate,
                    sim_advanced_ms as f64 / wall_ms as f64
                );
            }
        }
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
