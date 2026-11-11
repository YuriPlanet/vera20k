//! Redraw scheduling for a visible window.
//!
//! Offline, a picture that did not change was still drawn on every display
//! refresh: the in-game menu pages over a frozen match, and the battlefield
//! between game frames, kept the GPU busy (issue #3605). After a present
//! whose picture can stand, `about_to_wait` probes instead of redrawing: the
//! probe runs the ordinary pass (match service, audio and presentation
//! timers) and stops short of drawing while the picture's inputs are
//! unchanged, so every drawn frame still follows exactly one service. Window
//! events draw at once.
//!
//! A picture that does change is limited as well (issue #4032): a draw on
//! every refresh is more than a weak GPU can carry. Each present leaves
//! [`DrawLimits`] for the draws after it, and explicit `request_redraw` calls
//! follow them too, because most shell pages ask for the next draw from the
//! end of each one. Pointer motion alone waits longer: the window system
//! draws the cursor (`input::hardware_cursor`), so a move only changes what
//! the pointer hovers, which gamemd also shows once per game frame. While a
//! draw waits the loop keeps probing, so the match service holds its cadence.
//!
//! The battlefield stands only while nothing on it moves by itself. Effects
//! that step inside the tactical composite or read its clock (electric
//! bolts, trails, radiation beams, laser jitter, airstrike flares,
//! mind-control links, psychic lines, planning dashes, superweapon range
//! pulses, flying credits) move once per Main_Tick
//! ([`crate::app::presentation::tactical_composite`]), so the picture keys on
//! the tick count. Owners stepped once per pass or once per draw after a
//! wall-clock interval (zoom easing, radar housing and movies, the power bar,
//! the chrono screen, message typing) keep drawing until they settle; the
//! cursor animation and superweapon timer blinks draw again once their
//! deadline passes.
//!
//! A network match composites on every draw, as ThrottleFrame renders while
//! it waits, so its battlefield stands between frames only while the last
//! composite held none of those effects or lines
//! ([`crate::app::presentation::tactical_composite::TacticalComposite::animated`])
//! and no recovery line, which reads the wall clock, shows.

use super::frame_pacer::{FRAME_BUCKET, SERVICE_WAKE};
use crate::app::AppState;
use crate::app::input::precedence::ScreenPage;
use crate::ui::game_screen::GameScreen;
use crate::ui::pause_menu::InGameMenuState;
use crate::util::time::{Duration, Instant};

/// The shortest time between two draws of a changing picture: about 62 draws
/// a second, which a 60 Hz screen never reaches.
const PICTURE_INTERVAL: Duration = FRAME_BUCKET;

/// The same on a menu or lobby page, about 33 draws a second. The pages' own
/// timers step every 30 ms (`ui::shell::slide::WAVE_TICK_MS`,
/// `ui::shell::static_reveal::KIND1_TIMER_INTERVAL`), so every step still gets
/// its draw.
const MENU_INTERVAL: Duration = Duration::from_millis(30);

/// The wall clock a pass's picture is judged by. It is read once, before the
/// probe check and the draw: a deadline that passes during the draw then
/// costs one extra draw instead of leaving a stale picture.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PictureClock {
    pub(crate) at: Instant,
    wall_ms: u64,
}

impl PictureClock {
    pub(crate) fn now() -> Self {
        Self {
            at: Instant::now(),
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
/// message rows, the published detail frame rate, the talk bubble and each
/// published atlas growth, and stands only while none of its self-moving
/// owners runs (module docs). The shown performance panel changes it once a
/// second, when it has new figures.
/// `None` keeps drawing within the limits: captures, modals, other screens,
/// replay matches, a network match's moving composite, recovery line or
/// in-game menu, exits and loads, a held mouse button (gadgets repeat on it)
/// and a shake still decaying.
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
        .runtime()
        .map(|runtime| &runtime.simulation);
    let still = !capture
        && modal.is_none()
        && state.frontend.screen == GameScreen::InGame
        && !network_picture_moves(state, simulation, clock)
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
    let panel_due = state.renderer.performance_overlay.readings_due(clock.at);
    let fonts = crate::render::text_engine::fetched_fonts();
    match page {
        ScreenPage::InGameShell(menu) if state.match_state.live_network.is_none() => {
            Some(StillFrame::Menu {
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
                panel_due,
                fonts,
            })
        }
        ScreenPage::Battlefield => {
            let detail = presentation.detail.borrow();
            let moving = state.match_state.paused()
                || !crate::app::input::camera::zoom_settled(state)
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
                atlas_growths: presentation.atlas_growth.published(),
                panel_due,
                fonts,
            })
        }
        _ => None,
    }
}

