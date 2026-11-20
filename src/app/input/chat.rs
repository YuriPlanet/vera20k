//! In-battle multiplayer chat: the key pre-hook `0x0055E420` that Process_Command
//! (`0x0055DEE0`) calls first, the chat mask `0xA8D108[8]` that dialog 0x73's
//! chat boxes write, and the chat packet `0x0C`'s send and receive.
//!
//! The MessageList edit itself is `ui::messages`. A send queues
//! [`NetworkSendRequest::Chat`](crate::app::match_runtime::session::NetworkSendRequest),
//! which `lan_service`'s match-request drain sends as packet `0x0C`; a
//! received `0x0C` comes through `lan_service`'s global demux to
//! [`receive_packet`].
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
use crate::app::match_runtime::session::SessionMode;
use crate::sim::intern::InternedId;
use crate::sim::skirmish_cheats::SkirmishCheat;
use crate::ui::messages::{EditInput, EditKey};
use winit::event::MouseButton;
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// `[0xABCE18]`: who the open line goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ChatTarget {
    #[default]
    None,
    /// Local messages and cheat commands in an offline skirmish.
    Skirmish,
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
    /// Locks the kind of the full line when the editor starts emitting chunks.
    overflow: Option<LineKind>,
}

impl Default for ChatState {
    fn default() -> Self {
        Self {
            target: ChatTarget::None,
            muted: Default::default(),
            overflow: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Message,
    Command,
}

#[derive(Debug, PartialEq, Eq)]
enum SkirmishPost {
    Message(String),
    Command(String),
    CommandTooLong,
}

impl ChatState {
    /// The first chunk determines whether this whole input is chat or a slash
    /// command. An overflowing command cannot execute from a partial line.
    fn skirmish_post(&mut self, result: EditInput) -> Option<SkirmishPost> {
        let (line, more) = match result {
            EditInput::Sent(line) => (line, false),
            EditInput::Overflowed(line) => (line, true),
            EditInput::Ended => {
                self.target = ChatTarget::None;
                self.overflow = None;
                return None;
            }
            _ => return None,
        };
        let previous = self.overflow;
        let kind = previous.unwrap_or_else(|| {
            if line.trim_start().starts_with('/') {
                LineKind::Command
            } else {
                LineKind::Message
            }
        });
        self.overflow = more.then_some(kind);
        if !more {
            self.target = ChatTarget::None;
        }
        match kind {
            LineKind::Message => Some(SkirmishPost::Message(line)),
            LineKind::Command if more => None,
            LineKind::Command if previous.is_some() => Some(SkirmishPost::CommandTooLong),
            LineKind::Command => Some(SkirmishPost::Command(line)),
        }
    }

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

/// The plain chat keys in Skirmish. Modified keys remain available to hotkey bindings.
fn opens_skirmish_chat(key: &Key, modifiers: ModifiersState) -> bool {
    modifiers.is_empty() && matches!(key, Key::Named(NamedKey::Enter | NamedKey::Backspace))
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
        if !edge.state.is_pressed() || edge.repeat {
            return false;
        }
        let modifiers = state.match_state.input.hotkey_modifiers;
        if !state.match_state.session_mode().is_network() {
            if edge.logical == &Key::Named(NamedKey::Escape)
                && skirmish_input_available(state)
                && cancel_skirmish_spawn(state)
            {
                return true;
            }
            if opens_skirmish_chat(edge.logical, modifiers) && skirmish_input_available(state) {
                return open_edit(state, ChatTarget::Skirmish);
            }
            if edge.text == Some("/")
                && !modifiers.super_key()
                && state.persistence.options_profile.skirmish_cheats
                && skirmish_input_available(state)
            {
                let presentation = &mut state.match_state.match_presentation;
                if presentation.message_list.add_edit("", [1.0; 3], true) {
                    presentation.chat.target = ChatTarget::Skirmish;
                    presentation.chat.overflow = None;
                    cancel_skirmish_spawn(state);
                    apply_input(state, EditKey::Unit(u16::from(b'/')));
                    return true;
                }
            }
            return false;
        }
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
    if state.match_state.match_presentation.chat.target != ChatTarget::Skirmish
        && beacons::own_selected(state)
    {
        follow_label(state, None);
    }
}

/// The pre-hook's composition test `[0x00B7356C]` (`0x0055E671`): while an
/// input-method composition is open, the selected beacon's label follows
/// the line and the composition.
pub(crate) fn composition_changed(state: &mut AppState) {
    if state.match_state.match_presentation.chat.target != ChatTarget::Skirmish
        && state.match_state.match_presentation.message_list.editing()
        && composition(state).is_some()
    {
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
    // Android: the long press that opened this edit also carries the mapper's
    // right click, which would end it at once.
    #[cfg(target_os = "android")]
    if TOUCH_OPENED_AT.with(|at| {
        at.borrow()
            .is_some_and(|at| at.elapsed() < crate::util::time::Duration::from_millis(400))
    }) {
        return;
    }
    if state.match_state.match_presentation.message_list.editing() {
        apply_input(state, EditKey::RightClick);
    }
}

/// Android: when the lane's long press opened the edit, so the right click it
/// came with is dropped. Thread-local, like the rest of the input state.
#[cfg(target_os = "android")]
thread_local! {
    static TOUCH_OPENED_AT: std::cell::RefCell<Option<crate::util::time::Instant>> =
        const { std::cell::RefCell::new(None) };
}

/// The chat branch `0x0055E519..0x0055E5B4`: the prefix is `TXT_FROM` with
/// the player's House UI name, in the player's colour; overflow is on and
/// no sound plays. The cursor character 0 draws no caret.
/// Android: the message lane's long press opens the chat as Enter does: the
/// Skirmish target in an offline skirmish (which also cancels a pending spawn
/// order), the listeners otherwise. Returns whether the edit opened.
#[cfg(target_os = "android")]
pub(crate) fn open_touch_edit(state: &mut AppState) -> bool {
    if state.match_state.match_presentation.message_list.editing() {
        return false;
    }
    // Enter's own choice: plain Enter in a skirmish session is the Skirmish
    // chat (`opens_skirmish_chat`), so a long press must not pick a target the
    // keyboard would not, or a pending spawn order survives it.
    let target = if skirmish_input_available(state) {
        ChatTarget::Skirmish
    } else {
        ChatTarget::Listeners
    };
    let opened = open_edit(state, target);
    if opened {
        TOUCH_OPENED_AT.with(|at| *at.borrow_mut() = Some(crate::util::time::Instant::now()));
    }
    opened
}

/// Android: one Backspace in the open chat edit.
#[cfg(target_os = "android")]
pub(crate) fn backspace(state: &mut AppState) {
    if state.match_state.match_presentation.message_list.editing() {
        apply_input(state, EditKey::Delete);
    }
}

/// Android: a tap on the message lane while the edit is open sends it, as
/// Enter does; returns whether it did.
#[cfg(target_os = "android")]
pub(crate) fn send_touch(state: &mut AppState) -> bool {
    if !state.match_state.match_presentation.message_list.editing() {
        return false;
    }
    apply_input(state, EditKey::Enter);
    true
}

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
        presentation.chat.overflow = None;
        if target == ChatTarget::Skirmish {
            cancel_skirmish_spawn(state);
        }
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
    if presentation.chat.target == ChatTarget::Skirmish {
        match presentation.chat.skirmish_post(result) {
            Some(SkirmishPost::Message(text)) => {
                if let Some(local) = local_house(state)
                    && let Some(name) =
                        crate::app::presentation::house_ui::house_ui_name(state, local)
                {
                    let rgb = crate::app::input::messages::player_line_rgb(state, local);
                    crate::app::input::messages::post_chat_line(state, &name, &text, rgb);
                }
            }
            Some(SkirmishPost::Command(line)) => {
                let reply = skirmish_command(state, &line).unwrap_or_else(crate::i18n::tr);
                crate::app::input::messages::post_selection_navigation_text(state, &reply);
            }
            Some(SkirmishPost::CommandTooLong) => {
                crate::app::input::messages::post_selection_navigation_text(
                    state,
                    &crate::i18n::tr("VERA:CheatTooLong"),
                );
            }
            None => {}
        }
        return;
    }
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
            let whole_line = state.match_state.match_presentation.chat.overflow.is_none();
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
            state.match_state.match_presentation.chat.overflow = Some(LineKind::Message);
            let target = state.match_state.match_presentation.chat.target;
            send(state, target, &chunk);
        }
        _ => {}
    }
}

/// Skirmish chat follows the active local match, including its ordinary input locks.
fn skirmish_input_available(state: &AppState) -> bool {
    state.match_state.session_mode() == SessionMode::Skirmish
        && !state.match_state.match_diagnostics.is_replay_playback()
        && !state.match_state.frame_stopped
        && !state.match_state.paused()
        && !crate::app::input::campaign_input::is_locked(state)
        && !crate::app::App::house_outcome_wait_pending(state)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SpawnOrder {
    type_id: String,
    count: u16,
}

#[derive(Debug, PartialEq, Eq)]
enum SkirmishCommand {
    Help,
    Apply(SkirmishCheat),
    Place(SpawnOrder),
}

fn parse_skirmish_command(line: &str) -> Result<SkirmishCommand, &'static str> {
    const USAGE: &str = "VERA:CheatUsage";
    let mut words = line.split_ascii_whitespace();
    let name = words
        .next()
        .and_then(|word| word.strip_prefix('/'))
        .ok_or(USAGE)?;
    let command = if name.eq_ignore_ascii_case("help") {
        SkirmishCommand::Help
    } else if name.eq_ignore_ascii_case("spysat") {
        SkirmishCommand::Apply(SkirmishCheat::SpySat)
    } else if name.eq_ignore_ascii_case("money") {
        SkirmishCommand::Apply(SkirmishCheat::Money(
            words.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?,
        ))
    } else if name.eq_ignore_ascii_case("spawn") || name.eq_ignore_ascii_case("rhino") {
        let type_id = if name.eq_ignore_ascii_case("rhino") {
            "HTNK"
        } else {
            words.next().ok_or(USAGE)?
        };
        let count = words.next().and_then(|n| n.parse().ok()).ok_or(USAGE)?;
        SkirmishCommand::Place(SpawnOrder {
            type_id: type_id.to_owned(),
            count,
        })
    } else {
        return Err(USAGE);
    };
    if words.next().is_some() {
        return Err(USAGE);
    }
    Ok(command)
}

fn ready_skirmish_cheats(state: &mut AppState) -> Result<(), &'static str> {
    if !skirmish_input_available(state) {
        return Err("VERA:CheatUnavailable");
    }
    if !state.persistence.options_profile.skirmish_cheats {
        return Err("VERA:CheatDisabled");
    }
    crate::app::match_runtime::sim_tick::finish_threaded_frame(state);
    skirmish_input_available(state)
        .then_some(())
        .ok_or("VERA:CheatUnavailable")
}

