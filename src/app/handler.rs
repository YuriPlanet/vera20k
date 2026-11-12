//! Winit lifecycle and window-event routing for the app orchestrator.
//!
//! Event priority and consumption order are player-visible contracts; this
//! module keeps the original handler body intact.

use super::frame::{FramePass, FrameRenderOutcome};
use super::input::dispatch;
use super::input::hotkeys::HotkeyCommand;
use super::input::precedence::{InputLayer, ModalPage, ScreenPage};
use super::{
    ActiveEventLoop, App, AppState, ApplicationHandler, ControlFlow, GameScreen, Instant, KeyCode,
    KeyEventExtModifierSupplement, MouseButton, MouseScrollDelta, PhysicalKey, PhysicalSize,
    ShellKey, WindowEvent, WindowId,
};
use crate::ui::pause_menu::InGameMenuState;

/// Active YR's launcher parent ignores IDCANCEL. Taking only an immutable
/// reference makes the no-close/no-replacement guarantee explicit at the
/// keyboard routing seam.
fn launcher_options_consumes_escape<T>(dialog: &Option<T>, is_escape: bool) -> bool {
    is_escape && dialog.is_some()
}

fn capture_render_continues(deadline: &mut Option<Instant>, outcome: FrameRenderOutcome) -> bool {
    if outcome == FrameRenderOutcome::Deferred {
        *deadline = None;
        true
    } else {
        false
    }
}

impl App {
    /// An event can interrupt WaitUntil before a capture's render cadence.
    /// Keep that render deadline while still waking for independently due
    /// lobby/network services. Offscreen frames have no swapchain pacing.
    fn capture_render_wait_until(
        &self,
        now: Instant,
        service_wake: Option<Instant>,
    ) -> Option<Instant> {
        self.capture_render_not_before
            .filter(|deadline| now < *deadline)
            .map(|deadline| service_wake.map_or(deadline, |service| deadline.min(service)))
    }

    /// Route the physical keyboard edge before any underlying shell input.
    /// This owns main-menu modal admission; the existing message-box owner
    /// owns its result and teardown.
    /// Native source: gamemd.exe's modal pump5D4D50 and MessageBoxProc5D36A0;
    /// [Win32 dialog key dispatch](https://learn.microsoft.com/en-us/windows/win32/dlgbox/dlgbox-programming-considerations#dialog-box-keyboard-interface).
    pub(crate) fn handle_main_menu_dialog_key(
        state: &mut AppState,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> bool {
        if !Self::main_menu_dialog_open(state) {
            return false;
        }
        let is_escape = code == KeyCode::Escape && pressed && !repeat;
        // VERA's Esc menu: the held key that opened it must not close it, and
        // Esc closes its open list first.
        if repeat
            && state
                .frontend
                .shell_message_box
                .as_ref()
                .is_some_and(|message_box| message_box.keeps_page())
        {
            return true;
        }
        if is_escape && Self::close_shell_menu_list(state) {
            return true;
        }
        if state.frontend.shell_message_box.is_some() {
            // IsDialogMessage in the native modal pump5D4D50 admits Enter
            // as IDOK1 and Escape as IDCANCEL2. MessageBoxProc5D36A0 maps
            // both to result1; GameExitDefeat6860EA returns to the shell.
            // Keep retained launcher dialogs on their separate Escape path.
            // The native message pump forwards repeated WM_KEYDOWN too;
            // its IsDialogMessage path has no previous-key-state filter.
            if pressed
                && let Some(key @ (ShellKey::Enter | ShellKey::Escape)) =
                    Self::shell_key_for_code(code)
                // Alt+Enter is WM_SYSKEYDOWN, not the IDOK-producing
                // WM_KEYDOWN path. Winit combines them; retain the native
                // distinction using the existing modifier owner.
                && (key != ShellKey::Enter || !state.platform.live_modifiers.alt_key())
                && !Self::route_shell_message_box_key(state, key)
            {
                // Preserve the existing empty-controller-route fallback.
                Self::resolve_shell_message_box(
                    state,
                    crate::ui::shell::modal::ModalResult::MessageBox(1),
                );
                state.platform.window.request_redraw();
            }
        } else if is_escape {
            if launcher_options_consumes_escape(&state.frontend.options_dialog, is_escape) {
                // `0xD5` ignores IDCANCEL=2. Preserve the same
                // dialog instance and emit no Apply/write/event.
                state.platform.window.request_redraw();
            } else {
                Self::close_main_menu_dialogs(state);
                state.platform.window.request_redraw();
            }
        }
        true
    }

    /// Drop every sticky in-game pointer byte.
    ///
    /// gamemd's capture-changed case drops the button-held byte and tears the
    /// band rectangle down; without this the right-drag pan keeps applying its
    /// anchor-relative step every frame and edge scroll stays inhibited until
    /// the player right-clicks again. The gadget substrate keeps its own
    /// sticky capture and must die on the same edge, or the next click is still
    /// routed to the region that captured it (the release that would free it
    /// was lost).
    pub(crate) fn release_in_game_pointer_state(state: &mut AppState) {
        state.match_state.input.tactical_mouse.reset_on_focus_loss();
        state.match_state.input.selection_state.cancel_drag();
        // VERA formation line: a line being drawn goes with the band box.
        state.match_state.input.formation_line.cancel();
        state.match_state.input.cancel_minimap_press();
        // A touch stream can end without an `Ended` when the window loses
        // focus with fingers down, and the live gesture owns the edge-scroll
        // inhibit, so it dies on the same edge as the rest of the pointer
        // state.
        state.match_state.input.touch_mapper.reset();
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cancel_pointer_capture();
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.diplomacy = Default::default();
        if let Some(page) = state.match_state.match_presentation.options_page.as_mut() {
            page.dialog.shell_cancel_pointer_gesture();
        }
        if let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_mut() {
            dialog.reset_interaction();
        }
        if let Some(browser) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
        {
            browser.reset_interaction();
        }
    }

    fn resize_surface_for_window_size(state: &mut AppState, size: PhysicalSize<u32>) {
        state.renderer.gpu.resize(size.width, size.height);
        state.renderer.depth_view = state.renderer.gpu.create_depth_texture();
        state
            .renderer
            .shell_surface_presenter
            .resize(&state.renderer.gpu);
        // The frame-index wave is driven by wall-clock ticks and repaints every
        // frame, so a mid-flight resize simply lets it finish; no snap/cancel.
        Self::invalidate_main_menu_movie_if_base_changed(state);
        Self::sync_zoom_pass(state);
        crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    }

    /// Keep the zoom pass matching the window and the Options choices: a
    /// source target of the window size over the zoom, in the window's
    /// shape or the original 4:3, or none when that is the whole window. The
    /// match reads its size through `render_width`/`render_height`, so a
    /// resize mid-match re-projects the battlefield and sidebar the same way.
    /// A sealed capture never zooms or takes bars.
    pub(crate) fn sync_zoom_pass(state: &mut AppState) {
        let (width, height) = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let geometry = Self::match_zoom_geometry(state);
        if geometry.is_whole_window(width, height) {
            state.renderer.upscale_pass = None;
        } else if let Some(pass) = state.renderer.upscale_pass.as_mut() {
            pass.set_geometry(&state.renderer.gpu, geometry);
        } else {
            state.renderer.upscale_pass = Some(crate::render::upscale_pass::UpscalePass::new(
                &state.renderer.gpu,
                geometry,
            ));
        }
    }

    /// Where a match frame sits in the window at the Options choices: the
    /// geometry the zoom pass takes, and the battle frame the Options page
    /// offers sidebar sizes for.
    pub(crate) fn match_zoom_geometry(
        state: &AppState,
    ) -> crate::render::upscale_pass::ZoomGeometry {
        use crate::render::upscale_pass::{FrameShape, ZoomGeometry};
        let (width, height) = (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        );
        let profile = &state.persistence.options_profile;
        let (percent, shape) = if state.platform.capture_client_size.is_some() {
            let zoom = state.platform.capture_presentation.zoom;
            (zoom.percent(width, height), FrameShape::Window)
        } else if profile.original_shape {
            (profile.zoom.percent(width, height), FrameShape::Original)
        } else {
            (profile.zoom.percent(width, height), FrameShape::Window)
        };
        ZoomGeometry::new(width, height, percent, shape)
    }

    /// The Options page's Full Screen box: switch the window now. On macOS this
    /// is the system's own full screen with its menu bar and Dock hidden (see
    /// `initialize`). The resize that follows re-fits the menus and the zoom.
    /// The window's new state reaches the profile through
    /// `follow_window_full_screen`.
    ///
    /// Full screen covers the Display list's screen, not whichever one the
    /// window happens to touch, so entering full screen never changes screens.
    /// The window is centred on that screen first: `winit` saves the placement
    /// it restores on leaving full screen while it enters, so the windowed
    /// window then comes back on the same screen the choice names
    /// ([`crate::app::display_target`]).
    pub(crate) fn set_full_screen(state: &mut AppState, full_screen: bool) {
        if !full_screen {
            // VERA: the game's own switch (the Options and Esc menu boxes, the
            // Full Screen key) leaves full screen to the launch window, not to
            // the placement winit saved on entry. The title-bar button and the
            // system shortcut stay the window system's.
            Self::restore_launch_window(state);
            return;
        }
        let window = state.platform.window.clone();
        let displays = crate::app::display_target::current_displays(&window);
        let target = state.persistence.options_profile.display.clone();
        crate::app::display_target::center_windowed(&window, &displays, &target);
        window.set_fullscreen(Some(crate::app::display_target::fullscreen_for(
            &displays, &target,
        )));
    }

    /// The main menu Esc menu's Set Window Size button, and the Full Screen switch going
    /// off: the window as a launch opens it.
    /// A full-screen window leaves full screen first and macOS animates that,
    /// so `follow_window_full_screen` does the rest once the window is out.
    pub(crate) fn restore_launch_window(state: &mut AppState) {
        if state.platform.capture_client_size.is_none() {
            state.platform.launch_window_owed = true;
            state.platform.window.set_fullscreen(None);
        }
    }

    /// The window owns full screen while the game runs, and can change it by
    /// itself (its title-bar button, the system shortcut). Each event-loop pass
    /// copies its state into the profile and the Options box, so they and the
    /// next launch agree with what the player sees. A resize alone would miss
    /// leaving full screen on macOS: winit clears the state only after the
    /// exit animation's last resize.
    ///
    /// Leaving full screen also re-asserts the Display choice's screen: winit
    /// puts the window back at the placement it saved on entry, which is not
    /// the chosen screen when the choice changed while the window was full
    /// screen. That restore lands on winit's own thread, so the placement is
    /// re-asserted every pass until the window really is on that screen.
    fn follow_window_full_screen(state: &mut AppState) {
        if state.platform.capture_client_size.is_some() {
            return;
        }
        let full_screen = state.platform.window.fullscreen().is_some();
        #[cfg(target_os = "macos")]
        if !full_screen {
            show_menu_bar_and_dock_after_full_screen();
        }
        if full_screen {
            state.platform.recenter_windowed = false;
        } else {
            let leaving_full_screen = state.persistence.options_profile.full_screen;
            // Only a placement still owed asks the system for its screens: the
            // list costs a display-list query, too much for every pass.
            if leaving_full_screen || state.platform.recenter_windowed {
                let displays = crate::app::display_target::current_displays(&state.platform.window);
                let on_choice = crate::app::display_target::window_is_on(
                    &state.platform.window,
                    &displays,
                    &state.persistence.options_profile.display,
                );
                if !on_choice {
                    crate::app::display_target::center_windowed(
                        &state.platform.window,
                        &displays,
                        &state.persistence.options_profile.display,
                    );
                }
                state.platform.recenter_windowed = !on_choice;
            }
            if std::mem::take(&mut state.platform.launch_window_owed) {
                let displays = crate::app::display_target::current_displays(&state.platform.window);
                let size = state.platform.launch_window_size;
                crate::app::display_target::restore_launch_placement(
                    &state.platform.window,
                    &displays,
                    &state.persistence.options_profile.display,
                    winit::dpi::LogicalSize::new(size.width, size.height),
                );
            }
        }
        let changed = state.persistence.options_profile.full_screen != full_screen;
        state.persistence.options_profile.full_screen = full_screen;
        if let Some(dialog) = state.frontend.options_dialog.as_mut() {
            dialog.sync_full_screen(full_screen);
        }
        if let Some(page) = state.match_state.match_presentation.options_page.as_mut() {
            page.dialog.sync_full_screen(full_screen);
        }
        Self::shell_menu_follow_full_screen(state, full_screen, changed);
    }

    /// After a frame could not be taken, fit the surface to the window's size
    /// as it is now. Resize events normally keep the two in step, but a
    /// swapchain still stale after its reconfigure can be sized for a window
    /// size no event reported, and only the window's own size frees it. winit
    /// 0.30.12's Windows `set_fullscreen` notes such a race: marking the window
    /// full screen sends resize messages with the old size. A sealed capture
    /// keeps its fixed surface, and a minimised window has no size to fit.
    pub(super) fn follow_window_size(state: &mut AppState) {
        if state.platform.capture_client_size.is_some() {
            return;
        }
        let size = state.platform.window.inner_size();
        let config = &state.renderer.gpu.config;
        if size.width == 0
            || size.height == 0
            || (size.width, size.height) == (config.width, config.height)
        {
            return;
        }
        log::warn!(
            "Window is {}x{} but its surface is {}x{}; fitting the surface to the window",
            size.width,
            size.height,
            config.width,
            config.height
        );
        Self::resize_surface_for_window_size(state, size);
        crate::app::input::cursor::apply_cursor_confinement(state);
    }

    fn campaign_score_animation_deadline(state: &AppState) -> Option<Instant> {
        if Self::campaign_score_shell_active(state) {
            state
                .frontend
                .campaign_score_page
                .as_ref()
                .and_then(|page| page.next_emblem_animation_tick())
        } else {
            None
        }
    }
}

/// winit 0.30 hides the menu bar and Dock just before a borderless game enters
/// full screen and does not undo it on leaving, so a windowed game would keep
/// them hidden while it is in front. Restore them once the window is out; the
/// FullScreen option is still set while the exit animation runs.
#[cfg(target_os = "macos")]
fn show_menu_bar_and_dock_after_full_screen() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationPresentationOptions as Options};

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let options = app.presentationOptions();
    if options.intersects(Options::HideDock | Options::HideMenuBar)
        && !options.contains(Options::FullScreen)
    {
        app.setPresentationOptions(Options::Default);
    }
}

