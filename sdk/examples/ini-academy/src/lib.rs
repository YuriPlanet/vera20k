//! "INI Academy", a demo `rules.wasm` for INI names a module registers
//! (RFC 0001 section 6h). The modder configures everything in the mod's
//! `rulesmd.ini`; the module only implements it:
//!
//! - `[Demo.AcademyTypes]`, a registry: each entry is a section with
//!   `Rank=` (`Rookie`, `Veteran` or `Elite`; default `Elite`), `Allowed=`
//!   (the TechnoTypes that may be promoted; empty for any) and `Eject=`
//!   (yes: the building throws its occupants back out after a promotion).
//! - `Demo.Academy=<academy type>` on a garrisonable building's TechnoType
//!   makes it an academy. The module binds a `PassengerAdded` trigger to
//!   the type; when an occupier garrisons it (`value2` 1), the occupant
//!   gets the academy's rank.
//! - `Event=Wasm:Demo.Promoted`, an object event raised on the promoted
//!   unit (`Other` is the building, `value` the new rank: 0 Rookie,
//!   1 Veteran, 2 Elite).
//! - `Wasm:academyHello(target,text)`, a module action that greets its
//!   target with the parameter text.
//!
//! Eject orders the building's `Unload` mission, as a player's deploy order
//! on a garrisoned building does: it lets every occupant out.
//!
//! To exercise itself without a player, it places a Battle Bunker and a few
//! infantrymen of the human player near their base and orders them in (the
//! garrison click's Capture mission, aimed at the bunker). It
//! logs `DEMO-OK <feature>` the first time each feature works (the engine
//! test `sim::ext_modules::tests::ini_demo_mods_reach_their_marks` checks
//! them).
#![no_std]

use core::sync::atomic::{AtomicI32, AtomicI64, Ordering::Relaxed};

use vera20k_sdk::NO_HANDLE;
use vera20k_sdk::prelude::*;
use vera20k_sdk::triggers::PassengerAddedEvent;

vera20k_sdk::named_actions! { ACADEMY_HELLO = "academyHello" => 1 }

/// Dispatch keys of the INI registrations and the entry trigger
/// (`on_events`), distinct from the named action's.
const ACADEMY_TYPES: i32 = 10;
const ACADEMY_KEY: i32 = 11;
const ON_ENTER: i32 = 12;

static PROMOTED: ini::EventSlot = ini::EventSlot::new();

/// `Rank` values (appendix 7.1).
const ROOKIE: i64 = 0;
const VETERAN: i64 = 1;
const ELITE: i64 = 2;

// ------------------------------------------------------------------ Memory
// Module memory is saved and hashed with the match, so plain statics do.

fn ld(a: &AtomicI32) -> i32 {
    a.load(Relaxed)
}

fn st(a: &AtomicI32, value: i32) {
    a.store(value, Relaxed)
}

fn ld64(a: &AtomicI64) -> i64 {
    a.load(Relaxed)
}

fn st64(a: &AtomicI64, value: i64) {
    a.store(value, Relaxed)
}

/// Academy types from `[Demo.AcademyTypes]`, in list order.
const MAX_KINDS: usize = 16;
static KIND_COUNT: AtomicI32 = AtomicI32::new(0);
/// The entry section's name as a string ID.
static KIND_NAME: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_RANK: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_EJECT: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
/// `Allowed=`: up to `MAX_ALLOWED` TechnoType IDs per academy type, in
/// `KIND_ALLOWED[kind * MAX_ALLOWED..]`; a count of 0 allows any unit.
const MAX_ALLOWED: usize = 16;
static KIND_ALLOWED_COUNT: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_ALLOWED: [AtomicI32; MAX_KINDS * MAX_ALLOWED] =
    [const { AtomicI32::new(-1) }; MAX_KINDS * MAX_ALLOWED];

/// Building types with `Demo.Academy=`: type ID and academy type index.
const MAX_TYPES: usize = 64;
static TYPE_COUNT: AtomicI32 = AtomicI32::new(0);
static TYPE_ID: [AtomicI32; MAX_TYPES] = [const { AtomicI32::new(-1) }; MAX_TYPES];
static TYPE_KIND: [AtomicI32; MAX_TYPES] = [const { AtomicI32::new(0) }; MAX_TYPES];

