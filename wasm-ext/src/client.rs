//! The client world: `ui.wasm` modules under `vera:client@1.0`
//! ([RFC 0001 section 10](../../docs/rfcs/0001-wasm-extension-modules.md),
//! appendix section 2).
//!
//! A client module runs on the presentation side only: once per render
//! frame it gets `client_on_events`, then `client_on_render`. It may use
//! floats, is never saved and never enters the hash. It reaches the game
//! only through [`ClientHost`]; the app implements that trait
//! (`src/app/ui_modules`).

use crate::{ActionParams, FindPage, Invalid, Order, Query, Status};

/// The import module name for client API 1.0 (`vera:client@<major>.<minor>`).
pub const CLIENT_IMPORT_MODULE: &str = "vera:client@1.0";

/// Operations per render frame for one client module (RFC 15.4), shared by
/// that frame's `client_on_events` and `client_on_render` calls.
pub const CLIENT_FRAME_BUDGET: u64 = 100_000;
/// Client events per render frame for one client module (RFC 15.4).
pub const CLIENT_FRAME_EVENTS: usize = 1_024;
/// The one-time budget of `client_init` and of `client_on_start`, each, as
/// the simulation's one-time calls get (RFC 15.4 "One-time budgets"): the
/// start function builds the module's interface.
pub const CLIENT_ONE_TIME_BUDGET: u64 = 5_000_000;
/// The smallest `vera.delivery` area a module exporting `client_on_events`
/// may declare: one record with the largest event data, so every event
/// fits in some batch.
pub const CLIENT_DELIVERY_MIN: usize = ClientEventRecord::SIZE + crate::MAX_NOTIFY_BYTES;

/// The client imports this engine provides (appendix 2.1), with their core
/// Wasm signatures as text (`params -> results`).
pub const CLIENT_IMPORTS: &[(&str, &str)] = &[
    ("client_frame", "() -> i32"),
    ("client_random", "(lo: i32, hi: i32, out: ptr) -> status"),
    ("client_log", "(ptr, len)"),
    ("client_has_kind", "(table: i32, id: i32) -> i32"),
    ("client_lookup", "(kind: i32, ptr, len, out: ptr) -> status"),
    (
        "client_name_of",
        "(kind: i32, id: i32, out: ptr, cap: i32, len_out: ptr) -> status",
    ),
    ("client_get", "(ref: i64, prop: i32, out: ptr) -> status"),
    (
        "client_get_many",
        "(refs: ptr, n: i32, props: ptr, m: i32, out: ptr, count_out: ptr) -> status",
    ),
    (
        "client_find",
        "(query: ptr, len: i32, out: ptr, cap: i32, res: ptr) -> status",
    ),
    (
        "client_var_get",
        "(scope: i64, var: i32, out: ptr) -> status",
    ),
    ("client_set", "(ref: i64, prop: i32, value: i64) -> status"),
    (
        "client_set_bytes",
        "(ref: i64, prop: i32, ptr, len: i32) -> status",
    ),
    (
        "client_node_create",
        "(kind: i32, parent: i64, props: ptr, len: i32, out: ptr) -> status",
    ),
    ("client_node_remove", "(node: i64) -> status"),
    ("client_node_find", "(ptr, len: i32, out: ptr) -> status"),
    (
        "client_action",
        "(kind: i32, target: i64, params: ptr, len: i32) -> status",
    ),
    ("client_send_order", "(ptr, len: i32) -> status"),
    ("client_send_message", "(ptr, len: i32) -> status"),
    (
        "client_storage_read",
        "(key: ptr, key_len: i32, out: ptr, cap: i32, len_out: ptr) -> status",
    ),
    (
        "client_storage_write",
        "(key: ptr, key_len: i32, data: ptr, data_len: i32) -> status",
    ),
    ("client_status", "(ptr, len: i32) -> status"),
];

