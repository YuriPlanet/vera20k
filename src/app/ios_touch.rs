//! iOS touch delivery for the existing mouse-owned shell and tactical input.
//!
//! winit 0.30.12's `platform_impl/ios/view.rs::handle_touches` already converts
//! UIKit locations to physical window coordinates using `contentScaleFactor`.
//! Feed those unchanged through `WindowEvent::CursorMoved`, so the app's normal
//! cursor-space conversion remains the only owner of shell/viewport scaling.
//! https://github.com/rust-windowing/winit/blob/v0.30.12/src/platform_impl/ios/view.rs

use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, Touch, TouchPhase, WindowEvent};

#[path = "ios_ui_scroll.rs"]
pub(super) mod ui_scroll;
use ui_scroll::{ScrollRegion, ScrollTarget};

#[derive(Default)]
pub(super) struct TouchTranslation {
    pub events: Vec<WindowEvent>,
    pub scroll: Option<(ScrollTarget, i32)>,
    pub cancel_pointer: bool,
}

struct ScrollGesture {
    key: (DeviceId, u64),
    region: ScrollRegion,
    start: PhysicalPosition<f64>,
    last_y: f64,
    remainder: f64,
    slop: f64,
    swiping: bool,
    aborted: bool,
}

/// One finger operates the existing left mouse button. Other fingers are
/// ignored until every contact has lifted; an already-down finger never becomes
/// a fresh click when the primary one lifts.
#[derive(Default)]
pub(super) struct IosTouchPointer {
    contacts: Vec<(DeviceId, u64)>,
    primary: Option<(DeviceId, u64)>,
    scroll: Option<ScrollGesture>,
}

impl IosTouchPointer {
    pub(super) fn scroll_target(&self) -> Option<ScrollTarget> {
        self.scroll.as_ref().map(|gesture| gesture.region.target)
    }

    /// List contents reserve the gesture before mouse-down can select a row.
    /// A tap enters the existing mouse owner on release; a swipe changes only
    /// the captured list's scroll position, even if the finger leaves its rect.
    pub(super) fn translate_with_scroll(
        &mut self,
        touch: Touch,
        region: Option<ScrollRegion>,
        target_active: bool,
        scale_factor: f64,
    ) -> TouchTranslation {
        let key = (touch.device_id, touch.id);
        let first = self.contacts.is_empty();
        let native = self.translate(touch);
        if touch.phase == TouchPhase::Started && first && self.primary == Some(key) {
            if let Some(region) = region {
                self.scroll = Some(ScrollGesture {
                    key,
                    region,
                    start: touch.location,
                    last_y: touch.location.y,
                    remainder: 0.0,
                    slop: 7.0 * scale_factor.max(1.0),
                    swiping: false,
                    aborted: false,
                });
                return TouchTranslation::default();
            }
        }
        if let Some(mut gesture) = self.scroll.take() {
            gesture.aborted |= !target_active;
            let mut result = TouchTranslation::default();
            if gesture.key == key {
                gesture.aborted |= touch.phase == TouchPhase::Cancelled;
                if !gesture.aborted {
                    let displacement = (
                        touch.location.x - gesture.start.x,
                        touch.location.y - gesture.start.y,
                    );
                    let was_swiping = gesture.swiping;
                    gesture.swiping |=
                        displacement.0.abs() > gesture.slop || displacement.1.abs() > gesture.slop;
                    if gesture.swiping {
                        gesture.remainder +=
                            (gesture.last_y - touch.location.y) / gesture.region.row_height;
                        gesture.last_y = touch.location.y;
                        let rows = gesture.remainder.trunc() as i32;
                        gesture.remainder -= f64::from(rows);
                        if rows != 0 || !was_swiping {
                            result.scroll = Some((gesture.region.target, rows));
                        }
                    } else if touch.phase == TouchPhase::Ended {
                        result.events = vec![
                            cursor_moved(touch.device_id, touch.location),
                            left_button(touch.device_id, ElementState::Pressed),
                            left_button(touch.device_id, ElementState::Released),
                        ];
                    }
                }
                if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    return result;
                }
            }
            self.scroll = Some(gesture);
            return result;
        }
        let cancel_pointer = touch.phase == TouchPhase::Cancelled && native[1].is_some();
        TouchTranslation {
            events: native.into_iter().flatten().collect(),
            scroll: None,
            cancel_pointer,
        }
    }

    /// Translate into at most a cursor move followed by a button edge. Route
    /// these events through the ordinary `ApplicationHandler::window_event`.
    ///
    /// On cancellation the caller must also clear the existing in-game pointer
    /// capture before dispatching these events. Releasing outside the window
    /// clears shell button presses without activating their release hit-test.
    pub(super) fn translate(&mut self, touch: Touch) -> [Option<WindowEvent>; 2] {
        let identity = (touch.device_id, touch.id);
        match touch.phase {
            TouchPhase::Started => {
                if self.contacts.contains(&identity) {
                    return [None, None];
                }
                let first_contact = self.contacts.is_empty();
                self.contacts.push(identity);
                if !first_contact {
                    return [None, None];
                }
                self.primary = Some(identity);
                [
                    Some(cursor_moved(touch.device_id, touch.location)),
                    Some(left_button(touch.device_id, ElementState::Pressed)),
                ]
            }
            TouchPhase::Moved if self.primary == Some(identity) => {
                [Some(cursor_moved(touch.device_id, touch.location)), None]
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.contacts.retain(|contact| *contact != identity);
                if self.primary != Some(identity) {
                    return [None, None];
                }
                self.primary = None;
                let position = if touch.phase == TouchPhase::Cancelled {
                    PhysicalPosition::new(-1.0, -1.0)
                } else {
                    touch.location
                };
                [
                    Some(cursor_moved(touch.device_id, position)),
                    Some(left_button(touch.device_id, ElementState::Released)),
                ]
            }
            TouchPhase::Moved => [None, None],
        }
    }

    /// Call alongside the existing focus-loss/suspension pointer cleanup. That
    /// cleanup owns releasing buttons and captures; this drops only touch IDs.
    pub(super) fn reset(&mut self) {
        self.contacts.clear();
        self.primary = None;
        self.scroll = None;
    }
}

