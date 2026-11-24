//! INI names a module registers (RFC 0001 section 6h): keys on type
//! sections, registries (list sections) and module events, so a modder
//! writes INI and the module implements it.
//!
//! Register in the module's initialization function ([`crate::init!`]); a
//! registration anywhere else stops the module. Before the module's
//! `on_scenario_start`, `on_events` receives every `IniEntry` record, then
//! every `IniKey` record, each with the dispatch key given here, so the
//! handlers are listed in [`crate::on_events!`] beside the module's
//! actions. Values are the final merged rules text; the engine does not
//! check them.
//!
//! ```ignore
//! use vera20k_sdk::prelude::*;
//!
//! const SHIELD_TYPES: i32 = 10;
//! const SHIELD: i32 = 11;
//! static SHIELD_BROKEN: ini::EventSlot = ini::EventSlot::new();
//!
//! fn init() {
//!     let _ = ini::registry("Kratos.ShieldTypes", SHIELD_TYPES);
//!     let _ = ini::key("Kratos.Shield", IniSection::TechnoType, SHIELD);
//!     if let Ok(event) = ini::event("Kratos.ShieldBroken", EventScope::Object) {
//!         SHIELD_BROKEN.set(event);
//!     }
//! }
//!
//! fn load_shield_type(r: IniEntryRecord) {
//!     let mut name = [0u8; 64];
//!     let mut value = [0u8; 32];
//!     if let Ok(section) = r.entry_name(&mut name) {
//!         let strength = Rules::get(section, "Strength", &mut value).ok().and_then(ini::int);
//!         // ...
//!     }
//! }
//!
//! fn attach_shield(r: IniKeyRecord) {
//!     let mut value = [0u8; 64];
//!     let shield = r.value(&mut value); // "HeavyShield" on the type r.type_ref()
//!     // ...
//! }
//!
//! vera20k_sdk::init!(init);
//! vera20k_sdk::on_events!(SHIELD_TYPES => load_shield_type, SHIELD => attach_shield);
//! ```

use core::sync::atomic::{AtomicI32, Ordering};

use crate::triggers::view;
use crate::{
    EventRecord, Global, House, NameKind, Ref, Result, Status, Techno, TypeRef, check, lookup,
    name_of_raw, sys,
};

/// The section kind a registered key sits in (`IniSection`).
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IniSection {
    TechnoType = 0,
    Weapon = 1,
    Warhead = 2,
    Projectile = 3,
    SuperWeapon = 4,
    Country = 5,
    /// Only `[General]`.
    General = 6,
}

/// Where a module event happens.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventScope {
    /// On an object: triggers bound to it (or its type) fire.
    Object = 0,
    /// Once for the game: unbound triggers fire.
    Global = 1,
}

/// `IniKind`.
const KEY: i32 = 0;
const REGISTRY: i32 = 1;
const EVENT: i32 = 2;

fn register(kind: i32, section: i32, name: &str, key: i32) -> Result<i32> {
    let mut out = 0;
    check(unsafe {
        sys::ini_register(
            kind,
            section,
            name.as_ptr(),
            name.len() as i32,
            key,
            &mut out,
        )
    })?;
    Ok(out)
}

/// Registers key `name` on `section` sections: before `on_scenario_start`,
/// every such section holding `name=` sends one [`IniKeyRecord`] with
/// dispatch key `key`. Names are 1 to 63 ASCII letters, digits, `.` and `_`,
/// compared ignoring case. `Err(Refused)` when another registration (any
/// source) has the name, or it starts with `Vera.`.
pub fn key(name: &str, section: IniSection, key: i32) -> Result<()> {
    register(KEY, section as i32, name, key).map(drop)
}

/// Registers the list section `[name]`: before `on_scenario_start`, each
/// entry in it (an entry section's name; the first of duplicates) sends one
/// [`IniEntryRecord`] with dispatch key `key`, in list order. Refused as
/// [`key`] is.
pub fn registry(name: &str, key: i32) -> Result<()> {
    register(REGISTRY, 0, name, key).map(drop)
}

/// Registers module event `name`, which INI triggers use as
/// `Event=Wasm:name` and code as [`crate::Trigger::on_module_event`], and
/// returns its ID. Refused as [`key`] is.
pub fn event(name: &str, scope: EventScope) -> Result<ModuleEvent> {
    register(EVENT, scope as i32, name, -1).map(ModuleEvent)
}

/// A module event's ID, from [`event`] or [`ModuleEvent::named`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleEvent(pub i32);

impl ModuleEvent {
    /// The event any source registered as `name` (another module's too).
    pub fn named(name: &str) -> Result<Self> {
        lookup(NameKind::ModuleEvent, name).map(Self)
    }
}

