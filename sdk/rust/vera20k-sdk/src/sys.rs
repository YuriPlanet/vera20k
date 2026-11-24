//! The raw `vera:sim@1.0` imports and fixed layouts (RFC 0001 appendix,
//! "Fixed layouts" and 1.1). Every import returns its status and writes a
//! value through a trailing out pointer; `frame` alone returns its value.
//!
//! A module imports only what it calls: the linker drops unused imports, so
//! declaring all of them here costs nothing.

#[link(wasm_import_module = "vera:sim@1.0")]
unsafe extern "C" {
    pub fn frame() -> i32;
    pub fn random(lo: i32, hi: i32, out: *mut i32) -> i32;
    pub fn log(ptr: *const u8, len: i32);
    pub fn has_kind(table: i32, id: i32) -> i32;
    pub fn lookup(kind: i32, ptr: *const u8, len: i32, out: *mut i32) -> i32;
    pub fn name_of(kind: i32, id: i32, out: *mut u8, cap: i32, len_out: *mut i32) -> i32;
    pub fn get(reference: i64, prop: i32, out: *mut i64) -> i32;
    pub fn get_many(
        refs: *const i64,
        n: i32,
        props: *const i32,
        m: i32,
        out: *mut i64,
        count_out: *mut i32,
    ) -> i32;
    pub fn set(reference: i64, prop: i32, value: i64) -> i32;
    pub fn modify(
        reference: i64,
        prop: i32,
        op: i32,
        value: i64,
        frames: i32,
        source: i64,
        group: i32,
        out: *mut i64,
    ) -> i32;
    pub fn unmodify(modifier: i64) -> i32;
    pub fn rules_get(
        sec: *const u8,
        sec_len: i32,
        key: *const u8,
        key_len: i32,
        out: *mut u8,
        cap: i32,
        len_out: *mut i32,
    ) -> i32;
    pub fn rules_set(
        sec: *const u8,
        sec_len: i32,
        key: *const u8,
        key_len: i32,
        val: *const u8,
        val_len: i32,
    ) -> i32;
    pub fn var_get(scope: i64, var: i32, out: *mut i64) -> i32;
    pub fn var_set(scope: i64, var: i32, value: i64) -> i32;
    pub fn find(query: *const u8, len: i32, out: *mut i64, cap: i32, res: *mut FindResult) -> i32;
    pub fn trigger_create(def: *const u8, len: i32, out: *mut i64) -> i32;
    pub fn trigger_remove(trigger: i64) -> i32;
    pub fn trigger_bind(trigger: i64, reference: i64) -> i32;
    pub fn trigger_unbind(trigger: i64, reference: i64) -> i32;
    pub fn action(
        kind: i32,
        target: i64,
        params: *const ActionParams,
        len: i32,
        out: *mut i64,
    ) -> i32;
    pub fn send_notify(to: i64, id: i32, ptr: *const u8, len: i32) -> i32;
    pub fn order(house: i64, ptr: *const u8, len: i32) -> i32;
    /// Only inside `vera_init` (RFC 0001 section 6h).
    pub fn ini_register(
        kind: i32,
        section: i32,
        name: *const u8,
        len: i32,
        key: i32,
        out: *mut i32,
    ) -> i32;
}

/// `ActionParams`, 72 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ActionParams {
    pub id: i32,
    pub arg: i32,
    pub arg2: i32,
    pub reserved: i32,
    pub value: i64,
    pub value2: i64,
    pub cell: i64,
    pub house: i64,
    pub other: i64,
    pub text: i32,
    pub text_len: i32,
    pub extra: i32,
    pub extra_len: i32,
}

/// `Clause`, 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Clause {
    pub subject: u8,
    pub op: u8,
    pub scope: u8,
    pub reserved: u8,
    pub prop: i32,
    pub value: i64,
}

/// `Query`, 64 bytes; `clause_count` clauses follow it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Query {
    pub scope: i32,
    pub area: i32,
    pub cell: i64,
    pub radius: i32,
    pub height: i32,
    pub type_: i64,
    pub owner: i64,
    pub viewpoint: i64,
    pub after: i64,
    pub clause_count: i32,
    pub reserved: i32,
}

/// `FindResult`, 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct FindResult {
    pub count: i32,
    pub reserved: i32,
    pub next: i64,
}

/// `TriggerDef`, 32 bytes; `event_count` events (each an [`EventDef`] and
/// its clauses), then `action_count` [`ActionDef`]s follow it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct TriggerDef {
    pub house: i64,
    pub repeat: i32,
    pub difficulty: i32,
    pub flags: i32,
    pub decide_key: i32,
    pub event_count: i32,
    pub action_count: i32,
}

/// `EventDef`, 32 bytes; `clause_count` [`Clause`]s follow it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct EventDef {
    pub kind: i32,
    pub op: i32,
    pub interval: i32,
    pub id: i32,
    pub value: i64,
    pub clause_count: i32,
    pub reserved: i32,
}

/// `ActionDef` with its `ActionParams`, 80 bytes.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ActionDef {
    pub kind: i32,
    pub target: i32,
    pub params: ActionParams,
}

/// `EventRecord`, 120 bytes, read in place in `on_events`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct EventRecord {
    pub key: i32,
    pub kind: i32,
    pub trigger: i64,
    pub holder: i64,
    pub subject: i64,
    pub other: i64,
    pub house: i64,
    pub projectile: i64,
    pub modifier: i64,
    pub cell: i64,
    pub value: i64,
    pub value2: i64,
    pub id: i32,
    pub frame: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// String ID (`name_of(String, ..)`): a module action's parameter text,
    /// an `IniEntry` entry name, an `IniKey` value; 0 for none.
    pub args: i32,
    /// A module action's `ActionTarget`; 0 on other records.
    pub target: i32,
    pub reserved: i32,
}

/// `DecideRecord`, 160 bytes, read and written in place in `on_decide`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct DecideRecord {
    pub event: EventRecord,
    pub native: i64,
    pub previous: i64,
    pub result: i64,
    pub handled: i32,
    pub area: i32,
    pub area_cap: i32,
    pub previous_len: i32,
}

const _: () = {
    assert!(size_of::<ActionParams>() == 72);
    assert!(size_of::<Clause>() == 16);
    assert!(size_of::<Query>() == 64);
    assert!(size_of::<FindResult>() == 16);
    assert!(size_of::<TriggerDef>() == 32);
    assert!(size_of::<EventDef>() == 32);
    assert!(size_of::<ActionDef>() == 80);
    assert!(size_of::<EventRecord>() == 120);
    assert!(size_of::<DecideRecord>() == 160);
    // The offsets the engine writes (RFC 0001 appendix, fixed layouts).
    assert!(core::mem::offset_of!(EventRecord, args) == 108);
    assert!(core::mem::offset_of!(EventRecord, target) == 112);
    assert!(core::mem::offset_of!(DecideRecord, native) == 120);
    assert!(core::mem::offset_of!(DecideRecord, handled) == 144);
    assert!(core::mem::offset_of!(DecideRecord, previous_len) == 156);
};
