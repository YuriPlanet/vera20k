//! In-battle multiplayer chat: the key pre-hook `0x0055E420` that Process_Command
//! (`0x0055DEE0`) calls first, the chat mask `0xA8D108[8]` that dialog 0x73's
//! chat boxes write, and the chat packet `0x0C`'s send and receive.
//!
//! The MessageList edit itself is `ui::messages`. The transport is the
//! controlled network service: a send queues
//! [`NetworkLoadingRequest::Chat`](crate::app::match_runtime::session::NetworkLoadingRequest)
//! and a received packet arrives as `NetworkMatchEvent::Chat`.
//!
//! RESIDUAL: the beacon-message branch of the pre-hook (`0x0055E4E2`) needs a
//! beacon of the player's House; VERA has no beacons. WOL's `/page` and
//! `/reply` commands (`0x0078A6B0`) and its relay mask (`0x0055F056`) are WOL
//! backend. The IME queue `0xB730EC` is not modelled; typed text arrives as
//! the key event's text, every UTF-16 unit of it. Native WM_CHAR delivers one
//! ANSI byte per character (`0x0054F56C`), and Shift/Ctrl+Backspace delete
//! twice or append `0x7F` there; VERA deletes once.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::keyboard::InGameKeyEdge;
use crate::sim::intern::InternedId;
use crate::ui::messages::{EditInput, EditKey};
use winit::keyboard::{Key, NamedKey};

/// `[0xABCE18]`: who the open line goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ChatTarget {
    #[default]
    None,
    /// 1, Enter: the Houses whose chat-mask byte is set.
    Listeners,
    /// 2, Backspace or Home: allies.
    Allies,
    /// 3, `\` or End: everyone.
    All,
}

/// The chat target and the chat mask. Init_Game (`0x0052CAC8`) and
/// Post_Map_Init (`0x00686B08`) set every mask byte, so a new scenario
/// starts from `default()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatState {
    target: ChatTarget,
    /// Indexed by House array index. Dialog 0x73 writes it (`0x006582C3`);
    /// the local player's own byte is never written there.
    mask: [bool; 8],
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            target: ChatTarget::None,
            mask: [true; 8],
        }
    }
}

impl ChatState {
    pub(crate) fn listens(&self, house_index: usize) -> bool {
        // Native reads past the eight bytes for a higher index; Multiplay
        // Houses take the first indices.
        self.mask.get(house_index).copied().unwrap_or(false)
    }

    /// A chat box click: the byte takes the box's new check state
    /// (`HIWORD(wParam) == 1`, `0x00658294..0x006582C3`).
    pub(crate) fn set_listens(&mut self, house_index: usize, listens: bool) {
        if let Some(byte) = self.mask.get_mut(house_index) {
            *byte = listens;
        }
    }

    /// MPlayer_Defeated's local branch (`0x004FC23E`) sets every byte.
    pub(crate) fn local_player_defeated(&mut self) {
        self.mask = [true; 8];
    }
}

/// KeyToASCII `0x0054F450` for the three keys that open the edit: Enter
/// `0x0D`, Backspace or Home `8` and `\` or End `0x5C`. Home and End map
/// before any modifier is read; for the others ToAscii gives a control
/// character with Ctrl held, unless Alt is held too (AltGr).
fn start_target(logical: &Key, ctrl: bool, alt: bool) -> Option<ChatTarget> {
    match logical {
        Key::Named(NamedKey::Home) => Some(ChatTarget::Allies),
        Key::Named(NamedKey::End) => Some(ChatTarget::All),
        _ if ctrl && !alt => None,
        Key::Named(NamedKey::Enter) => Some(ChatTarget::Listeners),
        Key::Named(NamedKey::Backspace) => Some(ChatTarget::Allies),
        Key::Character(text) if text.as_str() == "\\" => Some(ChatTarget::All),
        _ => None,
    }
}

