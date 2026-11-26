//! Local client state: actions (appendix 5.4), the camera and local
//! settings references (appendix 7.10), hotkeys, selection changes,
//! `Notify` and `SimEvent` delivery (appendix 4.6), and the two ways into
//! the command stream (`client_send_order`, `client_send_message`; RFC 10
//! "Communication", 6i).
//!
//! Effects that need the app (sound, camera, selection, flash, messages,
//! commands) are queued in [`LocalEffects`]; the runtime hands them to
//! [`apply`] after the call. Checks that need the world (references,
//! visibility, the order table) run during the call against the
//! [`ClientView`], so a call answers its real status.
//!
//! Limits (RFC 15.4), counted in simulation frames (`client_frame`), since
//! what they protect is the command stream:
//! - orders: each unit at most once per 15 frames, house-wide orders at
//!   most 4 per 15 frames; over either the order is `Refused` and nothing
//!   enters the stream;
//! - messages: 96 bytes; per source 2 per frame and 16 in any 15 frames.
//!
//! Events wait per source until [`LocalState::take_events`] takes them, at
//! most [`CLIENT_FRAME_EVENTS`] a frame together with the frame's pointer
//! events; at most [`MAX_PENDING_EVENTS`] wait, newer ones beyond that are
//! dropped (a module that stopped taking them must not grow memory).

use std::collections::{BTreeSet, HashMap, VecDeque};

use vera20k_wasm_ext::client::{
    CLIENT_CAMERA, CLIENT_CURSOR, CLIENT_FRAME_EVENTS, CLIENT_SETTINGS, REF_KIND_CLIENT_GLOBAL,
    client_action, client_event,
};
use vera20k_wasm_ext::{ActionParams, ClientEvent, Invalid, Order, Source, Status, property_name};

use super::Viewer;
use super::reads::{self, ClientView};
use crate::app::AppState;
use crate::sim::command::Command;
use crate::sim::ext_modules::{
    ClientOutbox, check_client_order, client_cell, client_cell_ref, client_object,
};

/// Frames of the order and message windows (RFC 15.4).
const RATE_WINDOW: u32 = 15;
/// House-wide orders per window.
const HOUSE_ORDERS_PER_WINDOW: usize = 4;
/// Message size (RFC 15.4).
pub(crate) const MAX_MESSAGE_BYTES: usize = 96;
/// Messages per source per frame.
const MESSAGES_PER_FRAME: usize = 2;
/// Messages per source per window.
const MESSAGES_PER_WINDOW: usize = 16;
/// Hotkeys one module may register.
pub(crate) const MAX_HOTKEYS: usize = 256;
/// Hotkey name length in bytes.
const MAX_HOTKEY_NAME: usize = 64;
/// Events waiting for one source's module (the delivery backlog of 15.4).
pub(crate) const MAX_PENDING_EVENTS: usize = 32_768;
/// `EventRecord` size and the offset of its `other` field (appendix
/// "Fixed layouts").
const EVENT_RECORD_SIZE: usize = 120;
const RECORD_OTHER: usize = 32;

/// Modifier bits of an encoded key, as `KEYBOARDMD.INI` and the game's
/// bindings encode them (`app::input::hotkeys`): Win32 virtual key in the
/// low byte.
const SHIFT_BIT: u16 = 0x100;
const CTRL_BIT: u16 = 0x200;
const ALT_BIT: u16 = 0x400;
/// Keys the game always owns: Backspace, Tab, Enter, Esc (Options and
/// `Dismiss`), Space and Delete never take a binding (the key encoder
/// clears them), and the arrows scroll the camera.
const GAME_OWNED_KEYS: [u16; 10] = [0x08, 0x09, 0x0D, 0x1B, 0x20, 0x2E, 0x25, 0x26, 0x27, 0x28];

/// One local effect, applied by [`apply`] in order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LocalEffect {
    /// `PlaySound`: a SOUNDMD index, at a cell or not positioned.
    Sound {
        sound: i32,
        cell: Option<(u16, u16)>,
    },
    /// `PlaySpeech`: an EVA (`[DialogList]`) event name.
    Speech { event: String },
    /// `CenterCamera` (`velocity` as trigger action 48) or `JumpCamera`
    /// (`None`, as action 112).
    Camera {
        cell: (u16, u16),
        velocity: Option<i32>,
    },
    /// `Select`: objects for the local selection, added or replacing.
    Select { objects: Vec<u64>, additive: bool },
    /// `FlashObject`.
    Flash { object: u64, frames: i32 },
    /// `ShowMessage`: CSF entry name or text, and its colour.
    Message { text: String, rgb: Option<[f32; 3]> },
}

/// What the app applies after a module call.
#[derive(Default)]
pub(crate) struct LocalEffects {
    /// Commands for the local player's command stream, in order.
    pub commands: Vec<Command>,
    /// Local effects, in order.
    pub effects: Vec<LocalEffect>,
}

/// Local facts the camera and settings references read.
pub(crate) struct LocalInputs {
    /// The cell at the camera's centre.
    pub camera_cell: Option<(u16, u16)>,
}

/// One registered hotkey (`RegisterHotkey`).
struct Hotkey {
    name: String,
    /// The encoded key; `None` when its default key belongs to the game or
    /// names no key.
    key: Option<u16>,
}

/// One source's message window.
#[derive(Default)]
struct MessageWindow {
    /// The frames of the messages sent in the current window.
    sent: VecDeque<u32>,
}