/// How soon the draws after a present may follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DrawLimits {
    /// The shortest time from this draw to the next one of a changed picture.
    picture: Duration,
    /// How long pointer motion alone may wait for a draw. `None` while the
    /// frame itself holds the cursor, so motion changes the picture.
    pointer: Option<Duration>,
}

/// The limits the picture just presented sets. `status_step` says it showed a
/// new step of a status-line reveal.
pub(crate) fn draw_limits(state: &AppState, status_step: bool) -> DrawLimits {
    let (_, page) = crate::app::input::precedence::presented(state);
    page_limits(
        page,
        status_step,
        state.platform.hardware_cursor.on_cursor_plane(),
        super::network_match::game_frame_ms(state),
    )
}

fn page_limits(
    page: ScreenPage,
    status_step: bool,
    os_cursor: bool,
    game_frame_ms: Option<f32>,
) -> DrawLimits {
    let picture = match page {
        // Each pass runs one slice of the load.
        ScreenPage::Loading => Duration::ZERO,
        // A status line reveals one step per paint on a 15 ms timer.
        _ if is_menu(page) && !status_step => MENU_INTERVAL,
        _ => PICTURE_INTERVAL,
    };
    let pointer = os_cursor.then(|| match page {
        // A game frame's own draw shows what the pointer hovers. The extra
        // bucket lets it come first, so a pointer draw just before it does
        // not push it back by the picture limit.
        ScreenPage::Battlefield => {
            game_frame_ms.map_or(FRAME_BUCKET, |ms| Duration::from_millis(ms as u64)) + FRAME_BUCKET
        }
        _ => MENU_INTERVAL,
    });
    DrawLimits { picture, pointer }
}

fn is_menu(page: ScreenPage) -> bool {
    match page {
        ScreenPage::LanLobby
        | ScreenPage::NetworkSetup
        | ScreenPage::LauncherOptions
        | ScreenPage::InGameShell(_)
        | ScreenPage::Skirmish
        | ScreenPage::SinglePlayer
        | ScreenPage::MoviesAndCredits
        | ScreenPage::MovieList
        | ScreenPage::Campaign
        | ScreenPage::LoadSavedGame
        | ScreenPage::Wol
        | ScreenPage::MainMenu
        | ScreenPage::CoopScore
        | ScreenPage::CampaignScore
        | ScreenPage::Score => true,
        ScreenPage::Battlefield
        | ScreenPage::FullscreenMovie
        | ScreenPage::CreditsRoll
        | ScreenPage::Loading
        | ScreenPage::InGameCard
        | ScreenPage::ResultCard => false,
    }
}

/// A network match's picture moves without a new frame while its last
/// composite would move again or the recovery line shows.
fn network_picture_moves(
    state: &AppState,
    simulation: Option<&crate::sim::world::Simulation>,
    clock: PictureClock,
) -> bool {
    let Some(live) = state.match_state.live_network.as_ref() else {
        return false;
    };
    state
        .match_state
        .match_presentation
        .tactical_composite
        .animated()
        || simulation.is_some_and(|sim| {
            live.recovery_status(clock.wall_ms as u32, sim.session().binary_frame())
                .is_some()
        })
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
        /// Whether the shown performance panel has new figures.
        panel_due: bool,
        /// Fonts the browser build fetched, which text may now draw with
        /// ([`crate::render::text_engine::fetched_fonts`]).
        fonts: usize,
    },
    /// The battlefield with its sidebar and HUD.
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
        /// Atlas growths published; each draws sprites the last picture
        /// lacked.
        atlas_growths: u64,
        /// Whether the shown performance panel has new figures.
        panel_due: bool,
        fonts: usize,
    },
}

