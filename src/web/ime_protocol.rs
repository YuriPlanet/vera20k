//! How a browser's composition events map to winit's `Ime` events, which
//! `app::input::ime` consumes. DOM wiring lives in `web::ime`.
//!
//! Browsers order the events differently: Chrome's last `input` comes before
//! `compositionend`, Firefox's after it, by when the edit is already empty.
//! <https://w3c.github.io/uievents/#events-composition-input-events>

use winit::event::Ime;

/// `KeyboardEvent.keyCode` of a key the input method is processing.
const PROCESS_KEY_CODE: u32 = 229;

/// Whether a keydown belongs to the input method rather than the game.
/// `isComposing` alone misses the key that opens a composition and, in Safari,
/// the Enter that closes one.
/// <https://developer.mozilla.org/en-US/docs/Web/API/Element/keydown_event#keydown_events_with_ime>
pub(crate) fn input_method_owns_key(is_composing: bool, key_code: u32, key: &str) -> bool {
    is_composing || key_code == PROCESS_KEY_CODE || key == "Process"
}

/// The open composition of one edit.
#[derive(Debug, Default)]
pub(crate) struct Composition {
    open: bool,
}

impl Composition {
    pub(crate) fn start(&mut self) {
        self.open = true;
    }

    /// An `input` event, with the edit's value and caret in UTF-16 units. The
    /// edit holds only the composition: keys that type are forwarded instead.
    /// Text arriving with no composition open, from an emoji panel or Safari's
    /// direct insertion, is already finished. Returns the event and whether the
    /// edit must now be emptied.
    pub(crate) fn input(&mut self, value: &str, caret: Option<u32>) -> Option<(Ime, bool)> {
        if self.open {
            let byte = caret.map(|caret| byte_offset(value, caret));
            return Some((
                Ime::Preedit(value.to_owned(), byte.map(|at| (at, at))),
                false,
            ));
        }
        (!value.is_empty()).then(|| (Ime::Commit(value.to_owned()), true))
    }

    /// `compositionend`. Cancelling an input method leaves no text.
    pub(crate) fn end(&mut self, data: &str) -> Ime {
        self.open = false;
        if data.is_empty() {
            Ime::Preedit(String::new(), None)
        } else {
            Ime::Commit(data.to_owned())
        }
    }
}

/// The byte offset of a UTF-16 position, as winit reports a composition cursor.
fn byte_offset(text: &str, utf16: u32) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= utf16 as usize {
            return byte;
        }
        units += character.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_that_opens_a_composition_stays_with_the_input_method() {
        assert!(input_method_owns_key(false, 229, "n"));
        assert!(input_method_owns_key(false, 0, "Process"));
        assert!(input_method_owns_key(true, 13, "Enter"));
        assert!(!input_method_owns_key(false, 13, "Enter"));
    }

    #[test]
    fn firefox_input_after_compositionend_does_not_commit_twice() {
        let mut composition = Composition::default();
        composition.start();
        assert_eq!(composition.end("你好"), Ime::Commit("你好".to_owned()));
        // web::ime empties the edit on the commit, so this `input` finds nothing.
        assert_eq!(composition.input("", Some(0)), None);
    }

    #[test]
    fn composition_caret_converts_from_utf16_units_to_bytes() {
        let mut composition = Composition::default();
        composition.start();
        assert_eq!(
            composition.input("你a😀b", Some(4)),
            Some((Ime::Preedit("你a😀b".to_owned(), Some((8, 8))), false))
        );
        assert_eq!(composition.end(""), Ime::Preedit(String::new(), None));
    }
}
