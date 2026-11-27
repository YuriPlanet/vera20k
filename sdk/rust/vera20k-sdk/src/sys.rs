//! The raw `vera:sim@1.0` and `vera:client@1.0` imports and fixed layouts
//! (RFC 0001 appendix, "Fixed layouts", 1.1 and 2.1). Every import returns
//! its status and writes a value through a trailing out pointer; `frame` and
//! `client_frame` alone return their value. A simulation module calls only
//! the first block, a client module only the second: the engine refuses a
//! module importing the other world's functions.
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

#[link(wasm_import_module = "vera:client@1.0")]
unsafe extern "C" {
    pub fn client_frame() -> i32;
    pub fn client_random(lo: i32, hi: i32, out: *mut i32) -> i32;
    pub fn client_log(ptr: *const u8, len: i32);
    pub fn client_has_kind(table: i32, id: i32) -> i32;
    pub fn client_lookup(kind: i32, ptr: *const u8, len: i32, out: *mut i32) -> i32;
    pub fn client_name_of(kind: i32, id: i32, out: *mut u8, cap: i32, len_out: *mut i32) -> i32;
    pub fn client_get(reference: i64, prop: i32, out: *mut i64) -> i32;
    pub fn client_get_many(
        refs: *const i64,
        n: i32,
        props: *const i32,
        m: i32,
        out: *mut i64,
        count_out: *mut i32,
    ) -> i32;
    pub fn client_find(
        query: *const u8,
        len: i32,
        out: *mut i64,
        cap: i32,
        res: *mut FindResult,
    ) -> i32;
    pub fn client_var_get(scope: i64, var: i32, out: *mut i64) -> i32;
    pub fn client_set(reference: i64, prop: i32, value: i64) -> i32;
    pub fn client_set_bytes(reference: i64, prop: i32, ptr: *const u8, len: i32) -> i32;
    pub fn client_node_create(
        kind: i32,
        parent: i64,
        props: *const PropValue,
        len: i32,
        out: *mut i64,
    ) -> i32;
    pub fn client_node_remove(node: i64) -> i32;
    pub fn client_node_find(ptr: *const u8, len: i32, out: *mut i64) -> i32;
    pub fn client_action(kind: i32, target: i64, params: *const ActionParams, len: i32) -> i32;
    pub fn client_send_order(ptr: *const u8, len: i32) -> i32;
    pub fn client_send_message(ptr: *const u8, len: i32) -> i32;
    pub fn client_storage_read(
        key: *const u8,
        key_len: i32,
        out: *mut u8,
        cap: i32,
        len_out: *mut i32,
    ) -> i32;
    pub fn client_storage_write(
        key: *const u8,
        key_len: i32,
        data: *const u8,
        data_len: i32,
    ) -> i32;
    pub fn client_status(ptr: *const u8, len: i32) -> i32;
}

/// `ClientEventRecord`, 32 bytes, read in place in `client_on_events`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct ClientEventRecord {
    pub kind: i32,
    /// `Notify`: the sending module's source, 0 mod, 1 map.
    pub source: i32,
    pub reference: i64,
    pub id: i32,
    pub value: i32,
    /// `Notify`: its bytes; `SimEvent`: one `EventRecord`; both in the
    /// delivery area.
    pub data: i32,
    pub data_len: i32,
}

/// `PropValue`, 16 bytes: an initial property of `client_node_create`.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct PropValue {
    pub prop: i32,
    pub reserved: i32,
    pub value: i64,
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

/// `ProjectileSpec`, 48 bytes: one entry of a `Fire` shot plan.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ProjectileSpec {
    /// Projectile type ID.
    pub projectile_type: i32,
    /// Weapon ID giving damage and warhead; −1 for the shot's own weapon.
    pub weapon: i32,
    /// Object or cell reference; 0 for the shot's target.
    pub target: i64,
    /// Initial `Data0` (bytes 0..7) and `Data1` (8..15).
    pub data: [u8; 16],
    /// Bit 0: `origin` is absolute, otherwise an offset from the native
    /// launch position.
    pub flags: i32,
    /// x, y, z in leptons.
    pub origin: [i32; 3],
}

/// `PathSpec`'s 32-byte header; `point_count` points of three `i64`
/// leptons follow it.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PathSpecHeader {
    /// `PathKind`.
    pub kind: i32,
    pub point_count: i32,
    /// Leptons per frame, 16 fraction bits; 0 for the projectile's speed.
    pub speed: i64,
    pub param: [i64; 2],
}

/// `ProjectileStep`, 56 bytes: `ProjectileFrame`'s result.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct ProjectileStep {
    /// Bit 0 detonate, 1 set velocity, 2 set position, 3 set module data.
    pub flags: i32,
    pub reserved: i32,
    /// Leptons per frame, 16 fraction bits.
    pub velocity: [i32; 3],
    pub reserved2: i32,
    /// Leptons.
    pub position: [i32; 3],
    pub reserved3: i32,
    pub data: [u8; 16],
}

/// `MissionStepResult`, 24 bytes: `MissionStep`'s result. It comes holding
/// the mission body's outcome (or the previous link's answer).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MissionStepResult {
    /// Object or cell reference, 0 for none.
    pub target: i64,
    /// Object or cell reference, 0 for none.
    pub destination: i64,
    /// `Mission` number to queue, −1 for none.
    pub mission: i32,
    /// Frames until the mission body runs again (at least 1).
    pub delay: i32,
}

/// `MoveStep`, 16 bytes: `MoveFrame`'s result.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MoveStep {
    /// Bit 0 take the frame and move by `displacement`, 1 turn to
    /// `facing`, 2 stop the native movement.
    pub flags: i32,
    /// Body facing 0..255.
    pub facing: i32,
    /// x, y in leptons, each at most 2,048 either way.
    pub displacement: [i32; 2],
}

/// `PathCell`, 8 bytes: one cell of a `PathFound` route.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PathCell {
    pub x: i32,
    pub y: i32,
}

const _: () = {
    assert!(size_of::<MissionStepResult>() == 24);
    assert!(size_of::<MoveStep>() == 16);
    assert!(size_of::<PathCell>() == 8);
    assert!(size_of::<ProjectileSpec>() == 48);
    assert!(core::mem::offset_of!(ProjectileSpec, flags) == 32);
    assert!(size_of::<PathSpecHeader>() == 32);
    assert!(size_of::<ProjectileStep>() == 56);
    assert!(core::mem::offset_of!(ProjectileStep, data) == 40);
    assert!(size_of::<ActionParams>() == 72);
    assert!(size_of::<Clause>() == 16);
    assert!(size_of::<Query>() == 64);
    assert!(size_of::<FindResult>() == 16);
    assert!(size_of::<TriggerDef>() == 32);
    assert!(size_of::<EventDef>() == 32);
    assert!(size_of::<ActionDef>() == 80);
    assert!(size_of::<EventRecord>() == 120);
    assert!(size_of::<DecideRecord>() == 160);
    assert!(size_of::<ClientEventRecord>() == 32);
    assert!(core::mem::offset_of!(ClientEventRecord, data) == 24);
    assert!(size_of::<PropValue>() == 16);
    // The offsets the engine writes (RFC 0001 appendix, fixed layouts).
    assert!(core::mem::offset_of!(EventRecord, args) == 108);
    assert!(core::mem::offset_of!(EventRecord, target) == 112);
    assert!(core::mem::offset_of!(DecideRecord, native) == 120);
    assert!(core::mem::offset_of!(DecideRecord, handled) == 144);
    assert!(core::mem::offset_of!(DecideRecord, previous_len) == 156);
};