/// The keys one press feeds Input `0x005D4510`. The window procedure drops
/// key-down repeats (`0x0054F81D`, lParam bit 30) but not WM_CHAR repeats,
/// so held Backspace and held characters repeat while Enter, Escape, Home
/// and End do not. The keyboard map `0x0054F530` turns Home into a delete
/// and End into `\`; a raw VK_BACK deletes nothing, its WM_CHAR does.
fn edit_keys(edge: &InGameKeyEdge<'_>) -> Vec<EditKey> {
    let once = |key| if edge.repeat { Vec::new() } else { vec![key] };
    match edge.logical {
        Key::Named(NamedKey::Enter) => once(EditKey::Enter),
        Key::Named(NamedKey::Escape) => once(EditKey::Escape),
        Key::Named(NamedKey::Home) => once(EditKey::Delete),
        Key::Named(NamedKey::End) => once(EditKey::Unit(u16::from(b'\\'))),
        Key::Named(NamedKey::Backspace) => vec![EditKey::Delete],
        _ => edge
            .text
            .map(|text| text.encode_utf16().map(EditKey::Unit).collect())
            .unwrap_or_else(|| vec![EditKey::Other]),
    }
}

/// The pre-hook for one in-game key edge. True when the key is consumed:
/// it opened the edit, or an edit is open (Input zeroes every key's slot).
pub(crate) fn key_edge(state: &mut AppState, edge: &InGameKeyEdge<'_>) -> bool {
    let list = &state.match_state.match_presentation.message_list;
    if !list.editing() {
        if !edge.state.is_pressed() || edge.repeat || !state.match_state.session_mode().is_network()
        {
            return false;
        }
        let modifiers = state.match_state.input.hotkey_modifiers;
        let Some(target) = start_target(edge.logical, modifiers.control_key(), modifiers.alt_key())
        else {
            return false;
        };
        return open_edit(state, target);
    }
    if !edge.state.is_pressed() {
        return true;
    }
    for key in edit_keys(edge) {
        apply_input(state, key);
    }
    true
}

/// A right press or release (key 2 or `0x802`) ends an open edit; the game
/// still handles the click.
pub(crate) fn right_button(state: &mut AppState) {
    if state.match_state.match_presentation.message_list.editing() {
        apply_input(state, EditKey::RightClick);
    }
}

/// The chat branch `0x0055E519..0x0055E5B4`: the prefix is `TXT_FROM` with
/// the player's House UI name, in the player's colour; overflow is on and
/// no sound plays. The cursor character 0 draws no caret.
fn open_edit(state: &mut AppState, target: ChatTarget) -> bool {
    let Some(local) = local_house(state) else {
        return false;
    };
    let Some(name) = crate::app::presentation::house_ui::house_ui_name(state, local) else {
        return false;
    };
    let template = crate::app::frontend::shell_pass::resolve_csf(state, "TXT_FROM").into_owned();
    let prefix = crate::assets::csf_file::format_csf(
        &template,
        &[crate::assets::csf_file::CsfArg::Str(&name)],
    );
    let rgb = crate::app::input::messages::player_line_rgb(state, local);
    let presentation = &mut state.match_state.match_presentation;
    if presentation.message_list.add_edit(&prefix, rgb, true) {
        presentation.chat.target = target;
    }
    true
}

fn apply_input(state: &mut AppState, key: EditKey) {
    let font = &state.renderer.bit_font;
    let presentation = &mut state.match_state.match_presentation;
    let result = presentation
        .message_list
        .input(key, &|text| font.text_width(text) as i32);
    match result {
        EditInput::Ended if key == EditKey::Escape => presentation.chat.target = ChatTarget::None,
        EditInput::Sent(text) => {
            send(state, &text);
            // 0x0055F1BD: result 3 clears the target.
            state.match_state.match_presentation.chat.target = ChatTarget::None;
        }
        EditInput::Overflowed(chunk) => send(state, &chunk),
        _ => {}
    }
}

/// The send block `0x0055EDBF..0x0055F110` for results 3 and 4.
fn send(state: &mut AppState, text: &str) {
    if !state.match_state.session_mode().is_network() {
        return;
    }
    let Some(local) = local_house(state) else {
        return;
    };
    let target = state.match_state.match_presentation.chat.target;
    let Some((recipients, refused)) = route(state, local, target) else {
        return;
    };
    if refused {
        crate::app::input::messages::post_observer_refusal(state);
        state.match_state.match_presentation.chat.target = ChatTarget::None;
    } else if !recipients.is_empty() {
        state
            .match_state
            .session
            .request_chat(recipients, text.to_owned());
    }
    // 0x0055F0A8..0x0055F104: the local echo under the node name, sent or not.
    let session = &state.match_state.session;
    let name = crate::app::match_runtime::network_match::local_source(session)
        .map(|source| crate::app::match_runtime::network_match::participant_name(session, source))
        .unwrap_or_default();
    let rgb = crate::app::input::messages::player_line_rgb(state, local);
    crate::app::input::messages::post_chat_line(state, &name, text, rgb);
}