fn cursor_moved(device_id: DeviceId, position: PhysicalPosition<f64>) -> WindowEvent {
    WindowEvent::CursorMoved {
        device_id,
        position,
    }
}

fn left_button(device_id: DeviceId, state: ElementState) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id,
        state,
        button: MouseButton::Left,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: None,
            id,
        }
    }

    #[test]
    fn primary_tap_and_drag_keep_physical_coordinates_and_edge_order() {
        let mut pointer = IosTouchPointer::default();
        let device = DeviceId::dummy();

        assert_eq!(
            pointer.translate(touch(1, TouchPhase::Started, 120.5, 240.25)),
            [
                Some(cursor_moved(device, PhysicalPosition::new(120.5, 240.25))),
                Some(left_button(device, ElementState::Pressed)),
            ]
        );
        assert_eq!(
            pointer.translate(touch(1, TouchPhase::Moved, 130.5, 250.25)),
            [
                Some(cursor_moved(device, PhysicalPosition::new(130.5, 250.25))),
                None,
            ]
        );
        assert_eq!(
            pointer.translate(touch(1, TouchPhase::Ended, 140.5, 260.25)),
            [
                Some(cursor_moved(device, PhysicalPosition::new(140.5, 260.25))),
                Some(left_button(device, ElementState::Released)),
            ]
        );
    }

    #[test]
    fn secondary_contacts_never_steal_or_restart_the_primary_click() {
        let mut pointer = IosTouchPointer::default();
        pointer.translate(touch(1, TouchPhase::Started, 10.0, 20.0));
        for phase in [TouchPhase::Started, TouchPhase::Moved] {
            assert_eq!(pointer.translate(touch(2, phase, 80.0, 90.0)), [None, None]);
        }
        assert_eq!(
            pointer.translate(touch(2, TouchPhase::Cancelled, 80.0, 90.0)),
            [None, None]
        );
        assert!(pointer.translate(touch(1, TouchPhase::Moved, 12.0, 22.0))[0].is_some());
        assert_eq!(
            pointer.translate(touch(2, TouchPhase::Started, 80.0, 90.0)),
            [None, None]
        );
        pointer.translate(touch(1, TouchPhase::Ended, 10.0, 20.0));
        assert_eq!(
            pointer.translate(touch(2, TouchPhase::Moved, 80.0, 90.0)),
            [None, None]
        );
        assert_eq!(
            pointer.translate(touch(3, TouchPhase::Started, 50.0, 60.0)),
            [None, None]
        );
        assert_eq!(
            pointer.translate(touch(2, TouchPhase::Ended, 80.0, 90.0)),
            [None, None]
        );
        assert_eq!(
            pointer.translate(touch(3, TouchPhase::Ended, 50.0, 60.0)),
            [None, None]
        );
        assert!(matches!(
            pointer.translate(touch(4, TouchPhase::Started, 30.0, 40.0))[1],
            Some(WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            })
        ));
    }

    #[test]
    fn cancellation_releases_outside_and_a_later_contact_can_click() {
        let mut pointer = IosTouchPointer::default();
        pointer.translate(touch(1, TouchPhase::Started, 10.0, 20.0));

        assert_eq!(
            pointer.translate(touch(1, TouchPhase::Cancelled, 10.0, 20.0)),
            [
                Some(cursor_moved(
                    DeviceId::dummy(),
                    PhysicalPosition::new(-1.0, -1.0)
                )),
                Some(left_button(DeviceId::dummy(), ElementState::Released)),
            ]
        );
        assert_eq!(
            pointer.translate(touch(1, TouchPhase::Ended, 10.0, 20.0)),
            [None, None]
        );
        assert!(pointer.translate(touch(2, TouchPhase::Started, 30.0, 40.0))[1].is_some());
    }

    #[test]
    fn reset_drops_stale_fingers_after_existing_pointer_cleanup() {
        let mut pointer = IosTouchPointer::default();
        pointer.translate(touch(1, TouchPhase::Started, 10.0, 20.0));
        pointer.translate(touch(2, TouchPhase::Started, 30.0, 40.0));
        pointer.reset();

        for phase in [TouchPhase::Moved, TouchPhase::Ended, TouchPhase::Cancelled] {
            assert_eq!(pointer.translate(touch(1, phase, 10.0, 20.0)), [None, None]);
        }
        assert!(pointer.translate(touch(3, TouchPhase::Started, 50.0, 60.0))[1].is_some());
    }
}
