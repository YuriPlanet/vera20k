//! Client modules (`ui.wasm`, RFC 0001 section 10, appendix 2): bindings
//! for every `vera:client@1.0` import and the client exports.
//!
//! A client module runs on the local machine only, once per render frame:
//! `client_on_events` ([`on_client_event!`](crate::on_client_event!)), then
//! `client_on_render` ([`on_render!`](crate::on_render!)). It reads what
//! this player's interface may show, changes nodes, the camera and the
//! cursor, plays local effects, and reaches the game only through
//! [`Client::send_message`] (to its source's simulation module's
//! `on_message`) and [`Client::order`]. It may use floats.
//!
//! Only call this module's functions from a client module: the crate-root
//! wrappers (`get`, `log`, `Var`, ...) import `vera:sim` functions, which
//! the engine refuses in a client module. Reference types ([`House`],
//! [`Cell`], [`Techno`], [`Prop`]) are shared.
//!
//! ```ignore
//! use vera20k_sdk::client::prelude::*;
//!
//! static BUY: NodeSlot = NodeSlot::new();
//!
//! fn start() {
//!     if let Ok(button) = Ui::find("BuyReinforcements") {
//!         let _ = button.set_clickable(true);
//!         BUY.set(button);
//!     }
//! }
//!
//! fn on_click(e: &ClientEventRecord) {
//!     if e.node() == BUY.get() {
//!         let _ = Client::send_message(&[1]);
//!     }
//! }
//!
//! vera20k_sdk::on_start!(start);
//! vera20k_sdk::on_client_event!(ClientEvent::Click => on_click);
//! ```

use core::sync::atomic::{AtomicI32, AtomicI64, Ordering};

use crate::{
    Cell, House, NO_HANDLE, NameKind, Order, Prop, Ref, Result, Status, Techno, check, sys,
};

/// What a client module usually needs.
pub mod prelude {
    pub use super::{
        Camera, Client, ClientEvent, ClientEventRecord, ClientVar, Cursor, Hotkey, Local, Node,
        NodeKind, NodeSlot, NotifyName, Storage, Ui,
    };
    pub use crate::{Cell, House, NO_HANDLE, NameKind, Prop, Ref, Status, Techno, TypeRef};
}

// --------------------------------------------------------------- References

/// Bits 48..52 of a reference hold its kind (appendix "References").
const KIND_SHIFT: u32 = 48;
/// Client node handles (kind 5).
const KIND_NODE: i64 = 5 << KIND_SHIFT;
/// Client global references (kind 6): camera 1, local settings 2, cursor 3.
const CAMERA: i64 = 6 << KIND_SHIFT | 1;
const SETTINGS: i64 = 6 << KIND_SHIFT | 2;
const CURSOR: i64 = 6 << KIND_SHIFT | 3;

/// A node of the match interface (kind 5): one a layout made or the module
/// created. It expires when removed or when the match interface closes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Node(pub i64);

impl Ref for Node {
    fn raw(self) -> i64 {
        self.0
    }
}

impl Node {
    /// Whether this is `NO_HANDLE`.
    pub const fn is_none(self) -> bool {
        self.0 == NO_HANDLE
    }

    /// Whether this reference is a node handle (kind 5).
    pub const fn is_node(self) -> bool {
        self.0 & (0x1f << KIND_SHIFT) == KIND_NODE
    }
}

/// A node kept between callbacks, e.g. the button `client_on_start` found.
pub struct NodeSlot(AtomicI64);

impl NodeSlot {
    pub const fn new() -> Self {
        Self(AtomicI64::new(NO_HANDLE))
    }

    pub fn set(&self, node: Node) {
        self.0.store(node.0, Ordering::Relaxed);
    }

    /// The node, or `NO_HANDLE` before [`NodeSlot::set`].
    pub fn get(&self) -> Node {
        Node(self.0.load(Ordering::Relaxed))
    }
}