/// The one `PassengerAdded` trigger, bound to every academy building type.
static ENTER_TRIGGER: AtomicI64 = AtomicI64::new(NO_HANDLE);

/// Promotions and greetings so far; only the first few are logged.
static PROMOTIONS: AtomicI32 = AtomicI32::new(0);
static GREETINGS: AtomicI32 = AtomicI32::new(0);
const LOG_FIRST: i32 = 4;

/// The staged scene: done, attempts so far, the recruits and when they are
/// next ordered in.
static STAGED: AtomicI32 = AtomicI32::new(0);
static STAGE_TRIES: AtomicI32 = AtomicI32::new(0);
const MAX_STAGE_TRIES: i32 = 20;
static ACADEMY: AtomicI64 = AtomicI64::new(NO_HANDLE);
const RECRUITS: [&str; 4] = ["E1", "E2", "E1", "E2"];
static RECRUIT: [AtomicI64; RECRUITS.len()] = [const { AtomicI64::new(NO_HANDLE) }; RECRUITS.len()];
static ORDER_AT: AtomicI32 = AtomicI32::new(0);
/// Recruits not yet in are ordered in again every so often until then.
const REORDER_EVERY: i32 = 150;
const REORDER_UNTIL: i32 = 2400;

/// `DEMO-OK` lines already logged, one bit each, and other one-time lines.
static MARKS: AtomicI32 = AtomicI32::new(0);
const MARK_ENTRY: i32 = 1;
const MARK_KEY: i32 = 2;
const MARK_ENTER: i32 = 4;
const MARK_PROMOTE: i32 = 8;
const MARK_EVENT: i32 = 16;
const MARK_ACTION: i32 = 32;
const NOTE_EJECT: i32 = 64;
const NOTE_EJECT_FAILED: i32 = 128;
const NOTE_RAISE_FAILED: i32 = 256;
const NOTE_RANK_FAILED: i32 = 512;

/// Logs `line` the first time `bit` is marked.
fn mark(bit: i32, line: &str) {
    let marks = ld(&MARKS);
    if marks & bit == 0 {
        st(&MARKS, marks | bit);
        log(line);
    }
}

/// Whether `bit` is still unmarked; marks it.
fn first(bit: i32) -> bool {
    let marks = ld(&MARKS);
    st(&MARKS, marks | bit);
    marks & bit == 0
}

// -------------------------------------------------------------------- Init

fn init() {
    if ini::registry("Demo.AcademyTypes", ACADEMY_TYPES).is_err() {
        log("ini-academy: the name Demo.AcademyTypes is taken");
    }
    if ini::key("Demo.Academy", IniSection::TechnoType, ACADEMY_KEY).is_err() {
        log("ini-academy: the name Demo.Academy is taken");
    }
    match ini::event("Demo.Promoted", EventScope::Object) {
        Ok(event) => PROMOTED.set(event),
        Err(_) => log("ini-academy: the name Demo.Promoted is taken"),
    }
}

// --------------------------------------------------- INI records at start

/// `IniEntry`: one academy type of `[Demo.AcademyTypes]`.
fn load_academy_type(r: IniEntryRecord) {
    let mut name_buf = [0u8; 64];
    let Ok(section) = r.entry_name(&mut name_buf) else {
        log("ini-academy: a Demo.AcademyTypes entry name is longer than 63 characters");
        stop();
    };
    let count = ld(&KIND_COUNT) as usize;
    if count == MAX_KINDS {
        bad(section, "", "is past the 16 academy types this module holds");
    }
    let rank = read_rank(section);
    let eject = read_eject(section);
    let allowed = read_allowed(section, count);
    st(&KIND_NAME[count], r.record().args);
    st(&KIND_RANK[count], rank as i32);
    st(&KIND_EJECT[count], i32::from(eject));
    st(&KIND_ALLOWED_COUNT[count], allowed as i32);
    st(&KIND_COUNT, count as i32 + 1);
    let line = Line::new()
        .text("ini-academy: academy type ")
        .text(section)
        .text(" rank ")
        .text(rank_name(rank))
        .text(", ");
    let line = if allowed == 0 {
        line.text("any unit")
    } else {
        line.num(allowed as i64).text(" allowed types")
    };
    line.text(if eject { ", eject" } else { "" }).log();
    mark(MARK_ENTRY, "DEMO-OK entry");
}