/// The observer gate `0x0055EE60..0x0055EF32` and the recipients
/// `0x0055EF38..0x0055F04E`. Returns the participants that get the packet
/// and whether the gate refused the line: once the local player is out
/// (Session+0x300), a Listeners or Allies line with any Multiplay House
/// outside it is not sent. Each Multiplay House inside the target gets the
/// packet on the connection whose name equals its UI name; VERA's
/// participant-to-House map stands in for that match. AI Houses and the
/// local House have no connection.
fn route(state: &AppState, local: InternedId, target: ChatTarget) -> Option<(Vec<usize>, bool)> {
    let runtime = state.match_state.sim_runtime.as_ref()?;
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    let session = &state.match_state.session;
    let connected = session.network_connected_source_indices();
    let local_source = crate::app::match_runtime::network_match::local_source(session);
    let houses: Vec<ChatHouse> = sim
        .houses()
        .order()
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, house)| {
            sim.houses()
                .get(house)
                .and_then(|state| state.country())
                .is_some_and(|country| rules.country_multiplay(sim.interner().resolve(country)))
        })
        .map(|(index, house)| ChatHouse {
            index,
            allied: crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(sim.house_alliances()),
                sim.interner(),
                local,
                house,
            ),
            peer: session
                .network_owner_source(sim.interner().resolve(house))
                .filter(|source| connected.contains(source) && Some(*source) != local_source),
        })
        .collect();
    Some(route_houses(
        target,
        sim.local_player_defeated(),
        &state.match_state.match_presentation.chat,
        &houses,
    ))
}

/// The taunt recipients (`0x0053645A..0x0053652B`): the connected peers of
/// the Multiplay Houses whose chat-mask byte is set. The taunt command has
/// already refused a local player who is out, so the observer gate never
/// refuses here.
pub(crate) fn listener_peers(state: &AppState, local: InternedId) -> Vec<usize> {
    route(state, local, ChatTarget::Listeners).map_or_else(Vec::new, |(recipients, _)| recipients)
}

/// One Multiplay House as the send block sees it.
#[derive(Debug, Clone, Copy)]
struct ChatHouse {
    /// House array index, the chat mask's index.
    index: usize,
    /// IsAlliedWith(local House, this House).
    allied: bool,
    /// The connected peer whose connection carries this House's name.
    peer: Option<usize>,
}

fn route_houses(
    target: ChatTarget,
    local_inactive: bool,
    chat: &ChatState,
    houses: &[ChatHouse],
) -> (Vec<usize>, bool) {
    let inside = |house: &ChatHouse| match target {
        ChatTarget::None => false,
        ChatTarget::Listeners => chat.listens(house.index),
        ChatTarget::Allies => house.allied,
        ChatTarget::All => true,
    };
    if local_inactive
        && matches!(target, ChatTarget::Listeners | ChatTarget::Allies)
        && houses.iter().any(|house| !inside(house))
    {
        return (Vec::new(), true);
    }
    let recipients = houses
        .iter()
        .filter(|house| inside(house))
        .filter_map(|house| house.peer)
        .collect();
    (recipients, false)
}

/// Receive `0x0048D8DD..0x0048D9B8`: the line under the sender's node
/// name in its colour, then one more IncomingMessage (`0x0048D995`) on top
/// of Add_Message's own. No mask, alliance or defeat filter applies.
/// Controlled service input resolves the participant's name/colour. Physical
/// global messages use receive_packet with their original carrier fields.
pub(crate) fn receive(state: &mut AppState, source_index: usize, text: &str) {
    let session = &state.match_state.session;
    let name = crate::app::match_runtime::network_match::participant_name(session, source_index);
    let owner = session
        .network_source_owner(source_index)
        .and_then(|owner| {
            state
                .match_state
                .sim_runtime
                .as_ref()
                .and_then(|runtime| runtime.simulation.interner().get(&owner))
        });
    // Native shows the line whoever sent it; without a House, system white.
    let rgb = owner.map_or([1.0; 3], |owner| {
        crate::app::input::messages::player_line_rgb(state, owner)
    });
    receive_resolved(state, &name, text, rgb);
}