fn skirmish_command(state: &mut AppState, line: &str) -> Result<String, &'static str> {
    ready_skirmish_cheats(state)?;
    match parse_skirmish_command(line)? {
        SkirmishCommand::Help => Ok(crate::i18n::tr("VERA:CheatHelp")),
        SkirmishCommand::Apply(command) => apply_skirmish_cheat(state, command),
        SkirmishCommand::Place(order) => {
            let rt = state
                .match_state
                .sim_runtime
                .home_runtime()
                .ok_or("VERA:CheatUnavailable")?;
            let unit = crate::sim::skirmish_cheats::skirmish_spawn_type(
                &rt.resources.rules,
                &order.type_id,
                order.count,
            )?;
            let prompt = crate::i18n::tr_fill(
                "VERA:CheatChooseSpawnCell",
                &[("count", &order.count), ("unit", &unit.id)],
            );
            // A button held while typing cannot confirm the new order.
            state.match_state.input.cancel_tactical_drag();
            state
                .match_state
                .match_presentation
                .in_game_gadgets
                .cancel_pointer_capture();
            state.match_state.input.skirmish_spawn = Some(order);
            Ok(prompt)
        }
    }
}

fn cancel_skirmish_spawn(state: &mut AppState) -> bool {
    if state.match_state.input.skirmish_spawn.take().is_none() {
        return false;
    }
    state.match_state.input.tactical_mouse.release();
    super::messages::post_selection_navigation_text(
        state,
        &crate::i18n::tr("VERA:CheatSpawnCancelled"),
    );
    true
}

