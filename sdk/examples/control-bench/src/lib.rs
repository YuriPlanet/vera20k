//! "Control Bench", a benchmark `rules.wasm` (RFC 0001 phase 1): one module
//! that reaches nearly every control point the engine offers and logs a line
//! `DEMO-OK <mark>` the first time each one works. It is a test fixture, not
//! a game: it spawns its own units next to the player's start and drives them
//! itself, so a Battle against one computer house with nobody at the controls
//! is enough.
//!
//! The engine test `control_bench_reaches_every_mark` loads it the way a
//! mod loads (`VERA20K_TEST_CONTROL_BENCH`, see `README.zh-CN.md`): its
//! `rules.wasm` and `rulesmd.ini` in a mod's folder, with the bench's map.
//! A missing mark names the control point that stopped working; the
//! `BENCH-MISSING` lines list them.
//!
//! The work is split by area, one file each, so the marks stay independent:
//! - `basics`: host calls, properties, writes, variables, searches,
//!   modifiers of every wired stat, `ini_register`;
//! - `triggers`: trigger events and polls, atomic and stock actions, chains,
//!   house orders and their results, claims, INI triggers;
//! - `combat`: the combat decisions (damage, death, fire, projectiles);
//! - `movement`: production, command, movement, path and unit AI decisions;
//! - `finale`: once every area is done (or at frame 4800), the points that
//!   pause or end the match (`SuspendProduction`, `Defeat`, `EndFor`); the
//!   areas' frame work, records and decisions stop then.
//!
//! `lib` owns the exports, the dispatch, the marks, the arena and the rules
//! stage (`on_rules`, marks `rules.*`).
#![no_std]

mod basics;
mod combat;
mod finale;
mod movement;
mod triggers;

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

// ------------------------------------------------------------------ Keys

/// Each area owns a range of trigger keys (`Trigger::enqueue`,
/// `decide_sync`, INI registrations): `basics` 100..199, `triggers`
/// 200..299, `combat` 300..399, `movement` 400..499, `finale` 500..599.
/// `on_events` and `on_decide` route a record by its key's hundred.
pub const BASICS: i32 = 1;
pub const TRIGGERS: i32 = 2;
pub const COMBAT: i32 = 3;
pub const MOVEMENT: i32 = 4;
pub const FINALE: i32 = 5;

fn area_of(key: i32) -> i32 {
    key / 100
}

// ----------------------------------------------------------------- Marks

const MAX_MARKS: usize = 1024;
/// An open-addressed set of name hashes: `marked` and `mark` probe from the
/// hash's own slot, so a check costs a few operations however many marks
/// there are (the progress report checks every mark in one frame).
static MARKED: [AtomicI64; MAX_MARKS] = [const { AtomicI64::new(0) }; MAX_MARKS];

fn hash(name: &str) -> i64 {
    // FNV-1a, never 0 (0 is a free slot).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h | 1) as i64
}

/// Adds `h` to the set; false when it was there already (or the set is full).
fn insert(h: i64) -> bool {
    let first = (h as u64 >> 1) as usize % MAX_MARKS;
    for step in 0..MAX_MARKS {
        let slot = &MARKED[(first + step) % MAX_MARKS];
        match slot.load(Relaxed) {
            0 => {
                slot.store(h, Relaxed);
                return true;
            }
            v if v == h => return false,
            _ => {}
        }
    }
    false
}

fn contains(h: i64) -> bool {
    let first = (h as u64 >> 1) as usize % MAX_MARKS;
    for step in 0..MAX_MARKS {
        match MARKED[(first + step) % MAX_MARKS].load(Relaxed) {
            0 => return false,
            v if v == h => return true,
            _ => {}
        }
    }
    false
}

/// Logs `DEMO-OK <name>` the first time `name` is marked. Marks are short
/// ASCII names, unique across the areas (each area prefixes its own:
/// `rules.`, `basics.`, `triggers.`, `combat.`, `movement.`, `finale.`).
pub fn mark(name: &str) {
    if insert(hash(name)) {
        let mut line = Line::new();
        line.text("DEMO-OK ").text(name);
        line.log();
    }
}

/// Whether `name` was marked.
pub fn marked(name: &str) -> bool {
    contains(hash(name))
}