/// Per-match local client state.
pub(crate) struct LocalState {
    /// Each source's hotkeys; the ID is the index.
    hotkeys: [Vec<Hotkey>; 2],
    /// Every encoded key a game command is bound to, the always-owned keys
    /// included ([`Self::set_game_keys`]).
    game_keys: BTreeSet<u16>,
    /// The local selection last observed, sorted.
    selection: Vec<u64>,
    /// Events waiting for each source's module.
    pending: [VecDeque<ClientEvent>; 2],
    /// The last frame each unit got a client order in.
    unit_orders: HashMap<u64, u32>,
    /// The frames of the house-wide client orders of the current window.
    house_orders: VecDeque<u32>,
    messages: [MessageWindow; 2],
    /// A replay plays: its command stream is the recording's, so orders and
    /// messages are `Refused` ([`Self::set_playback`]).
    playback: bool,
}

impl LocalState {
    pub(crate) fn new() -> Self {
        Self {
            hotkeys: [Vec::new(), Vec::new()],
            game_keys: GAME_OWNED_KEYS.into_iter().collect(),
            selection: Vec::new(),
            pending: [VecDeque::new(), VecDeque::new()],
            unit_orders: HashMap::new(),
            house_orders: VecDeque::new(),
            messages: [MessageWindow::default(), MessageWindow::default()],
            playback: false,
        }
    }

    /// Whether a replay plays (set by the runtime each render frame).
    pub(crate) fn set_playback(&mut self, playback: bool) {
        self.playback = playback;
    }

    /// The keys the game's bindings use (`HotkeyBindings::bound_keys`):
    /// set when the match starts and again when the player rebinds. Game
    /// hotkeys always win over module hotkeys (RFC 10).
    pub(crate) fn set_game_keys(&mut self, keys: impl IntoIterator<Item = u16>) {
        self.game_keys = GAME_OWNED_KEYS.into_iter().chain(keys).collect();
    }

