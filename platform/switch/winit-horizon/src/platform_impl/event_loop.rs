use std::collections::VecDeque;
use std::ffi::c_char;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::dpi::PhysicalPosition;
use crate::error::EventLoopError;
use crate::event::{
    ElementState, Event, Force, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta,
    StartCause, Touch, TouchPhase, WindowEvent,
};
use crate::event_loop::{ControlFlow, DeviceEvents, EventLoopClosed};
use crate::keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey};
use crate::window::{CustomCursor, CustomCursorSource, Theme};

use super::{
    DeviceId, KeyEventExtra, MonitorHandle, PlatformSpecificEventLoopAttributes, WindowId,
};

unsafe extern "C" {
    fn platform_running() -> bool;
    fn platform_buttons() -> u64;
    fn platform_buttons_held() -> u64;
    fn platform_touches(output: *mut VeraTouchState) -> u32;
    fn platform_mouse(
        delta_x: *mut i32,
        delta_y: *mut i32,
        wheel_x: *mut i32,
        wheel_y: *mut i32,
        buttons: *mut u32,
        pressed_buttons: *mut u32,
        released_buttons: *mut u32,
    ) -> bool;
    fn platform_keyboard(keys: *mut u64) -> u64;
    fn platform_sticks(lx: *mut i32, ly: *mut i32, rx: *mut i32, ry: *mut i32);
    fn platform_focused() -> bool;
    fn platform_soft_keyboard_show(output: *mut c_char, output_size: usize) -> u32;
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct VeraTouch {
    id: u32,
    x: u32,
    y: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VeraTouchState {
    count: u32,
    touches: [VeraTouch; 16],
}

impl Default for VeraTouchState {
    fn default() -> Self {
        Self {
            count: 0,
            touches: [VeraTouch::default(); 16],
        }
    }
}

pub struct Shared {
    pub redraw: AtomicBool,
    pub window_created: AtomicBool,
    pub cursor_position: Mutex<PhysicalPosition<f64>>,
    pub touch_pan: Mutex<TouchPan>,
    pub camera_pan: Mutex<CameraPan>,
    pub ime_allowed: AtomicBool,
    pub pending_ime: Mutex<VecDeque<String>>,
    pub focused: AtomicBool,
    exiting: AtomicBool,
    alive: AtomicBool,
    control_flow: Mutex<ControlFlow>,
}

pub struct ActiveEventLoop {
    pub shared: Arc<Shared>,
}

impl ActiveEventLoop {
    pub fn set_control_flow(&self, flow: ControlFlow) {
        *self.shared.control_flow.lock().unwrap() = flow;
    }
    pub fn control_flow(&self) -> ControlFlow {
        *self.shared.control_flow.lock().unwrap()
    }
    pub fn exit(&self) {
        self.shared.exiting.store(true, Ordering::Relaxed);
    }
    pub fn exiting(&self) -> bool {
        self.shared.exiting.load(Ordering::Relaxed)
    }
    pub fn listen_device_events(&self, _: DeviceEvents) {}
    pub fn system_theme(&self) -> Option<Theme> {
        None
    }
    pub fn available_monitors(&self) -> Vec<MonitorHandle> {
        vec![MonitorHandle]
    }
    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle)
    }
    pub fn owned_display_handle(&self) -> OwnedDisplayHandle {
        OwnedDisplayHandle
    }
    pub fn create_custom_cursor(&self, source: CustomCursorSource) -> CustomCursor {
        super::custom_cursor(source)
    }
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        OwnedDisplayHandle.raw_display_handle_rwh_06()
    }
}

#[derive(Clone)]
pub struct OwnedDisplayHandle;
impl OwnedDisplayHandle {
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::HorizonDisplayHandle::new().into())
    }
}

pub struct EventLoop<T: 'static> {
    active: crate::event_loop::ActiveEventLoop,
    sender: mpsc::Sender<T>,
    receiver: mpsc::Receiver<T>,
}

impl<T> EventLoop<T> {
    pub fn new(_: &mut PlatformSpecificEventLoopAttributes) -> Result<Self, EventLoopError> {
        let shared = Arc::new(Shared {
            redraw: AtomicBool::new(false),
            window_created: AtomicBool::new(false),
            cursor_position: Mutex::new(PhysicalPosition::new(640.0, 360.0)),
            touch_pan: Mutex::new(TouchPan::default()),
            camera_pan: Mutex::new(CameraPan::default()),
            ime_allowed: AtomicBool::new(false),
            pending_ime: Mutex::new(VecDeque::new()),
            focused: AtomicBool::new(true),
            exiting: AtomicBool::new(false),
            alive: AtomicBool::new(true),
            control_flow: Mutex::new(ControlFlow::Wait),
        });
        let (sender, receiver) = mpsc::channel();
        Ok(Self {
            active: crate::event_loop::ActiveEventLoop {
                p: ActiveEventLoop { shared },
                _marker: PhantomData,
            },
            sender,
            receiver,
        })
    }

