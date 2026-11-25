//! Area `triggers` of the bench (see `lib.rs`). Marks are named `triggers.<point>`.
//!
//! Module triggers (RFC 0001 sections 6a and 7): their lifecycle and
//! bindings, the delivered, stock and polling events, filter clauses,
//! immediate actions and chains; the `action` import (every atomic action
//! the SDK has a function for, a few stock ones); house orders and their
//! results (6g, 6i); a domain claim (6i); an INI trigger (6h).
//!
//! Its own units stand in its arena (`crate::arena(TRIGGERS)`): player
//! units for the actions and events, a few enemy-owned ones (a weaponless
//! Rhino the duel's Grizzly shoots, two Rhinos the house orders drive, a
//! Yuri that mind-controls a player engineer), each removed again once its
//! part is done. The work runs on a schedule of frames after the arena
//! opens (`setup` for the units and triggers, `steps` for the actions);
//! `records` reads the delivered records.
//!
//! TeamTypes (`teams`): the bench's map adds two (`map.ini`); their names
//! round-trip through `lookup`/`name_of`, and stock 7, 80 and 107 bring
//! their members, stock 125 a building, each marked by its `Birth`.
//!
//! Not marked (left out of `MARKS`):
//! - `EndFor` and the `SuspendProduction` order: the finale (`finale.rs`)
//!   runs them once every area is done, since they end the match or pause
//!   the enemy's production the `movement` area relies on.
//! - `Push` and `SetFacing`: the `movement` area marks them with the
//!   position and facing they lead to.
//! - `Team` of a reinforcement's member: marked `triggers.prop-team` when
//!   the engine reads it, which this list does not count on.
//! - Stock cell events need a bound cell's row or column crossed by a foot;
//!   they are marked (any unit on those lines raises them), the others the
//!   engine does not take (24, 31, 33, 51, 53, 54, 59) are not tried.
//!
//! SDK gaps worked around here: none; the trigger properties `House` and
//! `Repeat` are read-only in this engine (a write answers `Refused`), which
//! `prop-house`/`prop-repeat` check.

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::ini::EventSlot;
use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get, set};

mod records;
mod setup;
mod steps;
mod teams;

// The crate's one `vera.actions` section: the names INI `Wasm:` actions
// address. Only `triggers` declares names.
vera20k_sdk::named_actions! {
    INI_DOG = "benchIniDog" => 290,
}

/// Every mark this area reaches in the bench scene.
pub const MARKS: &[&str] = &[
    // Lifecycle and bindings.
    "triggers.create",
    "triggers.bind",
    "triggers.unbind",
    "triggers.remove",
    "triggers.bind-type",
    "triggers.bind-weapon",
    "triggers.bind-warhead",
    "triggers.bind-projectile",
    "triggers.bind-cell",
    "triggers.prop-enabled",
    "triggers.prop-house",
    "triggers.prop-repeat",
    // Delivered events.
    "triggers.ev-take-damage",
    "triggers.ev-killed-warhead",
    "triggers.ev-fire",
    "triggers.ev-timer",
    "triggers.ev-timer-object",
    "triggers.ev-attacked-by",
    "triggers.ev-birth",
    "triggers.ev-transport-entered",
    "triggers.ev-passenger-added",
    "triggers.ev-transport-exited",
    "triggers.ev-passenger-removed",
    "triggers.ev-house-transferred",
    "triggers.ev-mind-control",
    "triggers.ev-sw-launched",
    "triggers.ev-mod-start",
    "triggers.ev-mod-stacks",
    "triggers.ev-mod-expire",
    "triggers.ev-module-event",
    "triggers.ev-module-global",
    // Stock events.
    "triggers.ev-stock-attacked",
    "triggers.ev-stock-destroyed",
    "triggers.ev-stock-house",
    "triggers.ev-stock-logic",
    "triggers.ev-cell-row",
    "triggers.ev-cell-column",
    // Polls.
    "triggers.poll-property",
    "triggers.poll-money",
    "triggers.poll-variable",
    "triggers.poll-power",
    "triggers.poll-modifier-count",
    "triggers.poll-distance",
    // Filter clauses.
    "triggers.filter-subject",
    "triggers.filter-other",
    "triggers.filter-house",
    // Immediate actions and chains.
    "triggers.imm-heal",
    "triggers.imm-add-money",
    "triggers.chain",
    // Atomic actions.
    "triggers.act-launch-sw",
    "triggers.act-remove",
    "triggers.act-kill",
    "triggers.act-damage",
    "triggers.act-create",
    "triggers.act-clear-target",
    "triggers.act-add-money",
    "triggers.act-set-owner",
    "triggers.act-set-var",
    "triggers.act-detonate-warhead",
    "triggers.act-log",
    "triggers.act-heal",
    "triggers.act-set-mission",
    "triggers.act-spawn-anim",
    "triggers.act-detonate-weapon",
    "triggers.act-add-experience",
    "triggers.act-add-ammo",
    "triggers.act-teleport",
    "triggers.act-clear-disguise",
    "triggers.act-clear-mind-control",
    "triggers.act-convert",
    "triggers.act-convert-timed",
    "triggers.act-set-score",
    "triggers.act-add-score",
    "triggers.act-set-property",
    "triggers.act-modify",
    "triggers.act-remove-modifiers",
    "triggers.act-fire-weapon",
    "triggers.act-attach-anim",
    "triggers.act-allow-type",
    "triggers.act-add-passenger",
    "triggers.act-raise-event",
    // Stock actions.
    "triggers.stock-timer",
    "triggers.stock-local",
    "triggers.stock-reveal",
    // House orders and their results.
    "triggers.order-move",
    "triggers.order-guard",
    "triggers.order-attack",
    "triggers.order-stop",
    "triggers.order-scatter",
    "triggers.order-queue",
    "triggers.order-rejected",
    // Claims.
    "triggers.claim",
    "triggers.claim-human",
    // INI triggers.
    "triggers.ini-trigger",
    // TeamTypes and the objects stock actions make (`teams`).
    "triggers.team-lookup",
    "triggers.ev-birth-reinforce",
    "triggers.ev-birth-reinforce-at",
    "triggers.ev-birth-chrono",
    "triggers.ev-birth-building",
];