/// Client event kinds (appendix 4.6), the `kind` of a [`ClientEventRecord`].
pub mod client_event {
    pub const CLICK: i32 = 0;
    pub const HOVER: i32 = 1;
    pub const HOTKEY: i32 = 2;
    pub const DISMISS: i32 = 3;
    pub const MAP_CLICK: i32 = 4;
    pub const SELECTION_CHANGED: i32 = 5;
    pub const NOTIFY: i32 = 6;
    pub const SIM_EVENT: i32 = 7;
}

/// Client node kinds (appendix 7.10 `NodeKind`).
pub mod node_kind {
    pub const TEXT: i32 = 0;
    pub const BUTTON: i32 = 1;
    pub const ROW: i32 = 2;
    pub const COLUMN: i32 = 3;
    pub const CAMEO_GRID: i32 = 4;
    pub const LIST: i32 = 5;
    pub const IMAGE: i32 = 6;
    pub const BAR: i32 = 7;
    /// Display properties attached to an object, over the battlefield.
    pub const DECORATION: i32 = 8;
    /// Points, lines, circles and text at map cells, over the battlefield
    /// and on the radar.
    pub const MAP_MARKER: i32 = 9;
}

/// Client action kinds (appendix 5.4), the `kind` of `client_action`.
pub mod client_action {
    pub const PLAY_SOUND: i32 = 0;
    pub const PLAY_SPEECH: i32 = 1;
    pub const CENTER_CAMERA: i32 = 2;
    pub const JUMP_CAMERA: i32 = 3;
    /// `later`: answers `Unsupported`.
    pub const HOME_CAMERA: i32 = 4;
    pub const SELECT: i32 = 5;
    pub const FLASH_OBJECT: i32 = 6;
    pub const SHOW_MESSAGE: i32 = 7;
    pub const REGISTER_HOTKEY: i32 = 8;
}

/// Reference kinds the client adds (appendix "References"): bits 48..52.
pub const REF_KIND_NODE: i64 = 5;
pub const REF_KIND_CLIENT_GLOBAL: i64 = 6;
/// Client global references (kind 6).
pub const CLIENT_CAMERA: i64 = 1;
pub const CLIENT_SETTINGS: i64 = 2;
pub const CLIENT_CURSOR: i64 = 3;

/// The sending module's source in a [`ClientEventRecord`] and for storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Mod = 0,
    Map = 1,
}

/// One client event before it is laid out for `client_on_events`. The
/// sandbox writes `data` into the delivery area and points the record's
/// `data`/`data_len` at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientEvent {
    pub kind: i32,
    /// `Notify`: the sending module's source; otherwise `Source::Mod` (0).
    pub source: Source,
    /// Click, Hover, Dismiss: node; MapClick: cell reference; else 0.
    pub reference: i64,
    /// Hotkey: hotkey ID; Notify: notify ID; else −1.
    pub id: i32,
    /// See appendix 4.6.
    pub value: i32,
    /// Notify: the notification's bytes; SimEvent: one 120-byte
    /// `EventRecord`; else empty.
    pub data: Vec<u8>,
}

/// `ClientEventRecord` (appendix "Fixed layouts"), 32 bytes little endian.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientEventRecord {
    pub kind: i32,
    pub source: i32,
    pub reference: i64,
    pub id: i32,
    pub value: i32,
    pub data: i32,
    pub data_len: i32,
}

impl ClientEventRecord {
    pub const SIZE: usize = 32;

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..4].copy_from_slice(&self.kind.to_le_bytes());
        b[4..8].copy_from_slice(&self.source.to_le_bytes());
        b[8..16].copy_from_slice(&self.reference.to_le_bytes());
        b[16..20].copy_from_slice(&self.id.to_le_bytes());
        b[20..24].copy_from_slice(&self.value.to_le_bytes());
        b[24..28].copy_from_slice(&self.data.to_le_bytes());
        b[28..32].copy_from_slice(&self.data_len.to_le_bytes());
        b
    }
}

