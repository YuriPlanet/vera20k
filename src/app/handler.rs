//! Winit lifecycle and window-event routing for the app orchestrator.
//!
//! Event priority and consumption order are player-visible contracts; this
//! module keeps the original handler body intact.

use super::audio_runtime::AppAudioRuntime;
use super::diagnostics::state::DiagnosticsState;
use super::frame::{FramePass, FrameRenderOutcome};
use super::frontend::menu_page_render::FamilyPage;
use super::frontend::state::FrontendState;
use super::input::dispatch;
use super::input::hotkeys::HotkeyCommand;
use super::input::precedence::{InputLayer, ModalPage, ScreenPage};
use super::input::state::MatchInputState;
use super::match_runtime::state::MatchState;
use super::persistence::PersistenceState;
use super::persistence::options_profile::RetailOptionsProfile;
use super::presentation::state::MatchPresentationState;
use super::process_assets::ProcessAssets;
use super::renderer_state::RendererState;
use super::{
    ActiveEventLoop, App, AppState, ApplicationHandler, ControlFlow, GameScreen, GpuContext,
    Instant, KeyCode, KeyEventExtModifierSupplement, MouseButton, MouseScrollDelta, PhysicalKey,
    PhysicalSize, PlatformState, SfxPlayer, Window, WindowEvent, WindowId,
};
use crate::ui::pause_menu::InGameMenuState;
use winit::event::{ElementState, KeyEvent};

/// One window input, routed to the layer that takes it.
#[derive(Clone, Copy)]
enum Input<'a> {
    Key(&'a KeyEvent),
    /// Pointer motion.
    Move,
    Button(MouseButton, ElementState),
    /// Wheel lines.
    Wheel(f32),
}