// ------------------------------------------------------------------ Keys

/// Dispatch keys of this area's module actions (200..299).
const K_TAKE_DAMAGE: i32 = 200;
const K_KILLED: i32 = 201;
const K_FIRE: i32 = 202;
const K_TIMER: i32 = 203;
const K_TIMER_OBJECT: i32 = 204;
const K_ATTACKED_BY: i32 = 205;
const K_BIRTH: i32 = 206;
const K_ENTERED: i32 = 207;
const K_ADDED: i32 = 208;
const K_EXITED: i32 = 209;
const K_REMOVED: i32 = 210;
const K_TRANSFERRED: i32 = 211;
const K_SW_LAUNCHED: i32 = 212;
const K_MOD_START: i32 = 213;
const K_MOD_STACKS: i32 = 214;
const K_MOD_EXPIRE: i32 = 215;
const K_PING: i32 = 216;
const K_PONG: i32 = 217;
const K_GLOBAL_EVENT: i32 = 218;
const K_STOCK_ATTACKED: i32 = 219;
const K_STOCK_DESTROYED: i32 = 220;
const K_STOCK_HOUSE: i32 = 221;
const K_STOCK_LOGIC: i32 = 222;
const K_CELL_ROW: i32 = 223;
const K_CELL_COLUMN: i32 = 224;
const K_POLL_PROPERTY: i32 = 225;
const K_POLL_VARIABLE: i32 = 226;
const K_POLL_POWER: i32 = 227;
const K_POLL_MOD_COUNT: i32 = 228;
const K_POLL_DISTANCE: i32 = 229;
const K_POLL_MONEY: i32 = 230;
const K_FILTER_SUBJECT: i32 = 231;
const K_FILTER_OTHER: i32 = 232;
const K_FILTER_HOUSE_YES: i32 = 233;
const K_FILTER_HOUSE_NO: i32 = 234;
const K_UNBIND: i32 = 235;
const K_SET_VAR: i32 = 236;
const K_FIRE_WEAPON_TYPE: i32 = 237;
const K_FIRE_WARHEAD_TYPE: i32 = 238;
const K_FIRE_PROJECTILE_TYPE: i32 = 239;
const K_COMMANDED: i32 = 240;
const K_ORDER_REFUSED: i32 = 241;
const K_BIRTH_TEAM: i32 = 242;
const K_BIRTH_CHRONO: i32 = 243;
const K_BIRTH_BUILDING: i32 = 244;

// ----------------------------------------------------------------- State