/// One `PropValue` of `client_node_create` (appendix "Fixed layouts").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInit {
    pub prop: i32,
    pub value: i64,
}

impl NodeInit {
    pub const SIZE: usize = 16;
}

/// What a client module reaches the game through: one method per
/// `vera:client` import that needs the game (appendix 2.1). The sandbox
/// checks arguments that need no game state, charges the budget, and copies
/// memory; `client_get_many`'s loop and `client_random`'s local stream
/// (xorshift64, seeded per module) are its own.
///
/// `Err(Invalid)` disables the module (RFC section 3, "Error model").
pub trait ClientHost {
    /// The latest finished simulation frame (`client_frame`).
    fn frame(&mut self) -> i32;
    /// One line for the in-game console (`client_log`).
    fn log(&mut self, line: &str);
    fn has_kind(&mut self, table: i32, id: i32) -> bool;
    /// As `lookup`; variable and notify names are the ones this match's
    /// simulation modules declared; `Hotkey` names are this module's.
    fn lookup(&mut self, kind: i32, name: &str) -> Result<(i32, Status), Invalid>;
    fn name_of(&mut self, kind: i32, id: i32) -> Result<(String, Status), Invalid>;
    /// A property as this player's interface may show it (appendix 7,
    /// "client" column): `Gone` for what cannot be seen, an empty value and
    /// `Refused` for a hidden property. Node and client-global references
    /// read their own properties (7.10).
    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid>;
    /// As `find`, only what this player can see.
    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid>;
    /// A variable, under its visibility (RFC 6e).
    fn var_get(&mut self, scope: i64, var: i32) -> Result<(i64, Status), Invalid>;
    /// Writes a client-owned reference's property: node, camera, cursor,
    /// local settings. `Refused` for a node this source may not write.
    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid>;
    /// Writes a string or bytes property (`Label`, `Art`, `Items`, ...).
    fn set_bytes(&mut self, reference: i64, prop: i32, bytes: &[u8]) -> Result<Status, Invalid>;
    /// Creates a node of `kind` under `parent` (0: top level); returns its
    /// handle (reference kind 5) or 0.
    fn node_create(
        &mut self,
        kind: i32,
        parent: i64,
        init: &[NodeInit],
    ) -> Result<(i64, Status), Invalid>;
    /// Removes a node and its children.
    fn node_remove(&mut self, node: i64) -> Result<Status, Invalid>;
    /// Finds a node by `Name`: this source's own first, then engine, mod,
    /// map; the first created on a tie. `(0, NotFound)` without one.
    fn node_find(&mut self, name: &str) -> Result<(i64, Status), Invalid>;
    /// A local action (appendix 5.4); `text` is `params.text` read from
    /// memory, `extra` is `params.extra`.
    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
        text: &str,
        extra: &[u8],
    ) -> Result<Status, Invalid>;
    /// A standard player order for this player, checked like player input;
    /// `Refused` unless `AutomationAllowed` and within the rate limit.
    fn send_order(&mut self, order: &Order) -> Result<Status, Invalid>;
    /// A message into the command stream for the same source's simulation
    /// module's `on_message`; `Refused` for spectators and over the limits.
    fn send_message(&mut self, data: &[u8]) -> Result<Status, Invalid>;
    /// The module source's local storage value for `key`; `None` and
    /// `NotFound` without one.
    fn storage_read(&mut self, key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid>;
    /// Writes (or with empty `data` deletes) a storage key; `Refused` over
    /// the quota (RFC 10, "Storage").
    fn storage_write(&mut self, key: &str, data: &[u8]) -> Result<Status, Invalid>;
    /// A status template as its INI section's text (`client_status`):
    /// registered, replacing this source's one of the same `Name=`, or
    /// removed when it has no elements. `Refused` over the limits or for a
    /// key the source may not use; `Invalid` for text that does not parse.
    fn status_template(&mut self, text: &str) -> Result<Status, Invalid>;
}