/// Logs `BENCH-FAIL <name>: <why>` once per name, for a control point that
/// answered wrongly (a mark that never comes says only that it was not
/// reached).
pub fn bench_fail(name: &str, why: &str) {
    if insert(hash(name) ^ 0x5a5a) {
        let mut line = Line::new();
        line.text("BENCH-FAIL ").text(name).text(": ").text(why);
        line.log();
    }
}

/// A log line built without `core::fmt` (its encoding fails the load
/// checks): text and numbers appended into a fixed buffer.
pub struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

impl Line {
    pub const fn new() -> Self {
        Self {
            buf: [0; 160],
            len: 0,
        }
    }

    pub fn text(&mut self, s: &str) -> &mut Self {
        for &b in s.as_bytes() {
            if self.len == self.buf.len() {
                break;
            }
            self.buf[self.len] = b;
            self.len += 1;
        }
        self
    }

    pub fn num(&mut self, value: i64) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut n = value.unsigned_abs();
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        if value < 0 {
            self.text("-");
        }
        // SAFETY: ASCII digits.
        self.text(unsafe { core::str::from_utf8_unchecked(&digits[at..]) })
    }

    pub fn log(&self) {
        // Byte-wise copy of ASCII and UTF-8 text cut at a byte boundary may
        // split a character; marks and reasons are ASCII.
        let text = core::str::from_utf8(&self.buf[..self.len]).unwrap_or("");
        log(text);
    }
}

// ----------------------------------------------------------------- Arena

static PLAYER: AtomicI64 = AtomicI64::new(NO_HANDLE);
static ENEMY: AtomicI64 = AtomicI64::new(NO_HANDLE);
static BASE: AtomicI64 = AtomicI64::new(NO_HANDLE);

/// The one human house.
pub fn player() -> House {
    House(PLAYER.load(Relaxed))
}

/// The first computer house.
pub fn enemy() -> House {
    House(ENEMY.load(Relaxed))
}

/// The player's starting cell (their first object, the MCV).
pub fn base() -> Cell {
    Cell(BASE.load(Relaxed))
}

pub fn offset(cell: Cell, dx: i32, dy: i32) -> Cell {
    let x = (i32::from(cell.x()) + dx).clamp(1, 510) as u16;
    let y = (i32::from(cell.y()) + dy).clamp(1, 510) as u16;
    Cell::at(x, y)
}

/// Each area's own corner of the map, a few cells out from the player's
/// start in its own direction, so one area's fights stay out of another's.
pub fn arena(area: i32) -> Cell {
    let (dx, dy) = match area {
        BASICS => (-10, -10),
        TRIGGERS => (10, -10),
        COMBAT => (-10, 10),
        _ => (10, 10),
    };
    offset(base(), dx, dy)
}

/// Creates a unit of type `id` for `house` on `spot` or the closest ring of
/// cells that takes it.
pub fn spawn(spot: Cell, id: &str, house: House) -> Option<Techno> {
    let ty = TypeRef::techno_type(id).ok()?;
    for ring in 0..=6i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                if let Ok(unit) = Action::create_object(offset(spot, dx, dy), ty, house) {
                    return Some(unit);
                }
            }
        }
    }
    None
}

pub fn alive(unit: Techno) -> bool {
    unit.0 != NO_HANDLE && unit.health().is_ok_and(|health| health > 0)
}

fn find_sides() -> bool {
    let mut humans = [0i64; 2];
    let Ok((1, _)) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 1)
        .page(&mut humans, 0)
    else {
        return false;
    };
    let player = House(humans[0]);
    let first = Find::objects().owned_by(player).first();
    let Ok(cell) = get(Techno(first), Prop::Cell) else {
        return false;
    };
    let mut computers = [0i64; 16];
    let count = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .where_prop(Prop::IsDefeated, Op::Eq, 0)
        .page(&mut computers, 0)
        .map_or(0, |(count, _)| count);
    // The first computer house that owns something: the opponent, not a
    // neutral or special house.
    let Some(&enemy) = computers[..count]
        .iter()
        .find(|&&h| Find::objects().owned_by(House(h)).first() != NO_HANDLE)
    else {
        return false;
    };
    PLAYER.store(player.0, Relaxed);
    ENEMY.store(enemy, Relaxed);
    BASE.store(cell, Relaxed);
    true
}

// ------------------------------------------------------------ Rules stage