    pub fn window_target(&self) -> &crate::event_loop::ActiveEventLoop {
        &self.active
    }
    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy {
            sender: self.sender.clone(),
            shared: self.active.p.shared.clone(),
        }
    }

    pub fn run(
        self,
        mut callback: impl FnMut(Event<T>, &crate::event_loop::ActiveEventLoop),
    ) -> Result<(), EventLoopError> {
        let active = &self.active;
        callback(Event::NewEvents(StartCause::Init), active);
        callback(Event::Resumed, active);
        let mut input = Input::new();
        active
            .p
            .shared
            .focused
            .store(input.focused, Ordering::Relaxed);
        if active.p.shared.window_created.load(Ordering::Relaxed) && !active.exiting() {
            for event in [
                WindowEvent::Focused(input.focused),
                WindowEvent::CursorEntered {
                    device_id: crate::event::DeviceId(DeviceId(1)),
                },
                WindowEvent::CursorMoved {
                    device_id: crate::event::DeviceId(DeviceId(1)),
                    position: *active.p.shared.cursor_position.lock().unwrap(),
                },
            ] {
                callback(window_event(event), active);
            }
        }
        let mut wait_start = Instant::now();
        while !active.exiting() {
            if !unsafe { platform_running() } {
                // The OS has requested exit, so a modal close confirmation
                // cannot retain the process past its lifecycle deadline.
                active.exit();
                break;
            }
            let events = input.poll(&active.p.shared);
            let user_events: Vec<T> = self.receiver.try_iter().collect();
            let now = Instant::now();
            let flow = active.control_flow();
            let timer_ready = matches!(flow, ControlFlow::WaitUntil(deadline) if now >= deadline);
            let redraw = active.p.shared.redraw.load(Ordering::Relaxed);
            let camera_input = active.p.shared.camera_pan.lock().unwrap().active;
            if events.is_empty()
                && user_events.is_empty()
                && !camera_input
                && !timer_ready
                && !redraw
                && flow != ControlFlow::Poll
            {
                let wait = match flow {
                    ControlFlow::WaitUntil(deadline) => deadline.saturating_duration_since(now),
                    _ => Duration::from_millis(4),
                };
                std::thread::sleep(wait.min(Duration::from_millis(4)));
                continue;
            }
            let cause = match flow {
                ControlFlow::Poll => StartCause::Poll,
                ControlFlow::WaitUntil(requested_resume) if timer_ready => {
                    StartCause::ResumeTimeReached {
                        start: wait_start,
                        requested_resume,
                    }
                }
                flow => StartCause::WaitCancelled {
                    start: wait_start,
                    requested_resume: match flow {
                        ControlFlow::WaitUntil(deadline) => Some(deadline),
                        _ => None,
                    },
                },
            };
            callback(Event::NewEvents(cause), active);
            for event in events {
                if active.exiting() {
                    break;
                }
                callback(window_event(event), active);
            }
            for event in user_events {
                if active.exiting() {
                    break;
                }
                callback(Event::UserEvent(event), active);
            }
            if !active.exiting() && active.p.shared.redraw.swap(false, Ordering::Relaxed) {
                callback(window_event(WindowEvent::RedrawRequested), active);
            }
            if !active.exiting() {
                callback(Event::AboutToWait, active);
            }
            wait_start = Instant::now();
        }
        callback(Event::LoopExiting, active);
        Ok(())
    }
}

impl<T> Drop for EventLoop<T> {
    fn drop(&mut self) {
        self.active.p.shared.alive.store(false, Ordering::Release);
    }
}

pub struct EventLoopProxy<T> {
    sender: mpsc::Sender<T>,
    shared: Arc<Shared>,
}
impl<T> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}
impl<T> EventLoopProxy<T> {
    pub fn send_event(&self, value: T) -> Result<(), EventLoopClosed<T>> {
        if !self.shared.alive.load(Ordering::Acquire) {
            return Err(EventLoopClosed(value));
        }
        self.sender
            .send(value)
            .map_err(|error| EventLoopClosed(error.0))
    }
}

fn window_event<T>(event: WindowEvent) -> Event<T> {
    Event::WindowEvent {
        window_id: crate::window::WindowId(WindowId(1)),
        event,
    }
}

/// Backend-owned two-finger pan proposal. The app converts the window-pixel
/// midpoint displacement into its camera owner on the next rendered frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchPan {
    pub delta: (f32, f32),
    pub active: bool,
}

/// Backend-owned right-stick camera proposal, in logical window pixels for the
/// sampling interval. The app converts it through the same presentation and
/// view scale as a two-finger drag.
#[derive(Clone, Copy, Debug, Default)]
pub struct CameraPan {
    pub delta: (f32, f32),
    pub active: bool,
}

#[derive(Default)]
struct TouchPanGesture {
    previous_midpoint: Option<PhysicalPosition<f64>>,
    active: bool,
}

impl TouchPanGesture {
    fn observe(&mut self, touches: &[(u64, PhysicalPosition<f64>)]) -> TouchPan {
        let midpoint = touch_midpoint(touches);
        let delta = if let Some(midpoint) = midpoint {
            let delta = self.previous_midpoint.map_or((0.0, 0.0), |previous| {
                (
                    (midpoint.x - previous.x) as f32,
                    (midpoint.y - previous.y) as f32,
                )
            });
            self.previous_midpoint = Some(midpoint);
            self.active = true;
            delta
        } else {
            // Keep `active` latched until every finger lifts. Releasing one
            // finger must not turn the remaining touch into a synthetic click.
            self.previous_midpoint = None;
            if touches.is_empty() {
                self.active = false;
            }
            (0.0, 0.0)
        };
        TouchPan {
            delta,
            active: self.active,
        }
    }
}

fn touch_midpoint(touches: &[(u64, PhysicalPosition<f64>)]) -> Option<PhysicalPosition<f64>> {
    (touches.len() >= 2).then(|| {
        PhysicalPosition::new(
            touches.iter().take(2).map(|touch| touch.1.x).sum::<f64>() * 0.5,
            touches.iter().take(2).map(|touch| touch.1.y).sum::<f64>() * 0.5,
        )
    })
}