impl Default for NodeSlot {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------- Basic calls

/// The latest finished simulation frame (0 in `client_init`).
pub fn frame() -> i32 {
    unsafe { sys::client_frame() }
}

/// A local, display-only number in `lo..=hi`. `lo > hi` disables the module.
pub fn random(lo: i32, hi: i32) -> i32 {
    let mut out = 0;
    unsafe { sys::client_random(lo, hi, &mut out) };
    out
}

/// One line in the in-game console.
pub fn log(line: &str) {
    unsafe { sys::client_log(line.as_ptr(), line.len() as i32) }
}

/// Whether entry `id` of `table` (appendix 8.3 `Table`) is available.
pub fn has_kind(table: i32, id: i32) -> bool {
    unsafe { sys::client_has_kind(table, id) != 0 }
}

/// The ID of `name` in name kind `kind`. Variable and notification names
/// are the ones this match's simulation modules declared; hotkey names
/// this module's.
pub fn lookup(kind: NameKind, name: &str) -> Result<i32> {
    let mut out = -1;
    check(unsafe { sys::client_lookup(kind as i32, name.as_ptr(), name.len() as i32, &mut out) })?;
    Ok(out)
}

/// Writes the name of `id` into `buf` and returns it. `Err(Refused)` when
/// `buf` is too small.
pub fn name_of(kind: NameKind, id: i32, buf: &mut [u8]) -> Result<&str> {
    let mut len = 0;
    check(unsafe {
        sys::client_name_of(
            kind as i32,
            id,
            buf.as_mut_ptr(),
            buf.len() as i32,
            &mut len,
        )
    })?;
    let bytes = buf.get(..len as usize).ok_or(Status::Refused)?;
    core::str::from_utf8(bytes).map_err(|_| Status::Unsupported)
}

// ---------------------------------------------------------------- Properties

/// `prop` of `reference` as this player's interface may show it: an object
/// out of sight is `Err(Gone)`, a hidden property `Err(Refused)`.
pub fn get(reference: impl Ref, prop: Prop) -> Result<i64> {
    let mut out = 0;
    check(unsafe { sys::client_get(reference.raw(), prop.0, &mut out) })?;
    Ok(out)
}

/// Reads `props` of every reference in `refs` into `out`, row-major
/// (`out[i * props.len() + j]`), each under `get`'s visibility. Returns the
/// first non-`Ok` status, the empty values already written.
pub fn get_many(refs: &[i64], props: &[i32], out: &mut [i64]) -> Result<usize> {
    let needed = refs.len().checked_mul(props.len()).ok_or(Status::Refused)?;
    if out.len() < needed {
        return Err(Status::Refused);
    }
    let mut count = 0;
    let status = unsafe {
        sys::client_get_many(
            refs.as_ptr(),
            refs.len() as i32,
            props.as_ptr(),
            props.len() as i32,
            out.as_mut_ptr(),
            &mut count,
        )
    };
    check(status).map(|()| count as usize)
}

/// Writes a client-owned reference's property: a node, the camera, the
/// cursor. `Err(Refused)` for a node this module may not write.
pub fn set(reference: impl Ref, prop: Prop, value: i64) -> Result<()> {
    check(unsafe { sys::client_set(reference.raw(), prop.0, value) })
}

/// Writes a string or bytes property (`Label`, `Art`, `Items`, ...).
pub fn set_bytes(reference: impl Ref, prop: Prop, bytes: &[u8]) -> Result<()> {
    check(unsafe {
        sys::client_set_bytes(reference.raw(), prop.0, bytes.as_ptr(), bytes.len() as i32)
    })
}

/// One page of a `find` query restricted to what this player sees, built
/// with [`crate::Find`]: references into `out`, and where the next page
/// starts (0 on the last).
pub fn find_page(query: &mut crate::Find, out: &mut [i64], after: i64) -> Result<(usize, i64)> {
    let (bytes, len) = query.raw(after);
    let mut result = sys::FindResult::default();
    check(unsafe {
        sys::client_find(
            bytes,
            len as i32,
            out.as_mut_ptr(),
            out.len() as i32,
            &mut result,
        )
    })?;
    Ok((result.count as usize, result.next))
}

// ----------------------------------------------------------------- Variables

/// A simulation module's declared variable, read under its visibility
/// (RFC 6e): resolved by name on first use with `client_lookup`.
pub struct ClientVar {
    name: &'static str,
    id: AtomicI32,
}

impl ClientVar {
    /// `name` as the simulation module's `vars!` declares it (`mod.` or
    /// `map.` addresses the other source's).
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            id: AtomicI32::new(-1),
        }
    }

    pub fn id(&self) -> Result<i32> {
        let id = self.id.load(Ordering::Relaxed);
        if id >= 0 {
            return Ok(id);
        }
        let id = lookup(NameKind::Variable, self.name)?;
        self.id.store(id, Ordering::Relaxed);
        Ok(id)
    }

    /// A global variable.
    pub fn get(&self) -> Result<i64> {
        var_get(NO_HANDLE, self.id()?)
    }

    /// A house variable; a private one only for this player's own house.
    pub fn get_house(&self, house: House) -> Result<i64> {
        var_get(house.0, self.id()?)
    }

    /// An object variable.
    pub fn get_object(&self, object: impl Ref) -> Result<i64> {
        var_get(object.raw(), self.id()?)
    }
}