/// Consume the whole placement gesture before gadgets, selection and unit orders.
/// A right drag still pans the view, and a right click cancels.
pub(crate) fn skirmish_spawn_mouse(
    state: &mut AppState,
    button: MouseButton,
    pressed: bool,
) -> bool {
    if state.match_state.input.skirmish_spawn.is_none() {
        return false;
    }
    let mouse = &mut state.match_state.input.tactical_mouse;
    match (button, pressed) {
        (MouseButton::Left, _) if mouse.right_held() => {}
        (MouseButton::Left, true) => {
            if super::camera::spawn_pointer_world_point(state).is_some() {
                state.match_state.input.tactical_mouse.begin_left_press();
            } else {
                super::messages::post_selection_navigation_text(
                    state,
                    &crate::i18n::tr("VERA:CheatPointAtMap"),
                );
            }
        }
        (MouseButton::Left, false) if mouse.end_left_press() => {
            let result = place_skirmish_spawn(state);
            if !matches!(result, Err("VERA:CheatPointAtMap")) {
                state.match_state.input.skirmish_spawn = None;
            }
            super::messages::post_selection_navigation_text(
                state,
                &result.unwrap_or_else(crate::i18n::tr),
            );
        }
        (MouseButton::Right, true) => {
            let cursor = state.cursor_position();
            state
                .match_state
                .input
                .tactical_mouse
                .begin_right_drag(cursor);
        }
        (MouseButton::Right, false) if mouse.end_right_press() == Some(true) => {
            cancel_skirmish_spawn(state);
        }
        (MouseButton::Left | MouseButton::Right, false) => {}
        _ => return false,
    }
    true
}

