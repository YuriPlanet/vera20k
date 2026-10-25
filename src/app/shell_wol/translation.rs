//! The app side of VERA's chat translation ([`crate::ui::chat_translation`]):
//! the `/translate` command, which received lines are sent, and where their
//! translations appear. The online service's request queue carries them to
//! the lobby; WOL's test service never sees them.

use super::{WolProvider, WolServiceRequest};
use crate::app::AppState;
use crate::ui::chat_translation::{self, TranslateCommand};
use crate::ui::lan_shell::LanChatLine;

/// Where a translation is shown: under a WOL transcript line, with that
/// line's prefix and colour, or as a match message row from its sender.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum TranslatedLine {
    Wol { prefix: Vec<u16>, color: u32 },
    Match { name: String, color: i32 },
}

/// Set between a line's prefix and its translation.
const MARKER: &str = "» ";

/// `/translate` typed into a WOL chat edit shows, saves or stops the choice.
/// Any other line returns false and goes to the native parser.
pub(crate) fn command(state: &mut AppState, line: &[u16]) -> bool {
    let Some(command) = chat_translation::parse_command(line) else {
        return false;
    };
    let text = match command {
        TranslateCommand::Show => {
            chat_translation::notice(state.persistence.options_profile.translate_chat)
        }
        TranslateCommand::Usage => chat_translation::USAGE.to_owned(),
        TranslateCommand::Set(language) => {
            let profile = &mut state.persistence.options_profile;
            profile.translate_chat = language;
            if let Some(config) = state.platform.game_config.as_ref()
                && let Err(error) = profile.commit_chat_translation(config.paths.settings_dir())
            {
                log::warn!("Could not save the chat translation language: {error}");
            }
            chat_translation::notice(language)
        }
    };
    crate::app::shell_lan::append_chat(
        state,
        LanChatLine {
            text: text.encode_utf16().collect(),
            color: u32::MAX,
        },
    );
    true
}

/// A line another player wrote reached the screen. With a language chosen
/// and the line in the other script, ask the online service to translate it.
pub(crate) fn received(state: &mut AppState, text: &[u16], line: TranslatedLine) {
    let Some(target) = state.persistence.options_profile.translate_chat else {
        return;
    };
    if target.needs(text)
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

/// A translation arrived. A WOL line shows outside a match, a match row in
/// one; a translation that outlived its screen is dropped.
pub(super) fn show(state: &mut AppState, line: TranslatedLine, translated: &str) {
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