/// `client_var_get(scope, var)`: scope 0 for a global variable.
pub fn var_get(scope: i64, var: i32) -> Result<i64> {
    let mut out = 0;
    check(unsafe { sys::client_var_get(scope, var, &mut out) })?;
    Ok(out)
}

// --------------------------------------------------------------------- Nodes

/// Node kinds (appendix 7.10 `NodeKind`). Modules create all but
/// `CameoGrid`, which only layouts make.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    Text = 0,
    Button = 1,
    Row = 2,
    Column = 3,
    CameoGrid = 4,
    List = 5,
    Image = 6,
    Bar = 7,
    /// Display properties attached to an object over the battlefield
    /// (`Anchor`, `X`, `Y`, `Art`, `Label`, `Number`, `Tint`,
    /// `TintIntensity`, `HideAnchor`); top level only.
    Decoration = 8,
    /// A point, line, circle and text at a cell, over the battlefield and
    /// on the radar (`Anchor`, `Items`, `Width`, `Hold`, `Tint`, ...); top
    /// level only.
    MapMarker = 9,
}

/// The interface tree: finding, creating and removing nodes (RFC 10).
pub struct Ui;

impl Ui {
    /// The node named `name`: a layout part's name (without the layout
    /// prefix) or a created node's `Name`. This module's source first, then
    /// engine, mod and map; `Err(NotFound)` without one.
    pub fn find(name: &str) -> Result<Node> {
        let mut out = NO_HANDLE;
        check(unsafe { sys::client_node_find(name.as_ptr(), name.len() as i32, &mut out) })?;
        Ok(Node(out))
    }

    /// A new node of `kind` under `parent` (`Node(NO_HANDLE)`: top level,
    /// drawn in the overlay for a map module).
    pub fn create(kind: NodeKind, parent: Node) -> Result<Node> {
        Self::create_with(kind, parent, &[])
    }

    /// A new node named `name` (see [`Node::set_name`]); the node is removed
    /// again when the name is refused.
    pub fn create_named(kind: NodeKind, parent: Node, name: &str) -> Result<Node> {
        let node = Self::create(kind, parent)?;
        if let Err(status) = node.set_name(name) {
            // The refused name is what to report, not the removal.
            let _ = node.remove();
            return Err(status);
        }
        Ok(node)
    }

    /// A new node with initial scalar properties, set in one call.
    pub fn create_with(kind: NodeKind, parent: Node, props: &[(Prop, i64)]) -> Result<Node> {
        const MAX: usize = 16;
        if props.len() > MAX {
            return Err(Status::Refused);
        }
        let mut init = [sys::PropValue::default(); MAX];
        for (slot, (prop, value)) in init.iter_mut().zip(props) {
            *slot = sys::PropValue {
                prop: prop.0,
                reserved: 0,
                value: *value,
            };
        }
        let len = props.len() * size_of::<sys::PropValue>();
        let mut out = NO_HANDLE;
        check(unsafe {
            sys::client_node_create(kind as i32, parent.0, init.as_ptr(), len as i32, &mut out)
        })?;
        Ok(Node(out))
    }

    /// Removes `node` and its children.
    pub fn remove(node: Node) -> Result<()> {
        check(unsafe { sys::client_node_remove(node.0) })
    }
}

impl Node {
    pub fn get(self, prop: Prop) -> Result<i64> {
        get(self, prop)
    }

