//! Hardware mouse and touchpad events from the activity's Java dispatch.
//!
//! winit 0.30's Android backend maps every `MotionEvent` to `Touch` and drops
//! `ACTION_HOVER_MOVE`, `ACTION_SCROLL` and `ACTION_BUTTON_PRESS/RELEASE`
//! (`TODO mouse events`). A mouse therefore never moves the pointer and its
//! clicks are lost. `VeraActivity` consumes pointing-device events and passes
//! them here; the event loop turns them into the `CursorMoved` / `MouseInput` /
//! `MouseWheel` events the rest of the client already handles.

use std::collections::VecDeque;
use std::sync::Mutex;

use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

/// Android `MotionEvent` action codes this bridge understands.
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

/// One `MotionEvent` the activity forwarded.
#[derive(Clone, Copy, Debug)]
pub struct PointerEvent {
    pub device_id: i32,
    pub action: i32,
    pub action_button: i32,
    pub buttons: i32,
    pub x: f32,
    pub y: f32,
    pub vscroll: f32,
    pub hscroll: f32,
    pub scroll: f32,
    pub pointer_count: i32,
    pub action_index: i32,
    pub tool: i32,
}

static QUEUE: Mutex<VecDeque<PointerEvent>> = Mutex::new(VecDeque::new());
/// Each device's held buttons, so a `DOWN` and its `BUTTON_PRESS` press once.
static PRESSED: Mutex<Vec<(i32, u32)>> = Mutex::new(Vec::new());
/// The previous scroll position per device; Android reports zero scroll axes
/// here, so a touchpad's drag has to be read from the pointer itself.
static LAST_SCROLL: Mutex<Vec<(i32, f32, f32, u64)>> = Mutex::new(Vec::new());
/// The touchpad drag accumulated since the last frame, in screen pixels.
static PAN: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));

/// Takes the touchpad drag the next frame should pan the camera by.
pub fn take_pan() -> Option<(f32, f32)> {
    let mut pan = PAN.lock().ok()?;
    let taken = *pan;
    *pan = (0.0, 0.0);
    (taken != (0.0, 0.0)).then_some(taken)
}
/// Devices that have scrolled: their pointer samples belong to gestures, not
/// to clicks or hover moves, and must not drag the game's pointer.
static SCROLLED_AT: Mutex<Vec<(i32, u64)>> = Mutex::new(Vec::new());
/// The frame's wheel steps at the gesture positions: menus send them to the
/// list or trackbar under the fingers.
static SCROLL_STEPS: Mutex<Vec<(f32, f32, i32)>> = Mutex::new(Vec::new());

/// Takes the frame's wheel steps at their gesture positions.
pub fn take_scroll_steps() -> Vec<(f32, f32, i32)> {
    SCROLL_STEPS
        .lock()
        .map(|mut steps| std::mem::take(&mut *steps))
        .unwrap_or_default()
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Queues one pointing-device event for the event loop.
pub fn push(event: PointerEvent) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push_back(event);
    }
}

fn take() -> Vec<PointerEvent> {
    QUEUE
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

fn scrolled_recently(device_id: i32, now: u64) -> bool {
    SCROLLED_AT
        .lock()
        .ok()
        .and_then(|times| {
            times
                .iter()
                .find(|(device, _)| *device == device_id)
                .map(|(_, at)| now.saturating_sub(*at) <= 100)
        })
        .unwrap_or(false)
}

fn mark_scrolled(device_id: i32, now: u64) {
    if let Ok(mut times) = SCROLLED_AT.lock() {
        if let Some(entry) = times.iter_mut().find(|(device, _)| *device == device_id) {
            entry.1 = now;
        } else {
            times.push((device_id, now));
        }
    }
}

fn button(bit: i32) -> Option<MouseButton> {
    match bit {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Right),
        4 => Some(MouseButton::Middle),
        8 => Some(MouseButton::Back),
        16 => Some(MouseButton::Forward),
        _ => None,
    }
}

fn held(device_id: i32) -> u32 {
    PRESSED
        .lock()
        .ok()
        .and_then(|pressed| {
            pressed
                .iter()
                .find(|(device, _)| *device == device_id)
                .map(|(_, mask)| *mask)
        })
        .unwrap_or(0)
}