impl App {
    fn accept_initialization_step(
        &mut self,
        event_loop: &ActiveEventLoop,
        result: anyhow::Result<super::initialize::InitializationStep>,
    ) {
        match result {
            Ok(super::initialize::InitializationStep::Pending(job)) => {
                self.pending_initialization = Some(job);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            Ok(super::initialize::InitializationStep::Ready(state, events)) => {
                let mut state = *state;
                // Resizes may have arrived while startup owned the renderer.
                Self::follow_window_size(&mut state);
                if let Some(request) = self.replay_request.take() {
                    let loading = crate::app::loading::pump::LoadingRequest::replay(
                        request
                            .replay
                            .standalone_startup()
                            .expect("validated replay request"),
                        request.map_beside.as_deref(),
                    );
                    state.match_state.match_diagnostics.begin_playback(
                        request.replay,
                        request.capture_output,
                        request.replay_sha256,
                    );
                    crate::app::loading::pump::begin_loading(&mut state, loading);
                }
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                let mut browser_spawn_failure = None;
                if let Some(request) = self.spawn_request.take()
                    && let Err(error) = Self::start_spawn_session(&mut state, request)
                {
                    let error = format!("CnCNet startup failed: {error:#}");
                    log::error!("{error}");
                    // docs/spawn.md: the page keeps its engine and may start
                    // another launch; the report follows the Ready status.
                    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                    {
                        crate::web::status(&error);
                        browser_spawn_failure = Some(error);
                    }
                    #[cfg(not(all(
                        feature = "wasm",
                        target_arch = "wasm32",
                        target_os = "unknown"
                    )))]
                    {
                        self.startup_failure = Some(error);
                        event_loop.exit();
                        return;
                    }
                }
                if let Some(session) = self.shell_capture.as_mut() {
                    session.prepare_state(&mut state);
                }
                if let Some(session) = self.hidden_run.as_mut()
                    && let Err(err) = session.prepare_state(&mut state)
                {
                    log::error!("The {} could not start: {err:#}", session.name());
                    session.fail(format!("{} preparation failed: {err:#}", session.name()));
                    event_loop.exit();
                    return;
                }
                // Clip the cursor to the window on first launch (
                // default). A focus regain re-applies it if the OS dropped it.
                let window_id = state.platform.window.id();
                self.state = Some(state);
                for event in events {
                    self.window_event(event_loop, window_id, event);
                }
                if let Some(state) = self.state.as_ref() {
                    crate::app::input::cursor::apply_cursor_confinement(state);
                }
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                crate::web::status(
                    &self
                        .state
                        .as_ref()
                        .and_then(|state| state.frontend.main_menu_shell_error.clone())
                        .unwrap_or_else(|| crate::i18n::tr("VERA:EngineReady")),
                );
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                if let (Some(error), Some(state)) = (browser_spawn_failure, self.state.as_mut()) {
                    Self::finish_browser_external_launch(state, Some(&error));
                }
                log::info!("Initialization complete");
            }
            Err(err) => {
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                {
                    // The lifecycle event keeps its English for the host; the
                    // page's status line is the player's.
                    crate::web::request_exit(Some(&format!("Startup failed: {err:#}")));
                    crate::web::status(&crate::i18n::tr_fill(
                        "VERA:StartupFailed",
                        &[("error", &format!("{err:#}"))],
                    ));
                }
                log::error!("Failed to initialize: {:#}", err);
                if self.spawn_request.is_some() || self.replay_request.is_some() {
                    self.startup_failure = Some(format!("CnCNet initialization failed: {err:#}"));
                } else if cfg!(not(all(target_arch = "wasm32", target_os = "unknown")))
                    && self.shell_capture.is_none()
                    && self.hidden_run.is_none()
                {
                    // A player's launch: say why before the process exits with
                    // a failure status (`finish_capture`). The browser page
                    // reports it above.
                    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                    crate::app::system_dialog::report_startup_failure(&err);
                    self.startup_failure = Some(format!("Startup failed: {err:#}"));
                }
                if let Some(session) = self.shell_capture.as_mut() {
                    session.fail(format!("shell capture initialization failed: {err:#}"));
                }
                if let Some(session) = self.hidden_run.as_mut() {
                    session.fail(format!("{} initialization failed: {err:#}", session.name()));
                }
                event_loop.exit();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        if let Some(job) = self.pending_initialization.as_mut() {
            job.admit_event_loop_turn();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.pending_initialization.is_some() {
            return;
        }
        if self.state.is_some() {
            #[cfg(target_os = "android")]
            {
                let state = self.state.as_mut().expect("checked initialized app");
                if let Err(error) = state.renderer.gpu.restore_surface() {
                    log::error!("Could not rebuild the surface: {error:#}");
                    event_loop.exit();
                    return;
                }
                // This owner resizes depth and offscreen targets as well, even
                // when Android changed dimensions without a resize event.
                Self::follow_window_size(state);
                state.platform.window.request_redraw();
            }
            return;
        }
        // Asked once AppKit has launched and activated the app; macOS keeps a
        // chooser from any other process behind the app the player was using.
        if std::mem::take(&mut self.ask_for_game_folder) && !super::app_bundle::ensure_game_folder()
        {
            log::info!("No game folder chosen; exiting");
            event_loop.exit();
            return;
        }
        log::info!("Application resumed — creating window and GPU context");
        let capture_dimensions = match (self.shell_capture.as_ref(), self.hidden_run.as_ref()) {
            (Some(session), None) => Some((session.request().width(), session.request().height())),
            (None, Some(session)) => Some(session.size()),
            (None, None) => None,
            (Some(_), Some(_)) => {
                log::error!("Shell capture and hidden runs are mutually exclusive");
                event_loop.exit();
                return;
            }
        };
        let result = Self::initialize(
            event_loop,
            capture_dimensions,
            self.startup_options,
            self.spawn_request.is_some() || self.replay_request.is_some(),
            self.replay_request
                .as_ref()
                .map(|request| request.retail_dir.as_path()),
            self.spawn_request.is_some() && Self::spawn_hidden_window_requested(),
            self.mod_switch.as_ref(),
        );
        let result = if capture_dimensions.is_some() {
            result.and_then(super::initialize::InitializationStep::finish_synchronously)
        } else {
            result
        };
        self.accept_initialization_step(event_loop, result);
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "android")]
        {
            // winit requires every native render surface to be dropped before
            // this callback returns. A startup continuation owns its GPU in a
            // closure, so drop it too and restart initialization on resume.
            // https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html#method.suspended
            self.pending_initialization = None;
            if let Some(state) = self.state.as_mut() {
                state.renderer.gpu.suspend_surface();
            }
            _event_loop.set_control_flow(ControlFlow::Wait);
        }
        log::info!("Application suspended");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Android may deliver a queued redraw after suspension. Keep focus and
        // input-release events flowing, but never render on a destroyed surface.
        #[cfg(target_os = "android")]
        if matches!(event, WindowEvent::RedrawRequested)
            && self
                .state
                .as_ref()
                .is_some_and(|state| !state.renderer.gpu.has_surface())
        {
            return;
        }
        if let Some(job) = &mut self.pending_initialization {
            if job.window_id() == _id {
                if matches!(event, WindowEvent::CloseRequested) {
                    self.pending_initialization = None;
                    event_loop.exit();
                } else if matches!(
                    &event,
                    WindowEvent::Focused(_)
                        | WindowEvent::Occluded(_)
                        | WindowEvent::Resized(_)
                        | WindowEvent::CursorEntered { .. }
                        | WindowEvent::CursorLeft { .. }
                        | WindowEvent::CursorMoved { .. }
                ) {
                    // The normal platform/input owners receive these edges
                    // once the state exists. No startup-only focus authority.
                    job.defer_window_event(event);
                }
            }
            return;
        }
        // The hidden window only waits for its network goodbyes to finish.
        if self
            .state
            .as_ref()
            .is_some_and(|state| state.frontend.network_goodbye_deadline.is_some())
        {
            return;
        }
        // Any event but a redraw may change the next picture; pointer motion
        // alone has a limit of its own.
        if let Some(state) = self.state.as_mut() {
            match event {
                WindowEvent::RedrawRequested => {}
                WindowEvent::CursorMoved { .. } | WindowEvent::AxisMotion { .. } => {
                    state.platform.redraw_gate.note_pointer_motion();
                }
                _ => state.platform.redraw_gate.note_input(),
            }
        }
        // docs/sim-thread.md: an event may write the world, which takes it
        // back from a tick still on the simulation thread. The browser's main
        // thread cannot wait for that, and on desktop the wait would hold the
        // picture. Every event but a redraw waits, in order, for the pass that
        // finds the tick back ([`Self::release_held_events`]). On desktop,
        // pointer motion with nothing held ahead of it goes on, so the cursor
        // keeps following the hand; a motion that writes the world waits there
        // for the tick instead.
        let pointer_motion = !cfg!(all(
            feature = "wasm-threads",
            target_arch = "wasm32",
            target_os = "unknown"
        )) && matches!(event, WindowEvent::CursorMoved { .. })
            && self.held_window_events.is_empty();
        if !matches!(event, WindowEvent::RedrawRequested)
            && !pointer_motion
            && self
                .state
                .as_ref()
                .is_some_and(|state| state.match_state.sim_runtime.is_lent())
        {
            self.held_window_events.push((_id, event));
            if let Some(state) = self.state.as_mut() {
                state.platform.window_events_held = true;
            }
            return;
        }
        let shell_capture_active = self.shell_capture.is_some();
        let hidden_run_active = self.hidden_run.is_some();
        // Set while this call re-dispatches a touch's mouse events: those carry
        // the parked touch cursor, not the hardware pointer.
        let from_touch = self.dispatching_touch;

        // Touchscreens have no OS cursor, so each contact is re-issued as the
        // mouse events its gesture means; `input::touch` owns that mapping and
        // carries the per-platform gate. Capture sessions stay input-sealed:
        // a raw touch falls through and counts as a violation below.
        #[cfg(not(target_os = "linux"))]
        {
            if !shell_capture_active
                && !hidden_run_active
                && let WindowEvent::Touch(touch) = &event
            {
                let Some(state) = self.state.as_mut() else {
                    return;
                };
                if state.persistence.pending_load.is_some() {
                    return;
                }
                // Android has no Escape key to offer, so a touch skips a playing
                // movie the way Escape does on desktop. An external mouse or
                // trackpad still does not: its clicks stay consumed and ignored
                // while a movie plays, exactly as on every other platform.
                #[cfg(target_os = "android")]
                if matches!(touch.phase, winit::event::TouchPhase::Started)
                    && let Some(movie) = state.frontend.fullscreen_movie.as_mut()
                {
                    log::info!("Touch skipped the movie");
                    movie.abort();
                    state.platform.window.request_redraw();
                    return;
                }
                let in_game = state.frontend.screen == GameScreen::InGame;
                let scale = state.platform.window.scale_factor();
                let output = state.match_state.input.touch_mapper.translate(
                    touch,
                    Instant::now(),
                    in_game,
                    scale,
                );
                // The pointer is the finger's until a real mouse moves again.
                state.match_state.input.cursor_from_touch = true;
                if let Some((dx, dy)) = output.pan
                    && in_game
                {
                    // Screen pixels to world pixels; the camera moves against
                    // the fingers so the map follows them. The per-frame camera
                    // clamp keeps an overshoot from leaving the playable area.
                    let zoom = state.match_state.input.zoom_level.max(0.01);
                    let (x, y) = (
                        state.match_state.input.camera_x,
                        state.match_state.input.camera_y,
                    );
                    crate::app::input::camera::set_camera_position(
                        &mut state.match_state.input,
                        (x + dx / zoom, y + dy / zoom),
                    );
                }
                self.dispatching_touch = true;
                for event in output.events.into_iter().flatten() {
                    self.window_event(event_loop, _id, event);
                }
                self.dispatching_touch = false;
                return;
            }
        }

        let Some(state) = &mut self.state else { return };
        state.sync_cursor_space();
        // Playback has presentation-only input. Exit is local and never
        // schedules EventClass EXIT or changes the recorded simulation.
        if state.match_state.match_diagnostics.is_replay_playback() {
            match &event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                    return;
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if code == KeyCode::Escape && event.state.is_pressed() {
                            event_loop.exit();
                        } else if matches!(
                            code,
                            KeyCode::ArrowLeft
                                | KeyCode::ArrowRight
                                | KeyCode::ArrowUp
                                | KeyCode::ArrowDown
                        ) {
                            if event.state.is_pressed() {
                                state.match_state.input.keys_held.insert(code);
                            } else {
                                state.match_state.input.keys_held.remove(&code);
                            }
                        }
                    }
                    return;
                }
                WindowEvent::CursorMoved { position, .. } => {
                    state.set_cursor_from_window(position.x as f32, position.y as f32);
                    return;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(position) => {
                            (position.y as f32 / 30.0).clamp(-3.0, 3.0)
                        }
                    };
                    crate::app::input::camera::apply_zoom(state, lines);
                    return;
                }
                WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::Occluded(_)
                | WindowEvent::RedrawRequested
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. } => {}
                _ => return,
            }
        }

        // winit 0.30.12's X11 Ime::create_context(window, false) sends an
        // Ime::Disabled lifecycle notification even for an unmapped window.
        // It carries no input. Recognize it at the shared capture entry;
        // the guards below still own every keyboard/pointer/IME input event.
        // https://github.com/rust-windowing/winit/blob/v0.30.12/src/platform_impl/linux/x11/ime/mod.rs
        #[cfg(target_os = "linux")]
        if (shell_capture_active || hidden_run_active)
            && state.platform.window.is_visible() == Some(false)
            && !state.platform.window.has_focus()
            && matches!(&event, WindowEvent::Ime(winit::event::Ime::Disabled))
        {
            return;
        }

        // A hidden run is driven entirely from `about_to_wait`. Input would
        // contaminate a capture's no-input production oracle or move a map
        // picture's camera, so none is ever dispatched.
        if hidden_run_active {
            let session = self.hidden_run.as_mut().expect("hidden run exists");
            match event {
                WindowEvent::CloseRequested => {
                    session.close_requested();
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    if session.accepts_size(size) {
                        Self::resize_surface_for_window_size(state, size);
                    } else {
                        event_loop.exit();
                    }
                }
                WindowEvent::Focused(true) => {
                    if session.reject_focus() {
                        event_loop.exit();
                    }
                }
                WindowEvent::KeyboardInput { .. }
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Ime(_)
                | WindowEvent::CursorMoved { .. }
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::PinchGesture { .. }
                | WindowEvent::PanGesture { .. }
                | WindowEvent::DoubleTapGesture { .. }
                | WindowEvent::RotationGesture { .. }
                | WindowEvent::TouchpadPressure { .. }
                | WindowEvent::AxisMotion { .. }
                | WindowEvent::Touch(_)
                | WindowEvent::DroppedFile(_)
                | WindowEvent::HoveredFile(_)
                | WindowEvent::HoveredFileCancelled => {
                    if session.reject_input("window") {
                        event_loop.exit();
                    }
                }
                _ => {}
            }
            return;
        }

        // A hidden Windows HWND is not guaranteed to receive WM_PAINT, so
        // capture frames are pumped from `about_to_wait`, never window input.
        if shell_capture_active {
            let session = self.shell_capture.as_mut().expect("capture session exists");
            match event {
                WindowEvent::CloseRequested => {
                    log::info!("Shell-capture window close requested");
                    session.fail("shell-capture window closed before bundle completion");
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    let expected =
                        PhysicalSize::new(session.request().width(), session.request().height());
                    if size != expected {
                        session.fail(format!(
                            "shell-capture surface resized to {}x{}, expected {}x{}",
                            size.width, size.height, expected.width, expected.height
                        ));
                        event_loop.exit();
                    } else {
                        Self::resize_surface_for_window_size(state, size);
                    }
                }
                WindowEvent::Focused(true) => {
                    session.fail("shell-capture window unexpectedly received focus");
                    event_loop.exit();
                }
                WindowEvent::KeyboardInput { .. }
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Ime(_)
                | WindowEvent::CursorMoved { .. }
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. }
                | WindowEvent::MouseWheel { .. }
                | WindowEvent::MouseInput { .. }
                | WindowEvent::PinchGesture { .. }
                | WindowEvent::PanGesture { .. }
                | WindowEvent::DoubleTapGesture { .. }
                | WindowEvent::RotationGesture { .. }
                | WindowEvent::TouchpadPressure { .. }
                | WindowEvent::AxisMotion { .. }
                | WindowEvent::Touch(_)
                | WindowEvent::DroppedFile(_)
                | WindowEvent::HoveredFile(_)
                | WindowEvent::HoveredFileCancelled => {
                    session.fail("shell-capture received unexpected window input");
                    event_loop.exit();
                }
                _ => {}
            }
            return;
        }

        // Physical input survives consumed shell, movie and tactical messages.
        // Native54F5C0->53EC90 polls the current buttons after such gates lift.
        if let WindowEvent::MouseInput {
            state: edge,
            button,
            ..
        } = &event
        {
            state
                .match_state
                .input
                .tactical_mouse
                .record_physical_button(*button, edge.is_pressed());
        }

        // Native does not expose menu/game input while its process-start splash
        // owns the display. Keep close/resize/redraw operational, but discard
        // player input until the post-present deadline has elapsed. The startup
        // logo plays before the splash and takes Play_Movie's own input.
        if state.frontend.fullscreen_movie.is_none()
            && state
                .frontend
                .startup_splash
                .as_ref()
                .is_some_and(|splash| splash.is_active(Instant::now()))
            && matches!(
                &event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::ModifiersChanged(_)
                    | WindowEvent::Ime(_)
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::CursorEntered { .. }
                    | WindowEvent::CursorLeft { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::PinchGesture { .. }
                    | WindowEvent::PanGesture { .. }
                    | WindowEvent::DoubleTapGesture { .. }
                    | WindowEvent::RotationGesture { .. }
                    | WindowEvent::TouchpadPressure { .. }
                    | WindowEvent::AxisMotion { .. }
                    | WindowEvent::Touch(_)
            )
        {
            return;
        }

        // A pause's end resyncs the held keys and gameplay modifiers; every
        // event passes here, so the frame after the pause sees it.
        let paused = state.match_state.paused();
        let key_edge = match &event {
            WindowEvent::KeyboardInput { event, .. } => match event.physical_key {
                PhysicalKey::Code(code) => Some((code, event.state.is_pressed())),
                _ => None,
            },
            _ => None,
        };
        state
            .match_state
            .input
            .track_pause(paused, state.platform.live_modifiers, key_edge);
        // Key edges own the modifier views on every screen, including utility
        // cards that consume command input. A release there must not stay held
        // after returning to the shell or entering another match.
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && let PhysicalKey::Code(code) = event.physical_key
        {
            crate::app::input::hotkeys::record_modifier_key(
                &mut state.platform.held_modifier_keys,
                &mut state.platform.live_modifiers,
                &mut state.match_state.input.hotkey_modifiers,
                code,
                event.state.is_pressed(),
                paused,
            );
        }
        // Keep physical button/modifier releases above alive while the modal
        // load owns input, so a key released here cannot stick after restore.
        if state.persistence.pending_load.is_some() {
            match &event {
                WindowEvent::CloseRequested => state.persistence.pending_load = None,
                WindowEvent::KeyboardInput { event, .. } if !event.state.is_pressed() => {}
                WindowEvent::RedrawRequested
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Focused(_)
                | WindowEvent::Occluded(_)
                | WindowEvent::CursorEntered { .. }
                | WindowEvent::CursorLeft { .. } => {}
                _ => return,
            }
        }
        if crate::app::frontend::status_screen::handle_event(state, &event, event_loop) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Close requested");
                Self::prepare_shell_terminal_exit(state);
                #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
                if Self::begin_network_goodbye(state) {
                    return;
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                // A zero dimension is how Windows reports a minimise — that
                // backend never sends `Occluded` — so the hidden flag is
                // derived here as well as from the occlusion event below.
                let hidden = size.width == 0
                    || size.height == 0
                    || state.platform.window.is_minimized().unwrap_or(false);
                Self::set_window_hidden(state, hidden);
                // A minimise carries no usable client size: the surface cannot
                // be configured to 0x0 and the UI-scale heuristic would read a
                // zero viewport. The restore edge delivers the real size.
                if !hidden {
                    Self::resize_surface_for_window_size(state, size);
                    // A resized window changes the rectangle the cursor is
                    // clipped to, so re-apply the lock (no-op if it is off).
                    crate::app::input::cursor::apply_cursor_confinement(state);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // A monitor DPI change starts a new physical coordinate frame.
                // The first subsequent cursor event must establish its anchor.
                state.match_state.input.reset_window_cursor_motion();
            }
            WindowEvent::Focused(active) => {
                state.match_state.input.reset_window_cursor_motion();
                // The OS drops the cursor clip on alt-tab, so re-apply the
                // player's confinement choice whenever the window returns.
                if active {
                    crate::app::input::cursor::refresh_pointer_in_client(state);
                    crate::app::input::cursor::apply_cursor_confinement(state);
                    // Back from the folder, perhaps with a new mod in it.
                    Self::refresh_mods_page(state);
                }
                if !active {
                    // Give the arrow back at once: a backgrounded window may
                    // draw no frame that would.
                    crate::app::input::hardware_cursor::give_back_arrow(state);
                    // `release_in_game_pointer_state` also drops a touch stream
                    // that ended without an `Ended`, so no phantom finger turns
                    // the next tap into a two-finger pan.
                    crate::app::input::cursor::release_cursor_confinement(state);
                    Self::release_in_game_pointer_state(state);
                    if let Some(browser) = state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .as_mut()
                    {
                        browser.reset_interaction();
                    }
                    if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                        dialog.reset_interaction();
                    }
                    if let Some(lan) = state.frontend.lan.as_mut() {
                        lan.lobby.cancel_pointer();
                    }
                    if let Some(crate::app::shell_wol::WolPage::Login(login)) =
                        state.frontend.wol.as_mut().map(|visit| &mut visit.page)
                    {
                        login.cancel_pointer();
                    }
                    if let Some(lobby) = state
                        .frontend
                        .wol
                        .as_mut()
                        .and_then(|visit| visit.lobby_mut())
                    {
                        lobby.ui.cancel_pointer();
                    }
                    if let Some(setup) = state.frontend.multiplayer_setup.as_mut() {
                        setup.cancel_pointer();
                    }
                    if let Some(dialog) = state.frontend.options_dialog.as_mut() {
                        dialog.shell_cancel_pointer_gesture();
                    }
                    // A shell slider holding the mouse loses it with the
                    // focus. Native keeps its hold bytes until the next move
                    // without the button releases them (`0x0061E3AF`).
                    let skirmish = &mut state.frontend.skirmish_shell_state;
                    skirmish.trackbar_hold = None;
                    if let Some(setup) = skirmish.random_map_setup_modal.as_mut() {
                        setup.players_hold = None;
                        setup.advanced_hold = None;
                    }
                    if let Some(campaign) = state.frontend.campaign.as_mut() {
                        campaign.slider_release();
                    }
                }
                Self::set_window_active(state, active);
            }
            WindowEvent::Occluded(occluded) => {
                // Fully covered on the backends that report it. Windows does
                // not; see the `Resized` arm above. A spawn test run's
                // deliberately invisible window keeps running as if shown.
                let invisible_spawn = Self::invisible_spawn_window(state);
                Self::set_window_hidden(state, occluded && !invisible_spawn);
            }
            WindowEvent::CursorEntered { .. } => {
                state.match_state.input.reset_window_cursor_motion();
                // The pointer is back over the drawable region: mouse.shp owns it
                // again and the OS arrow goes. Redraw so the cursor decision and the
                // clip both follow the edge.
                state.platform.pointer_in_client = true;
                state.platform.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                // macOS's cursor lock, or the strip beside a camera housing:
                // the pointer goes straight back in.
                if crate::app::input::cursor::hold_pointer_in_window(state) {
                    state.platform.window.request_redraw();
                    return;
                }
                state.match_state.input.reset_window_cursor_motion();
                // winit raises this from `WM_MOUSELEAVE`, which it arms on the
                // matching enter and which fires when the pointer crosses onto the
                // title bar or resize border, not only off the window. There the
                // system owns the arrow (`DefWindowProc` answers the non-client
                // `WM_SETCURSOR` with it), so VERA stops drawing mouse.shp rather
                // than fighting it for the same pixel.
                state.platform.pointer_in_client = false;
                state.platform.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(_) => {
                // Deliberately not a modifier source: winit re-derives this
                // event from each mouse event's flags, which drop a held
                // Control on macOS (see `HeldModifierKeys`). The key edges
                // recorded before screen-specific dispatch own this state.
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // An open composition owns the keys (`app::input::ime`).
                if event.state.is_pressed() && state.platform.ime.composing() {
                    return;
                }
                let layer = crate::app::input::precedence::input_layer(state);
                match layer {
                    InputLayer::HouseOutcomeWait => {
                        if !event.state.is_pressed()
                            && let PhysicalKey::Code(code) = event.physical_key
                        {
                            state.match_state.input.keys_held.remove(&code);
                        }
                        return;
                    }
                    InputLayer::Page(ModalPage::NetworkFailure) => {
                        crate::app::match_runtime::network_failure::key(state, &event);
                        return;
                    }
                    _ => {}
                }
                // Host delivery adaptation: dialogs may consume ordinary keys,
                // but a release for an admitted planning gesture must still
                // end that gesture. Opening a menu itself never commits it.
                if !event.state.is_pressed()
                    && state.frontend.screen == GameScreen::InGame
                    && let PhysicalKey::Code(code) = event.physical_key
                    && state.match_state.input.planning.owns_key(code)
                {
                    crate::app::input::planning_mode::key_edge(
                        state,
                        crate::app::input::hotkeys::HotkeyResolution::Unhandled,
                        code,
                        event.state,
                        event.repeat,
                    );
                    state.match_state.input.keys_held.remove(&code);
                    return;
                }
                // VERA's every-screen keys, as the Keyboard page's VERA section
                // binds them: Lock Cursor to Window (Ctrl+Alt+L), Full Screen
                // (Alt+Enter) and Performance Stats (Ctrl+Alt+F) by default. Scroll
                // Lock and, on macOS, the system's Ctrl+Cmd+F stay fixed
                // extras ahead of the bindings. All are consumed before retail
                // key dispatch, but a key the Keyboard page is capturing goes
                // to the page.
                let modifiers = state.platform.live_modifiers;
                let fixed = match event.physical_key {
                    PhysicalKey::Code(KeyCode::ScrollLock) => Some(HotkeyCommand::LockCursor),
                    PhysicalKey::Code(KeyCode::KeyF)
                        if cfg!(target_os = "macos")
                            && modifiers.control_key()
                            && modifiers.super_key()
                            && !modifiers.alt_key() =>
                    {
                        Some(HotkeyCommand::FullScreen)
                    }
                    _ => None,
                };
                let capturing = state
                    .frontend
                    .keyboard_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.capture_focused);
                let window_command = fixed.or_else(|| {
                    if capturing {
                        return None;
                    }
                    let unmodified = event.key_without_modifiers();
                    let logical = crate::app::input::hotkeys::binding_logical_key(
                        &event.logical_key,
                        &unmodified,
                        event.location,
                    );
                    state.match_state.input.hotkey_bindings.window_command(
                        logical,
                        event.location,
                        modifiers,
                    )
                });
                // A capture keeps its fixed surface, and its Full Screen key
                // goes on to the screen.
                if let Some(command) = window_command
                    && (command != HotkeyCommand::FullScreen
                        || state.platform.capture_client_size.is_none())
                {
                    if event.state.is_pressed() && !event.repeat {
                        dispatch::run_window_command(state, command);
                    }
                    return;
                }
                // The keys above work under every layer; the rest follow it.
                let screen = match layer {
                    InputLayer::Screen(screen) => screen,
                    // The wait loop pumps 0xEA without IsDialogMessage and
                    // polls no game keys while it is open (0x006497EC).
                    InputLayer::Page(ModalPage::NetworkWait) => {
                        if !event.state.is_pressed()
                            && let PhysicalKey::Code(code) = event.physical_key
                        {
                            state.match_state.input.keys_held.remove(&code);
                        }
                        return;
                    }
                    InputLayer::Page(ModalPage::Keyboard) => {
                        crate::app::input::keyboard::key(state, &event);
                        return;
                    }
                    InputLayer::Page(ModalPage::ShellMessageBox) => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            Self::raise_shell_message_box(state);
                            Self::handle_main_menu_dialog_key(
                                state,
                                code,
                                event.state.is_pressed(),
                                event.repeat,
                            );
                        }
                        return;
                    }
                    // The status card took the shell error's keys.
                    InputLayer::ShellTransition
                    | InputLayer::HouseOutcomeWait
                    | InputLayer::Page(ModalPage::NetworkFailure | ModalPage::ShellError) => {
                        return;
                    }
                };
                if matches!(screen, ScreenPage::NetworkSetup | ScreenPage::Skirmish)
                    && crate::app::frontend::preview_zoom::key(state, &event)
                {
                    return;
                }
                if Self::shell_menu_key(state, &event, screen) {
                    return;
                }
                if screen == ScreenPage::CoopScore {
                    crate::app::input::coop_score::key(state, &event);
                    return;
                }
                if screen == ScreenPage::LanLobby {
                    crate::app::shell_lan::key(state, &event);
                    return;
                }
                // VERA: typing in Choose Map fills its search line.
                if matches!(screen, ScreenPage::NetworkSetup | ScreenPage::Skirmish)
                    && Self::choose_map_search_key(state, &event)
                {
                    return;
                }
                if screen == ScreenPage::NetworkSetup {
                    if state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .is_none()
                    {
                        crate::app::input::multiplayer_setup::key(state, &event);
                    } else if event.state.is_pressed() && !event.repeat {
                        // ChooseMap's loop has no IsDialogMessage. Only the
                        // existing eject prompt consumes its default keys.
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if Self::shell_key_for_code(code)
                                .is_some_and(|key| Self::route_validation_modal_key(state, key))
                            {
                                return;
                            }
                            if matches!(
                                code,
                                KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
                            ) {
                                Self::answer_choose_map_prompt_key(state, code != KeyCode::Escape);
                            }
                        }
                    }
                    return;
                }
                if screen == ScreenPage::Skirmish
                    && state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .is_some()
                {
                    if event.state.is_pressed() {
                        let code = match event.physical_key {
                            PhysicalKey::Code(code) => Some(code),
                            _ => None,
                        };
                        Self::handle_saved_seed_browser_key(state, code, event.text.as_deref());
                        state.platform.window.request_redraw();
                    }
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    // ESC always reaches the handler when in-game (even when paused)
                    // so the player can toggle pause regardless of diagnostic GUI focus.
                    let is_escape: bool =
                        code == KeyCode::Escape && event.state.is_pressed() && !event.repeat;
                    let in_game: bool = state.frontend.screen == GameScreen::InGame;
                    let paused_at_event = in_game && state.match_state.paused();
                    let locally_paused_at_event = in_game && state.match_state.local_paused();

                    // A shared pause must be resumable without a simulation
                    // tick, including while the local Options dialog is open.
                    let modifiers = state.platform.live_modifiers;
                    if in_game
                        && code == KeyCode::Pause
                        && !(modifiers.shift_key()
                            || modifiers.control_key()
                            || modifiers.alt_key()
                            || modifiers.super_key())
                        && crate::app::input::pause_menu::multiplayer_control(state).is_some()
                    {
                        if event.state.is_pressed() && !event.repeat {
                            crate::app::input::pause_menu::toggle_multiplayer_pause(state);
                        }
                        return;
                    }

                    if screen == ScreenPage::FullscreenMovie {
                        Self::fullscreen_movie_key(state, code, event.state.is_pressed());
                        return;
                    }
                    if screen == ScreenPage::CreditsRoll {
                        let modifiers = state.platform.live_modifiers;
                        let bare = !(modifiers.shift_key()
                            || modifiers.control_key()
                            || modifiers.alt_key());
                        // Keyboard::Get() == 0x1B: a bare Escape press. Releases
                        // (0x800) and every other key are queued and ignored.
                        let input = if code == KeyCode::Escape && event.state.is_pressed() && bare {
                            crate::app::frontend::credits_roll::CreditsInput::Escape
                        } else {
                            crate::app::frontend::credits_roll::CreditsInput::Other
                        };
                        Self::credits_roll_input(state, input);
                        return;
                    }

                    if Self::handle_main_menu_dialog_key(
                        state,
                        code,
                        event.state.is_pressed(),
                        event.repeat,
                    ) {
                        return;
                    }
                    if Self::handle_main_menu_typed_code_key(state, &event) {
                        return;
                    }

                    if screen == ScreenPage::Skirmish
                        && event.state.is_pressed()
                        && !event.repeat
                        && Self::shell_key_for_code(code)
                            .is_some_and(|key| Self::route_validation_modal_key(state, key))
                    {
                        return;
                    }

                    if screen == ScreenPage::Skirmish
                        && event.state.is_pressed()
                        && !event.repeat
                        && matches!(
                            code,
                            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
                        )
                        && Self::answer_choose_map_prompt_key(state, code != KeyCode::Escape)
                    {
                        return;
                    }

                    if screen == ScreenPage::Wol {
                        Self::handle_wol_key(
                            state,
                            Some(code),
                            event.state.is_pressed(),
                            event.text.as_deref(),
                        );
                        state.platform.window.request_redraw();
                        return;
                    }

                    if screen == ScreenPage::LoadSavedGame {
                        // The message box takes Enter/Escape; the page
                        // ignores IDOK/IDCANCEL (0x00558A30).
                        if event.state.is_pressed() {
                            Self::handle_load_saved_game_key(state, code);
                        }
                        state.platform.window.request_redraw();
                        return;
                    }

                    if matches!(
                        screen,
                        ScreenPage::InGameShell(InGameMenuState::SavedGame(_))
                    ) {
                        if event.state.is_pressed() {
                            Self::saved_game_key(state, Some(code), event.text.as_deref());
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if in_game
                        && state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Surrender
                    {
                        // 0x005D36A0: IDOK and IDCANCEL both answer result 1.
                        if event.state.is_pressed()
                            && !event.repeat
                            && matches!(
                                code,
                                KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape
                            )
                        {
                            crate::app::input::surrender::key(state);
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if in_game
                        && state.match_state.match_presentation.in_game_menu
                            == InGameMenuState::Page
                    {
                        // 0xCF's loop takes every key; Escape names nothing.
                        // A release still drops its held key: the loop's
                        // KeyboardClear (0x0077D861) leaves nothing latched.
                        if event.state.is_pressed() {
                            crate::app::shell_wol::page::key_press(
                                state,
                                Some(code),
                                event.text.as_deref(),
                            );
                        } else {
                            state.match_state.input.keys_held.remove(&code);
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if matches!(
                        screen,
                        ScreenPage::InGameShell(
                            InGameMenuState::Sound
                                | InGameMenuState::Diplomacy
                                | InGameMenuState::MissionBriefing
                        )
                    ) {
                        // B8 has no IDOK/IDCANCEL close command. Neither has
                        // 0x73: its proc keys commands by GetDlgCtrlID(lParam)
                        // (0x006581A7), and Return 0x423 is owner-drawn, so
                        // Enter and Escape name no child it handles.
                        return;
                    }
                    if screen == ScreenPage::InGameShell(InGameMenuState::AbortConfirm) {
                        // B6 accepts default IDOK1 as Resume (4F1A59..4F1A65).
                        // Owner buttons reject focus through610CA0:6118BF..DE;
                        // their resource has no WS_TABSTOP. Do not invent a
                        // focused-button Return/Space route. IDCANCEL is ignored.
                        if event.state.is_pressed()
                            && !event.repeat
                            && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter)
                        {
                            crate::app::input::abort::activate(
                                state,
                                crate::ui::shell::abort::AbortButton::Resume,
                            );
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if !crate::app::input::hotkeys::input_admitted_while_paused(
                        // Shared pause still admits chat. The keyboard owner
                        // consumes chat first, then blocks gameplay hotkeys.
                        locally_paused_at_event,
                        &event.logical_key,
                    ) {
                        return;
                    }

                    if screen == ScreenPage::Skirmish
                        && event.state.is_pressed()
                        && !is_escape
                        && Self::handle_skirmish_shell_key_input(state, code, event.text.as_deref())
                    {
                        return;
                    }

                    // An existing modal handles Escape before tactical input.
                    // World Escape goes through the bindings below; Options
                    // opens B5 only if selected, or as the native fallback.
                    if in_game && is_escape && Self::in_game_menu_owns_escape(state) {
                        Self::route_in_game_menu_escape(state);
                        state.platform.window.request_redraw();
                        return;
                    }

                    if in_game {
                        let unmodified = event.key_without_modifiers();
                        crate::app::input::keyboard::in_game_key_edge(
                            state,
                            crate::app::input::keyboard::InGameKeyEdge {
                                physical: code,
                                logical: &event.logical_key,
                                unmodified: &unmodified,
                                location: event.location,
                                state: event.state,
                                repeat: event.repeat,
                                text: event.text.as_deref(),
                            },
                            paused_at_event,
                        );
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // The hardware pointer is back: edge auto-scroll may read the
                // band again. A touch's synthetic move carries the parked
                // cursor instead and keeps the inhibit up.
                if !from_touch {
                    state.match_state.input.cursor_from_touch = false;
                }
                state.set_cursor_from_window(position.x as f32, position.y as f32);
                let layer = crate::app::input::precedence::input_layer(state);
                match layer {
                    InputLayer::HouseOutcomeWait => return,
                    InputLayer::Page(ModalPage::NetworkFailure) => {
                        crate::app::match_runtime::network_failure::cursor_moved(state);
                        return;
                    }
                    _ => {}
                }
                // Nothing under VERA's enlarged map preview follows the
                // pointer; only the cursor over it moves.
                if matches!(
                    layer,
                    InputLayer::Screen(ScreenPage::NetworkSetup | ScreenPage::Skirmish)
                ) && crate::app::frontend::preview_zoom::is_open(state)
                {
                    state.platform.window.request_redraw();
                    return;
                }
                // Shared tooltip service: every move restarts the show delay
                // and hides a visible tip (study S1).
                crate::app::input::tooltips::on_mouse_move(state);
                let screen = match layer {
                    InputLayer::Screen(screen) => screen,
                    InputLayer::Page(ModalPage::NetworkWait) => {
                        crate::app::input::connection::cursor_moved(state);
                        return;
                    }
                    InputLayer::Page(ModalPage::Keyboard) => {
                        crate::app::input::keyboard::cursor_moved(state);
                        return;
                    }
                    // The open box takes the move; the update offer's change
                    // list follows it.
                    InputLayer::Page(ModalPage::ShellMessageBox) => {
                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        crate::app::updater::notes::cursor_moved(state);
                        Self::shell_menu_pointer(state, None);
                        return;
                    }
                    // The status card took the shell error's moves.
                    InputLayer::ShellTransition
                    | InputLayer::HouseOutcomeWait
                    | InputLayer::Page(ModalPage::NetworkFailure | ModalPage::ShellError) => {
                        return;
                    }
                };
                match screen {
                    ScreenPage::LanLobby => crate::app::shell_lan::cursor_moved(state),
                    ScreenPage::NetworkSetup => {
                        if state
                            .frontend
                            .skirmish_shell_state
                            .choose_map_modal
                            .is_some()
                        {
                            Self::handle_skirmish_shell_mouse_move(state);
                        } else {
                            crate::app::input::multiplayer_setup::cursor_moved(state);
                        }
                    }
                    ScreenPage::LauncherOptions => Self::handle_launcher_options_mouse(state, None),
                    ScreenPage::InGameShell(_) | ScreenPage::Battlefield => {
                        dispatch::handle_cursor_moved_in_game(state);
                    }
                    ScreenPage::Skirmish => Self::handle_skirmish_shell_mouse_move(state),
                    ScreenPage::SinglePlayer | ScreenPage::MoviesAndCredits => {
                        Self::handle_menu_page_mouse_move(state);
                    }
                    ScreenPage::MovieList => Self::handle_movie_list_mouse_move(state),
                    ScreenPage::Mods => Self::handle_mods_mouse_move(state),
                    ScreenPage::Campaign => Self::handle_campaign_mouse_move(state),
                    ScreenPage::LoadSavedGame => Self::handle_load_saved_game_mouse_move(state),
                    ScreenPage::Wol => Self::handle_wol_mouse_move(state),
                    ScreenPage::CoopScore => crate::app::input::coop_score::cursor_moved(state),
                    ScreenPage::Score => Self::handle_score_shell_mouse_move(state),
                    ScreenPage::CampaignScore => Self::handle_campaign_score_mouse_move(state),
                    // While the SHP quit-confirm modal owns the controller, the menu
                    // move handler must not re-activate 0xE2 and reset the gesture.
                    ScreenPage::MainMenu if state.frontend.shell_message_box.is_none() => {
                        Self::handle_main_menu_shell_mouse_move(state);
                    }
                    // The open message box takes the move; the update offer's
                    // change list follows it.
                    ScreenPage::MainMenu => {
                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        crate::app::updater::notes::cursor_moved(state);
                    }
                    ScreenPage::FullscreenMovie
                    | ScreenPage::CreditsRoll
                    | ScreenPage::Loading
                    | ScreenPage::InGameCard
                    | ScreenPage::ResultCard => {}
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                // A click in the game locks the cursor again after Ctrl+Alt+L
                // freed it, when the Options box keeps it locked. The click
                // still reaches the game.
                if btn_state.is_pressed() {
                    crate::app::input::cursor::relock_cursor_on_click(state);
                }
                let layer = crate::app::input::precedence::input_layer(state);
                match layer {
                    InputLayer::HouseOutcomeWait => {
                        if !btn_state.is_pressed() {
                            Self::release_in_game_pointer_state(state);
                        }
                        return;
                    }
                    InputLayer::Page(ModalPage::NetworkFailure) => {
                        crate::app::match_runtime::network_failure::mouse(
                            state,
                            button,
                            btn_state.is_pressed(),
                        );
                        return;
                    }
                    _ => {}
                }
                // Any button press/release kills a visible tooltip + pending
                // timer (all buttons incl. middle — study S1).
                crate::app::input::tooltips::on_button_event(state);
                let screen = match layer {
                    InputLayer::Screen(screen) => screen,
                    // 0xEA's route belongs to the in-game dispatch.
                    InputLayer::Page(ModalPage::NetworkWait) => {
                        dispatch::handle_mouse_input(state, button, btn_state);
                        return;
                    }
                    InputLayer::Page(ModalPage::Keyboard) => {
                        crate::app::input::keyboard::mouse(state, button, btn_state.is_pressed());
                        return;
                    }
                    InputLayer::Page(ModalPage::ShellMessageBox) => {
                        if button == MouseButton::Left {
                            Self::raise_shell_message_box(state);
                            if btn_state.is_pressed() {
                                Self::handle_shell_message_box_mouse_down(state);
                            } else {
                                Self::handle_shell_message_box_mouse_up(state);
                            }
                            state.platform.window.request_redraw();
                        }
                        return;
                    }
                    // The status card took the shell error's buttons.
                    InputLayer::ShellTransition
                    | InputLayer::HouseOutcomeWait
                    | InputLayer::Page(ModalPage::NetworkFailure | ModalPage::ShellError) => {
                        return;
                    }
                };
                let left = button == MouseButton::Left;
                let pressed = btn_state.is_pressed();
                if matches!(screen, ScreenPage::NetworkSetup | ScreenPage::Skirmish)
                    && crate::app::frontend::preview_zoom::mouse(state, pressed)
                {
                    return;
                }
                match screen {
                    // Play_Movie reads and ignores mouse buttons; Show_Credits
                    // queues them as ordinary Keyboard entries.
                    ScreenPage::FullscreenMovie => {}
                    ScreenPage::CreditsRoll => Self::credits_roll_input(
                        state,
                        crate::app::frontend::credits_roll::CreditsInput::Other,
                    ),
                    ScreenPage::MovieList if left => {
                        if pressed {
                            Self::handle_movie_list_mouse_down(state);
                        } else {
                            Self::handle_movie_list_mouse_up(state);
                        }
                    }
                    ScreenPage::Mods if left => {
                        if pressed {
                            Self::handle_mods_mouse_down(state);
                        } else {
                            Self::handle_mods_mouse_up(state);
                        }
                    }
                    ScreenPage::Campaign if left => {
                        if pressed {
                            Self::handle_campaign_mouse_down(state);
                        } else {
                            Self::handle_campaign_mouse_up(state);
                        }
                    }
                    ScreenPage::LoadSavedGame if left => {
                        Self::handle_load_saved_game_mouse(state, pressed);
                        state.platform.window.request_redraw();
                    }
                    ScreenPage::Wol if left => {
                        if pressed {
                            Self::handle_wol_mouse_down(state);
                        } else {
                            Self::handle_wol_mouse_up(state);
                        }
                        state.platform.window.request_redraw();
                    }
                    ScreenPage::LanLobby => crate::app::shell_lan::mouse(state, button, pressed),
                    ScreenPage::NetworkSetup => {
                        if state
                            .frontend
                            .skirmish_shell_state
                            .choose_map_modal
                            .is_none()
                        {
                            crate::app::input::multiplayer_setup::mouse(state, button, pressed);
                        } else if left && pressed {
                            Self::handle_skirmish_shell_mouse_down(state);
                        } else if left {
                            Self::handle_skirmish_shell_mouse_up(state);
                        }
                    }
                    ScreenPage::LauncherOptions if left => {
                        Self::handle_launcher_options_mouse(state, Some(pressed));
                    }
                    // A retained main-menu dialog takes the click: the SHP
                    // quit-confirm modal hit-tests its OK/Cancel here.
                    _ if Self::main_menu_dialog_open(state) => {
                        if screen == ScreenPage::MainMenu
                            && state.frontend.shell_message_box.is_some()
                            && left
                        {
                            if pressed {
                                Self::handle_shell_message_box_mouse_down(state);
                            } else {
                                Self::handle_shell_message_box_mouse_up(state);
                            }
                        }
                    }
                    ScreenPage::CoopScore => {
                        crate::app::input::coop_score::mouse(state, button, pressed)
                    }
                    ScreenPage::CampaignScore if left => {
                        if pressed {
                            Self::handle_campaign_score_mouse_down(state);
                        } else {
                            Self::handle_campaign_score_mouse_up(state);
                        }
                    }
                    ScreenPage::Score if left => {
                        if pressed {
                            Self::handle_score_shell_mouse_down(state);
                        } else {
                            Self::handle_score_shell_mouse_up(state);
                        }
                    }
                    ScreenPage::Skirmish if left => {
                        if pressed {
                            Self::handle_skirmish_shell_mouse_down(state);
                        } else {
                            Self::handle_skirmish_shell_mouse_up(state);
                        }
                    }
                    ScreenPage::SinglePlayer | ScreenPage::MoviesAndCredits if left => {
                        if pressed {
                            Self::handle_menu_page_mouse_down(state);
                        } else {
                            Self::handle_menu_page_mouse_up(state);
                        }
                    }
                    ScreenPage::MainMenu if left => {
                        if pressed {
                            Self::handle_main_menu_shell_mouse_down(state);
                        } else {
                            Self::handle_main_menu_shell_mouse_up(state);
                        }
                    }
                    ScreenPage::InGameShell(_) | ScreenPage::Battlefield => {
                        dispatch::handle_mouse_input(state, button, btn_state);
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y as f32 / 30.0).clamp(-3.0, 3.0),
                };
                let screen = match crate::app::input::precedence::input_layer(state) {
                    InputLayer::Screen(screen) => screen,
                    InputLayer::Page(ModalPage::Keyboard) => {
                        crate::app::input::keyboard::wheel(state, lines);
                        state.platform.window.request_redraw();
                        return;
                    }
                    InputLayer::Page(ModalPage::ShellMessageBox) => {
                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        crate::app::updater::notes::wheel(state, lines);
                        return;
                    }
                    // ComboDropWin consumes no mouse-wheel selection/scroll;
                    // the other pages have no wheel route.
                    InputLayer::HouseOutcomeWait
                    | InputLayer::ShellTransition
                    | InputLayer::Page(
                        ModalPage::ShellError | ModalPage::NetworkFailure | ModalPage::NetworkWait,
                    ) => return,
                };
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                if screen == ScreenPage::MainMenu && crate::app::updater::notes::wheel(state, lines)
                {
                    return;
                }
                if matches!(screen, ScreenPage::NetworkSetup | ScreenPage::Skirmish)
                    && crate::app::frontend::preview_zoom::is_open(state)
                {
                    return;
                }
                if screen == ScreenPage::Mods {
                    Self::handle_mods_wheel(state, lines);
                    return;
                }
                if screen == ScreenPage::Skirmish
                    && Self::handle_skirmish_shell_mouse_wheel(state, lines)
                {
                    state.platform.window.request_redraw();
                    return;
                }
                if matches!(screen, ScreenPage::InGameShell(_) | ScreenPage::Battlefield)
                    && !state.match_state.paused()
                    && !crate::app::input::campaign_input::is_locked(state)
                {
                    if crate::app::presentation::super_timers::wheel(state, lines) {
                        state.platform.window.request_redraw();
                        return;
                    }
                    // Optional player setting: over the battlefield the wheel changes the view
                    // distance tier, anchored on the tactical centre. Over the
                    // sidebar and the bottom strip it keeps gamemd's native
                    // SidebarUp/SidebarDown routing.
                    if state.persistence.options_profile.mouse_wheel_zoom
                        && crate::app::input::camera::cursor_over_battlefield(state)
                    {
                        crate::app::input::camera::cycle_view_distance(state, lines);
                        state.platform.window.request_redraw();
                        return;
                    }
                    // Every wheel notch scrolls the active build strip by one
                    // row. gamemd routes the wheel message straight to the
                    // SidebarUp / SidebarDown commands and has no world zoom.
                    if let Some(action) = dispatch::wheel_notch(lines) {
                        dispatch::sidebar_scroll_command(state, action);
                    }
                }
            }
            WindowEvent::Ime(ime) => crate::app::input::ime::event(state, ime),
            WindowEvent::RedrawRequested => {
                let state = {
                    self.release_held_events(event_loop);
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    state
                };
                // Inside the draw limits the draw waits for `about_to_wait`.
                if state
                    .platform
                    .redraw_gate
                    .hold_requested_draw(Instant::now())
                {
                    return;
                }
                match Self::render_frame(state, event_loop, None, None, FramePass::Draw) {
                    Ok(FrameRenderOutcome::Deferred) => {
                        event_loop.set_control_flow(ControlFlow::Poll);
                        state.platform.window.request_redraw();
                    }
                    Ok(_) => {}
                    Err(err) => {
                        log::error!("Render: {:#}", err);
                        if state
                            .match_state
                            .match_diagnostics
                            .playback_capture_requested()
                        {
                            state.frontend.external_launch_error =
                                Some(format!("Replay capture failed: {err:#}"));
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // `resumed` and `about_to_wait` may share the first event-loop turn.
        // A newly created continuation must wait for new_events, otherwise
        // its work is joined to the preceding slice without servicing input.
        if self
            .pending_initialization
            .as_ref()
            .is_some_and(|job| !job.may_resume())
        {
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }
        if let Some(job) = self.pending_initialization.take() {
            self.accept_initialization_step(event_loop, job.advance());
            // One completed startup slice per event-loop pass, including the
            // final admission. Ordinary input starts only with the full state.
            return;
        }
        // A tick on the simulation thread has ended (its wake brought this
        // call): finish it, replay the window events it held, then service
        // the match at once, so the next tick leaves without waiting for a
        // redraw. Captures and hidden runs service only with their pictures.
        self.install_sim_wake();
        let tick_back = self.shell_capture.is_none()
            && self.hidden_run.is_none()
            && self.state.as_mut().is_some_and(|state| {
                state.match_state.sim_runtime.is_lent()
                    && state.match_state.sim_runtime.pass_may_run(false)
            });
        self.release_held_events(event_loop);
        if tick_back
            && let Some(state) = self.state.as_mut()
            && !state.platform.window_hidden
        {
            let pass = Self::service_match_runtime(state, event_loop, true, true, false);
            if pass.quit_requested {
                return;
            }
            // Its frames count toward the online picture's limit, as the
            // render pass's own do (`defer_online_render`).
            state
                .platform
                .frame_pacer
                .count_frames_without_present(pass.completed_frames);
        }
        let shell_scroll_wake = if let Some(state) = self.state.as_mut() {
            #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
            if Self::poll_browser_exit(state, event_loop) {
                return;
            }
            #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
            if Self::poll_network_goodbye(state, event_loop) {
                return;
            }
            Self::follow_window_full_screen(state);
            // cursor lock: the OS drops the clip on focus loss /
            // Windows key, so re-assert the player's choice every frame while
            // the window is focused. Presentation-only, never part of the sim.
            crate::app::input::cursor::reassert_cursor_confinement(state);
            // Captures keep their sealed input; they never ask for an IME.
            // The browser's composed text waits with the window events a
            // tick on the simulation worker holds, so it cannot overtake a
            // key pressed before it ([`Self::release_held_events`]). Desktop
            // composition arrives as window events, which wait already.
            if self.shell_capture.is_none()
                && self.hidden_run.is_none()
                && !(cfg!(all(
                    feature = "wasm-threads",
                    target_arch = "wasm32",
                    target_os = "unknown"
                )) && state.match_state.sim_runtime.is_lent())
            {
                crate::app::input::ime::sync(state);
            }
            Self::update_saved_seed_browser_scroll(state, false);
            Self::update_load_saved_game_scroll(state, false);
            [
                crate::app::input::keyboard::poll_scroll_repeat(state),
                crate::app::shell_lan::poll(state),
                crate::app::shell_wol::poll(state),
                crate::app::shell_network_setup::poll(state),
                crate::app::input::multiplayer_setup::poll(state),
                crate::app::input::coop_score::active(state)
                    .then(|| crate::app::input::coop_score::poll(state))
                    .flatten(),
                crate::app::input::sound::poll_scroll_repeat(state),
                Self::poll_movie_list_scroll(state),
                Self::poll_mods_scroll(state),
                Self::poll_choose_map_scroll(state),
                Self::poll_skirmish_opponent_scroll(state),
                // Captures replay sealed input; they never read the library.
                (self.shell_capture.is_none() && self.hidden_run.is_none())
                    .then(|| crate::app::map_library::poll(state))
                    .flatten(),
                // Nor the login page's counts.
                (self.shell_capture.is_none() && self.hidden_run.is_none())
                    .then(|| crate::app::shell_wol::login_info::poll(state))
                    .flatten(),
                (self.shell_capture.is_none() && self.hidden_run.is_none())
                    .then(|| crate::app::map_picture::poll(state))
                    .flatten(),
                Self::poll_campaign(state),
                Self::poll_language_files(state),
                Self::campaign_score_animation_deadline(state),
                // Captures replay sealed input; no box tells them of a mod.
                (self.shell_capture.is_none() && self.hidden_run.is_none())
                    .then(|| crate::app::mods::poll(state))
                    .flatten(),
                // Captures replay sealed input; they never check or ask.
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                (self.shell_capture.is_none() && self.hidden_run.is_none())
                    .then(|| crate::app::updater::poll(state))
                    .flatten(),
            ]
            .into_iter()
            .flatten()
            .min()
        } else {
            None
        };
        // Keep background services on their existing owners while Android has
        // no surface, without scheduling a render or an acquire-retry loop.
        #[cfg(target_os = "android")]
        if self
            .state
            .as_ref()
            .is_some_and(|state| !state.renderer.gpu.has_surface())
        {
            let state = self.state.as_mut().expect("checked initialized app");
            let wake = if Self::match_runs_while_hidden(state) {
                if Self::service_match_runtime(state, event_loop, true, true, false).quit_requested
                {
                    return;
                }
                let service = Instant::now() + crate::app::match_runtime::frame_pacer::SERVICE_WAKE;
                Some(shell_scroll_wake.map_or(service, |wake| wake.min(service)))
            } else {
                shell_scroll_wake
            };
            event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
            return;
        }
        if self.shell_capture.is_some() || self.hidden_run.is_some() {
            // A callback already queued by the platform can arrive after the
            // completed one-shot capture requested exit. Never render it again.
            if self
                .shell_capture
                .as_ref()
                .is_some_and(|session| session.is_finished())
                || self
                    .hidden_run
                    .as_ref()
                    .is_some_and(|session| session.is_finished())
            {
                event_loop.exit();
                return;
            }
            if let Some(deadline) =
                self.capture_render_wait_until(Instant::now(), shell_scroll_wake)
            {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                return;
            }
        }
        if let (Some(state), Some(session)) = (self.state.as_mut(), self.hidden_run.as_mut()) {
            let outcome = match Self::render_frame(
                state,
                event_loop,
                None,
                Some(&mut *session),
                FramePass::Draw,
            ) {
                Ok(outcome) => outcome,
                Err(err) => {
                    log::error!("The {} could not render: {err:#}", session.name());
                    session.fail(format!("{} render failed: {err:#}", session.name()));
                    event_loop.exit();
                    return;
                }
            };
            if capture_render_continues(&mut self.capture_render_not_before, outcome) {
                event_loop.set_control_flow(ControlFlow::Poll);
                return;
            }
            if !session.is_finished() {
                let deadline = session.next_wake_deadline();
                self.capture_render_not_before = Some(deadline);
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    shell_scroll_wake.map_or(deadline, |service| deadline.min(service)),
                ));
            }
        } else if let (Some(state), Some(session)) =
            (self.state.as_mut(), self.shell_capture.as_mut())
        {
            let outcome = match Self::render_frame(
                state,
                event_loop,
                Some(&mut *session),
                None,
                FramePass::Draw,
            ) {
                Ok(outcome) => outcome,
                Err(err) => {
                    log::error!("Shell capture render: {err:#}");
                    session.fail(format!("shell capture render failed: {err:#}"));
                    event_loop.exit();
                    return;
                }
            };
            if capture_render_continues(&mut self.capture_render_not_before, outcome) {
                event_loop.set_control_flow(ControlFlow::Poll);
                return;
            }
            if !session.is_finished() {
                let mut deadline = session.next_wake_deadline();
                self.capture_render_not_before = Some(deadline);
                if let Some(wave_deadline) =
                    crate::app::frontend::shell_transition::main_menu_presented_wake_deadline(state)
                {
                    deadline = deadline.max(wave_deadline);
                }
                if let Some(animation_deadline) = Self::campaign_score_animation_deadline(state) {
                    deadline = deadline.min(animation_deadline);
                }
                if let Some(service) = shell_scroll_wake {
                    deadline = deadline.min(service);
                }
                event_loop.set_control_flow(if deadline <= Instant::now() {
                    ControlFlow::Poll
                } else {
                    ControlFlow::WaitUntil(deadline)
                });
            }
        } else if let Some(state) = self.state.as_mut() {
            if Self::next_online_render_frame(state).is_none() {
                state.platform.frame_pacer.clear_render_continuation();
            }
            if Self::invisible_spawn_window(state) {
                // An invisible window receives no RedrawRequested; drive the
                // same frame directly, as the hidden capture drivers do. It
                // stands for a visible window, so a still picture is probed.
                let pass = match state.platform.redraw_gate.next_pass(Instant::now()) {
                    crate::app::match_runtime::redraw_gate::NextPass::Probe => FramePass::Probe,
                    crate::app::match_runtime::redraw_gate::NextPass::Draw => FramePass::Draw,
                };
                let deferred = matches!(
                    Self::render_frame(state, event_loop, None, None, pass),
                    Ok(FrameRenderOutcome::Deferred)
                );
                event_loop.set_control_flow(if deferred {
                    ControlFlow::Poll
                } else {
                    ControlFlow::WaitUntil(Instant::now() + std::time::Duration::from_millis(8))
                });
            } else if state.platform.frame_pacer.render_deferred() {
                event_loop.set_control_flow(ControlFlow::Poll);
                state.platform.window.request_redraw();
            } else if crate::app::frontend::shell_transition::main_menu_presented_is_poisoned(state)
            {
                event_loop.set_control_flow(ControlFlow::Wait);
            } else if let Some(deadline) =
                crate::app::frontend::shell_transition::main_menu_presented_wake_deadline(state)
            {
                let deadline = shell_scroll_wake.map_or(deadline, |scroll| deadline.min(scroll));
                if Instant::now() >= deadline {
                    state.platform.window.request_redraw();
                } else {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
            } else if state.platform.window_hidden {
                if Self::match_runs_while_hidden(state) {
                    // Minimised or fully covered, but the match is not frozen:
                    // keep the world and the audio service advancing without
                    // drawing frames no one can see. The service is not the
                    // output gate: the `WM_ACTIVATEAPP` silence comes from
                    // `set_window_active`, which the minimise's deactivation
                    // edge reaches first. macOS stops presenting to an occluded
                    // layer, so this loop cannot ride on redraws; it wakes at
                    // half the pacer bucket to hold the cadence.
                    if Self::service_match_runtime(state, event_loop, true, true, false)
                        .quit_requested
                    {
                        return;
                    }
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        Instant::now() + crate::app::match_runtime::frame_pacer::SERVICE_WAKE,
                    ));
                } else {
                    // Nothing on screen to keep fresh: draw no frames until a
                    // window event (including the un-occlude edge). The shell's
                    // services still wake it, so a lobby, a LAN room or a Quick
                    // Match search keeps answering while the player is in
                    // another window.
                    event_loop.set_control_flow(
                        shell_scroll_wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
                    );
                }
            } else if state.platform.redraw_gate.next_pass(Instant::now())
                == crate::app::match_runtime::redraw_gate::NextPass::Probe
            {
                // The presented picture still stands, or the draw limits hold
                // the next one. Service the match as the hidden loop does; the
                // pass draws only once the picture changed and the limits let
                // it. Window events draw at once, and a moving picture or an
                // asked-for redraw draws as soon as the limits pass.
                let pass = state
                    .platform
                    .redraw_gate
                    .probe_due(Instant::now())
                    .then(|| Self::render_frame(state, event_loop, None, None, FramePass::Probe));
                match pass {
                    Some(Ok(FrameRenderOutcome::Deferred)) => {
                        event_loop.set_control_flow(ControlFlow::Poll);
                        state.platform.window.request_redraw();
                    }
                    outcome => {
                        if let Some(Err(err)) = outcome {
                            log::error!("Render: {err:#}");
                        }
                        let now = Instant::now();
                        if state.platform.redraw_gate.next_pass(now)
                            == crate::app::match_runtime::redraw_gate::NextPass::Draw
                        {
                            event_loop.set_control_flow(
                                shell_scroll_wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
                            );
                            state.platform.window.request_redraw();
                        } else {
                            let wake = state.platform.redraw_gate.probe_wake(now);
                            event_loop.set_control_flow(ControlFlow::WaitUntil(
                                shell_scroll_wake.map_or(wake, |service| service.min(wake)),
                            ));
                        }
                    }
                }
            } else {
                // A probe's wake must not outlive it: a past deadline would
                // spin this loop between refreshes.
                event_loop.set_control_flow(
                    shell_scroll_wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
                );
                state.platform.window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            crate::app::match_runtime::sim_tick::flush_replay_log(state);
            crate::app::map_picture::stop_for_exit(&mut state.frontend.map_pictures);
            crate::app::mods::left(state);
        }
        log::logger().flush();
        // A confirmed quit (its cascade's Theme Stop(0), 0x0052E7EC, has run)
        // or a page unload tears down a running game.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        self.browser_exiting();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deferred_capture_clears_render_delay_for_the_next_event_loop_pass() {
        let deadline = Instant::now() + std::time::Duration::from_millis(8);
        let mut not_before = Some(deadline);
        assert!(!capture_render_continues(
            &mut not_before,
            FrameRenderOutcome::Unavailable
        ));
        assert_eq!(not_before, Some(deadline));
        assert!(capture_render_continues(
            &mut not_before,
            FrameRenderOutcome::Deferred
        ));
        assert_eq!(not_before, None);
        assert!(!capture_render_continues(
            &mut not_before,
            FrameRenderOutcome::Presented
        ));
    }

    #[test]
    fn capture_render_deadline_survives_early_service_wakes_and_admits_when_due() {
        use std::time::Duration;

        let mut app = App::default();
        let rendered_at = Instant::now();
        assert_eq!(app.capture_render_wait_until(rendered_at, None), None);

        let deadline = rendered_at + Duration::from_millis(8);
        app.capture_render_not_before = Some(deadline);
        let service = rendered_at + Duration::from_millis(1);
        assert_eq!(
            app.capture_render_wait_until(rendered_at, Some(service)),
            Some(service)
        );
        assert_eq!(app.capture_render_wait_until(service, None), Some(deadline));
        assert_eq!(app.capture_render_not_before, Some(deadline));
        assert_eq!(app.capture_render_wait_until(deadline, None), None);
        assert_eq!(
            app.capture_render_wait_until(deadline + Duration::from_secs(1), None),
            None
        );
    }

    #[test]
    fn launcher_options_escape_is_consumed_without_replacing_the_parent() {
        let dialog = crate::ui::main_menu_dialogs::OptionsDialogState::default();
        let mut slot = Some(dialog);
        let identity_before = slot.as_ref().map(|value| value as *const _);

        assert!(launcher_options_consumes_escape(&slot, true));
        assert_eq!(
            slot.as_ref().map(|value| value as *const _),
            identity_before
        );
        assert!(!launcher_options_consumes_escape(&slot, false));

        slot = None;
        assert!(!launcher_options_consumes_escape(&slot, true));
    }
}