/// A key's release drops it from the held keys.
fn release_held_key(input: &mut MatchInputState, event: &KeyEvent) {
    if !event.state.is_pressed()
        && let PhysicalKey::Code(code) = event.physical_key
    {
        input.keys_held.remove(&code);
    }
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
        if !state.frontend.main_menu_dialog_open() {
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
        if is_escape
            && Self::close_shell_menu_list(
                &mut state.platform,
                &mut state.renderer,
                &mut state.frontend,
                &mut state.match_state,
                &mut state.process_assets,
                &mut state.audio,
                &mut state.persistence,
            )
        {
            return true;
        }
        if state.frontend.shell_message_box.is_some() {
            // IsDialogMessage in the native modal pump5D4D50 admits Enter
            // as IDOK1 and Escape as IDCANCEL2, which the box's hidden IDOK
            // answers; GameExitDefeat6860EA returns to the shell.
            // Keep retained launcher dialogs on their separate Escape path.
            // The native message pump forwards repeated WM_KEYDOWN too;
            // its IsDialogMessage path has no previous-key-state filter.
            if pressed
                && let Some(key) = crate::ui::shell::controller::gui_key(code, false)
                // Alt+Enter is WM_SYSKEYDOWN, not the IDOK-producing
                // WM_KEYDOWN path. Winit combines them; retain the native
                // distinction using the existing modifier owner.
                && (key != vera20k_gui::Key::Enter || !state.platform.live_modifiers.alt_key())
            {
                Self::route_shell_message_box_key(state, key);
            }
        }
        // Otherwise the launcher Options `0xD5` is up (A3 takes its own keys):
        // it has no Cancel button, as its proc ignores IDCANCEL=2, so Escape
        // keeps the same dialog instance and emits no Apply, write or event.
        true
    }

    /// Drop every sticky in-game pointer byte.
    ///
    /// gamemd's capture-changed case drops the button-held byte and tears the
    /// band rectangle down; without this the right-drag pan keeps applying its
    /// anchor-relative step every frame and edge scroll stays inhibited until
    /// the player right-clicks again. The match's chrome keeps its own
    /// sticky capture and must die on the same edge, or the next click is still
    /// routed to the region that captured it (the release that would free it
    /// was lost).
    pub(crate) fn release_in_game_pointer_state(match_state: &mut MatchState) {
        match_state.input.tactical_mouse.reset_on_focus_loss();
        match_state.input.selection_state.cancel_drag();
        // VERA right-click orders: a double-click cannot span a focus change.
        match_state.input.last_selection_click = None;
        // VERA formation line: a line being drawn goes with the band box.
        match_state.input.formation_line.cancel();
        match_state.input.cancel_minimap_press();
        // A touch stream can end without an `Ended` when the window loses
        // focus with fingers down, and the live gesture owns the edge-scroll
        // inhibit, so it dies on the same edge as the rest of the pointer
        // state.
        match_state.input.touch_mapper.reset();
        crate::app::input::chrome_input::cancel(&mut match_state.match_presentation);
        let presentation = &mut match_state.match_presentation;
        presentation.dialog_ui.gui_mut().cancel_pointer();
        presentation.diplomacy_page = Default::default();
        if let Some(page) = presentation.options_page.as_mut() {
            page.dialog.shell_cancel_pointer_gesture();
        }
        if let Some(dialog) = presentation.sound_dialog.as_mut() {
            dialog.ui.gui_mut().cancel_pointer();
        }
        if let Some(browser) = match_state.match_presentation.saved_game_browser.as_mut() {
            browser.reset_interaction();
        }
    }

    fn resize_surface_for_window_size(
        platform: &PlatformState,
        renderer: &mut RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
        size: PhysicalSize<u32>,
    ) {
        renderer.gpu.resize(size.width, size.height);
        renderer.depth_view = renderer.gpu.create_depth_texture();
        renderer.shell_surface_presenter.resize(&renderer.gpu);
        // The frame-index wave is driven by wall-clock ticks and repaints every
        // frame, so a mid-flight resize simply lets it finish; no snap/cancel.
        Self::invalidate_main_menu_movie_if_base_changed(&renderer.gpu, frontend);
        Self::sync_zoom_pass(platform, renderer, &persistence.options_profile);
        crate::app::presentation::sidebar_render::refresh_sidebar_projection(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
        );
    }

    /// Keep the zoom pass matching the window and the Options choices: a
    /// source target of the window size over the zoom, in the window's
    /// shape or the original 4:3, or none when that is the whole window. The
    /// match reads its size through `render_width`/`render_height`, so a
    /// resize mid-match re-projects the battlefield and sidebar the same way.
    /// A sealed capture never zooms or takes bars.
    pub(crate) fn sync_zoom_pass(
        platform: &PlatformState,
        renderer: &mut RendererState,
        profile: &RetailOptionsProfile,
    ) {
        let (width, height) = (renderer.gpu.config.width, renderer.gpu.config.height);
        let geometry = Self::match_zoom_geometry(platform, &renderer.gpu, profile);
        if geometry.is_whole_window(width, height) {
            renderer.upscale_pass = None;
        } else if let Some(pass) = renderer.upscale_pass.as_mut() {
            pass.set_geometry(&renderer.gpu, geometry);
        } else {
            renderer.upscale_pass = Some(crate::render::upscale_pass::UpscalePass::new(
                &renderer.gpu,
                geometry,
            ));
        }
    }

    /// Present by the Options page's VSync box (`[VERA] VSync=`). A sealed
    /// capture keeps presenting in step with the display, as its profile
    /// records.
    pub(crate) fn sync_present_mode(
        platform: &PlatformState,
        gpu: &mut GpuContext,
        profile: &RetailOptionsProfile,
    ) {
        let vsync = profile.vsync || platform.capture_client_size.is_some();
        gpu.set_vsync(vsync);
    }

    /// Where a match frame sits in the window at the Options choices: the
    /// geometry the zoom pass takes, and the battle frame the Options page
    /// offers sidebar sizes for.
    pub(crate) fn match_zoom_geometry(
        platform: &PlatformState,
        gpu: &GpuContext,
        profile: &RetailOptionsProfile,
    ) -> crate::ui::main_menu_dialogs::options::ZoomGeometry {
        use crate::ui::main_menu_dialogs::options::{FrameShape, ZoomGeometry};
        let (width, height) = (gpu.config.width, gpu.config.height);
        let (percent, shape) = if platform.capture_client_size.is_some() {
            let zoom = platform.capture_presentation.zoom;
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
    pub(crate) fn set_full_screen(
        platform: &mut PlatformState,
        profile: &RetailOptionsProfile,
        full_screen: bool,
    ) {
        if !full_screen {
            // VERA: the game's own switch (the Options and Esc menu boxes, the
            // Full Screen key) leaves full screen to the launch window, not to
            // the placement winit saved on entry. The title-bar button and the
            // system shortcut stay the window system's.
            Self::restore_launch_window(platform);
            return;
        }
        let window = platform.window.clone();
        let displays = crate::app::display_target::current_displays(&window);
        let target = profile.display.clone();
        crate::app::display_target::center_windowed(&window, &displays, &target);
        window.set_fullscreen(Some(crate::app::display_target::fullscreen_for(
            &displays, &target,
        )));
    }

    /// The main menu Esc menu's Set Window Size button, and the Full Screen switch going
    /// off: the window as a launch opens it.
    /// A full-screen window leaves full screen first and macOS animates that,
    /// so `follow_window_full_screen` does the rest once the window is out.
    pub(crate) fn restore_launch_window(platform: &mut PlatformState) {
        if platform.capture_client_size.is_none() {
            platform.launch_window_owed = true;
            platform.window.set_fullscreen(None);
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
    fn follow_window_full_screen(
        platform: &mut PlatformState,
        frontend: &mut FrontendState,
        presentation: &mut MatchPresentationState,
        profile: &mut RetailOptionsProfile,
    ) {
        if platform.capture_client_size.is_some() {
            return;
        }
        let full_screen = platform.window.fullscreen().is_some();
        #[cfg(target_os = "macos")]
        if !full_screen {
            show_menu_bar_and_dock_after_full_screen();
        }
        if full_screen {
            platform.recenter_windowed = false;
        } else {
            let leaving_full_screen = profile.full_screen;
            // Only a placement still owed asks the system for its screens: the
            // list costs a display-list query, too much for every pass.
            if leaving_full_screen || platform.recenter_windowed {
                let displays = crate::app::display_target::current_displays(&platform.window);
                let on_choice = crate::app::display_target::window_is_on(
                    &platform.window,
                    &displays,
                    &profile.display,
                );
                if !on_choice {
                    crate::app::display_target::center_windowed(
                        &platform.window,
                        &displays,
                        &profile.display,
                    );
                }
                platform.recenter_windowed = !on_choice;
            }
            if std::mem::take(&mut platform.launch_window_owed) {
                let displays = crate::app::display_target::current_displays(&platform.window);
                let size = platform.launch_window_size;
                crate::app::display_target::restore_launch_placement(
                    &platform.window,
                    &displays,
                    &profile.display,
                    winit::dpi::LogicalSize::new(size.width, size.height),
                );
            }
        }
        let changed = profile.full_screen != full_screen;
        profile.full_screen = full_screen;
        if let Some(dialog) = frontend.options_dialog.as_mut() {
            dialog.sync_full_screen(full_screen);
        }
        if let Some(page) = presentation.options_page.as_mut() {
            page.dialog.sync_full_screen(full_screen);
        }
        Self::shell_menu_follow_full_screen(
            platform,
            &mut frontend.shell_message_box,
            profile,
            full_screen,
            changed,
        );
    }

    /// After a frame could not be taken, fit the surface to the window's size
    /// as it is now. Resize events normally keep the two in step, but a
    /// swapchain still stale after its reconfigure can be sized for a window
    /// size no event reported, and only the window's own size frees it. winit
    /// 0.30.12's Windows `set_fullscreen` notes such a race: marking the window
    /// full screen sends resize messages with the old size. A sealed capture
    /// keeps its fixed surface, and a minimised window has no size to fit.
    pub(super) fn follow_window_size(
        platform: &mut PlatformState,
        renderer: &mut RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &ProcessAssets,
        audio: &mut AppAudioRuntime,
        persistence: &PersistenceState,
    ) {
        if platform.capture_client_size.is_some() {
            return;
        }
        let size = platform.window.inner_size();
        let config = &renderer.gpu.config;
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
        Self::resize_surface_for_window_size(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            size,
        );
        crate::app::input::cursor::apply_cursor_confinement(platform);
    }

    /// The screen page's pointer route: a move (`edge` `None`) or a button
    /// edge. A page reading only the left button takes its moves as `None`
    /// and its left edges as `Some(pressed)`.
    fn screen_pointer(
        state: &mut AppState,
        screen: ScreenPage,
        edge: Option<(MouseButton, winit::event::ElementState)>,
    ) {
        let down = edge.map(|(_, edge)| edge.is_pressed());
        let left = edge.is_none_or(|(button, _)| button == MouseButton::Left);
        let choose_map = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some();
        let skirmish = |state: &mut AppState| match down {
            None => Self::handle_skirmish_shell_mouse_move(state),
            Some(true) => Self::handle_skirmish_shell_mouse_down(state),
            Some(false) => Self::handle_skirmish_shell_mouse_up(state),
        };
        match screen {
            // Play_Movie reads and ignores mouse buttons; Show_Credits
            // queues them as ordinary Keyboard entries.
            ScreenPage::CreditsRoll if down.is_some() => Self::credits_roll_input(
                &mut state.frontend.credits_roll,
                crate::app::frontend::credits_roll::CreditsInput::Other,
            ),
            ScreenPage::LanLobby => match edge {
                None => crate::app::shell_lan::cursor_moved(state),
                Some((button, edge)) => {
                    crate::app::shell_lan::mouse(state, button, edge.is_pressed())
                }
            },
            ScreenPage::NetworkSetup if choose_map => {
                if left {
                    skirmish(state);
                }
            }
            ScreenPage::NetworkSetup => match edge {
                None => crate::app::input::multiplayer_setup::cursor_moved(state),
                Some((button, edge)) => {
                    crate::app::input::multiplayer_setup::mouse(state, button, edge.is_pressed())
                }
            },
            ScreenPage::MovieList if left => {
                let step = Self::handle_movie_list_mouse(
                    &state.renderer.gpu,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    down,
                );
                if let Some(step) = step {
                    Self::apply_step(state, step);
                }
            }
            ScreenPage::Mods if left => Self::handle_mods_mouse(state, down),
            ScreenPage::Campaign if left => match down {
                None => Self::handle_campaign_mouse_move(
                    &state.renderer.gpu,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                ),
                Some(true) => Self::handle_campaign_mouse_down(
                    &state.renderer.gpu,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                ),
                Some(false) => Self::handle_campaign_mouse_up(state),
            },
            ScreenPage::LoadSavedGame if left => match down {
                None => Self::handle_load_saved_game_mouse_move(state),
                Some(pressed) => Self::handle_load_saved_game_mouse(state, pressed),
            },
            ScreenPage::Wol if left => match down {
                None => Self::handle_wol_mouse_move(state),
                Some(true) => Self::handle_wol_mouse_down(state),
                Some(false) => Self::handle_wol_mouse_up(state),
            },
            ScreenPage::LauncherOptions if left => Self::handle_launcher_options_mouse(state, down),
            // A retained main-menu dialog takes the click: the SHP
            // quit-confirm modal hit-tests its OK/Cancel here.
            _ if down.is_some() && state.frontend.main_menu_dialog_open() => {
                if screen == ScreenPage::MainMenu
                    && state.frontend.shell_message_box.is_some()
                    && left
                {
                    Self::handle_shell_message_box_mouse(state, down == Some(true));
                }
            }
            ScreenPage::CoopScore => match edge {
                None => crate::app::input::coop_score::cursor_moved(
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                ),
                Some((button, edge)) => {
                    crate::app::input::coop_score::mouse(state, button, edge.is_pressed())
                }
            },
            ScreenPage::CampaignScore if left => Self::handle_campaign_score_mouse(state, down),
            ScreenPage::Score if left => Self::handle_score_shell_mouse(state, down),
            ScreenPage::Skirmish if left => skirmish(state),
            ScreenPage::SinglePlayer if left => {
                Self::handle_family_page_mouse(state, FamilyPage::SinglePlayer, down);
            }
            ScreenPage::MoviesAndCredits if left => {
                Self::handle_family_page_mouse(state, FamilyPage::MoviesAndCredits, down);
            }
            // While the SHP quit-confirm modal owns the controller, the menu
            // move handler must not re-activate 0xE2 and reset the gesture:
            // the open box takes the move, and the update offer's change list
            // follows it.
            ScreenPage::MainMenu
                if down.is_none() && state.frontend.shell_message_box.is_some() =>
            {
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                crate::app::updater::notes::cursor_moved(
                    &state.renderer.gpu,
                    &mut state.frontend,
                    &state.match_state.input,
                );
            }
            ScreenPage::MainMenu if left => Self::handle_main_menu_shell_mouse(state, down),
            ScreenPage::InGameShell(_) | ScreenPage::Battlefield => match edge {
                None => dispatch::handle_cursor_moved_in_game(state),
                Some((button, edge)) => dispatch::handle_mouse_input(state, button, edge),
            },
            _ => {}
        }
    }

    /// Routes one window input to the layer `input::precedence` names, then
    /// asks for one draw: the pages' input handlers ask for none of their
    /// own. Pointer motion over a match waits for the redraw gate's limit.
    fn route_input(state: &mut AppState, input: Input) {
        Self::route_to_layer(state, input);
        if !matches!(input, Input::Move) || state.frontend.screen != GameScreen::InGame {
            state.platform.window.request_redraw();
        }
    }

    fn route_to_layer(state: &mut AppState, input: Input) {
        use crate::app::frontend::preview_zoom;
        use crate::app::input::tooltips;
        let layer = crate::app::input::precedence::input_layer(
            &state.platform,
            &state.renderer,
            &state.frontend,
            &state.match_state,
            &state.persistence,
        );
        // The blocking layers take every input first.
        match (layer, input) {
            (InputLayer::HouseOutcomeWait, Input::Key(event)) => {
                return release_held_key(&mut state.match_state.input, event);
            }
            (InputLayer::HouseOutcomeWait, Input::Button(_, edge)) => {
                if !edge.is_pressed() {
                    Self::release_in_game_pointer_state(&mut state.match_state);
                }
                return;
            }
            (InputLayer::HouseOutcomeWait, _) => return,
            (InputLayer::Page(ModalPage::NetworkFailure), input) => {
                use crate::app::match_runtime::network_failure;
                match input {
                    Input::Key(event) => network_failure::key(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        event,
                    ),
                    Input::Move => network_failure::cursor_moved(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                    ),
                    Input::Button(button, edge) => network_failure::mouse(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        button,
                        edge.is_pressed(),
                    ),
                    Input::Wheel(_) => {}
                }
                return;
            }
            _ => {}
        }
        let zoom_page = matches!(
            layer,
            InputLayer::Screen(ScreenPage::NetworkSetup | ScreenPage::Skirmish)
        );
        match input {
            Input::Key(event)
                if Self::window_key(
                    &mut state.platform,
                    &mut state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &mut state.process_assets,
                    &state.persistence,
                    event,
                ) =>
            {
                return;
            }
            // VERA's enlarged map preview is a modal over the page: nothing
            // under it follows the pointer; only the cursor over it moves.
            Input::Move
                if zoom_page
                    && preview_zoom::pointer(
                        &state.platform.window,
                        &mut state.frontend,
                        &state.match_state.input,
                        None,
                    ) =>
            {
                return;
            }
            // Shared tooltip service: every move restarts the show delay and
            // hides a visible tip, and any button edge (middle too) kills a
            // visible tip and its timer (study S1).
            Input::Move => tooltips::on_mouse_move(
                &state.platform,
                &state.renderer,
                &state.diag,
                &state.frontend,
                &mut state.match_state,
                &state.process_assets,
                &state.persistence,
            ),
            Input::Button(..) => tooltips::on_button_event(&state.diag, &mut state.match_state),
            Input::Key(_) | Input::Wheel(_) => {}
        }
        let screen = match layer {
            InputLayer::Screen(screen) => screen,
            // The wait loop pumps 0xEA without IsDialogMessage and polls no
            // game keys while it is open (0x006497EC); its mouse route
            // belongs to the in-game dispatch.
            InputLayer::Page(ModalPage::NetworkWait) => {
                match input {
                    Input::Key(event) => release_held_key(&mut state.match_state.input, event),
                    Input::Move => crate::app::input::connection::cursor_moved(
                        &state.platform,
                        &state.renderer,
                        &state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        &state.persistence,
                    ),
                    Input::Button(button, edge) => {
                        dispatch::handle_mouse_input(state, button, edge)
                    }
                    Input::Wheel(_) => {}
                }
                return;
            }
            InputLayer::Page(ModalPage::Keyboard) => {
                use crate::app::input::keyboard;
                match input {
                    Input::Key(event) => keyboard::key(state, event),
                    Input::Move => keyboard::cursor_moved(state),
                    Input::Button(button, edge) => {
                        keyboard::mouse(state, button, edge.is_pressed())
                    }
                    Input::Wheel(lines) => keyboard::wheel(state, lines),
                }
                return;
            }
            // The open box takes the input; the update offer's change list
            // follows the pointer and the wheel.
            InputLayer::Page(ModalPage::ShellMessageBox) => {
                match input {
                    Input::Key(event) => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            Self::raise_shell_message_box(&mut state.frontend);
                            Self::handle_main_menu_dialog_key(
                                state,
                                code,
                                event.state.is_pressed(),
                                event.repeat,
                            );
                        }
                    }
                    Input::Move => {
                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        crate::app::updater::notes::cursor_moved(
                            &state.renderer.gpu,
                            &mut state.frontend,
                            &state.match_state.input,
                        );
                        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
                        super::shell_mod_import::cursor_moved(
                            &state.renderer,
                            &mut state.frontend,
                            &state.match_state.input,
                        );
                        Self::shell_menu_pointer(
                            &mut state.platform,
                            &mut state.renderer,
                            &mut state.frontend,
                            &mut state.match_state,
                            &mut state.process_assets,
                            &mut state.audio,
                            &mut state.persistence,
                            None,
                        );
                    }
                    Input::Button(MouseButton::Left, edge) => {
                        Self::raise_shell_message_box(&mut state.frontend);
                        // Add Mod's parts list takes its own presses.
                        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
                        if super::shell_mod_import::pointer_button(
                            &state.renderer,
                            &mut state.frontend,
                            edge.is_pressed(),
                        ) {
                            return;
                        }
                        Self::handle_shell_message_box_mouse(state, edge.is_pressed());
                    }
                    Input::Button(..) => {}
                    Input::Wheel(_lines) => {
                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        crate::app::updater::notes::wheel(
                            &state.renderer.gpu,
                            &mut state.frontend,
                            _lines,
                        );
                        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
                        super::shell_mod_import::wheel(
                            &state.platform.window,
                            &state.renderer,
                            &mut state.frontend,
                            _lines,
                        );
                    }
                }
                return;
            }
            // A slide or the quit cascade takes no input; the status card
            // took the shell error's.
            InputLayer::ShellTransition
            | InputLayer::HouseOutcomeWait
            | InputLayer::Page(ModalPage::NetworkFailure | ModalPage::ShellError) => return,
        };
        match input {
            Input::Key(event) => Self::screen_key(state, screen, event),
            Input::Move => Self::screen_pointer(state, screen, None),
            Input::Button(_, edge)
                if zoom_page
                    && preview_zoom::pointer(
                        &state.platform.window,
                        &mut state.frontend,
                        &state.match_state.input,
                        Some(edge.is_pressed()),
                    ) => {}
            Input::Button(button, edge) => {
                Self::screen_pointer(state, screen, Some((button, edge)))
            }
            Input::Wheel(lines) => Self::screen_wheel(state, screen, lines),
        }
    }

    /// The keys taken ahead of every page: the release that ends an
    /// admitted planning gesture, and VERA's every-screen window keys. True
    /// when the key was used.
    fn window_key(
        platform: &mut PlatformState,
        renderer: &mut RendererState,
        diag: &DiagnosticsState,
        frontend: &mut FrontendState,
        match_state: &mut MatchState,
        process_assets: &mut ProcessAssets,
        persistence: &PersistenceState,
        event: &KeyEvent,
    ) -> bool {
        // Host delivery adaptation: dialogs may consume ordinary keys,
        // but a release for an admitted planning gesture must still
        // end that gesture. Opening a menu itself never commits it.
        if !event.state.is_pressed()
            && frontend.screen == GameScreen::InGame
            && let PhysicalKey::Code(code) = event.physical_key
            && match_state.input.planning.owns_key(code)
        {
            crate::app::input::planning_mode::key_edge(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                persistence,
                crate::app::input::hotkeys::HotkeyResolution::Unhandled,
                code,
                event.state,
                event.repeat,
            );
            match_state.input.keys_held.remove(&code);
            return true;
        }
        // VERA's every-screen keys, as the Keyboard page's VERA section
        // binds them: Lock Cursor to Window (Ctrl+Alt+L), Full Screen
        // (Alt+Enter) and Performance Stats (Ctrl+Alt+F) by default. Scroll
        // Lock and, on macOS, the system's Ctrl+Cmd+F stay fixed
        // extras ahead of the bindings. All are consumed before retail
        // key dispatch, but a key the Keyboard page is capturing goes
        // to the page.
        let modifiers = platform.live_modifiers;
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
        let capturing = frontend
            .keyboard_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.capture_focused());
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
            match_state
                .input
                .hotkey_bindings
                .window_command(logical, event.location, modifiers)
        });
        // A capture keeps its fixed surface, and its Full Screen key
        // goes on to the screen.
        let Some(command) = window_command.filter(|command| {
            *command != HotkeyCommand::FullScreen || platform.capture_client_size.is_none()
        }) else {
            return false;
        };
        if event.state.is_pressed() && !event.repeat {
            dispatch::run_window_command(
                platform,
                &mut renderer.performance_overlay,
                frontend,
                match_state,
                process_assets,
                &persistence.options_profile,
                command,
            );
        }
        true
    }

    /// A key on the screen's page, in the order the pages take keys.
    fn screen_key(state: &mut AppState, screen: ScreenPage, event: &KeyEvent) {
        let pressed = event.state.is_pressed();
        let fresh_press = pressed && !event.repeat;
        let zoom_page = matches!(screen, ScreenPage::NetworkSetup | ScreenPage::Skirmish);
        let chooser_open = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some();
        if (zoom_page
            && crate::app::frontend::preview_zoom::key(
                &state.platform.window,
                &mut state.frontend,
                event,
            ))
            || Self::shell_menu_key(
                &state.platform,
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &state.process_assets,
                state.audio.launcher_audio_available,
                &state.persistence.options_profile,
                event,
                screen,
            )
        {
            return;
        }
        #[cfg(target_os = "android")]
        if screen == ScreenPage::Mods && state.frontend.mods.name_dialog.is_some() {
            crate::app::App::name_dialog_key(state, event);
            return;
        }
        match screen {
            ScreenPage::CoopScore => return crate::app::input::coop_score::key(state, event),
            ScreenPage::LanLobby => return crate::app::shell_lan::key(state, event),
            // VERA: typing in Choose Map fills its search line.
            _ if zoom_page
                && Self::choose_map_search_key(
                    &state.platform,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.persistence,
                    event,
                ) =>
            {
                return;
            }
            ScreenPage::NetworkSetup if !chooser_open => {
                return crate::app::input::multiplayer_setup::key(state, event);
            }
            // ChooseMap's loop has no IsDialogMessage. Only the existing
            // eject prompt consumes its default keys.
            ScreenPage::NetworkSetup => {
                if fresh_press
                    && let PhysicalKey::Code(code) = event.physical_key
                    && !Self::route_validation_modal_key(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                        code,
                    )
                    && matches!(
                        code,
                        KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
                    )
                {
                    Self::answer_choose_map_prompt_key(state, code != KeyCode::Escape);
                }
                return;
            }
            _ => {}
        }
        if screen == ScreenPage::Skirmish {
            // The seed browser, then VERA's Setups page, takes the keys
            // over the page.
            let shell = &state.frontend.skirmish_shell_state;
            let game_options = shell.game_options.is_some();
            let browser_key: Option<fn(&mut AppState, Option<KeyCode>, Option<&str>)> =
                if shell.saved_seed_browser.is_some() {
                    Some(|state, code, text| {
                        Self::handle_saved_seed_browser_key(
                            state.platform.game_config.as_ref(),
                            &state.renderer,
                            &mut state.frontend,
                            &mut state.match_state,
                            &mut state.process_assets,
                            &mut state.audio.sfx_player,
                            code,
                            text,
                        )
                    })
                } else if shell.setups_browser.is_some() {
                    Some(|state, code, text| {
                        Self::handle_skirmish_setups_key(
                            &state.platform.window,
                            &state.renderer,
                            &mut state.frontend,
                            &state.match_state,
                            &state.process_assets,
                            &mut state.audio.sfx_player,
                            code,
                            text,
                        )
                    })
                } else {
                    None
                };
            if let Some(browser_key) = browser_key {
                if pressed {
                    let code = match event.physical_key {
                        PhysicalKey::Code(code) => Some(code),
                        _ => None,
                    };
                    browser_key(state, code, event.text.as_deref());
                }
                return;
            }
            // VERA's Game Options box takes the keys over the page; its
            // OK takes Escape and Enter, as the room's does.
            if game_options {
                let key = match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => Some(vera20k_gui::Key::Escape),
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                        Some(vera20k_gui::Key::Enter)
                    }
                    _ => None,
                };
                if let Some(key) = key.filter(|_| fresh_press) {
                    Self::skirmish_game_options_key(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.process_assets,
                        &mut state.persistence.options_profile,
                        key,
                    );
                }
                return;
            }
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        // ESC always reaches the handler when in-game (even when paused)
        // so the player can toggle pause regardless of diagnostic GUI focus.
        let is_escape = code == KeyCode::Escape && fresh_press;
        let enter_or_escape = matches!(
            code,
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
        );
        let in_game = state.frontend.screen == GameScreen::InGame;
        let paused_at_event = in_game && state.match_state.paused();
        let locally_paused_at_event = in_game && state.match_state.local_paused();
        let modifiers = state.platform.live_modifiers;
        // A shared pause must be resumable without a simulation tick,
        // including while the local Options dialog is open.
        if in_game
            && code == KeyCode::Pause
            && !(modifiers.shift_key()
                || modifiers.control_key()
                || modifiers.alt_key()
                || modifiers.super_key())
            && crate::app::input::pause_menu::multiplayer_control(&state.match_state).is_some()
        {
            if fresh_press {
                crate::app::input::pause_menu::toggle_multiplayer_pause(
                    &state.platform.window,
                    &mut state.match_state,
                );
            }
            return;
        }
        match screen {
            ScreenPage::FullscreenMovie => {
                return Self::fullscreen_movie_key(
                    state.platform.live_modifiers,
                    &mut state.frontend.fullscreen_movie,
                    code,
                    pressed,
                );
            }
            ScreenPage::CreditsRoll => {
                use crate::app::frontend::credits_roll::CreditsInput;
                // Keyboard::Get() == 0x1B: a bare Escape press. Releases
                // (0x800) and every other key are queued and ignored.
                let bare =
                    !(modifiers.shift_key() || modifiers.control_key() || modifiers.alt_key());
                let input = if code == KeyCode::Escape && pressed && bare {
                    CreditsInput::Escape
                } else {
                    CreditsInput::Other
                };
                return Self::credits_roll_input(&mut state.frontend.credits_roll, input);
            }
            _ => {}
        }
        if Self::handle_main_menu_dialog_key(state, code, pressed, event.repeat)
            || Self::handle_main_menu_typed_code_key(
                &mut state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                &mut state.persistence.options_profile,
                event,
            )
            || (screen == ScreenPage::Skirmish
                && fresh_press
                && (Self::route_validation_modal_key(
                    &state.platform.window,
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    code,
                ) || (enter_or_escape
                    && Self::answer_choose_map_prompt_key(state, code != KeyCode::Escape))))
        {
            return;
        }
        let menu = state.match_state.match_presentation.in_game_menu;
        match screen {
            ScreenPage::Wol => {
                return Self::handle_wol_key(state, Some(code), pressed, event.text.as_deref());
            }
            ScreenPage::LoadSavedGame => {
                if pressed {
                    Self::handle_load_saved_game_key(state, Some(code), event.text.as_deref());
                }
                return;
            }
            ScreenPage::InGameShell(InGameMenuState::SavedGame(_)) => {
                if pressed {
                    Self::saved_game_key(state, Some(code), event.text.as_deref());
                }
                return;
            }
            // 0x005D36A0: IDOK and IDCANCEL both answer result 1.
            _ if in_game && menu == InGameMenuState::Surrender => {
                if fresh_press && enter_or_escape {
                    crate::app::input::surrender::key(
                        &mut state.platform,
                        &state.renderer,
                        &state.diag,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio,
                        &mut state.persistence,
                    );
                }
                return;
            }
            // 0xCF's loop takes every key; Escape names nothing. A release
            // still drops its held key: the loop's KeyboardClear
            // (0x0077D861) leaves nothing latched.
            _ if in_game && menu == InGameMenuState::Page => {
                if pressed {
                    crate::app::shell_wol::page::key_press(
                        &mut state.frontend,
                        &mut state.platform,
                        &mut state.match_state,
                        &state.process_assets,
                        &state.renderer,
                        &mut state.audio,
                        &mut state.persistence,
                        Some(code),
                        event.text.as_deref(),
                    );
                } else {
                    state.match_state.input.keys_held.remove(&code);
                }
                return;
            }
            // B8 has no IDOK/IDCANCEL close command. Neither has 0x73: its
            // proc keys commands by GetDlgCtrlID(lParam) (0x006581A7), and
            // Return 0x423 is owner-drawn, so Enter and Escape name no child
            // it handles.
            ScreenPage::InGameShell(
                InGameMenuState::Sound
                | InGameMenuState::Diplomacy
                | InGameMenuState::MissionBriefing,
            ) => return,
            // B6 accepts default IDOK1 as Resume (4F1A59..4F1A65). Owner
            // buttons reject focus through610CA0:6118BF..DE; their resource
            // has no WS_TABSTOP. Do not invent a focused-button Return/Space
            // route. IDCANCEL is ignored.
            ScreenPage::InGameShell(InGameMenuState::AbortConfirm) => {
                if fresh_press && matches!(code, KeyCode::Enter | KeyCode::NumpadEnter) {
                    crate::app::input::abort::activate(
                        &mut state.platform,
                        &state.renderer,
                        &state.diag,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio,
                        &mut state.persistence,
                        crate::ui::shell::abort::AbortButton::Resume,
                    );
                }
                return;
            }
            _ => {}
        }
        // Shared pause still admits chat. The keyboard owner consumes chat
        // first, then blocks gameplay hotkeys.
        if !crate::app::input::hotkeys::input_admitted_while_paused(
            locally_paused_at_event,
            &event.logical_key,
        ) || (screen == ScreenPage::Skirmish
            && pressed
            && !is_escape
            && Self::handle_skirmish_shell_key_input(
                &state.platform,
                &state.renderer.bit_font,
                &mut state.frontend,
                code,
                event.text.as_deref(),
            ))
            || !in_game
        {
            return;
        }
        // An existing modal handles Escape before tactical input. World
        // Escape goes through the bindings below; Options opens B5 only if
        // selected, or as the native fallback.
        if is_escape && Self::in_game_menu_owns_escape(&state.match_state.match_presentation) {
            return Self::route_in_game_menu_escape(
                &mut state.platform,
                &mut state.renderer,
                &state.diag,
                &mut state.frontend,
                &mut state.match_state,
                &mut state.process_assets,
                &mut state.audio,
                &mut state.persistence,
            );
        }
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

    /// The wheel on the screen's page. ComboDropWin consumes no mouse-wheel
    /// selection or scroll; the pages left out have no wheel route.
    fn screen_wheel(state: &mut AppState, screen: ScreenPage, lines: f32) {
        let chooser_open = state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_some();
        match screen {
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            ScreenPage::MainMenu => {
                crate::app::updater::notes::wheel(&state.renderer.gpu, &mut state.frontend, lines);
            }
            // Nothing under VERA's enlarged map preview takes it.
            ScreenPage::NetworkSetup | ScreenPage::Skirmish
                if crate::app::frontend::preview_zoom::wheel(
                    &state.platform.window,
                    &mut state.frontend,
                ) => {}
            ScreenPage::NetworkSetup if !chooser_open => {
                crate::app::input::multiplayer_setup::wheel(state, lines);
            }
            ScreenPage::Mods => {
                Self::handle_mods_wheel(&state.renderer.gpu, &mut state.frontend, lines)
            }
            ScreenPage::Skirmish => {
                Self::handle_skirmish_shell_mouse_wheel(
                    &state.renderer,
                    &mut state.frontend,
                    &state.match_state,
                    &state.process_assets,
                    lines,
                );
            }
            ScreenPage::InGameShell(_) | ScreenPage::Battlefield
                if !state.match_state.paused()
                    && !crate::app::input::campaign_input::is_locked(&state.match_state) =>
            {
                if crate::app::presentation::super_timers::wheel(
                    &state.platform,
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state,
                    &state.persistence,
                    lines,
                ) {
                    return;
                }
                // Optional player setting: over the battlefield the wheel
                // changes the view distance tier, anchored on the tactical
                // centre. Over the sidebar and the bottom strip it keeps
                // gamemd's native SidebarUp/SidebarDown routing.
                if state.persistence.options_profile.mouse_wheel_zoom
                    && crate::app::input::camera::cursor_over_battlefield(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &state.match_state,
                        &state.persistence,
                    )
                {
                    crate::app::input::camera::cycle_view_distance(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state,
                        &state.persistence,
                        lines,
                    );
                } else if let Some(action) = dispatch::wheel_notch(lines) {
                    // Every wheel notch scrolls the active build strip by
                    // one row. gamemd routes the wheel message straight to
                    // the SidebarUp / SidebarDown commands and has no world
                    // zoom.
                    dispatch::sidebar_scroll_command(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state,
                        &state.persistence,
                        action,
                    );
                }
            }
            _ => {}
        }
    }

    /// The timers of the page the shell route shows; the next wake.
    fn poll_route_page(
        window: &Window,
        renderer: &RendererState,
        frontend: &mut FrontendState,
        process_assets: &ProcessAssets,
        sfx_player: &mut Option<SfxPlayer>,
    ) -> Option<Instant> {
        match crate::app::input::precedence::route_page(frontend) {
            ScreenPage::MovieList | ScreenPage::Mods => {
                Self::poll_family_list(window, &mut frontend.shell_controller)
            }
            ScreenPage::Campaign => Self::poll_campaign(frontend, process_assets, sfx_player),
            ScreenPage::LoadSavedGame => {
                Self::update_load_saved_game_scroll(renderer, frontend);
                None
            }
            _ => None,
        }
    }

    fn campaign_score_animation_deadline(frontend: &FrontendState) -> Option<Instant> {
        if Self::campaign_score_shell_active(frontend) {
            frontend
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
                // Startup built every renderer and its shaders on this device:
                // its API works here, and a loss from now on is play's
                // (`persistence::startup_backend`).
                #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
                crate::app::persistence::startup_backend::start_finished();
                // Resizes may have arrived while startup owned the renderer.
                Self::follow_window_size(
                    &mut state.platform,
                    &mut state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &state.persistence,
                );
                if let Some(request) = self.replay_request.take() {
                    let loading = request.replay.standalone_startup().and_then(|startup| {
                        crate::app::loading::pump::LoadingRequest::replay(
                            startup,
                            request.map_beside.as_deref(),
                        )
                    });
                    match loading {
                        Ok(loading) => {
                            state.match_state.match_diagnostics.begin_playback(
                                request.replay,
                                request.capture_output,
                                request.replay_sha256,
                            );
                            crate::app::loading::pump::begin_loading(&mut state, loading);
                        }
                        Err(error) => log::error!("The diagnostic replay cannot start: {error:#}"),
                    }
                }
                #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                let mut browser_spawn_failure = None;
                if let Some(request) = self.spawn_request.take()
                    && let Err(error) = Self::start_spawn_session(&mut state, request)
                {
                    let rules_mismatch = error
                        .downcast_ref::<crate::app::frontend::cncnet_spawn::RulesMismatch>()
                        .and_then(|mismatch| mismatch.parts.clone());
                    let error = format!("CnCNet startup failed: {error:#}");
                    log::error!("{error}");
                    // docs/spawn.md: the page keeps its engine and may start
                    // another launch; the report follows the Ready status.
                    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
                    {
                        crate::web::status(&error);
                        state.frontend.external_rules_mismatch = rules_mismatch;
                        browser_spawn_failure = Some(error);
                    }
                    #[cfg(not(all(
                        feature = "wasm",
                        target_arch = "wasm32",
                        target_os = "unknown"
                    )))]
                    {
                        self.startup_failure = Some(error);
                        self.spawn_rules_mismatch = rules_mismatch;
                        event_loop.exit();
                        return;
                    }
                }
                if let Some(session) = self.shell_capture.as_mut() {
                    session.prepare_state(
                        &mut state.frontend,
                        &mut state.match_state,
                        &mut state.process_assets,
                    );
                }
                if let Some(session) = self.hidden_run.as_mut()
                    && let Err(err) = session.prepare_state(&mut state)
                {
                    log::error!("The {} could not start: {err:#}", session.name());
                    session.fail(format!("{} preparation failed: {err:#}", session.name()));
                    event_loop.exit();
                    return;
                }
                // A player's run checks for updates from here, before the
                // shell's first frame, so the main menu waits for the check
                // (`updater::launch`); captures and hidden runs never check.
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                if self.shell_capture.is_none() && self.hidden_run.is_none() {
                    crate::app::updater::launch(&mut state.frontend);
                }
                // Clip the cursor to the window on first launch (
                // default). A focus regain re-applies it if the OS dropped it.
                let window_id = state.platform.window.id();
                self.state = Some(state);
                for event in events {
                    self.window_event(event_loop, window_id, event);
                }
                if let Some(state) = self.state.as_mut() {
                    crate::app::input::cursor::apply_cursor_confinement(&mut state.platform);
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

    /// Close the game as a closed window does: finish what the shell has
    /// open, then exit, or first leave the network (`begin_network_goodbye`).
    pub(super) fn close_game(state: &mut AppState, event_loop: &ActiveEventLoop) {
        Self::prepare_shell_terminal_exit(state);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if Self::begin_network_goodbye(state) {
            return;
        }
        event_loop.exit();
    }
}

