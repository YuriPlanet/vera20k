//! Top-level frame orchestration, screen dispatch, presentation, and readback.
//!
//! Simulation admission, draw composition, submit/present, transition commits,
//! captures, and loading-after-present remain in their original source order.

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

impl App {
    /// Service the match runtime for one pass: the outcome voice wait, the
    /// scenario exit cascade, the quit cascade, the audio pump and, when
    /// admitted, one simulation advance. Shared by the render pass and the
    /// hidden-window service loop in `about_to_wait`, so the world keeps the
    /// same cadence whether or not a frame is drawn. Returns true when the
    /// quit cascade finished and requested the event loop exit.
    pub(super) fn service_match_runtime(
        state: &mut AppState,
        event_loop: &ActiveEventLoop,
        simulation_allowed: bool,
    ) -> bool {
        // HouseClass keeps simulating for SavourDelay, then blocks on the
        // current outcome Vox before it raises the victory/defeat exit global.
        // Drive that gate before deciding whether another sim frame is legal.
        let scenario_now_ms =
            crate::app::match_runtime::sim_tick::monotonic_frame_pacer_ms(state, Instant::now());
        Self::consume_executed_abort_exit(state, scenario_now_ms);
        crate::app::match_runtime::sim_tick::drive_local_player_outcome_voice_wait(
            state,
            scenario_now_ms,
        );

        // The native victory/defeat handlers synchronously finish their audio
        // teardown before entering the score dialog. Drive the equivalent
        // sequence before either another sim frame or the destination screen.
        Self::drive_scenario_exit(state, scenario_now_ms);

        // Drive the graceful quit cascade (started on Exit-confirm OK). Compute the
        // voice poll before borrowing the cascade mutably to avoid aliasing.
        if state.frontend.quit_cascade.is_some() {
            let now = Instant::now();
            let voices_active = state
                .audio
                .sfx_player
                .as_ref()
                .is_some_and(|sfx| sfx.voices_active());
            let tick = state
                .frontend
                .quit_cascade
                .as_mut()
                .expect("cascade present")
                .tick(now, voices_active);
            if let (Some(vol), Some(player)) =
                (tick.music_volume, state.audio.music_player.as_mut())
            {
                player.set_volume(vol);
            }
            if tick.stop_music {
                state.audio.stop_theme();
            }
            if tick.finished {
                state.frontend.quit_cascade = None;
                event_loop.exit();
                return true;
            }
        }

        // The audio service pass. gamemd drives `AudioSystem::Pump @
        // 0x00406F70` from `Network_ServiceLoop @ 0x0048D080`, which the main
        // tick, the frame throttler, the modal dialog pump, the shell dialog
        // loop and the loading screens all reach — so the sound arbiter, the
        // EVA queue and `ThemeClass::AI` keep being serviced on the main menu,
        // behind a pause or an open menu, and while the window is not the
        // foreground. It therefore sits here, outside every screen and
        // simulation gate, and carries its own `> 33 ms` rate limit.
        crate::app::match_runtime::sim_tick::pump_audio_service(state, scenario_now_ms);

        // Deactivated windows do not simulate when `pause_on_focus_loss` is
        // configured. gamemd parks its main tick in a sleep-and-network-only
        // loop while the app is not the foreground, so the world is exactly
        // where the player left it on Alt+Tab return; the default keeps the
        // match running behind other windows instead. The gate sits at the
        // call site, not inside the runtime, so a focus edge never re-anchors
        // the frame pacer on its own.
        if simulation_allowed
            && matches!(state.frontend.screen, GameScreen::InGame)
            && !state.platform.focus_freeze_active()
            && state.match_state.scenario_exit.is_none()
            && state.match_state.scenario_outcome.is_none()
        {
            let now = Instant::now();
            let now_ms = sim_tick::monotonic_frame_pacer_ms(state, now);
            sim_tick::advance_in_game_runtime(state, now_ms);
            // EventClass EXIT is dispatched at the simulation tail. Consume
            // its terminal edge before any outcome route can claim teardown.
            Self::consume_executed_abort_exit(state, now_ms);
            // The SavourDelay expiry is decided in the late house rung of this
            // exact frame. Anchor its 0x78-bucket wall wait to the same observed
            // wall time instead of delaying it to the next render pass.
            crate::app::match_runtime::sim_tick::drive_local_player_outcome_voice_wait(
                state, now_ms,
            );
            Self::drive_scenario_exit(state, now_ms);
        }
        false
    }