/// Global packet48D8DD supplies its own name and colour. Endpoint admission
/// and the optional -MESSAGES hash gate belong to the network service.
pub(crate) fn receive_packet(state: &mut AppState, name: &str, color: i32, text: &str) {
    let rgb = crate::app::input::messages::packet_player_line_rgb(state, color);
    receive_resolved(state, name, text, rgb);
}

fn receive_resolved(state: &mut AppState, name: &str, text: &str, rgb: [f32; 3]) {
    crate::app::input::messages::post_chat_line(state, name, text, rgb);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.incoming_message_sound().map(str::to_owned));
    crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
}

fn local_house(state: &AppState) -> Option<InternedId> {
    state
        .match_state
        .sim_runtime
        .as_ref()?
        .simulation
        .session()
        .current_house()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_keys_open_the_edit_and_ctrl_blocks_them() {
        let named = |key| Key::Named(key);
        assert_eq!(
            start_target(&named(NamedKey::Enter), false, false),
            Some(ChatTarget::Listeners)
        );
        assert_eq!(
            start_target(&named(NamedKey::Backspace), false, false),
            Some(ChatTarget::Allies)
        );
        assert_eq!(
            start_target(&named(NamedKey::Home), false, false),
            Some(ChatTarget::Allies)
        );
        assert_eq!(
            start_target(&named(NamedKey::End), false, false),
            Some(ChatTarget::All)
        );
        assert_eq!(
            start_target(&Key::Character("\\".into()), false, false),
            Some(ChatTarget::All)
        );
        assert_eq!(
            start_target(&Key::Character("a".into()), false, false),
            None
        );
        assert_eq!(start_target(&named(NamedKey::Enter), true, false), None);
        assert_eq!(
            start_target(&named(NamedKey::End), true, false),
            Some(ChatTarget::All),
            "Home and End ignore Ctrl"
        );
        assert_eq!(
            start_target(&Key::Character("\\".into()), true, true),
            Some(ChatTarget::All),
            "AltGr"
        );
    }

    fn houses() -> [ChatHouse; 3] {
        [
            // The local House, an allied peer and an enemy peer.
            ChatHouse {
                index: 0,
                allied: true,
                peer: None,
            },
            ChatHouse {
                index: 1,
                allied: true,
                peer: Some(1),
            },
            ChatHouse {
                index: 2,
                allied: false,
                peer: Some(2),
            },
        ]
    }

    #[test]
    fn each_target_picks_its_houses_connections() {
        let mut chat = ChatState::default();
        let houses = houses();
        let route = |target, chat: &ChatState| route_houses(target, false, chat, &houses);
        assert_eq!(route(ChatTarget::Listeners, &chat), (vec![1, 2], false));
        assert_eq!(route(ChatTarget::Allies, &chat), (vec![1], false));
        assert_eq!(route(ChatTarget::All, &chat), (vec![1, 2], false));
        assert_eq!(route(ChatTarget::None, &chat), (vec![], false));
        chat.set_listens(2, false);
        assert_eq!(route(ChatTarget::Listeners, &chat), (vec![1], false));
    }

    #[test]
    fn an_inactive_player_may_only_reach_everyone() {
        let mut chat = ChatState::default();
        let houses = houses();
        assert_eq!(
            route_houses(ChatTarget::Listeners, true, &chat, &houses),
            (vec![1, 2], false),
            "every House listens"
        );
        assert_eq!(
            route_houses(ChatTarget::Allies, true, &chat, &houses),
            (vec![], true),
            "an enemy is outside"
        );
        chat.set_listens(1, false);
        assert_eq!(
            route_houses(ChatTarget::Listeners, true, &chat, &houses),
            (vec![], true)
        );
        assert_eq!(
            route_houses(ChatTarget::All, true, &chat, &houses),
            (vec![1, 2], false)
        );
    }

    #[test]
    fn the_mask_starts_set_and_defeat_sets_it_again() {
        let mut chat = ChatState::default();
        assert!((0..8).all(|index| chat.listens(index)));
        chat.set_listens(2, false);
        assert!(!chat.listens(2));
        assert!(!chat.listens(8), "no byte past the eighth");
        chat.local_player_defeated();
        assert!(chat.listens(2));
    }
}