    /// `client_action` (appendix 5.4).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn action(
        &mut self,
        effects: &mut LocalEffects,
        view: &ClientView<'_>,
        source: Source,
        kind: i32,
        target: i64,
        params: &ActionParams,
        text: &str,
        extra: &[u8],
    ) -> Result<Status, Invalid> {
        let sim = &view.runtime.simulation;
        let cell = |reference| client_cell(sim, reference);
        Ok(match kind {
            client_action::PLAY_SOUND => {
                if params.id < 0 {
                    return Ok(Status::NotFound);
                }
                let cell = match target {
                    0 => None,
                    _ => match cell(target)? {
                        Ok(cell) => Some(cell),
                        Err(status) => return Ok(status),
                    },
                };
                effects.effects.push(LocalEffect::Sound {
                    sound: params.id,
                    cell,
                });
                Status::Ok
            }
            client_action::PLAY_SPEECH => {
                if text.is_empty() {
                    return Ok(Status::NotFound);
                }
                effects.effects.push(LocalEffect::Speech {
                    event: text.to_owned(),
                });
                Status::Ok
            }
            client_action::CENTER_CAMERA | client_action::JUMP_CAMERA => {
                let cell = match cell(target)? {
                    Ok(cell) => cell,
                    Err(status) => return Ok(status),
                };
                let velocity = if kind == client_action::CENTER_CAMERA {
                    Some(i32::try_from(params.value).map_err(|_| {
                        Invalid(format!("camera speed {} is out of range", params.value))
                    })?)
                } else {
                    None
                };
                effects.effects.push(LocalEffect::Camera { cell, velocity });
                Status::Ok
            }
            client_action::HOME_CAMERA => Status::Unsupported,
            client_action::SELECT => {
                if extra.len() % 8 != 0 {
                    return Err(Invalid(format!(
                        "Select: {} bytes are not whole handles",
                        extra.len()
                    )));
                }
                let mut objects = Vec::with_capacity(extra.len() / 8);
                for chunk in extra.chunks_exact(8) {
                    let handle = i64::from_le_bytes(chunk.try_into().unwrap_or_default());
                    let id = client_object(handle)?;
                    // What this player cannot see, it cannot pick; the
                    // selection's own admission decides the rest.
                    if sim.entities().get(id).is_some() && reads::sees_object(view, handle) {
                        objects.push(id);
                    }
                }
                effects.effects.push(LocalEffect::Select {
                    objects,
                    additive: params.value == 1,
                });
                Status::Ok
            }
            client_action::FLASH_OBJECT => {
                let id = client_object(target)?;
                let frames = i32::try_from(params.value)
                    .ok()
                    .filter(|frames| *frames >= 0)
                    .ok_or_else(|| Invalid(format!("flash of {} frames", params.value)))?;
                if sim.entities().get(id).is_none() || !reads::sees_object(view, target) {
                    return Ok(Status::Gone);
                }
                effects
                    .effects
                    .push(LocalEffect::Flash { object: id, frames });
                Status::Ok
            }
            client_action::SHOW_MESSAGE => {
                if text.is_empty() {
                    return Ok(Status::Refused);
                }
                effects.effects.push(LocalEffect::Message {
                    text: text.to_owned(),
                    rgb: message_rgb(params.value)?,
                });
                Status::Ok
            }
            client_action::REGISTER_HOTKEY => {
                let key = std::str::from_utf8(extra)
                    .map_err(|_| Invalid("RegisterHotkey: the key is not UTF-8".into()))?;
                self.register_hotkey(source, text, key)?
            }
            _ => return Err(Invalid(format!("no client action {kind}"))),
        })
    }

    /// `RegisterHotkey`: registers `name` (again: rebinds it) with its
    /// default `key` text. A key the game owns, or one no key name
    /// matches, registers the hotkey without a key and answers `Refused`;
    /// an empty key registers it without a key and answers `Ok`.
    fn register_hotkey(
        &mut self,
        source: Source,
        name: &str,
        key: &str,
    ) -> Result<Status, Invalid> {
        if name.is_empty() || name.len() > MAX_HOTKEY_NAME {
            return Err(Invalid(format!("hotkey name {name:?}")));
        }
        let (key, status) = match key.trim() {
            "" => (None, Status::Ok),
            text => match parse_key(text) {
                Some(key) if !self.game_owns(key) => (Some(key), Status::Ok),
                _ => (None, Status::Refused),
            },
        };
        let hotkeys = &mut self.hotkeys[source as usize];
        match hotkeys
            .iter()
            .position(|hotkey| hotkey.name.eq_ignore_ascii_case(name))
        {
            Some(index) => hotkeys[index].key = key,
            None if hotkeys.len() >= MAX_HOTKEYS => return Ok(Status::Refused),
            None => hotkeys.push(Hotkey {
                name: name.to_owned(),
                key,
            }),
        }
        Ok(status)
    }

    fn game_owns(&self, key: u16) -> bool {
        self.game_keys.contains(&key) || GAME_OWNED_KEYS.contains(&(key & 0xFF))
    }

    /// `client_get` on the camera or local settings (kind 6, payloads 1 and
    /// 2). The cursor (payload 3) is the tree's; asked here it answers
    /// `Unsupported`.
    pub(crate) fn global_get(
        &self,
        inputs: &LocalInputs,
        viewer: Viewer,
        reference: i64,
        prop: i32,
    ) -> Result<(i64, Status), Invalid> {
        let name = property_name(prop).ok_or_else(|| Invalid(format!("no property {prop}")))?;
        Ok(match (client_global(reference)?, name) {
            (CLIENT_CAMERA, "Cell") => match inputs.camera_cell {
                Some((x, y)) => (client_cell_ref(x, y), Status::Ok),
                None => (0, Status::Ok),
            },
            (CLIENT_SETTINGS, "Viewer") => {
                (if viewer.spectator { 0 } else { viewer.house }, Status::Ok)
            }
            (CLIENT_SETTINGS, "IsSpectator") => (i64::from(viewer.spectator), Status::Ok),
            // Camera `X`/`Y` are `later`; other local settings are not open.
            _ => (0, Status::Unsupported),
        })
    }

    /// `client_set` on the camera or local settings: nothing there is
    /// writable in version 1 (the camera moves with `JumpCamera`). Read-only
    /// properties answer `Refused`, `later` ones `Unsupported`.
    pub(crate) fn global_set(
        &mut self,
        _effects: &mut LocalEffects,
        reference: i64,
        prop: i32,
        _value: i64,
    ) -> Result<Status, Invalid> {
        let name = property_name(prop).ok_or_else(|| Invalid(format!("no property {prop}")))?;
        Ok(match (client_global(reference)?, name) {
            (CLIENT_CAMERA, "Cell") | (CLIENT_SETTINGS, "Viewer" | "IsSpectator") => {
                Status::Refused
            }
            _ => Status::Unsupported,
        })
    }

    /// `client_lookup(Hotkey, name)`: this source's hotkey, ASCII
    /// case-insensitive.
    pub(crate) fn hotkey_id(&self, source: Source, name: &str) -> (i32, Status) {
        match self.hotkeys[source as usize]
            .iter()
            .position(|hotkey| hotkey.name.eq_ignore_ascii_case(name))
        {
            Some(index) => (index as i32, Status::Ok),
            None => (-1, Status::NotFound),
        }
    }

    /// A key the game did not use (game hotkeys win), as key text like
    /// `Ctrl+Alt+K` ([`key_text`] makes it from an encoded key). true: a
    /// module hotkey fired; each source's hotkeys on the key get a `Hotkey`
    /// event.
    pub(crate) fn key_down(&mut self, key: &str) -> bool {
        let Some(key) = parse_key(key) else {
            return false;
        };
        if self.game_owns(key) {
            return false;
        }
        let mut fired = false;
        for source in [Source::Mod, Source::Map] {
            let ids: Vec<i32> = self.hotkeys[source as usize]
                .iter()
                .enumerate()
                .filter(|(_, hotkey)| hotkey.key == Some(key))
                .map(|(id, _)| id as i32)
                .collect();
            for id in ids {
                fired = true;
                self.push(
                    source,
                    ClientEvent {
                        kind: client_event::HOTKEY,
                        source: Source::Mod,
                        reference: 0,
                        id,
                        value: 0,
                        data: Vec::new(),
                    },
                );
            }
        }
        fired
    }

    /// The local selection this render frame; a change of its members (not
    /// of their order) raises `SelectionChanged` for both modules.
    pub(crate) fn observe_selection(&mut self, selected: &[u64]) {
        let mut sorted = selected.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted == self.selection {
            return;
        }
        self.selection = sorted;
        self.push_both(simple_event(client_event::SELECTION_CHANGED));
    }

    /// The frame's simulation outbox: the notifications and simulation
    /// events for this player ([`outbox_events`]), for both modules.
    pub(crate) fn deliver_outbox(&mut self, outbox: &ClientOutbox, view: &ClientView<'_>) {
        for event in outbox_events(outbox, view.viewer, |handle| {
            reads::sees_object(view, handle)
        }) {
            self.push_both(event);
        }
    }

    /// `client_send_order` (RFC 10, 6i): `Refused` for spectators, unless
    /// `AutomationAllowed`, and over the limits; otherwise checked like
    /// `order` ([`check_client_order`]: the command table, only this
    /// player's units) and queued as the player commands it becomes.
    pub(crate) fn send_order(
        &mut self,
        effects: &mut LocalEffects,
        view: &ClientView<'_>,
        automation_allowed: bool,
        order: &Order,
    ) -> Result<Status, Invalid> {
        if self.playback || view.viewer.spectator || view.viewer.house == 0 || !automation_allowed {
            return Ok(Status::Refused);
        }
        let sim = &view.runtime.simulation;
        let rules = &view.runtime.resources.rules;
        let checked = match check_client_order(sim, rules, view.viewer.house, order)? {
            Ok(checked) => checked,
            Err(status) => return Ok(status),
        };
        let now = sim.session().binary_frame();
        if !self.order_allowed(now, checked.house_wide(), checked.units()) {
            return Ok(Status::Refused);
        }
        let Some(commands) = checked.commands(sim, rules) else {
            return Ok(Status::NotFound);
        };
        self.record_order(now, checked.house_wide(), checked.units());
        effects.commands.extend(commands);
        Ok(Status::Ok)
    }

    /// Whether an order at frame `now` stays within RFC 15.4's client order
    /// limits; forgets what fell out of the window.
    fn order_allowed(&mut self, now: u32, house_wide: bool, units: &[u64]) -> bool {
        // A frame counter that went back (a loaded game) starts over.
        self.unit_orders
            .retain(|_, frame| *frame <= now && now - *frame < RATE_WINDOW);
        prune_window(&mut self.house_orders, now);
        if house_wide {
            self.house_orders.len() < HOUSE_ORDERS_PER_WINDOW
        } else {
            units
                .iter()
                .all(|unit| !self.unit_orders.contains_key(unit))
        }
    }

    fn record_order(&mut self, now: u32, house_wide: bool, units: &[u64]) {
        if house_wide {
            self.house_orders.push_back(now);
        }
        for &unit in units {
            self.unit_orders.insert(unit, now);
        }
    }

    /// `client_send_message` (RFC 10 "Communication", 11): `Refused` for
    /// spectators, over 96 bytes, and over 2 a frame or 16 in 15 frames for
    /// this source; otherwise `Command::ModuleMessage` for the command
    /// stream (the engine fills in the sender house).
    pub(crate) fn send_message(
        &mut self,
        effects: &mut LocalEffects,
        view: &ClientView<'_>,
        source: Source,
        data: &[u8],
    ) -> Result<Status, Invalid> {
        if self.playback {
            return Ok(Status::Refused);
        }
        let now = view.runtime.simulation.session().binary_frame();
        Ok(self.message(effects, view.viewer, now, source, data))
    }

    /// [`Self::send_message`] at frame `now`.
    fn message(
        &mut self,
        effects: &mut LocalEffects,
        viewer: Viewer,
        now: u32,
        source: Source,
        data: &[u8],
    ) -> Status {
        if viewer.spectator || viewer.house == 0 || data.len() > MAX_MESSAGE_BYTES {
            return Status::Refused;
        }
        let window = &mut self.messages[source as usize];
        prune_window(&mut window.sent, now);
        let this_frame = window.sent.iter().filter(|frame| **frame == now).count();
        if this_frame >= MESSAGES_PER_FRAME || window.sent.len() >= MESSAGES_PER_WINDOW {
            return Status::Refused;
        }
        window.sent.push_back(now);
        effects.commands.push(Command::ModuleMessage {
            source: source as u8,
            data: data.to_vec(),
        });
        Status::Ok
    }

    /// The frame's `Hotkey`, `SelectionChanged`, `Notify` and `SimEvent`
    /// events for `source`'s module, after what `out` already holds, up to
    /// [`CLIENT_FRAME_EVENTS`] in all; the rest wait for the next frame.
    pub(crate) fn take_events(&mut self, source: Source, out: &mut Vec<ClientEvent>) {
        let pending = &mut self.pending[source as usize];
        let room = CLIENT_FRAME_EVENTS
            .saturating_sub(out.len())
            .min(pending.len());
        out.extend(pending.drain(..room));
    }

    fn push(&mut self, source: Source, event: ClientEvent) {
        let pending = &mut self.pending[source as usize];
        if pending.len() >= MAX_PENDING_EVENTS {
            log::warn!("ui.wasm {source:?}: client event backlog full, event dropped");
            return;
        }
        pending.push_back(event);
    }

    fn push_both(&mut self, event: ClientEvent) {
        self.push(Source::Mod, event.clone());
        self.push(Source::Map, event);
    }
}