    pub fn set(self, prop: Prop, value: i64) -> Result<()> {
        set(self, prop, value)
    }

    /// Names a node this module created, for [`Ui::find`] (appendix 7.10):
    /// once per node, unique among this source's nodes. `Err(Refused)` for
    /// an empty name, a second name, a name taken, or a node of the engine,
    /// a layout or the other module.
    pub fn set_name(self, name: &str) -> Result<()> {
        set_bytes(self, Prop::Name, name.as_bytes())
    }

    /// A CSF entry name or literal text (`Text`, `Button`).
    pub fn set_label(self, text: &str) -> Result<()> {
        set_bytes(self, Prop::Label, text.as_bytes())
    }

    /// An image by name (`Button`, `Image`).
    pub fn set_art(self, name: &str) -> Result<()> {
        set_bytes(self, Prop::Art, name.as_bytes())
    }

    pub fn visible(self) -> Result<bool> {
        get(self, Prop::Visible).map(|v| v != 0)
    }

    pub fn set_visible(self, visible: bool) -> Result<()> {
        set(self, Prop::Visible, i64::from(visible))
    }

    /// `Button`, `List`: drawn disabled and without `Click` at false.
    pub fn set_enabled(self, enabled: bool) -> Result<()> {
        set(self, Prop::Enabled, i64::from(enabled))
    }

    /// Emits `Click` events.
    pub fn set_clickable(self, clickable: bool) -> Result<()> {
        set(self, Prop::Clickable, i64::from(clickable))
    }

    /// Emits `Hover` events.
    pub fn set_hoverable(self, hoverable: bool) -> Result<()> {
        set(self, Prop::Hoverable, i64::from(hoverable))
    }

    /// Emits `Dismiss` on a press outside it or Esc, while visible.
    pub fn set_dismissable(self, dismissable: bool) -> Result<()> {
        set(self, Prop::Dismissable, i64::from(dismissable))
    }

    /// Position in pixels (sidebar or screen, by where the node is).
    pub fn set_position(self, x: i32, y: i32) -> Result<()> {
        set(self, Prop::X, i64::from(x))?;
        set(self, Prop::Y, i64::from(y))
    }

    pub fn set_size(self, width: i32, height: i32) -> Result<()> {
        set(self, Prop::Width, i64::from(width))?;
        set(self, Prop::Height, i64::from(height))
    }

    /// A `Bar`'s fill, 0..100.
    pub fn set_fill(self, percent: i32) -> Result<()> {
        set(self, Prop::Fill, i64::from(percent))
    }

    /// Whether a `Text` over the battlefield stands on black (the
    /// default); off for a text on the module's own picture.
    pub fn set_backdrop(self, backdrop: bool) -> Result<()> {
        set(self, Prop::Backdrop, i64::from(backdrop))
    }

    /// A `List`'s rows: the entries joined with `\n` in one call.
    /// `Err(Refused)` past 1 KiB in all.
    pub fn set_items(self, items: &[&str]) -> Result<()> {
        let mut buf = [0u8; 1024];
        let mut len = 0;
        for (index, item) in items.iter().enumerate() {
            let sep = usize::from(index > 0);
            let end = len + sep + item.len();
            if end > buf.len() {
                return Err(Status::Refused);
            }
            if sep == 1 {
                buf[len] = b'\n';
            }
            buf[len + sep..end].copy_from_slice(item.as_bytes());
            len = end;
        }
        set_bytes(self, Prop::Items, &buf[..len])
    }

    /// Removes the node and its children.
    pub fn remove(self) -> Result<()> {
        Ui::remove(self)
    }
}

// ---------------------------------------------------------- Client globals

/// This player: who they are and the ways into the command stream.
pub struct Client;

impl Client {
    /// This player's house, or `House(NO_HANDLE)` for a spectator.
    pub fn viewer() -> House {
        House(get(Settings, Prop::Viewer).unwrap_or(NO_HANDLE))
    }

    pub fn is_spectator() -> bool {
        get(Settings, Prop::IsSpectator).is_ok_and(|v| v != 0)
    }