fn place_skirmish_spawn(state: &mut AppState) -> Result<String, &'static str> {
    use crate::app::match_runtime::sim_tick;
    // Preserve the point in the displayed camera before joining an outstanding tick.
    let (x, y) = super::camera::spawn_pointer_world_point(state).ok_or("VERA:CheatPointAtMap")?;
    ready_skirmish_cheats(state)?;
    let sim = &state
        .match_state
        .sim_runtime
        .home_runtime()
        .ok_or("VERA:CheatUnavailable")?
        .simulation;
    let terrain = sim_tick::tactical_cells(sim).ok_or("VERA:CheatPointAtMap")?;
    let cell = sim_tick::world_point_to_cell(x, y, Some(terrain));
    let order = state
        .match_state
        .input
        .skirmish_spawn
        .as_ref()
        .ok_or("VERA:CheatUnavailable")?;
    // The replay owns the confirmed order. Keep the cursor armed if the map
    // rejects the cell, so the player can click again without retyping.
    let command = SkirmishCheat::SpawnAt {
        type_id: order.type_id.clone(),
        count: order.count,
        cell,
    };
    apply_skirmish_cheat(state, command)
}

fn apply_skirmish_cheat(
    state: &mut AppState,
    command: SkirmishCheat,
) -> Result<String, &'static str> {
    use crate::app::match_runtime::{sim_host::SimWrite, sim_tick};
    use crate::i18n::{tr, tr_fill};
    use crate::sim::skirmish_cheats::CheatEffect;
    sim_tick::prepare_replay_recording(state);
    let Some(rt) = state.match_state.sim_runtime.write_sim(SimWrite::Command) else {
        return Err("VERA:CheatUnavailable");
    };
    let result = match state.match_state.match_diagnostics.replay_log.as_mut() {
        Some(log) => log.apply_skirmish_cheat(&mut rt.simulation, &rt.resources.rules, command),
        None => rt
            .simulation
            .apply_skirmish_cheat(&command, &rt.resources.rules),
    };
    Ok(match result? {
        CheatEffect::Money(amount) => tr_fill("VERA:CheatMoney", &[("amount", &amount)]),
        CheatEffect::SpySat => tr("VERA:CheatSpySat"),
        CheatEffect::Spawned {
            placed,
            requested,
            type_id,
        } => tr_fill(
            "VERA:CheatSpawn",
            &[
                ("placed", &placed),
                ("count", &requested),
                ("unit", &type_id),
            ],
        ),
    })
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
        ChatTarget::None | ChatTarget::Skirmish => false,
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

