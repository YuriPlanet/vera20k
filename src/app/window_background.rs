//! The window's own background, which shows until the game's first frame.
//!
//! The first swapchain present comes seconds into startup, once the renderers
//! and archives are built. Until then a window shows what its system draws for
//! it: on Windows, black and white patches of an unpainted surface, because
//! winit registers its window class with no background brush; on X11 an
//! unpainted surface too, as winit gives the window no background; on macOS the
//! system's grey window background. Startup paints the player's window black
//! ([`paint_black`]) and, once it has presented its first frame, gives Windows
//! and X11 back a window with no background ([`release`]). Wayland shows no
//! window before its first frame, and the browser draws into its own canvas.

use winit::window::Window;

/// Whether the player's window opens hidden and is shown once it is black.
/// Windows and macOS would show it inside winit's window creation, before it
/// can be painted. X11 cannot open it hidden, because winit sets full screen
/// there only on a window that is showing, so it paints the window a moment
/// after it appears.
pub(super) const OPEN_HIDDEN: bool = cfg!(any(windows, target_os = "macos"));

/// Paints the window black. Windows and X11 paint it now and again whenever
/// they show, resize or uncover it; macOS draws black under the game's picture.
pub(super) fn paint_black(window: &Window) {
    #[cfg(any(windows, target_os = "linux"))]
    set_black_background(window, true);
    #[cfg(target_os = "macos")]
    paint_macos_black(window);
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let _ = window;
}

/// Takes the black background away again on Windows and X11 once startup has
/// presented its first frame. From then on the swapchain owns the picture, and
/// both systems would paint the window black between frames on every resize.
/// macOS draws its background only under the picture, so it stays black.
pub(super) fn release(window: &Window) {
    #[cfg(any(windows, target_os = "linux"))]
    set_black_background(window, false);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = window;
}

/// Windows paints a window with its class brush when it shows or resizes it.
#[cfg(windows)]
fn set_black_background(window: &Window, black: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GCLP_HBRBACKGROUND, SetClassLongPtrW};

    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as *mut std::ffi::c_void;
    // SAFETY: `hwnd` is winit's live window, and a stock brush is never freed.
    unsafe {
        let brush = if black {
            GetStockObject(BLACK_BRUSH) as isize
        } else {
            0
        };
        SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brush);
    }
}

/// The X server paints a window's background pixel when the window is cleared
/// or uncovered. A Wayland window has nothing to paint: it shows from its
/// first frame.
#[cfg(target_os = "linux")]
fn set_black_background(window: &Window, black: bool) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

    let (Ok(window_handle), Ok(display_handle)) = (window.window_handle(), window.display_handle())
    else {
        return;
    };
    let (RawWindowHandle::Xlib(xwindow), RawDisplayHandle::Xlib(xdisplay)) =
        (window_handle.as_raw(), display_handle.as_raw())
    else {
        return;
    };
    let Some(display) = xdisplay.display else {
        return;
    };
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
        return;
    };
    let display = display.as_ptr().cast::<x11_dl::xlib::Display>();
    // SAFETY: winit's live connection and window, on the thread winit uses
    // them from.
    unsafe {
        if black {
            let pixel = (xlib.XBlackPixel)(display, xdisplay.screen);
            (xlib.XSetWindowBackground)(display, xwindow.window, pixel);
            (xlib.XClearWindow)(display, xwindow.window);
        } else {
            // `None`, the background winit creates the window with.
            (xlib.XSetWindowBackgroundPixmap)(display, xwindow.window, 0);
        }
        (xlib.XFlush)(display);
    }
}

#[cfg(target_os = "macos")]
fn paint_macos_black(window: &Window) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSColor, NSView};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    if MainThreadMarker::new().is_none() {
        return;
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit's live content view, used on the main thread (checked
    // above), where AppKit wants it.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    if let Some(ns_window) = view.window() {
        ns_window.setBackgroundColor(Some(&NSColor::blackColor()));
    }
}
