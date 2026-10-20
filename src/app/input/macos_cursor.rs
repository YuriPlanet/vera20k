//! macOS confinement: winit's Confined is unsupported, while Locked preserves
//! relative NSEvent motion. Integrate that motion through the existing window
//! pointer owner and warp only the native image, without re-associating it.

use std::collections::VecDeque;

/// Quartz warps do not emit a separate mouse event, but their displacement is
/// included in subsequent NSEvent deltas. SDL's Cocoa_HandleMouseWarp and
/// GLFW's cursorWarpDeltaX/Y compensate the same contribution. Keep it in
/// global display points, independent of the window's backing scale.
/// Sources: SDL2 src/video/cocoa/SDL_cocoamouse.m; GLFW src/cocoa_window.m.
#[derive(Default)]
struct WarpCompensation {
    pending: VecDeque<(f64, (f64, f64))>,
}

impl WarpCompensation {
    fn record(&mut self, before: (f64, f64), after: (f64, f64), at: f64) {
        let delta = (after.0 - before.0, after.1 - before.1);
        if !at.is_finite() || !delta.0.is_finite() || !delta.1.is_finite() || delta == (0.0, 0.0) {
            return;
        }
        self.pending.push_back((at, delta));
    }

    fn compensate(&mut self, delta: (f64, f64), sampled_at: Option<f64>) -> (f64, f64) {
        let Some(sampled_at) = sampled_at.filter(|time| time.is_finite() && *time > 0.0) else {
            return delta;
        };
        let mut warp = (0.0, 0.0);
        // An event may have been queued before a later programmatic warp.
        // It cannot contain that warp, so leave that contribution pending.
        while self
            .pending
            .front()
            .is_some_and(|(at, _)| *at <= sampled_at)
        {
            let (_, contribution) = self.pending.pop_front().expect("front exists");
            warp.0 += contribution.0;
            warp.1 += contribution.1;
        }
        (delta.0 - warp.0, delta.1 - warp.1)
    }

    fn clear(&mut self) {
        self.pending.clear();
    }
}

fn may_confine(
    requested: bool,
    capture: bool,
    active: bool,
    hidden: bool,
    focused: bool,
    size: (u32, u32),
) -> bool {
    requested && !capture && active && !hidden && focused && size.0 > 0 && size.1 > 0
}

fn clamped_position(position: (f64, f64), size: (u32, u32)) -> Option<(f64, f64)> {
    if size.0 == 0 || size.1 == 0 || !position.0.is_finite() || !position.1.is_finite() {
        return None;
    }
    Some((
        position.0.clamp(0.0, f64::from(size.0 - 1)),
        position.1.clamp(0.0, f64::from(size.1 - 1)),
    ))
}

fn moved_position(
    position: (f64, f64),
    logical_delta: (f64, f64),
    scale: f64,
    size: (u32, u32),
) -> Option<(f64, f64)> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    clamped_position(
        (
            position.0 + logical_delta.0 * scale,
            position.1 + logical_delta.1 * scale,
        ),
        size,
    )
}

fn same_pixel(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
}

