//! "INI Shield", a demo `rules.wasm` for INI names a module registers
//! (RFC 0001 section 6h). The modder configures everything in the mod's
//! `rulesmd.ini`; the module only implements it:
//!
//! - `[Demo.ShieldTypes]`, a registry: each entry is a section with
//!   `Strength=` (shield points), `Regen=` (points back every 15 frames)
//!   and `Delay=` (frames after a hit before regeneration starts).
//! - `Demo.Shield=<shield type>` on a TechnoType gives that type a shield.
//!   The module binds a `TakeDamage` decision trigger to the type; the
//!   shield, kept in module memory per object and filled on the first hit,
//!   takes damage before the unit does.
//! - `Event=Wasm:Demo.ShieldBroken`, an object event raised when a shield
//!   drops to 0. A decision handler may not write, so the break is noted in
//!   memory and raised in the next `on_frame`.
//! - `Wasm:restoreShield(target,amount)`, a module action: gives back
//!   `amount` shield points to its target.
//!
//! To exercise itself without a player, it stages a small fight of
//! shielded tanks and infantry between the human player and the first
//! computer opponent near the player's base. It logs `DEMO-OK <feature>`
//! the first time each feature works (the engine test
//! `sim::ext_modules::tests::ini_demo_mods_reach_their_marks` checks them).
#![no_std]

use core::sync::atomic::{AtomicI32, AtomicI64, Ordering::Relaxed};

use vera20k_sdk::NO_HANDLE;
use vera20k_sdk::prelude::*;

vera20k_sdk::named_actions! { RESTORE_SHIELD = "restoreShield" => 1 }

/// Dispatch keys of the INI registrations (`on_events`).
const SHIELD_TYPES: i32 = 10;
const SHIELD_KEY: i32 = 11;
/// The decision key of the `TakeDamage` decision trigger (`on_decide`).
const ABSORB: i32 = 20;

static SHIELD_BROKEN: ini::EventSlot = ini::EventSlot::new();

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

/// Shield types from `[Demo.ShieldTypes]`, in list order.
const MAX_KINDS: usize = 16;
static KIND_COUNT: AtomicI32 = AtomicI32::new(0);
/// The entry section's name as a string ID.
static KIND_NAME: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_STRENGTH: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_REGEN: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];
static KIND_DELAY: [AtomicI32; MAX_KINDS] = [const { AtomicI32::new(0) }; MAX_KINDS];

/// TechnoTypes with `Demo.Shield=`: type ID and shield type index.
const MAX_TYPES: usize = 64;
static TYPE_COUNT: AtomicI32 = AtomicI32::new(0);
static TYPE_ID: [AtomicI32; MAX_TYPES] = [const { AtomicI32::new(-1) }; MAX_TYPES];
static TYPE_KIND: [AtomicI32; MAX_TYPES] = [const { AtomicI32::new(0) }; MAX_TYPES];

/// The one `TakeDamage` decision trigger, bound to every shielded type.
static DECIDE_TRIGGER: AtomicI64 = AtomicI64::new(NO_HANDLE);

/// Live shields, one slot per object that has been hit (`NO_HANDLE` is a
/// free slot).
const MAX_SHIELDS: usize = 128;
static SHIELD_OBJECT: [AtomicI64; MAX_SHIELDS] = [const { AtomicI64::new(NO_HANDLE) }; MAX_SHIELDS];
static SHIELD_KIND: [AtomicI32; MAX_SHIELDS] = [const { AtomicI32::new(0) }; MAX_SHIELDS];
static SHIELD_POINTS: [AtomicI32; MAX_SHIELDS] = [const { AtomicI32::new(0) }; MAX_SHIELDS];
static SHIELD_HIT: [AtomicI32; MAX_SHIELDS] = [const { AtomicI32::new(0) }; MAX_SHIELDS];