/// What `about_to_wait` should run next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NextPass {
    /// Draw and present.
    Draw,
    /// Service the match; draw in the same pass only if the picture changed
    /// and the limits let it.
    Probe,
}

/// What a probe does about the picture it would draw, after its service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeVerdict {
    Draw,
    /// The presented picture is still right.
    Stands,
    /// The picture's inputs did not change, but the pointer moved: what it
    /// hovers shows when its limit passes.
    PointerWaits,
    /// A draw is owed and the picture limit holds it.
    DrawWaits,
}

#[derive(Debug, Clone, Copy)]
struct LastDraw {
    started: Instant,
    limits: DrawLimits,
}

#[derive(Debug, Default)]
pub(crate) struct RedrawGate {
    /// A window event other than pointer motion arrived since the last
    /// present.
    input_since_present: bool,
    /// The pointer moved since the last present.
    pointer_since_present: bool,
    /// A `request_redraw` waited for the limits since the last present.
    redraw_held: bool,
    /// The last presented picture, when it may stay on screen.
    presented: Option<StillFrame>,
    last_draw: Option<LastDraw>,
    /// No probe runs before this: pointer events can wake the loop far more
    /// often than the match needs its service.
    next_probe: Option<Instant>,
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

    /// Any window event may change the next picture, and draws at once.
    pub(crate) fn note_input(&mut self) {
        self.input_since_present = true;
    }

    /// The pointer moved: the next picture may change, within its limit.
    pub(crate) fn note_pointer_motion(&mut self) {
        self.pointer_since_present = true;
    }

    /// Record a present whose draw began at `started`. `still` is `None` while
    /// the picture may change without input, which keeps it drawing within
    /// `limits`.
    pub(crate) fn record_presented(
        &mut self,
        still: Option<StillFrame>,
        started: Instant,
        limits: DrawLimits,
    ) {
        self.input_since_present = false;
        self.pointer_since_present = false;
        self.redraw_held = false;
        self.presented = still;
        self.last_draw = Some(LastDraw { started, limits });
    }

    /// Input draws at once, and so does pointer motion past its limit.
    fn input_draw_due(&self, now: Instant) -> bool {
        self.input_since_present
            || (self.pointer_since_present
                && self.last_draw.is_none_or(|last| {
                    last.limits
                        .pointer
                        .is_none_or(|limit| now >= last.started + limit)
                }))
    }

    fn inside_picture_limit(&self, now: Instant) -> bool {
        self.last_draw
            .is_some_and(|last| now < last.started + last.limits.picture)
    }

    pub(crate) fn next_pass(&self, now: Instant) -> NextPass {
        if self.input_draw_due(now) {
            NextPass::Draw
        } else if self.inside_picture_limit(now) || (self.presented.is_some() && !self.redraw_held)
        {
            NextPass::Probe
        } else {
            NextPass::Draw
        }
    }

    /// A redraw was asked for. Returns whether it must wait for the limits;
    /// the wait is remembered, so the picture draws once they pass.
    pub(crate) fn hold_requested_draw(&mut self, now: Instant) -> bool {
        let hold = !self.input_draw_due(now) && self.inside_picture_limit(now);
        self.redraw_held |= hold;
        hold
    }

    /// After a probe's service: what happens to `current`, the picture it
    /// would draw.
    pub(crate) fn probe_verdict(&self, current: Option<StillFrame>, now: Instant) -> ProbeVerdict {
        if self.input_draw_due(now) {
            return ProbeVerdict::Draw;
        }
        let stands = !self.redraw_held && current.is_some() && current == self.presented;
        match (stands, self.pointer_since_present) {
            (true, false) => ProbeVerdict::Stands,
            (true, true) => ProbeVerdict::PointerWaits,
            (false, _) if self.inside_picture_limit(now) => ProbeVerdict::DrawWaits,
            (false, _) => ProbeVerdict::Draw,
        }
    }

    /// Whether a probe may run now.
    pub(crate) fn probe_due(&self, now: Instant) -> bool {
        self.next_probe.is_none_or(|at| now >= at)
    }

