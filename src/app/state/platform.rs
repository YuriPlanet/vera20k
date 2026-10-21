//! Process-local platform lifecycle and frame-pacing state.

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::app::match_runtime::frame_pacer::LocalFramePacer;
use crate::ui::game_screen::GameScreen;

/// Loading uses tactical dimensions only for pending match-resource setup;
/// its own artwork uses the window directly. Frontend layout and hit testing
/// must never inherit the match zoom's source size.
pub(super) fn render_dimensions(
    screen: &GameScreen,
    window: (u32, u32),
    tactical_source: Option<(u32, u32)>,
) -> (u32, u32) {
    match screen {
        GameScreen::MainMenu | GameScreen::MissionResult { .. } => window,
        GameScreen::Loading | GameScreen::InGame => tactical_source.unwrap_or(window),
    }
}

/// Window lifecycle and wall-clock pacing owned by the platform layer.
///
/// This state is process-local: it is never serialized, hashed, or read by the
/// deterministic simulation.
pub(crate) struct PlatformState {
    pub(crate) window: Arc<Window>,
    /// Whether the OS cursor is clipped to the window rectangle
    /// edge-scroll lock), so it cannot leave the game while the player scrolls
    /// the map. On by default, by default; the cursor-lock hotkey flips it,
    /// and `apply_cursor_confinement` re-applies it on every focus regain because
    /// the OS drops the clip on alt-tab. Presentation-only — never serialized or
    /// read by the deterministic simulation.
    pub(crate) cursor_confined: bool,
    /// Whether the last confinement call actually holds the OS pointer in the
    /// client area: `ClipCursor` on Windows, an accepted Confined grab
    /// elsewhere. winit 0.30.12 refuses Confined on macOS and pure Wayland, so
    /// there the pointer stays free while `cursor_confined` is on. Only
    /// `input::cursor`'s confinement calls write it. Presentation-only.
    pub(crate) pointer_held: std::cell::Cell<bool>,
    /// Whether the OS pointer is inside the window's client area, from winit's
    /// enter/leave edges. Outside it the system owns the pointer — the title bar
    /// and resize border need the system arrow, which `DefWindowProc` installs
    /// for every non-client `WM_SETCURSOR` hit — so VERA draws no mouse.shp
    /// there and asks for a visible OS cursor. A clipped (`cursor_confined`)
    /// pointer cannot leave, so this only moves while the lock is off.
    /// Presentation-only — never serialized or read by the deterministic
    /// simulation.
    pub(crate) pointer_in_client: bool,
    /// The last visibility handed to `Window::set_cursor_visible`, so a steady
    /// frame does not re-issue it. winit's call is not free on Windows: it runs
    /// `refresh_os_cursor`, which clears any OS cursor clip winit does not itself
    /// own, so repeating it per frame would tear `cursor_confined` down every
    /// frame. `show_os_cursor_only_without_game_cursor` is the only writer and
    /// re-asserts the clip after each transition. Presentation-only.
    pub(crate) os_cursor_visible: Option<bool>,
    /// Live message-pump modifiers for focused shell controls. Gameplay keeps
    /// its separate paused-input admission snapshot.
    pub(crate) live_modifiers: winit::keyboard::ModifiersState,
    /// The physical modifier keys held, the source both modifier views are
    /// derived from (see `HeldModifierKeys`).
    pub(crate) held_modifier_keys: crate::app::input::hotkeys::HeldModifierKeys,
    /// Whether this application currently owns the foreground.
    ///
    /// gamemd tracks the same edge-triggered byte from `WM_ACTIVATEAPP` and
    /// parks its main tick in a sleep-and-network-only loop while it is clear:
    /// the frame counter, input, AI, map logic and per-tick update all stop.
    /// Only the message pump keeps running. Starts true — a window that never
    /// reports an activation edge must keep running. Whether a clear flag
    /// freezes the match is `focus_freeze_active`; input admission and the
    /// `WM_ACTIVATEAPP` audio-output gate read this flag directly.
    pub(crate) window_active: bool,
    /// `[gameplay] pause_on_focus_loss`: whether a deactivated window freezes
    /// the match the way native does. Off, the match runs on in the background.
    pub(crate) pause_on_focus_loss: bool,
    /// Whether the window has no visible surface — minimised, or occluded on
    /// the platforms that report occlusion.
    ///
    /// Windows never emits `WindowEvent::Occluded` (winit only raises it from
    /// the iOS, X11, macOS and Web backends); a minimise arrives there as a
    /// zero-sized `Resized` instead. Both signals feed this flag, so the redraw
    /// loop parks on every platform. Presentation-only.
    pub(crate) window_hidden: bool,
    /// Local wall-clock admission state. Never serialized or read by the sim.
    pub(crate) frame_pacer: LocalFramePacer,
    /// Loaded GameConfig — missing config.toml falls back to the executable
    /// root; `None` only when config loading or executable-root discovery
    /// fails. Set once at process start from `GameConfig::load()`; not
    /// mutated afterwards.
    pub(crate) game_config: Option<crate::util::config::GameConfig>,
    /// A sealed capture's exact surface. Present only in capture runs, which
    /// keep it through match launch and never zoom or go full screen.
    pub(crate) capture_client_size: Option<PhysicalSize<u32>>,
}

impl PlatformState {
    pub(crate) fn new(
        window: Arc<Window>,
        game_config: Option<crate::util::config::GameConfig>,
        capture_client_size: Option<PhysicalSize<u32>>,
    ) -> Self {
        let pause_on_focus_loss = game_config
            .as_ref()
            .is_some_and(|config| config.gameplay.pause_on_focus_loss);
        Self {
            window,
            cursor_confined: true,
            pointer_held: std::cell::Cell::new(false),
            pointer_in_client: true,
            os_cursor_visible: None,
            live_modifiers: winit::keyboard::ModifiersState::empty(),
            held_modifier_keys: Default::default(),
            window_active: true,
            pause_on_focus_loss,
            window_hidden: false,
            frame_pacer: LocalFramePacer::new(),
            game_config,
            capture_client_size,
        }
    }

    /// Is the match frozen because the window is not the foreground?
    ///
    /// True only while the window is deactivated and `pause_on_focus_loss` is
    /// configured; the single gate for the simulation and the message clock.
    /// The audio stop is deliberately not part of it: gamemd gates its primary
    /// output on the activation edge itself, so that gate follows
    /// [`Self::window_active`] alone.
    pub(crate) fn focus_freeze_active(&self) -> bool {
        self.pause_on_focus_loss && !self.window_active
    }
}