/// `[section] Rank=`: `Rookie`, `Veteran` or `Elite` (or 0, 1, 2), ignoring
/// case; `Elite` when missing or empty.
fn read_rank(section: &str) -> i64 {
    let mut buf = [0u8; 32];
    let text = match Rules::get(section, "Rank", &mut buf) {
        Ok(text) => text.trim_ascii(),
        Err(Status::NotFound) => "",
        Err(_) => bad(section, "Rank", "could not be read"),
    };
    if text.is_empty() {
        return ELITE;
    }
    [("Rookie", ROOKIE), ("Veteran", VETERAN), ("Elite", ELITE)]
        .iter()
        .find(|(name, _)| text.eq_ignore_ascii_case(name))
        .map(|(_, rank)| *rank)
        .or_else(|| ini::int(text).filter(|rank| (ROOKIE..=ELITE).contains(rank)))
        .unwrap_or_else(|| bad(section, "Rank", "must be Rookie, Veteran or Elite"))
}

/// `[section] Eject=`: yes or no; no when missing or empty.
fn read_eject(section: &str) -> bool {
    let mut buf = [0u8; 32];
    match Rules::get(section, "Eject", &mut buf) {
        Ok(text) if text.trim_ascii().is_empty() => false,
        Ok(text) => ini::yes_no(text).unwrap_or_else(|| bad(section, "Eject", "must be yes or no")),
        Err(Status::NotFound) => false,
        Err(_) => bad(section, "Eject", "could not be read"),
    }
}

/// `[section] Allowed=` into academy type `kind`'s list; how many types it
/// names (0: any unit).
fn read_allowed(section: &str, kind: usize) -> usize {
    let mut buf = [0u8; 512];
    let text = match Rules::get(section, "Allowed", &mut buf) {
        Ok(text) => text,
        Err(Status::NotFound) => "",
        Err(_) => bad(section, "Allowed", "is longer than 511 characters"),
    };
    let mut count = 0;
    for name in ini::split(text).filter(|name| !name.is_empty()) {
        if count == MAX_ALLOWED {
            bad(section, "Allowed", "names more than 16 types");
        }
        let Ok(ty) = TypeRef::techno_type(name) else {
            let mut what = Line::new().text("names ").text(name);
            what = what.text(", which is no TechnoType");
            bad(section, "Allowed", what.as_str());
        };
        st(&KIND_ALLOWED[kind * MAX_ALLOWED + count], ty.id());
        count += 1;
    }
    count
}

/// `IniKey`: a building type with `Demo.Academy=`.
fn attach_academy(r: IniKeyRecord) {
    let mut section_buf = [0u8; 64];
    let section = r.section_name(&mut section_buf).unwrap_or("?");
    let Some(kind) = kind_named(r.record().args) else {
        bad(
            section,
            "Demo.Academy",
            "names no section listed in [Demo.AcademyTypes]",
        );
    };
    let count = ld(&TYPE_COUNT) as usize;
    if count == MAX_TYPES {
        bad(
            section,
            "Demo.Academy",
            "is past the 64 academy buildings this module holds",
        );
    }
    let ty = r.type_ref();
    let trigger = match ld64(&ENTER_TRIGGER) {
        NO_HANDLE => match Trigger::on(Event::PASSENGER_ADDED).enqueue(ON_ENTER).done() {
            Ok(trigger) => {
                st64(&ENTER_TRIGGER, trigger.0);
                trigger
            }
            Err(_) => {
                log("ini-academy: the PassengerAdded trigger could not be made");
                stop();
            }
        },
        handle => TriggerRef(handle),
    };
    if trigger.bind(ty).is_err() {
        bad(section, "Demo.Academy", "could not bind the academy to this type");
    }
    st(&TYPE_ID[count], ty.id());
    st(&TYPE_KIND[count], kind as i32);
    st(&TYPE_COUNT, count as i32 + 1);
    mark(MARK_KEY, "DEMO-OK key");
}

/// The academy type whose entry name is string `id`, ignoring case.
fn kind_named(id: i32) -> Option<usize> {
    let count = ld(&KIND_COUNT) as usize;
    if let Some(kind) = (0..count).find(|&i| ld(&KIND_NAME[i]) == id) {
        return Some(kind);
    }
    let mut want_buf = [0u8; 64];
    let want = vera20k_sdk::text(id, &mut want_buf).ok()?.trim_ascii();
    (0..count).find(|&i| {
        let mut have_buf = [0u8; 64];
        vera20k_sdk::text(ld(&KIND_NAME[i]), &mut have_buf)
            .is_ok_and(|have| have.eq_ignore_ascii_case(want))
    })
}