fn admitted_absolute(
    point: (f64, f64),
    current: (f64, f64),
    already_integrated: bool,
    size: (u32, u32),
) -> Option<(f64, f64)> {
    if already_integrated || same_pixel(current, point) {
        None
    } else {
        clamped_position(point, size)
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;

    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, class, msg_send};
    use objc2_app_kit::NSApplication;
    use winit::dpi::PhysicalPosition;
    use winit::window::CursorGrabMode;

    use crate::app::AppState;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWarpMouseCursorPosition(point: Point) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
        fn CGEventCreate(source: *const c_void) -> *const c_void;
        fn CGEventGetLocation(event: *const c_void) -> Point;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(object: *const c_void);
    }

    #[derive(Default)]
    pub(crate) struct MacCursorConfinement {
        locked: Cell<bool>,
        /// True only while device_event reuses the normal CursorMoved route.
        pub(crate) from_device: Cell<bool>,
        warp_compensation: RefCell<super::WarpCompensation>,
    }

    impl Drop for MacCursorConfinement {
        fn drop(&mut self) {
            if self.locked.get() {
                // SAFETY: process-local mouse association, restored even when
                // the application exits without a final Focused(false) edge.
                unsafe { CGAssociateMouseAndMouseCursorPosition(1) };
            }
        }
    }

    fn eligible(state: &AppState) -> bool {
        let size = state.platform.window.inner_size();
        super::may_confine(
            state.platform.cursor_confined,
            state.platform.capture_client_size.is_some(),
            state.platform.window_active,
            state.platform.window_hidden,
            state.platform.window.has_focus(),
            (size.width, size.height),
        )
    }

    fn current_event_has_relative_motion() -> bool {
        current_mouse_event().is_some_and(|event| {
            // SAFETY: the pointer is the live main-thread mouse NSEvent.
            unsafe {
                let dx: f64 = msg_send![event, deltaX];
                let dy: f64 = msg_send![event, deltaY];
                dx != 0.0 || dy != 0.0
            }
        })
    }

    fn current_event_timestamp() -> Option<f64> {
        current_mouse_event().map(|event| {
            // SAFETY: the pointer is the live main-thread mouse NSEvent.
            unsafe { msg_send![event, timestamp] }
        })
    }

    fn current_mouse_event() -> Option<*mut AnyObject> {
        let Some(mtm) = MainThreadMarker::new() else {
            return None;
        };
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: these public NSApplication/NSEvent accessors are read on
        // AppKit's main thread while winit is dispatching this same NSEvent.
        // No NSEvent is retained or used beyond its native callback.
        unsafe {
            let event: *mut AnyObject = msg_send![&*app, currentEvent];
            if event.is_null() {
                return None;
            }
            let event_type: usize = msg_send![event, type];
            // NSEventType: MouseMoved, Left/Right/OtherMouseDragged.
            if !matches!(event_type, 5 | 6 | 7 | 27) {
                return None;
            }
            Some(event)
        }
    }

    fn system_uptime() -> f64 {
        // NSEvent.timestamp and NSProcessInfo.systemUptime both use seconds
        // since system startup. Neither wall-clock adjustments nor display
        // scale changes can reorder the compensation against queued input.
        // SAFETY: processInfo is the documented non-null shared singleton;
        // reading systemUptime is safe on this main-thread callback.
        unsafe {
            let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
            msg_send![info, systemUptime]
        }
    }

    fn global_pointer_position() -> Option<(f64, f64)> {
        // SAFETY: CGEventCreate(NULL) snapshots current event-source state;
        // nothing is posted. Release its sole Create ownership after reading.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event);
            (point.x.is_finite() && point.y.is_finite()).then_some((point.x, point.y))
        }
    }

    fn warp(state: &AppState, position: (f64, f64)) {
        let backend = &state.platform.macos_cursor;
        let Ok(origin) = state.platform.window.inner_position() else {
            return;
        };
        let scale = state.platform.window.scale_factor();
        // Match winit 0.30.12's macOS physical-window -> global-point transform.
        // Its set_cursor_position also calls CGAssociate(..., true), which
        // would cancel Locked on every move; call only the warp operation.
        let point = Point {
            x: (f64::from(origin.x) + position.0) / scale,
            y: (f64::from(origin.y) + position.1) / scale,
        };
        let global = (point.x, point.y);
        let Some(before) = global_pointer_position() else {
            return;
        };
        if before == global {
            return;
        }
        // SAFETY: a finite point in the focused window's client rectangle.
        if unsafe { CGWarpMouseCursorPosition(point) } == 0 {
            if let Some(after) = global_pointer_position() {
                backend
                    .warp_compensation
                    .borrow_mut()
                    .record(before, after, system_uptime());
            }
        }
    }

    pub(crate) fn apply(state: &AppState, verbose: bool) {
        if !eligible(state) {
            release(state);
            return;
        }
        let backend = &state.platform.macos_cursor;
        if !backend.locked.get() {
            if let Err(error) = state
                .platform
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
            {
                if verbose {
                    log::warn!("macOS cursor confinement unavailable: {error}");
                }
                return;
            }
            backend.locked.set(true);
            backend.warp_compensation.borrow_mut().clear();
        }
        let (x, y) = state.window_cursor_position();
        let size = state.platform.window.inner_size();
        if let Some(position) =
            super::clamped_position((f64::from(x), f64::from(y)), (size.width, size.height))
        {
            warp(state, position);
        }
    }

    pub(crate) fn release(state: &AppState) {
        let backend = &state.platform.macos_cursor;
        if backend.locked.get()
            && state
                .platform
                .window
                .set_cursor_grab(CursorGrabMode::None)
                .is_ok()
        {
            backend.locked.set(false);
        }
        backend.from_device.set(false);
        backend.warp_compensation.borrow_mut().clear();
    }

    pub(crate) fn raw_motion(state: &AppState, delta: (f64, f64)) -> Option<PhysicalPosition<f64>> {
        let backend = &state.platform.macos_cursor;
        if !backend.locked.get() || !eligible(state) {
            return None;
        }
        if !delta.0.is_finite() || !delta.1.is_finite() {
            return None;
        }
        // Consume only earlier, successful warps before this raw event's own
        // output warp is recorded. The backing-scale conversion happens once,
        // after removing displacement measured in the same point units.
        let delta = backend
            .warp_compensation
            .borrow_mut()
            .compensate(delta, current_event_timestamp());
        let (x, y) = state.window_cursor_position();
        let origin = (f64::from(x), f64::from(y));
        let size = state.platform.window.inner_size();
        let position = super::moved_position(
            origin,
            delta,
            state.platform.window.scale_factor(),
            (size.width, size.height),
        )?;
        warp(state, position);
        Some(PhysicalPosition::new(position.0, position.1))
    }

    pub(crate) fn absolute_motion(
        state: &AppState,
        position: PhysicalPosition<f64>,
    ) -> Option<PhysicalPosition<f64>> {
        let backend = &state.platform.macos_cursor;
        if !backend.locked.get() || !eligible(state) || backend.from_device.get() {
            return Some(position);
        }
        let point = (position.x, position.y);
        let (x, y) = state.window_cursor_position();
        // Absolute synthesized events (e.g. accessibility automation) can have
        // delta zero and no DeviceEvent. Admit these rather than freezing them.
        let size = state.platform.window.inner_size();
        let point = super::admitted_absolute(
            point,
            (f64::from(x), f64::from(y)),
            // winit macOS app.rs::send_event delivers MouseMotion before
            // sending the same NSEvent to the view's CursorMoved callback.
            // Locked can keep its absolute anchor constant across many moves;
            // do not infer duplication from the anchor's coordinates.
            current_event_has_relative_motion(),
            (size.width, size.height),
        )?;
        warp(state, point);
        Some(PhysicalPosition::new(point.0, point.1))
    }
}

