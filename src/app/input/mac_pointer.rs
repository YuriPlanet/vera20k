//! macOS pointer control winit 0.30 does not offer: an app-wide arrow hide
//! and the pointer's place relative to the window when no event carries it.
//! Called from the main thread's event handling only. Presentation-only.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{NSCursor, NSImage, NSView};
use objc2_foundation::{NSData, NSPoint, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::render::cursor_atlas::OsCursorImage;

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

/// Put the system arrow up now, not when the pointer next moves.
pub(crate) fn set_system_arrow() {
    if MainThreadMarker::new().is_some() {
        NSCursor::arrowCursor().set();
    }
}

/// A game cursor image, pixel for pixel on a screen of its backing scale.
/// winit's custom cursors take one image point per pixel, which a 2x screen
/// would double, and cannot be made current without the pointer moving.
pub(crate) struct MacCursor(Retained<NSCursor>);

impl MacCursor {
    /// `backing` is the screen's physical pixels per point; an `NSImage`'s
    /// size and a cursor's hot spot are both in points.
    pub(crate) fn new(image: &OsCursorImage, backing: f64) -> Option<Self> {
        MainThreadMarker::new()?;
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(
            u32::from(image.width),
            u32::from(image.height),
            image.rgba.clone(),
        )?
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
        let data = NSData::with_bytes(png.get_ref());
        let ns_image = NSImage::initWithData(NSImage::alloc(), &data)?;
        let points = |pixels: u16| f64::from(pixels) / backing;
        ns_image.setSize(NSSize::new(points(image.width), points(image.height)));
        let hotspot = NSPoint::new(points(image.hotspot.0), points(image.hotspot.1));
        Some(Self(NSCursor::initWithImage_hotSpot(
            NSCursor::alloc(),
            &ns_image,
            hotspot,
        )))
    }

    /// Make this the app's cursor.
    pub(crate) fn set(&self) {
        self.0.set();
    }
}

/// Let AppKit apply the window's cursor rectangles, or stop it. winit's
/// rectangle is blank while the window hides the arrow, and AppKit applies it
/// whenever the pointer enters the window, so it would blank the game's image
/// until the next draw put it back: for as long as a slow frame takes.
pub(crate) fn set_cursor_rects_enabled(window: &Window, enabled: bool) {
    let Some(ns_window) = content_view(window).and_then(|view| view.window()) else {
        return;
    };
    if enabled {
        ns_window.enableCursorRects();
    } else {
        ns_window.disableCursorRects();
    }
}

/// winit's content view of `window`, on the main thread only.
fn content_view(window: &Window) -> Option<&NSView> {
    MainThreadMarker::new()?;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: winit hands out its live content view, which lasts as long as
    // `window`, and this runs on the main thread that owns it.
    Some(unsafe { handle.ns_view.cast::<NSView>().as_ref() })
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
    let view = content_view(window)?;
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