/// Logs what is wrong with `[section] key=` and stops the module.
fn bad(section: &str, key: &str, what: &str) -> ! {
    let line = Line::new().text("ini-academy: [").text(section).text("] ");
    let line = if key.is_empty() {
        line
    } else {
        line.text(key).text("= ")
    };
    line.text(what).log();
    stop();
}

/// Stops the module (a trap stops only this module, RFC section 3).
fn stop() -> ! {
    #[cfg(target_arch = "wasm32")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_arch = "wasm32"))]
    loop {}
}

fn rank_name(rank: i64) -> &'static str {
    match rank {
        ROOKIE => "Rookie",
        VETERAN => "Veteran",
        _ => "Elite",
    }
}

// ------------------------------------------------------------------ Start

fn start() {
    Line::new()
        .text("ini-academy: ")
        .num(i64::from(ld(&KIND_COUNT)))
        .text(" academy types on ")
        .num(i64::from(ld(&TYPE_COUNT)))
        .text(" building types")
        .log();
}

// --------------------------------------------------------------- Academy

/// The academy type of building type `type_id`, if it is an academy.
fn kind_of_type(type_id: i32) -> Option<usize> {
    let count = ld(&TYPE_COUNT) as usize;
    (0..count)
        .find(|&i| ld(&TYPE_ID[i]) == type_id)
        .map(|i| ld(&TYPE_KIND[i]) as usize)
}

/// Whether academy type `kind` may promote a unit of type `type_id`.
fn allowed(kind: usize, type_id: i32) -> bool {
    let count = ld(&KIND_ALLOWED_COUNT[kind]) as usize;
    count == 0
        || KIND_ALLOWED[kind * MAX_ALLOWED..kind * MAX_ALLOWED + count]
            .iter()
            .any(|id| ld(id) == type_id)
}

/// The `PassengerAdded` trigger's module action: an academy building took
/// an occupant.
fn on_enter(e: PassengerAddedEvent) {
    // Transports load passengers too; only a garrison counts.
    if e.garrison() != 1 {
        return;
    }
    mark(MARK_ENTER, "DEMO-OK enter");
    let (building, unit) = (e.transport(), e.passenger());
    let Some(kind) = building
        .type_ref()
        .ok()
        .and_then(|ty| kind_of_type(ty.id()))
    else {
        return;
    };
    let Ok(unit_type) = unit.type_ref() else {
        return; // Gone again before the module phase.
    };
    if !allowed(kind, unit_type.id()) {
        return;
    }
    let rank = i64::from(ld(&KIND_RANK[kind]));
    // Never a demotion: a unit at the academy's rank or above keeps its own.
    let Ok(current) = vera20k_sdk::get(unit, Prop::Rank) else {
        return;
    };
    if current >= rank {
        return;
    }
    if vera20k_sdk::set(unit, Prop::Rank, rank).is_err() {
        if first(NOTE_RANK_FAILED) {
            log("ini-academy: a rank could not be written");
        }
        return;
    }
    let now = vera20k_sdk::get(unit, Prop::Rank);
    if rank == ELITE && now == Ok(ELITE) {
        mark(MARK_PROMOTE, "DEMO-OK promote");
    }
    let promotions = ld(&PROMOTIONS) + 1;
    st(&PROMOTIONS, promotions);
    if promotions <= LOG_FIRST {
        Line::new()
            .text("ini-academy: a unit entered an academy and is now ")
            .text(rank_name(now.unwrap_or(rank)))
            .log();
    }
    match Action::raise_event(PROMOTED.get(), unit, building, rank, 0) {
        Ok(()) => mark(MARK_EVENT, "DEMO-OK event"),
        Err(status) => {
            if first(NOTE_RAISE_FAILED) {
                Line::new()
                    .text("ini-academy: Demo.Promoted could not be raised: ")
                    .text(status_name(status))
                    .log();
            }
        }
    }
    if ld(&KIND_EJECT[kind]) != 0 {
        // The building's owner orders it, as a player's deploy order does.
        match building.set_mission(Mission::Unload) {
            Ok(()) => {
                if first(NOTE_EJECT) {
                    log("ini-academy: the academy lets its occupants out");
                }
            }
            Err(status) => {
                if first(NOTE_EJECT_FAILED) {
                    Line::new()
                        .text("ini-academy: the academy could not let its occupants out: ")
                        .text(status_name(status))
                        .log();
                }
            }
        }
    }
}

