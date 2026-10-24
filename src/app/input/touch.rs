//! Touch → mouse translation for touchscreens.
//!
//! RA2's UI is a mouse UI: every screen reads the cursor position and the
//! button edges through the same `CursorMoved`/`MouseInput` paths the OS mouse
//! drives. A touchscreen has no cursor, and Windows delivers its contacts as
//! `WindowEvent::Touch` only (winit registers the window for touch and consumes
//! `WM_TOUCH`, which stops the system's mouse synthesis), so this mapper
//! re-issues each touch as the gesture it means:
//!
//! - a quick release clicks the left button — select, order, UI;
//! - a drag past the threshold holds the left button from the touch origin,
//!   which is RA2's own select box over the battlefield and its slider/list
//!   drag over the sidebar and menus;
//! - two fingers move the camera 1:1 with the hand (grab style: the map
//!   follows the fingers), reported as a `pan` displacement for the caller to
//!   apply — native RA2's right-drag pan is a joystick that keeps gliding
//!   while the finger rests, which overshoots badly on a touchscreen, and its
//!   release can fire the right-click cancel ladder;
//! - a long, still touch fires the right button — RA2's cancel/context action.
//!
//! A touch first only moves the cursor: the button decision waits until the
//! gesture is unambiguous.
//!
//! A third finger is ignored, and so is a second finger outside a match: both
//! would otherwise steal the gesture the hand is already making. A contact
//! that lands on top of a tracked finger is a second delivery of that finger,
//! not a new one (`DUPLICATE_CONTACT_PX`).
//!
//! The call site drives this on every host except Linux, where the exclusion is
//! X11-only: those desktops keep driving the pointer from touch, so translating
//! again would double every click. A Wayland touchscreen delivers `wl_touch`
//! with no pointer events to fall back on, so it gets no input here. macOS has
//! no touchscreen. Capture sessions are input-sealed everywhere: they keep
//! counting raw touches as violations.
//!
//! The mapper owns no game state and never reaches the simulation. `translate`
//! takes the current `Instant`, the in-game flag and the window scale factor as
//! parameters so every boundary is testable without a clock or a running app.

// The module stays compiled on every host so its gestures keep their tests
// there; only the call site is Windows-bound.
#![cfg_attr(target_os = "linux", allow(dead_code))]

use std::time::{Duration, Instant};

use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, Touch, TouchPhase, WindowEvent};

/// A pending touch held this long without leaving its origin becomes a right
/// click immediately.
const LONG_PRESS: Duration = Duration::from_millis(400);
/// Movement beyond this many logical pixels from the touch origin upgrades a
/// pending touch into a drag — Rusted Warfare's touch threshold. Multiplied
/// by the window scale factor so the hand travel stays the same on every
/// display density.
const DRAG_THRESHOLD_LOGICAL_PX: f64 = 15.0;
/// A contact this close to a tracked finger repeats it rather than adding one.
/// winit 0.30 reissues every Windows touch from two paths under two ids
/// (`WM_TOUCH`'s `TOUCHINPUT.dwID` and `WM_POINTER*`'s `pointerId`), so one
/// physical finger can arrive twice; taking the copy as a second finger would
/// turn every tap into a two-finger pan. Two fingers cannot share a point.
const DUPLICATE_CONTACT_PX: f64 = 2.0;
/// Fingers tracked at once: a lone finger drives the cursor, a pair drives the
/// camera pan.
const MAX_FINGERS: usize = 2;
/// Synthetic events one touch can mean. The longest step is a click: move to
/// the point, press, release.
const MAX_SYNTHETICS: usize = 3;

/// Synthetic events for one touch, in dispatch order.
pub(crate) type TouchSynthetics = [Option<WindowEvent>; MAX_SYNTHETICS];

