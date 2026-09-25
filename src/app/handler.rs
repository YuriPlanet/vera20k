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

trait ShellWindowModeOperations {
    fn shell_client_size(&self) -> PhysicalSize<u32>;
    fn set_resizable(&mut self, resizable: bool);
    fn inner_size(&self) -> PhysicalSize<u32>;
    fn request_inner_size(&mut self, size: PhysicalSize<u32>) -> Option<PhysicalSize<u32>>;
    fn resize_surface_for_window_size(&mut self, size: PhysicalSize<u32>);
    fn request_redraw(&mut self);
}

struct PlatformShellWindowModeOperations<'a> {
    state: &'a mut AppState,
}

impl ShellWindowModeOperations for PlatformShellWindowModeOperations<'_> {
    fn shell_client_size(&self) -> PhysicalSize<u32> {
        self.state.platform.shell_client_size
    }

    fn set_resizable(&mut self, resizable: bool) {
        self.state.platform.window.set_resizable(resizable);
    }

    fn inner_size(&self) -> PhysicalSize<u32> {
        self.state.platform.window.inner_size()
    }

    fn request_inner_size(&mut self, size: PhysicalSize<u32>) -> Option<PhysicalSize<u32>> {
        self.state.platform.window.request_inner_size(size)
    }

    fn resize_surface_for_window_size(&mut self, size: PhysicalSize<u32>) {
        App::resize_surface_for_window_size(self.state, size);
    }

    fn request_redraw(&mut self) {
        self.state.platform.window.request_redraw();
    }
}

fn enter_shell_window_mode_with_operations(operations: &mut impl ShellWindowModeOperations) {
    let target = operations.shell_client_size();
    apply_window_mode(operations, target, false);
}

fn apply_window_mode(
    operations: &mut impl ShellWindowModeOperations,
    target: PhysicalSize<u32>,
    resizable: bool,
) {
    operations.set_resizable(resizable);
    if operations.inner_size() == target {
        return;
    }
    // winit 0.30.12 Windows calls SetWindowPos but returns None. Read the
    // actual client size before installing size-dependent tactical resources;
    // waiting solely for its queued Resized event would use the old shell size.
    let applied_size = operations
        .request_inner_size(target)
        .unwrap_or_else(|| operations.inner_size());
    if applied_size.width != 0 && applied_size.height != 0 {
        operations.resize_surface_for_window_size(applied_size);
    }
    operations.request_redraw();
}

/// Active YR's launcher parent ignores IDCANCEL. Taking only an immutable
/// reference makes the no-close/no-replacement guarantee explicit at the
/// keyboard routing seam.
fn launcher_options_consumes_escape<T>(dialog: &Option<T>, is_escape: bool) -> bool {
    is_escape && dialog.is_some()
}