/// Module action `academyHello(target,text)`: greets its target.
fn academy_hello(r: &EventRecord) {
    let mut buf = [0u8; 64];
    let text = r.args_text(&mut buf).unwrap_or("");
    let unit = match r.target() {
        Target::Subject => r.subject(),
        Target::Other => r.other(),
        Target::TagHolder => Techno(r.holder),
        _ => Techno(NO_HANDLE),
    };
    if !text.is_empty() {
        mark(MARK_ACTION, "DEMO-OK action");
    }
    let greetings = ld(&GREETINGS) + 1;
    st(&GREETINGS, greetings);
    if greetings <= LOG_FIRST {
        let rank = vera20k_sdk::get(unit, Prop::Rank).unwrap_or(ROOKIE);
        Line::new()
            .text("ini-academy: academyHello says \"")
            .text(text)
            .text("\" to a new ")
            .text(rank_name(rank))
            .log();
    }
}

// ------------------------------------------------------- The staged scene

/// Construction yards and MCVs of the three sides: where a house's base is.
const BASES: [&str; 6] = ["GACNST", "NACNST", "YACNST", "AMCV", "SMCV", "PCV"];

/// The building the scene uses: the Soviet Battle Bunker (a 2x2 garrison).
const BUNKER: &str = "NABNKR";

/// The cell of `house`'s construction yard or MCV.
fn base_of(house: House) -> Option<Cell> {
    for name in BASES {
        let Ok(ty) = TypeRef::techno_type(name) else {
            continue;
        };
        let object = Find::objects().owned_by(house).of_type(ty).first();
        if object != NO_HANDLE {
            if let Ok(cell) = vera20k_sdk::get(Techno(object), Prop::Cell) {
                return Some(Cell(cell));
            }
        }
    }
    None
}

/// The first computer house with a base, where the scene leans to.
fn enemy_base() -> Option<Cell> {
    let mut houses = [0i64; 16];
    let (count, _) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .page(&mut houses, 0)
        .ok()?;
    houses[..count].iter().find_map(|&house| base_of(House(house)))
}

/// `(x, y)` as a cell when it is on the map's range.
fn cell_at(x: i32, y: i32) -> Option<Cell> {
    ((1..=500).contains(&x) && (1..=500).contains(&y)).then(|| Cell::at(x as u16, y as u16))
}

/// Whether the 2x2 block from `(x, y)` is open ground.
fn open_block(x: i32, y: i32) -> bool {
    (0..2).all(|dy| {
        (0..2).all(|dx| {
            cell_at(x + dx, y + dy).is_some_and(|cell| cell.passable(SpeedType::Track) == Ok(true))
        })
    })
}

fn stage() {
    let tries = ld(&STAGE_TRIES) + 1;
    if tries > MAX_STAGE_TRIES {
        return;
    }
    st(&STAGE_TRIES, tries);
    if !place_scene() && tries == MAX_STAGE_TRIES {
        log("ini-academy: found no place to stage the scene");
    }
}