/// Slots of the area's state (`ld`/`st`): handles, type references and
/// readings kept between frames. Module memory is saved, so a loaded game
/// keeps them.
const OPENED: usize = 0;
const ARENA: usize = 1;
// Type references (0: not in the rules).
const WEAPON_105: usize = 2;
const WARHEAD_AP: usize = 3;
const PROJECTILE_CANNON: usize = 4;
/// The `PIFF` animation's ID plus one.
const ANIM: usize = 5;
const SUPER: usize = 6;
const TYPE_FV: usize = 7;
const TYPE_HTNK: usize = 8;
const TYPE_E1: usize = 9;
const TYPE_E2: usize = 10;
const TYPE_ENGINEER: usize = 11;
// Units.
const SHOOTER: usize = 12;
const VICTIM: usize = 13;
const DUMMY: usize = 14;
const FOOT: usize = 15;
const KILLED: usize = 16;
const MOVER: usize = 17;
const CONVERTED: usize = 18;
const IFV: usize = 19;
const RIDER: usize = 20;
const SWITCHED: usize = 21;
const BAIT: usize = 22;
const ORDERED_1: usize = 23;
const ORDERED_2: usize = 24;
const YURI: usize = 25;
const CONTROLLED: usize = 26;
const MIRAGE: usize = 27;
const DOG: usize = 28;
// Triggers.
const TR_TIMER: usize = 29;
const TR_WEAPON: usize = 30;
const TR_WARHEAD: usize = 31;
const TR_PROJECTILE: usize = 32;
const TR_UNBIND: usize = 33;
const TR_HOUSE_NO: usize = 34;
const TR_SET_VAR: usize = 35;
const TR_TRANSFERRED: usize = 36;
// Readings and progress.
const PING_HEALTH: usize = 37;
const PING_MONEY: usize = 38;
const MODIFIER: usize = 39;
const CELL_ROW: usize = 40;
const CELL_COLUMN: usize = 41;
const MOVER_START: usize = 42;
const CLAIMED: usize = 43;
const SUPER_GRANTED: usize = 44;
const SUBJECT_YES: usize = 45;
const SUBJECT_NO: usize = 46;
const OTHER_YES: usize = 47;
const OTHER_NO: usize = 48;
const HOUSE_YES: usize = 49;
const HOUSE_NO: usize = 50;
const UNBIND_YES: usize = 51;
const UNBIND_NO: usize = 52;
/// The unit `Remove` took, checked on a later frame.
const REMOVED: usize = 53;
// TeamTypes (type references) and the types their `Birth` is bound to.
const TEAM: usize = 54;
const CHRONO_TEAM: usize = 55;
const TYPE_TNKD: usize = 56;
const TYPE_SNIPE: usize = 57;
const TYPE_GAP: usize = 58;
/// Which reinforcement the next Tank Destroyer's `Birth` belongs to.
const TEAM_PHASE: usize = 59;
const SLOTS: usize = 60;

static STATE: [AtomicI64; SLOTS] = [const { AtomicI64::new(0) }; SLOTS];

fn ld(slot: usize) -> i64 {
    STATE[slot].load(Relaxed)
}

fn st(slot: usize, value: i64) {
    STATE[slot].store(value, Relaxed);
}

/// Module events this area registers (`vera_init`).
static PING: EventSlot = EventSlot::new();
static PONG: EventSlot = EventSlot::new();
static GLOBAL_EVENT: EventSlot = EventSlot::new();

// --------------------------------------------------------------- Helpers

/// The cell `dx`, `dy` from the arena's corner.
fn at(dx: i32, dy: i32) -> Cell {
    crate::offset(Cell(ld(ARENA)), dx, dy)
}

fn unit(slot: usize) -> Techno {
    Techno(ld(slot))
}

fn kind(slot: usize) -> TypeRef {
    TypeRef(ld(slot))
}

fn trigger(slot: usize) -> TriggerRef {
    TriggerRef(ld(slot))
}

fn have(slot: usize) -> bool {
    ld(slot) != NO_HANDLE
}

/// A unit's health, −1 once it is gone.
fn health(unit: Techno) -> i64 {
    unit.health().unwrap_or(-1)
}

/// The player's money, −1 when it cannot be read.
fn money() -> i64 {
    get(crate::player(), Prop::Money).unwrap_or(-1)
}

/// Marks `name` when `ok`, else logs it as a wrong answer.
fn check(name: &str, ok: bool, why: &str) {
    if ok {
        crate::mark(name);
    } else {
        crate::bench_fail(name, why);
    }
}

/// Removes one of this area's units once its part is done.
fn retire(slot: usize) {
    if have(slot) && crate::alive(unit(slot)) {
        let _ = Action::remove(unit(slot));
    }
}

// --------------------------------------------------------------- Exports

/// `vera_init`: the module events this area raises (RFC 0001 section 6h).
pub fn init() {
    let events = [
        (&PING, "Bench.TrigPing", EventScope::Object),
        (&PONG, "Bench.TrigPong", EventScope::Object),
        (&GLOBAL_EVENT, "Bench.TrigAll", EventScope::Global),
    ];
    for (slot, name, scope) in events {
        match ini::event(name, scope) {
            Ok(event) => slot.set(event),
            Err(_) => crate::bench_fail("triggers.ev-module-event", "ini::event failed"),
        }
    }
}

/// `on_scenario_start`.
pub fn start() {}

/// The frame the arena opens: the houses and `crate::arena` are known.
pub fn open() {
    st(OPENED, i64::from(vera20k_sdk::frame()));
    st(ARENA, crate::arena(crate::TRIGGERS).0);
    setup::lookups();
}

/// Every later frame.
pub fn frame(now: i32) {
    let t = i64::from(now) - ld(OPENED);
    setup::step(t);
    steps::step(t);
    teams::step(t);
}

/// Records whose key is in this area's range.
pub fn events(record: &EventRecord) {
    records::dispatch(record);
}

/// Decisions whose key is in this area's range: this area makes none.
pub fn decide(_record: &mut DecideRecord) {}
