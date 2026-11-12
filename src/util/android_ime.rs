//! The Android soft keyboard and its text, for the chat and name edits.
//!
//! winit's Android backend shows and hides the keyboard (`set_ime_allowed`)
//! but reports no text: that backend has no `WindowEvent::Ime` producer, and
//! the `GameTextInput` buffer the IME edits is reachable only through
//! `AndroidApp::text_input_state`. Winit #4736 adds the translation upstream
//! (<https://github.com/rust-windowing/winit/pull/4736>); until it lands the
//! Android crate installs these hooks, `app::input::ime` seeds the buffer with
//! the focused edit, asks the keyboard to show, and turns each new buffer into
//! committed text.
//!
//! The hooks stay uninstalled everywhere else, so every function is a no-op
//! and the module still compiles for the desktop and the browser.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the input method holds: its whole buffer and the caret within it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextState {
    pub text: String,
    pub cursor: usize,
}

/// What the Android crate does for this module.
pub struct Hooks {
    /// Show the soft keyboard.
    pub show: fn(),
    /// Hide it.
    pub hide: fn(),
    /// The buffer the input method currently edits.
    pub state: fn() -> TextState,
    /// Replace that buffer, with the caret at `cursor` (in characters).
    pub set_state: fn(&str, usize),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// A tap on an edit asks for the keyboard again, even when the input method
/// still counts as enabled: the player may have collapsed it with its own hide
/// button, which the engine never sees.
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Ask for the keyboard on the next frame; the tap that asks is what a
/// collapsed keyboard has to answer.
pub fn request_show() {
    SHOW_REQUESTED.store(true, Ordering::Release);
}

/// Whether a tap asked for the keyboard, clearing the request.
pub fn take_show_request() -> bool {
    SHOW_REQUESTED.swap(false, Ordering::AcqRel)
}

/// Drop a request made on a screen the player has since left, so a new lobby
/// does not open with the keyboard up.
pub fn clear_show_request() {
    SHOW_REQUESTED.store(false, Ordering::Release);
}

/// Install the activity hooks. The Android crate calls this once, before the
/// event loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// Show the soft keyboard, when this build has an input method.
pub fn show() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.show)();
    }
}

/// Hide the soft keyboard.
pub fn hide() {
    if let Some(hooks) = HOOKS.get() {
        (hooks.hide)();
    }
}

/// The buffer the input method edits, empty when no hook is installed.
pub fn state() -> TextState {
    HOOKS.get().map(|hooks| (hooks.state)()).unwrap_or_default()
}

/// Replace the input method's buffer, with the caret at `cursor` characters.
pub fn set_state(text: &str, cursor: usize) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.set_state)(text, cursor);
    }
}

/// The change between two buffers, as the game must apply it: how many
/// characters to delete before the caret, and the text that replaces them.
///
/// The input method reports its whole buffer, so a keystroke shows up as the
/// span between the common prefix and the common suffix.
pub fn edit_of(old: &str, new: &str) -> (usize, String) {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(before, after)| before == after)
        .count();
    let shared = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(shared)
        .take_while(|(before, after)| before == after)
        .count();
    let removed = old.len() - prefix - suffix;
    let inserted: String = new[prefix..new.len() - suffix].iter().collect();
    (removed, inserted)
}

#[cfg(test)]
mod tests {
    use super::edit_of;

    #[test]
    fn typing_at_the_end_commits_the_added_characters() {
        assert_eq!(edit_of("", "abc"), (0, "abc".into()));
        assert_eq!(edit_of("abc", "abcd"), (0, "d".into()));
    }

    #[test]
    fn a_backspace_deletes_one_character_and_commits_nothing() {
        assert_eq!(edit_of("abcd", "abc"), (1, String::new()));
    }

    #[test]
    fn replacing_a_span_deletes_it_before_the_replacement() {
        assert_eq!(edit_of("abcd", "abXd"), (1, "X".into()));
        assert_eq!(edit_of("ab", "aXb"), (0, "X".into()));
    }
}