    /// Sends `data` (at most 96 bytes) to this source's simulation module's
    /// `on_message` through the command stream (RFC 10). `Err(Refused)` for
    /// spectators and over the size and rate limits.
    pub fn send_message(data: &[u8]) -> Result<()> {
        check(unsafe { sys::client_send_message(data.as_ptr(), data.len() as i32) })
    }

    /// A standard player order for this player, built with [`Order`]'s
    /// constructors and checked like player input. `Err(Refused)` unless the
    /// room allows automation (`AutomationAllowed`) and within the rate
    /// limits (RFC 6i).
    pub fn order(order: &Order) -> Result<()> {
        let (words, len) = order.words()?;
        check(unsafe { sys::client_send_order(words.as_ptr().cast(), len as i32) })
    }
}

/// The local settings reference (kind 6, 2).
#[derive(Clone, Copy)]
struct Settings;

impl Ref for Settings {
    fn raw(self) -> i64 {
        SETTINGS
    }
}

/// The local camera (kind 6, 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Camera;

impl Ref for Camera {
    fn raw(self) -> i64 {
        CAMERA
    }
}

impl Camera {
    /// The cell at the camera's centre.
    pub fn cell() -> Result<Cell> {
        get(Camera, Prop::Cell).map(Cell)
    }

    /// Jumps the local camera to `cell` (client action `JumpCamera`).
    pub fn jump_to(cell: Cell) -> Result<()> {
        Local::run(Local::JUMP_CAMERA, cell.0, &sys::ActionParams::default())
    }

    /// Moves the local camera to `cell` at `speed`, as stock action 48.
    pub fn center_on(cell: Cell, speed: i64) -> Result<()> {
        let params = sys::ActionParams {
            value: speed,
            ..Default::default()
        };
        Local::run(Local::CENTER_CAMERA, cell.0, &params)
    }
}

/// The mouse cursor (kind 6, 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cursor;

impl Ref for Cursor {
    fn raw(self) -> i64 {
        CURSOR
    }
}

impl Cursor {
    /// Screen pixels, as node `X` and `Y` count.
    pub fn position() -> Result<(i64, i64)> {
        Ok((get(Cursor, Prop::X)?, get(Cursor, Prop::Y)?))
    }

    /// The cell under the cursor, `Cell(NO_HANDLE)` off the map.
    pub fn cell() -> Result<Cell> {
        get(Cursor, Prop::Cell).map(Cell)
    }

    /// At true, map clicks reach only this module (`MapClick`), not the game.
    pub fn capture_map_clicks(capture: bool) -> Result<()> {
        set(Cursor, Prop::CaptureMapClick, i64::from(capture))
    }
}

// ------------------------------------------------------------- Local actions

/// Local actions (appendix 5.4): effects on this machine only.
pub struct Local;

impl Local {
    pub const PLAY_SOUND: i32 = 0;
    pub const PLAY_SPEECH: i32 = 1;
    pub const CENTER_CAMERA: i32 = 2;
    pub const JUMP_CAMERA: i32 = 3;
    pub const HOME_CAMERA: i32 = 4;
    pub const SELECT: i32 = 5;
    pub const FLASH_OBJECT: i32 = 6;
    pub const SHOW_MESSAGE: i32 = 7;
    pub const REGISTER_HOTKEY: i32 = 8;

    /// Any client action with its parameters.
    pub fn run(kind: i32, target: i64, params: &sys::ActionParams) -> Result<()> {
        check(unsafe {
            sys::client_action(kind, target, params, size_of::<sys::ActionParams>() as i32)
        })
    }

    fn with_text(kind: i32, target: i64, text: &str, extra: &[u8], value: i64) -> Result<()> {
        let params = sys::ActionParams {
            value,
            text: text.as_ptr() as i32,
            text_len: text.len() as i32,
            extra: extra.as_ptr() as i32,
            extra_len: extra.len() as i32,
            ..Default::default()
        };
        Self::run(kind, target, &params)
    }

    /// Plays sound `id` (`lookup(Sound, ..)`) at `cell`, or not positioned
    /// with `Cell(NO_HANDLE)`.
    pub fn play_sound(id: i32, cell: Cell) -> Result<()> {
        let params = sys::ActionParams {
            id,
            ..Default::default()
        };
        Self::run(Self::PLAY_SOUND, cell.0, &params)
    }

