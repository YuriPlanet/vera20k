//! macOS Cmd+Q during a match: the pause menu, not the process.
//!
//! The game has no autosave, so a stray Cmd+Q in a match loses it. AppKit's
//! default menu owns that shortcut, so the key never reaches winit while the
//! menu exists: a local NSEvent monitor sees it first. The monitor swallows it
//! only while a match is running; on the menus it hands the event back, so
//! Cmd+Q still quits the way the menu says.
//!
//! Where the key is swallowed, the request is served from the key handling in
//! `handler.rs` and treated exactly like Escape: a modal closes, the world
//! opens the pause menu. A request that nobody uses is cleared, so it cannot
//! open a menu by itself later.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set by the monitor, taken once by the key handling.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether a match is running, so the monitor leaves the menus to AppKit.
static IN_MATCH: AtomicBool = AtomicBool::new(false);

/// What one Cmd+Q press means where it arrived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuitAction {
    /// Outside a match, AppKit's menu quits the process.
    Ignore,
    /// A modal owns Escape here, so Cmd+Q closes it the way Escape does.
    CloseMenu,
    /// The world has no modal: Escape's Options fallback opens the pause menu.
    OpenPauseMenu,
}

/// The action for a Cmd+Q that arrived while `in_match`.
pub(crate) fn quit_action(in_match: bool, menu_owns_escape: bool) -> QuitAction {
    if !in_match {
        QuitAction::Ignore
    } else if menu_owns_escape {
        QuitAction::CloseMenu
    } else {
        QuitAction::OpenPauseMenu
    }
}

/// Is this the Cmd+Q shortcut?
///
/// `text` is the event's characters without modifiers, so the check follows
/// the keyboard layout: on AZERTY the key that types Q matches, and Cmd+A
/// (which sits where Q is on QWERTY) does not.
pub(crate) fn is_quit_shortcut(
    text: Option<&str>,
    command: bool,
    control: bool,
    option: bool,
) -> bool {
    command && !control && !option && text.is_some_and(|text| text.eq_ignore_ascii_case("q"))
}

/// The monitor saw Cmd+Q during a match; the key handling serves it.
pub(crate) fn note_quit_shortcut() {
    QUIT_REQUESTED.store(true, Ordering::Release);
}

/// Takes the pending request, if any.
pub(crate) fn take_quit_request() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::AcqRel)
}

/// Lets the monitor know whether a match is running.
pub(crate) fn set_in_match(in_match: bool) {
    IN_MATCH.store(in_match, Ordering::Release);
}

#[cfg(target_os = "macos")]
pub(crate) fn install() {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
    use std::ptr::NonNull;

    let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor a live NSEvent.
        let event = unsafe { event.as_ref() };
        let flags = unsafe { event.modifierFlags() };
        let text = unsafe { event.charactersIgnoringModifiers() }.map(|text| text.to_string());
        let quit = IN_MATCH.load(Ordering::Acquire)
            && is_quit_shortcut(
                text.as_deref(),
                flags.contains(NSEventModifierFlags::Command),
                flags.contains(NSEventModifierFlags::Control),
                flags.contains(NSEventModifierFlags::Option),
            );
        if quit {
            note_quit_shortcut();
            return std::ptr::null_mut();
        }
        // Not a match: let AppKit's menu quit as it always did.
        event as *const NSEvent as *mut NSEvent
    });
    if let Some(token) = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    } {
        // The monitor lives for the process; leaking the token keeps AppKit
        // from uninstalling it when this function returns.
        std::mem::forget(token);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn install() {}

#[cfg(test)]
mod tests {
    use super::{QuitAction, is_quit_shortcut, quit_action};

    #[test]
    fn cmd_q_outside_a_match_is_left_to_appkit() {
        assert_eq!(quit_action(false, false), QuitAction::Ignore);
        assert_eq!(quit_action(false, true), QuitAction::Ignore);
    }

    #[test]
    fn cmd_q_in_a_match_matches_escape() {
        assert_eq!(quit_action(true, false), QuitAction::OpenPauseMenu);
        assert_eq!(quit_action(true, true), QuitAction::CloseMenu);
    }

    #[test]
    fn the_shortcut_follows_the_typed_letter() {
        assert!(is_quit_shortcut(Some("q"), true, false, false));
        assert!(is_quit_shortcut(Some("Q"), true, false, false));
        // AZERTY: the key that types Q matches even though it is not US-Q.
        assert!(is_quit_shortcut(Some("q"), true, false, false));
        assert!(!is_quit_shortcut(Some("a"), true, false, false));
        assert!(!is_quit_shortcut(Some("q"), false, false, false));
        assert!(!is_quit_shortcut(Some("q"), true, true, false));
        assert!(!is_quit_shortcut(Some("q"), true, false, true));
        assert!(!is_quit_shortcut(None, true, false, false));
    }
}