/// One touch translated into what the app should do with it.
#[derive(Debug, Default)]
pub(crate) struct TouchOutput {
    /// Mouse events to dispatch, in order.
    pub events: TouchSynthetics,
    /// Camera displacement in screen pixels, grab style: the camera moves by
    /// this much so the map follows the fingers. The caller divides by the
    /// current zoom to reach world pixels. `None` outside a two-finger pan.
    pub pan: Option<(f32, f32)>,
}

impl TouchOutput {
    fn events(events: TouchSynthetics) -> Self {
        Self {
            events,
            ..Self::default()
        }
    }
}

/// One tracked finger's last known position.
#[derive(Clone, Copy, Debug)]
struct Finger {
    id: u64,
    pos: PhysicalPosition<f64>,
}

/// The gesture the tracked finger set is currently driving.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gesture {
    /// One finger down, no button synthesized yet; the cursor follows.
    Pending,
    /// A synthesized left-button drag: the select box, or a slider/list drag.
    Drag,
    /// Two-finger camera pan; `last` is the previous centroid.
    Pan { last: PhysicalPosition<f64> },
    /// A long-press right click fired; the touch's remaining events are
    /// consumed.
    Done,
}

/// Translates winit touch events into the synthetic mouse events they mean.
#[derive(Debug)]
pub(crate) struct TouchMouseMapper {
    /// Tracked fingers, oldest first — the survivor of a broken multi-finger
    /// gesture re-anchors from the oldest.
    fingers: [Option<Finger>; MAX_FINGERS],
    gesture: Gesture,
    /// The pending finger's origin, kept for the select-box jump-back and the
    /// long-press stillness check.
    origin: PhysicalPosition<f64>,
    started: Option<Instant>,
    /// Set when a pan's remaining finger is re-anchored: that finger was part
    /// of a multi-finger gesture, so its lift must not click and its hold
    /// must not fire the context button. Cleared on the next fresh touch or
    /// when it turns into a select drag.
    suppress_tap: bool,
}

impl TouchMouseMapper {
    pub(crate) const fn new() -> Self {
        Self {
            fingers: [None, None],
            gesture: Gesture::Pending,
            origin: PhysicalPosition::new(0.0, 0.0),
            started: None,
            suppress_tap: false,
        }
    }

    /// Drop every tracked touch. A stream can end without an `Ended` — the
    /// contact is cancelled, or the window loses focus with fingers down — and
    /// a phantom finger would turn every later tap into a second finger.
    pub(crate) fn reset(&mut self) {
        self.fingers = [None; MAX_FINGERS];
        self.gesture = Gesture::Pending;
        self.origin = PhysicalPosition::new(0.0, 0.0);
        self.started = None;
        self.suppress_tap = false;
    }

    /// Whether a two-finger pan owns the camera right now. The app inhibits
    /// edge auto-scroll while it does, standing in for the tactical capture
    /// native's right-drag pan holds.
    pub(crate) fn pan_active(&self) -> bool {
        matches!(self.gesture, Gesture::Pan { .. })
    }

    /// Map one touch into the events and camera motion it means. `in_game`
    /// gates the two-finger pan; `scale` is the window's scale factor, which
    /// scales the drag threshold.
    pub(crate) fn translate(
        &mut self,
        touch: &Touch,
        now: Instant,
        in_game: bool,
        scale: f64,
    ) -> TouchOutput {
        match touch.phase {
            TouchPhase::Started => self.on_finger_down(touch, now, in_game),
            TouchPhase::Moved => {
                self.finger_moved(touch);
                self.on_finger_move(touch, now, scale)
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let events = self.on_finger_up(touch, now, scale);
                self.finger_remove(touch.id);
                TouchOutput::events(events)
            }
        }
    }

    // --- finger table ---------------------------------------------------

    fn tracked(&self) -> usize {
        self.fingers.iter().flatten().count()
    }

    fn finger_tracked(&self, id: u64) -> bool {
        self.fingers.iter().flatten().any(|finger| finger.id == id)
    }