/// Drops the frames that left the window ending at `now`; all of them when
/// the frame counter went back.
fn prune_window(frames: &mut VecDeque<u32>, now: u32) {
    if frames.back().is_some_and(|last| *last > now) {
        frames.clear();
    }
    while frames
        .front()
        .is_some_and(|first| now - *first >= RATE_WINDOW)
    {
        frames.pop_front();
    }
}

/// A client global reference's payload (kind 6): camera or settings, or
/// the cursor; anything else is invalid.
fn client_global(reference: i64) -> Result<i64, Invalid> {
    let payload = reference & ((1 << 48) - 1);
    if reference >> 48 != REF_KIND_CLIENT_GLOBAL
        || !matches!(payload, CLIENT_CAMERA | CLIENT_SETTINGS | CLIENT_CURSOR)
    {
        return Err(Invalid(format!(
            "{reference:#x} is not a client global reference"
        )));
    }
    Ok(payload)
}

/// `ShowMessage`'s colour: `0xRRGGBB` like `TintColor` (appendix 7.2);
/// 0 keeps the trigger message colour of the player's side.
fn message_rgb(value: i64) -> Result<Option<[f32; 3]>, Invalid> {
    match value {
        0 => Ok(None),
        1..=0xFF_FFFF => Ok(Some(
            [(value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF].map(|c| c as f32 / 255.0),
        )),
        _ => Err(Invalid(format!("message colour {value:#x}"))),
    }
}

/// An event with no reference, ID or data.
fn simple_event(kind: i32) -> ClientEvent {
    ClientEvent {
        kind,
        source: Source::Mod,
        reference: 0,
        id: -1,
        value: 0,
        data: Vec::new(),
    }
}

/// The outbox's events for `viewer` (RFC 10, "Notifications" and
/// "Simulation events"; appendix 4.6), notifications first:
/// - `Notify`: those sent to the viewer's house; a spectator gets those
///   sent to spectators (`to` 0). A replay viewer reads as the player whose
///   view it shows, or as a spectator: whatever its [`Viewer`] says.
/// - `SimEvent`: those of the viewer's house (spectators have none), with
///   `other` zeroed when `sees(other)` says this player cannot see it.
fn outbox_events(
    outbox: &ClientOutbox,
    viewer: Viewer,
    sees: impl Fn(i64) -> bool,
) -> Vec<ClientEvent> {
    let notify_to = if viewer.spectator { 0 } else { viewer.house };
    let notifies = outbox
        .notifies
        .iter()
        .filter(|notify| notify.to == notify_to && (viewer.spectator || viewer.house != 0))
        .map(|notify| ClientEvent {
            kind: client_event::NOTIFY,
            source: notify.source,
            reference: 0,
            id: notify.id,
            value: 0,
            data: notify.data.clone(),
        });
    let sim_events = outbox
        .sim_events
        .iter()
        .filter(|event| !viewer.spectator && viewer.house != 0 && event.house == viewer.house)
        .map(|event| {
            let mut record = event.record.to_vec();
            debug_assert_eq!(record.len(), EVENT_RECORD_SIZE);
            let field = RECORD_OTHER..RECORD_OTHER + 8;
            let other = i64::from_le_bytes(record[field.clone()].try_into().unwrap_or_default());
            if other != 0 && !sees(other) {
                record[field].fill(0);
            }
            ClientEvent {
                data: record,
                ..simple_event(client_event::SIM_EVENT)
            }
        });
    notifies.chain(sim_events).collect()
}

/// Key names (case-insensitive) and their Win32 virtual keys; the first
/// name of a key is the one [`key_text`] writes. Letters, digits and
/// `F1`..`F24` are handled apart.
const KEY_NAMES: &[(&str, u16)] = &[
    ("Backspace", 0x08),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Return", 0x0D),
    ("Pause", 0x13),
    ("Esc", 0x1B),
    ("Escape", 0x1B),
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PgUp", 0x21),
    ("PageDown", 0x22),
    ("PgDn", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2D),
    ("Ins", 0x2D),
    ("Delete", 0x2E),
    ("Del", 0x2E),
    ("NumMultiply", 0x6A),
    ("NumAdd", 0x6B),
    ("NumSubtract", 0x6D),
    ("NumDecimal", 0x6E),
    ("NumDivide", 0x6F),
    (";", 0xBA),
    ("=", 0xBB),
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("`", 0xC0),
    ("[", 0xDB),
    ("\\", 0xDC),
    ("]", 0xDD),
    ("'", 0xDE),
];

/// Parses key text: modifiers (`Ctrl` or `Control`, `Alt`, `Shift`, any
/// order, each once) joined to a key name by `+`, ASCII case-insensitive,
/// e.g. `Ctrl+Alt+K`, `Shift+F5`, `Num7`. Returns the encoded key (virtual
/// key | modifier bits); `None` for text that names no key.
pub(crate) fn parse_key(text: &str) -> Option<u16> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last()?;
    let mut bits = 0u16;
    for modifier in modifiers {
        let bit = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => CTRL_BIT,
            "alt" => ALT_BIT,
            "shift" => SHIFT_BIT,
            _ => return None,
        };
        if bits & bit != 0 {
            return None;
        }
        bits |= bit;
    }
    Some(bits | virtual_key(key)?)
}