/// The rules stage's marks: `vera_init` ran before `on_rules` (both run
/// while the rules are processed), `Rules::get` reads a retail key,
/// TechnoType names and their properties answer there, and a
/// `Rules::set` of a type's `Cost` is what the match's type reads.
const RULES_MARKS: &[&str] = &["rules.init", "rules.get", "rules.type-lookup", "rules.set"];

/// The type and the cost `on_rules` gives it. No one builds the Yuri
/// Gattling tank in the bench's match.
const SET_TYPE: &str = "YTNK";
const SET_COST: i64 = 1234;
const SET_COST_TEXT: &str = "1234";

/// `vera_init` ran; `on_rules` wrote the cost.
static INIT_RAN: AtomicI64 = AtomicI64::new(0);
static COST_SET: AtomicI64 = AtomicI64::new(0);

/// A decimal number, `None` for other text.
fn parse(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for b in text.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(i64::from(b - b'0'))?;
    }
    Some(value)
}

fn rules_stage() {
    if INIT_RAN.load(Relaxed) != 0 && frame() == 0 {
        mark("rules.init");
    }
    let mut buf = [0u8; 32];
    // The GI's Strength, as text and as the type's property.
    let gi = TypeRef::techno_type("E1");
    let text = Rules::get("E1", "Strength", &mut buf).ok().and_then(parse);
    match (gi, text) {
        (Ok(gi), Some(text)) if get(gi, Prop::Strength) == Ok(text) && text > 0 => {
            mark("rules.get");
        }
        _ => bench_fail("rules.get", "[E1] Strength did not read as the type's"),
    }
    let Ok(kind) = TypeRef::techno_type(SET_TYPE) else {
        return bench_fail("rules.type-lookup", "YTNK is not a TechnoType here");
    };
    match get(kind, Prop::Cost) {
        Ok(cost) if cost > 0 && cost != SET_COST => mark("rules.type-lookup"),
        _ => return bench_fail("rules.type-lookup", "YTNK's Cost did not read"),
    }
    let written = Rules::set(SET_TYPE, "Cost", SET_COST_TEXT).is_ok()
        && Rules::get(SET_TYPE, "Cost", &mut buf).ok().and_then(parse) == Some(SET_COST);
    if written {
        COST_SET.store(1, Relaxed);
    } else {
        bench_fail("rules.set", "the written Cost did not read back");
    }
}

/// On the arena's frame: the match's type has the cost `on_rules` wrote.
fn rules_check() {
    if COST_SET.load(Relaxed) == 0 {
        return;
    }
    let cost = TypeRef::techno_type(SET_TYPE).and_then(|kind| get(kind, Prop::Cost));
    if cost == Ok(SET_COST) {
        mark("rules.set");
    } else {
        bench_fail("rules.set", "the match's YTNK has another Cost");
    }
}

// --------------------------------------------------------------- Exports

/// The frame the arena opens on: the starting units are on the map by the
/// second frame.
const OPEN_FRAME: i32 = 2;
static OPEN: AtomicI64 = AtomicI64::new(0);

fn init() {
    INIT_RAN.store(1, Relaxed);
    basics::init();
    triggers::init();
    combat::init();
    movement::init();
}

fn start() {
    basics::start();
    triggers::start();
    combat::start();
    movement::start();
}

fn tick() {
    let now = frame();
    if OPEN.load(Relaxed) == 0 {
        if now < OPEN_FRAME {
            return;
        }
        if !find_sides() {
            OPEN.store(-1, Relaxed);
            return log("BENCH-FAIL arena: needs one human house and a computer opponent");
        }
        OPEN.store(i64::from(now), Relaxed);
        rules_check();
        basics::open();
        triggers::open();
        combat::open();
        movement::open();
        return;
    }
    if OPEN.load(Relaxed) < 0 {
        return;
    }
    if finale_started() {
        finale::frame(now);
        if finale::done() {
            report(now);
        }
    } else {
        basics::frame(now);
        triggers::frame(now);
        combat::frame(now);
        movement::frame(now);
    }
    if now % 300 == 0 {
        report(now);
    }
}

/// Whether the finale began: the areas' frame work stops then.
pub fn finale_started() -> bool {
    finale::started()
}

static DONE: AtomicI64 = AtomicI64::new(0);

/// The areas' marks; the finale's come last.
const AREAS: [&[&str]; 6] = [
    RULES_MARKS,
    basics::MARKS,
    triggers::MARKS,
    combat::MARKS,
    movement::MARKS,
    finale::MARKS,
];

