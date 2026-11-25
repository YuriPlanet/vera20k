//! "Control Bench", a benchmark `rules.wasm` (RFC 0001 phase 1): one module
//! that reaches nearly every control point the engine offers and logs a line
//! `DEMO-OK <mark>` the first time each one works. It is a test fixture, not
//! a game: it spawns its own units next to the player's start and drives them
//! itself, so a Battle against one computer house with nobody at the controls
//! is enough.
//!
//! The engine test `ini_demo_mods_reach_their_marks` runs it
//! (`VERA20K_TEST_INI_DEMOS`, see `README.zh-CN.md` for the mark list); a
//! missing mark names the control point that stopped working.
//!
//! The work is split by area, one file each, so the marks stay independent:
//! - `basics`: host calls, properties, writes, variables, searches,
//!   modifiers of every wired stat, `ini_register`;
//! - `triggers`: trigger events and polls, atomic and stock actions, chains,
//!   house orders and their results, claims, INI triggers;
//! - `combat`: the combat decisions (damage, death, fire, projectiles);
//! - `movement`: production, command, movement, path and unit AI decisions.
//!
//! `lib` owns the exports, the dispatch, the marks and the arena.
#![no_std]

mod basics;
mod combat;
mod movement;
mod triggers;

use core::sync::atomic::{AtomicI64, Ordering::Relaxed};

use vera20k_sdk::prelude::*;
use vera20k_sdk::{NO_HANDLE, get};

// ------------------------------------------------------------------ Keys

/// Each area owns a range of trigger keys (`Trigger::enqueue`,
/// `decide_sync`, INI registrations): `basics` 100..199, `triggers`
/// 200..299, `combat` 300..399, `movement` 400..499. `on_events` and
/// `on_decide` route a record by its key's hundred.
pub const BASICS: i32 = 1;
pub const TRIGGERS: i32 = 2;
pub const COMBAT: i32 = 3;
pub const MOVEMENT: i32 = 4;

fn area_of(key: i32) -> i32 {
    key / 100
}

// ----------------------------------------------------------------- Marks

const MAX_MARKS: usize = 512;
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

/// Logs `DEMO-OK <name>` the first time `name` is marked. Marks are short
/// ASCII names, unique across the areas (each area prefixes its own:
/// `basics.`, `triggers.`, `combat.`, `movement.`).
pub fn mark(name: &str) {
    let h = hash(name);
    for slot in &MARKED {
        match slot.load(Relaxed) {
            0 => {
                slot.store(h, Relaxed);
                let mut line = Line::new();
                line.text("DEMO-OK ").text(name);
                line.log();
                return;
            }
            v if v == h => return,
            _ => {}
        }
    }
}

/// Whether `name` was marked.
pub fn marked(name: &str) -> bool {
    let h = hash(name);
    MARKED.iter().any(|slot| slot.load(Relaxed) == h)
}

/// Logs `BENCH-FAIL <name>: <why>` once per name, for a control point that
/// answered wrongly (a mark that never comes says only that it was not
/// reached).
pub fn bench_fail(name: &str, why: &str) {
    let mut line = Line::new();
    line.text("BENCH-FAIL ").text(name).text(": ").text(why);
    let h = hash(name) ^ 0x5a5a;
    for slot in &MARKED {
        match slot.load(Relaxed) {
            0 => {
                slot.store(h, Relaxed);
                line.log();
                return;
            }
            v if v == h => return,
            _ => {}
        }
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

// --------------------------------------------------------------- Exports

/// The frame the arena opens on: the starting units are on the map by the
/// second frame.
const OPEN_FRAME: i32 = 2;
static OPEN: AtomicI64 = AtomicI64::new(0);

fn init() {
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
        basics::open();
        triggers::open();
        combat::open();
        movement::open();
        return;
    }
    if OPEN.load(Relaxed) < 0 {
        return;
    }
    basics::frame(now);
    triggers::frame(now);
    combat::frame(now);
    movement::frame(now);
    if now % 300 == 0 {
        report(now);
    }
}

static DONE: AtomicI64 = AtomicI64::new(0);

/// Every 300 frames until all are in: `BENCH <reached>/<total>` and, once
/// every area's `MARKS` is reached, the one mark `bench.all`.
fn report(now: i32) {
    if DONE.load(Relaxed) != 0 {
        return;
    }
    let areas = [
        basics::MARKS,
        triggers::MARKS,
        combat::MARKS,
        movement::MARKS,
    ];
    let total: usize = areas.iter().map(|marks| marks.len()).sum();
    let reached = areas
        .iter()
        .flat_map(|marks| marks.iter())
        .filter(|name| marked(name))
        .count();
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
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn vera_init() {
    init();
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
    for record in records {
        match area_of(record.key) {
            BASICS => basics::events(record),
            TRIGGERS => triggers::events(record),
            COMBAT => combat::events(record),
            MOVEMENT => movement::events(record),
            _ => {}
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn on_decide(ptr: i32, count: i32) {
    // SAFETY: the engine's own arguments, used only here.
    let records = unsafe { DecideRecord::batch(ptr, count) };
    for record in records.iter_mut() {
        match area_of(record.event.key) {
            BASICS => basics::decide(record),
            TRIGGERS => triggers::decide(record),
            COMBAT => combat::decide(record),
            MOVEMENT => movement::decide(record),
            _ => {}
        }
    }
}
