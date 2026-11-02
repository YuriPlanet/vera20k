//! Redraw scheduling for a visible window whose picture stands still.
//!
//! Offline, a picture that did not change was still drawn on every display
//! refresh: the in-game menu pages over a frozen match, and the battlefield
//! between game frames, kept the GPU busy (issue #3605). After a present
//! whose picture can stand, `about_to_wait` probes instead of redrawing: the
//! probe runs the ordinary pass (match service, audio and presentation
//! timers) and stops short of drawing while the picture's inputs are
//! unchanged, so every drawn frame still follows exactly one service. Window
//! events and explicit `request_redraw` calls still draw.
//!
//! The battlefield stands only while nothing on it moves by itself. Effects
//! that step inside the tactical composite or read its clock (electric
//! bolts, trails, radiation beams, laser jitter, airstrike flares,
//! mind-control links, psychic lines, planning dashes, superweapon range
//! pulses, flying credits) move once per Main_Tick
//! ([`crate::app::presentation::tactical_composite`]), so the picture keys on
//! the tick count. Owners stepped once per pass or once per draw after a
//! wall-clock interval (zoom easing, radar housing and movies, the power bar,
//! the chrono screen, message typing) keep every refresh drawn until they
//! settle; the cursor animation and superweapon timer blinks draw again once
//! their deadline passes.

use crate::app::AppState;
use crate::app::input::precedence::ScreenPage;
use crate::ui::game_screen::GameScreen;
use crate::ui::pause_menu::InGameMenuState;

/// The wall clock a pass's picture is judged by. It is read once, before the
/// probe check and the draw: a deadline that passes during the draw then
/// costs one extra draw instead of leaving a stale picture.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PictureClock {
    wall_ms: u64,
}

impl PictureClock {
    pub(crate) fn now() -> Self {
        Self {
            wall_ms: super::frame_pacer::wall_clock_ms(),
        }
    }
}

/// The picture the current pass would draw, when it can stay on screen.
///
/// An offline in-game menu page covers the whole window and changes only on
/// input, a page switch, a world change, a tooltip, the Save page's blinking
/// caret or the screen shake (`shake_px`, the offset this pass draws). The
/// offline battlefield also changes with the camera, the sidebar view, the
/// message rows, the published detail frame rate and the talk bubble, and
/// stands only while none of its self-moving owners runs (module docs).
/// `None` keeps every refresh drawn: captures, modals, other screens,
/// network and replay matches, exits and loads, the FPS overlay, a held
/// mouse button (gadgets repeat on it) and a shake still decaying.
pub(crate) fn still_frame(
    state: &AppState,
    capture: bool,
    shake_px: Option<[i32; 2]>,
    clock: PictureClock,
) -> Option<StillFrame> {
    let (modal, page) = crate::app::input::precedence::presented(state);
    let presentation = &state.match_state.match_presentation;
    let input = &state.match_state.input;
    let simulation = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|runtime| &runtime.simulation);
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
        && !simulation.is_some_and(|sim| {
            presentation
                .screen_shake
                .decays_at(sim.session().binary_frame())
        });
    if !still {
        return None;
    }
    let revision = simulation.map(|sim| sim.revision());
    let tooltip = presentation
        .tooltips
        .active()
        .map(|tip| (tip.id, tip.x, tip.y));
    match page {
        ScreenPage::InGameShell(menu) => Some(StillFrame::Menu {
            menu,
            revision,
            shake_px,
            tooltip,
            caret: matches!(menu, InGameMenuState::SavedGame(_))
                && presentation
                    .saved_game_browser
                    .as_ref()
                    .is_some_and(|browser| {
                        browser.description_edit.focused && browser.caret_blink_on()
                    }),
        }),
        ScreenPage::Battlefield => {
            let detail = presentation.detail.borrow();
            let moving = state.match_state.paused()
                || !crate::app::input::camera::zoom_settled(state)
                || presentation.interpolation.readout().sampling
                || presentation
                    .radar_anim
                    .as_ref()
                    .is_some_and(|radar| radar.housing_moving())
                || crate::app::radar_movie::has_pending(state)
                || presentation.chrono_screen.active()
                || presentation.power_bar_anim.in_motion()
                || presentation.message_list.revealing()
                || detail.threshold_wraps()
                || (state.use_software_cursor() && crate::app::input::cursor::animation_step_due())
                || crate::app::presentation::super_timers::blink_due(state);
            (!moving).then(|| StillFrame::Battlefield {
                revision,
                composite: presentation.tactical_composite.ticks(),
                camera: [input.camera_x, input.camera_y, input.zoom_level].map(f32::to_bits),
                shake_px,
                tooltip,
                sidebar_view: presentation.sidebar_projection.view_revision(),
                messages: presentation.message_list.messages().len(),
                detail_fps: detail.frame_rate(),
                talk_bubble: presentation.talk_bubble.shown(clock.wall_ms),
            })
        }
        _ => None,
    }
}