impl App {
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
        crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    }

    pub(crate) fn enter_shell_window_mode(state: &mut AppState) {
        let mut operations = PlatformShellWindowModeOperations { state };
        enter_shell_window_mode_with_operations(&mut operations);
        log::info!(
            "Frontend surface {}x{}",
            state.renderer.gpu.config.width, state.renderer.gpu.config.height,
        );
    }

    pub(crate) fn enter_game_window_mode(state: &mut AppState) {
        // Scenario start 00683DBB..00683DF3 applies the game pair; shell return
        // 006857AE restores the independent frontend pair. Profile stays sole
        // authority, so launcher resolution edits take effect on the next match.
        let size = state.persistence.options_profile.game_screen_size();
        let target = state
            .platform
            .capture_client_size
            .unwrap_or_else(|| PhysicalSize::new(size.width, size.height));
        // Explicit low-mode startup is not a permanent return preference:
        // 006857AE restores Options' independent frontend pair after a match.
        let shell = crate::app::persistence::options_profile::RETAIL_SHELL_SIZE;
        state.platform.shell_client_size = state
            .platform
            .capture_client_size
            .unwrap_or_else(|| PhysicalSize::new(shell.width, shell.height));
        let mut operations = PlatformShellWindowModeOperations { state };
        apply_window_mode(&mut operations, target, true);
        log::info!(
            "Match surface {}x{} (requested {}x{})",
            state.renderer.gpu.config.width, state.renderer.gpu.config.height,
            target.width, target.height,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingShellWindowModeOperations {
        shell_client_size: PhysicalSize<u32>,
        current_size: PhysicalSize<u32>,
        applied_size: Option<PhysicalSize<u32>>,
        resizable_values: Vec<bool>,
        requested_sizes: Vec<PhysicalSize<u32>>,
        resized_surfaces: Vec<PhysicalSize<u32>>,
        redraw_requests: usize,
    }

    impl RecordingShellWindowModeOperations {
        fn new(shell_client_size: PhysicalSize<u32>, current_size: PhysicalSize<u32>) -> Self {
            Self {
                shell_client_size,
                current_size,
                applied_size: Some(shell_client_size),
                resizable_values: Vec::new(),
                requested_sizes: Vec::new(),
                resized_surfaces: Vec::new(),
                redraw_requests: 0,
            }
        }
    }

    impl ShellWindowModeOperations for RecordingShellWindowModeOperations {
        fn shell_client_size(&self) -> PhysicalSize<u32> {
            self.shell_client_size
        }

        fn set_resizable(&mut self, resizable: bool) {
            self.resizable_values.push(resizable);
        }

        fn inner_size(&self) -> PhysicalSize<u32> {
            self.current_size
        }

        fn request_inner_size(&mut self, size: PhysicalSize<u32>) -> Option<PhysicalSize<u32>> {
            self.requested_sizes.push(size);
            if self.applied_size.is_none() {
                self.current_size = size;
            }
            self.applied_size
        }

        fn resize_surface_for_window_size(&mut self, size: PhysicalSize<u32>) {
            self.resized_surfaces.push(size);
        }

        fn request_redraw(&mut self) {
            self.redraw_requests += 1;
        }
    }

    #[test]
    fn shell_window_mode_requests_retained_target_only_when_size_differs() {
        let target = PhysicalSize::new(640, 480);
        let mut unequal =
            RecordingShellWindowModeOperations::new(target, PhysicalSize::new(800, 600));

        enter_shell_window_mode_with_operations(&mut unequal);

        assert_eq!(unequal.resizable_values, [false]);
        assert_eq!(unequal.requested_sizes, [target]);
        assert_eq!(unequal.resized_surfaces, [target]);
        assert_eq!(unequal.redraw_requests, 1);

        let mut equal = RecordingShellWindowModeOperations::new(target, target);

        enter_shell_window_mode_with_operations(&mut equal);

        assert_eq!(equal.resizable_values, [false]);
        assert!(equal.requested_sizes.is_empty());
        assert!(equal.resized_surfaces.is_empty());
        assert_eq!(equal.redraw_requests, 0);
    }

    #[test]
    fn windows_none_resize_result_reads_back_the_applied_client_size() {
        let shell = PhysicalSize::new(800, 600);
        let game = PhysicalSize::new(640, 480);
        let mut operations = RecordingShellWindowModeOperations::new(shell, shell);
        operations.applied_size = None;
        apply_window_mode(&mut operations, game, true);
        assert_eq!(operations.resized_surfaces, [game]);
        enter_shell_window_mode_with_operations(&mut operations);
        assert_eq!(operations.requested_sizes, [game, shell]);
        assert_eq!(operations.resized_surfaces, [game, shell]);
    }

    #[test]
    fn game_mode_applies_the_returned_physical_size_before_resource_installation() {
        let shell = PhysicalSize::new(800, 600);
        let game = PhysicalSize::new(1024, 768);
        let applied = PhysicalSize::new(1000, 740);
        let mut operations = RecordingShellWindowModeOperations::new(shell, shell);
        operations.applied_size = Some(applied);
        apply_window_mode(&mut operations, game, true);
        assert_eq!(operations.resizable_values, [true]);
        assert_eq!(operations.requested_sizes, [game]);
        assert_eq!(operations.resized_surfaces, [applied]);
        assert_eq!(operations.redraw_requests, 1);
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
        match Self::initialize(event_loop, capture_dimensions, self.startup_options) {
            Ok(mut state) => {
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
                self.state = Some(state);
                log::info!("Initialization complete — showing main menu");
            }
            Err(err) => {
                log::error!("Failed to initialize: {:#}", err);
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

        // Native does not expose menu/game input while its process-start splash
        // owns the display. Keep close/resize/redraw operational, but discard
        // player input until the post-present deadline has elapsed.
        if state
            .frontend.startup_splash
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

        // Always let egui see the event first for input handling.
        let egui_response: egui_winit::EventResponse =
            state.renderer.egui.on_window_event(&state.platform.window, &event);
        if crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(state) {
            state.renderer.egui.discard_pending_input(&state.platform.window);
        }

        // In InGame mode, egui only renders non-interactive overlays
        // (mission banner). The custom sidebar handles its own hit-testing.
        // Ignore egui's `consumed` flag in-game to avoid stale UI state
        // from the Loading screen blocking mouse/keyboard input.
        // Exception: when paused or save/load panel is open, egui renders
        // interactive content.
        let egui_consumed: bool = egui_response.consumed
            && !crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(state)
            && (state.frontend.screen != GameScreen::InGame || state.match_state.paused || state.match_state.match_presentation.show_save_load_panel);

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
                    state.frontend.offline_skirmish_runtime.persist_snapshot();
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
                }
            }
            WindowEvent::Focused(active) => {
                if !active {
                    // Losing focus takes the mouse capture away, and gamemd
                    // handles that explicitly: its capture-changed case drops
                    // the button-held byte and tears the band rectangle down.
                    // Without this the right-drag pan keeps applying its
                    // anchor-relative step every frame and edge scroll stays
                    // inhibited until the player right-clicks again.
                    state.match_state.input.tactical_mouse = Default::default();
                    state.match_state.input.selection_state.cancel_drag();
                    state.match_state.input.minimap_dragging = false;
                    state.match_state.match_presentation.in_game_options.dragging_slider = None;
                    state.match_state.match_presentation.in_game_options.buttons = Default::default();
                    state.match_state.match_presentation.pause_menu_interaction = Default::default();
        state.match_state.match_presentation.abort_buttons = Default::default();
        if let Some(dialog)=state.match_state.match_presentation.sound_dialog.as_mut() {dialog.reset_interaction();}
                    if let Some(browser) = state.match_state.match_presentation.saved_game_browser.as_mut() {
                        browser.pressed_control = None;
                        browser.scroll_repeat_at = None;
                        browser.last_list_press = None;
                    }
                    if let Some(browser) = state.frontend.skirmish_shell_state.saved_seed_browser.as_mut() {
                        browser.pressed_control = None;
                        browser.scroll_repeat_at = None;
                        browser.last_list_press = None;
                    }
                    if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() { dialog.reset_interaction(); }
                    if let Some(dialog) = state.frontend.options_dialog.as_mut() {
                        dialog.shell_cancel_pointer_gesture();
                    }
                }
                Self::set_window_active(state, active);
            }
            WindowEvent::Occluded(occluded) => {
                // Fully covered on the backends that report it. Windows does
                // not; see the `Resized` arm above.
                Self::set_window_hidden(state, occluded);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // Native's paused input capture admits Escape only and does not
                // mutate the recorded keyboard state for other input.
                crate::app::input::hotkeys::record_modifier_event(
                    &mut state.platform.live_modifiers,
                    &mut state.match_state.input.hotkey_modifiers,
                    modifiers.state(), state.match_state.paused,
                );
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::key(state, &event);
                    return;
                }
                if Self::native_skirmish_shell_active(state)
                    && state.frontend.skirmish_shell_state.saved_seed_browser.is_some()
                {
                    if !crate::app::frontend::shell_transition::blocks_shell_input(state)
                        && event.state.is_pressed()
                    {
                        let code = match event.physical_key { PhysicalKey::Code(code) => Some(code), _ => None };
                        Self::handle_saved_seed_browser_key(state, code, event.text.as_deref());
                        state.platform.window.request_redraw();
                    }
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    // ESC always reaches the handler when in-game (even when paused)
                    // so the player can toggle pause regardless of egui focus.
                    let is_escape: bool =
                        code == KeyCode::Escape && event.state.is_pressed() && !event.repeat;
                    let in_game: bool = state.frontend.screen == GameScreen::InGame;
                    let paused_at_event = in_game && state.match_state.paused;

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
                    // (on_key → LIFO pop, D-B3); the egui-only dialogs are
                    // not on the stack and keep the direct close.
                    if Self::main_menu_dialog_open(state) {
                        if is_escape {
                            if state.frontend.exit_confirm_modal.is_some() {
                                if !Self::route_exit_confirm_modal_key(state, ShellKey::Escape) {
                                    // Defensive: on_key only fails with an
                                    // empty route — still close consistently.
                                    Self::close_exit_confirm_modal_from_controller(state);
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

                    if Self::native_skirmish_shell_active(state) && is_escape {
                        if state.frontend.skirmish_shell_state.choose_map_modal.is_some() {
                            // Native chooser `0x6B` has no verified Escape
                            // dismissal. Consume the key without applying the
                            // Cancel transaction or changing its selection.
                            state.platform.window.request_redraw();
                            return;
                        }
                        Self::handle_skirmish_shell_action(
                            state,
                            crate::ui::skirmish_shell::SkirmishShellAction::BackOrExit,
                            event_loop,
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

                    if in_game && matches!(state.match_state.match_presentation.in_game_menu, crate::ui::pause_menu::InGameMenuState::SavedGame(_)) {
                        if event.state.is_pressed() {
                            Self::saved_game_key(state, Some(code), event.text.as_deref());
                        }
                        state.platform.window.request_redraw();
                        return;
                    }
                    if in_game && state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::Sound {
                        // B8 has no IDOK/IDCANCEL close command.
                        return;
                    }
                    if in_game && state.match_state.match_presentation.in_game_menu == crate::ui::pause_menu::InGameMenuState::AbortConfirm {
                        // B6 accepts default IDOK1 as Resume (4F1A59..4F1A65).
                        // Owner buttons reject focus through610CA0:6118BF..DE;
                        // their resource has no WS_TABSTOP. Do not invent a
                        // focused-button Return/Space route. IDCANCEL is ignored.
                        if event.state.is_pressed() && !event.repeat && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter) {
                            crate::app::input::abort::activate(state, crate::ui::shell::abort::AbortButton::Resume);
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

                    // The in-scenario modal machine owns Escape: it opens the
                    // in-game menu, backs Options out to its parent menu, and
                    // leaves B5/B6 open. Escape's in-world
                    // cancel duties (placement/targeting, repair/sell) still
                    // run first — see `in_game_menu_owns_escape`.
                    if in_game && is_escape && Self::in_game_menu_owns_escape(state) {
                        Self::route_in_game_menu_escape(state);
                        state.platform.window.request_redraw();
                        return;
                    }

                    let key_without_modifiers = event.key_without_modifiers();
                    let binding_key = crate::app::input::hotkeys::binding_logical_key(
                        &event.logical_key,
                        &key_without_modifiers,
                        event.location,
                    );
                    let hotkey_resolution = state.match_state.input.hotkey_bindings.resolve_event(
                        binding_key,
                        event.location,
                        state.match_state.input.hotkey_modifiers,
                    );
                    if in_game && (is_escape || !egui_consumed) {
                        let type_select_consumed = dispatch::handle_type_select_key_edge(
                            state,
                            hotkey_resolution,
                            code,
                            event.state,
                            event.repeat,
                        );
                        if event.state.is_pressed() && !event.repeat && !type_select_consumed {
                            dispatch::handle_hotkey_pressed(state, hotkey_resolution, code);
                        }
                    }
                    // A key received by the paused capture changes no held-key
                    // state, including the Escape press that closes it.
                    if in_game && !paused_at_event && !egui_consumed {
                        if event.state.is_pressed() {
                            if let Some(scroll_key) =
                                crate::app::input::hotkeys::fallback_scroll_key(hotkey_resolution)
                            {
                                state.match_state.input.keys_held.insert(scroll_key);
                            } else if crate::app::input::hotkeys::physical_scroll_key(code).is_none() {
                                state.match_state.input.keys_held.insert(code);
                            }
                        } else {
                            // A release always clears a previously admitted
                            // scroll flag, even if NumLock or bindings changed
                            // while the key was held.
                            state.match_state.input.keys_held.remove(&code);
                            if let Some(scroll_key) =
                                crate::app::input::hotkeys::fallback_scroll_key(hotkey_resolution)
                                    .or_else(|| {
                                        crate::app::input::hotkeys::physical_scroll_key(code)
                                    })
                            {
                                state.match_state.input.keys_held.remove(&scroll_key);
                            }
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // When upscaling, remap window coordinates to render-target coordinates.
                let use_render_source_coords = state.renderer.upscale_pass.is_some()
                    && (state.frontend.screen == GameScreen::InGame
                        || state.frontend.screen == GameScreen::SpawnPick);
                let (sx, sy) = if use_render_source_coords {
                    (
                        state.render_width() as f32 / state.renderer.gpu.config.width as f32,
                        state.render_height() as f32 / state.renderer.gpu.config.height as f32,
                    )
                } else {
                    (1.0, 1.0)
                };
                state.match_state.input.cursor_x = position.x as f32 * sx;
                state.match_state.input.cursor_y = position.y as f32 * sy;
                // Keep OS cursor hidden whenever the software cursor is active.
                if state.use_software_cursor() {
                    state.platform.window.set_cursor_visible(false);
                }
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
                if Self::native_launcher_options_active(state) {
                    Self::handle_launcher_options_mouse(state, None);
                    return;
                }
                if !egui_consumed
                    && (state.frontend.screen == GameScreen::InGame || state.frontend.screen == GameScreen::SpawnPick)
                {
                    dispatch::handle_cursor_moved_in_game(state);
                }
                if !egui_consumed && Self::native_skirmish_shell_active(state) {
                    Self::handle_skirmish_shell_mouse_move(state);
                }
                if !egui_consumed && Self::menu_page_active(state) {
                    Self::handle_menu_page_mouse_move(state);
                }
                if !egui_consumed && Self::movie_list_active(state) {
                    Self::handle_movie_list_mouse_move(state);
                }
                if !egui_consumed && Self::campaign_active(state) {
                    Self::handle_campaign_mouse_move(state);
                }
                if !egui_consumed && Self::load_saved_game_active(state) {
                    Self::handle_load_saved_game_mouse_move(state);
                }
                if Self::score_shell_active(state) {
                    Self::handle_score_shell_mouse_move(state);
                }
                if !egui_consumed
                    && state.frontend.screen == GameScreen::MainMenu
                    && !state.frontend.main_menu_shell_failed
                    && !Self::menu_page_active(state)
                    && !Self::movie_list_active(state)
                    && !Self::campaign_active(state)
                    && !Self::load_saved_game_active(state)
                    && state.frontend.fullscreen_movie.is_none()
                    && state.frontend.credits_roll.is_none()
                    && !Self::native_skirmish_shell_active(state)
                    // While the SHP quit-confirm modal owns the controller, the menu
                    // move handler must not re-activate 0xE2 and reset the gesture.
                    && state.frontend.exit_confirm_modal.is_none()
                {
                    Self::handle_main_menu_shell_mouse_move(state);
                }
            }
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                // Keep OS cursor hidden on click events (not just CursorMoved).
                // Without this, rapid clicks without mouse movement let the OS
                // cursor flash visible between WM_SETCURSOR and the next render.
                if state.use_software_cursor() {
                    state.platform.window.set_cursor_visible(false);
                }
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
                }
                // While a main-menu modal dialog is open, route the click to the
                // SHP quit-confirm modal's OK/Cancel hit-test on the normal shell
                // path; the egui fallback and the other egui dialogs (options/movies/
                // campaign) were already handled by egui above.
                if state.frontend.keyboard_dialog.is_some() {
                    crate::app::input::keyboard::mouse(state, button, btn_state.is_pressed());
                    return;
                }
                if Self::main_menu_dialog_open(state) {
                    if Self::native_launcher_options_active(state) && button == MouseButton::Left {
                        Self::handle_launcher_options_mouse(state, Some(btn_state.is_pressed()));
                        return;
                    }
                    if state.frontend.exit_confirm_modal.is_some()
                        && state.frontend.screen == GameScreen::MainMenu
                        && !state.frontend.main_menu_shell_failed
                        && button == MouseButton::Left
                    {
                        if btn_state.is_pressed() {
                            Self::handle_exit_confirm_modal_mouse_down(state);
                        } else {
                            Self::handle_exit_confirm_modal_mouse_up(state);
                        }
                    }
                    return;
                }
                if Self::score_shell_active(state) {
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
                            Self::handle_skirmish_shell_mouse_up(state, event_loop);
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
                    && !state.frontend.main_menu_shell_failed
                    && !egui_consumed
                {
                    if button == MouseButton::Left {
                        if btn_state.is_pressed() {
                            Self::handle_main_menu_shell_mouse_down(state);
                        } else {
                            Self::handle_main_menu_shell_mouse_up(state, event_loop);
                        }
                    }
                } else if !egui_consumed && state.frontend.screen == GameScreen::SpawnPick {
                    if button == MouseButton::Left && btn_state.is_pressed() {
                        crate::app::presentation::spawn_pick::handle_spawn_pick_click(state);
                    }
                } else if !egui_consumed && state.frontend.screen == GameScreen::InGame {
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
                if !egui_consumed
                    && state.frontend.screen == GameScreen::MainMenu
                    && Self::native_skirmish_shell_active(state)
                    && Self::handle_skirmish_shell_mouse_wheel(state, lines)
                {
                    state.platform.window.request_redraw();
                    return;
                }
                if !egui_consumed
                    && (state.frontend.screen == GameScreen::SpawnPick
                        || (state.frontend.screen == GameScreen::InGame && !state.match_state.paused))
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
            Self::update_saved_seed_browser_scroll(state, false);
            Self::update_load_saved_game_scroll(state, false);
            [
                crate::app::input::keyboard::poll_scroll_repeat(state),
                crate::app::input::sound::poll_scroll_repeat(state),
                Self::poll_movie_list_scroll(state),
                Self::poll_campaign(state),
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
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
        } else if let Some(state) = &self.state {
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
                // Nothing on screen to keep fresh. Park the redraw loop until a
                // window event (including the un-occlude edge) wakes it, rather
                // than rendering frames no one can see.
                event_loop.set_control_flow(ControlFlow::Wait);
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