fn show_soft_keyboard() -> Option<String> {
    // swkbd reports UTF-8 bytes; 1024 characters can occupy 4096 bytes plus
    // the terminator, so leave another buffer's worth of headroom.
    let mut output = [0_u8; 8192];
    let result =
        unsafe { platform_soft_keyboard_show(output.as_mut_ptr().cast::<c_char>(), output.len()) };
    if result != 0 {
        return None;
    }
    let length = output
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(output.len());
    std::str::from_utf8(&output[..length])
        .ok()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

struct Input {
    pointer_buttons: u32,
    keyboard: [u64; 4],
    modifiers: crate::keyboard::ModifiersState,
    active_touches: Vec<(u64, PhysicalPosition<f64>)>,
    touch_gesture_latched: bool,
    touch_pan: TouchPanGesture,
    position: PhysicalPosition<f64>,
    focused: bool,
    last_sample: Instant,
}

impl Input {
    fn new() -> Self {
        Self {
            pointer_buttons: 0,
            keyboard: [0; 4],
            modifiers: crate::keyboard::ModifiersState::empty(),
            active_touches: Vec::new(),
            touch_gesture_latched: false,
            touch_pan: TouchPanGesture::default(),
            position: PhysicalPosition::new(640.0, 360.0),
            focused: unsafe { platform_focused() },
            last_sample: Instant::now(),
        }
    }

    fn poll(&mut self, shared: &Shared) -> Vec<WindowEvent> {
        let mut events = Vec::new();
        let device_id = crate::event::DeviceId(DeviceId(1));
        let pending_ime = std::mem::take(
            &mut *shared
                .pending_ime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for text in pending_ime {
            events.push(WindowEvent::Ime(Ime::Commit(text)));
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_sample).as_secs_f64().min(0.05);
        self.last_sample = now;
        // `platform_buttons` updates the pad and returns this sample's new
        // controller edges. Held state is queried separately after that.
        let pressed_buttons = unsafe { platform_buttons() };
        let focused = unsafe { platform_focused() };
        shared.focused.store(focused, Ordering::Relaxed);
        let held = if focused {
            unsafe { platform_buttons_held() }
        } else {
            0
        };
        if focused != self.focused {
            events.push(WindowEvent::Focused(focused));
            self.focused = focused;
        }
        let mut keyboard = [0_u64; 4];
        let keyboard_modifiers = if focused {
            unsafe { platform_keyboard(keyboard.as_mut_ptr()) }
        } else {
            0
        };
        let ime_allowed = focused && shared.ime_allowed.load(Ordering::Acquire);
        // Merge physical keys before computing edges so releasing a controller
        // modifier cannot clear the same modifier still held on a USB keyboard.
        for (button, hid_key) in [
            (2, 7),
            (3, 4),
            (6, 225),
            (7, 224),
            (8, 43),
            (9, 44),
            (10, 41),
            (11, 67),
            (12, 10), // D-pad Left: Guard (G).
            (13, 30), // D-pad Up: Team 1.
            (14, 29), // D-pad Right: Planning Mode (Z), held until release.
            (15, 31), // D-pad Down: Team 2.
        ] {
            // An active edit owns controller Plus. Clearing only the edge
            // frame would synthesize Escape on the next poll while it is held.
            if ime_allowed && button == 10 {
                continue;
            }
            if held & (1 << button) != 0 {
                keyboard[hid_key / 64] |= 1 << (hid_key % 64);
            }
        }
        if ime_allowed && pressed_buttons & (1 << 10) != 0 {
            if let Some(text) = show_soft_keyboard() {
                events.push(WindowEvent::Ime(Ime::Commit(text)));
            }
        }
        // Modifiers precede ordinary keys and mouse edges in this poll, so
        // Ctrl+click and Shift+hotkey also work when pressed in the same sample.
        let caps_lock = keyboard_modifiers & (1 << 8) != 0;
        let modifier_state = modifiers_state(&keyboard);
        let modifiers_changed = modifier_state != self.modifiers;
        if modifiers_changed {
            self.modifiers = modifier_state;
            events.push(WindowEvent::ModifiersChanged(Modifiers::from(
                modifier_state,
            )));
        }
        for event in keyboard_edges(&self.keyboard, &keyboard, caps_lock) {
            events.push(WindowEvent::KeyboardInput {
                device_id,
                event,
                is_synthetic: false,
            });
        }
        self.keyboard = keyboard;
        let mut touch_state = VeraTouchState::default();
        let touch_count = if focused {
            unsafe { platform_touches(&mut touch_state) }
        } else {
            0
        };
        let touches = touch_state.touches[..usize::try_from(touch_count).unwrap_or(0).min(16)]
            .iter()
            .map(|touch| {
                (
                    u64::from(touch.id),
                    PhysicalPosition::new(
                        f64::from(touch.x.min(1279)),
                        f64::from(touch.y.min(719)),
                    ),
                )
            })
            .collect::<Vec<_>>();
        let had_two_touches = self.active_touches.len() >= 2;
        let has_two_touches = touches.len() >= 2;
        let previous_touches = self.active_touches.clone();
        self.active_touches = touches.clone();
        if has_two_touches {
            if !self.touch_gesture_latched && !previous_touches.is_empty() {
                // A single finger may already have produced a synthetic left
                // press. Close that gesture before treating the pair as a pan.
                events.push(WindowEvent::Touch(Touch {
                    device_id,
                    phase: TouchPhase::Ended,
                    location: previous_touches[0].1,
                    force: None,
                    id: previous_touches[0].0,
                }));
            }
            self.touch_gesture_latched = true;
        } else if touches.is_empty() {
            self.touch_gesture_latched = false;
        }
        let touch_pan = self.touch_pan.observe(&touches);
        *shared.touch_pan.lock().unwrap() = touch_pan;
        let camera_input_allowed = focused && !touch_pan.active && touches.is_empty();
        let touched = focused && !touches.is_empty();
        // The whole gesture is synthetic-left-click suppressed. `touch_pan`
        // itself clears on the release frame, while the latch survives until
        // the last finger lifts.
        let suppress_touch_click = self.touch_gesture_latched || touch_pan.active;
        let touch_left = touched && !suppress_touch_click;
        let (mut mouse_dx, mut mouse_dy, mut wheel_x, mut wheel_y, mut mouse_buttons) =
            (0, 0, 0, 0, 0);
        let (mut mouse_pressed, mut mouse_released) = (0, 0);
        let mouse_present = focused
            && unsafe {
                platform_mouse(
                    &mut mouse_dx,
                    &mut mouse_dy,
                    &mut wheel_x,
                    &mut wheel_y,
                    &mut mouse_buttons,
                    &mut mouse_pressed,
                    &mut mouse_released,
                )
            };
        if !mouse_present {
            mouse_buttons = 0;
            mouse_pressed = 0;
            mouse_released = 0;
        }
        let (mut lx, mut ly, mut rx, mut ry) = (0, 0, 0, 0);
        if focused {
            unsafe { platform_sticks(&mut lx, &mut ly, &mut rx, &mut ry) };
        }
        let mut camera_input = None;
        {
            let mut position = shared.cursor_position.lock().unwrap();
            if touched {
                *position = touches[0].1;
            } else if focused && (mouse_dx != 0 || mouse_dy != 0) {
                position.x = (position.x + f64::from(mouse_dx)).clamp(0.0, 1279.0);
                position.y = (position.y + f64::from(mouse_dy)).clamp(0.0, 719.0);
            } else if focused {
                position.x = (position.x + stick_axis(lx) * 850.0 * dt).clamp(0.0, 1279.0);
                position.y = (position.y - stick_axis(ly) * 850.0 * dt).clamp(0.0, 719.0);
            }
            if *position != self.position {
                self.position = *position;
                events.push(WindowEvent::CursorMoved {
                    device_id,
                    position: *position,
                });
            }
        }
        if focused && !touched {
            let (rx, ry) = (stick_axis(rx), stick_axis(ry));
            camera_input = Some(((rx * 850.0 * dt) as f32, (-ry * 850.0 * dt) as f32));
        }
        let mut camera_pan = shared.camera_pan.lock().unwrap();
        if camera_input_allowed {
            if let Some(delta) = camera_input {
                camera_pan.delta = delta;
                camera_pan.active = delta != (0.0, 0.0);
            }
        } else {
            camera_pan.delta = (0.0, 0.0);
            camera_pan.active = false;
        }
        drop(camera_pan);
        let synthetic_buttons = (held as u32 & 3) | u32::from(touch_left);
        for (button, state) in pointer_button_edges(
            self.pointer_buttons,
            synthetic_buttons,
            mouse_buttons,
            mouse_pressed,
            mouse_released,
        ) {
            events.push(WindowEvent::MouseInput {
                device_id,
                state,
                button,
            });
        }
        self.pointer_buttons = synthetic_buttons | mouse_buttons;
        if wheel_x != 0 || wheel_y != 0 {
            events.push(WindowEvent::MouseWheel {
                device_id,
                delta: MouseScrollDelta::LineDelta(wheel_x as f32 / 120.0, wheel_y as f32 / 120.0),
                phase: TouchPhase::Moved,
            });
        }
        if focused && has_two_touches {
            for touch in &touches {
                let phase = if previous_touches
                    .iter()
                    .any(|previous| previous.0 == touch.0)
                {
                    TouchPhase::Moved
                } else {
                    TouchPhase::Started
                };
                events.push(WindowEvent::Touch(Touch {
                    device_id,
                    phase,
                    location: touch.1,
                    force: Some(Force::Normalized(1.0)),
                    id: touch.0,
                }));
            }
        } else if had_two_touches {
            for touch in &previous_touches {
                events.push(WindowEvent::Touch(Touch {
                    device_id,
                    phase: TouchPhase::Ended,
                    location: touch.1,
                    force: None,
                    id: touch.0,
                }));
            }
        }
        events
    }
}

fn pointer_button_edges(
    previous: u32,
    synthetic: u32,
    mouse: u32,
    pressed: u32,
    released: u32,
) -> Vec<(MouseButton, ElementState)> {
    let mut events = Vec::new();
    for (mask, button) in [
        (1, MouseButton::Left),
        (1 << 1, MouseButton::Right),
        (1 << 2, MouseButton::Middle),
        (1 << 3, MouseButton::Forward),
        (1 << 4, MouseButton::Back),
    ] {
        let mut down = previous & mask != 0;
        let mut record = |current: bool| {
            if current != down {
                events.push((
                    button,
                    if current {
                        ElementState::Pressed
                    } else {
                        ElementState::Released
                    },
                ));
                down = current;
            }
        };
        if synthetic & mask == 0 {
            // A complete USB click may occur between polls. When both edges
            // were seen, the final state determines which edge came last.
            if pressed & released & mouse & mask != 0 {
                record(false);
                record(true);
            } else {
                if pressed & mask != 0 {
                    record(true);
                }
                if released & mask != 0 {
                    record(false);
                }
            }
        }
        // A/B, touch and USB share one logical button. Releasing one source
        // must not release a button still held by another source.
        record((synthetic | mouse) & mask != 0);
    }
    events
}

fn stick_axis(value: i32) -> f64 {
    let axis = (f64::from(value) / 32767.0).clamp(-1.0, 1.0);
    if axis.abs() <= 0.15 {
        0.0
    } else {
        axis.signum() * (axis.abs() - 0.15) / 0.85
    }
}

fn character_text(key: &Key) -> Option<smol_str::SmolStr> {
    match key {
        Key::Character(text) => Some(text.clone()),
        Key::Named(NamedKey::Space) => Some(" ".into()),
        _ => None,
    }
}

fn keyboard_edges(previous: &[u64; 4], current: &[u64; 4], caps_lock: bool) -> Vec<KeyEvent> {
    if previous == current {
        return Vec::new();
    }
    let held = |key: usize| current[key / 64] & (1 << (key % 64)) != 0;
    let shift = held(225) || held(229);
    let shortcut = held(224) || held(228) || held(226) || held(230);
    let mut events = Vec::new();
    for hid_key in (224_u16..232).chain(0..224).chain(232..256) {
        let (index, mask) = (usize::from(hid_key / 64), 1_u64 << (hid_key % 64));
        if (previous[index] ^ current[index]) & mask == 0 {
            continue;
        }
        let Some((code, unmodified, location)) = keyboard_mapping(hid_key) else {
            continue;
        };
        let pressed = current[index] & mask != 0;
        let logical = match &unmodified {
            Key::Character(text) if location != KeyLocation::Numpad => Key::Character(
                text.chars()
                    .map(|ch| shifted_character(ch, shift, caps_lock))
                    .collect(),
            ),
            _ => unmodified.clone(),
        };
        let text = (pressed && !shortcut)
            .then(|| character_text(&logical))
            .flatten();
        events.push(KeyEvent {
            physical_key: PhysicalKey::Code(code),
            logical_key: logical,
            text: text.clone(),
            location,
            state: if pressed {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            repeat: false,
            platform_specific: KeyEventExtra {
                key_without_modifiers: unmodified,
                text_with_all_modifiers: text,
            },
        });
    }
    events
}

fn modifiers_state(keys: &[u64; 4]) -> crate::keyboard::ModifiersState {
    let held = |key: usize| keys[key / 64] & (1 << (key % 64)) != 0;
    let mut state = crate::keyboard::ModifiersState::empty();
    state.set(
        crate::keyboard::ModifiersState::SHIFT,
        held(225) || held(229),
    );
    state.set(
        crate::keyboard::ModifiersState::CONTROL,
        held(224) || held(228),
    );
    state.set(crate::keyboard::ModifiersState::ALT, held(226) || held(230));
    state.set(
        crate::keyboard::ModifiersState::SUPER,
        held(227) || held(231),
    );
    state
}

fn shifted_character(ch: char, shift: bool, caps_lock: bool) -> char {
    if ch.is_ascii_alphabetic() {
        return if shift ^ caps_lock {
            ch.to_ascii_uppercase()
        } else {
            ch
        };
    }
    if !shift {
        return ch;
    }
    match ch {
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        '`' => '~',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => ch,
    }
}

fn keyboard_mapping(hid_key: u16) -> Option<(KeyCode, Key, KeyLocation)> {
    use NamedKey as N;
    let standard = |code, key| Some((code, key, KeyLocation::Standard));
    let numpad = |code, key| Some((code, key, KeyLocation::Numpad));
    match hid_key {
        4..=29 => {
            let letter = char::from(b'a' + (hid_key - 4) as u8);
            let code = match letter {
                'a' => KeyCode::KeyA,
                'b' => KeyCode::KeyB,
                'c' => KeyCode::KeyC,
                'd' => KeyCode::KeyD,
                'e' => KeyCode::KeyE,
                'f' => KeyCode::KeyF,
                'g' => KeyCode::KeyG,
                'h' => KeyCode::KeyH,
                'i' => KeyCode::KeyI,
                'j' => KeyCode::KeyJ,
                'k' => KeyCode::KeyK,
                'l' => KeyCode::KeyL,
                'm' => KeyCode::KeyM,
                'n' => KeyCode::KeyN,
                'o' => KeyCode::KeyO,
                'p' => KeyCode::KeyP,
                'q' => KeyCode::KeyQ,
                'r' => KeyCode::KeyR,
                's' => KeyCode::KeyS,
                't' => KeyCode::KeyT,
                'u' => KeyCode::KeyU,
                'v' => KeyCode::KeyV,
                'w' => KeyCode::KeyW,
                'x' => KeyCode::KeyX,
                'y' => KeyCode::KeyY,
                _ => KeyCode::KeyZ,
            };
            standard(code, Key::Character(letter.to_string().into()))
        }
        30..=39 => {
            let digit = match hid_key {
                30 => '1',
                31 => '2',
                32 => '3',
                33 => '4',
                34 => '5',
                35 => '6',
                36 => '7',
                37 => '8',
                38 => '9',
                _ => '0',
            };
            let code = match digit {
                '1' => KeyCode::Digit1,
                '2' => KeyCode::Digit2,
                '3' => KeyCode::Digit3,
                '4' => KeyCode::Digit4,
                '5' => KeyCode::Digit5,
                '6' => KeyCode::Digit6,
                '7' => KeyCode::Digit7,
                '8' => KeyCode::Digit8,
                '9' => KeyCode::Digit9,
                _ => KeyCode::Digit0,
            };
            standard(code, Key::Character(digit.to_string().into()))
        }
        40 => standard(KeyCode::Enter, Key::Named(N::Enter)),
        41 => standard(KeyCode::Escape, Key::Named(N::Escape)),
        42 => standard(KeyCode::Backspace, Key::Named(N::Backspace)),
        43 => standard(KeyCode::Tab, Key::Named(N::Tab)),
        44 => standard(KeyCode::Space, Key::Named(N::Space)),
        45 => standard(KeyCode::Minus, Key::Character("-".into())),
        46 => standard(KeyCode::Equal, Key::Character("=".into())),
        47 => standard(KeyCode::BracketLeft, Key::Character("[".into())),
        48 => standard(KeyCode::BracketRight, Key::Character("]".into())),
        49 => standard(KeyCode::Backslash, Key::Character("\\".into())),
        51 => standard(KeyCode::Semicolon, Key::Character(";".into())),
        52 => standard(KeyCode::Quote, Key::Character("'".into())),
        53 => standard(KeyCode::Backquote, Key::Character("`".into())),
        54 => standard(KeyCode::Comma, Key::Character(",".into())),
        55 => standard(KeyCode::Period, Key::Character(".".into())),
        56 => standard(KeyCode::Slash, Key::Character("/".into())),
        57 => standard(KeyCode::CapsLock, Key::Named(N::CapsLock)),
        58 => standard(KeyCode::F1, Key::Named(N::F1)),
        59 => standard(KeyCode::F2, Key::Named(N::F2)),
        60 => standard(KeyCode::F3, Key::Named(N::F3)),
        61 => standard(KeyCode::F4, Key::Named(N::F4)),
        62 => standard(KeyCode::F5, Key::Named(N::F5)),
        63 => standard(KeyCode::F6, Key::Named(N::F6)),
        64 => standard(KeyCode::F7, Key::Named(N::F7)),
        65 => standard(KeyCode::F8, Key::Named(N::F8)),
        66 => standard(KeyCode::F9, Key::Named(N::F9)),
        67 => standard(KeyCode::F10, Key::Named(N::F10)),
        68 => standard(KeyCode::F11, Key::Named(N::F11)),
        69 => standard(KeyCode::F12, Key::Named(N::F12)),
        70 => standard(KeyCode::PrintScreen, Key::Named(N::PrintScreen)),
        71 => standard(KeyCode::ScrollLock, Key::Named(N::ScrollLock)),
        72 => standard(KeyCode::Pause, Key::Named(N::Pause)),
        73 => standard(KeyCode::Insert, Key::Named(N::Insert)),
        74 => standard(KeyCode::Home, Key::Named(N::Home)),
        75 => standard(KeyCode::PageUp, Key::Named(N::PageUp)),
        76 => standard(KeyCode::Delete, Key::Named(N::Delete)),
        77 => standard(KeyCode::End, Key::Named(N::End)),
        78 => standard(KeyCode::PageDown, Key::Named(N::PageDown)),
        79 => standard(KeyCode::ArrowRight, Key::Named(N::ArrowRight)),
        80 => standard(KeyCode::ArrowLeft, Key::Named(N::ArrowLeft)),
        81 => standard(KeyCode::ArrowDown, Key::Named(N::ArrowDown)),
        82 => standard(KeyCode::ArrowUp, Key::Named(N::ArrowUp)),
        83 => standard(KeyCode::NumLock, Key::Named(N::NumLock)),
        84 => numpad(KeyCode::NumpadDivide, Key::Character("/".into())),
        85 => numpad(KeyCode::NumpadMultiply, Key::Character("*".into())),
        86 => numpad(KeyCode::NumpadSubtract, Key::Character("-".into())),
        87 => numpad(KeyCode::NumpadAdd, Key::Character("+".into())),
        88 => numpad(KeyCode::NumpadEnter, Key::Named(N::Enter)),
        89..=97 => {
            let digit = b'1' + (hid_key - 89) as u8;
            let code = match digit {
                b'1' => KeyCode::Numpad1,
                b'2' => KeyCode::Numpad2,
                b'3' => KeyCode::Numpad3,
                b'4' => KeyCode::Numpad4,
                b'5' => KeyCode::Numpad5,
                b'6' => KeyCode::Numpad6,
                b'7' => KeyCode::Numpad7,
                b'8' => KeyCode::Numpad8,
                _ => KeyCode::Numpad9,
            };
            numpad(code, Key::Character(char::from(digit).to_string().into()))
        }
        98 => numpad(KeyCode::Numpad0, Key::Character("0".into())),
        99 => numpad(KeyCode::NumpadDecimal, Key::Character(".".into())),
        100 => standard(KeyCode::Backslash, Key::Character("\\".into())),
        224 => Some((
            KeyCode::ControlLeft,
            Key::Named(N::Control),
            KeyLocation::Left,
        )),
        225 => Some((KeyCode::ShiftLeft, Key::Named(N::Shift), KeyLocation::Left)),
        226 => Some((KeyCode::AltLeft, Key::Named(N::Alt), KeyLocation::Left)),
        228 => Some((
            KeyCode::ControlRight,
            Key::Named(N::Control),
            KeyLocation::Right,
        )),
        229 => Some((
            KeyCode::ShiftRight,
            Key::Named(N::Shift),
            KeyLocation::Right,
        )),
        230 => Some((KeyCode::AltRight, Key::Named(N::Alt), KeyLocation::Right)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Devices {
        buttons_down: u64,
        held: u64,
        touches: Vec<(u32, u32, u32)>,
        mouse: u32,
        pressed: u32,
        released: u32,
        unfocused: bool,
        keyboard_text: Option<&'static str>,
        keyboard_calls: u32,
    }

    thread_local! {
        static DEVICES: std::cell::RefCell<Devices> = std::cell::RefCell::default();
    }

    #[unsafe(no_mangle)]
    extern "C" fn platform_buttons() -> u64 {
        DEVICES.with(|d| d.borrow().buttons_down)
    }
    #[unsafe(no_mangle)]
    extern "C" fn platform_buttons_held() -> u64 {
        DEVICES.with(|d| d.borrow().held)
    }
    #[unsafe(no_mangle)]
    extern "C" fn platform_focused() -> bool {
        DEVICES.with(|d| !d.borrow().unfocused)
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn platform_touches(output: *mut VeraTouchState) -> u32 {
        DEVICES.with(|d| {
            let devices = d.borrow();
            let output = unsafe { &mut *output };
            output.count = devices.touches.len() as u32;
            for (target, &(id, x, y)) in output.touches.iter_mut().zip(&devices.touches) {
                *target = VeraTouch { id, x, y };
            }
            output.count
        })
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn platform_mouse(
        dx: *mut i32,
        dy: *mut i32,
        wx: *mut i32,
        wy: *mut i32,
        buttons: *mut u32,
        pressed: *mut u32,
        released: *mut u32,
    ) -> bool {
        DEVICES.with(|d| {
            let mut devices = d.borrow_mut();
            unsafe {
                *dx = 0;
                *dy = 0;
                *wx = 0;
                *wy = 0;
                *buttons = devices.mouse;
                *pressed = devices.pressed;
                *released = devices.released;
            }
            devices.pressed = 0;
            devices.released = 0;
            true
        })
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn platform_keyboard(keys: *mut u64) -> u64 {
        unsafe {
            std::slice::from_raw_parts_mut(keys, 4).fill(0);
        }
        0
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn platform_soft_keyboard_show(
        output: *mut c_char,
        output_size: usize,
    ) -> u32 {
        DEVICES.with(|d| {
            let mut devices = d.borrow_mut();
            devices.keyboard_calls += 1;
            let Some(text) = devices.keyboard_text else {
                return 1;
            };
            let bytes = text.as_bytes();
            if bytes.len() + 1 > output_size {
                return 1;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast::<u8>(), bytes.len());
                *output.add(bytes.len()) = 0;
            }
            0
        })
    }
    #[unsafe(no_mangle)]
    unsafe extern "C" fn platform_sticks(lx: *mut i32, ly: *mut i32, rx: *mut i32, ry: *mut i32) {
        unsafe {
            *lx = 0;
            *ly = 0;
            *rx = 0;
            *ry = 0;
        }
    }

    fn buttons(input: &mut Input, shared: &Shared) -> Vec<(MouseButton, ElementState)> {
        input
            .poll(shared)
            .into_iter()
            .filter_map(|event| match event {
                WindowEvent::MouseInput { button, state, .. } => Some((button, state)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn physical_plus_sends_escape_edges_without_repeating_while_held() {
        DEVICES.with(|d| *d.borrow_mut() = Devices::default());
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        for (held, expected) in [
            (1 << 10, Some(ElementState::Pressed)),
            (1 << 10, None),
            (0, Some(ElementState::Released)),
            (0, None),
        ] {
            DEVICES.with(|d| d.borrow_mut().held = held);
            let keys: Vec<_> = input
                .poll(shared)
                .into_iter()
                .filter_map(|event| match event {
                    WindowEvent::KeyboardInput { event, .. } => Some(event),
                    _ => None,
                })
                .collect();
            assert_eq!(keys.len(), usize::from(expected.is_some()));
            if let Some(state) = expected {
                assert_eq!(keys[0].physical_key, PhysicalKey::Code(KeyCode::Escape));
                assert_eq!(keys[0].logical_key, Key::Named(NamedKey::Escape));
                assert_eq!(keys[0].state, state);
                assert!(!keys[0].repeat);
            }
        }
    }

    #[test]
    fn focused_ime_uses_plus_for_software_keyboard_without_sending_escape() {
        DEVICES.with(|d| {
            *d.borrow_mut() = Devices {
                buttons_down: 1 << 10,
                held: 1 << 10,
                keyboard_text: Some("测试"),
                ..Devices::default()
            }
        });
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        shared.ime_allowed.store(true, Ordering::Release);
        let mut input = Input::new();
        let events = input.poll(shared);
        assert!(!events.iter().any(|event| matches!(
            event,
            WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            WindowEvent::Ime(Ime::Commit(text)) if text == "测试"
        )));
        // A held Plus must not leak an Escape edge on subsequent polls.
        DEVICES.with(|d| d.borrow_mut().buttons_down = 0);
        let events = input.poll(shared);
        assert!(!events.iter().any(|event| matches!(
            event,
            WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
        )));
        DEVICES.with(|d| assert_eq!(d.borrow().keyboard_calls, 1));
    }

    #[test]
    fn dpad_sends_team_guard_and_planning_keys_with_matching_releases() {
        DEVICES.with(|d| *d.borrow_mut() = Devices::default());
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        for (mask, key) in [
            (1 << 13, KeyCode::Digit1),
            (1 << 15, KeyCode::Digit2),
            (1 << 12, KeyCode::KeyG),
            (1 << 14, KeyCode::KeyZ),
        ] {
            for (held, expected) in [
                (mask, Some(ElementState::Pressed)),
                (mask, None),
                (0, Some(ElementState::Released)),
            ] {
                DEVICES.with(|d| d.borrow_mut().held = held);
                let keys: Vec<_> = input
                    .poll(shared)
                    .into_iter()
                    .filter_map(|event| match event {
                        WindowEvent::KeyboardInput { event, .. } => Some(event),
                        _ => None,
                    })
                    .collect();
                assert_eq!(keys.len(), usize::from(expected.is_some()));
                if let Some(state) = expected {
                    assert_eq!(keys[0].physical_key, PhysicalKey::Code(key));
                    assert_eq!(keys[0].state, state);
                    assert!(!keys[0].repeat);
                }
            }
        }
    }

    #[test]
    fn r_and_dpad_deliver_ctrl_before_the_team_number() {
        DEVICES.with(|d| *d.borrow_mut() = Devices::default());
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        for (mask, key) in [(1 << 13, KeyCode::Digit1), (1 << 15, KeyCode::Digit2)] {
            DEVICES.with(|d| d.borrow_mut().held = (1 << 7) | mask);
            let events = input.poll(shared);
            assert!(
                matches!(events.first(), Some(WindowEvent::ModifiersChanged(m)) if m.state().control_key())
            );
            let keys: Vec<_> = events
                .into_iter()
                .filter_map(|event| match event {
                    WindowEvent::KeyboardInput { event, .. } => Some(event.physical_key),
                    _ => None,
                })
                .collect();
            assert_eq!(
                keys,
                [
                    PhysicalKey::Code(KeyCode::ControlLeft),
                    PhysicalKey::Code(key)
                ]
            );
            DEVICES.with(|d| *d.borrow_mut() = Devices::default());
            input.poll(shared);
            assert!(!input.modifiers.control_key());
        }
    }

    #[test]
    fn poll_delivers_controller_and_single_touch_clicks() {
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        for (held, touches, button) in [
            (1, vec![], MouseButton::Left),
            (2, vec![], MouseButton::Right),
            (0, vec![(1, 1013, 453)], MouseButton::Left),
        ] {
            DEVICES.with(|d| {
                *d.borrow_mut() = Devices {
                    held,
                    touches,
                    ..Devices::default()
                }
            });
            assert_eq!(
                buttons(&mut input, shared),
                [(button, ElementState::Pressed)]
            );
            assert!(buttons(&mut input, shared).is_empty());
            DEVICES.with(|d| *d.borrow_mut() = Devices::default());
            assert_eq!(
                buttons(&mut input, shared),
                [(button, ElementState::Released)]
            );
        }
    }

    #[test]
    fn poll_merges_usb_with_controller_and_releases_on_focus_loss() {
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        DEVICES.with(|d| {
            *d.borrow_mut() = Devices {
                held: 1,
                ..Devices::default()
            }
        });
        assert_eq!(
            buttons(&mut input, shared),
            [(MouseButton::Left, ElementState::Pressed)]
        );
        DEVICES.with(|d| {
            let mut d = d.borrow_mut();
            d.mouse = 1;
            d.pressed = 1;
        });
        assert!(buttons(&mut input, shared).is_empty());
        DEVICES.with(|d| d.borrow_mut().held = 0);
        assert!(buttons(&mut input, shared).is_empty());
        DEVICES.with(|d| d.borrow_mut().unfocused = true);
        assert_eq!(
            buttons(&mut input, shared),
            [(MouseButton::Left, ElementState::Released)]
        );
        assert!(buttons(&mut input, shared).is_empty());
        DEVICES.with(|d| {
            *d.borrow_mut() = Devices {
                pressed: 1,
                released: 1,
                ..Devices::default()
            }
        });
        assert_eq!(
            buttons(&mut input, shared),
            [
                (MouseButton::Left, ElementState::Pressed),
                (MouseButton::Left, ElementState::Released),
            ]
        );
    }

    #[test]
    fn poll_two_finger_pan_suppresses_click_until_every_finger_lifts() {
        let event_loop = EventLoop::<()>::new(&mut PlatformSpecificEventLoopAttributes).unwrap();
        let shared = &event_loop.active.p.shared;
        let mut input = Input::new();
        DEVICES.with(|d| {
            *d.borrow_mut() = Devices {
                touches: vec![(1, 100, 100)],
                ..Devices::default()
            }
        });
        assert_eq!(
            buttons(&mut input, shared),
            [(MouseButton::Left, ElementState::Pressed)]
        );
        DEVICES.with(|d| d.borrow_mut().touches.push((2, 200, 100)));
        assert_eq!(
            buttons(&mut input, shared),
            [(MouseButton::Left, ElementState::Released)]
        );
        DEVICES.with(|d| {
            d.borrow_mut().touches.pop();
        });
        assert!(buttons(&mut input, shared).is_empty());
        DEVICES.with(|d| d.borrow_mut().touches.clear());
        assert!(buttons(&mut input, shared).is_empty());
        DEVICES.with(|d| d.borrow_mut().touches.push((1, 100, 100)));
        assert_eq!(
            buttons(&mut input, shared),
            [(MouseButton::Left, ElementState::Pressed)]
        );
    }

    #[test]
    fn keyboard_modifiers_precede_same_sample_hotkeys_and_text() {
        let mut keys = [0_u64; 4];
        keys[225 / 64] |= 1 << (225 % 64);
        keys[4 / 64] |= 1 << (4 % 64);
        keys[30 / 64] |= 1 << (30 % 64);
        let events = keyboard_edges(&[0; 4], &keys, false);
        assert_eq!(
            events[0].physical_key,
            PhysicalKey::Code(KeyCode::ShiftLeft)
        );
        assert_eq!(events[1].text.as_deref(), Some("A"));
        assert_eq!(
            events[1].platform_specific.key_without_modifiers,
            Key::Character("a".into())
        );
        assert_eq!(events[2].text.as_deref(), Some("!"));
        let caps = keyboard_edges(&[0; 4], &keys, true);
        assert_eq!(caps[1].text.as_deref(), Some("a"));
        assert!(keyboard_edges(&keys, &keys, false).is_empty());
    }

    #[test]
    fn keyboard_shortcuts_do_not_type_into_text_fields() {
        let mut keys = [0_u64; 4];
        keys[224 / 64] |= 1 << (224 % 64);
        keys[4 / 64] |= 1 << (4 % 64);
        let events = keyboard_edges(&[0; 4], &keys, false);
        assert_eq!(
            events[0].physical_key,
            PhysicalKey::Code(KeyCode::ControlLeft)
        );
        assert!(events[1].text.is_none());
        let released = keyboard_edges(&keys, &[0; 4], false);
        assert!(released
            .iter()
            .all(|event| event.state == ElementState::Released && event.text.is_none()));
    }

    #[test]
    fn keyboard_mapping_covers_letters_digits_punctuation_and_numpad() {
        assert_eq!(
            keyboard_mapping(4),
            Some((
                KeyCode::KeyA,
                Key::Character("a".into()),
                KeyLocation::Standard
            ))
        );
        assert_eq!(
            keyboard_mapping(30),
            Some((
                KeyCode::Digit1,
                Key::Character("1".into()),
                KeyLocation::Standard
            ))
        );
        assert_eq!(
            keyboard_mapping(45),
            Some((
                KeyCode::Minus,
                Key::Character("-".into()),
                KeyLocation::Standard
            ))
        );
        assert_eq!(
            keyboard_mapping(87),
            Some((
                KeyCode::NumpadAdd,
                Key::Character("+".into()),
                KeyLocation::Numpad
            ))
        );
        assert_eq!(keyboard_mapping(101), None);
    }

    #[test]
    fn touch_midpoint_uses_the_first_two_fingers() {
        let touches = [
            (1, PhysicalPosition::new(100.0, 200.0)),
            (2, PhysicalPosition::new(300.0, 400.0)),
            (3, PhysicalPosition::new(1000.0, 600.0)),
        ];
        assert_eq!(
            touch_midpoint(&touches),
            Some(PhysicalPosition::new(200.0, 300.0))
        );
        assert_eq!(touch_midpoint(&touches[..1]), None);
    }

    #[test]
    fn two_finger_latch_survives_one_finger_lift_until_all_release() {
        let mut gesture = TouchPanGesture::default();
        let both = [
            (1, PhysicalPosition::new(100.0, 100.0)),
            (2, PhysicalPosition::new(200.0, 100.0)),
        ];
        let first = gesture.observe(&both);
        assert!(first.active && first.delta == (0.0, 0.0));
        let moved = gesture.observe(&[
            (1, PhysicalPosition::new(120.0, 100.0)),
            (2, PhysicalPosition::new(220.0, 100.0)),
        ]);
        assert!(moved.active);
        assert_eq!(moved.delta, (20.0, 0.0));
        let one = gesture.observe(&[(1, PhysicalPosition::new(120.0, 100.0))]);
        assert!(one.active && one.delta == (0.0, 0.0));
        let released = gesture.observe(&[]);
        assert!(!released.active && released.delta == (0.0, 0.0));
    }
}