/// The inputs a still picture is drawn from: two passes with equal values
/// draw the same pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StillFrame {
    /// An offline in-game menu page.
    Menu {
        menu: InGameMenuState,
        /// The world's [`crate::sim::world::Simulation::revision`].
        revision: Option<u64>,
        /// The screen shake offset drawn, in screen pixels.
        shake_px: Option<[i32; 2]>,
        /// The shown tooltip's region and anchor.
        tooltip: Option<(u32, i32, i32)>,
        /// Whether the Save page draws its name caret.
        caret: bool,
    },
    /// The offline battlefield with its sidebar and HUD.
    Battlefield {
        revision: Option<u64>,
        /// The Main_Ticks counted: each owes a tactical composite.
        composite: u64,
        /// Camera x and y, in world pixels, and the zoom, as bits.
        camera: [u32; 3],
        shake_px: Option<[i32; 2]>,
        tooltip: Option<(u32, i32, i32)>,
        /// The sidebar view's revision.
        sidebar_view: u64,
        /// The message rows shown; each new or expired row changes it.
        messages: usize,
        /// The published detail frame rate (lasers, beams, particles).
        detail_fps: u32,
        /// Whether the talk bubble shows.
        talk_bubble: bool,
    },
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
    audit: Option<GateAudit>,
}

/// The gate's consistency check, on with `VERA20K_REDRAW_GATE_AUDIT=1`:
/// every present is read back, and each pass the gate would skip is drawn
/// anyway and must match the picture on screen pixel for pixel. A mismatch
/// and a running count go to the log.
#[derive(Debug, Default)]
struct GateAudit {
    /// The last present's BGRA8 pixels.
    pixels: Vec<u8>,
    /// Skipped passes drawn for the check, and how many of them differed.
    checked: u64,
    differed: u64,
}

impl RedrawGate {
    /// The gate, with its audit when `VERA20K_REDRAW_GATE_AUDIT=1`.
    pub(crate) fn from_environment() -> Self {
        let audit = std::env::var("VERA20K_REDRAW_GATE_AUDIT").is_ok_and(|value| value == "1");
        Self {
            audit: audit.then(GateAudit::default),
            ..Self::default()
        }
    }

    /// Whether skipped passes are drawn and every present read back.
    pub(crate) fn auditing(&self) -> bool {
        self.audit.is_some()
    }

    /// Check a present's pixels. `skipped` marks a pass the gate would have
    /// skipped: its picture must equal the last present's.
    pub(crate) fn audit_presented(&mut self, pixels: Vec<u8>, skipped: bool) {
        let Some(audit) = self.audit.as_mut() else {
            return;
        };
        if skipped {
            audit.checked += 1;
            if pixels != audit.pixels {
                audit.differed += 1;
                let changed = pixels
                    .chunks_exact(4)
                    .zip(audit.pixels.chunks_exact(4))
                    .filter(|(now, before)| now != before)
                    .count();
                log::warn!(
                    "redraw gate audit: a skipped pass drew {changed} changed pixels over {:?}",
                    self.presented
                );
            }
            if audit.checked.is_power_of_two() || audit.checked % 4096 == 0 {
                log::info!(
                    "redraw gate audit: {} skipped passes drawn, {} differed",
                    audit.checked,
                    audit.differed
                );
            }
        }
        audit.pixels = pixels;
    }

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

