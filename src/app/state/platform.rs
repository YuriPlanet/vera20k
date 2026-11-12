//! Process-local platform lifecycle and frame-pacing state.

use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::{UserAttentionType, Window};

use crate::app::frontend::startup_options::ScreenSize;
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
    /// the map. Off until startup copies the player's saved `LockCursor`
    /// choice (off by default); the cursor-lock hotkey flips it,
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
    /// frame. `input::hardware_cursor` issues it and re-asserts the clip after
    /// each transition. Presentation-only.
    pub(crate) os_cursor_visible: Option<bool>,
    /// What the window system shows at the pointer.
    pub(crate) hardware_cursor: crate::app::input::hardware_cursor::HardwareCursor,
    /// Live message-pump modifiers for focused shell controls. Gameplay keeps
    /// its separate paused-input admission snapshot.
    pub(crate) live_modifiers: winit::keyboard::ModifiersState,
    /// The physical modifier keys held, the source both modifier views are
    /// derived from (see `HeldModifierKeys`).
    pub(crate) held_modifier_keys: crate::app::input::hotkeys::HeldModifierKeys,
    /// The input method's state for the chat edits (`app::input::ime`).
    pub(crate) ime: crate::app::input::ime::ImeState,
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
    /// Whether [`Self::request_attention`] asked for the player's attention
    /// since the window was last active, so the regain edge clears it: X11
    /// keeps its urgency hint until then. Presentation-only.
    pub(crate) attention_requested: bool,
    /// Local wall-clock admission state. Never serialized or read by the sim.
    pub(crate) frame_pacer: LocalFramePacer,
    /// Whether the next visible pass must draw, or may only service the
    /// match because the presented picture still stands. Presentation-only.
    pub(crate) redraw_gate: crate::app::match_runtime::redraw_gate::RedrawGate,
    /// Loaded GameConfig — missing config.toml falls back to the executable
    /// root; `None` only when config loading or executable-root discovery
    /// fails. Set once at process start from `GameConfig::load()`; not
    /// mutated afterwards.
    pub(crate) game_config: Option<crate::util::config::GameConfig>,
    /// A sealed capture's exact surface. Present only in capture runs, which
    /// keep it through match launch and never go full screen.
    pub(crate) capture_client_size: Option<PhysicalSize<u32>>,
    /// A capture's battle zoom and sidebar size in place of the Options
    /// choices; read only while `capture_client_size` is set.
    pub(crate) capture_presentation: CapturePresentation,
    /// Whether the windowed window still has to be put on the Display choice's
    /// screen. Set when the window comes back from full screen, because winit
    /// restores the placement it saved on entry and does that on its own
    /// thread, so the choice is re-asserted over the next passes until the
    /// window is really there. Presentation-only, never serialized.
    pub(crate) recenter_windowed: bool,
    /// The size a launch opens the player's window at, in logical units:
    /// `player_launch_window_size` of gamemd's startup size
    /// (`RetailOptionsLoad::startup_shell_screen`), 1024x768 by default. The
    /// Esc menu's Set Window Size button and Full Screen going off go back to
    /// it. Presentation-only.
    pub(crate) launch_window_size: ScreenSize,
    /// Whether that button is still waiting for the window to come out of full
    /// screen. Presentation-only, never serialized.
    pub(crate) launch_window_owed: bool,
}

/// The Options presentation choices a capture runs its battle at. A sealed
/// map profile may set them (`MapPresentationProfile`); otherwise a capture
/// keeps zoom 100%, the Normal sidebar, the native radar and the sidebar's
/// own pixels, whatever its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CapturePresentation {
    pub(crate) zoom: crate::ui::main_menu_dialogs::options::Zoom,
    pub(crate) sidebar: crate::ui::main_menu_dialogs::options::SidebarSize,
    pub(crate) hd_radar: bool,
    pub(crate) smooth_sidebar: bool,
    /// A map picture (`app::map_picture`) draws the battlefield alone,
    /// without the build description over it, and its camera goes past the
    /// playable area's edges.
    pub(crate) map_picture: bool,
}

impl Default for CapturePresentation {
    fn default() -> Self {
        Self {
            zoom: crate::ui::main_menu_dialogs::options::Zoom::Percent(100),
            sidebar: crate::ui::main_menu_dialogs::options::SidebarSize::Normal,
            hd_radar: false,
            smooth_sidebar: false,
            map_picture: false,
        }
    }
}

impl PlatformState {
    pub(crate) fn new(
        window: Arc<Window>,
        game_config: Option<crate::util::config::GameConfig>,
        capture_client_size: Option<PhysicalSize<u32>>,
        launch_window_size: ScreenSize,
    ) -> Self {
        let pause_on_focus_loss = game_config
            .as_ref()
            .is_some_and(|config| config.gameplay.pause_on_focus_loss);
        Self {
            window,
            cursor_confined: false,
            pointer_held: std::cell::Cell::new(false),
            pointer_in_client: true,
            os_cursor_visible: None,
            hardware_cursor: Default::default(),
            live_modifiers: winit::keyboard::ModifiersState::empty(),
            held_modifier_keys: Default::default(),
            ime: Default::default(),
            window_active: true,
            pause_on_focus_loss,
            window_hidden: false,
            attention_requested: false,
            frame_pacer: LocalFramePacer::new(),
            redraw_gate: crate::app::match_runtime::redraw_gate::RedrawGate::from_environment(),
            game_config,
            capture_client_size,
            capture_presentation: CapturePresentation::default(),
            recenter_windowed: false,
            launch_window_size,
            launch_window_owed: false,
        }
    }

    /// True while the window is deactivated and `pause_on_focus_loss` is
    /// configured; [`super::AppState::focus_freeze_active`] adds the session
    /// gate and is the single gate for the simulation and the message clock.
    /// The audio stop is deliberately not part of it: gamemd gates its primary
    /// output on the activation edge itself, so that gate follows
    /// [`Self::window_active`] alone.
    pub(crate) fn focus_freeze_configured(&self) -> bool {
        self.pause_on_focus_loss && !self.window_active
    }

    /// Whether the game owns the pointer and draws mouse.shp for it: the
    /// pointer is over the client area of the active window. Over an inactive
    /// window the system draws its own arrow there (macOS applies a window's
    /// cursor only while it is key), so mouse.shp beside it would be a second
    /// cursor. Presentation-only.
    pub(crate) fn owns_pointer(&self) -> bool {
        self.pointer_in_client && self.window_active
    }
    /// VERA's own (gamemd never flashes its window): while another
    /// application is in front, flash the taskbar button (Windows), bounce
    /// the Dock icon (macOS) or mark the window urgent (X11, Wayland), so a
    /// player waiting in the background sees that their game needs them.
    /// `until_back` keeps the Dock icon bouncing until the player returns;
    /// otherwise it bounces once. Presentation-only.
    pub(crate) fn request_attention(&mut self, until_back: bool) {
        if self.window_active {
            return;
        }
        let kind = if until_back {
            UserAttentionType::Critical
        } else {
            UserAttentionType::Informational
        };
        log::info!("Asked for the player's attention ({kind:?})");
        self.window.request_user_attention(Some(kind));
        self.attention_requested = true;
    }

    /// The window is active again: drop a request the window system keeps.
    pub(crate) fn clear_attention(&mut self) {
        if std::mem::take(&mut self.attention_requested) {
            self.window.request_user_attention(None);
        }
    }
}