#[cfg(target_os = "macos")]
pub(crate) use native::{MacCursorConfinement, absolute_motion, apply, raw_motion, release};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_confinement_requires_a_visible_focused_interactive_window() {
        assert!(may_confine(true, false, true, false, true, (800, 600)));
        for (requested, capture, active, hidden, focused, size) in [
            (false, false, true, false, true, (800, 600)),
            (true, true, true, false, true, (800, 600)),
            (true, false, false, false, true, (800, 600)),
            (true, false, true, true, true, (800, 600)),
            (true, false, true, false, false, (800, 600)),
            (true, false, true, false, true, (0, 600)),
            (true, false, true, false, true, (800, 0)),
        ] {
            assert!(!may_confine(
                requested, capture, active, hidden, focused, size
            ));
        }
    }

    #[test]
    fn macos_relative_motion_reaches_every_edge_at_retina_scale() {
        assert_eq!(
            moved_position((400.0, 300.0), (20.0, -10.0), 2.0, (800, 600)),
            Some((440.0, 280.0))
        );
        assert_eq!(
            moved_position((440.0, 280.0), (-1000.0, -1000.0), 2.0, (800, 600)),
            Some((0.0, 0.0))
        );
        assert_eq!(
            moved_position((0.0, 0.0), (1000.0, 1000.0), 1.0, (800, 600)),
            Some((799.0, 599.0))
        );
        assert_eq!(
            moved_position((799.0, 599.0), (-1.0, -1.0), 2.0, (800, 600)),
            Some((797.0, 597.0))
        );
        assert_eq!(
            clamped_position((799.0, 599.0), (320, 200)),
            Some((319.0, 199.0))
        );
        assert_eq!(
            moved_position((0.0, 0.0), (f64::NAN, 1.0), 2.0, (800, 600)),
            None
        );
        assert_eq!(
            moved_position((0.0, 0.0), (1.0, 1.0), 0.0, (800, 600)),
            None
        );
    }

    #[test]
    fn macos_native_duplicate_filter_keeps_distinct_absolute_input() {
        let old = (100.0, 80.0);
        let current = moved_position(old, (10.0, 5.0), 2.0, (800, 600)).unwrap();
        assert_eq!(current, (120.0, 90.0));
        // The raw NSEvent has already moved the shared pointer. Neither its
        // frozen native anchor nor a warp echo may move it back/double it.
        assert_eq!(admitted_absolute(old, current, true, (800, 600)), None);
        let next = moved_position(current, (10.0, 5.0), 2.0, (800, 600)).unwrap();
        assert_eq!(admitted_absolute(old, next, true, (800, 600)), None);
        assert_eq!(admitted_absolute(current, current, false, (800, 600)), None);
        // A separately synthesized absolute event has no raw delta to consume.
        assert_eq!(
            admitted_absolute((640.0, 400.0), current, false, (800, 600)),
            Some((640.0, 400.0))
        );
        assert_eq!(
            admitted_absolute((1200.0, -30.0), current, false, (800, 600)),
            Some((799.0, 0.0))
        );
    }

    #[test]
    fn macos_warp_feedback_keeps_repeated_hardware_motion_at_constant_speed() {
        for scale in [1.0, 2.0] {
            let mut compensation = WarpCompensation::default();
            let mut cursor = (100.0 * scale, 100.0 * scale);
            let mut native_cursor = (100.0, 100.0);
            let mut previous_warp = (0.0, 0.0);
            for step in 1..=5 {
                let sampled_at = f64::from(step) * 2.0;
                // AppKit folds the preceding programmatic warp into the next
                // event; each new hardware movement is still only (10, -2).
                let received = (10.0 + previous_warp.0, -2.0 + previous_warp.1);
                let corrected = compensation.compensate(received, Some(sampled_at));
                assert_eq!(corrected, (10.0, -2.0));
                cursor = moved_position(cursor, corrected, scale, (1600, 1200)).unwrap();
                assert_eq!(
                    cursor,
                    (
                        (100.0 + f64::from(step) * 10.0) * scale,
                        (100.0 - f64::from(step) * 2.0) * scale
                    )
                );
                let target = (cursor.0 / scale, cursor.1 / scale);
                previous_warp = (target.0 - native_cursor.0, target.1 - native_cursor.1);
                compensation.record(native_cursor, target, sampled_at + 0.5);
                native_cursor = target;
            }
        }
    }

    #[test]
    fn macos_warp_compensation_respects_queued_event_time_and_consumes_once() {
        let mut compensation = WarpCompensation::default();
        assert_eq!(compensation.compensate((3.0, -2.0), Some(9.0)), (3.0, -2.0));
        compensation.record((100.0, 100.0), (112.0, 94.0), 10.0);
        compensation.record((112.0, 94.0), (105.0, 98.0), 12.0);
        // Neither an older queued event nor one with an unknown source time
        // contains a proven warp contribution; preserve the original motion.
        assert_eq!(compensation.compensate((3.0, -2.0), Some(9.5)), (3.0, -2.0));
        assert_eq!(compensation.compensate((3.0, -2.0), None), (3.0, -2.0));
        // The first event includes only the positive warp, the next only the
        // reverse warp. Original hardware motion is unchanged on both axes.
        assert_eq!(
            compensation.compensate((15.0, -8.0), Some(11.0)),
            (3.0, -2.0)
        );
        assert_eq!(
            compensation.compensate((-4.0, 2.0), Some(13.0)),
            (3.0, -2.0)
        );
        assert_eq!(
            compensation.compensate((3.0, -2.0), Some(14.0)),
            (3.0, -2.0)
        );

        compensation.record((0.0, 0.0), (12.0, -6.0), 20.0);
        compensation.record((12.0, -6.0), (5.0, -2.0), 21.0);
        assert_eq!(
            compensation.compensate((8.0, -4.0), Some(22.0)),
            (3.0, -2.0)
        );
    }

    #[test]
    fn macos_warp_points_survive_backing_scale_change_and_clear_on_new_focus() {
        let mut compensation = WarpCompensation::default();
        // A warp performed at scale 1 remains five global points after the
        // window moves to a 2x screen. Subtract before applying the new scale.
        compensation.record((100.0, 100.0), (105.0, 96.0), 1.0);
        let corrected = compensation.compensate((8.0, -2.0), Some(2.0));
        assert_eq!(
            moved_position((200.0, 200.0), corrected, 2.0, (800, 600)),
            Some((206.0, 204.0))
        );
        compensation.record((100.0, 100.0), (105.0, 96.0), 3.0);
        let corrected = compensation.compensate((8.0, -2.0), Some(4.0));
        assert_eq!(
            moved_position((100.0, 100.0), corrected, 1.0, (800, 600)),
            Some((103.0, 102.0))
        );

        compensation.record((100.0, 100.0), (110.0, 90.0), 5.0);
        compensation.clear(); // Focus loss releases relative mode.
        compensation.clear(); // Successful re-acquisition starts a new ledger.
        assert_eq!(compensation.compensate((3.0, -2.0), Some(6.0)), (3.0, -2.0));
        compensation.record((300.0, 300.0), (300.0, 300.0), 7.0);
        assert!(
            compensation.pending.is_empty(),
            "zero-distance warps add no debt"
        );
    }
}