    /// When a probing loop should wake: for its next service, or sooner when
    /// a limit passes. A service wake already past starts the next one.
    pub(crate) fn probe_wake(&mut self, now: Instant) -> Instant {
        let service = match self.next_probe {
            Some(at) if at > now => at,
            _ => *self.next_probe.insert(now + SERVICE_WAKE),
        };
        let Some(last) = self.last_draw else {
            return service;
        };
        let pointer = last.limits.pointer.filter(|_| self.pointer_since_present);
        [Some(last.limits.picture), pointer]
            .into_iter()
            .flatten()
            .map(|limit| last.started + limit)
            .filter(|at| *at > now)
            .fold(service, Instant::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: DrawLimits = DrawLimits {
        picture: Duration::from_millis(16),
        pointer: Some(Duration::from_millis(64)),
    };

    fn menu(revision: u64, caret: bool, shake: [i32; 2]) -> StillFrame {
        StillFrame::Menu {
            menu: InGameMenuState::Menu,
            revision: Some(revision),
            shake_px: Some(shake),
            tooltip: None,
            caret,
            panel_due: false,
            fonts: 0,
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
            atlas_growths: 0,
            panel_due: false,
            fonts: 0,
        }
    }

    #[test]
    fn a_still_menu_is_probed_until_input_arrives() {
        let t = Instant::now();
        let shown = menu(120, false, [0, 0]);
        let mut gate = RedrawGate::default();
        assert_eq!(gate.next_pass(t), NextPass::Draw);
        gate.record_presented(Some(shown), t, LIMITS);
        assert_eq!(gate.next_pass(t), NextPass::Probe);
        assert_eq!(gate.probe_verdict(Some(shown), t), ProbeVerdict::Stands);
        gate.note_input();
        assert_eq!(gate.next_pass(t), NextPass::Draw);
        assert_eq!(gate.probe_verdict(Some(shown), t), ProbeVerdict::Draw);
    }

    #[test]
    fn a_world_change_caret_blink_or_shake_step_draws() {
        let t = Instant::now();
        let after_limit = t + LIMITS.picture;
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(menu(120, false, [0, 0])), t, LIMITS);
        for changed in [
            menu(121, false, [0, 0]),
            menu(120, true, [0, 0]),
            menu(120, false, [-1, 0]),
        ] {
            assert_eq!(
                gate.probe_verdict(Some(changed), after_limit),
                ProbeVerdict::Draw
            );
        }
        assert_eq!(gate.probe_verdict(None, after_limit), ProbeVerdict::Draw);
        gate.record_presented(None, t, LIMITS);
        assert_eq!(gate.next_pass(after_limit), NextPass::Draw);
    }

    /// A standing battlefield is probed; a world change, a scroll, a new
    /// sidebar view, a talk bubble running out or new performance figures
    /// draws, and a draw that showed motion (recorded as `None`) keeps
    /// drawing.
    #[test]
    fn a_still_battlefield_draws_on_world_camera_view_or_bubble_changes() {
        let t = Instant::now();
        let after_limit = t + LIMITS.picture;
        let shown = battlefield(120, 480.0, 7, true);
        let mut new_figures = shown;
        if let StillFrame::Battlefield { panel_due, .. } = &mut new_figures {
            *panel_due = true;
        }
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(shown), t, LIMITS);
        assert_eq!(gate.next_pass(after_limit), NextPass::Probe);
        assert_eq!(
            gate.probe_verdict(Some(shown), after_limit),
            ProbeVerdict::Stands
        );
        for changed in [
            battlefield(121, 480.0, 7, true),
            battlefield(120, 481.0, 7, true),
            battlefield(120, 480.0, 8, true),
            battlefield(120, 480.0, 7, false),
            menu(120, false, [0, 0]),
            new_figures,
        ] {
            assert_eq!(
                gate.probe_verdict(Some(changed), after_limit),
                ProbeVerdict::Draw
            );
        }
        gate.record_presented(None, t, LIMITS);
        assert_eq!(gate.next_pass(after_limit), NextPass::Draw);
    }