/// Shields broken in a decision, raised as `Demo.ShieldBroken` in the next
/// `on_frame`.
const MAX_PENDING: usize = 32;
static PENDING_COUNT: AtomicI32 = AtomicI32::new(0);
static PENDING: [AtomicI64; MAX_PENDING] = [const { AtomicI64::new(NO_HANDLE) }; MAX_PENDING];

/// 1 once a decision absorbed damage (logged from `on_frame`).
static ABSORBED: AtomicI32 = AtomicI32::new(0);

/// The staged fight: done, and attempts so far.
static STAGED: AtomicI32 = AtomicI32::new(0);
static STAGE_TRIES: AtomicI32 = AtomicI32::new(0);
const MAX_STAGE_TRIES: i32 = 20;

/// `DEMO-OK` lines already logged, one bit each.
static MARKS: AtomicI32 = AtomicI32::new(0);
const MARK_ENTRY: i32 = 1;
const MARK_KEY: i32 = 2;
const MARK_ABSORB: i32 = 4;
const MARK_BROKEN: i32 = 8;
const MARK_RESTORE: i32 = 16;

/// Logs `line` the first time `bit` is marked.
fn mark(bit: i32, line: &str) {
    let marks = ld(&MARKS);
    if marks & bit == 0 {
        st(&MARKS, marks | bit);
        log(line);
    }
}

// -------------------------------------------------------------------- Init

fn init() {
    if ini::registry("Demo.ShieldTypes", SHIELD_TYPES).is_err() {
        log("ini-shield: the name Demo.ShieldTypes is taken");
    }
    if ini::key("Demo.Shield", IniSection::TechnoType, SHIELD_KEY).is_err() {
        log("ini-shield: the name Demo.Shield is taken");
    }
    match ini::event("Demo.ShieldBroken", EventScope::Object) {
        Ok(event) => SHIELD_BROKEN.set(event),
        Err(_) => log("ini-shield: the name Demo.ShieldBroken is taken"),
    }
}

// --------------------------------------------------- INI records at start

/// `IniEntry`: one shield type of `[Demo.ShieldTypes]`.
fn load_shield_type(r: IniEntryRecord) {
    let mut name_buf = [0u8; 64];
    let Ok(section) = r.entry_name(&mut name_buf) else {
        log("ini-shield: a Demo.ShieldTypes entry name is longer than 63 characters");
        stop();
    };
    let count = ld(&KIND_COUNT) as usize;
    if count == MAX_KINDS {
        bad(section, "", "is past the 16 shield types this module holds");
    }
    let strength = read_int(section, "Strength", None, 1, 1_000_000);
    let regen = read_int(section, "Regen", Some(0), 0, 1_000_000);
    let delay = read_int(section, "Delay", Some(0), 0, 1_000_000);
    st(&KIND_NAME[count], r.record().args);
    st(&KIND_STRENGTH[count], strength as i32);
    st(&KIND_REGEN[count], regen as i32);
    st(&KIND_DELAY[count], delay as i32);
    st(&KIND_COUNT, count as i32 + 1);
    Line::new()
        .text("ini-shield: shield type ")
        .text(section)
        .text(" strength ")
        .num(strength)
        .text(", regen ")
        .num(regen)
        .text(", delay ")
        .num(delay)
        .log();
    mark(MARK_ENTRY, "DEMO-OK entry");
}

/// `IniKey`: a TechnoType with `Demo.Shield=`.
fn attach_shield(r: IniKeyRecord) {
    let mut section_buf = [0u8; 64];
    let section = r.section_name(&mut section_buf).unwrap_or("?");
    let Some(kind) = kind_named(r.record().args) else {
        bad(
            section,
            "Demo.Shield",
            "names no section listed in [Demo.ShieldTypes]",
        );
    };
    let count = ld(&TYPE_COUNT) as usize;
    if count == MAX_TYPES {
        bad(
            section,
            "Demo.Shield",
            "is past the 64 shielded types this module holds",
        );
    }
    let ty = r.type_ref();
    let trigger = match ld64(&DECIDE_TRIGGER) {
        NO_HANDLE => match Trigger::on(Event::TAKE_DAMAGE).decide_sync(ABSORB) {
            Ok(trigger) => {
                st64(&DECIDE_TRIGGER, trigger.0);
                trigger
            }
            Err(_) => {
                log("ini-shield: the TakeDamage decision trigger could not be made");
                stop();
            }
        },
        handle => TriggerRef(handle),
    };
    if trigger.bind(ty).is_err() {
        bad(
            section,
            "Demo.Shield",
            "could not bind the shield to this type",
        );
    }
    st(&TYPE_ID[count], ty.id());
    st(&TYPE_KIND[count], kind as i32);
    st(&TYPE_COUNT, count as i32 + 1);
    mark(MARK_KEY, "DEMO-OK key");
}

