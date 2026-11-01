//! Redraw scheduling for a visible window while an in-game menu page stands
//! still.
//!
//! Offline, the match is frozen behind the in-game menu pages and their
//! pictures change only on input, yet every display refresh drew the whole
//! shell again, which kept the GPU busy behind a paused menu (issue #3605).
//! After a present whose picture can stand, `about_to_wait` probes instead of
//! redrawing: the probe runs the ordinary pass (match service, audio and
//! presentation timers) and stops short of drawing while the picture's inputs
//! are unchanged, so every drawn frame still follows exactly one service.
//! Window events and explicit `request_redraw` calls still draw.
//!
//! The battlefield keeps a redraw per refresh: several of its effects step
//! once per draw (electric bolts, trails, beams, laser jitter) or read the
//! wall clock while drawing (control links, superweapon timers).

use crate::app::AppState;
use crate::app::input::precedence::ScreenPage;
use crate::ui::game_screen::GameScreen;
use crate::ui::pause_menu::InGameMenuState;

/// The picture the current pass would draw, when it can stay on screen.
///
/// Only an offline in-game menu page can stand. Its shell covers the whole
/// window and changes only on input, a page switch, a game frame, a tooltip,
/// the Save page's blinking caret or the screen shake (`shake_px`, the
/// offset this pass draws). `None` keeps every refresh drawn: captures,
/// modals, the battlefield, network and replay matches, exits and loads, the
/// FPS overlay, a held mouse button (gadgets repeat on it) and a shake still
/// decaying.
pub(crate) fn still_frame(
    state: &AppState,
    capture: bool,
    shake_px: Option<[i32; 2]>,
) -> Option<StillFrame> {
    let (modal, page) = crate::app::input::precedence::presented(state);
    let ScreenPage::InGameShell(menu) = page else {
        return None;
    };
    let presentation = &state.match_state.match_presentation;
    let input = &state.match_state.input;
    let sim_frame = state.match_state.sim_runtime.as_ref().map(|runtime| {
        let session = runtime.simulation.session();
        (session.tick(), session.binary_frame())
    });
    let still = !capture
        && modal.is_none()
        && state.frontend.screen == GameScreen::InGame
        && state.match_state.live_network.is_none()
        && !state.match_state.match_diagnostics.is_replay_playback()
        && !state
            .match_state
            .match_diagnostics
            .actor_presentation_observer_active()
        && state.persistence.pending_load.is_none()
        && state.frontend.quit_cascade.is_none()
        && state.match_state.scenario_exit.is_none()
        && state.match_state.scenario_outcome.is_none()
        && !crate::app::App::house_outcome_wait_pending(state)
        && !state.renderer.fps_overlay.is_visible()
        && !input.tactical_mouse.left_held()
        && !input.tactical_mouse.right_held()
        && !sim_frame.is_some_and(|(_, frame)| presentation.screen_shake.decays_at(frame));
    still.then(|| StillFrame {
        menu,
        sim_frame,
        shake_px,
        tooltip: presentation
            .tooltips
            .active()
            .map(|tip| (tip.id, tip.x, tip.y)),
        caret: matches!(menu, InGameMenuState::SavedGame(_))
            && presentation
                .saved_game_browser
                .as_ref()
                .is_some_and(|browser| {
                    browser.description_edit.focused && browser.caret_blink_on()
                }),
    })
}

/// The inputs a still menu page is drawn from: two passes with equal values
/// draw the same pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StillFrame {
    pub(crate) menu: InGameMenuState,
    /// The simulation's (tick, binary frame).
    pub(crate) sim_frame: Option<(u64, u32)>,
    /// The screen shake offset drawn, in screen pixels.
    pub(crate) shake_px: Option<[i32; 2]>,
    /// The shown tooltip's region and anchor.
    pub(crate) tooltip: Option<(u32, i32, i32)>,
    /// Whether the Save page draws its name caret.
    pub(crate) caret: bool,
}

/// What `about_to_wait` should run next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NextPass {
    /// Draw and present, as on every refresh before.
    Draw,
    /// Service the match; draw in the same pass only if the picture changed.
    Probe,
}

#[derive(Debug, Default)]
pub(crate) struct RedrawGate {
    /// A window event arrived since the last present.
    input_since_present: bool,
    /// The last presented picture, when it may stay on screen.
    presented: Option<StillFrame>,
}

impl RedrawGate {
    /// Any window event may change the next picture.
    pub(crate) fn note_input(&mut self) {
        self.input_since_present = true;
    }

    /// Record a present. `still` is `None` while the picture may change
    /// without input, which keeps every refresh drawn.
    pub(crate) fn record_presented(&mut self, still: Option<StillFrame>) {
        self.input_since_present = false;
        self.presented = still;
    }

    pub(crate) fn next_pass(&self) -> NextPass {
        if self.presented.is_some() && !self.input_since_present {
            NextPass::Probe
        } else {
            NextPass::Draw
        }
    }

    /// After a probe's service: does the presented picture still stand?
    pub(crate) fn still_presented(&self, current: Option<StillFrame>) -> bool {
        !self.input_since_present && current.is_some() && current == self.presented
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paused_menu() -> StillFrame {
        StillFrame {
            menu: InGameMenuState::Menu,
            sim_frame: Some((120, 120)),
            shake_px: Some([0, 0]),
            tooltip: None,
            caret: false,
        }
    }

    #[test]
    fn a_still_menu_is_probed_until_input_arrives() {
        let mut gate = RedrawGate::default();
        assert_eq!(gate.next_pass(), NextPass::Draw);
        gate.record_presented(Some(paused_menu()));
        assert_eq!(gate.next_pass(), NextPass::Probe);
        assert!(gate.still_presented(Some(paused_menu())));
        gate.note_input();
        assert_eq!(gate.next_pass(), NextPass::Draw);
        assert!(!gate.still_presented(Some(paused_menu())));
    }

    #[test]
    fn a_game_frame_caret_blink_or_shake_step_draws() {
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(paused_menu()));
        let next_frame = StillFrame {
            sim_frame: Some((121, 121)),
            ..paused_menu()
        };
        let caret = StillFrame {
            caret: true,
            ..paused_menu()
        };
        let shaken = StillFrame {
            shake_px: Some([-1, 0]),
            ..paused_menu()
        };
        for changed in [next_frame, caret, shaken] {
            assert!(!gate.still_presented(Some(changed)));
        }
        assert!(!gate.still_presented(None));
        gate.record_presented(None);
        assert_eq!(gate.next_pass(), NextPass::Draw);
    }
}