    /// Plays EVA speech `name`.
    pub fn play_speech(name: &str) -> Result<()> {
        Self::with_text(Self::PLAY_SPEECH, NO_HANDLE, name, &[], 0)
    }

    /// Selects `units` locally (only what this player may select); `append`
    /// keeps the current selection.
    /// `Err(Refused)` past [`crate::ORDER_UNITS`] units.
    pub fn select(units: &[Techno], append: bool) -> Result<()> {
        // `extra` is an array of i64 handles.
        let mut handles = [0u8; 8 * crate::ORDER_UNITS];
        if units.len() > crate::ORDER_UNITS {
            return Err(Status::Refused);
        }
        for (slot, unit) in handles.chunks_exact_mut(8).zip(units) {
            slot.copy_from_slice(&unit.0.to_le_bytes());
        }
        let extra = &handles[..units.len() * 8];
        Self::with_text(Self::SELECT, NO_HANDLE, "", extra, i64::from(append))
    }

    /// Flashes `object` for `frames` frames.
    pub fn flash(object: Techno, frames: i64) -> Result<()> {
        let params = sys::ActionParams {
            value: frames,
            ..Default::default()
        };
        Self::run(Self::FLASH_OBJECT, object.0, &params)
    }

    /// Shows one line: a CSF entry name or literal text, in `color`.
    pub fn show_message(text: &str, color: i64) -> Result<()> {
        Self::with_text(Self::SHOW_MESSAGE, NO_HANDLE, text, &[], color)
    }

    /// Registers hotkey `name` with its default chord (`"Ctrl+Alt+K"`);
    /// read its ID with [`Hotkey::id`]. `Err(Refused)` when a game hotkey
    /// already has the chord: the hotkey exists, without a key.
    pub fn register_hotkey(name: &str, chord: &str) -> Result<()> {
        Self::with_text(Self::REGISTER_HOTKEY, NO_HANDLE, name, chord.as_bytes(), 0)
    }
}

/// A hotkey this module registered.
pub struct Hotkey;

impl Hotkey {
    /// The ID a `Hotkey` client event carries for `name`.
    pub fn id(name: &str) -> Result<i32> {
        lookup(NameKind::Hotkey, name)
    }
}

/// A notification name some simulation module of this match declared,
/// resolved once with `client_lookup(Notify, ..)`: compare with a `Notify`
/// event's [`sys::ClientEventRecord::id`].
pub struct NotifyName {
    name: &'static str,
    id: AtomicI32,
}

impl NotifyName {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            id: AtomicI32::new(-1),
        }
    }

    pub fn id(&self) -> Result<i32> {
        let id = self.id.load(Ordering::Relaxed);
        if id != -1 {
            return Ok(id);
        }
        let id = lookup(NameKind::Notify, self.name)?;
        self.id.store(id, Ordering::Relaxed);
        Ok(id)
    }

    /// Whether `event` is this notification.
    pub fn is(&self, event: &ClientEventRecord) -> bool {
        event.kind == ClientEvent::Notify as i32 && self.id().is_ok_and(|id| id == event.id)
    }
}

// ------------------------------------------------------------------ Storage

/// This module source's local key-value store (RFC 10, "Storage"): ASCII
/// keys of 1..64 bytes, values up to 16 KiB, 256 keys and 256 KiB in all.
pub struct Storage;

impl Storage {
    /// The value of `key`, written into `buf`. `Err(NotFound)` without
    /// one; `Err(Refused)` when `buf` is too small (see [`Storage::len`]).
    pub fn read<'b>(key: &str, buf: &'b mut [u8]) -> Result<&'b [u8]> {
        let len = Self::read_raw(key, buf)?;
        buf.get(..len).ok_or(Status::Refused)
    }

    /// The length of `key`'s value.
    pub fn len(key: &str) -> Result<usize> {
        Self::read_raw(key, &mut [])
    }

    fn read_raw(key: &str, buf: &mut [u8]) -> Result<usize> {
        let mut len = -1;
        check(unsafe {
            sys::client_storage_read(
                key.as_ptr(),
                key.len() as i32,
                buf.as_mut_ptr(),
                buf.len() as i32,
                &mut len,
            )
        })?;
        Ok(len.max(0) as usize)
    }

    /// Writes `key`. `Err(Refused)` over a limit, storage unchanged.
    pub fn write(key: &str, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Err(Status::Refused);
        }
        Self::write_raw(key, data)
    }

    /// Deletes `key`.
    pub fn remove(key: &str) -> Result<()> {
        Self::write_raw(key, &[])
    }

    fn write_raw(key: &str, data: &[u8]) -> Result<()> {
        check(unsafe {
            sys::client_storage_write(
                key.as_ptr(),
                key.len() as i32,
                data.as_ptr(),
                data.len() as i32,
            )
        })
    }
}

