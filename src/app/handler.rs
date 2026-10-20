//! Winit lifecycle and window-event routing for the app orchestrator.
//!
//! Event priority and consumption order are player-visible contracts; this
//! module keeps the original handler body intact.

use super::input::dispatch;
use super::{
    ActiveEventLoop, App, AppState, ApplicationHandler, ControlFlow, GameScreen, Instant, KeyCode,
    KeyEventExtModifierSupplement, MouseButton, MouseScrollDelta, PhysicalKey, PhysicalSize,
    ShellKey, WindowEvent, WindowId,
};

/// Active YR's launcher parent ignores IDCANCEL. Taking only an immutable
/// reference makes the no-close/no-replacement guarantee explicit at the
/// keyboard routing seam.
fn launcher_options_consumes_escape<T>(dialog: &Option<T>, is_escape: bool) -> bool {
    is_escape && dialog.is_some()
}

impl App {
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
        state.match_state.input.minimap_dragging = false;
        state
            .match_state
            .match_presentation
            .in_game_gadgets
            .cancel_pointer_capture();
        state
            .match_state
            .match_presentation
            .in_game_options
            .dragging_slider = None;
        state.match_state.match_presentation.in_game_options.buttons = Default::default();
        state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        state.match_state.match_presentation.diplomacy = Default::default();
        if let Some(dialog) = state.match_state.match_presentation.sound_dialog.as_mut() {
            dialog.reset_interaction();
        }
        if let Some(browser) = state
            .match_state
            .match_presentation
            .saved_game_browser
            .as_mut()
        {
            browser.pressed_control = None;
            browser.scroll_repeat_at = None;
            browser.last_list_press = None;
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
            (100, FrameShape::Window)
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
    pub(crate) fn set_full_screen(state: &mut AppState, full_screen: bool) {
        state
            .platform
            .window
            .set_fullscreen(full_screen.then_some(winit::window::Fullscreen::Borderless(None)));
    }

    /// The window owns full screen while the game runs, and can change it by
    /// itself (its title-bar button, the system shortcut). Each event-loop pass
    /// copies its state into the profile and the Options box, so they and the
    /// next launch agree with what the player sees. A resize alone would miss
    /// leaving full screen on macOS: winit clears the state only after the
    /// exit animation's last resize.
    fn follow_window_full_screen(state: &mut AppState) {
        if state.platform.capture_client_size.is_some() {
            return;
        }
        let full_screen = state.platform.window.fullscreen().is_some();
        #[cfg(target_os = "macos")]
        if !full_screen {
            show_menu_bar_and_dock_after_full_screen();
        }
        state.persistence.options_profile.full_screen = full_screen;
        if let Some(dialog) = state.frontend.options_dialog.as_mut() {
            dialog.sync_full_screen(full_screen);
        }
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        log::info!("Application resumed — creating window and GPU context");
        let capture_dimensions = match (self.shell_capture.as_ref(), self.tactical_capture.as_ref())
        {
            (Some(session), None) => Some((session.request().width(), session.request().height())),
            (None, Some(session)) => Some((session.request().width(), session.request().height())),
            (None, None) => None,
            (Some(_), Some(_)) => {
                log::error!("Shell and tactical capture modes are mutually exclusive");
                event_loop.exit();
                return;
            }
        };
        match Self::initialize(
            event_loop,
            capture_dimensions,
            self.startup_options,
            self.spawn_request.is_some(),
        ) {
            Ok(mut state) => {
                if let Some(request) = self.spawn_request.take()
                    && let Err(error) = Self::start_spawn_session(&mut state, request)
                {
                    let error = format!("CnCNet startup failed: {error:#}");
                    log::error!("{error}");
                    self.startup_failure = Some(error);
                    event_loop.exit();
                    return;
                }
                if let Some(session) = self.shell_capture.as_mut() {
                    session.prepare_state(&mut state);
                }
                if let Some(session) = self.tactical_capture.as_mut()
                    && let Err(err) = session.prepare_state(&mut state)
                {
                    log::error!("Tactical capture preparation failed: {err:#}");
                    session.fail(format!("tactical capture preparation failed: {err:#}"));
                    event_loop.exit();
                    return;
                }
                // Clip the cursor to the window on first launch (
                // default). A focus regain re-applies it if the OS dropped it.
                crate::app::input::cursor::apply_cursor_confinement(&state);
                self.state = Some(state);
                log::info!("Initialization complete");
            }
            Err(err) => {
                log::error!("Failed to initialize: {:#}", err);
                if self.spawn_request.is_some() {
                    self.startup_failure = Some(format!("CnCNet initialization failed: {err:#}"));
                }
                if let Some(session) = self.shell_capture.as_mut() {
                    session.fail(format!("shell capture initialization failed: {err:#}"));
                }
                if let Some(session) = self.tactical_capture.as_mut() {
                    session.fail(format!("tactical capture initialization failed: {err:#}"));
                }
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let shell_capture_active = self.shell_capture.is_some();
        let tactical_capture_active = self.tactical_capture.is_some();
        let Some(state) = &mut self.state else { return };
        state.sync_cursor_space();

        // winit 0.30.12's X11 Ime::create_context(window, false) sends an
        // Ime::Disabled lifecycle notification even for an unmapped window.
        // It carries no input. Recognize it at the shared capture entry;
        // the guards below still own every keyboard/pointer/IME input event.
        // https://github.com/rust-windowing/winit/blob/v0.30.12/src/platform_impl/linux/x11/ime/mod.rs
        #[cfg(target_os = "linux")]
        if (shell_capture_active || tactical_capture_active)
            && state.platform.window.is_visible() == Some(false)
            && !state.platform.window.has_focus()
            && matches!(&event, WindowEvent::Ime(winit::event::Ime::Disabled))
        {
            return;
        }

        // This checkpoint is driven entirely from `about_to_wait`. Focused
        // input would contaminate its hidden, no-input production oracle.
        if tactical_capture_active {
            let session = self
                .tactical_capture
                .as_mut()
                .expect("tactical capture session exists");
            match event {
                WindowEvent::CloseRequested => {
                    session.fail("tactical-capture window closed before bundle completion");
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    let expected =
                        PhysicalSize::new(session.request().width(), session.request().height());
                    if size != expected {
                        session.fail(format!(
                            "tactical-capture surface resized to {}x{}, expected {}x{}",
                            size.width, size.height, expected.width, expected.height
                        ));
                        event_loop.exit();
                    } else {
                        Self::resize_surface_for_window_size(state, size);
                    }
                }
                WindowEvent::Focused(true) => {
                    session.record_focus_violation();
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
                    session.record_input_violation("window");
                    event_loop.exit();
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
        // player input until the post-present deadline has elapsed.
        if state
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

        // Key edges own the modifier views on every screen, including utility
        // cards that consume command input. A release there must not stay held
        // after returning to the shell or entering another match.
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && let PhysicalKey::Code(code) = event.physical_key
        {
            let paused = state.match_state.paused();
            crate::app::input::hotkeys::record_modifier_key(
                &mut state.platform.held_modifier_keys,
                &mut state.platform.live_modifiers,
                &mut state.match_state.input.hotkey_modifiers,
                code,
                event.state.is_pressed(),
                paused,
            );
        }
        if crate::app::frontend::status_screen::handle_event(state, &event, event_loop) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                log::info!("Close requested");
                // Active YR treats a terminated launcher Options pump as an
                // always-apply final result: Apply -> destroy -> one write.
                // Complete that transaction before unrelated shell teardown.
                crate::app::input::keyboard::prepare_terminal_exit(state);
                Self::close_launcher_options_terminal(state);
                if Self::native_skirmish_shell_active(state) {
                    // Pump/quit exits write the last durable snapshot after
                    // teardown, without a fresh control pack or RNG draw.
                    Self::teardown_skirmish_shell_for_start(state);
                    state.frontend.multiplayer_shell_runtime.persist_snapshot();
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
            WindowEvent::Focused(active) => {
                // The OS drops the cursor clip on alt-tab, so re-apply the
                // player's confinement choice whenever the window returns.
                if active {
                    crate::app::input::cursor::apply_cursor_confinement(state);
                }
                if !active {
                    crate::app::input::cursor::release_cursor_confinement(state);
                    Self::release_in_game_pointer_state(state);
                    if let Some(browser) = state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .as_mut()
                    {
                        browser.pressed_control = None;
                        browser.scroll_repeat_at = None;
                        browser.last_list_press = None;
                    }
                    if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                        dialog.reset_interaction();
                    }
                    if let Some(dialog) = state.frontend.network_options.as_mut() {
                        dialog.cancel_pointer();
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
                    }
                    if let Some(campaign) = state.frontend.campaign.as_mut() {
                        campaign.slider_release();
                    }
                }
                Self::set_window_active(state, active);
            }
            WindowEvent::Occluded(occluded) => {
                // Fully covered on the backends that report it. Windows does
                // not; see the `Resized` arm above.
                Self::set_window_hidden(state, occluded);
            }
            WindowEvent::CursorEntered { .. } => {
                // The pointer is back over the drawable region: mouse.shp owns it
                // again and the OS arrow goes. Redraw so the cursor decision and the
                // clip both follow the edge.
                state.platform.pointer_in_client = true;
                state.platform.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
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
                // cursor lock toggle: flip the window cursor
                // clip so the pointer cannot leave the game (and back). Works
                // on every screen, like the rest of the lock feature.
                if let PhysicalKey::Code(KeyCode::ScrollLock) = event.physical_key
                    && event.state.is_pressed()
                    && !event.repeat
                {
                    crate::app::input::cursor::toggle_cursor_confinement(state);
                    state.platform.window.request_redraw();
                    return;
                }
                // VERA's FPS readout toggle. Like the cursor lock it works on
                // every screen and is consumed before retail key dispatch.
                if let PhysicalKey::Code(KeyCode::KeyF) = event.physical_key
                    && state.platform.live_modifiers.control_key()
                    && state.platform.live_modifiers.alt_key()
                {
                    if event.state.is_pressed() && !event.repeat {
                        let shown = state.renderer.fps_overlay.toggle();
                        if let Some(dialog) = state.frontend.options_dialog.as_mut() {
                            dialog.sync_fps_counter(shown);
                        }
                        log::info!("FPS overlay: {}", if shown { "shown" } else { "hidden" });
                        state.platform.window.request_redraw();
                    }
                    return;
                }
                // The wait loop pumps 0xEA without IsDialogMessage and polls
                // no game keys while it is open (0x006497EC).
                if state.match_state.network_wait.dialog_open() {
                    if !event.state.is_pressed()
                        && let PhysicalKey::Code(code) = event.physical_key
                    {
                        state.match_state.input.keys_held.remove(&code);
                    }
                    return;
                }
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::key(state, &event);
                    return;
                }
                if state.frontend.network_options.is_some() {
                    crate::app::input::network_options::key(state, &event);
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.lan_lobby()
                {
                    crate::app::shell_lan::key(state, &event);
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.network_setup()
                {
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
                                Self::cancel_choose_map_eject_prompt(state);
                            }
                        }
                    }
                    return;
                }
                if Self::native_skirmish_shell_active(state)
                    && state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .is_some()
                {
                    if !crate::app::frontend::shell_transition::blocks_shell_input(state)
                        && event.state.is_pressed()
                    {
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

                    if crate::app::frontend::shell_transition::blocks_shell_input(state) {
                        return;
                    }

                    if state.frontend.screen == GameScreen::MainMenu
                        && state.frontend.fullscreen_movie.is_some()
                    {
                        Self::fullscreen_movie_key(state, code, event.state.is_pressed());
                        return;
                    }
                    if state.frontend.screen == GameScreen::MainMenu
                        && state.frontend.credits_roll.is_some()
                    {
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

                    // A main-menu modal dialog (exit confirm, options, movies,
                    // campaign select) takes ESC first: close it and stay,
                    // never propagating to the shell-close handlers below.
                    // The exit-confirm modal routes through the controller
                    // (on_key → LIFO pop, D-B3); the retained child dialogs are
                    // not on the stack and keep the direct close.
                    if Self::main_menu_dialog_open(state) {
                        if is_escape {
                            if state.frontend.shell_message_box.is_some() {
                                if !Self::route_shell_message_box_key(state, ShellKey::Escape) {
                                    // Defensive: on_key only fails with an
                                    // empty route — still answer IDCANCEL.
                                    Self::resolve_shell_message_box(
                                        state,
                                        crate::ui::shell::modal::ModalResult::MessageBox(1),
                                    );
                                    state.platform.window.request_redraw();
                                }
                            } else if launcher_options_consumes_escape(
                                &state.frontend.options_dialog,
                                is_escape,
                            ) {
                                // `0xD5` ignores IDCANCEL=2. Preserve the same
                                // dialog instance and emit no Apply/write/event.
                                state.platform.window.request_redraw();
                            } else {
                                Self::close_main_menu_dialogs(state);
                                state.platform.window.request_redraw();
                            }
                        }
                        return;
                    }

                    if Self::native_skirmish_shell_active(state)
                        && event.state.is_pressed()
                        && !event.repeat
                        && Self::shell_key_for_code(code)
                            .is_some_and(|key| Self::route_validation_modal_key(state, key))
                    {
                        return;
                    }

                    if Self::native_skirmish_shell_active(state)
                        && event.state.is_pressed()
                        && !event.repeat
                        && matches!(
                            code,
                            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
                        )
                        && Self::cancel_choose_map_eject_prompt(state)
                    {
                        return;
                    }
                    if Self::native_skirmish_shell_active(state) && is_escape {
                        if state
                            .frontend
                            .skirmish_shell_state
                            .choose_map_modal
                            .is_some()
                        {
                            // The chooser's loop (`0x007759E0`) runs without
                            // IsDialogMessage: Escape does nothing.
                            state.platform.window.request_redraw();
                            return;
                        }
                        Self::handle_skirmish_shell_action(
                            state,
                            crate::ui::skirmish_shell::SkirmishShellAction::BackOrExit,
                        );
                        state.platform.window.request_redraw();
                        return;
                    }

                    if Self::wol_active(state) {
                        Self::handle_wol_key(
                            state,
                            Some(code),
                            event.state.is_pressed(),
                            event.text.as_deref(),
                        );
                        state.platform.window.request_redraw();
                        return;
                    }

                    if Self::load_saved_game_active(state) {
                        // The message box takes Enter/Escape; the page
                        // ignores IDOK/IDCANCEL (0x00558A30).
                        if event.state.is_pressed() {
                            Self::handle_load_saved_game_key(state, code);
                        }
                        state.platform.window.request_redraw();
                        return;
                    }

                    if (Self::menu_page_active(state)
                        || Self::movie_list_active(state)
                        || Self::campaign_active(state))
                        && is_escape
                    {
                        // IsDialogMessageA turns Escape into IDCANCEL (id 2),
                        // which the 0x100/0x101/0x129/0x94 procs and the common
                        // handler 0x00622B50 ignore: the dialog stays open.
                        return;
                    }

                    if in_game
                        && matches!(
                            state.match_state.match_presentation.in_game_menu,
                            crate::ui::pause_menu::InGameMenuState::SavedGame(_)
                        )
                    {
                        if event.state.is_pressed() {
                            Self::saved_game_key(state, Some(code), event.text.as_deref());
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if in_game
                        && state.match_state.match_presentation.in_game_menu
                            == crate::ui::pause_menu::InGameMenuState::Page
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
                    if in_game
                        && matches!(
                            state.match_state.match_presentation.in_game_menu,
                            crate::ui::pause_menu::InGameMenuState::Sound
                                | crate::ui::pause_menu::InGameMenuState::Diplomacy
                                | crate::ui::pause_menu::InGameMenuState::MissionBriefing
                        )
                    {
                        // B8 has no IDOK/IDCANCEL close command. Neither has
                        // 0x73: its proc keys commands by GetDlgCtrlID(lParam)
                        // (0x006581A7), and Return 0x423 is owner-drawn, so
                        // Enter and Escape name no child it handles.
                        return;
                    }
                    if in_game
                        && state.match_state.match_presentation.in_game_menu
                            == crate::ui::pause_menu::InGameMenuState::AbortConfirm
                    {
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
                        paused_at_event,
                        &event.logical_key,
                    ) {
                        return;
                    }

                    if Self::native_skirmish_shell_active(state)
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
                state.set_cursor_from_window(position.x as f32, position.y as f32);
                // Shared tooltip service: every move restarts the show delay
                // and hides a visible tip (study S1).
                crate::app::input::tooltips::on_mouse_move(state);
                if crate::app::frontend::shell_transition::blocks_shell_input(state) {
                    return;
                }
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::cursor_moved(state);
                    return;
                }
                if state.frontend.network_options.is_some() {
                    crate::app::input::network_options::cursor_moved(state);
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.lan_lobby()
                {
                    crate::app::shell_lan::cursor_moved(state);
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.network_setup()
                {
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
                    return;
                }
                if Self::native_launcher_options_active(state) {
                    Self::handle_launcher_options_mouse(state, None);
                    return;
                }
                if state.frontend.screen == GameScreen::InGame {
                    dispatch::handle_cursor_moved_in_game(state);
                }
                if Self::native_skirmish_shell_active(state) {
                    Self::handle_skirmish_shell_mouse_move(state);
                }
                if Self::menu_page_active(state) {
                    Self::handle_menu_page_mouse_move(state);
                }
                if Self::movie_list_active(state) {
                    Self::handle_movie_list_mouse_move(state);
                }
                if Self::campaign_active(state) {
                    Self::handle_campaign_mouse_move(state);
                }
                if Self::load_saved_game_active(state) {
                    Self::handle_load_saved_game_mouse_move(state);
                }
                if Self::wol_active(state) {
                    Self::handle_wol_mouse_move(state);
                }
                if Self::score_shell_active(state) {
                    Self::handle_score_shell_mouse_move(state);
                }
                if Self::campaign_score_shell_active(state) {
                    Self::handle_campaign_score_mouse_move(state);
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.main_menu_shell_error.is_none()
                    && !Self::menu_page_active(state)
                    && !Self::movie_list_active(state)
                    && !Self::campaign_active(state)
                    && !Self::load_saved_game_active(state)
                    && !Self::wol_active(state)
                    && state.frontend.fullscreen_movie.is_none()
                    && state.frontend.credits_roll.is_none()
                    && !Self::native_skirmish_shell_active(state)
                    // While the SHP quit-confirm modal owns the controller, the menu
                    // move handler must not re-activate 0xE2 and reset the gesture.
                    && state.frontend.shell_message_box.is_none()
                {
                    Self::handle_main_menu_shell_mouse_move(state);
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                // Any button press/release kills a visible tooltip + pending
                // timer (all buttons incl. middle — study S1).
                crate::app::input::tooltips::on_button_event(state);
                if crate::app::frontend::shell_transition::blocks_shell_input(state) {
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu {
                    // Play_Movie reads and ignores mouse buttons; Show_Credits
                    // queues them as ordinary Keyboard entries.
                    if state.frontend.fullscreen_movie.is_some() {
                        return;
                    }
                    if state.frontend.credits_roll.is_some() {
                        Self::credits_roll_input(
                            state,
                            crate::app::frontend::credits_roll::CreditsInput::Other,
                        );
                        return;
                    }
                    if Self::movie_list_active(state) {
                        if button == MouseButton::Left {
                            if btn_state.is_pressed() {
                                Self::handle_movie_list_mouse_down(state);
                            } else {
                                Self::handle_movie_list_mouse_up(state);
                            }
                        }
                        return;
                    }
                    if Self::campaign_active(state) {
                        if button == MouseButton::Left {
                            if btn_state.is_pressed() {
                                Self::handle_campaign_mouse_down(state);
                            } else {
                                Self::handle_campaign_mouse_up(state);
                            }
                        }
                        return;
                    }
                    if Self::load_saved_game_active(state) {
                        if button == MouseButton::Left {
                            Self::handle_load_saved_game_mouse(state, btn_state.is_pressed());
                            state.platform.window.request_redraw();
                        }
                        return;
                    }
                    if Self::wol_active(state) {
                        if button == MouseButton::Left {
                            if btn_state.is_pressed() {
                                Self::handle_wol_mouse_down(state);
                            } else {
                                Self::handle_wol_mouse_up(state);
                            }
                            state.platform.window.request_redraw();
                        }
                        return;
                    }
                }
                // While a main-menu modal dialog is open, route the click to the
                // SHP quit-confirm modal's OK/Cancel hit-test on the normal shell
                // path; native child dialogs own their input routes below.
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::mouse(state, button, btn_state.is_pressed());
                    return;
                }
                if state.frontend.network_options.is_some() {
                    crate::app::input::network_options::mouse(
                        state,
                        button,
                        btn_state.is_pressed(),
                    );
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.lan_lobby()
                {
                    crate::app::shell_lan::mouse(state, button, btn_state.is_pressed());
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.shell_route.network_setup()
                {
                    if state
                        .frontend
                        .skirmish_shell_state
                        .choose_map_modal
                        .is_some()
                    {
                        if button == MouseButton::Left {
                            if btn_state.is_pressed() {
                                Self::handle_skirmish_shell_mouse_down(state);
                            } else {
                                Self::handle_skirmish_shell_mouse_up(state);
                            }
                        }
                    } else {
                        crate::app::input::multiplayer_setup::mouse(
                            state,
                            button,
                            btn_state.is_pressed(),
                        );
                    }
                    return;
                }
                if Self::main_menu_dialog_open(state) {
                    if Self::native_launcher_options_active(state) && button == MouseButton::Left {
                        Self::handle_launcher_options_mouse(state, Some(btn_state.is_pressed()));
                        return;
                    }
                    if state.frontend.shell_message_box.is_some()
                        && state.frontend.screen == GameScreen::MainMenu
                        && state.frontend.main_menu_shell_error.is_none()
                        && button == MouseButton::Left
                    {
                        if btn_state.is_pressed() {
                            Self::handle_shell_message_box_mouse_down(state);
                        } else {
                            Self::handle_shell_message_box_mouse_up(state);
                        }
                    }
                    return;
                }
                if Self::campaign_score_shell_active(state) {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_campaign_score_mouse_down(state);
                        } else {
                            Self::handle_campaign_score_mouse_up(state);
                        }
                    }
                } else if Self::score_shell_active(state) {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_score_shell_mouse_down(state);
                        } else {
                            Self::handle_score_shell_mouse_up(state);
                        }
                    }
                } else if Self::native_skirmish_shell_active(state) {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_skirmish_shell_mouse_down(state);
                        } else {
                            Self::handle_skirmish_shell_mouse_up(state);
                        }
                    }
                } else if Self::menu_page_active(state) {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_menu_page_mouse_down(state);
                        } else {
                            Self::handle_menu_page_mouse_up(state);
                        }
                    }
                } else if state.frontend.screen == GameScreen::MainMenu
                    && state.frontend.main_menu_shell_error.is_none()
                {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_main_menu_shell_mouse_down(state);
                        } else {
                            Self::handle_main_menu_shell_mouse_up(state, event_loop);
                        }
                    }
                } else if state.frontend.screen == GameScreen::InGame {
                    dispatch::handle_mouse_input(state, button, btn_state);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y as f32 / 30.0).clamp(-3.0, 3.0),
                };
                if crate::app::frontend::shell_transition::blocks_shell_input(state) {
                    return;
                }
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::wheel(state, lines);
                    state.platform.window.request_redraw();
                    return;
                }
                if state.frontend.network_options.is_some() {
                    // ComboDropWin consumes no mouse-wheel selection/scroll.
                    return;
                }
                if state.frontend.screen == GameScreen::MainMenu
                    && Self::native_skirmish_shell_active(state)
                    && Self::handle_skirmish_shell_mouse_wheel(state, lines)
                {
                    state.platform.window.request_redraw();
                    return;
                }
                if state.frontend.screen == GameScreen::InGame
                    && !state.match_state.paused()
                    && !state.match_state.network_wait.dialog_open()
                {
                    // Every wheel notch scrolls the active build strip by one
                    // row, wherever the cursor is. gamemd routes the wheel
                    // message straight to the SidebarUp / SidebarDown commands
                    // and has no world zoom for it to reach.
                    dispatch::sidebar_wheel_scroll(state, lines);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = Self::render_frame(state, event_loop, None, None) {
                    log::error!("Render: {:#}", err);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let shell_scroll_wake = if let Some(state) = self.state.as_mut() {
            Self::follow_window_full_screen(state);
            // cursor lock: the OS drops the clip on focus loss /
            // Windows key, so re-assert the player's choice every frame while
            // the window is focused. Presentation-only, never part of the sim.
            crate::app::input::cursor::reassert_cursor_confinement(state);
            Self::update_saved_seed_browser_scroll(state, false);
            Self::update_load_saved_game_scroll(state, false);
            [
                crate::app::input::keyboard::poll_scroll_repeat(state),
                crate::app::input::network_options::poll_scroll_repeat(state),
                crate::app::shell_lan::poll(state),
                crate::app::shell_wol::poll(state),
                crate::app::shell_network_setup::poll(state),
                crate::app::input::multiplayer_setup::poll(state),
                crate::app::input::sound::poll_scroll_repeat(state),
                Self::poll_movie_list_scroll(state),
                Self::poll_choose_map_scroll(state),
                Self::poll_campaign(state),
                Self::campaign_score_animation_deadline(state),
            ]
            .into_iter()
            .flatten()
            .min()
        } else {
            None
        };
        if let (Some(state), Some(session)) = (self.state.as_mut(), self.tactical_capture.as_mut())
        {
            if let Err(err) = Self::render_frame(state, event_loop, None, Some(&mut *session)) {
                log::error!("Tactical capture render: {err:#}");
                session.fail(format!("tactical capture render failed: {err:#}"));
                event_loop.exit();
                return;
            }
            if !session.is_finished() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(session.next_wake_deadline()));
            }
        } else if let (Some(state), Some(session)) =
            (self.state.as_mut(), self.shell_capture.as_mut())
        {
            // Windows may deliver another wait callback after exit was requested.
            // The completed one-shot bundle must not enter rendering again.
            if session.is_finished() {
                event_loop.exit();
                return;
            }
            if let Err(err) = Self::render_frame(state, event_loop, Some(&mut *session), None) {
                log::error!("Shell capture render: {err:#}");
                session.fail(format!("shell capture render failed: {err:#}"));
                event_loop.exit();
                return;
            }
            if !session.is_finished() {
                let mut deadline = session.next_wake_deadline();
                if let Some(wave_deadline) =
                    crate::app::frontend::shell_transition::main_menu_presented_wake_deadline(state)
                {
                    deadline = deadline.max(wave_deadline);
                }
                if let Some(animation_deadline) = Self::campaign_score_animation_deadline(state) {
                    deadline = deadline.min(animation_deadline);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
        } else if let Some(state) = self.state.as_mut() {
            if crate::app::frontend::shell_transition::main_menu_presented_is_poisoned(state) {
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
                    if Self::service_match_runtime(state, event_loop, true) {
                        return;
                    }
                    event_loop.set_control_flow(ControlFlow::WaitUntil(
                        Instant::now()
                            + crate::app::match_runtime::frame_pacer::HIDDEN_SERVICE_WAKE,
                    ));
                } else {
                    // Nothing on screen to keep fresh. Park the redraw loop
                    // until a window event (including the un-occlude edge)
                    // wakes it, rather than rendering frames no one can see.
                    event_loop.set_control_flow(ControlFlow::Wait);
                }
            } else {
                if let Some(deadline) = shell_scroll_wake {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
                }
                state.platform.window.request_redraw();
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            crate::app::match_runtime::sim_tick::flush_replay_log(state);
        }
        log::logger().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
