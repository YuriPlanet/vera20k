//! Hardware mouse and touchpad events forwarded by VeraActivity. winit's
//! Android backend routes touch contacts and drops mouse hover and wheel events.

use std::collections::HashMap;
use std::sync::Mutex;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

const DOWN: i32 = 0;
const UP: i32 = 1;
const MOVE: i32 = 2;
const CANCEL: i32 = 3;
const POINTER_DOWN: i32 = 5;
const POINTER_UP: i32 = 6;
const HOVER_MOVE: i32 = 7;
const SCROLL: i32 = 8;
const HOVER_ENTER: i32 = 9;
const HOVER_EXIT: i32 = 10;
const BUTTON_PRESS: i32 = 11;
const BUTTON_RELEASE: i32 = 12;
const TOOL_FINGER: i32 = 1;

const BUTTONS: [(i32, MouseButton); 5] = [
    (1, MouseButton::Left),
    (2, MouseButton::Right),
    (4, MouseButton::Middle),
    (8, MouseButton::Back),
    (16, MouseButton::Forward),
];

/// One MotionEvent from Android's UI thread. Coordinates are window pixels
/// and event_time is Android's monotonic uptime in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct PointerEvent {
    pub device_id: i32,
    pub event_time: i64,
    pub action: i32,
    pub action_button: i32,
    pub buttons: i32,
    pub x: f32,
    pub y: f32,
    pub vscroll: f32,
    pub hscroll: f32,
    pub scroll: f32,
    pub tool: i32,
}

static QUEUE: Mutex<Vec<PointerEvent>> = Mutex::new(Vec::new());

/// Queue an event. The UI thread wakes winit when the queue becomes nonempty.
pub fn push(event: PointerEvent) -> bool {
    let Ok(mut queue) = QUEUE.lock() else {
        return false;
    };
    let wake = queue.is_empty();
    if event.action == HOVER_MOVE
        && let Some(last) = queue.last_mut()
        && last.action == HOVER_MOVE
        && last.device_id == event.device_id
    {
        *last = event;
    } else {
        queue.push(event);
    }
    wake
}

pub(crate) fn take() -> Vec<PointerEvent> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

#[derive(Default)]
struct Device {
    held: i32,
    scroll_sample: Option<(i64, f32, f32)>,
}

/// Button transitions and zero-axis touchpad samples belong to each device.
#[derive(Default)]
pub(crate) struct PointerTracker {
    devices: HashMap<i32, Device>,
}

