//! In-battle multiplayer chat: the key pre-hook `0x0055E420` that Process_Command
//! (`0x0055DEE0`) calls first, the chat mask `0xA8D108[8]` that dialog 0x73's
//! chat boxes write, and the chat packet `0x0C`'s send and receive.
//!
//! The MessageList edit itself is `ui::messages`. The transport is the
//! controlled network service: a send queues
//! [`NetworkLoadingRequest::Chat`](crate::app::match_runtime::session::NetworkLoadingRequest)
//! and a received packet arrives as `NetworkMatchEvent::Chat`.
//!
//! With an own beacon selected, the same three keys open a hidden beacon
//! edit instead (`0x0055E4F0..0x0055E5F8`): the typed text, with a `_`
//! cursor, is written into the selected beacon's label as it changes
//! (`0x0055E642..0x0055E734`) and Enter sends it as the label
//! (`0x0055E77B..0x0055E7AE`), not as a line. VERA also writes the label
//! after committed input-method text; native's head loop
//! (`0x0055E42C..0x0055E465`) leaves that to the next key.
//!
//! RESIDUAL: WOL's chat commands, which take a line before the send block
//! (page `0x0078A6B0`, online `0x0078A470`, reply `0x0078A8F0`), and its relay
//! mask (`0x0055F056`) are WOL backend. Reply also takes a line that starts
//! with `TXT_REPLY_COMMAND_SHORT`; a port must leave VERA's `/pause` and
//! `/resume` lines ([`pause_command`]) to the send block, which sends them like
//! any line before they ask for the shared pause. Typed text arrives as the
//! key event's text, every UTF-16 unit of it, and input-method text through
//! [`commit_text`]. Native WM_CHAR delivers one ANSI byte per character
//! (`0x0054F56C`), and Shift/Ctrl+Backspace delete twice or append `0x7F`
//! there; VERA deletes once.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::keyboard::InGameKeyEdge;
use crate::app::match_runtime::beacons::{self, BeaconTarget};
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
    muted: std::collections::BTreeSet<usize>,
    /// VERA: part of the open line already went out as an overflow chunk, so
    /// what is left of it is chat, never a command.
    overflowed: bool,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            target: ChatTarget::None,
            muted: Default::default(),
            overflowed: false,
        }
    }
}

impl ChatState {
    pub(crate) fn listens(&self, house_index: usize) -> bool {
        // VERA extends the native eight-byte mask to actual House indices,
        // including player Houses after Neutral/Special in a large roster.
        !self.muted.contains(&house_index)
    }

    /// A chat box click: the byte takes the box's new check state
    /// (`HIWORD(wParam) == 1`, `0x00658294..0x006582C3`).
    pub(crate) fn set_listens(&mut self, house_index: usize, listens: bool) {
        if listens {
            self.muted.remove(&house_index);
        } else {
            self.muted.insert(house_index);
        }
    }