    fn finger_moved(&mut self, touch: &Touch) {
        for slot in &mut self.fingers {
            if let Some(finger) = slot
                && finger.id == touch.id
            {
                finger.pos = touch.location;
            }
        }
    }

    /// Remove a finger and keep the table dense, so the next touch takes the
    /// free slot instead of overwriting a live finger.
    fn finger_remove(&mut self, id: u64) {
        let mut dense = [None; MAX_FINGERS];
        for (slot, finger) in self
            .fingers
            .iter()
            .flatten()
            .filter(|finger| finger.id != id)
            .enumerate()
        {
            dense[slot] = Some(*finger);
        }
        self.fingers = dense;
    }

    fn centroid(&self) -> PhysicalPosition<f64> {
        let mut sum = (0.0, 0.0);
        for finger in self.fingers.iter().flatten() {
            sum.0 += finger.pos.x;
            sum.1 += finger.pos.y;
        }
        let count = self.tracked().max(1) as f64;
        PhysicalPosition::new(sum.0 / count, sum.1 / count)
    }

    /// Put the oldest surviving finger back into the pending state, so a drag
    /// can continue naturally after a multi-finger gesture ends. The excluded
    /// finger is the one being lifted and may still be in the table.
    fn reanchor_as_pending(&mut self, now: Instant, lifting: u64) {
        if let Some(finger) = self
            .fingers
            .iter()
            .flatten()
            .find(|finger| finger.id != lifting)
            .copied()
        {
            self.gesture = Gesture::Pending;
            self.origin = finger.pos;
            self.started = Some(now);
            // The finger rode a multi-finger gesture: lifting it must not
            // click, and only a real drag may start a select.
            self.suppress_tap = true;
        } else {
            self.gesture = Gesture::Done;
            self.started = None;
        }
    }

    // --- gesture edges --------------------------------------------------

    fn on_finger_down(&mut self, touch: &Touch, now: Instant, in_game: bool) -> TouchOutput {
        let tracked_before = self.tracked();
        // A copy of a tracked contact, not a new finger (see
        // `DUPLICATE_CONTACT_PX`); it stays untracked, so its moves and its
        // lift fall through the guards below.
        if self
            .fingers
            .iter()
            .flatten()
            .any(|finger| distance(finger.pos, touch.location) <= DUPLICATE_CONTACT_PX)
        {
            return TouchOutput::default();
        }
        // A further finger past the pan pair is ignored, and so is any second
        // finger outside a match: it never joins the finger table.
        if tracked_before >= MAX_FINGERS || (tracked_before == 1 && !in_game) {
            return TouchOutput::default();
        }
        let Some(slot) = self.fingers.iter().position(Option::is_none) else {
            return TouchOutput::default();
        };
        self.fingers[slot] = Some(Finger {
            id: touch.id,
            pos: touch.location,
        });
        let events = if tracked_before == 0 {
            // First finger: start pending and follow the cursor.
            self.gesture = Gesture::Pending;
            self.origin = touch.location;
            self.started = Some(now);
            self.suppress_tap = false;
            [Some(Self::cursor_moved(touch)), None, None]
        } else {
            // Second finger: take over as the camera pan, releasing the
            // select button the first finger may hold.
            let release = match self.gesture {
                Gesture::Drag => Some(Self::mouse_input(
                    touch,
                    ElementState::Released,
                    MouseButton::Left,
                )),
                Gesture::Pending | Gesture::Pan { .. } | Gesture::Done => None,
            };
            let centroid = self.centroid();
            self.gesture = Gesture::Pan { last: centroid };
            [
                release,
                Some(Self::cursor_at(touch.device_id, centroid)),
                None,
            ]
        };
        TouchOutput::events(events)
    }