fn set_held(device_id: i32, mask: u32) {
    if let Ok(mut pressed) = PRESSED.lock() {
        if let Some(entry) = pressed.iter_mut().find(|(device, _)| *device == device_id) {
            entry.1 = mask;
        } else {
            pressed.push((device_id, mask));
        }
    }
}

fn cursor(x: f32, y: f32) -> WindowEvent {
    WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: winit::dpi::PhysicalPosition::new(f64::from(x), f64::from(y)),
    }
}

fn press(button: MouseButton) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Pressed,
        button,
    }
}

fn release(button: MouseButton) -> WindowEvent {
    WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button,
    }
}

/// Translates every queued event into the window events the client handles.
pub fn drain_events() -> Vec<WindowEvent> {
    let mut out = Vec::new();
    let events = take();
    let now = now_ms();
    // A scroll may follow its pointer samples within the same batch: suppress
    // those earlier samples too, not only the ones after it.
    let scrolling: Vec<i32> = events
        .iter()
        .filter(|event| event.action == SCROLL)
        .map(|event| event.device_id)
        .collect();
    for event in events {
        if event.action == SCROLL {
            mark_scrolled(event.device_id, now);
        }
        // A hover exit means the pointer left the window: whatever was held is
        // over, or the next touch would resume a drag the user already ended.
        if event.action == HOVER_EXIT {
            let mask = held(event.device_id);
            if mask != 0 {
                for bit in [1, 2, 4, 8, 16] {
                    if mask & (bit as u32) != 0
                        && let Some(button) = button(bit)
                    {
                        out.push(release(button));
                    }
                }
                set_held(event.device_id, 0);
            }
            continue;
        }
        // The pointer samples Android emits during a two-finger scroll are not
        // clicks: without this they move the cursor and select list rows. A
        // button action is never one of those samples, or its release would be
        // lost and the next move would drag with the button still held.
        let cursor_sample = matches!(event.action, HOVER_MOVE | MOVE | POINTER_DOWN | POINTER_UP);
        if cursor_sample
            && (scrolling.contains(&event.device_id) || scrolled_recently(event.device_id, now))
        {
            continue;
        }
        let mask = held(event.device_id);
        let mut pressed = mask;
        let cursor_first = |out: &mut Vec<WindowEvent>| out.push(cursor(event.x, event.y));
        match event.action {
            // The activity is told the pointer's position when it enters the
            // window even if the pointer does not move; without this the
            // engine's cursor stays at the top-left until the first move.
            HOVER_ENTER => {
                cursor_first(&mut out);
            }
            HOVER_MOVE | MOVE | POINTER_DOWN if event.buttons == 0 => {
                cursor_first(&mut out);
            }
            DOWN | MOVE | POINTER_DOWN => {
                cursor_first(&mut out);
                for bit in [1, 2, 4, 8, 16] {
                    if event.buttons & bit != 0 && pressed & (bit as u32) == 0 {
                        if let Some(button) = button(bit) {
                            out.push(press(button));
                            pressed |= bit as u32;
                        }
                    }
                }
            }
            BUTTON_PRESS => {
                cursor_first(&mut out);
                // The pogo touchpad reports its press with a finger tool and no
                // primary button bit; treat it as the left button it stands for.
                let bit = if event.tool == 1 && event.action_button == 0 {
                    1
                } else {
                    event.action_button
                };
                if let Some(button) = button(bit)
                    && pressed & (bit as u32) == 0
                {
                    out.push(press(button));
                    pressed |= bit as u32;
                }
            }
            BUTTON_RELEASE => {
                cursor_first(&mut out);
                let bit = if event.tool == 1 && event.action_button == 0 {
                    1
                } else {
                    event.action_button
                };
                if let Some(button) = button(bit)
                    && pressed & (bit as u32) != 0
                {
                    out.push(release(button));
                    pressed &= !(bit as u32);
                }
            }
            UP | POINTER_UP => {
                cursor_first(&mut out);
                for bit in [1, 2, 4, 8, 16] {
                    if pressed & (bit as u32) != 0 {
                        if let Some(button) = button(bit) {
                            out.push(release(button));
                            pressed &= !(bit as u32);
                        }
                    }
                }
            }
            CANCEL => {
                for bit in [1, 2, 4, 8, 16] {
                    if pressed & (bit as u32) != 0 {
                        if let Some(button) = button(bit) {
                            out.push(release(button));
                            pressed &= !(bit as u32);
                        }
                    }
                }
            }
            SCROLL => {
                // Android sends zero axes for both this trackpad and the
                // mouse wheel. A touchpad drag still moves the pointer, so its
                // step is the drag; a wheel at a fixed position gives nothing.
                let now = now_ms();
                let previous = LAST_SCROLL.lock().ok().and_then(|mut last| {
                    let found = last
                        .iter()
                        .find(|(device, _, _, _)| *device == event.device_id)
                        .filter(|(_, _, _, at)| now.saturating_sub(*at) <= 250)
                        .map(|(_, x, y, _)| (*x, *y));
                    if let Some(entry) = last
                        .iter_mut()
                        .find(|(device, _, _, _)| *device == event.device_id)
                    {
                        entry.1 = event.x;
                        entry.2 = event.y;
                        entry.3 = now;
                    } else {
                        last.push((event.device_id, event.x, event.y, now));
                    }
                    found
                });
                let (dx, dy) = if event.vscroll != 0.0 || event.hscroll != 0.0 {
                    (event.hscroll, event.vscroll)
                } else if let Some((x, y)) = previous {
                    ((event.x - x) / 60.0, (event.y - y) / 60.0)
                } else {
                    (0.0, 0.0)
                };
                if dx != 0.0 || dy != 0.0 {
                    if let Ok(mut pan) = PAN.lock() {
                        // The camera follows the fingers, as the touchscreen's
                        // two-finger pan does.
                        pan.0 += previous.map_or(0.0, |p| p.0) - event.x;
                        pan.1 += previous.map_or(0.0, |p| p.1) - event.y;
                    }
                    let lines = dy.round() as i32;
                    if event.vscroll == 0.0 && event.hscroll == 0.0 && lines != 0 {
                        if let Ok(mut steps) = SCROLL_STEPS.lock() {
                            steps.push((event.x, event.y, lines));
                        }
                    } else {
                        out.push(WindowEvent::MouseWheel {
                            device_id: DeviceId::dummy(),
                            delta: MouseScrollDelta::LineDelta(dx, dy),
                            phase: TouchPhase::Moved,
                        });
                    }
                }
            }
            _ => {}
        }
        set_held(event.device_id, pressed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use winit::event::WindowEvent;

    /// The queue and pressed mask are process-wide: run these one at a time.
    static LOCK: Mutex<()> = Mutex::new(());

    fn names(events: &[WindowEvent]) -> Vec<&'static str> {
        events
            .iter()
            .map(|event| match event {
                WindowEvent::CursorMoved { .. } => "move",
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => "left-press",
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    ..
                } => "left-release",
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                    ..
                } => "right-press",
                WindowEvent::MouseWheel { .. } => "wheel",
                _ => "other",
            })
            .collect()
    }

    fn event(action: i32, action_button: i32, buttons: i32) -> PointerEvent {
        PointerEvent {
            device_id: 7,
            action,
            action_button,
            buttons,
            x: 10.0,
            y: 20.0,
            vscroll: 0.0,
            hscroll: 0.0,
            scroll: 0.0,
            pointer_count: 1,
            action_index: 0,
            tool: 3,
        }
    }

    #[test]
    fn a_click_and_its_duplicate_button_events_press_once() {
        let _guard = LOCK.lock().unwrap();
        // Android reports one click as DOWN, BUTTON_PRESS, BUTTON_RELEASE, UP.
        for event in [
            event(HOVER_MOVE, 0, 0),
            event(DOWN, 0, 1),
            event(BUTTON_PRESS, 1, 1),
            event(BUTTON_RELEASE, 1, 0),
            event(UP, 0, 0),
        ] {
            push(event);
        }
        let kinds = names(&drain_events());
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "left-press").count(),
            1
        );
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "left-release").count(),
            1
        );
        assert!(kinds.contains(&"move"));
    }

    #[test]
    fn a_right_button_press_and_scroll_keep_their_meaning() {
        let _guard = LOCK.lock().unwrap();
        for event in [
            event(BUTTON_PRESS, 2, 2),
            event(BUTTON_RELEASE, 2, 0),
            PointerEvent {
                vscroll: -1.0,
                ..event(SCROLL, 0, 0)
            },
        ] {
            push(event);
        }
        let kinds = names(&drain_events());
        assert!(kinds.contains(&"right-press"));
        assert!(kinds.contains(&"wheel"));
    }
}