    /// Pointer motion over a standing picture waits for its limit, and probes
    /// keep the service's pace however fast the events come. Other input
    /// draws at once, and a cursor in the frame makes motion input.
    #[test]
    fn pointer_motion_waits_for_its_limit_unless_the_frame_holds_the_cursor() {
        let t = Instant::now();
        let shown = battlefield(120, 480.0, 7, true);
        let early = t + Duration::from_millis(20);
        let pointer_limit = t + LIMITS.pointer.unwrap();
        let mut gate = RedrawGate::default();
        gate.record_presented(Some(shown), t, LIMITS);
        gate.note_pointer_motion();
        assert_eq!(gate.next_pass(early), NextPass::Probe);
        assert_eq!(
            gate.probe_verdict(Some(shown), early),
            ProbeVerdict::PointerWaits
        );
        let wake = gate.probe_wake(early);
        assert_eq!(wake, early + SERVICE_WAKE);
        assert!(!gate.probe_due(early + Duration::from_millis(1)));
        assert!(gate.probe_due(wake));
        // The last wake before the limit lands on it.
        assert_eq!(
            gate.probe_wake(pointer_limit - Duration::from_millis(4)),
            pointer_limit
        );
        assert_eq!(gate.next_pass(pointer_limit), NextPass::Draw);
        assert_eq!(
            gate.probe_verdict(Some(shown), pointer_limit),
            ProbeVerdict::Draw
        );

        gate.note_input();
        assert_eq!(gate.next_pass(early), NextPass::Draw);

        let cursor_in_frame = DrawLimits {
            pointer: None,
            ..LIMITS
        };
        gate.record_presented(Some(shown), t, cursor_in_frame);
        gate.note_pointer_motion();
        assert_eq!(gate.next_pass(t), NextPass::Draw);
        assert_eq!(gate.probe_verdict(Some(shown), t), ProbeVerdict::Draw);
    }

    /// A moving picture, and a redraw asked for over a standing one, wait for
    /// the picture limit and then draw; input does not wait.
    #[test]
    fn a_changed_picture_or_an_asked_for_redraw_waits_for_the_picture_limit() {
        let t = Instant::now();
        let early = t + Duration::from_millis(10);
        let after_limit = t + LIMITS.picture;
        let shown = menu(120, false, [0, 0]);
        let mut gate = RedrawGate::default();
        gate.record_presented(None, t, LIMITS);
        assert_eq!(gate.next_pass(early), NextPass::Probe);
        assert_eq!(gate.probe_verdict(None, early), ProbeVerdict::DrawWaits);
        assert_eq!(gate.next_pass(after_limit), NextPass::Draw);

        gate.record_presented(Some(shown), t, LIMITS);
        assert!(gate.hold_requested_draw(early));
        assert_eq!(
            gate.probe_verdict(Some(shown), early),
            ProbeVerdict::DrawWaits
        );
        assert_eq!(gate.next_pass(after_limit), NextPass::Draw);
        assert!(!gate.hold_requested_draw(after_limit));

        gate.note_input();
        assert!(!gate.hold_requested_draw(early));
        assert_eq!(gate.probe_verdict(Some(shown), early), ProbeVerdict::Draw);
    }

    #[test]
    fn menus_battles_and_loads_have_their_own_limits() {
        let limits =
            |page, status_step, os_cursor| page_limits(page, status_step, os_cursor, Some(48.0));
        assert_eq!(
            limits(ScreenPage::MainMenu, false, true),
            DrawLimits {
                picture: MENU_INTERVAL,
                pointer: Some(MENU_INTERVAL),
            }
        );
        // A status line's 15 ms steps keep their pace.
        assert_eq!(
            limits(ScreenPage::Skirmish, true, true).picture,
            PICTURE_INTERVAL
        );
        assert_eq!(
            limits(ScreenPage::Battlefield, false, true),
            DrawLimits {
                picture: PICTURE_INTERVAL,
                pointer: Some(Duration::from_millis(64)),
            }
        );
        assert_eq!(
            limits(ScreenPage::Loading, false, true).picture,
            Duration::ZERO
        );
        assert_eq!(limits(ScreenPage::Battlefield, false, false).pointer, None);
        assert_eq!(
            page_limits(ScreenPage::Battlefield, false, true, None).pointer,
            Some(FRAME_BUCKET * 2),
            "unlimited speed has no game frame to wait for"
        );
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