    fn on_finger_move(&mut self, touch: &Touch, now: Instant, scale: f64) -> TouchOutput {
        if !self.finger_tracked(touch.id) {
            return TouchOutput::default();
        }
        match self.gesture {
            Gesture::Done => TouchOutput::default(),
            Gesture::Pending => {
                let threshold = drag_threshold(scale);
                let held = !self.suppress_tap
                    && self
                        .started
                        .is_some_and(|start| now.duration_since(start) >= LONG_PRESS);
                if held && distance(touch.location, self.origin) <= threshold {
                    // Long press: fire the context click right away.
                    self.gesture = Gesture::Done;
                    return TouchOutput::events(Self::click_at(touch, MouseButton::Right));
                }
                if distance(touch.location, self.origin) > threshold {
                    // Upgrade to the select box: press the left button where
                    // the finger landed, so the box starts at the origin. The
                    // same path drags sliders and lists over the UI.
                    self.gesture = Gesture::Drag;
                    self.suppress_tap = false;
                    TouchOutput::events([
                        Some(Self::cursor_at(touch.device_id, self.origin)),
                        Some(Self::mouse_input(
                            touch,
                            ElementState::Pressed,
                            MouseButton::Left,
                        )),
                        None,
                    ])
                } else {
                    // Still pending: the cursor follows the finger.
                    TouchOutput::events([Some(Self::cursor_moved(touch)), None, None])
                }
            }
            // The select box and UI drags follow the finger.
            Gesture::Drag => TouchOutput::events([Some(Self::cursor_moved(touch)), None, None]),
            // The camera follows the fingers: the map moves with the hand, so
            // the camera moves against it.
            Gesture::Pan { last } => {
                let centroid = self.centroid();
                self.gesture = Gesture::Pan { last: centroid };
                TouchOutput {
                    events: [Some(Self::cursor_at(touch.device_id, centroid)), None, None],
                    pan: Some(((last.x - centroid.x) as f32, (last.y - centroid.y) as f32)),
                }
            }
        }
    }

    fn on_finger_up(&mut self, touch: &Touch, now: Instant, scale: f64) -> TouchSynthetics {
        if !self.finger_tracked(touch.id) {
            return none();
        }
        let remaining = self.tracked() - 1;
        match self.gesture {
            Gesture::Done => none(),
            Gesture::Pending => {
                // Read the start before clearing it: the fallback below needs
                // the real hold time.
                let start = self.pending_started(now);
                let suppressed = std::mem::take(&mut self.suppress_tap);
                self.gesture = Gesture::Done;
                self.started = None;
                // A pan's remaining finger lifts without clicking, and a
                // cancelled contact is not a click either.
                if suppressed || touch.phase != TouchPhase::Ended {
                    return none();
                }
                let threshold = drag_threshold(scale);
                let held = now.duration_since(start) >= LONG_PRESS;
                let still = distance(touch.location, self.origin) <= threshold;
                let button = if held && still {
                    // Long-press fallback for coarse event delivery: the live
                    // check in the move handler usually fires first.
                    MouseButton::Right
                } else {
                    // A quick release clicks.
                    MouseButton::Left
                };
                Self::click_at(touch, button)
            }
            Gesture::Drag => {
                // The drag is single-finger, so its own lift ends it; a second
                // finger taking over cannot normally reach here, but must not
                // leave the button latched if it does.
                if remaining == 0 {
                    self.gesture = Gesture::Done;
                } else {
                    self.reanchor_as_pending(now, touch.id);
                }
                // The band box ends where the finger lifted, not at its last
                // move.
                [
                    Some(Self::cursor_at(touch.device_id, touch.location)),
                    Some(Self::mouse_input(
                        touch,
                        ElementState::Released,
                        MouseButton::Left,
                    )),
                    None,
                ]
            }
            Gesture::Pan { .. } if remaining == 0 => {
                self.gesture = Gesture::Done;
                none()
            }
            Gesture::Pan { .. } => {
                // One finger of the pan stays down: it continues as a fresh
                // pending touch, free to select or drag from here.
                self.reanchor_as_pending(now, touch.id);
                none()
            }
        }
    }

