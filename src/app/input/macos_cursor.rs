//! macOS confinement: winit's Confined is unsupported, while Locked preserves
//! relative NSEvent motion. Integrate that motion through the existing window
//! pointer owner and warp only the native image, without re-associating it.

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
    use std::cell::Cell;

    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSApplication;
    use winit::dpi::PhysicalPosition;
    use winit::window::CursorGrabMode;

    use crate::app::AppState;

    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWarpMouseCursorPosition(point: Point) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    }

    #[derive(Default)]
    pub(crate) struct MacCursorConfinement {
        locked: Cell<bool>,
        /// True only while device_event reuses the normal CursorMoved route.
        pub(crate) from_device: Cell<bool>,
        last_warp: Cell<Option<(f64, f64)>>,
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
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: these public NSApplication/NSEvent accessors are read on
        // AppKit's main thread while winit is dispatching this same NSEvent.
        // No NSEvent is retained or used beyond its native callback.
        unsafe {
            let event: *mut AnyObject = msg_send![&*app, currentEvent];
            if event.is_null() {
                return false;
            }
            let event_type: usize = msg_send![event, type];
            // NSEventType: MouseMoved, Left/Right/OtherMouseDragged.
            if !matches!(event_type, 5 | 6 | 7 | 27) {
                return false;
            }
            let dx: f64 = msg_send![event, deltaX];
            let dy: f64 = msg_send![event, deltaY];
            dx != 0.0 || dy != 0.0
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
        if backend.last_warp.get() == Some(global) {
            return;
        }
        // SAFETY: a finite point in the focused window's client rectangle.
        if unsafe { CGWarpMouseCursorPosition(point) } == 0 {
            backend.last_warp.set(Some(global));
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
            backend.last_warp.set(None);
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
        backend.last_warp.set(None);
    }

    pub(crate) fn raw_motion(state: &AppState, delta: (f64, f64)) -> Option<PhysicalPosition<f64>> {
        let backend = &state.platform.macos_cursor;
        if !backend.locked.get() || !eligible(state) {
            return None;
        }
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
}