/// A key name's virtual key.
fn virtual_key(name: &str) -> Option<u16> {
    let upper = name.to_ascii_uppercase();
    if let [byte] = upper.as_bytes()
        && byte.is_ascii_alphanumeric()
    {
        return Some(u16::from(*byte));
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&number)
        && !upper[1..].starts_with('0')
    {
        return Some(0x6F + number);
    }
    for prefix in ["NUMPAD", "NUM"] {
        if let Some(rest) = upper.strip_prefix(prefix)
            && let [digit @ b'0'..=b'9'] = rest.as_bytes()
        {
            return Some(0x60 + u16::from(digit - b'0'));
        }
    }
    KEY_NAMES
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, vk)| *vk)
}

/// The canonical text of an encoded key (`Ctrl+Alt+Shift+K`), what
/// [`LocalState::key_down`] takes; `None` for a key without a name here.
pub(crate) fn key_text(encoded: u16) -> Option<String> {
    let vk = encoded & 0xFF;
    let name = match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => KEY_NAMES
            .iter()
            .find(|(_, key)| *key == vk)
            .map(|(name, _)| (*name).to_owned())?,
    };
    let mut text = String::new();
    for (bit, modifier) in [
        (CTRL_BIT, "Ctrl+"),
        (ALT_BIT, "Alt+"),
        (SHIFT_BIT, "Shift+"),
    ] {
        if encoded & bit != 0 {
            text.push_str(modifier);
        }
    }
    text.push_str(&name);
    Some(text)
}

