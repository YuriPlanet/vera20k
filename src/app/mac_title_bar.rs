//! The macOS title bar: near black, with VERA's emblem and "20K" wordmark
//! after the window buttons.
//!
//! Not gamemd-derived; gamemd's window keeps the system title bar. The window
//! is created with a transparent title bar (`App::initialize`), so the bar
//! shows the window's background colour, which `style` sets. The system title
//! is hidden; the logo stands for its "VERA20K", and anything the title adds
//! after that (a build's extensions) follows the logo as text.

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBitmapImageRep, NSColor, NSFont, NSImage, NSImageView,
    NSTextField, NSView, NSWindowButton, NSWindowTitleVisibility,
};
use objc2_foundation::{NSData, NSPoint, NSRect, NSSize, NSString};
use winit::window::Window;

/// The bar's colour, sRGB 6,6,6.
const BAR_RGB: [f64; 3] = [6.0, 6.0, 6.0];
/// The colour of title text after the logo, a light grey with a slight
/// purple tint (sRGB).
const TITLE_RGB: [f64; 3] = [168.0, 156.0, 192.0];
/// The emblem and "20K", one pixel per point and two, so neither a plain
/// nor a Retina screen draws them scaled.
const LOGO_1X_PNG: &[u8] = include_bytes!("title_bar_logo.png");
const LOGO_2X_PNG: &[u8] = include_bytes!("title_bar_logo@2x.png");
/// The part of the window title the logo stands for.
const LOGO_TITLE: &str = "VERA20K";
/// The gap between the window buttons and the logo, and between the logo
/// and any title text after it, in points.
const BUTTONS_GAP: f64 = 14.0;
const TITLE_GAP: f64 = 4.0;

/// Colours `window`'s title bar and puts the logo after its buttons.
pub(super) fn style(window: &Window) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(ns_window) =
        super::input::mac_pointer::content_view(window).and_then(|view| view.window())
    else {
        return;
    };
    ns_window.setBackgroundColor(Some(&srgb(BAR_RGB)));

    // The window buttons' parent is the title bar, which spans the window.
    let Some(zoom) = ns_window.standardWindowButton(NSWindowButton::ZoomButton) else {
        return;
    };
    // SAFETY: the window's own zoom button, on the main thread; its parent is
    // the window's title bar, which the window keeps alive and this retains.
    let Some(bar) = (unsafe { zoom.superview() }) else {
        return;
    };
    let Some(logo_1x) =
        NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &NSData::with_bytes(LOGO_1X_PNG))
    else {
        return;
    };
    let logo_size = NSSize::new(logo_1x.pixelsWide() as f64, logo_1x.pixelsHigh() as f64);
    let logo = NSImage::initWithSize(NSImage::alloc(), logo_size);
    logo.addRepresentation(&logo_1x);
    if let Some(logo_2x) =
        NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &NSData::with_bytes(LOGO_2X_PNG))
    {
        logo_2x.setSize(logo_size);
        logo.addRepresentation(&logo_2x);
    }
    let logo_view = NSImageView::imageViewWithImage(&logo, mtm);

    let full_title = ns_window.title().to_string();
    let rest = full_title
        .strip_prefix(LOGO_TITLE)
        .map(str::trim)
        .unwrap_or(&full_title);
    let title = (!rest.is_empty()).then(|| {
        let title = NSTextField::labelWithString(&NSString::from_str(rest), mtm);
        title.setFont(Some(&NSFont::titleBarFontOfSize(0.0)));
        title.setTextColor(Some(&srgb(TITLE_RGB)));
        title.sizeToFit();
        title
    });
    let title_size = title
        .as_ref()
        .map_or(NSSize::new(0.0, 0.0), |title| title.frame().size);
    let title_x = logo_size.width + TITLE_GAP;

    let size = NSSize::new(
        if title.is_some() {
            title_x + title_size.width
        } else {
            logo_size.width
        },
        logo_size.height.max(title_size.height),
    );
    // Level with the window buttons, a gap right of the last one.
    let buttons = zoom.frame();
    let group = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(
            NSPoint::new(
                (buttons.origin.x + buttons.size.width + BUTTONS_GAP).round(),
                (buttons.origin.y + (buttons.size.height - size.height) / 2.0).round(),
            ),
            size,
        ),
    );
    // Stays beside the buttons however the window is resized.
    group.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewMaxXMargin
            | NSAutoresizingMaskOptions::ViewMinYMargin
            | NSAutoresizingMaskOptions::ViewMaxYMargin,
    );
    logo_view.setFrame(NSRect::new(
        NSPoint::new(0.0, ((size.height - logo_size.height) / 2.0).round()),
        logo_size,
    ));
    group.addSubview(&logo_view);
    if let Some(title) = &title {
        title.setFrameOrigin(NSPoint::new(
            title_x,
            ((size.height - title_size.height) / 2.0).round(),
        ));
        group.addSubview(title);
    }
    bar.addSubview(&group);
    ns_window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
}

/// An opaque colour from 0-255 sRGB channels.
fn srgb(rgb: [f64; 3]) -> Retained<NSColor> {
    let [red, green, blue] = rgb.map(|channel| channel / 255.0);
    NSColor::colorWithSRGBRed_green_blue_alpha(red, green, blue, 1.0)
}