/// Camera displacement in screen pixels. The normal camera clamp runs per frame.
#[cfg(not(target_os = "linux"))]
fn pan_touch_camera(input: &mut MatchInputState, dx: f32, dy: f32) {
    let zoom = input.zoom_level.max(0.01);
    let position = (input.camera_x + dx / zoom, input.camera_y + dy / zoom);
    crate::app::input::camera::set_camera_position(input, position);
}

/// Android: turns the activity's queued pointing-device events into window events.
#[cfg(target_os = "android")]
fn pump_android_pointer(app: &mut App, event_loop: &ActiveEventLoop) {
    let window_id = app.state.as_ref().map(|state| state.platform.window.id())
        .or_else(|| app.pending_initialization.as_ref().map(|job| job.window_id()));
    let Some(window_id) = window_id else { return };
    let mut translated = Vec::new();
    for event in crate::app::input::android_mouse::take() {
        let in_game = app.state.as_ref().is_some_and(|state| {
            state.frontend.screen == GameScreen::InGame && !state.match_state.paused()
        });
        app.android_pointer.translate(event, in_game, &mut translated);
        for event in translated.drain(..) {
            app.window_event(event_loop, window_id, event);
        }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        if let Some(job) = self.pending_initialization.as_mut() {
            job.admit_event_loop_turn();
        }
        // Android: the activity consumes hardware mouse and touchpad events,
        // which winit's Android backend would otherwise drop or read as touches.
        // They arrive before this loop turn's redraw, so the engine's pointer is
        // not a frame behind the system one.
        #[cfg(target_os = "android")]
        pump_android_pointer(self, _event_loop);
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
                Self::follow_window_size(
                    &mut state.platform,
                    &mut state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio,
                    &state.persistence,
                );
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
            self.spawn_request
                .as_ref()
                .and_then(|request| request.ares_setting)
                .or_else(|| {
                    self.hidden_run
                        .as_ref()
                        .and_then(super::hidden_run::HiddenRun::ares_setting)
                }),
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
            self.android_pointer = Default::default();
            let _ = crate::app::input::android_mouse::take();
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

        #[cfg(target_os = "android")]
        if let WindowEvent::PanGesture { delta, .. } = &event {
            if !shell_capture_active && !hidden_run_active
                && let Some(state) = self.state.as_mut()
                && state.frontend.screen == GameScreen::InGame
                && !state.match_state.paused()
                && state.persistence.pending_load.is_none()
            {
                pan_touch_camera(&mut state.match_state.input, -delta.x, -delta.y);
                state.platform.window.request_redraw();
            }
            return;
        }

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
                #[cfg(target_os = "android")]
                {
                    let right = output.events.iter().flatten().any(|event| {
                        matches!(
                            event,
                            WindowEvent::MouseInput {
                                state: ElementState::Pressed,
                                button: MouseButton::Right,
                                ..
                            }
                        )
                    });
                    let inside = crate::app::input::messages::lane_contains_window_px(
                        state,
                        touch.location.x as i32,
                        touch.location.y as i32,
                    );
                    if in_game && right && inside {
                        crate::app::input::chat::open_touch_edit(state);
                        return;
                    }
                    // Android: once the chat edit is open, a fresh tap on the
                    // lane sends it, standing in for the keyboard's Enter. The
                    // finger that opened the edit must not: its lift would
                    // send an empty line and close the edit at once.
                    if in_game
                        && inside
                        && matches!(touch.phase, winit::event::TouchPhase::Started)
                        && crate::app::input::chat::send_touch(state)
                    {
                        return;
                    }
                }
                if let Some((dx, dy)) = output.pan
                    && in_game
                {
                    pan_touch_camera(&mut state.match_state.input, dx, dy);
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
        crate::app::state::sync_cursor_space(
            &state.renderer,
            &state.frontend.screen,
            &mut state.match_state.input,
        );
        // Playback has presentation-only input. Exit is local and never
        // schedules EventClass EXIT or changes the recorded simulation. A
        // watcher's is presentation-only too: it gives no orders, Tab follows
        // the next player, Escape leaves the match and its mouse moves the
        // view.
        let replay = state.match_state.match_diagnostics.is_replay_playback();
        let watching = !replay
            && state.frontend.screen == GameScreen::InGame
            && crate::app::match_runtime::watching::active(&state.match_state);
        if replay || watching {
            match &event {
                WindowEvent::CloseRequested if replay => {
                    event_loop.exit();
                    return;
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if let PhysicalKey::Code(code) = event.physical_key {
                        let pressed = event.state.is_pressed();
                        // The modules' hotkeys read Shift/Ctrl/Alt from the
                        // key edges, as a player's keys do below.
                        crate::app::input::hotkeys::record_modifier_key(
                            &mut state.platform.held_modifier_keys,
                            &mut state.platform.live_modifiers,
                            &mut state.match_state.input.hotkey_modifiers,
                            code,
                            pressed,
                            false,
                        );
                        let in_game = state.frontend.screen == GameScreen::InGame;
                        if code == KeyCode::Escape
                            && pressed
                            && !event.repeat
                            && in_game
                            && crate::app::ui_modules::key_escape(state)
                        {
                            // Esc dismissed a client module's node before
                            // the viewer's own Esc (RFC 0001 section 10).
                        } else if replay && code == KeyCode::Escape {
                            if pressed {
                                event_loop.exit();
                            }
                        } else if watching
                            && crate::app::match_runtime::watching::key(
                                &state.platform,
                                &state.frontend,
                                &mut state.match_state,
                                &mut state.audio.sfx_player,
                                code,
                                pressed,
                                event.repeat,
                            )
                        {
                            // Escape and Tab are the watcher's own.
                        } else if matches!(
                            code,
                            KeyCode::ArrowLeft
                                | KeyCode::ArrowRight
                                | KeyCode::ArrowUp
                                | KeyCode::ArrowDown
                        ) {
                            if pressed {
                                state.match_state.input.keys_held.insert(code);
                            } else {
                                state.match_state.input.keys_held.remove(&code);
                            }
                        } else if pressed && !event.repeat && in_game {
                            // A key the viewer does not use goes to the
                            // client modules' hotkeys, as a player's unused
                            // key does (`input::keyboard`).
                            let unmodified = event.key_without_modifiers();
                            let key = crate::app::input::hotkeys::binding_logical_key(
                                &event.logical_key,
                                &unmodified,
                                event.location,
                            );
                            let modifiers = state.match_state.input.hotkey_modifiers;
                            crate::app::ui_modules::key_unused(
                                state,
                                key,
                                event.location,
                                modifiers,
                            );
                        }
                    }
                    return;
                }
                WindowEvent::CursorMoved { position, .. } => {
                    crate::app::state::set_cursor_from_window(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state.input,
                        position.x as f32,
                        position.y as f32,
                    );
                    // Client module nodes take a viewer's pointer too.
                    if state.frontend.screen == GameScreen::InGame {
                        crate::app::ui_modules::pointer_moved(state);
                    }
                    return;
                }
                // Client module nodes take a viewer's buttons first; a
                // replay's do nothing else.
                WindowEvent::MouseInput {
                    state: button_state,
                    button,
                    ..
                } => {
                    if !crate::app::ui_modules::pointer_button(state, *button, button_state.is_pressed())
                        && watching
                    {
                        crate::app::match_runtime::watching::mouse(
                            &state.platform,
                            &mut state.renderer,
                            &state.frontend,
                            &mut state.match_state,
                            &state.persistence,
                            *button,
                            button_state.is_pressed(),
                        );
                    }
                    return;
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => *y,
                        MouseScrollDelta::PixelDelta(position) => {
                            (position.y as f32 / 30.0).clamp(-3.0, 3.0)
                        }
                    };
                    crate::app::input::camera::apply_zoom(&mut state.match_state.input, lines);
                    return;
                }
                // A watcher's window closes as a player's does.
                WindowEvent::CloseRequested
                | WindowEvent::Resized(_)
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
                        Self::resize_surface_for_window_size(
                            &state.platform,
                            &mut state.renderer,
                            &state.diag,
                            &mut state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &state.persistence,
                            size,
                        );
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
                        Self::resize_surface_for_window_size(
                            &state.platform,
                            &mut state.renderer,
                            &state.diag,
                            &mut state.frontend,
                            &mut state.match_state,
                            &state.process_assets,
                            &mut state.audio,
                            &state.persistence,
                            size,
                        );
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
                Self::close_game(state, event_loop);
            }
            WindowEvent::Resized(size) => {
                // A zero dimension is how Windows reports a minimise — that
                // backend never sends `Occluded` — so the hidden flag is
                // derived here as well as from the occlusion event below.
                let hidden = size.width == 0
                    || size.height == 0
                    || state.platform.window.is_minimized().unwrap_or(false);
                Self::set_window_hidden(&mut state.platform, &mut state.frontend, hidden);
                // A minimise carries no usable client size: the surface cannot
                // be configured to 0x0 and the UI-scale heuristic would read a
                // zero viewport. The restore edge delivers the real size.
                if !hidden {
                    Self::resize_surface_for_window_size(
                        &state.platform,
                        &mut state.renderer,
                        &state.diag,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio,
                        &state.persistence,
                        size,
                    );
                    // A resized window changes the rectangle the cursor is
                    // clipped to, so re-apply the lock (no-op if it is off).
                    crate::app::input::cursor::apply_cursor_confinement(&mut state.platform);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                // A monitor DPI change starts a new physical coordinate frame.
                // The first subsequent cursor event must establish its anchor.
                state.match_state.input.reset_window_cursor_motion();
            }
            WindowEvent::Focused(active) => {
                #[cfg(target_os = "android")]
                if !active {
                    self.android_pointer = Default::default();
                }
                state.match_state.input.reset_window_cursor_motion();
                // The OS drops the cursor clip on alt-tab, so re-apply the
                // player's confinement choice whenever the window returns.
                if active {
                    crate::app::input::cursor::refresh_pointer_in_client(&mut state.platform);
                    crate::app::input::cursor::apply_cursor_confinement(&mut state.platform);
                    // Back from the folder, perhaps with a new mod in it.
                    Self::refresh_mods_page(
                        state.platform.game_config.as_ref(),
                        &mut state.frontend,
                    );
                }
                if !active {
                    // Give the arrow back at once: a backgrounded window may
                    // draw no frame that would.
                    crate::app::input::hardware_cursor::give_back_arrow(&mut state.platform);
                    // `release_in_game_pointer_state` also drops a touch stream
                    // that ended without an `Ended`, so no phantom finger turns
                    // the next tap into a two-finger pan.
                    crate::app::input::cursor::release_cursor_confinement(&mut state.platform);
                    Self::release_in_game_pointer_state(&mut state.match_state);
                    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
                    super::shell_mod_import::pointer_button(
                        &state.renderer,
                        &mut state.frontend,
                        false,
                    );
                    if let Some(browser) = state
                        .frontend
                        .skirmish_shell_state
                        .saved_seed_browser
                        .as_mut()
                    {
                        browser.reset_interaction();
                    }
                    if let Some(browser) =
                        state.frontend.skirmish_shell_state.setups_browser.as_mut()
                    {
                        browser.reset_interaction();
                    }
                    if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                        dialog.reset_interaction();
                    }
                    if state.frontend.lan.is_some() {
                        state.frontend.shell_controller.gui_mut().cancel_pointer();
                    }
                    // The login page's own buttons let go; an account
                    // popup over it keeps its press.
                    if let Some(crate::app::shell_wol::WolPage::Login(_)) =
                        state.frontend.wol.as_ref().map(|visit| &visit.page)
                    {
                        let dialog = crate::ui::wol_login::DIALOG;
                        state.frontend.shell_controller.drop_pointer_in(dialog);
                    }
                    if let Some(lobby) = state
                        .frontend
                        .wol
                        .as_mut()
                        .and_then(|visit| visit.lobby_mut())
                    {
                        lobby.ui.release_buttons();
                    }
                    if state.frontend.multiplayer_setup.is_some() {
                        crate::app::input::multiplayer_setup::drop_pointer(
                            &mut state.frontend,
                            &state.match_state,
                            false,
                        );
                    }
                    if let Some(dialog) = state.frontend.options_dialog.as_mut() {
                        dialog.shell_cancel_pointer_gesture();
                    }
                    // A shell slider holding the mouse loses it with the
                    // focus. Native keeps its hold bytes until the next move
                    // without the button releases them (`0x0061E3AF`).
                    if [
                        crate::ui::skirmish_shell::page::DIALOG,
                        crate::ui::skirmish_shell::random_map_page::DIALOG,
                    ]
                    .map(Some)
                    .contains(&state.frontend.shell_controller.top_id())
                    {
                        state.frontend.shell_controller.gui_mut().cancel_pointer();
                    }
                    Self::release_campaign_slider(&mut state.frontend.shell_controller);
                }
                Self::set_window_active(
                    &mut state.platform,
                    &state.renderer,
                    &state.diag,
                    &mut state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &state.persistence,
                    &mut state.audio,
                    active,
                );
            }
            WindowEvent::Occluded(occluded) => {
                // Fully covered on the backends that report it. Windows does
                // not; see the `Resized` arm above. A spawn test run's
                // deliberately invisible window keeps running as if shown.
                let invisible_spawn =
                    Self::invisible_spawn_window(&state.platform, &state.frontend);
                Self::set_window_hidden(
                    &mut state.platform,
                    &mut state.frontend,
                    occluded && !invisible_spawn,
                );
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
                if crate::app::input::cursor::hold_pointer_in_window(
                    &state.platform,
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                ) {
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
                crate::app::input::tooltips::on_button_event(&state.diag, &mut state.match_state);
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
                if !(event.state.is_pressed() && state.platform.ime.composing()) {
                    Self::route_input(state, Input::Key(&event));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // The hardware pointer is back: edge auto-scroll may read the
                // band again. A touch's synthetic move carries the parked
                // cursor instead and keeps the inhibit up.
                if !from_touch {
                    state.match_state.input.cursor_from_touch = false;
                }
                crate::app::state::set_cursor_from_window(
                    &state.platform,
                    &state.renderer,
                    &state.frontend.screen,
                    &mut state.match_state.input,
                    position.x as f32,
                    position.y as f32,
                );
                Self::route_input(state, Input::Move);
            }
            WindowEvent::MouseInput {
                state: edge,
                button,
                ..
            } => {
                // A click in the game locks the cursor again after Ctrl+Alt+L
                // freed it, when the Options box keeps it locked. The click
                // still reaches the game.
                if edge.is_pressed() {
                    crate::app::input::cursor::relock_cursor_on_click(
                        &mut state.platform,
                        &state.persistence.options_profile,
                    );
                }
                Self::route_input(state, Input::Button(button, edge));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y as f32 / 30.0).clamp(-3.0, 3.0),
                };
                Self::route_input(state, Input::Wheel(lines));
            }
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            WindowEvent::DroppedFile(path) => Self::mod_import_drop(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.process_assets,
                &state.persistence.options_profile,
                path,
            ),
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
        // For the performance panel's Ticks row.
        if let Some(state) = self.state.as_mut() {
            state.platform.frame_pacer.count_loop_turn();
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
            if Self::poll_network_goodbye(state, event_loop)
                || Self::close_after_graphics_loss(state, event_loop)
            {
                return;
            }
            Self::follow_window_full_screen(
                &mut state.platform,
                &mut state.frontend,
                &mut state.match_state.match_presentation,
                &mut state.persistence.options_profile,
            );
            // cursor lock: the OS drops the clip on focus loss /
            // Windows key, so re-assert the player's choice every frame while
            // the window is focused. Presentation-only, never part of the sim.
            crate::app::input::cursor::reassert_cursor_confinement(&mut state.platform);
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
            Self::update_saved_seed_browser_scroll(
                state.platform.game_config.as_ref(),
                &state.renderer,
                &mut state.frontend,
                &mut state.match_state,
                &mut state.process_assets,
                &mut state.audio.sfx_player,
                false,
            );
            Self::update_skirmish_setups_scroll(
                &state.platform.window,
                &state.renderer,
                &mut state.frontend,
                &state.match_state,
                &state.process_assets,
                &mut state.audio.sfx_player,
                false,
            );
            type Poll = fn(&mut AppState) -> Option<Instant>;
            let polls: &[Poll] = &[
                |state| {
                    crate::app::input::keyboard::poll_scroll_repeat(
                        &state.platform.window,
                        &mut state.frontend,
                    )
                },
                |state| {
                    crate::app::persistence::autosave::poll(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state,
                        &mut state.persistence,
                        &state.audio,
                        &state.diag,
                        &state.process_assets,
                    );
                    None
                },
                crate::app::shell_lan::poll,
                crate::app::shell_wol::poll,
                |state| {
                    crate::app::shell_network_setup::poll(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                    )
                },
                crate::app::input::multiplayer_setup::poll,
                |state| {
                    use crate::app::input::coop_score;
                    coop_score::active(&state.frontend)
                        .then(|| {
                            coop_score::poll(
                                &state.platform.window,
                                &mut state.frontend.shell_controller,
                            )
                        })
                        .flatten()
                },
                |state| {
                    crate::app::input::sound::poll_scroll_repeat(
                        &state.platform.window,
                        &mut state.match_state.match_presentation,
                    )
                },
                |state| {
                    Self::poll_route_page(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &mut state.audio.sfx_player,
                    )
                },
                |state| {
                    Self::poll_mod_files(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                        &state.persistence.options_profile,
                    )
                },
                #[cfg(any(
                    windows,
                    target_os = "macos",
                    target_os = "linux",
                    target_os = "android"
                ))]
                |state| {
                    Self::poll_mod_import(
                        &state.platform,
                        &state.renderer,
                        &state.diag,
                        &mut state.frontend,
                        &mut state.match_state,
                        &state.process_assets,
                        &mut state.audio,
                        &state.persistence,
                    )
                },
                |state| {
                    Self::poll_choose_map_scroll(
                        &state.platform.window,
                        &state.renderer,
                        &mut state.frontend,
                    )
                },
                |state| {
                    Self::poll_skirmish_opponent_scroll(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.persistence,
                    )
                },
                |state| {
                    Self::poll_language_files(
                        &state.platform,
                        &mut state.frontend,
                        &mut state.match_state,
                        &mut state.process_assets,
                    )
                },
                |state| Self::campaign_score_animation_deadline(&state.frontend),
            ];
            // Captures and hidden runs replay sealed input: they never read
            // the library, the login page's counts or the map pictures, no
            // box tells them of a mod, and they never check for an update.
            let live_polls: &[Poll] = &[
                |state| {
                    crate::app::map_library::poll(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.process_assets,
                    )
                },
                |state| crate::app::shell_wol::login_info::poll(&mut state.frontend),
                |state| {
                    crate::app::map_picture::poll(
                        &state.platform,
                        &mut state.frontend,
                        &state.persistence,
                    )
                },
                |state| {
                    crate::app::mods::poll(
                        &state.platform,
                        &state.renderer,
                        &mut state.frontend,
                        &state.match_state,
                        &state.process_assets,
                        &state.persistence,
                    )
                },
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                crate::app::updater::poll,
            ];
            let live = self.shell_capture.is_none() && self.hidden_run.is_none();
            polls
                .iter()
                .chain(if live { live_polls } else { &[] })
                .filter_map(|poll| poll(state))
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
            let wake = if Self::match_runs_while_hidden(
                &state.platform,
                &state.frontend,
                &state.match_state,
            ) {
                if Self::service_match_runtime(state, event_loop, true, true, false).quit_requested
                {
                    return;
                }
                let service = Instant::now() + crate::app::match_runtime::frame_pacer::SERVICE_WAKE;
                Some(shell_scroll_wake.map_or(service, |wake| wake.min(service)))
            } else {
                shell_scroll_wake
            };
            // Android's input method commits text without waking winit
            // (winit#4673), so keep polling while its keyboard is up.
            let flow = if crate::app::input::ime::keyboard_active(state) {
                ControlFlow::Poll
            } else {
                wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil)
            };
            event_loop.set_control_flow(flow);
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
                if let Some(animation_deadline) =
                    Self::campaign_score_animation_deadline(&state.frontend)
                {
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
            if Self::next_online_render_frame(
                &state.platform,
                &state.diag,
                &state.frontend,
                &state.match_state,
            )
            .is_none()
            {
                state.platform.frame_pacer.clear_render_continuation();
            }
            if Self::invisible_spawn_window(&state.platform, &state.frontend) {
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
                    ControlFlow::WaitUntil(Self::match_service_wake(
                        &state.platform.frame_pacer,
                        &state.frontend.screen,
                        &state.match_state,
                        Instant::now(),
                    ))
                });
            } else if Self::online_start_runs_while_hidden(&state.platform, &state.match_state) {
                // The relay is installed before its RTT barrier. Its IO
                // worker cannot advance the lobby, map or Ready on our behalf.
                crate::app::lan_service::poll(
                    state,
                    crate::app::match_runtime::frame_pacer::wall_clock_ms(),
                );
                Self::drive_shell_exit(state);
                crate::app::loading::pump::advance_hidden_network_loading(state);
                event_loop.set_control_flow(ControlFlow::WaitUntil(Self::match_service_wake(
                    &state.platform.frame_pacer,
                    &state.frontend.screen,
                    &state.match_state,
                    Instant::now(),
                )));
            } else if state.platform.frame_pacer.render_deferred() {
                event_loop.set_control_flow(ControlFlow::Poll);
                state.platform.window.request_redraw();
            } else if state.platform.window_hidden {
                if Self::match_runs_while_hidden(
                    &state.platform,
                    &state.frontend,
                    &state.match_state,
                ) {
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
                    event_loop.set_control_flow(ControlFlow::WaitUntil(Self::match_service_wake(
                        &state.platform.frame_pacer,
                        &state.frontend.screen,
                        &state.match_state,
                        Instant::now(),
                    )));
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
                            // The next frame's due time wakes the loop too,
                            // so its tick and picture come on time.
                            let frame_due = Self::next_local_frame_due(
                                &state.platform.frame_pacer,
                                &state.frontend.screen,
                                &state.match_state,
                                now,
                            );
                            let wake = state.platform.redraw_gate.probe_wake(now, frame_due);
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
            crate::app::mods::left(&mut state.frontend);
        }
        log::logger().flush();
        // A lost graphics device closed the game: say so once the replay is
        // written and the network left.
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        if let Some(state) = self.state.as_mut()
            && let Some(lost) = state.frontend.graphics_lost.take()
        {
            Self::leave_screen(&mut state.platform, &mut state.audio);
            crate::app::system_dialog::report_graphics_lost(&lost);
        }
        // A confirmed quit (its cascade's Theme Stop(0), 0x0052E7EC, has run)
        // or a page unload tears down a running game.
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        self.browser_exiting();
    }
}

#[cfg(all(test, tests_app))]
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
}