/// Receive `0x0048D8DD..0x0048D9B8`: the line under the packet's own sender
/// name in its colour, then one more IncomingMessage (`0x0048D995`) on top
/// of Add_Message's own. No mask, alliance or defeat filter applies.
/// RESIDUAL: the `-MESSAGES` game-name hash filter (`[0xA8D60D]`, off by
/// default) is not ported; the switch is accepted and ignored.
pub(crate) fn receive_packet(state: &mut AppState, name: &str, color: i32, text: &str) {
    let rgb = crate::app::input::messages::packet_player_line_rgb(state, color);
    crate::app::input::messages::post_chat_line(state, name, text, rgb);
    let sound = state
        .rules()
        .and_then(|rules| rules.general.incoming_message_sound().map(str::to_owned));
    crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
    let units: Vec<u16> = text.encode_utf16().collect();
    crate::app::shell_wol::translation::received(
        state,
        &units,
        false,
        crate::app::shell_wol::translation::TranslatedLine::Match {
            name: name.to_owned(),
            color,
        },
    );
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
    fn skirmish_chat_opens_on_plain_enter_and_backspace_and_accepts_chat_or_commands() {
        for key in [NamedKey::Enter, NamedKey::Backspace] {
            assert!(opens_skirmish_chat(
                &Key::Named(key),
                ModifiersState::empty()
            ));
            assert!(!opens_skirmish_chat(
                &Key::Named(key),
                ModifiersState::CONTROL
            ));
        }
        let mut chat = ChatState::default();
        let mut list = crate::ui::messages::MessageList::scenario();
        list.set_width(640);
        for (text, expected) in [
            ("hello", SkirmishPost::Message("hello ".into())),
            ("/help", SkirmishPost::Command("/help ".into())),
        ] {
            chat.target = ChatTarget::Skirmish;
            assert!(list.add_edit("Player: ", [1.0; 3], true));
            for unit in text.encode_utf16() {
                assert_eq!(
                    chat.skirmish_post(list.input(EditKey::Unit(unit), &|s| s.len() as i32)),
                    None
                );
            }
            assert_eq!(
                chat.skirmish_post(list.input(EditKey::Enter, &|s| s.len() as i32)),
                Some(expected)
            );
            assert_eq!(chat.target, ChatTarget::None);
        }
    }

    #[test]
    fn wrapped_skirmish_chat_cannot_turn_into_a_cheat() {
        let mut chat = ChatState::default();
        let message = "Here is the command ";
        assert_eq!(
            chat.skirmish_post(EditInput::Overflowed(message.into())),
            Some(SkirmishPost::Message(message.into()))
        );
        assert_eq!(
            chat.skirmish_post(EditInput::Sent("/money 10000".into())),
            Some(SkirmishPost::Message("/money 10000".into()))
        );
        assert_eq!(
            chat.skirmish_post(EditInput::Overflowed("/spawn ".into())),
            None
        );
        assert_eq!(
            chat.skirmish_post(EditInput::Sent("/money 10000".into())),
            Some(SkirmishPost::CommandTooLong)
        );
    }

    #[test]
    fn spawn_command_accepts_an_order_before_picking_a_cell() {
        assert_eq!(
            parse_skirmish_command("/spawn ZEP 100"),
            Ok(SkirmishCommand::Place(SpawnOrder {
                type_id: "ZEP".into(),
                count: 100
            }))
        );
    }

    #[test]
    fn the_rhino_shortcut_and_rules_id_reach_the_same_command() {
        assert_eq!(
            parse_skirmish_command("/RhInO 200 "),
            parse_skirmish_command("/spawn HTNK 200")
        );
        assert!(parse_skirmish_command("/money 10000 extra").is_err());
        assert!(parse_skirmish_command("/rhino -1").is_err());
        assert_eq!(parse_skirmish_command("/help"), Ok(SkirmishCommand::Help));
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