// ------------------------------------------------------------ Status bars

/// Status templates over the battlefield (`client_status`): bars, pip rows,
/// icons and numbers at every object this player sees, in the keys of a
/// `veraui.ini` `[VeraStatus.<name>]` section (docs/wasm-modules/status-templates.md).
pub struct StatusTemplates;

impl StatusTemplates {
    /// Registers `template` (its keys, `Name=` first), replacing this
    /// module's one of the same name. `Err(Refused)` when a key does not
    /// parse or is over a limit; the console says why.
    pub fn set(template: &str) -> Result<()> {
        check(unsafe { sys::client_status(template.as_ptr(), template.len() as i32) })
    }

    /// Removes this module's template `name` (at most 64 bytes;
    /// `Err(Refused)` for a longer one).
    pub fn remove(name: &str) -> Result<()> {
        const PREFIX: &[u8] = b"Name=";
        let mut text = [0u8; PREFIX.len() + 64];
        let Some(end) = PREFIX.len().checked_add(name.len()).filter(|&end| end <= text.len()) else {
            return Err(Status::Refused);
        };
        text[..PREFIX.len()].copy_from_slice(PREFIX);
        text[PREFIX.len()..end].copy_from_slice(name.as_bytes());
        check(unsafe { sys::client_status(text.as_ptr(), end as i32) })
    }
}

// ------------------------------------------------------------------- Events

/// Client event kinds (appendix 4.6).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClientEvent {
    /// A clickable node: [`ClientEventRecord::node`], `value` the `List` row
    /// or `CameoGrid` item, −1 otherwise.
    Click = 0,
    /// A hoverable node: `value` 1 enter, 0 leave.
    Hover = 1,
    /// A registered hotkey: [`sys::ClientEventRecord::id`].
    Hotkey = 2,
    /// A dismissable node: a press outside it or Esc.
    Dismiss = 3,
    /// A map click: [`ClientEventRecord::cell`], `value` 0 left, 1 right.
    MapClick = 4,
    /// The local selection changed.
    SelectionChanged = 5,
    /// A simulation module's notification: `source`, `id`,
    /// [`ClientEventRecord::data`].
    Notify = 6,
    /// One of this player's simulation events:
    /// [`ClientEventRecord::sim_event`].
    SimEvent = 7,
}

/// One client event and its payload, borrowed together for this callback.
#[derive(Clone, Copy, Debug)]
pub struct ClientEventRecord<'a> {
    raw: &'a sys::ClientEventRecord,
    payload: &'a [u8],
}

impl core::ops::Deref for ClientEventRecord<'_> {
    type Target = sys::ClientEventRecord;

    fn deref(&self) -> &Self::Target {
        self.raw
    }
}

impl<'a> ClientEventRecord<'a> {
    /// The events delivered to `client_on_events(ptr, count)`.
    ///
    /// # Safety
    /// Use the engine's original arguments. The records must be aligned and
    /// initialized, and both the records and their payloads must remain
    /// readable and unchanged for the lifetime of the iterator and its items.
    pub unsafe fn batch(ptr: i32, count: i32) -> impl Iterator<Item = Self> {
        // SAFETY: the engine writes the records in its aligned delivery area.
        let records = unsafe {
            core::slice::from_raw_parts(
                ptr as usize as *const sys::ClientEventRecord,
                count as usize,
            )
        };
        records.iter().map(|raw| {
            let payload = if raw.data_len > 0 {
                // SAFETY: these initialized bytes belong to this event for this callback.
                unsafe {
                    core::slice::from_raw_parts(
                        raw.data as usize as *const u8,
                        raw.data_len as usize,
                    )
                }
            } else {
                &[]
            };
            Self { raw, payload }
        })
    }

