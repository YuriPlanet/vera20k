//! macOS Cmd+Q guard.
//!
//! The game has no autosave. A local NSEvent monitor swallows Cmd+Q before
//! AppKit's menu/terminate handling sees it and leaves a request for the next
//! frame to open the in-game pause menu instead.

use std::sync::atomic::{AtomicBool, Ordering};

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn take_quit_request() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
pub(crate) fn install() {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use std::ptr::NonNull;

    let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a live NSEvent for the monitor's lifetime.
        let event = unsafe { event.as_ref() };
        let flags = unsafe { event.modifierFlags() };
        let is_cmd = flags.contains(NSEventModifierFlags::Command);
        let plain = !flags.contains(NSEventModifierFlags::Control)
            && !flags.contains(NSEventModifierFlags::Option);
        if is_cmd && plain && unsafe { event.keyCode() } == 12 {
            QUIT_REQUESTED.store(true, Ordering::Relaxed);
            return std::ptr::null_mut();
        }
        event as *const NSEvent as *mut NSEvent
    });
    if let Some(token) =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
    {
        // The monitor lives for the process; leaking its token keeps AppKit
        // from uninstalling it when this function returns.
        std::mem::forget(token);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install() {}