    /// The pending start instant, treating a lost clock as "just started".
    fn pending_started(&self, now: Instant) -> Instant {
        self.started.unwrap_or(now)
    }

    // --- synthetic events -------------------------------------------------

    fn cursor_at(device_id: DeviceId, position: PhysicalPosition<f64>) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id,
            position,
        }
    }

    fn cursor_moved(touch: &Touch) -> WindowEvent {
        Self::cursor_at(touch.device_id, touch.location)
    }

    fn mouse_input(touch: &Touch, state: ElementState, button: MouseButton) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: touch.device_id,
            state,
            button,
        }
    }

    /// A press and release of `button` where the finger lifted.
    fn click_at(touch: &Touch, button: MouseButton) -> TouchSynthetics {
        [
            Some(Self::cursor_at(touch.device_id, touch.location)),
            Some(Self::mouse_input(touch, ElementState::Pressed, button)),
            Some(Self::mouse_input(touch, ElementState::Released, button)),
        ]
    }
}

fn none() -> TouchSynthetics {
    Default::default()
}

fn drag_threshold(scale: f64) -> f64 {
    DRAG_THRESHOLD_LOGICAL_PX * scale.max(1.0)
}

fn distance(a: PhysicalPosition<f64>, b: PhysicalPosition<f64>) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::Force;

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: Some(Force::Calibrated {
                force: 1.0,
                max_possible_force: 1.0,
                altitude_angle: None,
            }),
            id,
        }
    }

    /// Render synthetic mouse events as short names, e.g. "move", "Pressed Left".
    fn names(output: TouchOutput) -> Vec<String> {
        output
            .events
            .into_iter()
            .flatten()
            .map(|event| match event {
                WindowEvent::CursorMoved { .. } => "move".to_owned(),
                WindowEvent::MouseInput { state, button, .. } => format!(
                    "{:?} {}",
                    state,
                    match button {
                        MouseButton::Left => "Left",
                        MouseButton::Right => "Right",
                        _ => "Other",
                    }
                ),
                _ => "other".to_owned(),
            })
            .collect()
    }

    /// One finger step at scale 1.0; the tests scale distances off it.
    const STEP: f64 = DRAG_THRESHOLD_LOGICAL_PX + 1.0;

    // --- one finger ------------------------------------------------------

    #[test]
    fn a_quick_tap_clicks_the_left_button() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = mapper.translate(&touch(1, TouchPhase::Started, 40.0, 60.0), now, true, 1.0);
        assert!(!mapper.pan_active());

        let click = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 42.0, 60.0),
            now + Duration::from_millis(80),
            true,
            1.0,
        ));

        // The click lands where the finger lifted, so the cursor moves there
        // before the button edge.
        assert_eq!(
            click,
            vec![
                "move".to_owned(),
                "Pressed Left".to_owned(),
                "Released Left".to_owned()
            ]
        );
    }

    #[test]
    fn a_single_finger_drag_selects_from_the_touch_origin() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(&touch(1, TouchPhase::Started, 40.0, 60.0), now, true, 1.0));

        // The select box presses where the finger landed, then follows it.
        let upgrade = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 40.0 + STEP, 60.0),
            now,
            true,
            1.0,
        ));
        let follow = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 40.0 + STEP + 30.0, 60.0),
            now,
            true,
            1.0,
        ));
        let lift = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 40.0 + STEP + 30.0, 60.0),
            now + Duration::from_millis(200),
            true,
            1.0,
        ));

        assert_eq!(upgrade, vec!["move".to_owned(), "Pressed Left".to_owned()]);
        assert_eq!(follow, vec!["move".to_owned()]);
        assert_eq!(lift, vec!["move".to_owned(), "Released Left".to_owned()]);
    }

    #[test]
    fn a_ui_drag_uses_the_same_select_gesture() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 300.0, 400.0), now, true, 1.0));

        let upgrade = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 300.0 + STEP, 400.0),
            now,
            true,
            1.0,
        ));

        // Over the sidebar the same left-button drag moves a slider or list,
        // so the press must land on the grabbed origin, not the moved finger.
        assert_eq!(upgrade, vec!["move".to_owned(), "Pressed Left".to_owned()]);
    }

    #[test]
    fn the_drag_threshold_scales_with_display_density() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(&touch(1, TouchPhase::Started, 0.0, 0.0), now, true, 2.0));

        // At scale 2.0 the threshold is 30 physical px: 25 px stays pending,
        // 31 px upgrades.
        let below =
            names(mapper.translate(&touch(1, TouchPhase::Moved, 25.0, 0.0), now, true, 2.0));
        let above =
            names(mapper.translate(&touch(1, TouchPhase::Moved, 31.0, 0.0), now, true, 2.0));

        assert_eq!(below, vec!["move".to_owned()]);
        assert_eq!(above, vec!["move".to_owned(), "Pressed Left".to_owned()]);
    }

    #[test]
    fn menu_touches_keep_the_plain_click_gestures() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(
            &touch(1, TouchPhase::Started, 100.0, 100.0),
            now,
            false,
            1.0,
        ));

        // A second finger cannot pan on a menu, so it is ignored...
        let second = names(mapper.translate(
            &touch(2, TouchPhase::Started, 200.0, 200.0),
            now,
            false,
            1.0,
        ));

        // ...and the first finger still drags the select box / slider.
        let drag = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 100.0 + STEP, 100.0),
            now,
            false,
            1.0,
        ));

        assert!(second.is_empty());
        assert_eq!(drag, vec!["move".to_owned(), "Pressed Left".to_owned()]);
    }

    // --- long press ------------------------------------------------------

    #[test]
    fn a_long_still_touch_fires_the_right_button_immediately() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(&touch(1, TouchPhase::Started, 40.0, 60.0), now, true, 1.0));

        let fired = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 42.0, 60.0),
            now + LONG_PRESS + Duration::from_millis(1),
            true,
            1.0,
        ));
        let consumed = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 42.0, 60.0),
            now + LONG_PRESS + Duration::from_millis(40),
            true,
            1.0,
        ));

        assert_eq!(
            fired,
            vec![
                "move".to_owned(),
                "Pressed Right".to_owned(),
                "Released Right".to_owned()
            ]
        );
        assert!(consumed.is_empty());
    }

    #[test]
    fn a_long_press_without_any_move_fires_on_release() {
        // Coarse delivery can skip move events entirely; the release must
        // still read as a long press, not as a tap.
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(&touch(1, TouchPhase::Started, 40.0, 60.0), now, true, 1.0));

        let fired = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 40.0, 60.0),
            now + LONG_PRESS + Duration::from_millis(50),
            true,
            1.0,
        ));

        assert_eq!(
            fired,
            vec![
                "move".to_owned(),
                "Pressed Right".to_owned(),
                "Released Right".to_owned()
            ]
        );
    }

    #[test]
    fn a_cancelled_pending_touch_clicks_nothing() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ = names(mapper.translate(&touch(1, TouchPhase::Started, 40.0, 60.0), now, true, 1.0));

        let cancelled = names(mapper.translate(
            &touch(1, TouchPhase::Cancelled, 42.0, 60.0),
            now + Duration::from_millis(80),
            true,
            1.0,
        ));

        assert!(cancelled.is_empty());
    }

    // --- two fingers -----------------------------------------------------

    #[test]
    fn two_fingers_pan_the_camera_grab_style() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));

        let takeover =
            mapper.translate(&touch(2, TouchPhase::Started, 200.0, 200.0), now, true, 1.0);
        assert!(takeover.pan.is_none());
        assert!(mapper.pan_active());
        assert_eq!(names(takeover), vec!["move".to_owned()]);

        // The fingers move right and down by 20 px each; the camera must move
        // the other way so the map follows the hand.
        let pan = mapper.translate(&touch(2, TouchPhase::Moved, 220.0, 220.0), now, true, 1.0);
        let (dx, dy) = pan.pan.expect("pan displacement");
        assert_eq!(names(pan), vec!["move".to_owned()]);
        assert_eq!((dx, dy), (-10.0, -10.0));

        // No button was ever synthesized: lifting simply ends the pan.
        let lift = mapper.translate(
            &touch(2, TouchPhase::Ended, 220.0, 220.0),
            now + Duration::from_millis(40),
            true,
            1.0,
        );
        assert!(!mapper.pan_active());
        assert!(names(lift).is_empty());
    }

    #[test]
    fn a_second_finger_ends_an_in_progress_select() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));
        let _ = names(mapper.translate(
            &touch(1, TouchPhase::Moved, 100.0 + STEP, 100.0),
            now,
            true,
            1.0,
        ));

        // The select releases first, then the pan anchors at the centroid.
        let takeover =
            names(mapper.translate(&touch(2, TouchPhase::Started, 300.0, 100.0), now, true, 1.0));

        assert_eq!(
            takeover,
            vec!["Released Left".to_owned(), "move".to_owned()]
        );
    }

    #[test]
    fn lifting_a_pan_finger_never_clicks() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));
        let _ =
            names(mapper.translate(&touch(2, TouchPhase::Started, 200.0, 200.0), now, true, 1.0));
        let _ = names(mapper.translate(&touch(2, TouchPhase::Moved, 260.0, 260.0), now, true, 1.0));

        // One finger leaves the pan; the other must not turn its own lift
        // into a left click (it was part of a gesture, not a tap).
        let _ = names(mapper.translate(
            &touch(2, TouchPhase::Ended, 260.0, 260.0),
            now + Duration::from_millis(40),
            true,
            1.0,
        ));
        let last_lift = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 100.0, 100.0),
            now + Duration::from_millis(90),
            true,
            1.0,
        ));

        assert!(last_lift.is_empty());
    }

    #[test]
    fn a_third_finger_is_ignored() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));
        let _ =
            names(mapper.translate(&touch(2, TouchPhase::Started, 200.0, 100.0), now, true, 1.0));

        let third =
            names(mapper.translate(&touch(3, TouchPhase::Started, 300.0, 100.0), now, true, 1.0));
        assert!(third.is_empty());

        // The pan keeps following the two fingers it started with: moving
        // finger 2 right by 20 px moves the centroid by 10 px.
        let pan = mapper.translate(&touch(2, TouchPhase::Moved, 220.0, 100.0), now, true, 1.0);
        assert_eq!(pan.pan, Some((-10.0, 0.0)));
    }

    #[test]
    fn a_new_finger_after_a_lift_keeps_the_live_hand() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));
        let _ =
            names(mapper.translate(&touch(2, TouchPhase::Started, 200.0, 100.0), now, true, 1.0));
        let _ = names(mapper.translate(
            &touch(1, TouchPhase::Ended, 100.0, 100.0),
            now + Duration::from_millis(40),
            true,
            1.0,
        ));

        // The lifted finger left a hole in the table: the next touch must take
        // that slot instead of overwriting finger 2, which is still down.
        let _ =
            names(mapper.translate(&touch(3, TouchPhase::Started, 300.0, 100.0), now, true, 1.0));

        // Both live fingers are tracked again, so the pan still reads the pair:
        // centroid 250 -> 260 when finger 2 moves right by 20 px.
        let pan = mapper.translate(&touch(2, TouchPhase::Moved, 220.0, 100.0), now, true, 1.0);
        assert_eq!(pan.pan, Some((-10.0, 0.0)));
    }

    #[test]
    fn one_contact_delivered_twice_is_still_one_finger() {
        // winit reissues a Windows touch from `WM_TOUCH` and `WM_POINTER*`
        // under two ids. The copy must not read as a second finger, or the
        // first tap of every gesture becomes a two-finger pan.
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let first =
            names(mapper.translate(&touch(7, TouchPhase::Started, 120.0, 90.0), now, true, 1.0));
        let copy =
            names(mapper.translate(&touch(8, TouchPhase::Started, 120.5, 90.5), now, true, 1.0));

        assert_eq!(first, vec!["move".to_owned()]);
        assert!(copy.is_empty());
        assert!(!mapper.pan_active());

        // The surviving finger still selects and clicks normally.
        let upgrade = names(mapper.translate(
            &touch(7, TouchPhase::Moved, 120.0 + STEP, 90.0),
            now,
            true,
            1.0,
        ));
        assert_eq!(upgrade, vec!["move".to_owned(), "Pressed Left".to_owned()]);
    }

    // --- reset and the edge-scroll gate ----------------------------------

    #[test]
    fn a_reset_drops_a_lost_touch_stream() {
        let mut mapper = TouchMouseMapper::new();
        let now = Instant::now();
        let _ =
            names(mapper.translate(&touch(1, TouchPhase::Started, 100.0, 100.0), now, true, 1.0));
        let _ =
            names(mapper.translate(&touch(2, TouchPhase::Started, 200.0, 100.0), now, true, 1.0));

        // A cancelled contact or a focus loss can end a stream without an
        // `Ended`; the phantom pair must not turn the next tap into a pan.
        mapper.reset();

        let down =
            names(mapper.translate(&touch(5, TouchPhase::Started, 40.0, 60.0), now, true, 1.0));
        let click = names(mapper.translate(
            &touch(5, TouchPhase::Ended, 40.0, 60.0),
            now + Duration::from_millis(60),
            true,
            1.0,
        ));

        assert_eq!(down, vec!["move".to_owned()]);
        assert_eq!(
            click,
            vec![
                "move".to_owned(),
                "Pressed Left".to_owned(),
                "Released Left".to_owned()
            ]
        );
    }

    #[test]
    fn a_touch_pan_removes_the_edge_scroll_band() {
        use crate::app::input::camera::touch_owns_pointer;
        use crate::app::input::state::MatchInputState;

        let mut input = MatchInputState::new(Default::default());
        let now = Instant::now();
        let _ = input.touch_mapper.translate(
            &touch(1, TouchPhase::Started, 100.0, 100.0),
            now,
            true,
            1.0,
        );
        // A single finger leaves the band to the mouse: it can still click at
        // the window edge.
        assert!(!touch_owns_pointer(&input));

        let _ = input.touch_mapper.translate(
            &touch(2, TouchPhase::Started, 200.0, 100.0),
            now,
            true,
            1.0,
        );
        assert!(touch_owns_pointer(&input));

        let _ = input.touch_mapper.translate(
            &touch(2, TouchPhase::Ended, 200.0, 100.0),
            now + Duration::from_millis(30),
            true,
            1.0,
        );
        assert!(!touch_owns_pointer(&input));
    }

    #[test]
    fn a_parked_touch_pointer_removes_the_edge_scroll_band() {
        use crate::app::input::camera::touch_owns_pointer;
        use crate::app::input::state::MatchInputState;

        // Without a real pointer the synthetic cursor stays where the last
        // contact left it, so the band would scroll with nothing to carry it
        // away. The handler raises this on every touch and the next hardware
        // `CursorMoved` drops it again.
        let mut input = MatchInputState::new(Default::default());
        assert!(!touch_owns_pointer(&input));

        input.cursor_from_touch = true;
        assert!(touch_owns_pointer(&input));

        input.cursor_from_touch = false;
        assert!(!touch_owns_pointer(&input));
    }
}