impl PointerTracker {
    pub(crate) fn translate(
        &mut self,
        event: PointerEvent,
        in_game: bool,
        out: &mut Vec<WindowEvent>,
    ) {
        let device = self.devices.entry(event.device_id).or_default();
        let id = DeviceId::dummy();
        let cursor = || WindowEvent::CursorMoved {
            device_id: id,
            position: PhysicalPosition::new(f64::from(event.x), f64::from(event.y)),
        };
        let mut held = device.held;
        match event.action {
            SCROLL => {
                // Physical wheels report axes. A touchpad with no axes can
                // instead report successive gesture positions in window pixels.
                let vertical = if event.vscroll == 0.0 {
                    event.scroll
                } else {
                    event.vscroll
                };
                let delta = if vertical != 0.0 || event.hscroll != 0.0 {
                    device.scroll_sample = None;
                    MouseScrollDelta::LineDelta(event.hscroll, vertical)
                } else if event.tool == TOOL_FINGER {
                    let previous =
                        device
                            .scroll_sample
                            .replace((event.event_time, event.x, event.y));
                    let Some((time, x, y)) = previous else { return };
                    if !(0..=250).contains(&(event.event_time.saturating_sub(time))) {
                        return;
                    }
                    let (dx, dy) = (event.x - x, event.y - y);
                    if dx == 0.0 && dy == 0.0 {
                        return;
                    }
                    if in_game {
                        out.push(WindowEvent::PanGesture {
                            device_id: id,
                            delta: PhysicalPosition::new(dx, dy),
                            phase: TouchPhase::Moved,
                        });
                        return;
                    }
                    MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                        f64::from(dx),
                        f64::from(dy),
                    ))
                } else {
                    return;
                };
                out.push(cursor());
                out.push(WindowEvent::MouseWheel {
                    device_id: id,
                    delta,
                    phase: TouchPhase::Moved,
                });
            }
            HOVER_ENTER | HOVER_MOVE | MOVE | DOWN | POINTER_DOWN | POINTER_UP | UP
            | BUTTON_PRESS | BUTTON_RELEASE => {
                // Duplicate DOWN/BUTTON_PRESS and BUTTON_RELEASE/UP reports
                // deliver each transition once. Android reports all held bits
                // on ordinary mouse events; some finger-tool touchpads send
                // their primary press without a bit.
                let bit = if event.tool == TOOL_FINGER && event.action_button == 0 {
                    1
                } else {
                    event.action_button
                };
                match event.action {
                    BUTTON_PRESS => held |= bit,
                    BUTTON_RELEASE => held &= !bit,
                    UP => held = 0,
                    DOWN if event.tool == TOOL_FINGER && event.buttons == 0 => held |= 1,
                    HOVER_MOVE | HOVER_ENTER => {}
                    MOVE | POINTER_DOWN | POINTER_UP
                        if event.tool == TOOL_FINGER && event.buttons == 0 => {}
                    _ => held = event.buttons,
                }
                if matches!(event.action, DOWN | BUTTON_PRESS | HOVER_ENTER) {
                    device.scroll_sample = None;
                }
                out.push(cursor());
            }
            CANCEL | HOVER_EXIT => {
                held = 0;
                device.scroll_sample = None;
            }
            _ => return,
        }
        for (bit, button) in BUTTONS {
            if (held ^ device.held) & bit != 0 {
                let state = if held & bit == 0 {
                    ElementState::Released
                } else {
                    ElementState::Pressed
                };
                out.push(WindowEvent::MouseInput {
                    device_id: id,
                    state,
                    button,
                });
            }
        }
        device.held = held;
        if held == 0 && device.scroll_sample.is_none() {
            self.devices.remove(&event.device_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_click_reports_deliver_one_press_and_one_release() {
        let mut pointer = PointerTracker::default();
        let mut out = Vec::new();
        for (action, action_button, buttons) in [
            (DOWN, 0, 1),
            (BUTTON_PRESS, 1, 1),
            (BUTTON_RELEASE, 1, 0),
            (UP, 0, 0),
        ] {
            pointer.translate(
                PointerEvent {
                    action,
                    action_button,
                    buttons,
                    ..Default::default()
                },
                true,
                &mut out,
            );
        }
        let edges: Vec<_> = out
            .iter()
            .filter_map(|event| match event {
                WindowEvent::MouseInput {
                    button: MouseButton::Left,
                    state,
                    ..
                } => Some(*state),
                _ => None,
            })
            .collect();
        assert_eq!(edges, [ElementState::Pressed, ElementState::Released]);
    }

    #[test]
    fn the_first_wheel_at_an_offset_position_scrolls_without_panning() {
        let mut out = Vec::new();
        PointerTracker::default().translate(
            PointerEvent {
                action: SCROLL,
                x: 900.0,
                y: 600.0,
                vscroll: -1.0,
                ..Default::default()
            },
            true,
            &mut out,
        );
        assert!(matches!(
            out.as_slice(),
            [
                WindowEvent::CursorMoved { .. },
                WindowEvent::MouseWheel {
                    delta: MouseScrollDelta::LineDelta(0.0, -1.0),
                    ..
                }
            ]
        ));
    }

    #[test]
    fn a_touchpad_without_scroll_axes_pans_only_between_gesture_samples() {
        let mut pointer = PointerTracker::default();
        let mut out = Vec::new();
        let sample = PointerEvent {
            action: SCROLL,
            tool: TOOL_FINGER,
            x: 120.0,
            y: 90.0,
            ..Default::default()
        };
        pointer.translate(sample, true, &mut out);
        pointer.translate(
            PointerEvent {
                x: 126.0,
                y: 88.0,
                event_time: 12,
                ..sample
            },
            true,
            &mut out,
        );
        pointer.translate(
            PointerEvent {
                event_time: 900,
                ..sample
            },
            true,
            &mut out,
        );
        assert!(
            matches!(out.as_slice(), [WindowEvent::PanGesture { delta, .. }] if (delta.x, delta.y) == (6.0, -2.0))
        );
    }
}