    fn menu(revision: u64, caret: bool, shake: [i32; 2]) -> StillFrame {
        StillFrame::Menu {
            menu: InGameMenuState::Menu,
            revision: Some(revision),
            shake_px: Some(shake),
            tooltip: None,
            caret,
        }
    }

    fn battlefield(revision: u64, camera_x: f32, sidebar_view: u64, bubble: bool) -> StillFrame {
        StillFrame::Battlefield {
            revision: Some(revision),
            composite: revision,
            camera: [camera_x, 240.0, 1.0].map(f32::to_bits),
            shake_px: Some([0, 0]),
            tooltip: None,
            sidebar_view,
            messages: 2,
            detail_fps: 15,
            talk_bubble: bubble,
        }
    }

    #[test]
    fn a_still_menu_is_probed_until_input_arrives() {
        let mut gate = RedrawGate::default();
        assert_eq!(gate.next_pass(), NextPass::Draw);
        gate.record_presented(Some(menu(120, false, [0, 0])));
        assert_eq!(gate.next_pass(), NextPass::Probe);
        assert!(gate.still_presented(Some(menu(120, false, [0, 0]))));
        gate.note_input();
        assert_eq!(gate.next_pass(), NextPass::Draw);
        assert!(!gate.still_presented(Some(menu(120, false, [0, 0]))));
    }

    #[test]
    fn a_world_change_caret_blink_or_shake_step_draws() {
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(menu(120, false, [0, 0])));
        for changed in [
            menu(121, false, [0, 0]),
            menu(120, true, [0, 0]),
            menu(120, false, [-1, 0]),
        ] {
            assert!(!gate.still_presented(Some(changed)));
        }
        assert!(!gate.still_presented(None));
        gate.record_presented(None);
        assert_eq!(gate.next_pass(), NextPass::Draw);
    }

    /// A standing battlefield is probed; a world change, a scroll, a new
    /// sidebar view or a talk bubble running out draws, and a draw that
    /// showed motion (recorded as `None`) keeps every refresh drawn.
    #[test]
    fn a_still_battlefield_draws_on_world_camera_view_or_bubble_changes() {
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(battlefield(120, 480.0, 7, true)));
        assert_eq!(gate.next_pass(), NextPass::Probe);
        assert!(gate.still_presented(Some(battlefield(120, 480.0, 7, true))));
        for changed in [
            battlefield(121, 480.0, 7, true),
            battlefield(120, 481.0, 7, true),
            battlefield(120, 480.0, 8, true),
            battlefield(120, 480.0, 7, false),
            menu(120, false, [0, 0]),
        ] {
            assert!(!gate.still_presented(Some(changed)));
        }
        gate.record_presented(None);
        assert_eq!(gate.next_pass(), NextPass::Draw);
    }

    /// The audit counts a skipped pass whose pixels differ from the last
    /// present's, and only a skipped one.
    #[test]
    fn the_audit_flags_a_skipped_pass_that_drew_other_pixels() {
        let mut gate = RedrawGate {
            audit: Some(GateAudit::default()),
            ..RedrawGate::default()
        };
        gate.audit_presented(vec![1, 2, 3, 4], false);
        gate.audit_presented(vec![1, 2, 3, 4], true);
        gate.audit_presented(vec![9, 2, 3, 4], false);
        gate.audit_presented(vec![1, 2, 3, 4], true);
        let audit = gate.audit.as_ref().unwrap();
        assert_eq!((audit.checked, audit.differed), (2, 1));
    }
}
