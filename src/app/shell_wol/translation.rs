//! The app side of VERA's chat translation ([`crate::ui::chat_translation`]):
//! which received lines are sent, into which language, and where their
//! translations appear. The online service's request queue carries them to
//! the lobby; WOL's test service never sees them. A reply shows only while
//! the language it was asked for is still the chosen one.

use super::{WolProvider, WolServiceRequest};
use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::shell_wol::WolVisit;
use crate::audio::sfx::SfxPlayer;
use crate::ui::chat_translation::{ChatLanguage, ChatReply, MARKER, without_note};
use crate::ui::lan_shell::LanChatLine;

/// Where a translation is shown: under a WOL transcript line, with that
/// line's prefix and colour, or as a match message row from its sender.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum TranslatedLine {
    /// `visit`: a line of the public lobby a visitor looks round from their
    /// room, which goes to the lobby page while they still do.
    Wol {
        prefix: Vec<u16>,
        color: u32,
        visit: bool,
    },
    Match {
        name: String,
        color: i32,
    },
}

/// A line another player wrote reached the screen; `localized` says it is
/// the lobby's own line, shown from its phrases in the interface language.
/// If the chosen language needs it translated, and it was not said in a
/// match while Translate in Matches is off, ask the online service.
pub(crate) fn received(
    mut wol: Option<&mut WolVisit>,
    options_profile: &RetailOptionsProfile,
    text: &[u16],
    localized: bool,
    line: TranslatedLine,
) {
    let shown_in = localized
        .then(|| ChatLanguage::of_interface(&crate::i18n::selected_language()))
        .flatten();
    let chat = options_profile.chat_translation;
    let in_match = matches!(line, TranslatedLine::Match { .. });
    let Some(target) = chat.target(text, in_match, shown_in) else {
        return;
    };
    if wol
        .as_deref()
        .is_some_and(|visit| visit.provider == WolProvider::Online)
    {
        super::service::request(
            wol.as_deref_mut(),
            WolServiceRequest::Translate {
                text: text.to_vec(),
                target,
                line,
            },
        );
    }
}

/// A translation into `target` arrived, shown without the translator's note
/// naming the source language. A WOL line shows outside a match, a match row
/// in one; a translation that outlived its screen is dropped, and so is one
/// that arrives after the chosen language changed or that the translator
/// says was already in it.
pub(super) fn show(
    frontend: &mut FrontendState,
    platform: &PlatformState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    line: TranslatedLine,
    target: ChatLanguage,
    reply: &ChatReply,
) {
    let chat = persistence.options_profile.chat_translation;
    if !chat.shows(target, reply) {
        return;
    }
    let translated = without_note(&reply.text);
    let in_match = super::in_match(frontend.wol.as_ref());
    match line {
        TranslatedLine::Wol {
            mut prefix,
            color,
            visit,
        } if !in_match => {
            prefix.extend(MARKER.encode_utf16().chain(translated.encode_utf16()));
            let line = LanChatLine {
                text: prefix,
                color,
            };
            if !visit {
                crate::app::shell_lan::append_room_chat(
                    renderer,
                    frontend,
                    match_state,
                    process_assets,
                    line,
                );
            } else if super::lobby::visiting(frontend.wol.as_ref()) {
                crate::app::shell_lan::append_wol_lobby_chat(
                    renderer,
                    frontend,
                    match_state,
                    process_assets,
                    line,
                    None,
                );
            }
            platform.window.request_redraw();
        }
        TranslatedLine::Match { name, color } if in_match => {
            let rgb =
                crate::app::input::messages::packet_player_line_rgb(frontend, match_state, color);
            crate::app::input::messages::post_chat_line(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                sfx_player,
                persistence,
                &name,
                &format!("{MARKER}{translated}"),
                rgb,
            );
        }
        _ => {}
    }
}
