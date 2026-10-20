//! iOS touch delivery for the existing mouse-owned shell and tactical input.
//!
//! winit 0.30.12's `platform_impl/ios/view.rs::handle_touches` already converts
//! UIKit locations to physical window coordinates using `contentScaleFactor`.
//! Feed those unchanged through `WindowEvent::CursorMoved`, so the app's normal
//! cursor-space conversion remains the only owner of shell/viewport scaling.
//! https://github.com/rust-windowing/winit/blob/v0.30.12/src/platform_impl/ios/view.rs

use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, Touch, TouchPhase, WindowEvent};

/// One finger operates the existing left mouse button. Other fingers are
/// ignored until every contact has lifted; an already-down finger never becomes
/// a fresh click when the primary one lifts.
#[derive(Default)]
pub(super) struct IosTouchPointer {
    contacts: Vec<(DeviceId, u64)>,
    primary: Option<(DeviceId, u64)>,
}

impl IosTouchPointer {
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
