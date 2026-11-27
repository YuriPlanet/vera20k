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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// What the input method holds: its whole buffer and the caret within it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextState {
    pub text: String,
    /// Selection start in UTF-16 units, as reported by Android.
    pub cursor: usize,
    /// The composing range within `text`, in UTF-16 units. The input method
    /// owns it until it commits.
    pub compose: Option<(usize, usize)>,
}

impl TextState {
    /// Seed the keyboard from an edit whose caret counts characters.
    pub fn from_edit(text: &str, cursor: usize) -> Self {
        Self {
            text: text.to_owned(),
            cursor: text.chars().take(cursor).map(char::len_utf16).sum(),
            compose: None,
        }
    }
}

/// Split the platform's buffer into the text that is committed and the open
/// composition, both as UTF-16 units, with the composition's caret inside it.
/// Committed text excludes the composing range, so a pending pinyin run never
/// reaches the game as typed letters.
pub fn split_composition(
    units: &[u16],
    compose: Option<(usize, usize)>,
    cursor: usize,
) -> (Vec<u16>, Option<(Vec<u16>, usize)>) {
    let Some((start, end)) = compose else {
        return (units.to_vec(), None);
    };
    let start = start.min(units.len());
    let end = end.clamp(start, units.len());
    if start == end {
        return (units.to_vec(), None);
    }
    let mut committed = Vec::with_capacity(units.len() - (end - start));
    committed.extend_from_slice(&units[..start]);
    committed.extend_from_slice(&units[end..]);
    let preedit = units[start..end].to_vec();
    let caret = cursor.saturating_sub(start).min(preedit.len());
    (committed, Some((preedit, caret)))
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
    /// Tell the input method whether the focused edit hides its text, so it
    /// cannot learn or suggest a password.
    pub set_secret: fn(bool),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// A tap on an edit asks for the keyboard again, even when the input method
/// still counts as enabled: the player may have collapsed it with its own hide
/// button, which the engine never sees.
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Android shows the keyboard for a tapped edit only, so a tap arms one shell
/// dialog here, in one place for every screen: `app::input::ime` asks for the
/// keyboard for the focused edit of that dialog, and the pages give their edits
/// the focus when they open, so no page brings the keyboard up by itself. The
/// arming is per dialog, so a tap on one page's edit never opens the keyboard
/// on the next.
static ARMED: AtomicU32 = AtomicU32::new(0);

/// Record a tap on `dialog`'s GUI edit; the keyboard may come up for its
/// focused edit.
pub fn arm(dialog: u16) {
    ARMED.store(u32::from(dialog) + 1, Ordering::Release);
}

/// Whether a tap armed `dialog`'s edit.
pub fn armed(dialog: u16) -> bool {
    ARMED.load(Ordering::Acquire) == u32::from(dialog) + 1
}

/// Drop the arming; the keyboard hides with it.
pub fn clear_armed() {
    ARMED.store(0, Ordering::Release);
}

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

/// Tell the input method whether the focused edit hides its text, so its
/// dictionary and suggestions never see a password.
pub fn set_secret(secret: bool) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.set_secret)(secret);
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

/// The platform buffer must follow the edit when a replay left the two
/// disagreeing: the edit refused a character (a byte-only name box, a length
/// cap) or a screen's filter dropped one. Returns the edit's text to re-seed
/// with, or `None` when they already agree.
///
/// Without this the baseline stays the platform's text, so the next Backspace
/// removes the character before the one the player meant.
pub fn resync_to(edit: &str, platform: &str) -> Option<String> {
    (edit != platform).then(|| edit.to_owned())
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::{TextState, edit_of, resync_to};

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
    fn keyboard_keeps_supplementary_text_and_uses_a_utf16_caret() {
        let state = TextState::from_edit("a𠀀🙂b", 3);
        assert_eq!(state.text, "a𠀀🙂b");
        assert_eq!(state.cursor, 5);
    }

    #[test]
    fn a_character_the_edit_refused_makes_the_platform_follow_it() {
        // The LAN name box keeps ASCII, so the platform can hold a character
        // the edit dropped. Replaying a Backspace from the platform's text
        // would remove the character before it; the edit's text is what the
        // platform must show instead.
        let corrected = resync_to("ab", "ab你").unwrap();
        assert_eq!(corrected, "ab");
        assert_eq!(edit_of(&corrected, "ab"), (0, String::new()));
        assert_eq!(resync_to("ab", &corrected), None);
    }

    #[test]
    fn replacing_a_span_deletes_it_before_the_replacement() {
        assert_eq!(edit_of("abcd", "abXd"), (1, "X".into()));
        assert_eq!(edit_of("ab", "aXb"), (0, "X".into()));
    }
}

#[cfg(all(test, tests_util))]
mod composition_tests {
    use super::split_composition;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn no_composition_keeps_the_whole_buffer() {
        let buffer = units("hello");
        assert_eq!(split_composition(&buffer, None, 5), (buffer, None));
    }

    #[test]
    fn a_composition_is_held_back_and_its_caret_is_relative() {
        // "ni" composed at the end of "hi": the platform's buffer is "hini".
        let (committed, preedit) = split_composition(&units("hini"), Some((2, 4)), 4);
        assert_eq!(String::from_utf16_lossy(&committed), "hi");
        let (text, caret) = preedit.unwrap();
        assert_eq!(String::from_utf16_lossy(&text), "ni");
        assert_eq!(caret, 2);
    }

    #[test]
    fn an_empty_composition_reads_as_none() {
        let buffer = units("hi");
        assert_eq!(split_composition(&buffer, Some((2, 2)), 2), (buffer, None));
    }
}