    /// Does a hidden window still have a running match to service? True while
    /// in-game and the configured focus freeze is not holding the world; a
    /// tactical capture never reaches the hidden service loop.
    pub(super) fn match_runs_while_hidden(state: &AppState) -> bool {
        state.frontend.screen == GameScreen::InGame && !state.platform.focus_freeze_active()
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
        anyhow::ensure!(
            shell_capture.is_none() || tactical_capture.is_none(),
            "shell and tactical capture cannot share a render"
        );
        if let Some(session) = tactical_capture.as_deref_mut() {
            session.drive_before_render(state)?;
        }
        state.diag.frame_timer.sample(Instant::now());
        let tooltip_ms = crate::app::input::tooltips::update(state);
        // The message clock has to observe the focus freeze exactly as it
        // observes a modal pause: a banner on screen when the player Alt+Tabs
        // must survive the absence with its remaining lifetime intact, not
        // expire against wall time while the world is stopped. Park the clock
        // and skip the expiry pass; `messages::update` closes the span and
        // resumes ownership on the first foreground frame. Without the
        // configured freeze the world keeps running, so the clock does too.
        let message_ms = if state.frontend.screen == GameScreen::InGame
            && state.platform.focus_freeze_active()
        {
            let wall = crate::app::input::tooltips::now_ms(state);
            state
                .match_state
                .match_presentation
                .message_clock
                .set_paused(true, wall);
            None
        } else {
            crate::app::input::messages::update(state)
        };
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
            startup_splash::render_and_present(
                &state.renderer.gpu,
                &state.renderer.batch_renderer,
                &state.renderer.shell_surface_presenter,
                &state.renderer.depth_view,
                splash,
            )?;
            state
                .frontend
                .startup_splash
                .as_mut()
                .expect("active startup splash exists")
                .mark_presented(Instant::now());
            return Ok(());
        }
        state.frontend.startup_splash = None;

        // Outcome gates, exit cascades, the audio pump and the simulation
        // advance, shared with the hidden-window service loop.
        if Self::service_match_runtime(state, event_loop, tactical_capture.is_none()) {
            return Ok(());
        }

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

        let output: wgpu::SurfaceTexture = state
            .renderer
            .gpu
            .surface
            .get_current_texture()
            .map_err(|e| anyhow::anyhow!("Surface texture: {}", e))?;
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
        let shell_capture_current_frame = match shell_capture.as_deref_mut() {
            Some(session) => session.should_capture_current_frame(state)?,
            None => false,
        };
        let mut game_render_output: Option<crate::app::presentation::render::GameRenderOutput> =
            None;

        if state.match_state.match_presentation.in_game_menu.is_open() {
            Self::ensure_skirmish_shell_chrome(state);
        }
        Self::update_saved_game_browser(state, false);
        match &state.frontend.screen {
            GameScreen::MainMenu if state.frontend.main_menu_shell_error.is_some() => {
                Self::render_shell_error(state, &mut encoder, &view)?;
            }
            _ if state.frontend.keyboard_dialog.is_some() => {
                crate::app::frontend::skirmish_shell_render::render_keyboard_shell(state, &mut encoder, &output.texture)?;
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
                } else if Self::wol_welcome_active(state) {
                    if crate::app::frontend::wol_welcome_render::render_wol_welcome_page(
                        state,
                        &mut encoder,
                        &output.texture,
                    )? {
                        presented_shell = PresentedShell::WolWelcome;
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
                } else if matches!(state.match_state.match_presentation.in_game_menu, crate::ui::pause_menu::InGameMenuState::SavedGame(_)) {
                    crate::app::frontend::skirmish_shell_render::render_saved_game_shell(state, &mut encoder, &output.texture)?;
                } else {
                    crate::app::frontend::skirmish_shell_render::render_in_game_options_shell(
                        state, &mut encoder, &output.texture,
                    )?;
                }
            }
            GameScreen::InGame if state.match_state.match_presentation.in_game_menu.is_open() => {
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
                let times = render::GameRenderTimes {
                    radar_ms: state.radar_presentation_ms(Instant::now()),
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
                #[cfg(feature = "dev-ui")]
                Self::render_diagnostic_ui(state, &mut encoder, &view);
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
                let score_rendered = Self::score_shell_active(state)
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
                    )?);
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
        let submission = state
            .renderer
            .gpu
            .queue
            .submit(std::iter::once(encoder.finish()));
        output.present();
        // wgpu 27 requires released surface references before reconfiguration;
        // the diagnostic action and loading commit below may resize the window.
        // https://docs.rs/wgpu/27.0.1/wgpu/struct.Surface.html#method.configure
        drop(view);
        state
            .renderer
            .retail_screenshot_frame_cache
            .commit_presented();
        // A family renderer that drew a timer-driven 0x71C frame this pass
        // advances it now that the frame reached the screen.
        state.frontend.shell_monitor.commit_presented();
        state.frontend.shell_page_title.commit_presented();
        state.frontend.shell_status_line.commit_presented();
        if let Some(page) = state.frontend.score_page.as_mut() {
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
        if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
            if let Some(receipt) = dialog.title_receipt.take() {
                anyhow::ensure!(
                    dialog.title.record_presented(receipt),
                    "keyboard title receipt was stale at present commit"
                );
            }
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
            if shell_capture_current_frame {
                shell_capture
                    .as_deref_mut()
                    .expect("shell capture session exists when readback completes")
                    .complete(state, surface_format, &pixels)?;
            } else {
                tactical_capture
                    .as_deref_mut()
                    .expect("tactical capture session exists when readback completes")
                    .complete_after_readback(state, surface_format, &pixels)?;
            }
            event_loop.exit();
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

        #[cfg(feature = "dev-ui")]
        Self::commit_diagnostic_ui_action(state);
        crate::app::loading::pump::after_loading_frame_presented(state);

        Ok(())
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