/// Places a bunker of the human player's a quarter of the way from their
/// base towards the computer's, and the recruits around it.
fn place_scene() -> bool {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        return false;
    }
    let human = House(human);
    let Some(home) = base_of(human) else {
        return false;
    };
    let (Ok(bunker), Ok(_)) = (TypeRef::techno_type(BUNKER), TypeRef::techno_type("E1")) else {
        log("ini-academy: the rules have no NABNKR or E1");
        st(&STAGE_TRIES, MAX_STAGE_TRIES);
        return false;
    };
    let away = enemy_base().unwrap_or(home);
    let (hx, hy) = (i32::from(home.x()), i32::from(home.y()));
    let ax = hx + (i32::from(away.x()) - hx) / 4;
    let ay = hy + (i32::from(away.y()) - hy) / 4;
    // The bunker: the first open block in rings around that point.
    let mut academy = None;
    'rings: for ring in 0..=8i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring || !open_block(ax + dx, ay + dy) {
                    continue;
                }
                let Some(cell) = cell_at(ax + dx, ay + dy) else {
                    continue;
                };
                if let Ok(building) = Action::create_object(cell, bunker, human) {
                    academy = Some((building, ax + dx, ay + dy));
                    break 'rings;
                }
            }
        }
    }
    let Some((building, bx, by)) = academy else {
        return false;
    };
    st64(&ACADEMY, building.0);
    // The recruits: open cells two to five cells from the bunker.
    let mut made = 0;
    'around: for ring in 2..=5i32 {
        for dy in -ring..=ring + 1 {
            for dx in -ring..=ring + 1 {
                if made == RECRUITS.len() {
                    break 'around;
                }
                let edge = dx == -ring || dy == -ring || dx == ring + 1 || dy == ring + 1;
                if !edge {
                    continue;
                }
                let Some(cell) = cell_at(bx + dx, by + dy) else {
                    continue;
                };
                if cell.passable(SpeedType::Foot) != Ok(true) {
                    continue;
                }
                let Ok(kind) = TypeRef::techno_type(RECRUITS[made]) else {
                    made += 1;
                    continue;
                };
                // An occupied cell takes no unit; the next one is tried.
                if let Ok(unit) = Action::create_object(cell, kind, human) {
                    st64(&RECRUIT[made], unit.0);
                    made += 1;
                }
            }
        }
    }
    st(&STAGED, 1);
    // Ordered in from the next pass, once the new objects have settled.
    st(&ORDER_AT, frame() + 15);
    Line::new()
        .text("ini-academy: a Battle Bunker academy and ")
        .num(made as i64)
        .text(" recruits near (")
        .num(i64::from(bx))
        .text(",")
        .num(i64::from(by))
        .text(")")
        .log();
    true
}

/// Orders the recruits not yet inside, and not yet Elite, into the bunker.
fn order_recruits(now: i32) {
    let academy = Techno(ld64(&ACADEMY));
    if academy.health().is_err() {
        st(&ORDER_AT, REORDER_UNTIL + 1);
        return;
    }
    for slot in &RECRUIT {
        let unit = Techno(ld64(slot));
        if unit.is_none() || unit.health().is_err() {
            continue;
        }
        let inside = unit.transport().is_ok_and(|t| !t.is_none());
        let elite = vera20k_sdk::get(unit, Prop::Rank) == Ok(ELITE);
        if !inside && !elite {
            // A player's garrison click is the Capture order with the
            // building as its destination (`Command::CaptureBuilding`).
            let _ = Action::set_mission_on(unit, Mission::Capture, academy);
        }
    }
    st(&ORDER_AT, now + REORDER_EVERY);
}

// ------------------------------------------------------------------ Frame

fn tick() {
    let now = frame();
    if ld(&STAGED) == 0 {
        if now >= 30 && now % 15 == 0 {
            stage();
        }
        return;
    }
    if now <= REORDER_UNTIL && now >= ld(&ORDER_AT) {
        order_recruits(now);
    }
}

// ------------------------------------------------------------------- Text

/// A log line built without an allocator (`core::fmt` would pull in code
/// the engine does not load).
struct Line {
    buf: [u8; 160],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 160],
            len: 0,
        }
    }

    fn text(mut self, text: &str) -> Self {
        for &byte in text.as_bytes() {
            if self.len == self.buf.len() {
                break;
            }
            self.buf[self.len] = byte;
            self.len += 1;
        }
        self
    }

    fn num(self, value: i64) -> Self {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value.unsigned_abs();
        loop {
            at -= 1;
            digits[at] = b'0' + (rest % 10) as u8;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        let line = if value < 0 { self.text("-") } else { self };
        line.text(core::str::from_utf8(&digits[at..]).unwrap_or("?"))
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    fn log(&self) {
        log(self.as_str());
    }
}

/// A status's name; `core::fmt` would pull in code the engine does not
/// load. The log never says "refused" (the demo test looks for it).
fn status_name(status: Status) -> &'static str {
    match status {
        Status::Gone => "gone",
        Status::NotFound => "not found",
        Status::Refused => "declined",
        Status::Unsupported => "unsupported",
    }
}

vera20k_sdk::init!(init);
vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
vera20k_sdk::on_events!(
    ACADEMY_TYPES => load_academy_type,
    ACADEMY_KEY => attach_academy,
    ON_ENTER => on_enter,
    ACADEMY_HELLO => academy_hello,
);
