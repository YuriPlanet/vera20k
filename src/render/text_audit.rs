//! The texts the interface lays out, for checking a language on every screen.
//!
//! With `VERA20K_TEXT_AUDIT` set, the log names each distinct text the text
//! engine lays out, once, and each text its box cuts off even at the tightest
//! squeeze ([`crate::render::text_engine::Fit`]). Under the pseudo-language
//! every text from a language file or the game's string table reads in
//! brackets, so a named text without them comes from neither, and a cut-off
//! one needs a shorter translation or a wider box.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, PoisonError};

static SEEN: LazyLock<Option<Mutex<HashSet<(Kind, String)>>>> =
    LazyLock::new(|| std::env::var_os("VERA20K_TEXT_AUDIT").map(|_| Mutex::default()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    LaidOut,
    CutOff,
}

/// `text` was shaped for drawing or measuring.
pub(crate) fn laid_out(text: &str) {
    note(Kind::LaidOut, text);
}

/// `text` is wider than `width` native pixels, or needs more lines than its
/// box holds, at every squeeze.
pub(crate) fn cut_off(text: &str, width: u32) {
    note(Kind::CutOff, &format!("{text} (box {width} px)"));
}

fn note(kind: Kind, text: &str) {
    let Some(seen) = SEEN.as_ref() else {
        return;
    };
    let new = seen
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert((kind, text.to_owned()));
    if new {
        log::info!("Text audit, {kind:?}: {text:?}");
    }
}
