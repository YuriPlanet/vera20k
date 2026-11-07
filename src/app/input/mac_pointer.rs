//! macOS pointer control winit 0.30 does not offer: an app-wide arrow hide
//! and the pointer's place relative to the window when no event carries it.
//! Called from the main thread's event handling only. Presentation-only.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSCursor, NSView};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Whether this module holds one `NSCursor` hide. AppKit counts hides, so
/// each hide here is paired with exactly one unhide.
static ARROW_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Hide or show the system arrow for the whole app.
pub(crate) fn set_arrow_hidden(hidden: bool) {
    if MainThreadMarker::new().is_none() || ARROW_HIDDEN.swap(hidden, Ordering::Relaxed) == hidden {
        return;
    }
    if hidden {
        NSCursor::hide();
    } else {
        NSCursor::unhide();
    }
}

/// The pointer in the window's client area, in its top-left logical points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewPointer {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl ViewPointer {
    pub(crate) fn inside(self) -> bool {
        (0.0..self.width).contains(&self.x) && (0.0..self.height).contains(&self.y)
    }

    /// The closest point of the client area, one point in from its far edges.
    pub(crate) fn nearest_inside(self) -> (f64, f64) {
        (
            self.x.clamp(0.0, (self.width - 1.0).max(0.0)),
            self.y.clamp(0.0, (self.height - 1.0).max(0.0)),
        )
    }
}

/// Where the pointer is now relative to the window's client area, wherever
/// it is on the desktop.
pub(crate) fn pointer_in_view(window: &Window) -> Option<ViewPointer> {
    MainThreadMarker::new()?;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out its live content view, and this runs on the
    // main thread that owns it.
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    let ns_window = view.window()?;
    // winit's view is flipped, so view points run from its top-left corner.
    let point = view.convertPoint_fromView(ns_window.mouseLocationOutsideOfEventStream(), None);
    let size = view.frame().size;
    Some(ViewPointer {
        x: point.x,
        y: point.y,
        width: size.width,
        height: size.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_past_an_edge_comes_back_on_that_edge() {
        let at = |x, y| ViewPointer {
            x,
            y,
            width: 800.0,
            height: 600.0,
        };
        assert!(at(0.0, 0.0).inside());
        assert!(!at(800.0, 10.0).inside());
        assert_eq!(at(-40.0, 300.0).nearest_inside(), (0.0, 300.0));
        assert_eq!(at(900.0, -5.0).nearest_inside(), (799.0, 0.0));
        assert_eq!(at(400.0, 650.0).nearest_inside(), (400.0, 599.0));
    }
}
