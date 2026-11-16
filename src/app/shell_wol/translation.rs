//! The app side of VERA's chat translation ([`crate::ui::chat_translation`]):
//! which received lines are sent, and where their translations appear. The
//! online service's request queue carries them to the lobby; WOL's test
//! service never sees them. Each reply is checked against the language as it
//! is when it arrives.

use super::{WolProvider, WolServiceRequest};
use crate::app::AppState;
use crate::ui::chat_translation::{ChatReply, MARKER, without_note};
use crate::ui::lan_shell::LanChatLine;

/// Where a translation is shown: under a WOL transcript line, with that
/// line's prefix and colour, or as a match message row from its sender.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum TranslatedLine {
    Wol { prefix: Vec<u16>, color: u32 },
    Match { name: String, color: i32 },
}

/// A line another player wrote reached the screen. If the chosen language
/// needs it translated, and it was not said in a match while Translate in
/// Matches is off, ask the online service.
pub(crate) fn received(state: &mut AppState, text: &[u16], line: TranslatedLine) {
    let chat = state.persistence.options_profile.chat_translation;
    if chat.sends(text, matches!(line, TranslatedLine::Match { .. }))
        && state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| visit.provider == WolProvider::Online)
    {
        super::service::request(
            state,
            WolServiceRequest::Translate {
                text: text.to_vec(),
                line,
            },
        );
    }
}

/// A translation arrived, shown without the translator's note naming the
/// source language. A WOL line shows outside a match, a match row in one; a
/// translation that outlived its screen is dropped, and so is one that
/// arrives after translation was turned off or that the translator says was
/// already in the chosen language.
pub(super) fn show(state: &mut AppState, line: TranslatedLine, reply: &ChatReply) {
    let language = state.persistence.options_profile.chat_translation.language;
    if !language.is_some_and(|language| language.shows(reply)) {
        return;
    }
    let translated = without_note(&reply.text);
    let in_match = super::in_match(state);
    match line {
        TranslatedLine::Wol { mut prefix, color } if !in_match => {
            prefix.extend(MARKER.encode_utf16().chain(translated.encode_utf16()));
            crate::app::shell_lan::append_chat(
                state,
                LanChatLine {
                    text: prefix,
                    color,
                },
            );
            state.platform.window.request_redraw();
        }
        TranslatedLine::Match { name, color } if in_match => {
            let rgb = crate::app::input::messages::packet_player_line_rgb(state, color);
            crate::app::input::messages::post_chat_line(
                state,
                &name,
                &format!("{MARKER}{translated}"),
                rgb,
            );
        }
        _ => {}
    }
}