/// Applies a call's queued effects to the app: local effects first, in
/// order, then the commands into the local player's command stream, as the
/// player's own input would send them (`input::commands`). A command the
/// stream does not take (replay playback, locked input) is dropped.
pub(crate) fn apply(effects: LocalEffects, state: &mut AppState) {
    use crate::app::match_runtime::sim_host::SimWrite;
    for effect in effects.effects {
        match effect {
            LocalEffect::Sound { sound, cell } => {
                let target = match cell {
                    None => crate::sim::world::TriggerSoundTarget::Centered,
                    Some((x, y)) => {
                        let Some([x, y, z]) = cell_coord(state, x, y) else {
                            continue;
                        };
                        crate::sim::world::TriggerSoundTarget::Coordinate {
                            coordinate: crate::sim::components::DriveCoord { x, y, z },
                            flags: 0,
                        }
                    }
                };
                state.match_state.match_audio.sound_events.push(
                    crate::audio::events::GameSoundEvent::TriggerAudio {
                        reference: crate::rules::sound_ini::SoundReference::Index(sound),
                        target,
                    },
                );
            }
            LocalEffect::Speech { event } => {
                crate::app::input::dispatch::push_local_eva(&mut state.match_state.match_audio, &event);
            }
            LocalEffect::Camera { cell, velocity } => {
                if let Some(coord) = cell_coord(state, cell.0, cell.1) {
                    crate::app::input::camera::apply_trigger_camera(
                        &state.platform,
                        &state.renderer,
                        &state.frontend.screen,
                        &mut state.match_state,
                        &state.persistence,
                        coord,
                        velocity,
                    );
                }
            }
            LocalEffect::Select { objects, additive } => {
                crate::app::input::dispatch::queue_selection_snapshot_command(
                    &state.platform,
                    &state.renderer,
                    &state.diag,
                    &state.frontend,
                    &mut state.match_state,
                    &state.process_assets,
                    &mut state.audio.sfx_player,
                    &state.persistence,
                    objects,
                    additive,
                );
            }
            LocalEffect::Flash { object, frames } => {
                if let Some(runtime) = state.match_state.sim_runtime.write_sim(SimWrite::Command) {
                    runtime.simulation.flash_object_locally(object, frames);
                }
            }
            LocalEffect::Message { text, rgb } => {
                crate::app::input::messages::post_module_message(state, &text, rgb);
            }
        }
    }
    if effects.commands.is_empty() {
        return;
    }
    let Some(owner) = crate::app::input::commands::preferred_command_owner(&state.match_state.sim_runtime) else {
        return;
    };
    for command in effects.commands {
        if crate::app::input::commands::try_schedule_command(&mut state.match_state, &owner, command).is_none() {
            log::debug!("ui.wasm: a module's command did not enter the command stream");
        }
    }
}