/// A module event ID kept in module memory, set once in initialization:
/// `static BROKEN: EventSlot = EventSlot::new();`. Saves keep it.
pub struct EventSlot(AtomicI32);

impl EventSlot {
    pub const fn new() -> Self {
        Self(AtomicI32::new(-1))
    }

    pub fn set(&self, event: ModuleEvent) {
        self.0.store(event.0, Ordering::Relaxed);
    }

    /// The event; `ModuleEvent(-1)` before [`EventSlot::set`].
    pub fn get(&self) -> ModuleEvent {
        ModuleEvent(self.0.load(Ordering::Relaxed))
    }
}

impl Default for EventSlot {
    fn default() -> Self {
        Self::new()
    }
}

view!(
    /// `IniEntry`: one entry of a registered list section.
    IniEntryRecord {
        /// The entry's position in the merged list, from 0.
        position: i64 = |r| r.value,
    }
);

impl IniEntryRecord<'_> {
    /// The entry section's name, written into `buf`; read its keys with
    /// [`crate::Rules::get`]. `Err(Refused)` when `buf` is too small.
    pub fn entry_name<'b>(&self, buf: &'b mut [u8]) -> Result<&'b str> {
        self.0.args_text(buf)
    }
}

view!(
    /// `IniKey`: one section holding a registered key.
    IniKeyRecord {
        /// The section's type (`TypeRef::id` within its kind); the global
        /// rules reference for `[General]` ([`IniKeyRecord::is_general`]).
        type_ref: TypeRef = |r| TypeRef(r.subject),
        /// Whether the key is in `[General]`.
        is_general: bool = |r| r.subject == Global.raw(),
    }
);

impl IniKeyRecord<'_> {
    /// The key's merged value text, written into `buf`. `Err(Refused)` when
    /// `buf` is too small.
    pub fn value<'b>(&self, buf: &'b mut [u8]) -> Result<&'b str> {
        self.0.args_text(buf)
    }

    /// The section's name, written into `buf`, for a message that names
    /// the section and key of a bad value.
    pub fn section_name<'b>(&self, buf: &'b mut [u8]) -> Result<&'b str> {
        if self.is_general() {
            let general = "General";
            let out = buf.get_mut(..general.len()).ok_or(Status::Refused)?;
            out.copy_from_slice(general.as_bytes());
            return core::str::from_utf8(out).map_err(|_| Status::Unsupported);
        }
        // Bits 32..48 of a type reference hold its name kind.
        let kind = (self.0.subject >> 32) as i32 & 0xFFFF;
        name_of_raw(kind, TypeRef(self.0.subject).id(), buf)
    }
}

view!(
    /// `ModuleEvent`: a raised module event, with what `RaiseEvent` gave.
    ModuleEventRecord {
        event: ModuleEvent = |r| ModuleEvent(r.id),
        /// The object an object event was raised on.
        subject: Techno = |r| Techno(r.subject),
        other: Techno = |r| Techno(r.other),
        value: i64 = |r| r.value,
        value2: i64 = |r| r.value2,
        house: House = |r| House(r.house),
    }
);

// --------------------------------------------------------------- Parsing

/// The comma-separated parts of `text`, each without surrounding ASCII
/// spaces: `split("Subject, 50")` gives `"Subject"`, `"50"`.
pub fn split(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').map(str::trim_ascii)
}

/// A decimal integer with an optional sign, surrounding spaces ignored;
/// `None` otherwise.
pub fn int(text: &str) -> Option<i64> {
    let text = text.trim_ascii();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for byte in digits.bytes() {
        let digit = byte.checked_sub(b'0').filter(|d| *d <= 9)?;
        value = value.checked_mul(10)?.checked_add(i64::from(digit))?;
    }
    Some(if negative { -value } else { value })
}

/// An INI yes/no as the game reads it: first letter `y`, `t` or `1` is
/// true, `n`, `f` or `0` false; `None` otherwise.
pub fn yes_no(text: &str) -> Option<bool> {
    match text.trim_ascii().as_bytes().first()? {
        b'y' | b'Y' | b't' | b'T' | b'1' => Some(true),
        b'n' | b'N' | b'f' | b'F' | b'0' => Some(false),
        _ => None,
    }
}

/// The first `N` comma-separated integers of `text` ([`split`], [`int`]);
/// a missing or malformed part is `None`.
pub fn ints<const N: usize>(text: &str) -> [Option<i64>; N] {
    let mut out = [None; N];
    for (slot, part) in out.iter_mut().zip(split(text)) {
        *slot = int(part);
    }
    out
}
