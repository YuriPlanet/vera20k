//! Process-local platform lifecycle and frame-pacing state.

use std::sync::Arc;
use std::time::Instant;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::app::match_runtime::frame_pacer::LocalFramePacer;
use crate::ui::game_screen::GameScreen;

/// Loading uses tactical dimensions only for pending match-resource setup;
/// its own artwork uses the window directly. Frontend layout and hit testing
/// must never inherit a gameplay upscaler's source size.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_upscaling_never_changes_frontend_projection() {
        let window = (800, 600);
        let source = (640, 480);
        for screen in [
            GameScreen::MainMenu,
            GameScreen::MissionResult {
                title: String::new(),
                detail: String::new(),
            },
        ] {
            assert_eq!(render_dimensions(&screen, window, Some(source)), window);
        }
        for screen in [GameScreen::Loading, GameScreen::InGame] {
            assert_eq!(render_dimensions(&screen, window, Some(source)), source);
            assert_eq!(render_dimensions(&screen, window, None), window);
        }
    }
}

/// Window lifecycle and wall-clock pacing owned by the platform layer.
///
/// This state is process-local: it is never serialized, hashed, or read by the
/// deterministic simulation.
pub(crate) struct PlatformState {
    pub(crate) window: Arc<Window>,
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
    /// freezes the match is `focus_freeze_active`; input admission reads this
    /// flag directly.
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
    /// Monotonic epoch used only by the app-local gameplay-frame pacer.
    pub(crate) frame_pacer_epoch: Instant,
    /// Local wall-clock admission state. Never serialized or read by the sim.
    pub(crate) frame_pacer: LocalFramePacer,
    /// Loaded GameConfig — missing config.toml falls back to the executable
    /// root; `None` only when config loading or executable-root discovery
    /// fails. Set once at process start from `GameConfig::load()`; not
    /// mutated afterwards.
    pub(crate) game_config: Option<crate::util::config::GameConfig>,
    /// Effective shell client size for this process. Interactive launches use
    /// the independent retail frontend pair; sealed captures retain their explicit
    /// dimensions as the higher-priority automation projection.
    pub(crate) shell_client_size: PhysicalSize<u32>,
    /// Sealed captures keep their requested surface through match launch too.
    pub(crate) capture_client_size: Option<PhysicalSize<u32>>,
}

impl PlatformState {
    pub(crate) fn new(
        window: Arc<Window>,
        game_config: Option<crate::util::config::GameConfig>,
        shell_client_size: PhysicalSize<u32>,
        capture_client_size: Option<PhysicalSize<u32>>,
    ) -> Self {
        let pause_on_focus_loss = game_config
            .as_ref()
            .is_some_and(|config| config.gameplay.pause_on_focus_loss);
        Self {
            window,
            live_modifiers: winit::keyboard::ModifiersState::empty(),
            held_modifier_keys: Default::default(),
            window_active: true,
            pause_on_focus_loss,
            window_hidden: false,
            frame_pacer_epoch: Instant::now(),
            frame_pacer: LocalFramePacer::new(),
            game_config,
            shell_client_size,
            capture_client_size,
        }
    }

    /// Is the match frozen because the window is not the foreground?
    ///
    /// True only while the window is deactivated and `pause_on_focus_loss` is
    /// configured; the single gate for the simulation, the message clock and
    /// the focus-edge audio stop.
    pub(crate) fn focus_freeze_active(&self) -> bool {
        self.pause_on_focus_loss && !self.window_active
    }
}