/// A cell's centre as trigger actions 48 and 112 resolve it (ground, or a
/// bridge deck): `Simulation::trigger_cell_coord`.
fn cell_coord(state: &AppState, x: u16, y: u16) -> Option<[i32; 3]> {
    let runtime = state.match_state.sim_runtime.runtime()?;
    let cell = (i16::try_from(x).ok()?, i16::try_from(y).ok()?);
    runtime.simulation.trigger_cell_coord(cell)
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::sim::ext_modules::{ClientNotify, ClientSimEvent};

    fn player(house: i64) -> Viewer {
        Viewer {
            house,
            spectator: false,
            sees_whole_map: false,
        }
    }

    const SPECTATOR: Viewer = Viewer {
        house: 0,
        spectator: true,
        sees_whole_map: true,
    };

    #[test]
    fn key_text_parses_in_any_case_and_order_and_round_trips() {
        let k = parse_key("Ctrl+Alt+K").unwrap();
        assert_eq!(k, CTRL_BIT | ALT_BIT | 0x4B);
        assert_eq!(parse_key("alt + control + k"), Some(k));
        assert_eq!(key_text(k).as_deref(), Some("Ctrl+Alt+K"));
        assert_eq!(parse_key("Shift+F5"), Some(SHIFT_BIT | 0x74));
        assert_eq!(parse_key("F24"), Some(0x87));
        assert_eq!(parse_key("Num7"), Some(0x67));
        assert_eq!(parse_key("numpad0"), Some(0x60));
        assert_eq!(parse_key("PgUp"), Some(0x21));
        assert_eq!(key_text(0x21).as_deref(), Some("PageUp"));
        assert_eq!(parse_key("Ctrl+;"), Some(CTRL_BIT | 0xBA));
        for text in [
            "",
            "Ctrl+",
            "Ctrl+Ctrl+K",
            "Hyper+K",
            "F0",
            "F25",
            "F01",
            "KK",
            "Ctrl++",
        ] {
            assert_eq!(parse_key(text), None, "{text:?}");
        }
        for vk in (0x30..=0x39)
            .chain(0x41..=0x5A)
            .chain(0x60..=0x69)
            .chain(0x70..=0x87)
        {
            let encoded = vk | SHIFT_BIT;
            assert_eq!(parse_key(&key_text(encoded).unwrap()), Some(encoded));
        }
    }

    #[test]
    fn hotkeys_yield_to_game_keys_and_fire_per_source() {
        let mut local = LocalState::new();
        local.set_game_keys([CTRL_BIT | ALT_BIT | 0x46]);
        assert_eq!(
            local.register_hotkey(Source::Mod, "Report", "Ctrl+Alt+K"),
            Ok(Status::Ok)
        );
        // The game's key, a reserved key, an unknown name: registered keyless.
        assert_eq!(
            local.register_hotkey(Source::Mod, "Stats", "Ctrl+Alt+F"),
            Ok(Status::Refused)
        );
        assert_eq!(
            local.register_hotkey(Source::Mod, "Chat", "Shift+Enter"),
            Ok(Status::Refused)
        );
        assert_eq!(
            local.register_hotkey(Source::Map, "Odd", "Mouse4"),
            Ok(Status::Refused)
        );
        assert_eq!(
            local.register_hotkey(Source::Map, "Bare", ""),
            Ok(Status::Ok)
        );
        assert!(local.register_hotkey(Source::Map, "", "K").is_err());
        assert_eq!(local.hotkey_id(Source::Mod, "report"), (0, Status::Ok));
        assert_eq!(local.hotkey_id(Source::Mod, "Stats"), (1, Status::Ok));
        assert_eq!(
            local.hotkey_id(Source::Map, "Report"),
            (-1, Status::NotFound)
        );
        assert_eq!(
            local.register_hotkey(Source::Map, "MapReport", "ctrl+alt+k"),
            Ok(Status::Ok)
        );

        assert!(!local.key_down("Ctrl+Alt+F"));
        assert!(!local.key_down("Enter"));
        assert!(local.key_down("Alt+Ctrl+K"));
        let (mut mod_events, mut map_events) = (Vec::new(), Vec::new());
        local.take_events(Source::Mod, &mut mod_events);
        local.take_events(Source::Map, &mut map_events);
        assert_eq!(mod_events.len(), 1);
        assert_eq!(
            (mod_events[0].kind, mod_events[0].id),
            (client_event::HOTKEY, 0)
        );
        // The map's third registration (after "Odd" and "Bare").
        assert_eq!(
            (map_events[0].kind, map_events[0].id),
            (client_event::HOTKEY, 2)
        );

        // Rebinding keeps the ID.
        assert_eq!(
            local.register_hotkey(Source::Mod, "REPORT", "J"),
            Ok(Status::Ok)
        );
        assert_eq!(local.hotkey_id(Source::Mod, "Report"), (0, Status::Ok));
        assert!(local.key_down("J"));
    }

    #[test]
    fn hotkeys_per_source_are_capped() {
        let mut local = LocalState::new();
        for n in 0..MAX_HOTKEYS {
            assert_eq!(
                local.register_hotkey(Source::Mod, &format!("k{n}"), ""),
                Ok(Status::Ok)
            );
        }
        assert_eq!(
            local.register_hotkey(Source::Mod, "one more", ""),
            Ok(Status::Refused)
        );
        assert_eq!(local.register_hotkey(Source::Map, "k0", ""), Ok(Status::Ok));
    }

    #[test]
    fn selection_changes_raise_one_event_per_change() {
        let mut local = LocalState::new();
        local.observe_selection(&[]);
        local.observe_selection(&[3, 1]);
        local.observe_selection(&[1, 3]);
        local.observe_selection(&[1]);
        let mut events = Vec::new();
        local.take_events(Source::Map, &mut events);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|e| e.kind == client_event::SELECTION_CHANGED)
        );
    }

    #[test]
    fn events_wait_past_the_frame_cap_and_the_backlog_is_bounded() {
        let mut local = LocalState::new();
        for _ in 0..MAX_PENDING_EVENTS + 10 {
            local.push(Source::Mod, simple_event(client_event::SELECTION_CHANGED));
        }
        let mut out = vec![simple_event(client_event::CLICK); 24];
        local.take_events(Source::Mod, &mut out);
        assert_eq!(out.len(), CLIENT_FRAME_EVENTS);
        assert_eq!(
            local.pending[0].len(),
            MAX_PENDING_EVENTS - (CLIENT_FRAME_EVENTS - 24)
        );
    }

    #[test]
    fn messages_keep_size_and_rate_limits() {
        let mut local = LocalState::new();
        let mut effects = LocalEffects::default();
        let me = player(7);
        let mut send = |local: &mut LocalState, frame, source, len| {
            local.message(&mut effects, me, frame, source, &vec![1; len])
        };
        assert_eq!(send(&mut local, 10, Source::Mod, 97), Status::Refused);
        assert_eq!(send(&mut local, 10, Source::Mod, 96), Status::Ok);
        assert_eq!(send(&mut local, 10, Source::Mod, 0), Status::Ok);
        assert_eq!(send(&mut local, 10, Source::Mod, 1), Status::Refused);
        // The map's own count.
        assert_eq!(send(&mut local, 10, Source::Map, 1), Status::Ok);
        // 16 in any 15 frames: 2 at 10, then 2 a frame from 11.
        for frame in 11..18 {
            assert_eq!(send(&mut local, frame, Source::Mod, 1), Status::Ok);
            assert_eq!(send(&mut local, frame, Source::Mod, 1), Status::Ok);
        }
        assert_eq!(send(&mut local, 18, Source::Mod, 1), Status::Refused);
        assert_eq!(send(&mut local, 24, Source::Mod, 1), Status::Refused);
        // Frame 10's two leave the window at 25.
        assert_eq!(send(&mut local, 25, Source::Mod, 1), Status::Ok);
        assert_eq!(send(&mut local, 25, Source::Mod, 1), Status::Ok);
        assert_eq!(send(&mut local, 25, Source::Mod, 1), Status::Refused);
        drop(send);
        assert_eq!(
            local.message(&mut effects, SPECTATOR, 40, Source::Mod, &[1]),
            Status::Refused
        );
        assert_eq!(effects.commands.len(), 2 + 1 + 14 + 2);
        assert!(matches!(
            &effects.commands[2],
            Command::ModuleMessage { source: 1, data } if data == &[1]
        ));
    }

    #[test]
    fn orders_keep_unit_and_house_limits() {
        let mut local = LocalState::new();
        let send = |local: &mut LocalState, now, house_wide, units: &[u64]| {
            let allowed = local.order_allowed(now, house_wide, units);
            if allowed {
                local.record_order(now, house_wide, units);
            }
            allowed
        };
        assert!(send(&mut local, 100, false, &[1, 2]));
        assert!(!send(&mut local, 100, false, &[2, 3]));
        assert!(send(&mut local, 100, false, &[3]));
        assert!(!send(&mut local, 114, false, &[1]));
        assert!(send(&mut local, 115, false, &[1]));
        for _ in 0..HOUSE_ORDERS_PER_WINDOW {
            assert!(send(&mut local, 120, true, &[]));
        }
        assert!(!send(&mut local, 134, true, &[]));
        assert!(send(&mut local, 135, true, &[]));
        // A loaded game's earlier frame starts the windows over.
        assert!(send(&mut local, 5, false, &[1]));
        assert!(send(&mut local, 5, true, &[]));
    }

    fn record(house: i64, other: i64) -> ClientSimEvent {
        let mut record = [0u8; 120];
        record[4..8].copy_from_slice(&7i32.to_le_bytes());
        record[32..40].copy_from_slice(&other.to_le_bytes());
        ClientSimEvent { house, record }
    }

    fn notify(to: i64, id: i32) -> ClientNotify {
        ClientNotify {
            to,
            source: Source::Map,
            id,
            data: vec![id as u8],
        }
    }

    #[test]
    fn outbox_reaches_only_its_player_and_hides_unseen_others() {
        let outbox = ClientOutbox {
            notifies: vec![notify(5, 1), notify(6, 2), notify(0, 3)],
            sim_events: vec![record(5, 40), record(5, 41), record(6, 42), record(5, 0)],
        };
        let sees = |handle: i64| handle == 40;
        let events = outbox_events(&outbox, player(5), sees);
        let notifies: Vec<_> = events
            .iter()
            .filter(|e| e.kind == client_event::NOTIFY)
            .map(|e| (e.id, e.source, e.data.clone()))
            .collect();
        assert_eq!(notifies, vec![(1, Source::Map, vec![1])]);
        let others: Vec<i64> = events
            .iter()
            .filter(|e| e.kind == client_event::SIM_EVENT)
            .map(|e| {
                assert_eq!(e.data.len(), 120);
                assert_eq!(e.data[4], 7);
                i64::from_le_bytes(e.data[32..40].try_into().unwrap())
            })
            .collect();
        assert_eq!(others, vec![40, 0, 0]);

        let events = outbox_events(&outbox, SPECTATOR, |_| true);
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].kind, events[0].id), (client_event::NOTIFY, 3));
    }

    #[test]
    fn global_references_read_camera_and_settings() {
        let local = LocalState::new();
        let camera = (REF_KIND_CLIENT_GLOBAL << 48) | CLIENT_CAMERA;
        let settings = (REF_KIND_CLIENT_GLOBAL << 48) | CLIENT_SETTINGS;
        let prop = |name| vera20k_wasm_ext::property_id(name).unwrap();
        let inputs = LocalInputs {
            camera_cell: Some((12, 34)),
        };
        assert_eq!(
            local.global_get(&inputs, player(9), camera, prop("Cell")),
            Ok((client_cell_ref(12, 34), Status::Ok))
        );
        assert_eq!(
            local.global_get(&inputs, player(9), settings, prop("Viewer")),
            Ok((9, Status::Ok))
        );
        assert_eq!(
            local.global_get(&inputs, SPECTATOR, settings, prop("IsSpectator")),
            Ok((1, Status::Ok))
        );
        assert_eq!(
            local.global_get(&inputs, player(9), camera, prop("X")),
            Ok((0, Status::Unsupported))
        );
        assert!(
            local
                .global_get(&inputs, player(9), 1 << 48, prop("Cell"))
                .is_err()
        );
        assert!(local.global_get(&inputs, player(9), camera, -1).is_err());
    }

    #[test]
    fn message_colours_are_rgb() {
        assert_eq!(message_rgb(0), Ok(None));
        assert_eq!(message_rgb(0xFF0000), Ok(Some([1.0, 0.0, 0.0])));
        assert!(message_rgb(0x100_0000).is_err());
        assert!(message_rgb(-1).is_err());
    }
}