    /// `Click`, `Hover`, `Dismiss`: the node.
    pub fn node(&self) -> Node {
        Node(self.reference)
    }

    /// `MapClick`: the cell.
    pub fn cell(&self) -> Cell {
        Cell(self.reference)
    }

    /// `Notify`: whether the sender is the map's simulation module (else the
    /// mod's).
    pub fn from_map(&self) -> bool {
        self.source == 1
    }

    /// `Notify`: the notification's bytes; empty for other events.
    pub fn data(&self) -> &[u8] {
        if self.kind == ClientEvent::Notify as i32 {
            self.payload
        } else {
            &[]
        }
    }

    /// `SimEvent`: the forwarded `EventRecord` (`other` is 0 when this
    /// player cannot see it).
    pub fn sim_event(&self) -> Option<&crate::EventRecord> {
        if self.kind != ClientEvent::SimEvent as i32 {
            return None;
        }
        // SAFETY: EventRecord contains only integers, accepting every bit pattern.
        // The slice pattern requires exactly one aligned record with no spare bytes.
        match unsafe { self.payload.align_to::<crate::EventRecord>() } {
            ([], [event], []) => Some(event),
            _ => None,
        }
    }
}

/// Where the SDK's client delivery area lies: from address 8 to 1024, the
/// first KiB the simulation SDK's area also uses
/// ([`crate::triggers::DELIVERY_OFFSET`] explains why it is free). It holds
/// a record with the largest notification (288 bytes), or 31 records
/// without data.
pub const CLIENT_DELIVERY_OFFSET: u32 = 8;
pub const CLIENT_DELIVERY_BYTES: u32 = 1016;

/// The client module's `vera.delivery` section.
pub const CLIENT_DELIVERY_SECTION: [u8; 9] = {
    let offset = CLIENT_DELIVERY_OFFSET.to_le_bytes();
    let bytes = CLIENT_DELIVERY_BYTES.to_le_bytes();
    [
        1, offset[0], offset[1], offset[2], offset[3], bytes[0], bytes[1], bytes[2], bytes[3],
    ]
};

// ------------------------------------------------------------------ Exports

/// Exports `client_init`, the module's one-time setup. Only
/// [`lookup`], [`name_of`], [`has_kind`] and [`log`] work there.
#[macro_export]
macro_rules! client_init {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn client_init() {
            $f()
        }
    };
}

/// Exports `client_on_start`, calling `$f` once the match interface is built
/// (and again after a rejoining player's catch-up).
#[macro_export]
macro_rules! on_start {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn client_on_start() {
            $f()
        }
    };
}

/// Exports `client_on_render`, calling `$f(dt: f32, alpha: f32)` once per
/// render frame after the frame's events.
#[macro_export]
macro_rules! on_render {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn client_on_render(dt: f32, alpha: f32) {
            $f(dt, alpha)
        }
    };
}

/// Exports `client_on_events`, calling the handler listed for each record's
/// kind, in order; a handler takes `&ClientEventRecord`. Declares the
/// client delivery area ([`CLIENT_DELIVERY_SECTION`]).
///
/// ```ignore
/// vera20k_sdk::on_client_event!(
///     ClientEvent::Click => on_click,
///     ClientEvent::Notify => on_notify,
/// );
/// ```
#[macro_export]
macro_rules! on_client_event {
    ($($kind:path => $handler:path),* $(,)?) => {
        #[used]
        #[unsafe(link_section = "vera.delivery")]
        static __VERA_DELIVERY: [u8; 9] = $crate::client::CLIENT_DELIVERY_SECTION;

        #[unsafe(no_mangle)]
        /// # Safety
        /// Only the engine may call this export with its current delivery records.
        pub unsafe extern "C" fn client_on_events(ptr: i32, count: i32) {
            // SAFETY: these are the engine's own arguments, used only here.
            let records = unsafe { $crate::client::ClientEventRecord::batch(ptr, count) };
            for record in records {
                $(
                    if record.kind == $kind as i32 {
                        let handler: for<'event> fn(&$crate::client::ClientEventRecord<'event>) = $handler;
                        handler(&record);
                        continue;
                    }
                )*
            }
        }
    };
}