    /// MPlayer_Defeated's local branch (`0x004FC23E`) sets every byte.
    pub(crate) fn local_player_defeated(&mut self) {
        self.muted.clear();
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
        // 0x0055E4E2: PlayerPtr's selected beacon picks the branch.
        if beacons::own_selected(state) {
            return open_beacon_edit(state);
        }
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

/// The pre-hook's head (`0x0055E42C..0x0055E465`): every unit queued from
/// WM_IME_CHAR at `0xB730EC` goes to Input before the frame's key.
pub(crate) fn commit_text(state: &mut AppState, text: &str) {
    if !state.match_state.match_presentation.message_list.editing() {
        return;
    }
    for unit in text.encode_utf16() {
        apply_input(state, EditKey::Unit(unit));
    }
    // VERA: the label shows the committed text now (see the module notes).
    if beacons::own_selected(state) {
        follow_label(state, None);
    }
}

/// The pre-hook's composition test `[0x00B7356C]` (`0x0055E671`): while an
/// input-method composition is open, the selected beacon's label follows
/// the line and the composition.
pub(crate) fn composition_changed(state: &mut AppState) {
    if state.match_state.match_presentation.message_list.editing() && composition(state).is_some() {
        follow_label(state, None);
    }
}

/// The open composition `0x00B73318` the label appends.
fn composition(state: &AppState) -> Option<Vec<u16>> {
    state
        .platform
        .ime
        .composition(crate::app::input::ime::ImeTarget::InGameChat)
        .map(|(units, _)| units.to_vec())
}

/// `0x0055E6B3..0x0055E734`: GetEditBuffer, then the composition and `_`,
/// written into the first selected beacon without sending it. `typed`
/// stands in for the list's buffer once Input has ended the edit.
fn follow_label(state: &mut AppState, typed: Option<Vec<u16>>) {
    let mut text = typed
        .or_else(|| {
            let list = &state.match_state.match_presentation.message_list;
            list.edit_typed().map(<[u16]>::to_vec)
        })
        .unwrap_or_default();
    text.extend(composition(state).unwrap_or_default());
    text.push(u16::from(b'_'));
    beacons::edit_beacon_message(state, Some(&text), BeaconTarget::Selected, false);
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
    // VERA shared control: a shared House's player writes under its own name.
    let session = &state.match_state.session;
    let Some(name) = crate::app::match_runtime::network_match::local_source(session)
        .and_then(|source| {
            crate::app::match_runtime::network_match::shared_house_player_name(session, source)
        })
        .or_else(|| crate::app::presentation::house_ui::house_ui_name(state, local))
    else {
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
        presentation.chat.overflowed = false;
    }
    true
}

/// The beacon branch `0x0055E4F0..0x0055E5F8`: a `TXT_BEACON_MESSAGE` edit
/// in the player's colour, hidden (`0x0055E5C1`), without overflow
/// (`0x0055E5CA`) and with the chat target untouched; the selected beacon's
/// label becomes the bare cursor.
fn open_beacon_edit(state: &mut AppState) -> bool {
    let Some(local) = local_house(state) else {
        return false;
    };
    let prefix =
        crate::app::frontend::shell_pass::resolve_csf(state, "TXT_BEACON_MESSAGE").into_owned();
    let rgb = crate::app::input::messages::player_line_rgb(state, local);
    let list = &mut state.match_state.match_presentation.message_list;
    if list.add_edit(&prefix, rgb, false) {
        list.hide_edit();
    }
    let cursor = [u16::from(b'_')];
    beacons::edit_beacon_message(state, Some(&cursor), BeaconTarget::Selected, false);
    true
}

fn apply_input(state: &mut AppState, key: EditKey) {
    // GetEditBuffer 0x005D4400 reads the list's own buffer, which keeps the
    // last text after Input ends the edit.
    let before = state
        .match_state
        .match_presentation
        .message_list
        .edit_typed()
        .map(<[u16]>::to_vec)
        .unwrap_or_default();
    let font = &state.renderer.bit_font;
    let presentation = &mut state.match_state.match_presentation;
    let result = presentation
        .message_list
        .input(key, &|text| font.text_width(text) as i32);
    let typed = match &result {
        EditInput::Sent(text) => text.encode_utf16().collect(),
        _ => presentation
            .message_list
            .edit_typed()
            .map_or(before, <[u16]>::to_vec),
    };
    // 0x0055E642..0x0055E734: result 1 with an own beacon selected, results
    // 2 and 4 and an open composition update the first selected beacon;
    // Escape clears and sends its label instead.
    let own = beacons::own_selected(state);
    if (own && result == EditInput::Typed)
        || matches!(
            result,
            EditInput::Deleted | EditInput::Ended | EditInput::Overflowed(_)
        )
        || composition(state).is_some()
    {
        if key == EditKey::Escape {
            beacons::edit_beacon_message(state, None, BeaconTarget::Selected, true);
        } else {
            follow_label(state, Some(typed.clone()));
        }
    }
    // 0x0055E77B..0x0055E7AE: results 3 and 4 with an own beacon selected
    // send the buffer as its label, and no line.
    if matches!(result, EditInput::Sent(_) | EditInput::Overflowed(_)) && own {
        beacons::edit_beacon_message(state, Some(&typed), BeaconTarget::Selected, true);
        if matches!(result, EditInput::Sent(_)) {
            state.match_state.match_presentation.chat.target = ChatTarget::None;
        }
        return;
    }
    let presentation = &mut state.match_state.match_presentation;
    match result {
        EditInput::Ended if key == EditKey::Escape => presentation.chat.target = ChatTarget::None,
        EditInput::Sent(text) => {
            let target = state.match_state.match_presentation.chat.target;
            let whole_line = !state.match_state.match_presentation.chat.overflowed;
            // The line goes to its recipients like any other, so a `/pause`
            // or `/resume` line tells them who asked before it asks.
            send(state, target, &text);
            if let Some(want) = pause_command(&text).filter(|_| whole_line) {
                crate::app::input::pause_menu::chat_command(state, want);
            }
            // 0x0055F1BD: result 3 clears the target.
            state.match_state.match_presentation.chat.target = ChatTarget::None;
        }
        EditInput::Overflowed(chunk) => {
            state.match_state.match_presentation.chat.overflowed = true;
            let target = state.match_state.match_presentation.chat.target;
            send(state, target, &chunk);
        }
        _ => {}
    }
}

/// VERA: a typed line that is only `/pause` or `/resume`, ASCII
/// case-insensitively, also asks for the shared pause (true) or its end,
/// after it is sent as chat like any other line.
fn pause_command(line: &str) -> Option<bool> {
    let word = line.trim_matches(' ');
    if word.eq_ignore_ascii_case("/pause") {
        Some(true)
    } else if word.eq_ignore_ascii_case("/resume") {
        Some(false)
    } else {
        None
    }
}

/// VERA: a line this game sends to everyone by itself, sent and echoed like
/// a typed one: the caught-up line (`network_match::post_caught_up`).
pub(crate) fn send_to_all(state: &mut AppState, text: &str) {
    send(state, ChatTarget::All, text);
}

/// The send block `0x0055EDBF..0x0055F110` for results 3 and 4.
fn send(state: &mut AppState, target: ChatTarget, text: &str) {
    if !state.match_state.session_mode().is_network() {
        return;
    }
    let Some(local) = local_house(state) else {
        return;
    };
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
    let runtime = state.match_state.sim_runtime.runtime()?;
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
            peers: session
                .network_owner_sources(sim.interner().resolve(house))
                .into_iter()
                .filter(|source| connected.contains(source) && Some(*source) != local_source)
                .collect(),
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
#[derive(Debug, Clone)]
struct ChatHouse {
    /// House array index, the chat mask's index.
    index: usize,
    /// IsAlliedWith(local House, this House).
    allied: bool,
    /// The connected peers whose connections carry this House's name: one,
    /// or each other player sharing it (VERA shared control), so a line to
    /// the local House reaches its teammates.
    peers: Vec<usize>,
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
        .flat_map(|house| house.peers.iter().copied())
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
                .runtime()
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
    let units: Vec<u16> = text.encode_utf16().collect();
    crate::app::shell_wol::translation::received(
        state,
        &units,
        crate::app::shell_wol::translation::TranslatedLine::Match {
            name: name.to_owned(),
            color,
        },
    );
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
        .runtime()?
        .simulation
        .session()
        .current_house()
}

#[cfg(all(test, tests_app_input_a_c))]
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

    #[test]
    fn only_a_whole_pause_or_resume_line_is_a_command() {
        assert_eq!(pause_command("/pause"), Some(true));
        assert_eq!(pause_command(" /Resume "), Some(false));
        assert_eq!(pause_command("/pause please"), None);
        assert_eq!(pause_command("pause"), None);
    }

    fn houses() -> [ChatHouse; 3] {
        [
            // The local House, an allied peer and an enemy peer.
            ChatHouse {
                index: 0,
                allied: true,
                peers: Vec::new(),
            },
            ChatHouse {
                index: 1,
                allied: true,
                peers: vec![1],
            },
            ChatHouse {
                index: 2,
                allied: false,
                peers: vec![2],
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
    fn later_house_chat_and_beacons_survive_defeat_and_reset() {
        let mut chat = ChatState::default();
        let houses = [
            ChatHouse {
                index: 8,
                allied: false,
                peers: vec![7],
            },
            ChatHouse {
                index: 31,
                allied: true,
                peers: vec![29],
            },
        ];
        assert_eq!(
            route_houses(ChatTarget::Listeners, false, &chat, &houses),
            (vec![7, 29], false)
        );
        chat.set_listens(31, false);
        assert_eq!(
            route_houses(ChatTarget::Listeners, false, &chat, &houses),
            (vec![7], false)
        );
        chat.local_player_defeated();
        assert_eq!(
            route_houses(ChatTarget::Listeners, true, &chat, &houses),
            (vec![7, 29], false)
        );
        let mut beacons = crate::app::match_runtime::beacons::BeaconManager::default();
        assert_eq!(beacons.store(31, [10, 20, 0], None), Some(0));
        assert_eq!(
            beacons.iter().next().map(|(house, slot, _)| (house, slot)),
            Some((31, 0))
        );
        beacons = Default::default();
        assert_eq!(beacons.iter().count(), 0);
        assert!(beacons.can_place(31));
    }

    #[test]
    fn the_mask_starts_set_and_defeat_sets_it_again() {
        let mut chat = ChatState::default();
        assert!((0..8).all(|index| chat.listens(index)));
        chat.set_listens(2, false);
        assert!(!chat.listens(2));
        assert!(chat.listens(31), "VERA includes every House registry index");
        chat.local_player_defeated();
        assert!(chat.listens(2));
    }
}