/// The shield type whose entry name is string `id`, ignoring case.
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

/// `[section] key` as a whole number in `min..=max`; `default` when the key
/// is missing (`None`: required). Stops the module on a bad value.
fn read_int(section: &str, key: &str, default: Option<i64>, min: i64, max: i64) -> i64 {
    let mut buf = [0u8; 32];
    let value = match Rules::get(section, key, &mut buf) {
        Ok(text) => ini::int(text),
        Err(Status::NotFound) => default,
        Err(_) => None,
    };
    match value {
        Some(value) if value >= min && value <= max => value,
        _ => {
            Line::new()
                .text("ini-shield: [")
                .text(section)
                .text("] ")
                .text(key)
                .text("= must be a whole number from ")
                .num(min)
                .text(" to ")
                .num(max)
                .log();
            stop();
        }
    }
}

/// Logs what is wrong with `[section] key=` and stops the module.
fn bad(section: &str, key: &str, what: &str) -> ! {
    let line = Line::new().text("ini-shield: [").text(section).text("] ");
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

// ------------------------------------------------------------------ Start

fn start() {
    Line::new()
        .text("ini-shield: ")
        .num(i64::from(ld(&KIND_COUNT)))
        .text(" shield types on ")
        .num(i64::from(ld(&TYPE_COUNT)))
        .text(" unit types")
        .log();
}

// --------------------------------------------------------------- Shields

/// The shield type of TechnoType `type_id`, if it has one.
fn kind_of_type(type_id: i32) -> Option<usize> {
    let count = ld(&TYPE_COUNT) as usize;
    (0..count)
        .find(|&i| ld(&TYPE_ID[i]) == type_id)
        .map(|i| ld(&TYPE_KIND[i]) as usize)
}

/// `object`'s shield slot, made full when it has none; `None` when every
/// slot is taken or its type has no shield.
fn shield_of(object: Techno) -> Option<usize> {
    if let Some(slot) = (0..MAX_SHIELDS).find(|&i| ld64(&SHIELD_OBJECT[i]) == object.0) {
        return Some(slot);
    }
    let kind = kind_of_type(object.type_ref().ok()?.id())?;
    let slot = (0..MAX_SHIELDS).find(|&i| ld64(&SHIELD_OBJECT[i]) == NO_HANDLE)?;
    st64(&SHIELD_OBJECT[slot], object.0);
    st(&SHIELD_KIND[slot], kind as i32);
    st(&SHIELD_POINTS[slot], ld(&KIND_STRENGTH[kind]));
    st(&SHIELD_HIT[slot], frame());
    Some(slot)
}

/// The `TakeDamage` decision: the shield takes the final damage first.
/// Reads only, and writes module memory.
fn absorb(mut d: TakeDamageDecision) {
    let damage = d.previous();
    if damage <= 0 {
        return;
    }
    let victim = d.victim();
    let Some(slot) = shield_of(victim) else {
        return;
    };
    st(&SHIELD_HIT[slot], frame());
    let points = i64::from(ld(&SHIELD_POINTS[slot]));
    if points <= 0 {
        return;
    }
    let taken = points.min(damage);
    let left = points - taken;
    st(&SHIELD_POINTS[slot], left as i32);
    d.set_result(damage - taken);
    st(&ABSORBED, 1);
    if left == 0 {
        let pending = ld(&PENDING_COUNT) as usize;
        if pending < MAX_PENDING {
            st64(&PENDING[pending], victim.0);
            st(&PENDING_COUNT, pending as i32 + 1);
        }
    }
}

/// Raises `Demo.ShieldBroken` for the shields the decisions broke.
fn raise_broken() {
    let pending = ld(&PENDING_COUNT) as usize;
    st(&PENDING_COUNT, 0);
    for entry in &PENDING[..pending] {
        let object = Techno(ld64(entry));
        // `Gone` when the hit that broke the shield also killed the unit.
        let raised = Action::raise_event(SHIELD_BROKEN.get(), object, Techno(NO_HANDLE), 0, 0);
        if raised.is_ok() {
            mark(MARK_BROKEN, "DEMO-OK broken");
        }
    }
}

/// Every 15 frames: shields not hit for their `Delay=` regain `Regen=`;
/// slots of dead objects are freed.
fn regenerate(now: i32) {
    for slot in 0..MAX_SHIELDS {
        let object = ld64(&SHIELD_OBJECT[slot]);
        if object == NO_HANDLE {
            continue;
        }
        if Techno(object).health().is_err() {
            st64(&SHIELD_OBJECT[slot], NO_HANDLE);
            continue;
        }
        let kind = ld(&SHIELD_KIND[slot]) as usize;
        let strength = ld(&KIND_STRENGTH[kind]);
        let points = ld(&SHIELD_POINTS[slot]);
        if points < strength && now - ld(&SHIELD_HIT[slot]) >= ld(&KIND_DELAY[kind]) {
            st(
                &SHIELD_POINTS[slot],
                (points + ld(&KIND_REGEN[kind])).min(strength),
            );
        }
    }
}

/// Module action `restoreShield(target,amount)`.
fn restore_shield(r: &EventRecord) {
    let mut buf = [0u8; 32];
    let Some(amount) = r.args_text(&mut buf).ok().and_then(ini::int) else {
        log("ini-shield: restoreShield wants a whole number, as in restoreShield(Subject,40)");
        return;
    };
    mark(MARK_RESTORE, "DEMO-OK restore");
    let object = match r.target() {
        Target::Subject => r.subject(),
        Target::Other => r.other(),
        Target::TagHolder => Techno(r.holder),
        _ => return,
    };
    if amount <= 0 || object.health().is_err() {
        return;
    }
    let Some(slot) = shield_of(object) else {
        return;
    };
    let strength = i64::from(ld(&KIND_STRENGTH[ld(&SHIELD_KIND[slot]) as usize]));
    let points = i64::from(ld(&SHIELD_POINTS[slot]));
    st(&SHIELD_POINTS[slot], (points + amount).min(strength) as i32);
}

// ------------------------------------------------------- The staged fight

/// Construction yards and MCVs of the three sides: where a house's base is.
const BASES: [&str; 6] = ["GACNST", "NACNST", "YACNST", "AMCV", "SMCV", "PCV"];

/// The fight, interleaved so enemies stand next to each other; `true` for
/// the human player's units.
const ROSTER_LEN: usize = 10;
const ROSTER: [(&str, bool); ROSTER_LEN] = [
    ("MTNK", true),
    ("HTNK", false),
    ("MTNK", true),
    ("HTNK", false),
    ("MTNK", true),
    ("HTNK", false),
    ("E1", true),
    ("E2", false),
    ("E1", true),
    ("E2", false),
];

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

/// The first human player and the first computer house with a base.
fn sides() -> Option<(House, House)> {
    let human = Find::houses().where_prop(Prop::IsHuman, Op::Eq, 1).first();
    if human == NO_HANDLE {
        return None;
    }
    let mut houses = [0i64; 16];
    let (count, _) = Find::houses()
        .where_prop(Prop::IsHuman, Op::Eq, 0)
        .page(&mut houses, 0)
        .ok()?;
    houses[..count]
        .iter()
        .map(|&house| House(house))
        .find(|&house| base_of(house).is_some())
        .map(|enemy| (House(human), enemy))
}

/// Creates the roster a quarter of the way from the human player's base
/// towards the computer's, on passable cells in rings around that point,
/// and sets each unit on an enemy.
fn stage() {
    let tries = ld(&STAGE_TRIES) + 1;
    if tries > MAX_STAGE_TRIES {
        return;
    }
    st(&STAGE_TRIES, tries);
    let placed = place_roster();
    if placed == 0 && tries == MAX_STAGE_TRIES {
        log("ini-shield: found no place to stage the fight");
    }
}

/// Places the roster; how many units it made once both sides have some.
fn place_roster() -> usize {
    let Some((human, enemy)) = sides() else {
        return 0;
    };
    let Some(home) = base_of(human) else {
        return 0;
    };
    let away = base_of(enemy).unwrap_or(home);
    let (hx, hy) = (i32::from(home.x()), i32::from(home.y()));
    let ax = hx + (i32::from(away.x()) - hx) / 4;
    let ay = hy + (i32::from(away.y()) - hy) / 4;
    let mut ours = [Techno(NO_HANDLE); ROSTER_LEN];
    let mut theirs = [Techno(NO_HANDLE); ROSTER_LEN];
    let (mut n_ours, mut n_theirs) = (0, 0);
    let mut next = 0;
    'rings: for ring in 0..=7i32 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if next == ROSTER_LEN {
                    break 'rings;
                }
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let (x, y) = (ax + dx, ay + dy);
                if !(1..=500).contains(&x) || !(1..=500).contains(&y) {
                    continue;
                }
                let cell = Cell::at(x as u16, y as u16);
                if cell.passable(SpeedType::Track) != Ok(true) {
                    continue;
                }
                let (name, allied) = ROSTER[next];
                let Ok(kind) = TypeRef::techno_type(name) else {
                    next += 1;
                    continue;
                };
                let owner = if allied { human } else { enemy };
                // An occupied cell takes no unit; the next one is tried.
                if let Ok(unit) = Action::create_object(cell, kind, owner) {
                    if allied {
                        ours[n_ours] = unit;
                        n_ours += 1;
                    } else {
                        theirs[n_theirs] = unit;
                        n_theirs += 1;
                    }
                    next += 1;
                }
            }
        }
    }
    if n_ours == 0 || n_theirs == 0 {
        return 0;
    }
    st(&STAGED, 1);
    for (i, unit) in ours[..n_ours].iter().enumerate() {
        let _ = Action::set_mission_on(*unit, Mission::Attack, theirs[i % n_theirs]);
    }
    for (i, unit) in theirs[..n_theirs].iter().enumerate() {
        let _ = Action::set_mission_on(*unit, Mission::Attack, ours[i % n_ours]);
    }
    Line::new()
        .text("ini-shield: staged a fight of ")
        .num((n_ours + n_theirs) as i64)
        .text(" shielded units near (")
        .num(i64::from(ax))
        .text(",")
        .num(i64::from(ay))
        .text(")")
        .log();
    n_ours + n_theirs
}

// ------------------------------------------------------------------ Frame

fn tick() {
    if ld(&ABSORBED) != 0 {
        mark(MARK_ABSORB, "DEMO-OK absorb");
    }
    raise_broken();
    let now = frame();
    if now % 15 == 0 {
        regenerate(now);
        if ld(&STAGED) == 0 && now >= 30 {
            stage();
        }
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

    fn log(&self) {
        if let Ok(text) = core::str::from_utf8(&self.buf[..self.len]) {
            log(text);
        }
    }
}

vera20k_sdk::init!(init);
vera20k_sdk::on_scenario_start!(start);
vera20k_sdk::on_frame!(tick);
vera20k_sdk::on_events!(
    SHIELD_TYPES => load_shield_type,
    SHIELD_KEY => attach_shield,
    RESTORE_SHIELD => restore_shield,
);
vera20k_sdk::on_decide!(shared_delivery: ABSORB => absorb);