/// Every `MISSING_EVERY` frames until all are in, each missing mark.
const MISSING_EVERY: i32 = 1500;
/// Most `BENCH-MISSING` lines a report writes.
const MISSING_LINES: usize = 24;
/// The frame (a report frame) the finale begins at even if an area still
/// misses marks, so the finale is tested on its own: its steps take at most
/// about 1050 frames (`finale.rs`), which a 6000-frame run leaves room for.
const FINALE_DEADLINE: i32 = 4800;

/// Every 300 frames until all are in, `BENCH frame <n>: <reached>/<total>`
/// and every `MISSING_EVERY` frames a line `BENCH-MISSING <mark>` for each
/// missing one (one pass over the marks: the frame has an operation
/// budget). Once the match's areas (`basics` to `movement`) have every
/// mark, or at `FINALE_DEADLINE` (the areas' missing marks listed then),
/// the finale begins; once every mark is in, the one mark `bench.all`.
fn report(now: i32) {
    if DONE.load(Relaxed) != 0 {
        return;
    }
    let deadline = now >= FINALE_DEADLINE && !finale_started();
    let list_missing = now % MISSING_EVERY == 0 || deadline;
    let mut listed = 0;
    let (mut total, mut reached, mut areas_missing) = (0, 0, 0);
    for (group, marks) in AREAS.iter().enumerate() {
        for name in marks.iter() {
            total += 1;
            if marked(name) {
                reached += 1;
                continue;
            }
            if (1..AREAS.len() - 1).contains(&group) {
                areas_missing += 1;
            }
            if list_missing && listed <= MISSING_LINES {
                let mut line = Line::new();
                line.text("BENCH-MISSING ")
                    .text(if listed == MISSING_LINES {
                        "..."
                    } else {
                        *name
                    });
                line.log();
                listed += 1;
            }
        }
    }
    let mut line = Line::new();
    line.text("BENCH frame ")
        .num(i64::from(now))
        .text(": ")
        .num(reached as i64)
        .text("/")
        .num(total as i64);
    line.log();
    if reached == total {
        DONE.store(1, Relaxed);
        mark("bench.all");
    } else if areas_missing == 0 || deadline {
        finale::begin(now);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn vera_init() {
    init();
}

/// The rules stage (RFC 0001 section 8): runs after `vera_init`, while the
/// rules are processed, when the module is the selected mod's.
#[unsafe(no_mangle)]
pub extern "C" fn on_rules() {
    rules_stage();
}

#[unsafe(no_mangle)]
pub extern "C" fn on_scenario_start() {
    start();
}

#[unsafe(no_mangle)]
pub extern "C" fn on_frame() {
    tick();
}

#[used]
#[unsafe(link_section = "vera.delivery")]
static __VERA_DELIVERY: [u8; 9] = vera20k_sdk::DELIVERY_SECTION;

#[unsafe(no_mangle)]
pub extern "C" fn on_events(ptr: i32, count: i32) {
    // SAFETY: the engine's own arguments, used only here.
    let records = unsafe { EventRecord::batch(ptr, count) };
    // Once the finale began the areas hear nothing more: their scene may be
    // mid-way, and the computer house goes.
    let finale = finale_started();
    for record in records {
        if finale && area_of(record.key) != FINALE {
            continue;
        }
        match area_of(record.key) {
            BASICS => basics::events(record),
            TRIGGERS => triggers::events(record),
            COMBAT => combat::events(record),
            MOVEMENT => movement::events(record),
            FINALE => finale::events(record),
            _ => {}
        }
    }
}

/// Dispatches one engine decision batch.
///
/// # Safety
/// The engine must supply a valid, exclusively accessible decision batch and
/// its associated result areas for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn on_decide(ptr: i32, count: i32) {
    // SAFETY: the caller supplies the engine's batch, used only in this call.
    let records = unsafe { DecideRecord::batch(ptr, count) };
    // As in `on_events`; an unanswered decision keeps the engine's result.
    let finale = finale_started();
    for mut record in records {
        if finale && area_of(record.event.key) != FINALE {
            continue;
        }
        match area_of(record.event.key) {
            BASICS => basics::decide(&mut record),
            TRIGGERS => triggers::decide(&mut record),
            COMBAT => combat::decide(&mut record),
            MOVEMENT => movement::decide(&mut record),
            FINALE => finale::decide(&mut record),
            _ => {}
        }
    }
}
